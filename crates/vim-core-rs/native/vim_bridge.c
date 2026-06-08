#include "vim_bridge.h"
#include "upstream_runtime.h"

#include <stdlib.h>
#include <stdio.h>

struct vim_bridge_state {
    upstream_runtime_session_t* runtime;
};

void vim_bridge_set_debug_log_path(const char* path, uintptr_t path_len) {
    upstream_runtime_set_debug_log_path(path, path_len);
}

vim_bridge_state_t* vim_bridge_state_new(
    const char* initial_text,
    uintptr_t text_len
) {
    vim_bridge_state_t* state = (vim_bridge_state_t*)calloc(1U, sizeof(vim_bridge_state_t));
    if (state == NULL) {
        return NULL;
    }

    state->runtime = upstream_runtime_session_new(initial_text, text_len);
    if (state->runtime == NULL) {
        free(state);
        return NULL;
    }

    return state;
}

void vim_bridge_state_free(vim_bridge_state_t* state) {
    if (state == NULL) {
        return;
    }

    upstream_runtime_session_free(state->runtime);
    free(state);
}

vim_core_snapshot_t vim_bridge_snapshot(const vim_bridge_state_t* state) {
    return upstream_runtime_snapshot(state != NULL ? state->runtime : NULL);
}

vim_core_snapshot_t vim_bridge_light_snapshot(const vim_bridge_state_t* state) {
    return upstream_runtime_light_snapshot(state != NULL ? state->runtime : NULL);
}

vim_core_command_result_t vim_bridge_execute_normal_command(
    vim_bridge_state_t* state,
    const char* command,
    uintptr_t command_len
) {
    if (state == NULL) {
        return upstream_runtime_execute_normal_command(NULL, command, command_len);
    }

    return upstream_runtime_execute_normal_command(state->runtime, command, command_len);
}

vim_core_command_result_t vim_bridge_execute_ex_command(
    vim_bridge_state_t* state,
    const char* command,
    uintptr_t command_len
) {
    if (state == NULL) {
        return upstream_runtime_execute_ex_command(NULL, command, command_len);
    }

    return upstream_runtime_execute_ex_command(state->runtime, command, command_len);
}

vim_host_action_t vim_bridge_take_pending_host_action(vim_bridge_state_t* state) {
    return upstream_runtime_take_pending_host_action(state != NULL ? state->runtime : NULL);
}

vim_core_event_t vim_bridge_take_pending_event(vim_bridge_state_t* state) {
    return upstream_runtime_take_pending_event(state != NULL ? state->runtime : NULL);
}

vim_runtime_backend_identity_t vim_bridge_backend_identity(
    const vim_bridge_state_t* state
) {
    return upstream_runtime_backend_identity(state != NULL ? state->runtime : NULL);
}

vim_core_register_get_result_t vim_bridge_get_register(
    const vim_bridge_state_t* state,
    char regname
) {
    static const vim_core_register_get_result_t empty_result = {0};
    if (state == NULL) {
        return empty_result;
    }
    return upstream_runtime_get_register(state->runtime, regname);
}

void vim_bridge_set_register(vim_bridge_state_t* state, char regname, const char* text, uintptr_t text_len) {
    if (state == NULL) {
        return;
    }
    upstream_runtime_set_register(state->runtime, regname, text, text_len);
}

void vim_bridge_free_string(char* ptr) {
    free(ptr);
}

vim_core_option_get_result_t vim_bridge_get_option(
    const vim_bridge_state_t* state,
    const char* name,
    vim_core_option_scope_t scope
) {
    
    if (state == NULL) {
        return upstream_runtime_get_option(NULL, name, scope);
    }

    return upstream_runtime_get_option(state->runtime, name, scope);
}

vim_core_option_set_result_t vim_bridge_set_option_number(
    vim_bridge_state_t* state,
    const char* name,
    int64_t value,
    vim_core_option_scope_t scope
) {
    
    if (state == NULL) {
        return upstream_runtime_set_option_number(NULL, name, value, scope);
    }

    return upstream_runtime_set_option_number(state->runtime, name, value, scope);
}

