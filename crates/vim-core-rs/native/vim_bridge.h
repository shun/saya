#ifndef VIM_BRIDGE_H
#define VIM_BRIDGE_H

#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

typedef struct vim_bridge_state vim_bridge_state_t;

typedef enum vim_core_status {
    VIM_CORE_STATUS_OK = 0,
    VIM_CORE_STATUS_COMMAND_ERROR = 1,
    VIM_CORE_STATUS_SESSION_ERROR = 2
} vim_core_status_t;

typedef enum vim_core_mode {
    VIM_CORE_MODE_NORMAL = 0,
    VIM_CORE_MODE_INSERT = 1,
    VIM_CORE_MODE_VISUAL = 2,
    VIM_CORE_MODE_VISUAL_LINE = 3,
    VIM_CORE_MODE_VISUAL_BLOCK = 4,
    VIM_CORE_MODE_REPLACE = 5,
    VIM_CORE_MODE_SELECT = 6,
    VIM_CORE_MODE_SELECT_LINE = 7,
    VIM_CORE_MODE_SELECT_BLOCK = 8,
    VIM_CORE_MODE_COMMAND_LINE = 9,
    VIM_CORE_MODE_OPERATOR_PENDING = 10
} vim_core_mode_t;

typedef enum vim_core_pending_input {
    VIM_CORE_PENDING_INPUT_NONE = 0,
    VIM_CORE_PENDING_INPUT_CHAR = 1,
    VIM_CORE_PENDING_INPUT_REPLACE = 2,
    VIM_CORE_PENDING_INPUT_MARK_SET = 3,
    VIM_CORE_PENDING_INPUT_MARK_JUMP = 4,
    VIM_CORE_PENDING_INPUT_REGISTER = 5
} vim_core_pending_input_t;

typedef enum vim_core_command_outcome_kind {
    VIM_CORE_COMMAND_OUTCOME_NO_CHANGE = 0,
    VIM_CORE_COMMAND_OUTCOME_BUFFER_CHANGED = 1,
    VIM_CORE_COMMAND_OUTCOME_CURSOR_CHANGED = 2,
    VIM_CORE_COMMAND_OUTCOME_MODE_CHANGED = 3,
    VIM_CORE_COMMAND_OUTCOME_HOST_ACTION_QUEUED = 4
} vim_core_command_outcome_kind_t;

typedef enum vim_core_input_request_kind {
    VIM_CORE_INPUT_REQUEST_COMMAND_LINE = 0,
    VIM_CORE_INPUT_REQUEST_CONFIRMATION = 1,
    VIM_CORE_INPUT_REQUEST_SECRET = 2
} vim_core_input_request_kind_t;

typedef enum vim_runtime_backend_identity {
    VIM_CORE_BACKEND_IDENTITY_BRIDGE_STUB = 0,
    VIM_CORE_BACKEND_IDENTITY_UPSTREAM_RUNTIME = 1
} vim_runtime_backend_identity_t;

typedef enum vim_core_option_type {
    VIM_CORE_OPTION_TYPE_BOOL = 0,
    VIM_CORE_OPTION_TYPE_NUMBER = 1,
    VIM_CORE_OPTION_TYPE_STRING = 2,
    VIM_CORE_OPTION_TYPE_UNKNOWN = 3
} vim_core_option_type_t;

typedef enum vim_core_option_scope {
    VIM_CORE_OPTION_SCOPE_DEFAULT = 0,
    VIM_CORE_OPTION_SCOPE_GLOBAL = 1,
    VIM_CORE_OPTION_SCOPE_LOCAL = 2
} vim_core_option_scope_t;

enum {
    VIM_HOST_ACTION_NONE = 0,
    VIM_HOST_ACTION_WRITE = 1,
    VIM_HOST_ACTION_QUIT = 2,
    VIM_HOST_ACTION_REDRAW = 3,
    VIM_HOST_ACTION_REQUEST_INPUT = 4,
    VIM_HOST_ACTION_BELL = 5,
    VIM_HOST_ACTION_BUF_ADD = 6,
    VIM_HOST_ACTION_WIN_NEW = 7,
    VIM_HOST_ACTION_LAYOUT_CHANGED = 8,
    VIM_HOST_ACTION_JOB_START = 9,
    VIM_HOST_ACTION_JOB_STOP = 10
};

