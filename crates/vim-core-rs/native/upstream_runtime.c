#include "upstream_runtime.h"
#include "vim_bridge.h"
#include <stdarg.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <time.h>

#define NO_VIM_MAIN
#include "vim.h"
#include "proto/ex_docmd.pro"
#include "proto/getchar.pro"
#include "proto/edit.pro"
#include "proto/normal.pro"
#include "proto/main.pro"
#include "proto/mark.pro"
#include "proto/move.pro"
#include "proto/ui.pro"
#include "proto/term.pro"
#include "proto/usercmd.pro"
#include "proto/memline.pro"
#include "proto/option.pro"
#include "proto/ex_getln.pro"
#include "proto/buffer.pro"
#include "proto/window.pro"
#include "proto/screen.pro"
#include "proto/channel.pro"
#include "proto/job.pro"
#include "proto/eval.pro"
#include "proto/evalvars.pro"
#include "proto/list.pro"
#include "proto/typval.pro"
#include "proto/search.pro"
#include "proto/undo.pro"
#include "proto/change.pro"
/* NOTE: dict.pro and popupmenu.pro are included via vim.h auto-generated protos */
#include "proto/insexpand.pro"

#ifndef UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS
#define UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS 1024
#endif

typedef struct upstream_runtime_pending_action {
    vim_host_action_t action;
} upstream_runtime_pending_action_t;

typedef struct upstream_runtime_pending_event {
    vim_core_event_t event;
} upstream_runtime_pending_event_t;

#ifndef UPSTREAM_RUNTIME_MAX_TRACKED_WINDOWS
#define UPSTREAM_RUNTIME_MAX_TRACKED_WINDOWS 64
#endif

typedef struct {
    int id;
    int row;
    int col;
    int width;
    int height;
} upstream_runtime_window_geometry_t;

struct upstream_runtime_session {
    uint64_t revision;
    varnumber_T last_changedtick;
    uintptr_t last_cursor_row;
    uintptr_t last_cursor_col;
    vim_core_mode_t last_mode;
    vim_core_pending_input_t pending_input;
    uint64_t next_correlation_id;
    char* leased_snapshot_text;
    char* pending_input_response_value;
    uintptr_t pending_input_response_len;
    int pending_input_response_available;
    int pending_input_response_cancelled;

    upstream_runtime_pending_action_t queue[UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS];
    size_t queue_head;
    size_t queue_len;
    upstream_runtime_pending_event_t event_queue[UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS];
    size_t event_queue_head;
    size_t event_queue_len;

    /* Task 3.2: Track window geometry for layout change detection */
    upstream_runtime_window_geometry_t tracked_windows[UPSTREAM_RUNTIME_MAX_TRACKED_WINDOWS];
    size_t tracked_window_count;

    jmp_buf quit_env;
    oparg_T oa;
    int is_executing;
};

static int upstream_runtime_bootstrapped = FALSE;
static upstream_runtime_session_t* upstream_runtime_active_session = NULL;
int upstream_runtime_is_injecting = FALSE;
jmp_buf* upstream_runtime_active_quit_env = NULL;
static FILE* upstream_runtime_debug_log_file = NULL;
static int upstream_runtime_debug_log_to_stderr = FALSE;

static void upstream_runtime_debug_vprintf(const char* format, va_list args) {
    FILE* stream = upstream_runtime_debug_log_file != NULL
        ? upstream_runtime_debug_log_file
        : (upstream_runtime_debug_log_to_stderr ? stderr : NULL);
    if (stream == NULL) {
        return;
    }
    vfprintf(stream, format, args);
    fflush(stream);
}

static void upstream_runtime_debug_printf(const char* format, ...) {
    va_list args;
    va_start(args, format);
    upstream_runtime_debug_vprintf(format, args);
    va_end(args);
}

static uint64_t upstream_runtime_now_ns(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
        return 0;
    }
    return ((uint64_t)ts.tv_sec * 1000000000ULL) + (uint64_t)ts.tv_nsec;
}

static uint64_t upstream_runtime_elapsed_ms(uint64_t started_ns) {
    uint64_t finished_ns = upstream_runtime_now_ns();
    if (started_ns == 0 || finished_ns < started_ns) {
        return 0;
    }
    return (finished_ns - started_ns) / 1000000ULL;
}

static void upstream_runtime_sync_env_var(const char* name) {
    const char* value = getenv(name);
    if (value == NULL) {
        vim_unsetenv_ext((char_u *)name);
        return;
    }

    vim_setenv_ext((char_u *)name, (char_u *)value);
}

static void upstream_runtime_apply_xdg_runtimepath(void) {
    const char* xdg_config_home = getenv("XDG_CONFIG_HOME");
    if (xdg_config_home == NULL || *xdg_config_home == '\0') {
        return;
    }

    const char* runtimepath = (const char*)p_rtp;
    const char* packpath = (const char*)p_pp;
    if (runtimepath == NULL || *runtimepath == '\0' || packpath == NULL || *packpath == '\0') {
        return;
    }

    size_t vimrc_len = strlen(xdg_config_home) + strlen("/vim/vimrc") + 1;
    char* vimrc_path = (char*)malloc(vimrc_len);
    if (vimrc_path == NULL) {
        return;
    }
    snprintf(vimrc_path, vimrc_len, "%s/vim/vimrc", xdg_config_home);

    FILE* vimrc = fopen(vimrc_path, "r");
    if (vimrc == NULL) {
        free(vimrc_path);
        return;
    }
    fclose(vimrc);

    size_t xdg_dir_len = strlen(xdg_config_home) + strlen("/vim") + 1;
    char* xdg_dir = (char*)malloc(xdg_dir_len);
    if (xdg_dir == NULL) {
        free(vimrc_path);
        return;
    }
    snprintf(xdg_dir, xdg_dir_len, "%s/vim", xdg_config_home);

    if (strstr(runtimepath, xdg_dir) == NULL) {
        size_t merged_len = strlen(xdg_dir) + 1 + strlen(runtimepath) + 1;
        char* merged = (char*)malloc(merged_len);
        if (merged != NULL) {
            snprintf(merged, merged_len, "%s,%s", xdg_dir, runtimepath);
            set_option_value_give_err((char_u *)"rtp", 0L, (char_u *)merged, 0);
            free(merged);
        }
    }

    if (strstr(packpath, xdg_dir) == NULL) {
        size_t merged_len = strlen(xdg_dir) + 1 + strlen(packpath) + 1;
        char* merged = (char*)malloc(merged_len);
        if (merged != NULL) {
            snprintf(merged, merged_len, "%s,%s", xdg_dir, packpath);
            set_option_value_give_err((char_u *)"pp", 0L, (char_u *)merged, 0);
            free(merged);
        }
    }

    free(xdg_dir);
    free(vimrc_path);
}

void upstream_runtime_set_debug_log_path(const char* path, uintptr_t path_len) {
    if (upstream_runtime_debug_log_file != NULL) {
        fclose(upstream_runtime_debug_log_file);
        upstream_runtime_debug_log_file = NULL;
    }

    if (path == NULL || path_len == 0) {
        upstream_runtime_debug_log_to_stderr = FALSE;
        return;
    }

    char* owned_path = (char*)malloc((size_t)path_len + 1U);
    if (owned_path == NULL) {
        upstream_runtime_debug_log_to_stderr = FALSE;
        return;
    }

    memcpy(owned_path, path, (size_t)path_len);
    owned_path[path_len] = '\0';

    upstream_runtime_debug_log_file = fopen(owned_path, "a");
    upstream_runtime_debug_log_to_stderr = FALSE;
    free(owned_path);
}

/* Stubs for Input Method functions to fix link errors on some platforms */
char *did_set_imactivatefunc(optset_T *args) { (void)args; return NULL; }
char *did_set_imstatusfunc(optset_T *args) { (void)args; return NULL; }
int im_get_status(void) { return 0; }
void im_set_active(int active) { (void)active; }
int set_ref_in_im_funcs(int copyID) { (void)copyID; return 0; }

static void upstream_runtime_capture_window_geometry(upstream_runtime_session_t* session);
static int upstream_runtime_detect_layout_change(upstream_runtime_session_t* session);
static void upstream_runtime_drain_vcr_events(upstream_runtime_session_t* session);
static int upstream_runtime_enqueue_event(
    upstream_runtime_session_t* session,
    const vim_core_event_t* event
);
static int upstream_runtime_enqueue_message_event_for_session(
    upstream_runtime_session_t* session,
    const char* text,
    uintptr_t text_len,
    vim_core_message_severity_t severity,
    vim_core_message_category_t category
);
static void upstream_runtime_queue_bell_event(upstream_runtime_session_t* session);
static void upstream_runtime_queue_redraw_event(
    upstream_runtime_session_t* session,
    int full,
    int clear_before_draw
);
static void upstream_runtime_queue_buf_add_event(
    upstream_runtime_session_t* session,
    int buf_id
);
static void upstream_runtime_queue_win_new_event(
    upstream_runtime_session_t* session,
    int win_id
);
static void upstream_runtime_queue_layout_changed_event(
    upstream_runtime_session_t* session
);
static void upstream_runtime_queue_pager_prompt_event(
    upstream_runtime_session_t* session,
    vim_core_pager_prompt_kind_t kind
);
static int upstream_runtime_dispatch_core_quit(upstream_runtime_session_t* session, int force);
static int upstream_runtime_cb_redraw(exarg_T *eap_ptr);
static vim_core_mode_t upstream_runtime_get_mode(const upstream_runtime_session_t* session);
static char* upstream_runtime_copy_text(const char* text, uintptr_t text_len);
static char* upstream_runtime_copy_normal_input(const char* text, uintptr_t text_len);
static vim_core_command_result_t upstream_runtime_result(upstream_runtime_session_t* session, vim_core_status_t status, uint32_t reason_code, vim_core_command_outcome_kind_t outcome);
static vim_core_command_result_t upstream_runtime_ok_result(upstream_runtime_session_t* session);
static vim_core_command_result_t upstream_runtime_host_action_result(upstream_runtime_session_t* session);
static vim_core_command_result_t upstream_runtime_command_error_result(upstream_runtime_session_t* session, uint32_t reason_code);
static vim_core_command_result_t upstream_runtime_detect_outcome(upstream_runtime_session_t* session);
static void upstream_runtime_refresh_pending_input(
    upstream_runtime_session_t* session,
    const char* command,
    uintptr_t command_len
);
static int upstream_runtime_can_set_mark(char mark_name);
static int upstream_runtime_mark_position_is_valid(buf_T* buf, pos_T pos);
void upstream_runtime_queue_bell_action(upstream_runtime_session_t* session);
static int upstream_runtime_option_scope_to_vim_flags(vim_core_option_scope_t scope);
static vim_core_option_type_t upstream_runtime_option_type_from_flags(int option_flags);
static int upstream_runtime_option_rejects_local_scope(const char* name, int* option_flags);
static vim_core_option_get_result_t upstream_runtime_option_get_result_with_status(
    vim_core_status_t status
);
static vim_core_option_set_result_t upstream_runtime_option_set_result_with_error(
    vim_core_status_t status,
    const char* error_message
);

typedef struct {
    char name[32];
    int (*callback)(void* eap);
} upstream_runtime_ex_command_t;

#define UPSTREAM_RUNTIME_MAX_CUSTOM_COMMANDS 32
static upstream_runtime_ex_command_t upstream_runtime_custom_commands[UPSTREAM_RUNTIME_MAX_CUSTOM_COMMANDS];
static int upstream_runtime_custom_commands_count = 0;