vim_core_option_set_result_t vim_bridge_set_option_string(
    vim_bridge_state_t* state,
    const char* name,
    const char* value,
    vim_core_option_scope_t scope
) {
    
    if (state == NULL) {
        return upstream_runtime_set_option_string(NULL, name, value, scope);
    }

    return upstream_runtime_set_option_string(state->runtime, name, value, scope);
}

vim_core_status_t vim_bridge_commit_buffer_update(
    vim_bridge_state_t* state,
    const vim_core_buffer_commit_t* commit
) {
    if (state == NULL || commit == NULL) return VIM_CORE_STATUS_SESSION_ERROR;
    return upstream_runtime_commit_buffer_update(state->runtime, commit);
}

void vim_bridge_set_screen_size(vim_bridge_state_t* state, int rows, int cols) {
    if (state == NULL) return;
    upstream_runtime_set_screen_size(state->runtime, rows, cols);
}

vim_core_status_t vim_bridge_switch_to_buffer(vim_bridge_state_t* state, int buf_id) {
    if (state == NULL) return VIM_CORE_STATUS_SESSION_ERROR;
    return upstream_runtime_switch_to_buffer(state->runtime, buf_id);
}

vim_core_status_t vim_bridge_switch_to_window(vim_bridge_state_t* state, int win_id) {
    if (state == NULL) return VIM_CORE_STATUS_SESSION_ERROR;
    return upstream_runtime_switch_to_window(state->runtime, win_id);
}

char* vim_bridge_get_buffer_text(const vim_bridge_state_t* state, int buf_id) {
    if (state == NULL) return NULL;
    return upstream_runtime_get_buffer_text(state->runtime, buf_id);
}

vim_core_buffer_line_range_t vim_bridge_get_buffer_line_range(
    const vim_bridge_state_t* state,
    int buf_id,
    uintptr_t start_row,
    uintptr_t line_count
) {
    if (state == NULL) {
        vim_core_buffer_line_range_t empty = {0};
        return empty;
    }
    return upstream_runtime_get_buffer_line_range(state->runtime, buf_id, start_row, line_count);
}

void vim_bridge_free_buffer_line_range(vim_core_buffer_line_range_t range) {
    if (range.lines == NULL) return;
    for (uintptr_t i = 0; i < range.line_count; ++i) {
        if (range.lines[i].text_ptr != NULL) {
            free((void*)range.lines[i].text_ptr);
        }
    }
    free(range.lines);
}

vim_core_status_t vim_bridge_set_buffer_text(
    vim_bridge_state_t* state,
    int buf_id,
    const char* text,
    uintptr_t text_len
) {
    if (state == NULL) return VIM_CORE_STATUS_SESSION_ERROR;
    return upstream_runtime_set_buffer_text(state->runtime, buf_id, text, text_len);
}

vim_core_status_t vim_bridge_set_buffer_name(
    vim_bridge_state_t* state,
    int buf_id,
    const char* name,
    uintptr_t name_len
) {
    if (state == NULL) return VIM_CORE_STATUS_SESSION_ERROR;
    return upstream_runtime_set_buffer_name(state->runtime, buf_id, name, name_len);
}

vim_core_status_t vim_bridge_set_buffer_dirty(
    vim_bridge_state_t* state,
    int buf_id,
    bool dirty
) {
    if (state == NULL) return VIM_CORE_STATUS_SESSION_ERROR;
    return upstream_runtime_set_buffer_dirty(state->runtime, buf_id, dirty);
}

vim_core_pending_input_t vim_bridge_get_pending_input(const vim_bridge_state_t* state) {
    return upstream_runtime_get_pending_input(state != NULL ? state->runtime : NULL);
}

bool vim_bridge_get_mark(
    const vim_bridge_state_t* state,
    char mark_name,
    vim_core_mark_position_t* out_mark
) {
    return upstream_runtime_get_mark(state != NULL ? state->runtime : NULL, mark_name, out_mark);
}

vim_core_status_t vim_bridge_set_mark(
    vim_bridge_state_t* state,
    char mark_name,
    int buf_id,
    uintptr_t row,
    uintptr_t col
) {
    if (state == NULL) {
        return VIM_CORE_STATUS_SESSION_ERROR;
    }
    return upstream_runtime_set_mark(state->runtime, mark_name, buf_id, row, col);
}