typedef struct vim_core_job_start_request {
    int job_id;
    char* argv_buf;
    uintptr_t argv_len;
    char* cwd;
    int vfd_in;
    int vfd_out;
    int vfd_err;
} vim_core_job_start_request_t;

/* --- VFS POD 契約 (Task 4.1) --- */

typedef enum vim_core_vfs_operation_kind {
    VIM_CORE_VFS_OPERATION_NONE = 0,
    VIM_CORE_VFS_OPERATION_RESOLVE = 1,
    VIM_CORE_VFS_OPERATION_EXISTS = 2,
    VIM_CORE_VFS_OPERATION_LOAD = 3,
    VIM_CORE_VFS_OPERATION_SAVE = 4
} vim_core_vfs_operation_kind_t;

typedef enum vim_core_buffer_source_kind {
    VIM_CORE_BUFFER_SOURCE_LOCAL = 0,
    VIM_CORE_BUFFER_SOURCE_VFS = 1
} vim_core_buffer_source_kind_t;

typedef struct vim_core_buffer_commit {
    int target_buf_id;
    bool replace_text;
    const char* text_ptr;
    uintptr_t text_len;
    const char* display_name_ptr;
    uintptr_t display_name_len;
    bool clear_dirty;
} vim_core_buffer_commit_t;

/* --- Buffer info with VFS metadata (Task 4.2) --- */

typedef struct vim_core_buffer_info {
    int id;
    const char* name_ptr;
    uintptr_t name_len;
    uint64_t source_revision;
    bool dirty;
    bool is_active;
    /* VFS metadata fields */
    uint32_t source_kind;      /* vim_core_buffer_source_kind_t */
    const char* document_id_ptr;
    uintptr_t document_id_len;
    uint32_t pending_vfs_operation; /* vim_core_vfs_operation_kind_t */
    bool deferred_close;
    const char* last_vfs_error_ptr;
    uintptr_t last_vfs_error_len;
} vim_core_buffer_info_t;

typedef struct vim_core_window_info {
    int id;
    int buf_id;
    uintptr_t row;
    uintptr_t col;
    uintptr_t width;
    uintptr_t height;
    uintptr_t topline;
    uintptr_t botline;
    uintptr_t leftcol;
    uintptr_t skipcol;
    uintptr_t cursor_row;
    uintptr_t cursor_col;
    bool is_active;
} vim_core_window_info_t;

typedef struct vim_core_mark_position {
    bool is_set;
    int buf_id;
    uintptr_t row;
    uintptr_t col;
} vim_core_mark_position_t;

typedef struct vim_core_jumplist_entry {
    int buf_id;
    uintptr_t row;
    uintptr_t col;
} vim_core_jumplist_entry_t;

typedef struct vim_core_jumplist {
    vim_core_jumplist_entry_t* entries;
    uintptr_t entry_count;
    uintptr_t current_index;
    bool has_current_index;
} vim_core_jumplist_t;

typedef struct vim_core_pum_item {
    const char* word;
    const char* abbr;
    const char* menu;
    const char* kind;
    const char* info;
} vim_core_pum_item_t;

typedef struct vim_core_pum_info {
    int row;
    int col;
    int width;
    int height;
    int selected_index;
    vim_core_pum_item_t* items;
    size_t item_count;
} vim_core_pum_info_t;

typedef enum vim_core_message_severity {
    VIM_CORE_MESSAGE_SEVERITY_INFO = 0,
    VIM_CORE_MESSAGE_SEVERITY_WARNING = 1,
    VIM_CORE_MESSAGE_SEVERITY_ERROR = 2
} vim_core_message_severity_t;

