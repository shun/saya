#ifndef UPSTREAM_RUNTIME_H
#define UPSTREAM_RUNTIME_H

#include "vim_bridge.h"

typedef struct upstream_runtime_session upstream_runtime_session_t;

upstream_runtime_session_t* upstream_runtime_session_new(
    const char* initial_text,
    uintptr_t text_len
);

void upstream_runtime_set_debug_log_path(const char* path, uintptr_t path_len);

void upstream_runtime_session_free(upstream_runtime_session_t* session);

vim_core_command_result_t upstream_runtime_execute_normal_command(
    upstream_runtime_session_t* session,
    const char* command,
    uintptr_t command_len
);

vim_core_command_result_t upstream_runtime_execute_ex_command(
    upstream_runtime_session_t* session,
    const char* command,
    uintptr_t command_len
);

vim_core_snapshot_t upstream_runtime_snapshot(
    const upstream_runtime_session_t* session
);
vim_core_snapshot_t upstream_runtime_light_snapshot(
    const upstream_runtime_session_t* session
);

vim_host_action_t upstream_runtime_take_pending_host_action(
    upstream_runtime_session_t* session
);
vim_core_event_t upstream_runtime_take_pending_event(
    upstream_runtime_session_t* session
);

vim_runtime_backend_identity_t upstream_runtime_backend_identity(
    const upstream_runtime_session_t* session
);

vim_core_register_get_result_t upstream_runtime_get_register(
    const upstream_runtime_session_t* session,
    char regname
);
void upstream_runtime_set_register(upstream_runtime_session_t* session, char regname, const char* text, uintptr_t text_len);
vim_core_option_get_result_t upstream_runtime_get_option(
    const upstream_runtime_session_t* session,
    const char* name,
    vim_core_option_scope_t scope
);
vim_core_option_set_result_t upstream_runtime_set_option_number(
    upstream_runtime_session_t* session,
    const char* name,
    int64_t value,
    vim_core_option_scope_t scope
);
vim_core_option_set_result_t upstream_runtime_set_option_string(
    upstream_runtime_session_t* session,
    const char* name,
    const char* value,
    vim_core_option_scope_t scope
);

vim_core_status_t upstream_runtime_commit_buffer_update(
    upstream_runtime_session_t* session,
    const vim_core_buffer_commit_t* commit
);

int upstream_runtime_intercept_command(void *eap);

void upstream_runtime_set_screen_size(upstream_runtime_session_t* session, int rows, int cols);
vim_core_status_t upstream_runtime_switch_to_buffer(upstream_runtime_session_t* session, int buf_id);
vim_core_status_t upstream_runtime_switch_to_window(upstream_runtime_session_t* session, int win_id);
char* upstream_runtime_get_buffer_text(const upstream_runtime_session_t* session, int buf_id);
vim_core_buffer_line_range_t upstream_runtime_get_buffer_line_range(
    const upstream_runtime_session_t* session,
    int buf_id,
    uintptr_t start_row,
    uintptr_t line_count
);
vim_core_status_t upstream_runtime_set_buffer_text(
    upstream_runtime_session_t* session,
    int buf_id,
    const char* text,
    uintptr_t text_len
);
vim_core_status_t upstream_runtime_set_buffer_name(
    upstream_runtime_session_t* session,
    int buf_id,
    const char* name,
    uintptr_t name_len
);
vim_core_status_t upstream_runtime_set_buffer_dirty(
    upstream_runtime_session_t* session,
    int buf_id,
    bool dirty
);
vim_core_pending_input_t upstream_runtime_get_pending_input(
    const upstream_runtime_session_t* session
);
bool upstream_runtime_get_mark(
    const upstream_runtime_session_t* session,
    char mark_name,
    vim_core_mark_position_t* out_mark
);
vim_core_status_t upstream_runtime_set_mark(
    upstream_runtime_session_t* session,
    char mark_name,
    int buf_id,
    uintptr_t row,
    uintptr_t col
);
vim_core_jumplist_t upstream_runtime_get_jumplist(
    const upstream_runtime_session_t* session
);
void upstream_runtime_free_jumplist(vim_core_jumplist_t jumplist);

int upstream_runtime_get_undo_tree(int buf_id, vim_core_undo_tree_t* out_tree);
void upstream_runtime_free_undo_tree(vim_core_undo_tree_t tree);
int upstream_runtime_undo_jump(int buf_id, long seq);

int upstream_runtime_get_line_syntax(int win_id, long lnum, int* out_ids, int max_cols);
const char* upstream_runtime_get_syntax_name(int syn_id);

char* upstream_runtime_eval_string(upstream_runtime_session_t* session, const char* expr);
void upstream_runtime_submit_input_response(
    upstream_runtime_session_t* session,
    const char* value,
    uintptr_t value_len,
    bool cancelled
);
int upstream_runtime_embedded_mode_active(void);
void upstream_runtime_enqueue_message_event(
    const char* text,
    uintptr_t text_len,
    vim_core_message_severity_t severity,
    vim_core_message_category_t category
);
void upstream_runtime_enqueue_input_request(
    const char* prompt,
    uintptr_t prompt_len,
    vim_core_input_request_kind_t kind
);
int upstream_runtime_take_input_response(
    vim_core_input_request_kind_t kind,
    char** value_ptr,
    uintptr_t* value_len,
    bool* cancelled
);
void upstream_runtime_enqueue_pager_prompt_event(vim_core_pager_prompt_kind_t kind);
void upstream_runtime_enqueue_bell_for_active_session(void);

#endif