vim_core_jumplist_t vim_bridge_get_jumplist(const vim_bridge_state_t* state) {
    return upstream_runtime_get_jumplist(state != NULL ? state->runtime : NULL);
}

void vim_bridge_free_jumplist(vim_core_jumplist_t jumplist) {
    upstream_runtime_free_jumplist(jumplist);
}

int vim_bridge_get_undo_tree(const vim_bridge_state_t* state, int buf_id, vim_core_undo_tree_t* out_tree) {
    if (state == NULL || state->runtime == NULL) return -1;
    return upstream_runtime_get_undo_tree(buf_id, out_tree);
}

void vim_bridge_free_undo_tree(vim_core_undo_tree_t tree) {
    upstream_runtime_free_undo_tree(tree);
}

int vim_bridge_undo_jump(vim_bridge_state_t* state, int buf_id, long seq) {
    if (state == NULL || state->runtime == NULL) return -1;
    return upstream_runtime_undo_jump(buf_id, seq);
}

int vim_bridge_get_line_syntax(const vim_bridge_state_t* state, int win_id, long lnum, int* out_ids, int max_cols) {
    if (state == NULL || state->runtime == NULL) return -1;
    return upstream_runtime_get_line_syntax(win_id, lnum, out_ids, max_cols);
}

const char* vim_bridge_get_syntax_name(const vim_bridge_state_t* state, int syn_id) {
    if (state == NULL || state->runtime == NULL) return NULL;
    return upstream_runtime_get_syntax_name(syn_id);
}

char* vim_bridge_eval_string(vim_bridge_state_t* state, const char* expr) {
    if (state == NULL || state->runtime == NULL || expr == NULL) return NULL;
    return upstream_runtime_eval_string(state->runtime, expr);
}

void vim_bridge_submit_input_response(
    vim_bridge_state_t* state,
    const char* value,
    uintptr_t value_len,
    bool cancelled
) {
    if (state == NULL || state->runtime == NULL) return;
    upstream_runtime_submit_input_response(state->runtime, value, value_len, cancelled);
}

int vim_core_bridge_embedded_mode_active(void) {
    return upstream_runtime_embedded_mode_active();
}

void vim_core_bridge_enqueue_message_event(
    const char* text,
    uintptr_t text_len,
    vim_core_message_severity_t severity,
    vim_core_message_category_t category
) {
    upstream_runtime_enqueue_message_event(text, text_len, severity, category);
}

void vim_core_bridge_enqueue_input_request(
    const char* prompt,
    uintptr_t prompt_len,
    vim_core_input_request_kind_t kind
) {
    upstream_runtime_enqueue_input_request(prompt, prompt_len, kind);
}

int vim_core_bridge_take_input_response(
    vim_core_input_request_kind_t kind,
    char** value_ptr,
    uintptr_t* value_len,
    bool* cancelled
) {
    return upstream_runtime_take_input_response(kind, value_ptr, value_len, cancelled);
}

void vim_core_bridge_free_input_response(char* value) {
    free(value);
}

void vim_core_bridge_enqueue_pager_prompt_event(vim_core_pager_prompt_kind_t kind) {
    upstream_runtime_enqueue_pager_prompt_event(kind);
}

void vim_core_bridge_enqueue_bell(void) {
    upstream_runtime_enqueue_bell_for_active_session();
}

void vim_bridge_free_pum_info(vim_core_pum_info_t* pum) {
    if (pum == NULL) return;

    

    for (size_t i = 0; i < pum->item_count; i++) {
        /* 各候補の文字列フィールドを個別に解放（NULLチェック付き） */
        if (pum->items[i].word) free((void*)pum->items[i].word);
        if (pum->items[i].abbr) free((void*)pum->items[i].abbr);
        if (pum->items[i].menu) free((void*)pum->items[i].menu);
        if (pum->items[i].kind) free((void*)pum->items[i].kind);
        if (pum->items[i].info) free((void*)pum->items[i].info);
    }

    /* 候補配列を解放 */
    if (pum->items) free(pum->items);

    /* 構造体自体を解放 */
    free(pum);
}