typedef enum vim_core_message_category {
    VIM_CORE_MESSAGE_CATEGORY_USER_VISIBLE = 0,
    VIM_CORE_MESSAGE_CATEGORY_COMMAND_FEEDBACK = 1
} vim_core_message_category_t;

typedef enum vim_core_event_kind {
    VIM_CORE_EVENT_NONE = 0,
    VIM_CORE_EVENT_MESSAGE = 1,
    VIM_CORE_EVENT_BELL = 2,
    VIM_CORE_EVENT_REDRAW = 3,
    VIM_CORE_EVENT_BUF_ADD = 4,
    VIM_CORE_EVENT_WIN_NEW = 5,
    VIM_CORE_EVENT_LAYOUT_CHANGED = 6,
    VIM_CORE_EVENT_PAGER_PROMPT = 7
} vim_core_event_kind_t;

typedef enum vim_core_pager_prompt_kind {
    VIM_CORE_PAGER_PROMPT_MORE = 0,
    VIM_CORE_PAGER_PROMPT_HIT_RETURN = 1
} vim_core_pager_prompt_kind_t;

typedef struct vim_core_event {
    uint32_t kind;
    vim_core_message_severity_t message_severity;
    vim_core_message_category_t message_category;
    vim_core_pager_prompt_kind_t pager_prompt_kind;
    const char* text_ptr;
    uintptr_t text_len;
    bool full;
    bool clear_before_draw;
    int buf_id;
    int win_id;
} vim_core_event_t;

typedef struct vim_core_snapshot {
    const char* text_ptr;
    uintptr_t text_len;
    uint64_t revision;
    bool dirty;
    vim_core_mode_t mode;
    vim_core_pending_input_t pending_input;
    uintptr_t cursor_row;
    uintptr_t cursor_col;
    uintptr_t pending_host_actions;

    vim_core_buffer_info_t* buffers;
    uintptr_t buffer_count;
    vim_core_window_info_t* windows;
    uintptr_t window_count;
    vim_core_pum_info_t* pum; // NULLの場合はポップアップメニュー非表示
} vim_core_snapshot_t;

typedef struct vim_core_buffer_line {
    const char* text_ptr;
    uintptr_t text_len;
} vim_core_buffer_line_t;

typedef struct vim_core_buffer_line_range {
    bool found;
    int buffer_id;
    uint64_t source_revision;
    uintptr_t start_row;
    uintptr_t line_count;
    uintptr_t total_line_count;
    vim_core_buffer_line_t* lines;
} vim_core_buffer_line_range_t;

typedef struct vim_host_action {
    uint32_t kind;
    uint64_t issued_after_revision;
    bool force;
    bool full;
    bool clear_before_draw;
    uint64_t correlation_id;
    vim_core_input_request_kind_t input_kind;
    const char* input_prompt;
    const char* primary_text_ptr;
    uintptr_t primary_text_len;
    
    // Event payload fields for buffer/window events
    int event_buf_id;
    int event_win_id;

    // Payload for VIM_HOST_ACTION_JOB_START
    vim_core_job_start_request_t job_start_request;

    // Legacy/Internal fields for upstream_runtime
    bool quit_requested;
    bool quit_force;
    bool redraw_force;
} vim_host_action_t;

typedef struct vim_core_command_result {
    vim_core_status_t status;
    uint32_t reason_code;
    vim_core_command_outcome_kind_t outcome;
    vim_core_snapshot_t snapshot;
} vim_core_command_result_t;

typedef struct vim_core_option_get_result {
    vim_core_status_t status;
    vim_core_option_type_t option_type;
    int64_t number_value;
    const char* string_value_ptr;
    uintptr_t string_value_len;
} vim_core_option_get_result_t;

typedef struct vim_core_option_set_result {
    vim_core_status_t status;
    const char* error_message_ptr;
    uintptr_t error_message_len;
} vim_core_option_set_result_t;

typedef struct vim_core_register_get_result {
    const char* payload_ptr;
    uintptr_t payload_len;
} vim_core_register_get_result_t;

vim_bridge_state_t* vim_bridge_state_new(
    const char* initial_text,
    uintptr_t text_len
);