static int upstream_runtime_dispatch_core_write(upstream_runtime_session_t* session, const char* path, int force) {
    size_t tail = (session->queue_head + session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
    if (session->queue_len >= UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) return FALSE;
    
    session->queue[tail].action.kind = VIM_HOST_ACTION_WRITE;
    session->queue[tail].action.force = (force != 0);
    session->queue[tail].action.primary_text_ptr = (char*)malloc(strlen(path) + 1);
    strcpy((char*)session->queue[tail].action.primary_text_ptr, path);
    session->queue[tail].action.primary_text_len = strlen(path);
    session->queue_len++;
    
    if (session->is_executing) longjmp(session->quit_env, 1);
    return TRUE;
}

static int upstream_runtime_cb_internal_write(exarg_T *eap) {
    if (upstream_runtime_active_session == NULL) return FALSE;
    char* path = (char*)(eap->arg ? (char*)eap->arg : "");
    upstream_runtime_dispatch_core_write(upstream_runtime_active_session, path, eap->forceit);
    return TRUE;
}

static void upstream_runtime_bootstrap(void) {
    if (upstream_runtime_bootstrapped) return;
    
    mch_early_init();
    
    mparm_T params;
    memset(&params, 0, sizeof(params));
    
    static char *argv[] = {"vim-core-rs", NULL};
    params.argc = 1;
    params.argv = argv;
    
    char_u *empty = (char_u *)"";
    params.tagname = empty;
    
    common_init_1();
    common_init_2(&params);
    
    upstream_runtime_bootstrapped = TRUE;
}

static int upstream_runtime_dispatch_core_quit(upstream_runtime_session_t* session, int force) {
    size_t tail = (session->queue_head + session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
    if (session->queue_len >= UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
        return FALSE; 
    }
    
    session->queue[tail].action.kind = VIM_HOST_ACTION_QUIT;
    session->queue[tail].action.quit_requested = TRUE;
    session->queue[tail].action.quit_force = (force != 0);
    session->queue[tail].action.force = (force != 0);
    session->queue_len++;
    
    if (session->is_executing) {
        longjmp(session->quit_env, 1);
    }
    
    return TRUE;
}

static vim_core_command_result_t upstream_runtime_queue_quit_action(upstream_runtime_session_t* session, const char* arg) {
    int force = (arg && strchr(arg, '!'));
    upstream_runtime_dispatch_core_quit(session, force);
    return upstream_runtime_host_action_result(session);
}

void getout(int exitval) {
    if (upstream_runtime_active_session != NULL && upstream_runtime_active_session->is_executing) {
        // Queue quit action if it came from a direct Vim exit call (not our callback)
        // exitval 0 is normal exit, non-zero is error exit.
        // We'll treat all as quit for now.
        size_t tail = (upstream_runtime_active_session->queue_head + upstream_runtime_active_session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
        if (upstream_runtime_active_session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
            upstream_runtime_active_session->queue[tail].action.kind = VIM_HOST_ACTION_QUIT;
            upstream_runtime_active_session->queue[tail].action.quit_requested = TRUE;
            /* getout() が呼ばれた時点で Vim は既に exit を決定済み。
             * :q! / ZQ の forceit 情報は getout の引数に含まれないため、
             * Vim 内部から呼ばれた quit は常に force=true として扱う。
             * ホスト側での未保存チェックが必要な :q は
             * upstream_runtime_try_intercept_ex で先にインターセプトされ、
             * getout() まで到達しない。 */
            upstream_runtime_active_session->queue[tail].action.quit_force = TRUE;
            upstream_runtime_active_session->queue[tail].action.force = TRUE;
            upstream_runtime_active_session->queue_len++;
        }
        longjmp(upstream_runtime_active_session->quit_env, 1);
    }
    exit(exitval);
}

/*
 * mch_inchar オーバーライド: コマンド注入中（upstream_runtime_is_injecting）に
 * Vim が入力待ちでブロックするのを防ぐ。
 * 入力待ち（wtime != 0）かつ typeahead が空の場合、longjmp で脱出する。
 * os_unix.c の mch_inchar をリンカレベルで上書きする。
 */
int mch_inchar(char_u *buf, int maxlen, long wtime, int tb_change_cnt) {
    (void)buf; (void)maxlen; (void)tb_change_cnt;
    /* printf debug removed */

    if (upstream_runtime_is_injecting) {
        if (wtime != 0 && stuff_empty()) {
            /* printf debug removed */
            if (upstream_runtime_active_quit_env != NULL) {
                longjmp(*upstream_runtime_active_quit_env, 1);
            }
        }
        return 0;
    }

    /* 非 injecting 時: 通常のヘッドレス動作ではここに到達しないはずだが、
       安全のため即座に 0 を返す（入力なし） */
    if (wtime == 0) return 0;
    if (wtime > 0) return 0;
    /* wtime == -1 (無期限待ち): ヘッドレスモードではブロックできないので脱出 */
    if (upstream_runtime_active_quit_env != NULL) {
        longjmp(*upstream_runtime_active_quit_env, 1);
    }
    return 0;
}

void upstream_runtime_mch_exit(int r) {
    (void)r;
    if (upstream_runtime_active_session != NULL) {
        if (upstream_runtime_active_session->queue_len > 0) {
            size_t last_idx = (upstream_runtime_active_session->queue_head + upstream_runtime_active_session->queue_len - 1) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
            if (upstream_runtime_active_session->queue[last_idx].action.kind == VIM_HOST_ACTION_QUIT) {
                if (upstream_runtime_active_session->is_executing) {
                    longjmp(upstream_runtime_active_session->quit_env, 1);
                }
                return;
            }
        }
        upstream_runtime_queue_quit_action(upstream_runtime_active_session, "!");
        if (upstream_runtime_active_session->is_executing) {
            longjmp(upstream_runtime_active_session->quit_env, 1);
        }
    }
}

static vim_core_command_result_t upstream_runtime_queue_input_action(upstream_runtime_session_t* session, const char* prompt, vim_core_input_request_kind_t kind);

static int upstream_runtime_cb_q(exarg_T *eap) {
    if (upstream_runtime_active_session == NULL) return FALSE;
    upstream_runtime_dispatch_core_quit(upstream_runtime_active_session, eap->forceit);
    return TRUE;
}

static int upstream_runtime_cb_qa(exarg_T *eap) {
    if (upstream_runtime_active_session == NULL) return FALSE;
    upstream_runtime_dispatch_core_quit(upstream_runtime_active_session, eap->forceit);
    return TRUE;
}

static int upstream_runtime_cb_input(exarg_T *eap) {
    if (upstream_runtime_active_session == NULL) return FALSE;
    upstream_runtime_queue_input_action(
        upstream_runtime_active_session, (const char*)(eap->arg ? (char*)eap->arg : ""), VIM_CORE_INPUT_REQUEST_COMMAND_LINE);
    return TRUE;
}

static int upstream_runtime_cb_bell(exarg_T *eap) {
    (void)eap;
    if (upstream_runtime_active_session == NULL) return FALSE;
    upstream_runtime_queue_bell_action(upstream_runtime_active_session);
    return TRUE;
}

static int upstream_runtime_cb_buf_add(exarg_T *eap) {
    if (upstream_runtime_active_session == NULL) return FALSE;

    /* Get the buffer number from Vim's autocmd context or from the current buffer.
     * In BufAdd context, curbuf is the newly added buffer. */
    int buf_id = 0;
    if (eap->arg != NULL && eap->arg[0] != '\0') {
        buf_id = atoi((const char*)eap->arg);
    }
    /* Fallback: if <abuf> was not expanded (literal '<abuf>'), use curbuf */
    if (buf_id <= 0) {
        buf_id = curbuf->b_fnum;
    }
    

    size_t tail = (upstream_runtime_active_session->queue_head + upstream_runtime_active_session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
    if (upstream_runtime_active_session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
        upstream_runtime_active_session->queue[tail].action.kind = VIM_HOST_ACTION_BUF_ADD;
        upstream_runtime_active_session->queue[tail].action.event_buf_id = buf_id;
        upstream_runtime_active_session->queue_len++;
        upstream_runtime_queue_buf_add_event(upstream_runtime_active_session, buf_id);
    }

    return TRUE;
}

static int upstream_runtime_cb_win_new(exarg_T *eap) {
    (void)eap;
    if (upstream_runtime_active_session == NULL) return FALSE;

    /* WinNew fires in the context of the new window, so curwin->w_id is the new window */
    int win_id = curwin->w_id;
    

    size_t tail = (upstream_runtime_active_session->queue_head + upstream_runtime_active_session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
    if (upstream_runtime_active_session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
        upstream_runtime_active_session->queue[tail].action.kind = VIM_HOST_ACTION_WIN_NEW;
        upstream_runtime_active_session->queue[tail].action.event_win_id = win_id;
        upstream_runtime_active_session->queue_len++;
        upstream_runtime_queue_win_new_event(upstream_runtime_active_session, win_id);
    }

    return TRUE;
}

static int upstream_runtime_cb_layout_changed(exarg_T *eap) {
    (void)eap;
    if (upstream_runtime_active_session == NULL) return FALSE;

    

    size_t tail = (upstream_runtime_active_session->queue_head + upstream_runtime_active_session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
    if (upstream_runtime_active_session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
        upstream_runtime_active_session->queue[tail].action.kind = VIM_HOST_ACTION_LAYOUT_CHANGED;
        upstream_runtime_active_session->queue_len++;
        upstream_runtime_queue_layout_changed_event(upstream_runtime_active_session);
    }

    return TRUE;
}

static int upstream_runtime_cb_redraw(exarg_T *eap_ptr) {
    if (upstream_runtime_active_session == NULL || eap_ptr == NULL) return FALSE;
    exarg_T eap = *eap_ptr;
    
    size_t tail = (upstream_runtime_active_session->queue_head + upstream_runtime_active_session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
    if (upstream_runtime_active_session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
        upstream_runtime_active_session->queue[tail].action.kind = VIM_HOST_ACTION_REDRAW;
        upstream_runtime_active_session->queue[tail].action.redraw_force = (eap.forceit != 0);
        upstream_runtime_active_session->queue_len++;
        upstream_runtime_queue_redraw_event(
            upstream_runtime_active_session,
            TRUE,
            eap.forceit != 0
        );
    }
    
    return TRUE;
}

void vim_bridge_register_ex_callback(const char* name, int (*callback)(void* eap)) {
    if (upstream_runtime_custom_commands_count >= UPSTREAM_RUNTIME_MAX_CUSTOM_COMMANDS) return;
    
    upstream_runtime_ex_command_t* entry = &upstream_runtime_custom_commands[upstream_runtime_custom_commands_count++];
    strncpy(entry->name, name, sizeof(entry->name) - 1);
    entry->name[sizeof(entry->name) - 1] = '\0';
    entry->callback = callback;
}

static void upstream_runtime_register_callbacks(void) {
    static int registered = FALSE;
    if (registered) return;

    /* Canonical command names are registered so user-facing Ex contracts stay stable. */
    vim_bridge_register_ex_callback("input", (int (*)(void*))upstream_runtime_cb_input);
    vim_bridge_register_ex_callback("bell", (int (*)(void*))upstream_runtime_cb_bell);
    /* Internal aliases remain available for debugging and future scripts. */
    vim_bridge_register_ex_callback("CoreQuit", (int (*)(void*))upstream_runtime_cb_q);
    vim_bridge_register_ex_callback("CoreQuitAll", (int (*)(void*))upstream_runtime_cb_qa);
    vim_bridge_register_ex_callback("CoreInput", (int (*)(void*))upstream_runtime_cb_input);
    vim_bridge_register_ex_callback("CoreBell", (int (*)(void*))upstream_runtime_cb_bell);
    vim_bridge_register_ex_callback("CoreRedraw", (int (*)(void*))upstream_runtime_cb_redraw);
    vim_bridge_register_ex_callback("HostQuit", (int (*)(void*))upstream_runtime_cb_q);
    vim_bridge_register_ex_callback("HostQuitAll", (int (*)(void*))upstream_runtime_cb_qa);
    vim_bridge_register_ex_callback("HostInput", (int (*)(void*))upstream_runtime_cb_input);
    vim_bridge_register_ex_callback("HostBell", (int (*)(void*))upstream_runtime_cb_bell);
    vim_bridge_register_ex_callback("HostRedraw", (int (*)(void*))upstream_runtime_cb_redraw);
    vim_bridge_register_ex_callback("CoreInternalWrite", (int (*)(void*))upstream_runtime_cb_internal_write);
    vim_bridge_register_ex_callback("HostBufAdd", (int (*)(void*))upstream_runtime_cb_buf_add);
    vim_bridge_register_ex_callback("HostWinNew", (int (*)(void*))upstream_runtime_cb_win_new);
    vim_bridge_register_ex_callback("HostLayoutChanged", (int (*)(void*))upstream_runtime_cb_layout_changed);

    registered = TRUE;
    }
/* Requirements: 1.1, 1.2, 5.4, 6.1, 6.2 */
void mch_job_start(char **argv, job_T *job, jobopt_T *options, int is_terminal) {
    (void)is_terminal;
    
    if (upstream_runtime_active_session == NULL) {
        job->jv_status = JOB_FAILED;
        return;
    }

    size_t total_len = 0;
    for (int i = 0; argv && argv[i] != NULL; ++i) {
        total_len += strlen(argv[i]) + 1;
    }

    char* argv_buf = malloc(total_len > 0 ? total_len : 1);
    if (!argv_buf) {
        job->jv_status = JOB_FAILED;
        return;
    }

    char* p = argv_buf;
    for (int i = 0; argv && argv[i] != NULL; ++i) {
        size_t len = strlen(argv[i]);
        memcpy(p, argv[i], len);
        p += len;
        *p++ = '\0';
    }
    if (total_len == 0) {
        argv_buf[0] = '\0';
    }

    static int next_vfd = 512;
    static int next_job_id = 1;
    int job_id = next_job_id++;
    
    int use_null_for_in = options->jo_io[PART_IN] == JIO_NULL;
    int use_null_for_out = options->jo_io[PART_OUT] == JIO_NULL;
    int use_null_for_err = options->jo_io[PART_ERR] == JIO_NULL;

    int vfd_in = use_null_for_in ? -1 : next_vfd++;
    int vfd_out = use_null_for_out ? -1 : next_vfd++;
    int vfd_err = use_null_for_err ? -1 : next_vfd++;

    channel_T *channel = add_channel();
    if (channel == NULL) {
        free(argv_buf);
        job->jv_status = JOB_FAILED;
        return;
    }

    channel_set_pipes(channel, vfd_in, vfd_out, vfd_err);
    channel_set_job(channel, job, options);

    job->jv_channel = channel;
    ++channel->ch_refcount;

    job->jv_pid = job_id;
    job->jv_status = JOB_STARTED;

    size_t tail = (upstream_runtime_active_session->queue_head + upstream_runtime_active_session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
    if (upstream_runtime_active_session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
        upstream_runtime_active_session->queue[tail].action.kind = VIM_HOST_ACTION_JOB_START;
        upstream_runtime_active_session->queue[tail].action.job_start_request.job_id = job_id;
        upstream_runtime_active_session->queue[tail].action.job_start_request.argv_buf = argv_buf;
        upstream_runtime_active_session->queue[tail].action.job_start_request.argv_len = total_len;
        upstream_runtime_active_session->queue[tail].action.job_start_request.cwd = NULL; // We don't implement CWD in the stub for now, or we can check options->jo_cwd
        if (options->jo_cwd != NULL) {
            upstream_runtime_active_session->queue[tail].action.job_start_request.cwd = strdup((char *)options->jo_cwd);
        } else {
            upstream_runtime_active_session->queue[tail].action.job_start_request.cwd = NULL;
        }
        upstream_runtime_active_session->queue[tail].action.job_start_request.vfd_in = vfd_in;
        upstream_runtime_active_session->queue[tail].action.job_start_request.vfd_out = vfd_out;
        upstream_runtime_active_session->queue[tail].action.job_start_request.vfd_err = vfd_err;
        upstream_runtime_active_session->queue_len++;
    } else {
        free(argv_buf);
        job->jv_status = JOB_FAILED;
    }
}

extern int vim_core_job_get_status(int job_id, int *exit_code_out);
extern void vim_core_job_clear(int job_id);

char *mch_job_status(job_T *job) {
    if (job == NULL) return "dead";
    int exit_code = 0;
    int status = vim_core_job_get_status(job->jv_pid, &exit_code);
    if (status == 1) { // Ended
        job->jv_exitval = exit_code;
        if (job->jv_status < JOB_ENDED)
            job->jv_status = JOB_ENDED;
        return "dead";
    } else if (status == 2) { // Dead already reaped
        return "dead";
    } else if (status == 0) { // Running
        return "run";
    }
    return "dead";
}

job_T *mch_detect_ended_job(job_T *job_list) {
    job_T *job;
    for (job = job_list; job != NULL; job = job->jv_next) {
        int exit_code = 0;
        int status = vim_core_job_get_status(job->jv_pid, &exit_code);
        if (status == 1) { // Ended and not reaped yet
            job->jv_exitval = exit_code;
            if (job->jv_status < JOB_ENDED)
                job->jv_status = JOB_ENDED;
            return job;
        }
    }
    return NULL;
}

int mch_signal_job(job_T *job, char_u *how) {
    (void)how; // ignoring signal type for now, we just stop it
    if (job == NULL) return FAIL;
    
    if (upstream_runtime_active_session != NULL) {
        size_t tail = (upstream_runtime_active_session->queue_head + upstream_runtime_active_session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
        if (upstream_runtime_active_session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
            upstream_runtime_active_session->queue[tail].action.kind = VIM_HOST_ACTION_JOB_STOP;
            upstream_runtime_active_session->queue[tail].action.job_start_request.job_id = job->jv_pid; 
            upstream_runtime_active_session->queue_len++;
        }
    }
    return OK;
}

void mch_clear_job(job_T *job) {
    if (job == NULL) return;
    vim_core_job_clear(job->jv_pid);
}

upstream_runtime_session_t* upstream_runtime_session_new(const char* initial_text, uintptr_t text_len) {
    upstream_runtime_bootstrap();
    upstream_runtime_register_callbacks();
    /* Embedded runtime still needs full-screen layout behavior for viewport sync. */
    full_screen = TRUE;

    /* embedded mode ではページャプロンプトを CoreEvent 経由で通知する。
     * vi 互換モードでは 'more' がデフォルト OFF のため、明示的に有効化する。 */
    p_more = TRUE;

    /* Embedded editing should use Vim undo semantics: repeated "u" keeps
     * moving backward and does not toggle into redo at the oldest change. */
    do_cmdline_cmd((char_u*)"set cpo-=u");

    /* Embedded hosts dispatch typed Insert-mode keys one at a time. Use Vim's
     * standard non-compatible Backspace behavior so a standalone Ctrl-H can
     * delete text inserted earlier in the same Insert-mode session. */
    do_cmdline_cmd((char_u*)"set backspace=indent,eol,start");

    /* Session-local tests may override 'helpfile', which changes how Vim
     * re-derives $VIM and $VIMRUNTIME. Reset those globals before each
     * session so runtime discovery falls back to the generated pathdef
     * values instead of a previous session's helpfile path. */
    set_string_option_direct((char_u *)"hf", -1, (char_u *)DFLT_HELPFILE, 0, SID_NONE);
    vim_unsetenv_ext((char_u *)"VIM");
    vim_unsetenv_ext((char_u *)"VIMRUNTIME");
    upstream_runtime_sync_env_var("HOME");
    upstream_runtime_sync_env_var("XDG_CONFIG_HOME");
    upstream_runtime_apply_xdg_runtimepath();

    /* Clear any leftover typeahead from previous sessions/tests */
    if (typebuf.tb_len > 0) {
        del_typebuf(typebuf.tb_len, 0);
    }

    /* Requirement 6.3: Reset Vim's global state for isolation */
    /* Collapse leftover tabpages first so firstwin/curwin and tabpage lists
     * are rebuilt from a single active tab before per-window/per-buffer reset. */
    while (first_tabpage != NULL && first_tabpage->tp_next != NULL) {
        tabpage_close(TRUE);
    }

    /* Close all windows except the current one */

    while (firstwin != lastwin) {
        win_close(lastwin == curwin ? firstwin : lastwin, TRUE);
    }

    /* Close all buffers except the current one */

    while (firstbuf != lastbuf) {
        buf_T* target = (lastbuf == curbuf) ? firstbuf : lastbuf;
        close_buffer(NULL, target, DOBUF_WIPE, FALSE, FALSE, FALSE);
    }

    /* Re-allocate screen after closing windows to keep Vim's internal state consistent */
    if (Rows > 0 && Columns > 0) {
        screenalloc(TRUE);
        shell_new_rows();
        shell_new_columns();
    }

    /* Clear current buffer content (keep at least one line) */
    while (curbuf->b_ml.ml_line_count > 1) {
        ml_delete(1);
    }
    ml_replace(1, (char_u*)"", TRUE);
    curbuf->b_changed = 0;
    
    /* Reset cursor to top-left */
    curwin->w_cursor.lnum = 1;
    curwin->w_cursor.col = 0;
    curwin->w_cursor.coladd = 0;
    curwin->w_curswant = 0;
    curwin->w_set_curswant = TRUE;
    
    /* Ensure we are in Normal mode */
    State = MODE_NORMAL;
    finish_op = FALSE;
    VIsual_active = FALSE;
    VIsual_select = FALSE;
    VIsual_mode = 'v';
    restart_edit = 0;
    vgetc_busy = 0;
    ex_normal_busy = 0;
    got_int = FALSE;

    upstream_runtime_session_t* session = malloc(sizeof(upstream_runtime_session_t));
    memset(session, 0, sizeof(upstream_runtime_session_t));
    

    if (initial_text && text_len > 0) {
        char_u* copy = (char_u*)malloc(text_len + 1);
        memcpy(copy, initial_text, text_len);
        copy[text_len] = '\0';

        /* Requirement 6.3: Split by newline and append as lines */
        char_u* line_start = copy;
        int line_count = 0;
        for (uintptr_t i = 0; i < text_len; ++i) {
            if (copy[i] == '\n') {
                copy[i] = '\0';
                if (line_count == 0) {
                    ml_replace(1, line_start, TRUE);
                } else {
                    ml_append(line_count, line_start, 0, FALSE);
                }
                line_count++;
                line_start = &copy[i + 1];
            }
        }
        /* Handle remaining text after last newline (if any) */
        if (*line_start != '\0') {
            if (line_count == 0) {
                ml_replace(1, line_start, TRUE);
            } else {
                ml_append(line_count, line_start, 0, FALSE);
            }
        }

        curbuf->b_changed = 0;
        free(copy);
    }

    /* The embedded runtime reuses one Vim instance across sessions. After
     * installing the initial text, discard the previous buffer's undo tree so
     * undo starts from this session's initial state. */
    u_clearallandblockfree(curbuf);

    /* Reset revision tracking AFTER initialization */
    session->revision = 0;
    session->last_changedtick = CHANGEDTICK(curbuf);
    session->last_cursor_row = 0;
    session->last_cursor_col = 0;
    session->last_mode = upstream_runtime_get_mode(session);
    session->pending_input = VIM_CORE_PENDING_INPUT_NONE;

    /* Task 3.1/3.2: Set up autocommands for buffer/window event notification.
     * zero-patch 設計: Vim ソースを編集せず、Vim のユーザーコマンド機構
     * (:command!) でホストイベントコマンドを登録する。
     * autocommand が発火するとグローバルリスト g:_vcr_events に
     * イベント情報が蓄積され、コマンド実行後にポーリングで取得する。 */
    
    do_cmdline_cmd((char_u*)"let g:_vcr_events = []");
    do_cmdline_cmd((char_u*)"command! -nargs=? HostBufAdd call add(g:_vcr_events, 'BufAdd:' . <q-args>)");
    do_cmdline_cmd((char_u*)"command! -nargs=0 HostWinNew call add(g:_vcr_events, 'WinNew:' . win_getid())");
    do_cmdline_cmd((char_u*)"command! -nargs=0 HostLayoutChanged call add(g:_vcr_events, 'LayoutChanged')");
    do_cmdline_cmd((char_u*)"silent! autocmd! HostEvents");
    do_cmdline_cmd((char_u*)"augroup HostEvents");
    do_cmdline_cmd((char_u*)"autocmd!");
    do_cmdline_cmd((char_u*)"autocmd BufAdd * HostBufAdd <abuf>");
    do_cmdline_cmd((char_u*)"autocmd WinNew * HostWinNew");
    do_cmdline_cmd((char_u*)"autocmd WinScrolled * HostLayoutChanged");
    do_cmdline_cmd((char_u*)"augroup END");
    

    /* Drain any actions queued during autocommand setup */
    session->queue_head = 0;
    session->queue_len = 0;
    session->event_queue_head = 0;
    session->event_queue_len = 0;
    /* autocommand 登録中に蓄積されたイベントもクリア */
    do_cmdline_cmd((char_u*)"let g:_vcr_events = []");

    /* Task 3.2: Capture initial window geometry */
    upstream_runtime_capture_window_geometry(session);

    return session;
    }
void upstream_runtime_session_free(upstream_runtime_session_t* session) {
    size_t idx;
    if (upstream_runtime_active_session == session) {
        upstream_runtime_active_session = NULL;
    }
    for (idx = 0; idx < session->event_queue_len; ++idx) {
        size_t pos = (session->event_queue_head + idx) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
        if (session->event_queue[pos].event.text_ptr != NULL) {
            free((char*)session->event_queue[pos].event.text_ptr);
        }
    }
    if (session->pending_input_response_value) free(session->pending_input_response_value);
    if (session->leased_snapshot_text) free(session->leased_snapshot_text);
    free(session);
}

static char* upstream_runtime_copy_text(const char* text, uintptr_t text_len) {
    char* copy = malloc(text_len + 1);
    if (copy == NULL) return NULL;
    memcpy(copy, text, text_len);
    copy[text_len] = '\0';
    return copy;
}

static char* upstream_runtime_copy_normal_input(const char* text, uintptr_t text_len) {
    char* copy = malloc((text_len * 3) + 1);
    uintptr_t read_idx;
    uintptr_t write_idx = 0;
    if (copy == NULL) return NULL;

    for (read_idx = 0; read_idx < text_len; ++read_idx) {
        unsigned char ch = (unsigned char)text[read_idx];
        if (ch == BS) {
            copy[write_idx++] = (char)K_SPECIAL;
            copy[write_idx++] = 'k';
            copy[write_idx++] = 'b';
        } else {
            copy[write_idx++] = (char)ch;
        }
    }
    copy[write_idx] = '\0';
    return copy;
}

/*
 * 事前パース用ヘルパー: コマンド文字列の先頭から空白と ':' を飛ばし、
 * コマンド名部分の長さを返す。force ('!') の有無も検出する。
 */
static const char* upstream_runtime_parse_ex_cmd(
    const char* input,
    size_t* cmd_len_out,
    int* force_out,
    const char** arg_out
) {
    const char* p = input;

    /* 先頭の空白と ':' をスキップ */
    while (*p == ' ' || *p == '\t' || *p == ':') p++;

    /* コマンド名の開始位置 */
    const char* cmd_start = p;
    while (*p != NUL && *p != '!' && *p != ' ' && *p != '\t') p++;

    *cmd_len_out = (size_t)(p - cmd_start);

    /* force ('!') の検出 */
    *force_out = (*p == '!');
    if (*p == '!') p++;

    /* 引数部分（空白スキップ後） */
    while (*p == ' ' || *p == '\t') p++;
    *arg_out = p;

    return cmd_start;
}

/*
 * コマンド名が指定キーワードに前方一致するか判定する。
 * Vim の省略形ルール（例: "redr" → "redraw"）に対応。
 * min_len は最低限必要な文字数（省略の下限）。
 */
static int upstream_runtime_cmd_matches(
    const char* cmd_start,
    size_t cmd_len,
    const char* keyword,
    size_t min_len
) {
    size_t kw_len = strlen(keyword);
    if (cmd_len < min_len || cmd_len > kw_len) return FALSE;
    return (strncasecmp(cmd_start, keyword, cmd_len) == 0); /* AUDIT-ALLOW: bridge pre-parse intercept (zero-patch design) */
}

/*
 * ブリッジ層でのコマンド事前インターセプト。
 * Vim ソースに一切パッチを当てずに、:write / :update / :quit 系 / :redraw 系を
 * ホストアクションとして捕捉する。
 * 戻り値: TRUE ならインターセプト済み（Vim に渡さない）。
 */
static int upstream_runtime_try_intercept_ex(
    upstream_runtime_session_t* session,
    const char* command
) {
    size_t cmd_len = 0;
    int force = 0;
    const char* arg = NULL;
    const char* cmd = upstream_runtime_parse_ex_cmd(command, &cmd_len, &force, &arg);

    if (cmd_len == 0) return FALSE;

    /* :write, :w */
    if (upstream_runtime_cmd_matches(cmd, cmd_len, "write", 1)) {
        
        upstream_runtime_dispatch_core_write(session, arg, force);
        return TRUE;
    }

    /* :update, :up — ホスト側で dirty 判定するため常に write としてディスパッチ */
    if (upstream_runtime_cmd_matches(cmd, cmd_len, "update", 2)) {
        
        upstream_runtime_dispatch_core_write(session, arg, force);
        return TRUE;
    }

    /* :quit, :q */
    if (upstream_runtime_cmd_matches(cmd, cmd_len, "quit", 1)) {
        
        upstream_runtime_dispatch_core_quit(session, force);
        return TRUE;
    }

    /* :exit, :exi — Write + Quit（dirty 判定は Rust 側で実施） */
    if (upstream_runtime_cmd_matches(cmd, cmd_len, "exit", 3)) {

        upstream_runtime_dispatch_core_write(session, arg, force);
        upstream_runtime_dispatch_core_quit(session, force);
        return TRUE;
    }

    /* :xit, :x — Write + Quit（dirty 判定は Rust 側で実施） */
    if (upstream_runtime_cmd_matches(cmd, cmd_len, "xit", 1)) {

        upstream_runtime_dispatch_core_write(session, arg, force);
        upstream_runtime_dispatch_core_quit(session, force);
        return TRUE;
    }

    /* :wq — Write を先にキュー、次に Quit（ホストが save-before-quit を調整） */
    if (upstream_runtime_cmd_matches(cmd, cmd_len, "wq", 2)) {

        upstream_runtime_dispatch_core_write(session, arg, force);
        upstream_runtime_dispatch_core_quit(session, force);
        return TRUE;
    }

    /* :quitall, :qa, :qall */
    if (upstream_runtime_cmd_matches(cmd, cmd_len, "quitall", 2)
        || upstream_runtime_cmd_matches(cmd, cmd_len, "qall", 2)) {
        
        upstream_runtime_dispatch_core_quit(session, force);
        return TRUE;
    }

    /* :wqall, :wqa, :xall, :xa — Write + Quit（save-before-quit 用） */
    if (upstream_runtime_cmd_matches(cmd, cmd_len, "wqall", 3)
        || upstream_runtime_cmd_matches(cmd, cmd_len, "xall", 2)) {

        upstream_runtime_dispatch_core_write(session, arg, force);
        upstream_runtime_dispatch_core_quit(session, force);
        return TRUE;
    }

    /* :redraw, :redr */
    if (upstream_runtime_cmd_matches(cmd, cmd_len, "redraw", 4)) {
        size_t tail = (session->queue_head + session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
        if (session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
            session->queue[tail].action.kind = VIM_HOST_ACTION_REDRAW;
            session->queue[tail].action.redraw_force = (force != 0);
            session->queue_len++;
            upstream_runtime_queue_redraw_event(session, TRUE, force != 0);
        }
        return TRUE;
    }

    /* カスタムコマンドテーブルを検索（bell, input, HostBufAdd 等）
     * Vim の省略形ルールに従い、前方一致で検索する。
     * 例: "inp" → "input" にマッチ */
    for (int i = 0; i < upstream_runtime_custom_commands_count; i++) {
        const char* reg_name = upstream_runtime_custom_commands[i].name;
        size_t reg_len = strlen(reg_name);
        if (cmd_len <= reg_len && strncmp(cmd, reg_name, cmd_len) == 0) { /* AUDIT-ALLOW: callback table dispatch (zero-patch design) */
            
            /* コールバックに渡す exarg_T を構築 */
            exarg_T eap;
            CLEAR_FIELD(eap);
            eap.arg = (char_u*)arg;
            eap.forceit = force;
            upstream_runtime_custom_commands[i].callback((void*)&eap);
            return TRUE;
        }
    }

    return FALSE;
}

vim_core_command_result_t upstream_runtime_execute_ex_command(
    upstream_runtime_session_t* session,
    const char* command,
    uintptr_t command_len
) {
    char* cmd_copy = upstream_runtime_copy_text(command, command_len);
    if (cmd_copy == NULL) return upstream_runtime_command_error_result(session, 1U);

    upstream_runtime_active_session = session;

    /* ブリッジ層でコマンドを事前インターセプト（Vim ソースパッチ不要）
     * インターセプト時は Vim の実行ループに入らないため、
     * is_executing / quit_env は設定しない。
     * ※ コールバック内で longjmp(quit_env) が呼ばれると
     *   setjmp 前の無効な jmp_buf で SEGV する問題を防ぐ。 */
    if (upstream_runtime_try_intercept_ex(session, cmd_copy)) {
        free(cmd_copy);
        return upstream_runtime_host_action_result(session);
    }

    session->is_executing = TRUE;
    upstream_runtime_active_quit_env = &session->quit_env;

    /* Task 3.2: Capture window geometry before execution for layout change detection */
    upstream_runtime_capture_window_geometry(session);

    if (setjmp(session->quit_env) == 0) {
        do_cmdline_cmd((char_u*)cmd_copy);
    }

    /* Task 3.2: Detect layout changes and queue notification */
    if (upstream_runtime_detect_layout_change(session)) {
        size_t tail = (session->queue_head + session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
        if (session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
            session->queue[tail].action.kind = VIM_HOST_ACTION_LAYOUT_CHANGED;
            session->queue_len++;
            upstream_runtime_queue_layout_changed_event(session);
        }
        /* Update tracked geometry to current state */
        upstream_runtime_capture_window_geometry(session);
    }

    upstream_runtime_active_quit_env = NULL;
    session->is_executing = FALSE;

    /* autocommand 経由のイベントをポーリング */
    upstream_runtime_drain_vcr_events(session);

    free(cmd_copy);

    return upstream_runtime_detect_outcome(session);
}

vim_core_command_result_t upstream_runtime_execute_normal_command(
    upstream_runtime_session_t* session,
    const char* command,
    uintptr_t command_len
) {
    char* cmd_copy = upstream_runtime_copy_normal_input(command, command_len);
    if (cmd_copy == NULL) return upstream_runtime_command_error_result(session, 1U);

    upstream_runtime_active_session = session;
    session->is_executing = TRUE;
    upstream_runtime_is_injecting = TRUE;
    upstream_runtime_active_quit_env = &session->quit_env;
    session->pending_input = VIM_CORE_PENDING_INPUT_NONE;

    /* longjmpで残った可能性のあるロック状態をリセット（コマンド実行前）。
     * 前回のコマンドが補完操作中にlongjmpで脱出した場合、
     * compl_started=TRUE が残り、edit()再入時にE565エラーになるため、
     * 新しいコマンド開始前にクリアする。 */
    if (textlock != 0) {
        
        textlock = 0;
    }
    if (ins_compl_active()) {
        
        ins_compl_clear();
    }

    /* Task 3.2: Capture window geometry before execution for layout change detection */
    upstream_runtime_capture_window_geometry(session);

    if (setjmp(session->quit_env) == 0) {
        /* Requirement 4.1, 4.4: Safe initialization of oparg_T */
        clear_oparg(&session->oa);

        

        /* Set timeouts to 0 to prevent hanging on partial mappings/ESC */
        long old_p_ttm = p_ttm;
        long old_p_tm = p_tm;
        p_ttm = 0;
        p_tm = 0;

        /* Requirement 6.1: Use standard path (ins_typebuf + standard executors) */
        /* remap = REMAP_YES to support mappings in Requirement 6.5 */
        /* not_typed = FALSE to be treated as user input */
        ins_typebuf((char_u*)cmd_copy, REMAP_YES, 0, FALSE, FALSE);

        /* 
         * Requirement 6.3: Delegate state management to Vim.
         * We call the appropriate executor based on current State.
         */
        while (!stuff_empty() || typebuf.tb_len > 0) {
            int state = get_real_state();
            if (state & MODE_INSERT) {
                edit(NUL, FALSE, 1L);
            } else {
                update_curswant();
                normal_cmd(&session->oa, TRUE);
            }

            if (got_int) {
                got_int = FALSE;
                break;
            }
        }

        p_ttm = old_p_ttm;
        p_tm = old_p_tm;
    } else {
        
    }

    /* Reset Vim's internal flags that might have been left set by longjmp */
    vgetc_busy = 0;
    ex_normal_busy = 0;
    got_int = FALSE;
    textlock = 0;

    /* Task 3.2: Detect layout changes and queue notification */
    if (upstream_runtime_detect_layout_change(session)) {
        size_t tail = (session->queue_head + session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
        if (session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
            session->queue[tail].action.kind = VIM_HOST_ACTION_LAYOUT_CHANGED;
            session->queue_len++;
            upstream_runtime_queue_layout_changed_event(session);
        }
        upstream_runtime_capture_window_geometry(session);
    }

    upstream_runtime_is_injecting = FALSE;
    upstream_runtime_active_quit_env = NULL;
    session->is_executing = FALSE;

    /* autocommand 経由のイベントをポーリング */
    upstream_runtime_drain_vcr_events(session);

    upstream_runtime_refresh_pending_input(session, command, command_len);
    free(cmd_copy);

    return upstream_runtime_detect_outcome(session);
}

static char_u* upstream_runtime_get_curbuf_text(void) {
    uint64_t total_started_ns = upstream_runtime_now_ns();
    uint64_t measure_started_ns = upstream_runtime_now_ns();
    size_t total_len = 0;
    for (linenr_T lnum = 1; lnum <= curbuf->b_ml.ml_line_count; ++lnum) {
        total_len += STRLEN(ml_get(lnum)) + 1; // +1 for \n
    }
    uint64_t measure_ms = upstream_runtime_elapsed_ms(measure_started_ns);
    
    measure_started_ns = upstream_runtime_now_ns();
    char_u* buf = malloc(total_len + 1);
    if (buf == NULL) return NULL;
    uint64_t alloc_ms = upstream_runtime_elapsed_ms(measure_started_ns);
    
    measure_started_ns = upstream_runtime_now_ns();
    char_u* p = buf;
    for (linenr_T lnum = 1; lnum <= curbuf->b_ml.ml_line_count; ++lnum) {
        char_u* line = ml_get(lnum);
        size_t len = STRLEN(line);
        memcpy(p, line, len);
        p += len;
        *p++ = '\n';
    }
    *p = '\0';
    uint64_t copy_ms = upstream_runtime_elapsed_ms(measure_started_ns);
    upstream_runtime_debug_printf(
        "[PERF][upstream_runtime] get_curbuf_text line_count=%ld bytes=%zu measure_ms=%llu alloc_ms=%llu copy_ms=%llu total_ms=%llu\n",
        (long)curbuf->b_ml.ml_line_count,
        total_len,
        (unsigned long long)measure_ms,
        (unsigned long long)alloc_ms,
        (unsigned long long)copy_ms,
        (unsigned long long)upstream_runtime_elapsed_ms(total_started_ns)
    );
    return buf;
}

static vim_core_mode_t upstream_runtime_get_mode(const upstream_runtime_session_t* session) {
    int state = get_real_state();

    if ((state & MODE_CMDLINE) != 0) return VIM_CORE_MODE_COMMAND_LINE;
    if ((state & MODE_OP_PENDING) != 0) return VIM_CORE_MODE_OPERATOR_PENDING;
    if ((state & MODE_SELECT) != 0) {
        if (VIsual_mode == 'V') return VIM_CORE_MODE_SELECT_LINE;
        if (VIsual_mode == Ctrl_V) return VIM_CORE_MODE_SELECT_BLOCK;
        return VIM_CORE_MODE_SELECT;
    }
    if ((state & MODE_VISUAL) != 0) {
        if (VIsual_mode == 'V') return VIM_CORE_MODE_VISUAL_LINE;
        if (VIsual_mode == Ctrl_V) return VIM_CORE_MODE_VISUAL_BLOCK;
        return VIM_CORE_MODE_VISUAL;
    }
    if ((state & REPLACE_FLAG) != 0) return VIM_CORE_MODE_REPLACE;
    if ((state & MODE_INSERT) != 0) return VIM_CORE_MODE_INSERT;
    if (session != NULL
        && (state & MODE_NORMAL) != 0
        && session->oa.op_type != OP_NOP
        && !VIsual_active
        && restart_edit == 0) {
        return VIM_CORE_MODE_OPERATOR_PENDING;
    }
    return VIM_CORE_MODE_NORMAL;
}

vim_core_pending_input_t upstream_runtime_get_pending_input(
    const upstream_runtime_session_t* session
) {
    if (session == NULL) {
        return VIM_CORE_PENDING_INPUT_NONE;
    }

    
    return session->pending_input;
}

static void upstream_runtime_refresh_pending_input(
    upstream_runtime_session_t* session,
    const char* command,
    uintptr_t command_len
) {
    int state;
    char last_char;

    if (session == NULL) {
        return;
    }

    session->pending_input = VIM_CORE_PENDING_INPUT_NONE;
    if (command == NULL || command_len == 0) {
        
        return;
    }

    state = State;
    last_char = command[command_len - 1];
    if (last_char == 'r'
        && (state & MODE_REPLACE) != 0) {
        session->pending_input = VIM_CORE_PENDING_INPUT_REPLACE;
    } else if (last_char == 'f' || last_char == 'F'
               || last_char == 't' || last_char == 'T') {
        session->pending_input = VIM_CORE_PENDING_INPUT_CHAR;
    } else if (last_char == 'm') {
        session->pending_input = VIM_CORE_PENDING_INPUT_MARK_SET;
    } else if (last_char == '\'' || last_char == '`') {
        session->pending_input = VIM_CORE_PENDING_INPUT_MARK_JUMP;
    } else if (last_char == '"') {
        session->pending_input = VIM_CORE_PENDING_INPUT_REGISTER;
    }

    
}

bool upstream_runtime_get_mark(
    const upstream_runtime_session_t* session,
    char mark_name,
    vim_core_mark_position_t* out_mark
) {
    if (out_mark == NULL) {
        return false;
    }

    memset(out_mark, 0, sizeof(*out_mark));
    if (session == NULL || curbuf == NULL) {
        
        return false;
    }

    int mark_buf_id = curbuf->b_fnum;
    pos_T* pos = getmark_buf_fnum(curbuf, (int)(unsigned char)mark_name, FALSE, &mark_buf_id);
    if (pos == NULL || pos->lnum <= 0) {
        
        return false;
    }

    out_mark->is_set = true;
    out_mark->buf_id = mark_buf_id;
    out_mark->row = (uintptr_t)pos->lnum - 1;
    out_mark->col = (uintptr_t)pos->col;
    
    return true;
}

vim_core_status_t upstream_runtime_set_mark(
    upstream_runtime_session_t* session,
    char mark_name,
    int buf_id,
    uintptr_t row,
    uintptr_t col
) {
    pos_T pos;
    buf_T* buf;
    int result;

    if (session == NULL) {
        
        return VIM_CORE_STATUS_SESSION_ERROR;
    }

    

    if (!upstream_runtime_can_set_mark(mark_name)) {
        
        return VIM_CORE_STATUS_COMMAND_ERROR;
    }

    buf = buflist_findnr(buf_id);
    if (buf == NULL) {
        
        return VIM_CORE_STATUS_COMMAND_ERROR;
    }

    pos.lnum = (linenr_T)row + 1;
    pos.col = (colnr_T)col;
    pos.coladd = 0;

    if (!upstream_runtime_mark_position_is_valid(buf, pos)) {
        
        return VIM_CORE_STATUS_COMMAND_ERROR;
    }

    result = setmark_pos((int)(unsigned char)mark_name, &pos, buf_id);
    if (result != OK) {
        
        return VIM_CORE_STATUS_COMMAND_ERROR;
    }

    
    return VIM_CORE_STATUS_OK;
}

vim_core_jumplist_t upstream_runtime_get_jumplist(
    const upstream_runtime_session_t* session
) {
    vim_core_jumplist_t jumplist;
    memset(&jumplist, 0, sizeof(jumplist));

    if (session == NULL || curwin == NULL) {
        
        return jumplist;
    }

    cleanup_jumplist(curwin, TRUE);

    int valid_count = 0;
    for (int i = 0; i < curwin->w_jumplistlen; ++i) {
        if (curwin->w_jumplist[i].fmark.mark.lnum > 0) {
            valid_count++;
        }
    }

    jumplist.current_index = (uintptr_t)curwin->w_jumplistidx;
    jumplist.has_current_index = true;
    jumplist.entry_count = (uintptr_t)valid_count;

    upstream_runtime_debug_printf(
        "[DEBUG] get_jumplist: raw_len=%d raw_index=%d valid_count=%d\n",
        curwin->w_jumplistlen,
        curwin->w_jumplistidx,
        valid_count
    );

    if (valid_count == 0) {
        return jumplist;
    }

    jumplist.entries = (vim_core_jumplist_entry_t*)calloc(
        (size_t)valid_count,
        sizeof(vim_core_jumplist_entry_t)
    );
    if (jumplist.entries == NULL) {
        jumplist.entry_count = 0;
        
        return jumplist;
    }

    int out_index = 0;
    for (int i = 0; i < curwin->w_jumplistlen; ++i) {
        xfmark_T mark = curwin->w_jumplist[i];
        if (mark.fmark.mark.lnum <= 0) {
            continue;
        }

        jumplist.entries[out_index].buf_id = mark.fmark.fnum;
        jumplist.entries[out_index].row = (uintptr_t)mark.fmark.mark.lnum - 1;
        jumplist.entries[out_index].col = (uintptr_t)mark.fmark.mark.col;
        upstream_runtime_debug_printf(
            "[DEBUG] get_jumplist: entry[%d] buf_id=%d row=%lu col=%lu\n",
            out_index,
            jumplist.entries[out_index].buf_id,
            (unsigned long)jumplist.entries[out_index].row,
            (unsigned long)jumplist.entries[out_index].col
        );
        out_index++;
    }

    if (jumplist.current_index > jumplist.entry_count) {
        jumplist.current_index = jumplist.entry_count;
    }

    return jumplist;
}

void upstream_runtime_free_jumplist(vim_core_jumplist_t jumplist) {
    if (jumplist.entries != NULL) {
        free(jumplist.entries);
    }
}

static int upstream_runtime_can_set_mark(char mark_name) {
    int c = (int)(unsigned char)mark_name;

    return ASCII_ISLOWER(c) || ASCII_ISUPPER(c);
}

static int upstream_runtime_mark_position_is_valid(buf_T* buf, pos_T pos) {
    char_u* line;
    colnr_T line_len;

    if (buf == NULL) {
        return FALSE;
    }

    if (pos.lnum <= 0 || pos.lnum > buf->b_ml.ml_line_count) {
        return FALSE;
    }

    line = ml_get_buf(buf, pos.lnum, FALSE);
    if (line == NULL) {
        return FALSE;
    }

    line_len = (colnr_T)STRLEN(line);
    if (pos.col > line_len) {
        return FALSE;
    }

    return TRUE;
}

static void upstream_runtime_populate_buffers(vim_core_snapshot_t* snapshot) {
    /* Count buffers */
    uintptr_t count = 0;
    buf_T* buf;
    for (buf = firstbuf; buf != NULL; buf = buf->b_next) {
        count++;
    }
    

    if (count == 0) {
        snapshot->buffers = NULL;
        snapshot->buffer_count = 0;
        return;
    }

    vim_core_buffer_info_t* infos = (vim_core_buffer_info_t*)calloc(count, sizeof(vim_core_buffer_info_t));
    if (infos == NULL) {
        snapshot->buffers = NULL;
        snapshot->buffer_count = 0;
        return;
    }

    uintptr_t idx = 0;
    for (buf = firstbuf; buf != NULL && idx < count; buf = buf->b_next, idx++) {
        infos[idx].id = buf->b_fnum;
        if (buf->b_fname != NULL) {
            infos[idx].name_ptr = (const char*)buf->b_fname;
            infos[idx].name_len = STRLEN(buf->b_fname);
        } else {
            infos[idx].name_ptr = NULL;
            infos[idx].name_len = 0;
        }
        infos[idx].source_revision = (uint64_t)CHANGEDTICK(buf);
        infos[idx].dirty = buf->b_changed ? true : false;
        infos[idx].is_active = (buf == curbuf) ? true : false;
        /* VFS metadata: C 側はデフォルト値（Local / None）で初期化。
         * 実際の VFS metadata は Rust 側 coordinator が snapshot に投影する。 */
        infos[idx].source_kind = VIM_CORE_BUFFER_SOURCE_LOCAL;
        infos[idx].document_id_ptr = NULL;
        infos[idx].document_id_len = 0;
        infos[idx].pending_vfs_operation = VIM_CORE_VFS_OPERATION_NONE;
        infos[idx].deferred_close = false;
        infos[idx].last_vfs_error_ptr = NULL;
        infos[idx].last_vfs_error_len = 0;
        
    }

    snapshot->buffers = infos;
    snapshot->buffer_count = count;
}

static void upstream_runtime_populate_windows(vim_core_snapshot_t* snapshot) {
    /* Count windows */
    uintptr_t count = 0;
    win_T* wp;
    for (wp = firstwin; wp != NULL; wp = wp->w_next) {
        count++;
    }
    

    if (count == 0) {
        snapshot->windows = NULL;
        snapshot->window_count = 0;
        return;
    }

    vim_core_window_info_t* infos = (vim_core_window_info_t*)calloc(count, sizeof(vim_core_window_info_t));
    if (infos == NULL) {
        snapshot->windows = NULL;
        snapshot->window_count = 0;
        return;
    }

    uintptr_t idx = 0;
    for (wp = firstwin; wp != NULL && idx < count; wp = wp->w_next, idx++) {
        infos[idx].id = wp->w_id;
        infos[idx].buf_id = wp->w_buffer ? wp->w_buffer->b_fnum : 0;
        infos[idx].row = (uintptr_t)wp->w_winrow;
        infos[idx].col = (uintptr_t)wp->w_wincol;
        infos[idx].width = (uintptr_t)wp->w_width;
        infos[idx].height = (uintptr_t)wp->w_height;
        infos[idx].topline = (uintptr_t)wp->w_topline;
        infos[idx].botline = (uintptr_t)wp->w_botline;
        infos[idx].leftcol = (uintptr_t)wp->w_leftcol;
        infos[idx].skipcol = (uintptr_t)wp->w_skipcol;
        infos[idx].cursor_row = (uintptr_t)wp->w_cursor.lnum - 1;
        infos[idx].cursor_col = (uintptr_t)wp->w_cursor.col;
        infos[idx].is_active = (wp == curwin) ? true : false;
        
    }

    snapshot->windows = infos;
    snapshot->window_count = count;
}

/*
 * ポップアップメニュー（補完候補）の情報を抽出し、vim_core_pum_info_t として返す。
 * 補完モードが非アクティブ（complete_info()でモードが空）の場合は NULL を返す。
 * pum_visible() が true の場合は pum_getpos() で座標情報も取得する。
 * エラーが発生した場合も NULL を返す（Graceful Degradation）。
 */
static vim_core_pum_info_t* upstream_runtime_extract_pum_info(void) {
    /* インサートモードでない場合は補完状態を確認する必要がない */
    if (!(State & MODE_INSERT)) {
        
        return NULL;
    }

    
    fflush(stdout);

    /* --- complete_info(['items', 'selected', 'mode']) で補完状態を確認 --- */
    /* eval_expr は引数の文字列を一時的に書き換えるため、書き換え可能なバッファが必要 */
    char complete_info_expr[] = "complete_info(['items', 'selected', 'mode'])";
    typval_T* tv_info = eval_expr((char_u*)complete_info_expr, NULL);
    if (tv_info == NULL || tv_info->v_type != VAR_DICT || tv_info->vval.v_dict == NULL) {
        
        if (tv_info != NULL) free_tv(tv_info);
        return NULL;
    }

    dict_T* info_dict = tv_info->vval.v_dict;

    /* mode が空文字列なら補完モードではない */
    char_u* mode_str = dict_get_string(info_dict, "mode", FALSE);
    if (mode_str == NULL || mode_str[0] == '\0') {
        
        free_tv(tv_info);
        return NULL;
    }

    

    int selected_index = (int)dict_get_number_def(info_dict, "selected", -1);
    

    /* items リストを取得 */
    dictitem_T* di_items = dict_find(info_dict, (char_u*)"items", -1);
    if (di_items == NULL || di_items->di_tv.v_type != VAR_LIST || di_items->di_tv.vval.v_list == NULL) {
        
        free_tv(tv_info);
        return NULL;
    }

    list_T* items_list = di_items->di_tv.vval.v_list;
    int item_count = items_list->lv_len;
    

    /* 座標情報: pum_visible() が true の場合のみ pum_getpos() を使用 */
    int pum_row = 0, pum_col = 0, pum_width = 0, pum_height = 0;
    if (pum_visible()) {
        char pum_getpos_expr[] = "pum_getpos()";
        typval_T* tv_pos = eval_expr((char_u*)pum_getpos_expr, NULL);
        if (tv_pos != NULL && tv_pos->v_type == VAR_DICT && tv_pos->vval.v_dict != NULL) {
            dict_T* pos_dict = tv_pos->vval.v_dict;
            pum_row = (int)dict_get_number(pos_dict, "row");
            pum_col = (int)dict_get_number(pos_dict, "col");
            pum_width = (int)dict_get_number(pos_dict, "width");
            pum_height = (int)dict_get_number(pos_dict, "height");
        }
        if (tv_pos != NULL) free_tv(tv_pos);
        
    } else {
        
        /* ヘッドレスモードではPUMが画面描画されないため、
         * カーソル位置から推定した座標情報を提供する */
        pum_row = (int)curwin->w_wrow + W_WINROW(curwin) + 1;
        pum_col = (int)curwin->w_wcol + curwin->w_wincol;
        pum_height = item_count;
        /* 幅は候補の最大長から推定 */
        pum_width = 1;
        listitem_T* li_w = items_list->lv_first;
        for (int i = 0; i < item_count && li_w != NULL; i++, li_w = li_w->li_next) {
            if (li_w->li_tv.v_type == VAR_DICT && li_w->li_tv.vval.v_dict != NULL) {
                char_u* w = dict_get_string(li_w->li_tv.vval.v_dict, "word", FALSE);
                if (w != NULL) {
                    int wlen = (int)STRLEN(w);
                    if (wlen > pum_width) pum_width = wlen;
                }
            }
        }
    }

    /* pum_info 構造体を確保 */
    vim_core_pum_info_t* pum = (vim_core_pum_info_t*)calloc(1, sizeof(vim_core_pum_info_t));
    if (pum == NULL) {
        
        free_tv(tv_info);
        return NULL;
    }

    pum->row = pum_row;
    pum->col = pum_col;
    pum->width = pum_width;
    pum->height = pum_height;
    pum->selected_index = selected_index;
    pum->item_count = (size_t)item_count;

    if (item_count > 0) {
        pum->items = (vim_core_pum_item_t*)calloc((size_t)item_count, sizeof(vim_core_pum_item_t));
        if (pum->items == NULL) {
            
            free(pum);
            free_tv(tv_info);
            return NULL;
        }

        listitem_T* li = items_list->lv_first;
        for (int i = 0; i < item_count && li != NULL; i++, li = li->li_next) {
            if (li->li_tv.v_type != VAR_DICT || li->li_tv.vval.v_dict == NULL) {
                
                continue;
            }

            dict_T* item_dict = li->li_tv.vval.v_dict;

            /* 文字列フィールドをヒープにコピー（free_tv後もコピーが生存するようにする） */
            char_u* w;

            w = dict_get_string(item_dict, "word", FALSE);
            pum->items[i].word = w ? strdup((const char*)w) : strdup("");

            w = dict_get_string(item_dict, "abbr", FALSE);
            pum->items[i].abbr = w ? strdup((const char*)w) : strdup("");

            w = dict_get_string(item_dict, "menu", FALSE);
            pum->items[i].menu = w ? strdup((const char*)w) : strdup("");

            w = dict_get_string(item_dict, "kind", FALSE);
            pum->items[i].kind = w ? strdup((const char*)w) : strdup("");

            w = dict_get_string(item_dict, "info", FALSE);
            pum->items[i].info = w ? strdup((const char*)w) : strdup("");

            
        }
    }

    free_tv(tv_info);

    
    fflush(stdout);
    return pum;
}

static vim_core_snapshot_t upstream_runtime_snapshot_internal(
    const upstream_runtime_session_t* session,
    int include_text
) {
    uint64_t total_started_ns = upstream_runtime_now_ns();
    uint64_t measure_started_ns = upstream_runtime_now_ns();
    /* Synchronize layout state for the current window without side-effects.
     * Do not use update_screen(0) here as it triggers host actions and redraws.
     * We just need accurate coordinates (topline, botline, etc). */
    win_T* wp_sync;
    for (wp_sync = firstwin; wp_sync != NULL; wp_sync = wp_sync->w_next) {
        if (wp_sync == curwin) {
            update_topline();
            validate_cursor();
            validate_botline();
        }
    }
    uint64_t layout_ms = upstream_runtime_elapsed_ms(measure_started_ns);

    upstream_runtime_session_t* s = (upstream_runtime_session_t*)session;
    if (s == NULL) {
        vim_core_snapshot_t empty;
        memset(&empty, 0, sizeof(empty));
        return empty;
    }
    char_u* text = NULL;
    uint64_t text_ms = 0;
    if (include_text) {
        if (s->leased_snapshot_text) free(s->leased_snapshot_text);

        measure_started_ns = upstream_runtime_now_ns();
        text = upstream_runtime_get_curbuf_text();
        text_ms = upstream_runtime_elapsed_ms(measure_started_ns);
        s->leased_snapshot_text = (char*)text;
    }

    /* Requirement 4: Track buffer changes via CHANGEDTICK */
    varnumber_T current_tick = CHANGEDTICK(curbuf);
    if (current_tick != s->last_changedtick) {
        s->revision++;
        
        s->last_changedtick = current_tick;
    }

    vim_core_snapshot_t snapshot;
    memset(&snapshot, 0, sizeof(snapshot));
    snapshot.text_ptr = (const char*)text;
    if (text) snapshot.text_len = strlen((const char*)text);
    snapshot.cursor_row = (uintptr_t)curwin->w_cursor.lnum - 1;
    snapshot.cursor_col = (uintptr_t)curwin->w_cursor.col;
    snapshot.mode = upstream_runtime_get_mode(session);
    snapshot.pending_input = upstream_runtime_get_pending_input(session);
    snapshot.revision = s->revision;
    snapshot.dirty = curbuf->b_changed;
    snapshot.pending_host_actions = s->queue_len;

    /* Multi-buffer-window: populate buffer and window lists */
    measure_started_ns = upstream_runtime_now_ns();
    upstream_runtime_populate_buffers(&snapshot);
    uint64_t buffers_ms = upstream_runtime_elapsed_ms(measure_started_ns);
    measure_started_ns = upstream_runtime_now_ns();
    upstream_runtime_populate_windows(&snapshot);
    uint64_t windows_ms = upstream_runtime_elapsed_ms(measure_started_ns);

    /* ポップアップメニュー（補完候補）情報の抽出 */
    measure_started_ns = upstream_runtime_now_ns();
    snapshot.pum = upstream_runtime_extract_pum_info();
    uint64_t pum_ms = upstream_runtime_elapsed_ms(measure_started_ns);
    /* eval_expr + complete_info() の副作用でtextlockが残る場合があるため、リセット */
    if (textlock != 0) {
        
        fflush(stdout);
        textlock = 0;
    }

    upstream_runtime_debug_printf(
        "[PERF][upstream_runtime] snapshot include_text=%d line_count=%ld text_len=%zu layout_ms=%llu text_ms=%llu buffers_ms=%llu windows_ms=%llu pum_ms=%llu total_ms=%llu\n",
        include_text,
        (long)curbuf->b_ml.ml_line_count,
        text ? strlen((const char*)text) : 0,
        (unsigned long long)layout_ms,
        (unsigned long long)text_ms,
        (unsigned long long)buffers_ms,
        (unsigned long long)windows_ms,
        (unsigned long long)pum_ms,
        (unsigned long long)upstream_runtime_elapsed_ms(total_started_ns)
    );
    return snapshot;
}

vim_core_snapshot_t upstream_runtime_snapshot(const upstream_runtime_session_t* session) {
    return upstream_runtime_snapshot_internal(session, TRUE);
}

vim_core_snapshot_t upstream_runtime_light_snapshot(const upstream_runtime_session_t* session) {
    return upstream_runtime_snapshot_internal(session, FALSE);
}

static vim_core_command_result_t upstream_runtime_result(upstream_runtime_session_t* session, vim_core_status_t status, uint32_t reason_code, vim_core_command_outcome_kind_t outcome) {
    vim_core_command_result_t result;
    memset(&result, 0, sizeof(result));
    result.status = status;
    result.reason_code = reason_code;
    result.outcome = outcome;
    result.snapshot = upstream_runtime_snapshot(session);
    session->last_cursor_row = result.snapshot.cursor_row;
    session->last_cursor_col = result.snapshot.cursor_col;
    session->last_mode = result.snapshot.mode;
    return result;
}

static vim_core_command_result_t upstream_runtime_ok_result(upstream_runtime_session_t* session) {
    return upstream_runtime_result(session, VIM_CORE_STATUS_OK, 0, VIM_CORE_COMMAND_OUTCOME_BUFFER_CHANGED);
}

static vim_core_command_result_t upstream_runtime_host_action_result(upstream_runtime_session_t* session) {
    return upstream_runtime_result(session, VIM_CORE_STATUS_OK, 0, VIM_CORE_COMMAND_OUTCOME_HOST_ACTION_QUEUED);
}

static vim_core_command_result_t upstream_runtime_command_error_result(upstream_runtime_session_t* session, uint32_t reason_code) {
    return upstream_runtime_result(session, VIM_CORE_STATUS_COMMAND_ERROR, reason_code, VIM_CORE_COMMAND_OUTCOME_NO_CHANGE);
}

static void upstream_runtime_capture_window_geometry(upstream_runtime_session_t* session) {
    size_t idx = 0;
    win_T* wp;
    for (wp = firstwin; wp != NULL && idx < UPSTREAM_RUNTIME_MAX_TRACKED_WINDOWS; wp = wp->w_next, idx++) {
        session->tracked_windows[idx].id = wp->w_id;
        session->tracked_windows[idx].row = wp->w_winrow;
        session->tracked_windows[idx].col = wp->w_wincol;
        session->tracked_windows[idx].width = wp->w_width;
        session->tracked_windows[idx].height = wp->w_height;
    }
    session->tracked_window_count = idx;
}

static int upstream_runtime_detect_layout_change(upstream_runtime_session_t* session) {
    /* Compare current window geometry with tracked state */
    size_t current_count = 0;
    win_T* wp;
    for (wp = firstwin; wp != NULL; wp = wp->w_next) {
        current_count++;
    }

    /* Window count changed → layout changed */
    if (current_count != session->tracked_window_count) {
        
        return TRUE;
    }

    /* Check each window's geometry */
    size_t idx = 0;
    for (wp = firstwin; wp != NULL && idx < session->tracked_window_count; wp = wp->w_next, idx++) {
        if (wp->w_id != session->tracked_windows[idx].id ||
            wp->w_winrow != session->tracked_windows[idx].row ||
            wp->w_wincol != session->tracked_windows[idx].col ||
            wp->w_width != session->tracked_windows[idx].width ||
            wp->w_height != session->tracked_windows[idx].height) {
            
            return TRUE;
        }
    }

    return FALSE;
}

static vim_core_command_result_t upstream_runtime_detect_outcome(upstream_runtime_session_t* session) {
    if (session->queue_len > 0) {
        return upstream_runtime_host_action_result(session);
    }

    if (CHANGEDTICK(curbuf) != session->last_changedtick) {
        return upstream_runtime_ok_result(session);
    }

    {
        vim_core_mode_t current_mode = upstream_runtime_get_mode(session);
        if (current_mode != session->last_mode) {
            return upstream_runtime_result(
                session,
                VIM_CORE_STATUS_OK,
                0,
                VIM_CORE_COMMAND_OUTCOME_MODE_CHANGED
            );
        }
    }

    {
        uintptr_t current_row = (uintptr_t)curwin->w_cursor.lnum - 1;
        uintptr_t current_col = (uintptr_t)curwin->w_cursor.col;
        if (current_row != session->last_cursor_row || current_col != session->last_cursor_col) {
            return upstream_runtime_result(
                session,
                VIM_CORE_STATUS_OK,
                0,
                VIM_CORE_COMMAND_OUTCOME_CURSOR_CHANGED
            );
        }
    }

    return upstream_runtime_result(
        session,
        VIM_CORE_STATUS_OK,
        0,
        VIM_CORE_COMMAND_OUTCOME_NO_CHANGE
    );
}

vim_host_action_t upstream_runtime_take_pending_host_action(upstream_runtime_session_t* session) {
    if (session == NULL || session->queue_len == 0) {
        vim_host_action_t action;
        memset(&action, 0, sizeof(action));
        return action;
    }

    vim_host_action_t action = session->queue[session->queue_head].action;
    session->queue_head = (session->queue_head + 1) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
    session->queue_len--;

    return action;
}

vim_core_event_t upstream_runtime_take_pending_event(upstream_runtime_session_t* session) {
    vim_core_event_t event;
    memset(&event, 0, sizeof(event));
    if (session == NULL || session->event_queue_len == 0) {
        return event;
    }

    event = session->event_queue[session->event_queue_head].event;
    session->event_queue_head =
        (session->event_queue_head + 1) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
    session->event_queue_len--;

    return event;
}

vim_runtime_backend_identity_t upstream_runtime_backend_identity(const upstream_runtime_session_t* session) {
    (void)session;
    return VIM_CORE_BACKEND_IDENTITY_UPSTREAM_RUNTIME;
}

void upstream_runtime_queue_bell_action(upstream_runtime_session_t* session) {
    size_t tail = (session->queue_head + session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
    if (session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
        session->queue[tail].action.kind = VIM_HOST_ACTION_BELL;
        session->queue_len++;
        upstream_runtime_queue_bell_event(session);
    }
}

static vim_core_command_result_t upstream_runtime_queue_input_action(upstream_runtime_session_t* session, const char* prompt, vim_core_input_request_kind_t kind) {
    size_t tail = (session->queue_head + session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
    if (session->queue_len >= UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
        return upstream_runtime_command_error_result(session, 2U);
    }

    size_t prompt_len = prompt != NULL ? strlen(prompt) : 0;
    char* prompt_copy = (char*)malloc(prompt_len + 1);
    if (prompt_copy == NULL) {
        return upstream_runtime_command_error_result(session, 3U);
    }
    if (prompt_len > 0) {
        memcpy(prompt_copy, prompt, prompt_len);
    }
    prompt_copy[prompt_len] = '\0';

    session->queue[tail].action.kind = VIM_HOST_ACTION_REQUEST_INPUT;
    session->queue[tail].action.input_kind = kind;
    session->queue[tail].action.primary_text_ptr = prompt_copy;
    session->queue[tail].action.primary_text_len = prompt_len;
    session->queue[tail].action.correlation_id = ++session->next_correlation_id;
    session->queue_len++;
    
    if (session->is_executing) {
        longjmp(session->quit_env, 1);
    }
    
    return upstream_runtime_host_action_result(session);
}

void upstream_runtime_set_register(upstream_runtime_session_t* session, char regname, const char* text, uintptr_t text_len) {
    (void)session;
    if (text == NULL) return;
    
    if (!valid_yank_reg((int)regname, TRUE)) return;
    
    char_u* text_copy = (char_u*)malloc(text_len + 1);
    memcpy(text_copy, text, text_len);
    text_copy[text_len] = '\0';
    
    write_reg_contents((int)regname, text_copy, -1, FALSE);
    free(text_copy);
}

vim_core_register_get_result_t upstream_runtime_get_register(
    const upstream_runtime_session_t* session,
    char regname
) {
    (void)session;
    vim_core_register_get_result_t result = {0};
    char_u* text = get_reg_contents((int)regname, 0);
    char* text_copy;

    if (text == NULL) {
        return result;
    }

    result.payload_len = (uintptr_t)STRLEN(text);
    text_copy = (char*)malloc(result.payload_len + 1U);
    if (text_copy == NULL) {
        result.payload_len = 0;
        return result;
    }

    if (result.payload_len > 0) {
        memcpy(text_copy, text, result.payload_len);
    }
    text_copy[result.payload_len] = '\0';
    result.payload_ptr = text_copy;
    return result;
}

static int upstream_runtime_option_scope_to_vim_flags(vim_core_option_scope_t scope) {
    switch (scope) {
        case VIM_CORE_OPTION_SCOPE_GLOBAL:
            return OPT_GLOBAL;
        case VIM_CORE_OPTION_SCOPE_LOCAL:
            return OPT_LOCAL;
        case VIM_CORE_OPTION_SCOPE_DEFAULT:
        default:
            return 0;
    }
}

static vim_core_option_type_t upstream_runtime_option_type_from_flags(int option_flags) {
    if ((option_flags & P_STRING) != 0) {
        return VIM_CORE_OPTION_TYPE_STRING;
    }
    if ((option_flags & P_NUM) != 0) {
        return VIM_CORE_OPTION_TYPE_NUMBER;
    }
    if ((option_flags & P_BOOL) != 0) {
        return VIM_CORE_OPTION_TYPE_BOOL;
    }
    return VIM_CORE_OPTION_TYPE_UNKNOWN;
}

static int upstream_runtime_option_rejects_local_scope(const char* name, int* option_flags) {
    int opt_idx;

    if (name == NULL) {
        return FALSE;
    }

    opt_idx = findoption((char_u*)name);
    if (opt_idx < 0) {
        return FALSE;
    }

    if (option_flags != NULL) {
        *option_flags = (int)get_option_flags(opt_idx);
    }

    return is_global_option(opt_idx);
}

static vim_core_option_get_result_t upstream_runtime_option_get_result_with_status(
    vim_core_status_t status
) {
    vim_core_option_get_result_t result;
    memset(&result, 0, sizeof(result));
    result.status = status;
    result.option_type = VIM_CORE_OPTION_TYPE_UNKNOWN;
    return result;
}

static vim_core_option_set_result_t upstream_runtime_option_set_result_with_error(
    vim_core_status_t status,
    const char* error_message
) {
    vim_core_option_set_result_t result;
    memset(&result, 0, sizeof(result));
    result.status = status;

    if (error_message != NULL) {
        char* copy = strdup(error_message);
        if (copy == NULL) {
            result.status = VIM_CORE_STATUS_SESSION_ERROR;
            return result;
        }
        result.error_message_ptr = copy;
        result.error_message_len = strlen(copy);
    }

    return result;
}

vim_core_option_get_result_t upstream_runtime_get_option(
    const upstream_runtime_session_t* session,
    const char* name,
    vim_core_option_scope_t scope
) {
    vim_core_option_get_result_t result;
    long number_value = 0;
    char_u* string_value = NULL;
    int option_flags = 0;
    int vim_scope = upstream_runtime_option_scope_to_vim_flags(scope);
    getoption_T option_kind;

    if (session == NULL || name == NULL) {
        
        return upstream_runtime_option_get_result_with_status(VIM_CORE_STATUS_SESSION_ERROR);
    }

    

    result = upstream_runtime_option_get_result_with_status(VIM_CORE_STATUS_OK);
    if (scope == VIM_CORE_OPTION_SCOPE_LOCAL
        && upstream_runtime_option_rejects_local_scope(name, &option_flags)) {
        result.status = VIM_CORE_STATUS_COMMAND_ERROR;
        result.option_type = upstream_runtime_option_type_from_flags(option_flags);
        upstream_runtime_debug_printf(
            "[DEBUG] get_option: local scope unsupported for name='%s' flags=%d\n",
            name,
            option_flags
        );
        return result;
    }

    option_kind = get_option_value(
        (char_u*)name,
        &number_value,
        &string_value,
        &option_flags,
        vim_scope
    );

    result.option_type = upstream_runtime_option_type_from_flags(option_flags);
    switch (option_kind) {
        case gov_bool:
        case gov_number:
            result.number_value = (int64_t)number_value;
            break;
        case gov_string:
            if (string_value != NULL) {
                char* copied = strdup((const char*)string_value);
                if (copied == NULL) {
                    
                    vim_free(string_value);
                    return upstream_runtime_option_get_result_with_status(
                        VIM_CORE_STATUS_SESSION_ERROR
                    );
                }
                result.string_value_ptr = copied;
                result.string_value_len = strlen(copied);
                vim_free(string_value);
            }
            break;
        case gov_hidden_bool:
            result.option_type = VIM_CORE_OPTION_TYPE_BOOL;
            result.status = VIM_CORE_STATUS_COMMAND_ERROR;
            break;
        case gov_hidden_number:
            result.option_type = VIM_CORE_OPTION_TYPE_NUMBER;
            result.status = VIM_CORE_STATUS_COMMAND_ERROR;
            break;
        case gov_hidden_string:
            result.option_type = VIM_CORE_OPTION_TYPE_STRING;
            result.status = VIM_CORE_STATUS_COMMAND_ERROR;
            break;
        case gov_unknown:
        default:
            result.option_type = VIM_CORE_OPTION_TYPE_UNKNOWN;
            result.status = VIM_CORE_STATUS_COMMAND_ERROR;
            break;
    }

    upstream_runtime_debug_printf(
        "[DEBUG] get_option: name='%s' kind=%d status=%d type=%d number=%lld string_len=%lu\n",
        name,
        (int)option_kind,
        (int)result.status,
        (int)result.option_type,
        (long long)result.number_value,
        (unsigned long)result.string_value_len
    );

    return result;
}

vim_core_option_set_result_t upstream_runtime_set_option_number(
    upstream_runtime_session_t* session,
    const char* name,
    int64_t value,
    vim_core_option_scope_t scope
) {
    int vim_scope = upstream_runtime_option_scope_to_vim_flags(scope);
    char* error_message;

    if (session == NULL || name == NULL) {
        
        return upstream_runtime_option_set_result_with_error(
            VIM_CORE_STATUS_SESSION_ERROR,
            NULL
        );
    }

    

    if (findoption((char_u*)name) < 0) {
        
        return upstream_runtime_option_set_result_with_error(
            VIM_CORE_STATUS_COMMAND_ERROR,
            "Unknown option"
        );
    }

    error_message = set_option_value((char_u*)name, (long)value, NULL, vim_scope);
    if (error_message != NULL) {
        
        return upstream_runtime_option_set_result_with_error(
            VIM_CORE_STATUS_COMMAND_ERROR,
            error_message
        );
    }

    return upstream_runtime_option_set_result_with_error(VIM_CORE_STATUS_OK, NULL);
}

vim_core_option_set_result_t upstream_runtime_set_option_string(
    upstream_runtime_session_t* session,
    const char* name,
    const char* value,
    vim_core_option_scope_t scope
) {
    int vim_scope = upstream_runtime_option_scope_to_vim_flags(scope);
    char* error_message;

    if (session == NULL || name == NULL || value == NULL) {
        
        return upstream_runtime_option_set_result_with_error(
            VIM_CORE_STATUS_SESSION_ERROR,
            NULL
        );
    }

    

    if (findoption((char_u*)name) < 0) {
        
        return upstream_runtime_option_set_result_with_error(
            VIM_CORE_STATUS_COMMAND_ERROR,
            "Unknown option"
        );
    }

    error_message = set_option_value((char_u*)name, 0L, (char_u*)value, vim_scope);
    if (error_message != NULL) {
        
        return upstream_runtime_option_set_result_with_error(
            VIM_CORE_STATUS_COMMAND_ERROR,
            error_message
        );
    }

    return upstream_runtime_option_set_result_with_error(VIM_CORE_STATUS_OK, NULL);
}

void upstream_runtime_set_screen_size(upstream_runtime_session_t* session, int rows, int cols) {

    Rows = rows;
    Columns = cols;
    screenalloc(TRUE);
    shell_new_rows();
    shell_new_columns();
    /* 画面サイズ変更後、メッセージページャの残行数を再同期する。
     * msg_starthere() は条件付き（!msg_didany || lines_left < 0）でしか
     * 呼ばれないため、lines_left を -1 にして次の msg_start() で
     * cmdline_row から再計算させる。 */
    lines_left = -1;
    

    /* Task 3.2: Notify layout change after screen resize */
    if (session != NULL) {
        size_t tail = (session->queue_head + session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
        if (session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
            session->queue[tail].action.kind = VIM_HOST_ACTION_LAYOUT_CHANGED;
            session->queue_len++;
            upstream_runtime_queue_layout_changed_event(session);
        }
    }
}

vim_core_status_t upstream_runtime_switch_to_buffer(upstream_runtime_session_t* session, int buf_id) {
    (void)session;
    

    buf_T* buf = buflist_findnr(buf_id);
    if (buf == NULL) {
        
        return VIM_CORE_STATUS_COMMAND_ERROR;
    }

    set_curbuf(buf, DOBUF_GOTO);
    
    return VIM_CORE_STATUS_OK;
}

vim_core_status_t upstream_runtime_switch_to_window(upstream_runtime_session_t* session, int win_id) {
    (void)session;
    

    win_T* wp = win_id2wp(win_id);
    if (wp == NULL) {
        
        return VIM_CORE_STATUS_COMMAND_ERROR;
    }

    win_goto(wp);
    
    return VIM_CORE_STATUS_OK;
}

char* upstream_runtime_get_buffer_text(const upstream_runtime_session_t* session, int buf_id) {
    (void)session;
    

    buf_T* buf = buflist_findnr(buf_id);
    if (buf == NULL) {
        
        return NULL;
    }

    /* Calculate total text length */
    size_t total_len = 0;
    for (linenr_T lnum = 1; lnum <= buf->b_ml.ml_line_count; ++lnum) {
        total_len += STRLEN(ml_get_buf(buf, lnum, FALSE)) + 1;
    }

    char* result = (char*)malloc(total_len + 1);
    if (result == NULL) return NULL;

    char* p = result;
    for (linenr_T lnum = 1; lnum <= buf->b_ml.ml_line_count; ++lnum) {
        char_u* line = ml_get_buf(buf, lnum, FALSE);
        size_t len = STRLEN(line);
        memcpy(p, line, len);
        p += len;
        /* Add newline between lines but also after last line to match existing behavior */
        if (lnum < buf->b_ml.ml_line_count) {
            *p++ = '\n';
        }
    }
    *p = '\0';

    
    return result;
}

vim_core_buffer_line_range_t upstream_runtime_get_buffer_line_range(
    const upstream_runtime_session_t* session,
    int buf_id,
    uintptr_t start_row,
    uintptr_t line_count
) {
    (void)session;

    vim_core_buffer_line_range_t result;
    memset(&result, 0, sizeof(result));
    result.buffer_id = buf_id;
    result.start_row = start_row;

    buf_T* buf = buflist_findnr(buf_id);
    if (buf == NULL) {
        return result;
    }

    result.found = true;
    result.source_revision = (uint64_t)CHANGEDTICK(buf);
    result.total_line_count = (uintptr_t)buf->b_ml.ml_line_count;

    if (line_count == 0 || start_row >= result.total_line_count) {
        return result;
    }

    uintptr_t available = result.total_line_count - start_row;
    uintptr_t resolved_count = line_count < available ? line_count : available;
    vim_core_buffer_line_t* lines =
        (vim_core_buffer_line_t*)calloc(resolved_count, sizeof(vim_core_buffer_line_t));
    if (lines == NULL) {
        return result;
    }

    for (uintptr_t idx = 0; idx < resolved_count; ++idx) {
        linenr_T lnum = (linenr_T)(start_row + idx + 1);
        char_u* line = ml_get_buf(buf, lnum, FALSE);
        size_t len = STRLEN(line);
        char* copy = (char*)malloc(len + 1);
        if (copy == NULL) {
            for (uintptr_t cleanup = 0; cleanup < idx; ++cleanup) {
                free((void*)lines[cleanup].text_ptr);
            }
            free(lines);
            result.lines = NULL;
            result.line_count = 0;
            return result;
        }
        memcpy(copy, line, len);
        copy[len] = '\0';
        lines[idx].text_ptr = copy;
        lines[idx].text_len = (uintptr_t)len;
    }

    result.lines = lines;
    result.line_count = resolved_count;
    return result;
}

vim_core_status_t upstream_runtime_set_buffer_text(
    upstream_runtime_session_t* session,
    int buf_id,
    const char* text,
    uintptr_t text_len
) {
    (void)session;
    

    buf_T* buf = buflist_findnr(buf_id);
    if (buf == NULL) {
        
        return VIM_CORE_STATUS_COMMAND_ERROR;
    }

    buf_T* previous_buf = curbuf;
    if (previous_buf != buf) {
        set_curbuf(buf, DOBUF_GOTO);
    }

    while (curbuf->b_ml.ml_line_count > 1) {
        ml_delete(1);
    }
    ml_replace(1, (char_u*)"", TRUE);

    if (text != NULL && text_len > 0) {
        char* copy = (char*)malloc(text_len + 1);
        if (copy == NULL) {
            if (previous_buf != buf) {
                set_curbuf(previous_buf, DOBUF_GOTO);
            }
            return VIM_CORE_STATUS_SESSION_ERROR;
        }
        memcpy(copy, text, text_len);
        copy[text_len] = '\0';

        char_u* line_start = (char_u*)copy;
        int line_count = 0;
        for (uintptr_t i = 0; i < text_len; ++i) {
            if (copy[i] == '\n') {
                copy[i] = '\0';
                if (line_count == 0) {
                    ml_replace(1, line_start, TRUE);
                } else {
                    ml_append(line_count, line_start, 0, FALSE);
                }
                line_count++;
                line_start = (char_u*)&copy[i + 1];
            }
        }
        if (*line_start != '\0' || line_count == 0) {
            if (line_count == 0) {
                ml_replace(1, line_start, TRUE);
            } else {
                ml_append(line_count, line_start, 0, FALSE);
            }
        }

        free(copy);
    }

    unchanged(curbuf, FALSE, TRUE);
    if (previous_buf != buf) {
        set_curbuf(previous_buf, DOBUF_GOTO);
    }

    return VIM_CORE_STATUS_OK;
}

vim_core_status_t upstream_runtime_set_buffer_name(
    upstream_runtime_session_t* session,
    int buf_id,
    const char* name,
    uintptr_t name_len
) {
    (void)session;
    

    if (name == NULL) {
        return VIM_CORE_STATUS_COMMAND_ERROR;
    }

    char_u* copy = vim_strnsave((char_u*)name, (int)name_len);
    if (copy == NULL) {
        return VIM_CORE_STATUS_SESSION_ERROR;
    }

    buf_set_name(buf_id, copy);
    vim_free(copy);
    return VIM_CORE_STATUS_OK;
}

vim_core_status_t upstream_runtime_set_buffer_dirty(
    upstream_runtime_session_t* session,
    int buf_id,
    bool dirty
) {
    (void)session;
    

    buf_T* buf = buflist_findnr(buf_id);
    if (buf == NULL) {
        
        return VIM_CORE_STATUS_COMMAND_ERROR;
    }

    buf->b_changed = dirty ? 1 : 0;
    return VIM_CORE_STATUS_OK;
}

vim_core_status_t upstream_runtime_commit_buffer_update(
    upstream_runtime_session_t* session,
    const vim_core_buffer_commit_t* commit
) {
    if (commit == NULL) {
        
        return VIM_CORE_STATUS_SESSION_ERROR;
    }

    

    vim_core_status_t status;

    if (commit->replace_text && commit->text_ptr != NULL) {
        status = upstream_runtime_set_buffer_text(
            session, commit->target_buf_id,
            commit->text_ptr, commit->text_len
        );
        if (status != VIM_CORE_STATUS_OK) {
            
            return status;
        }
    }

    if (commit->display_name_ptr != NULL && commit->display_name_len > 0) {
        status = upstream_runtime_set_buffer_name(
            session, commit->target_buf_id,
            commit->display_name_ptr, commit->display_name_len
        );
        if (status != VIM_CORE_STATUS_OK) {
            
            return status;
        }
    }

    if (commit->clear_dirty) {
        status = upstream_runtime_set_buffer_dirty(
            session, commit->target_buf_id, false
        );
        if (status != VIM_CORE_STATUS_OK) {
            
            return status;
        }
    }

    
    return VIM_CORE_STATUS_OK;
}

static void upstream_runtime_collect_undo_tree(buf_T* buf, u_header_T* first_uhp, vim_core_undo_node_t* nodes, uintptr_t* index) {
    u_header_T* uhp = first_uhp;
    while (uhp != NULL) {
        if (*index >= (uintptr_t)buf->b_u_numhead) {
            break;
        }
        
        vim_core_undo_node_t* node = &nodes[*index];
        (*index)++;
        
        node->seq = uhp->uh_seq;
        node->time = (long)uhp->uh_time;
        node->save_nr = uhp->uh_save_nr;
        node->prev_seq = uhp->uh_prev.ptr ? uhp->uh_prev.ptr->uh_seq : 0;
        node->next_seq = uhp->uh_next.ptr ? uhp->uh_next.ptr->uh_seq : 0;
        node->alt_next_seq = uhp->uh_alt_next.ptr ? uhp->uh_alt_next.ptr->uh_seq : 0;
        node->alt_prev_seq = uhp->uh_alt_prev.ptr ? uhp->uh_alt_prev.ptr->uh_seq : 0;
        node->is_newhead = (uhp == buf->b_u_newhead);
        node->is_curhead = (uhp == buf->b_u_curhead);
        
        if (uhp->uh_alt_next.ptr != NULL) {
            upstream_runtime_collect_undo_tree(buf, uhp->uh_alt_next.ptr, nodes, index);
        }
        uhp = uhp->uh_prev.ptr;
    }
}

int upstream_runtime_get_undo_tree(int buf_id, vim_core_undo_tree_t* out_tree) {
    if (out_tree == NULL) return -1;
    memset(out_tree, 0, sizeof(*out_tree));
    
    buf_T* buf = buflist_findnr(buf_id);
    if (buf == NULL) return -1;
    
    out_tree->synced = buf->b_u_synced ? true : false;
    out_tree->seq_last = buf->b_u_seq_last;
    out_tree->save_last = buf->b_u_save_nr_last;
    out_tree->seq_cur = buf->b_u_seq_cur;
    out_tree->time_cur = (long)buf->b_u_time_cur;
    out_tree->save_cur = buf->b_u_save_nr_cur;
    
    if (buf->b_u_numhead == 0 || buf->b_u_oldhead == NULL) {
        out_tree->length = 0;
        out_tree->nodes = NULL;
        return 0;
    }
    
    out_tree->length = buf->b_u_numhead;
    out_tree->nodes = (vim_core_undo_node_t*)calloc(out_tree->length, sizeof(vim_core_undo_node_t));
    if (out_tree->nodes == NULL) {
        out_tree->length = 0;
        return -1;
    }
    
    uintptr_t current_index = 0;
    upstream_runtime_collect_undo_tree(buf, buf->b_u_oldhead, out_tree->nodes, &current_index);
    out_tree->length = current_index;
    
    return 0;
}

void upstream_runtime_free_undo_tree(vim_core_undo_tree_t tree) {
    if (tree.nodes != NULL) {
        free(tree.nodes);
    }
}

static bool upstream_runtime_check_undo_seq(buf_T* buf, u_header_T* first_uhp, long target_seq) {
    if (target_seq == 0) return true;
    u_header_T* uhp = first_uhp;
    while (uhp != NULL) {
        if (uhp->uh_seq == target_seq) return true;
        if (uhp->uh_alt_next.ptr != NULL) {
            if (upstream_runtime_check_undo_seq(buf, uhp->uh_alt_next.ptr, target_seq)) return true;
        }
        uhp = uhp->uh_prev.ptr;
    }
    return false;
}

int upstream_runtime_undo_jump(int buf_id, long seq) {
    buf_T* buf = buflist_findnr(buf_id);
    if (buf == NULL) return -1;

    if (!upstream_runtime_check_undo_seq(buf, buf->b_u_oldhead, seq)) {
        return -1;
    }

    aco_save_T aco;
    aucmd_prepbuf(&aco, buf);

    undo_time(seq, FALSE, FALSE, TRUE);

    aucmd_restbuf(&aco);

    return 0;
}

int upstream_runtime_get_line_syntax(int win_id, long lnum, int* out_ids, int max_cols) {
    win_T* wp = win_id2wp(win_id);
    if (wp == NULL || wp->w_buffer == NULL) return -1;

    buf_T* buf = wp->w_buffer;
    if (lnum < 1 || lnum > buf->b_ml.ml_line_count) return -1;

    char_u* line = ml_get_buf(buf, lnum, FALSE);
    if (line == NULL) return -1;

    int line_len = (int)STRLEN(line);
    int cols = line_len > max_cols ? max_cols : line_len;

    for (int col = 0; col < cols; col++) {
        out_ids[col] = syn_get_id(wp, lnum, (colnr_T)col, 1, NULL, FALSE);
    }
    
    return cols;
}

const char* upstream_runtime_get_syntax_name(int syn_id) {
    return (const char*)syn_id2name(syn_id);
}

char* upstream_runtime_eval_string(upstream_runtime_session_t* session, const char* expr) {
    

    if (session == NULL || expr == NULL) {
        
        return NULL;
    }

    size_t queue_len_before = session->queue_len;

    /* Vim の内部状態を eval 用にセットアップ */
    upstream_runtime_session_t* prev_active = upstream_runtime_active_session;
    upstream_runtime_active_session = session;

    /* AUDIT-ALLOW: eval_to_string is the Vim C API for expression evaluation */
    char_u* result = eval_to_string((char_u*)expr, FALSE, FALSE);

    /* 元の状態に復帰 */
    upstream_runtime_active_session = prev_active;

    if (session->queue_len > queue_len_before) {
        int queued_input = FALSE;
        size_t idx;
        for (idx = queue_len_before; idx < session->queue_len; ++idx) {
            size_t pos = (session->queue_head + idx) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
            if (session->queue[pos].action.kind == VIM_HOST_ACTION_REQUEST_INPUT) {
                queued_input = TRUE;
                upstream_runtime_debug_printf(
                    "[DEBUG] eval_string: input request queued during eval prompt_len=%lu input_kind=%u correlation_id=%llu\n",
                    (unsigned long)session->queue[pos].action.primary_text_len,
                    (unsigned int)session->queue[pos].action.input_kind,
                    (unsigned long long)session->queue[pos].action.correlation_id
                );
                break;
            }
        }
        if (queued_input) {
            if (result != NULL) vim_free(result);
            return NULL;
        }
    }

    if (result == NULL) {
        
        return NULL;
    }

    /* eval_to_string returns vim_strsave'd memory (vim_alloc).
       We copy it to a standard malloc'd buffer so Rust can free it with vim_bridge_free_string. */
    size_t len = STRLEN(result);
    char* out = (char*)malloc(len + 1);
    if (out == NULL) {
        vim_free(result);
        
        return NULL;
    }
    memcpy(out, result, len + 1);
    vim_free(result);

    
    return out;
}

void upstream_runtime_submit_input_response(
    upstream_runtime_session_t* session,
    const char* value,
    uintptr_t value_len,
    bool cancelled
) {
    char* copy = NULL;

    if (session == NULL) return;
    if (session->pending_input_response_value) {
        free(session->pending_input_response_value);
        session->pending_input_response_value = NULL;
    }

    if (!cancelled && value != NULL && value_len > 0) {
        copy = (char*)malloc((size_t)value_len + 1U);
        if (copy == NULL) {
            session->pending_input_response_available = FALSE;
            session->pending_input_response_len = 0;
            session->pending_input_response_cancelled = TRUE;
            return;
        }
        memcpy(copy, value, (size_t)value_len);
        copy[value_len] = '\0';
    }

    session->pending_input_response_value = copy;
    session->pending_input_response_len = cancelled ? 0 : value_len;
    session->pending_input_response_cancelled = cancelled ? TRUE : FALSE;
    session->pending_input_response_available = TRUE;
}

/*
 * g:_vcr_events ポーリング: autocommand 経由で蓄積されたイベントを
 * ホストアクションキューにディスパッチし、リストをクリアする。
 */
static void upstream_runtime_drain_vcr_events(upstream_runtime_session_t* session) {
    if (session == NULL) return;

    /* g:_vcr_events リストを直接 C API で取得（eval_to_string を避ける） */
    dictitem_T* di = find_var((char_u*)"g:_vcr_events", NULL, TRUE);
    if (di == NULL || di->di_tv.v_type != VAR_LIST) return;

    list_T* events = di->di_tv.vval.v_list;
    if (events == NULL || events->lv_len == 0) return;

    

    /* リストの各要素を処理 */
    listitem_T* li;
    FOR_ALL_LIST_ITEMS(events, li) {
        if (li->li_tv.v_type != VAR_STRING || li->li_tv.vval.v_string == NULL) continue;
        const char* event = (const char*)li->li_tv.vval.v_string;

        if (strncmp(event, "BufAdd:", 7) == 0) { /* AUDIT-ALLOW: event type dispatch (zero-patch design) */
            int buf_id = atoi(event + 7);
            if (buf_id <= 0) buf_id = curbuf->b_fnum;
            size_t tail = (session->queue_head + session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
            if (session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
                session->queue[tail].action.kind = VIM_HOST_ACTION_BUF_ADD;
                session->queue[tail].action.event_buf_id = buf_id;
                session->queue_len++;
                upstream_runtime_queue_buf_add_event(session, buf_id);
            }
        } else if (strncmp(event, "WinNew:", 7) == 0) { /* AUDIT-ALLOW: event type dispatch (zero-patch design) */
            int win_id = atoi(event + 7);
            size_t tail = (session->queue_head + session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
            if (session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
                session->queue[tail].action.kind = VIM_HOST_ACTION_WIN_NEW;
                session->queue[tail].action.event_win_id = win_id;
                session->queue_len++;
                upstream_runtime_queue_win_new_event(session, win_id);
            }
        } else if (strncmp(event, "LayoutChanged", 13) == 0) { /* AUDIT-ALLOW: event type dispatch (zero-patch design) */
            size_t tail = (session->queue_head + session->queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
            if (session->queue_len < UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) {
                session->queue[tail].action.kind = VIM_HOST_ACTION_LAYOUT_CHANGED;
                session->queue_len++;
                upstream_runtime_queue_layout_changed_event(session);
            }
        }
    }

    /* リストをクリア（do_cmdline_cmd で安全にリセット） */
    do_cmdline_cmd((char_u*)"let g:_vcr_events = []");
}

static int upstream_runtime_enqueue_event(
    upstream_runtime_session_t* session,
    const vim_core_event_t* event
) {
    size_t tail;
    if (session == NULL || event == NULL) return FALSE;
    if (session->event_queue_len >= UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS) return FALSE;

    tail =
        (session->event_queue_head + session->event_queue_len) % UPSTREAM_RUNTIME_MAX_PENDING_ACTIONS;
    session->event_queue[tail].event = *event;
    session->event_queue_len++;
    return TRUE;
}

static int upstream_runtime_enqueue_message_event_for_session(
    upstream_runtime_session_t* session,
    const char* text,
    uintptr_t text_len,
    vim_core_message_severity_t severity,
    vim_core_message_category_t category
) {
    vim_core_event_t event;
    char* copy;
    if (session == NULL || text == NULL || text_len == 0) return FALSE;

    copy = (char*)malloc((size_t)text_len + 1U);
    if (copy == NULL) return FALSE;
    memcpy(copy, text, (size_t)text_len);
    copy[text_len] = '\0';

    memset(&event, 0, sizeof(event));
    event.kind = VIM_CORE_EVENT_MESSAGE;
    event.message_severity = severity;
    event.message_category = category;
    event.text_ptr = copy;
    event.text_len = text_len;
    if (!upstream_runtime_enqueue_event(session, &event)) {
        free(copy);
        return FALSE;
    }
    return TRUE;
}

static void upstream_runtime_queue_bell_event(upstream_runtime_session_t* session) {
    vim_core_event_t event;
    memset(&event, 0, sizeof(event));
    event.kind = VIM_CORE_EVENT_BELL;
    (void)upstream_runtime_enqueue_event(session, &event);
}

static void upstream_runtime_queue_redraw_event(
    upstream_runtime_session_t* session,
    int full,
    int clear_before_draw
) {
    vim_core_event_t event;
    memset(&event, 0, sizeof(event));
    event.kind = VIM_CORE_EVENT_REDRAW;
    event.full = (full != 0);
    event.clear_before_draw = (clear_before_draw != 0);
    (void)upstream_runtime_enqueue_event(session, &event);
}

static void upstream_runtime_queue_buf_add_event(
    upstream_runtime_session_t* session,
    int buf_id
) {
    vim_core_event_t event;
    memset(&event, 0, sizeof(event));
    event.kind = VIM_CORE_EVENT_BUF_ADD;
    event.buf_id = buf_id;
    (void)upstream_runtime_enqueue_event(session, &event);
}

static void upstream_runtime_queue_win_new_event(
    upstream_runtime_session_t* session,
    int win_id
) {
    vim_core_event_t event;
    memset(&event, 0, sizeof(event));
    event.kind = VIM_CORE_EVENT_WIN_NEW;
    event.win_id = win_id;
    (void)upstream_runtime_enqueue_event(session, &event);
}

static void upstream_runtime_queue_layout_changed_event(
    upstream_runtime_session_t* session
) {
    vim_core_event_t event;
    memset(&event, 0, sizeof(event));
    event.kind = VIM_CORE_EVENT_LAYOUT_CHANGED;
    (void)upstream_runtime_enqueue_event(session, &event);
}

static void upstream_runtime_queue_pager_prompt_event(
    upstream_runtime_session_t* session,
    vim_core_pager_prompt_kind_t kind
) {
    vim_core_event_t event;
    memset(&event, 0, sizeof(event));
    event.kind = VIM_CORE_EVENT_PAGER_PROMPT;
    event.pager_prompt_kind = kind;
    (void)upstream_runtime_enqueue_event(session, &event);
}

int upstream_runtime_embedded_mode_active(void) {
    return upstream_runtime_active_session != NULL;
}

void upstream_runtime_enqueue_message_event(
    const char* text,
    uintptr_t text_len,
    vim_core_message_severity_t severity,
    vim_core_message_category_t category
) {
    (void)upstream_runtime_enqueue_message_event_for_session(
        upstream_runtime_active_session,
        text,
        text_len,
        severity,
        category
    );
}

void upstream_runtime_enqueue_input_request(
    const char* prompt,
    uintptr_t prompt_len,
    vim_core_input_request_kind_t kind
) {
    size_t bounded_len;
    char* prompt_copy;

    if (upstream_runtime_active_session == NULL) return;
    if (prompt == NULL) prompt = "";

    bounded_len = (size_t)prompt_len;
    prompt_copy = (char*)malloc(bounded_len + 1U);
    if (prompt_copy == NULL) return;
    if (bounded_len > 0) memcpy(prompt_copy, prompt, bounded_len);
    prompt_copy[bounded_len] = '\0';

    (void)upstream_runtime_queue_input_action(
        upstream_runtime_active_session,
        prompt_copy,
        kind
    );
    free(prompt_copy);
}

int upstream_runtime_take_input_response(
    vim_core_input_request_kind_t kind,
    char** value_ptr,
    uintptr_t* value_len,
    bool* cancelled
) {
    upstream_runtime_session_t* session = upstream_runtime_active_session;
    char* out = NULL;

    if (value_ptr != NULL) *value_ptr = NULL;
    if (value_len != NULL) *value_len = 0;
    if (cancelled != NULL) *cancelled = false;

    if (session == NULL || !session->pending_input_response_available) {
        return FALSE;
    }

    if (!session->pending_input_response_cancelled
        && session->pending_input_response_value != NULL
        && session->pending_input_response_len > 0) {
        out = (char*)malloc((size_t)session->pending_input_response_len + 1U);
        if (out == NULL) {
            return FALSE;
        }
        memcpy(out, session->pending_input_response_value, (size_t)session->pending_input_response_len);
        out[session->pending_input_response_len] = '\0';
    }

    upstream_runtime_debug_printf(
        "[DEBUG] take_input_response: consumed input_kind=%u cancelled=%d value_len=%lu\n",
        (unsigned int)kind,
        session->pending_input_response_cancelled,
        (unsigned long)session->pending_input_response_len
    );

    if (value_ptr != NULL) *value_ptr = out;
    else if (out != NULL) free(out);
    if (value_len != NULL) *value_len = session->pending_input_response_len;
    if (cancelled != NULL) *cancelled = session->pending_input_response_cancelled ? true : false;

    if (session->pending_input_response_value) {
        free(session->pending_input_response_value);
        session->pending_input_response_value = NULL;
    }
    session->pending_input_response_len = 0;
    session->pending_input_response_cancelled = FALSE;
    session->pending_input_response_available = FALSE;

    return TRUE;
}

void upstream_runtime_enqueue_pager_prompt_event(vim_core_pager_prompt_kind_t kind) {
    if (upstream_runtime_active_session == NULL) return;
    upstream_runtime_queue_pager_prompt_event(upstream_runtime_active_session, kind);
}

void upstream_runtime_enqueue_bell_for_active_session(void) {
    if (upstream_runtime_active_session == NULL) return;
    upstream_runtime_queue_bell_action(upstream_runtime_active_session);
}

/* 
 * Search Highlight Extraction API 
 */

static char* upstream_runtime_leased_search_string = NULL;

static int upstream_runtime_search_cmdline_type(void) {
    cmdline_info_T* cmdline = get_cmdline_info();

    if ((State & MODE_CMDLINE) == 0 || cmdline == NULL || cmdline->cmdbuff == NULL) {
        return NUL;
    }

    if (cmdline->cmdfirstc == NUL) {
        return '-';
    }

    return cmdline->cmdfirstc;
}

static char_u* upstream_runtime_search_cmdline_pattern(void) {
    int cmd_type = upstream_runtime_search_cmdline_type();
    cmdline_info_T* cmdline = get_cmdline_info();

    if (cmd_type != '/' && cmd_type != '?') {
        return NULL;
    }

    if (cmdline == NULL || cmdline->cmdbuff == NULL || cmdline->cmdlen == 0) {
        return NULL;
    }

    return vim_strnsave(cmdline->cmdbuff, cmdline->cmdlen);
}

static int upstream_runtime_incsearch_active(void) {
    int cmd_type;

    if (!p_is) {
        return FALSE;
    }

    cmd_type = upstream_runtime_search_cmdline_type();
    return cmd_type == '/' || cmd_type == '?';
}

static int upstream_runtime_range_contains_cursor(
    linenr_T start_row,
    colnr_T start_col,
    linenr_T end_row,
    colnr_T end_col,
    pos_T cursor
) {
    if (cursor.lnum < start_row || cursor.lnum > end_row) {
        return FALSE;
    }

    if (start_row == end_row) {
        return cursor.col >= start_col && cursor.col < end_col;
    }

    if (cursor.lnum == start_row) {
        return cursor.col >= start_col;
    }

    if (cursor.lnum == end_row) {
        return cursor.col < end_col;
    }

    return TRUE;
}

static int upstream_runtime_locate_incsearch_match(win_T* wp, pos_T* match_pos) {
    char_u* pattern = upstream_runtime_search_cmdline_pattern();
    size_t pattern_len = 0;
    int cmd_type = upstream_runtime_search_cmdline_type();
    int direction = cmd_type == '?' ? BACKWARD : FORWARD;
    int found = FAIL;
    win_T* saved_curwin = curwin;
    buf_T* saved_curbuf = curbuf;
    pos_T start_pos;

    if (wp == NULL || wp->w_buffer == NULL || match_pos == NULL || pattern == NULL) {
        vim_free(pattern);
        return FALSE;
    }

    pattern_len = STRLEN(pattern);
    start_pos = wp->w_cursor;
    curwin = wp;
    curbuf = wp->w_buffer;
    found = searchit(
        wp,
        wp->w_buffer,
        &start_pos,
        NULL,
        direction,
        pattern,
        pattern_len,
        1,
        SEARCH_KEEP,
        RE_SEARCH,
        NULL
    );
    curwin = saved_curwin;
    curbuf = saved_curbuf;
    vim_free(pattern);

    if (found == FAIL) {
        return FALSE;
    }

    *match_pos = start_pos;
    return TRUE;
}

static int upstream_runtime_prepare_search_regmatch(
    win_T* wp,
    int use_incsearch_pattern,
    regmmatch_T* regmatch
) {
    char_u* pattern = NULL;
    size_t pattern_len = 0;
    int compiled = FALSE;
    win_T* saved_curwin = curwin;
    buf_T* saved_curbuf = curbuf;

    if (wp == NULL || wp->w_buffer == NULL || regmatch == NULL) {
        return FALSE;
    }

    curwin = wp;
    curbuf = wp->w_buffer;

    if (use_incsearch_pattern) {
        pattern = upstream_runtime_search_cmdline_pattern();
        if (pattern != NULL) {
            pattern_len = STRLEN(pattern);
            compiled = search_regcomp(
                pattern,
                pattern_len,
                NULL,
                RE_SEARCH,
                RE_SEARCH,
                SEARCH_KEEP,
                regmatch
            ) == OK;
        }
        vim_free(pattern);
    } else {
        pattern = last_search_pattern();
        pattern_len = last_search_pattern_len();
        if (pattern != NULL && *pattern != NUL) {
            compiled = search_regcomp(
                pattern,
                pattern_len,
                NULL,
                RE_SEARCH,
                RE_SEARCH,
                SEARCH_KEEP,
                regmatch
            ) == OK;
        }
    }

    curwin = saved_curwin;
    curbuf = saved_curbuf;
    return compiled;
}

static const char* upstream_runtime_store_search_string(char_u* value) {
    size_t len;

    free(upstream_runtime_leased_search_string);
    upstream_runtime_leased_search_string = NULL;

    if (value == NULL || *value == NUL) {
        vim_free(value);
        return "";
    }

    len = STRLEN(value);
    upstream_runtime_leased_search_string = (char*)malloc(len + 1);
    if (upstream_runtime_leased_search_string == NULL) {
        vim_free(value);
        return "";
    }

    memcpy(upstream_runtime_leased_search_string, value, len + 1);
    vim_free(value);
    return upstream_runtime_leased_search_string;
}

const char* vim_bridge_get_search_pattern(void) {
    char_u* pat = last_search_pat();
    if (pat == NULL) return "";
    return (const char*)pat;
}

int vim_bridge_is_hlsearch_active(void) {
    return (p_hls && !no_hlsearch) ? 1 : 0;
}

int vim_bridge_get_search_direction(void) {
    return get_vim_var_nr(VV_SEARCHFORWARD) ? 1 : 0;
}

int vim_bridge_is_incsearch_active(void) {
    return upstream_runtime_incsearch_active() ? 1 : 0;
}

const char* vim_bridge_get_incsearch_pattern(void) {
    if (!upstream_runtime_incsearch_active()) {
        return "";
    }

    return upstream_runtime_store_search_string(
        upstream_runtime_search_cmdline_pattern()
    );
}

const char* vim_bridge_get_search_input_pattern(void) {
    return upstream_runtime_store_search_string(
        upstream_runtime_search_cmdline_pattern()
    );
}

vim_core_match_list_t vim_bridge_get_search_highlights(int window_id, int start_row, int end_row) {
    vim_core_match_list_t list;
    int incsearch_active = upstream_runtime_incsearch_active();
    regmmatch_T regmatch;
    pos_T cursor;
    pos_T incsearch_match;
    int has_incsearch_match = FALSE;
    CLEAR_FIELD(incsearch_match);
    list.count = 0;
    list.ranges = NULL;

    if (!incsearch_active && !vim_bridge_is_hlsearch_active()) return list;

    win_T* wp = win_id2wp(window_id);
    if (wp == NULL || wp->w_buffer == NULL) return list;
    buf_T* buf = wp->w_buffer;
    cursor = wp->w_cursor;
    if (incsearch_active) {
        has_incsearch_match = upstream_runtime_locate_incsearch_match(
            wp,
            &incsearch_match
        );
    }

    if (!upstream_runtime_prepare_search_regmatch(wp, incsearch_active, &regmatch)) {
        return list;
    }

    int capacity = 16;
    list.ranges = (vim_core_match_range_t*)malloc(sizeof(vim_core_match_range_t) * capacity);

    for (linenr_T r = start_row; r <= end_row; ++r) {
        colnr_T matchcol = 0;
        while (1) {
            int timed_out = 0;
            int nmatched = vim_regexec_multi(&regmatch, wp, buf, r, matchcol, &timed_out);
            if (nmatched == 0 || timed_out) break;

            long start_line = r + regmatch.startpos[0].lnum;
            if (start_line > end_row) break;

            if (start_line >= start_row) {
                if (list.count == capacity) {
                    capacity *= 2;
                    list.ranges = (vim_core_match_range_t*)realloc(list.ranges, sizeof(vim_core_match_range_t) * capacity);
                }
                list.ranges[list.count].start_row = start_line;
                list.ranges[list.count].start_col = regmatch.startpos[0].col;
                list.ranges[list.count].end_row = r + regmatch.endpos[0].lnum;
                list.ranges[list.count].end_col = regmatch.endpos[0].col;
                list.ranges[list.count].match_type =
                    (incsearch_active && has_incsearch_match
                        && list.ranges[list.count].start_row == incsearch_match.lnum
                        && list.ranges[list.count].start_col == incsearch_match.col)
                    || upstream_runtime_range_contains_cursor(
                        list.ranges[list.count].start_row,
                        list.ranges[list.count].start_col,
                        list.ranges[list.count].end_row,
                        list.ranges[list.count].end_col,
                        cursor
                    )
                        ? VIM_CORE_MATCH_CURSEARCH
                        : (incsearch_active
                            ? VIM_CORE_MATCH_INCSEARCH
                            : VIM_CORE_MATCH_REGULAR);
                list.count++;
            }

            if (regmatch.endpos[0].lnum == 0 && regmatch.endpos[0].col <= regmatch.startpos[0].col) {
                char_u *ml = ml_get_buf(buf, r, FALSE) + regmatch.startpos[0].col;
                if (*ml == NUL) break;
                matchcol = regmatch.startpos[0].col + (*mb_ptr2len)(ml);
            } else if (regmatch.startpos[0].lnum == 0) {
                matchcol = regmatch.endpos[0].col;
            } else {
                break;
            }
        }
    }

    vim_regfree(regmatch.regprog);
    return list;
}

void vim_bridge_free_match_list(vim_core_match_list_t list) {
    if (list.ranges != NULL) {
        free(list.ranges);
    }
}

vim_core_cursor_match_info_t vim_bridge_get_cursor_match_info(int window_id, int row, int col, int max_count, int timeout_ms) {
    vim_core_cursor_match_info_t info;
    info.is_on_match = 0;
    info.current_match_index = 0;
    info.total_matches = 0;
    info.status = VIM_CORE_MATCH_COUNT_CALCULATED;

    if (!vim_bridge_is_hlsearch_active()) return info;

    win_T* wp = win_id2wp(window_id);
    if (wp == NULL) return info;

    // Temporarily set cursor
    win_T* old_curwin = curwin;
    pos_T old_cursor = wp->w_cursor;

    curwin = wp;
    curbuf = wp->w_buffer;
    wp->w_cursor.lnum = row;
    wp->w_cursor.col = col;

    char expr[512];
    snprintf(expr, sizeof(expr),
        "get(searchcount({'maxcount':%d,'timeout':%d}),'exact_match',0).','.get(searchcount({'maxcount':%d,'timeout':%d}),'current',0).','.get(searchcount({'maxcount':%d,'timeout':%d}),'total',0).','.get(searchcount({'maxcount':%d,'timeout':%d}),'incomplete',0)",
        max_count, timeout_ms, max_count, timeout_ms, max_count, timeout_ms, max_count, timeout_ms);

    char_u* res = eval_to_string((char_u*)expr, FALSE, FALSE);
    if (res != NULL) {
        int exact = 0, current = 0, total = 0, incomplete = 0;
        if (sscanf((char*)res, "%d,%d,%d,%d", &exact, &current, &total, &incomplete) == 4) {
            info.is_on_match = exact;
            info.current_match_index = current;
            info.total_matches = total;
            if (incomplete == 1) {
                info.status = VIM_CORE_MATCH_COUNT_TIMED_OUT;
            } else if (incomplete == 2) {
                info.status = VIM_CORE_MATCH_COUNT_MAX_REACHED;
            }
        }
        vim_free(res);
    }

    // Restore
    wp->w_cursor = old_cursor;
    curwin = old_curwin;
    curbuf = curwin->w_buffer;

    return info;
}