void vim_bridge_set_debug_log_path(const char* path, uintptr_t path_len);

void vim_bridge_state_free(vim_bridge_state_t* state);

vim_core_snapshot_t vim_bridge_snapshot(const vim_bridge_state_t* state);
vim_core_snapshot_t vim_bridge_light_snapshot(const vim_bridge_state_t* state);

vim_core_command_result_t vim_bridge_execute_normal_command(
    vim_bridge_state_t* state,
    const char* command,
    uintptr_t command_len
);

vim_core_command_result_t vim_bridge_execute_ex_command(
    vim_bridge_state_t* state,
    const char* command,
    uintptr_t command_len
);

vim_host_action_t vim_bridge_take_pending_host_action(vim_bridge_state_t* state);
vim_core_event_t vim_bridge_take_pending_event(vim_bridge_state_t* state);

vim_runtime_backend_identity_t vim_bridge_backend_identity(
    const vim_bridge_state_t* state
);

vim_core_register_get_result_t vim_bridge_get_register(
    const vim_bridge_state_t* state,
    char regname
);
void vim_bridge_set_register(vim_bridge_state_t* state, char regname, const char* text, uintptr_t text_len);
void vim_bridge_free_string(char* ptr);
vim_core_option_get_result_t vim_bridge_get_option(
    const vim_bridge_state_t* state,
    const char* name,
    vim_core_option_scope_t scope
);
vim_core_option_set_result_t vim_bridge_set_option_number(
    vim_bridge_state_t* state,
    const char* name,
    int64_t value,
    vim_core_option_scope_t scope
);
vim_core_option_set_result_t vim_bridge_set_option_string(
    vim_bridge_state_t* state,
    const char* name,
    const char* value,
    vim_core_option_scope_t scope
);

vim_core_status_t vim_bridge_commit_buffer_update(
    vim_bridge_state_t* state,
    const vim_core_buffer_commit_t* commit
);

void vim_bridge_register_ex_callback(const char* name, int (*callback)(void* eap));

void vim_bridge_set_screen_size(vim_bridge_state_t* state, int rows, int cols);
vim_core_status_t vim_bridge_switch_to_buffer(vim_bridge_state_t* state, int buf_id);
vim_core_status_t vim_bridge_switch_to_window(vim_bridge_state_t* state, int win_id);
char* vim_bridge_get_buffer_text(const vim_bridge_state_t* state, int buf_id);
vim_core_buffer_line_range_t vim_bridge_get_buffer_line_range(
    const vim_bridge_state_t* state,
    int buf_id,
    uintptr_t start_row,
    uintptr_t line_count
);
void vim_bridge_free_buffer_line_range(vim_core_buffer_line_range_t range);
vim_core_status_t vim_bridge_set_buffer_text(
    vim_bridge_state_t* state,
    int buf_id,
    const char* text,
    uintptr_t text_len
);
vim_core_status_t vim_bridge_set_buffer_name(
    vim_bridge_state_t* state,
    int buf_id,
    const char* name,
    uintptr_t name_len
);
vim_core_status_t vim_bridge_set_buffer_dirty(
    vim_bridge_state_t* state,
    int buf_id,
    bool dirty
);
vim_core_pending_input_t vim_bridge_get_pending_input(const vim_bridge_state_t* state);
bool vim_bridge_get_mark(
    const vim_bridge_state_t* state,
    char mark_name,
    vim_core_mark_position_t* out_mark
);
vim_core_status_t vim_bridge_set_mark(
    vim_bridge_state_t* state,
    char mark_name,
    int buf_id,
    uintptr_t row,
    uintptr_t col
);
vim_core_jumplist_t vim_bridge_get_jumplist(const vim_bridge_state_t* state);
void vim_bridge_free_jumplist(vim_core_jumplist_t jumplist);

typedef struct vim_core_undo_node {
    long seq;
    long time;
    long save_nr;
    long prev_seq;
    long next_seq;
    long alt_next_seq;
    long alt_prev_seq;
    bool is_newhead;
    bool is_curhead;
} vim_core_undo_node_t;

typedef struct vim_core_undo_tree {
    vim_core_undo_node_t* nodes;
    uintptr_t length;
    bool synced;
    long seq_last;
    long save_last;
    long seq_cur;
    long time_cur;
    long save_cur;
} vim_core_undo_tree_t;

int vim_bridge_get_undo_tree(const vim_bridge_state_t* state, int buf_id, vim_core_undo_tree_t* out_tree);
void vim_bridge_free_undo_tree(vim_core_undo_tree_t tree);
int vim_bridge_undo_jump(vim_bridge_state_t* state, int buf_id, long seq);

int vim_bridge_get_line_syntax(const vim_bridge_state_t* state, int win_id, long lnum, int* out_ids, int max_cols);
const char* vim_bridge_get_syntax_name(const vim_bridge_state_t* state, int syn_id);

// Vimscript式を評価し、結果の文字列を返す。結果はvim_bridge_free_stringで解放する。
// 評価に失敗した場合はNULLを返す。
char* vim_bridge_eval_string(vim_bridge_state_t* state, const char* expr);
void vim_bridge_submit_input_response(
    vim_bridge_state_t* state,
    const char* value,
    uintptr_t value_len,
    bool cancelled
);
int vim_core_bridge_embedded_mode_active(void);
void vim_core_bridge_enqueue_message_event(
    const char* text,
    uintptr_t text_len,
    vim_core_message_severity_t severity,
    vim_core_message_category_t category
);
void vim_core_bridge_enqueue_input_request(
    const char* prompt,
    uintptr_t prompt_len,
    vim_core_input_request_kind_t kind
);
int vim_core_bridge_take_input_response(
    vim_core_input_request_kind_t kind,
    char** value_ptr,
    uintptr_t* value_len,
    bool* cancelled
);
void vim_core_bridge_free_input_response(char* value);
void vim_core_bridge_enqueue_pager_prompt_event(vim_core_pager_prompt_kind_t kind);
void vim_core_bridge_enqueue_bell(void);

// ポップアップメニュー情報のメモリを解放する専用関数
// 各候補の文字列フィールド→候補配列→構造体自体の順で解放する
void vim_bridge_free_pum_info(vim_core_pum_info_t* pum);

/* --- Search Highlight Extraction (Tasks) --- */
typedef enum {
    VIM_CORE_MATCH_REGULAR = 0,
    VIM_CORE_MATCH_INCSEARCH = 1,
    VIM_CORE_MATCH_CURSEARCH = 2
} vim_core_match_type_t;

typedef struct {
    int start_row;
    int start_col;
    int end_row;
    int end_col;
    vim_core_match_type_t match_type;
} vim_core_match_range_t;

typedef struct {
    vim_core_match_range_t* ranges;
    int count;
} vim_core_match_list_t;

typedef enum {
    VIM_CORE_MATCH_COUNT_CALCULATED = 0,
    VIM_CORE_MATCH_COUNT_MAX_REACHED = 1,
    VIM_CORE_MATCH_COUNT_TIMED_OUT = 2
} vim_core_match_count_status_t;

typedef struct {
    int is_on_match;
    int current_match_index; // 1-based, 0 if not on match
    int total_matches;       // count value
    vim_core_match_count_status_t status; // Indicates if total_matches is exact, max reached, or timed out
} vim_core_cursor_match_info_t;

vim_core_match_list_t vim_bridge_get_search_highlights(int window_id, int start_row, int end_row);
void vim_bridge_free_match_list(vim_core_match_list_t list);

const char* vim_bridge_get_search_pattern(void);
int vim_bridge_is_hlsearch_active(void);

int vim_bridge_get_search_direction(void);

vim_core_cursor_match_info_t vim_bridge_get_cursor_match_info(int window_id, int row, int col, int max_count, int timeout_ms);

int vim_bridge_is_incsearch_active(void);
const char* vim_bridge_get_incsearch_pattern(void);
const char* vim_bridge_get_search_input_pattern(void);

#endif
