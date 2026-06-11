//! FFI 構造体を Core 型へ変換するヘルパー群。
//!
//! クレートルート（lib.rs）から抽出した、bindgen 生成の C 構造体（`bindings::*`）を
//! Core 型（`CoreSnapshot` / `CoreHostAction` / `CoreEvent` 等）へ変換する純粋な
//! ヘルパー群。変換結果である公開 Core 型および共有型 `ConvertedOptionValue`・
//! 共有 FFI ヘルパー `string_from_parts` は lib.rs に残し、本モジュールからは
//! `use super::*;` 経由で参照する。

use super::*;

pub(super) fn convert_command_result_with_snapshot(
    result: bindings::vim_core_command_result_t,
) -> Result<(CoreCommandOutcome, CoreSnapshot), CoreCommandError> {
    let snapshot = convert_snapshot(result.snapshot);
    match result.status {
        value if value == bindings::vim_core_status_VIM_CORE_STATUS_OK => Ok(match result.outcome {
            outcome if outcome == bindings::vim_core_command_outcome_kind_VIM_CORE_COMMAND_OUTCOME_NO_CHANGE => {
                (CoreCommandOutcome::NoChange, snapshot)
            }
            outcome
                if outcome
                    == bindings::vim_core_command_outcome_kind_VIM_CORE_COMMAND_OUTCOME_BUFFER_CHANGED =>
            {
                (
                    CoreCommandOutcome::BufferChanged {
                        revision: snapshot.revision,
                    },
                    snapshot,
                )
            }
            outcome
                if outcome
                    == bindings::vim_core_command_outcome_kind_VIM_CORE_COMMAND_OUTCOME_CURSOR_CHANGED =>
            {
                (
                    CoreCommandOutcome::CursorChanged {
                        row: snapshot.cursor_row,
                        col: snapshot.cursor_col,
                    },
                    snapshot,
                )
            }
            outcome
                if outcome
                    == bindings::vim_core_command_outcome_kind_VIM_CORE_COMMAND_OUTCOME_MODE_CHANGED =>
            {
                (
                    CoreCommandOutcome::ModeChanged {
                        mode: snapshot.mode,
                    },
                    snapshot,
                )
            }
            outcome
                if outcome
                    == bindings::vim_core_command_outcome_kind_VIM_CORE_COMMAND_OUTCOME_HOST_ACTION_QUEUED =>
            {
                (CoreCommandOutcome::HostActionQueued, snapshot)
            }
            _ => (CoreCommandOutcome::NoChange, snapshot),
        }),
        value if value == bindings::vim_core_status_VIM_CORE_STATUS_COMMAND_ERROR => {
            Err(CoreCommandError::OperationFailed {
                reason_code: result.reason_code,
            })
        }
        value if value == bindings::vim_core_status_VIM_CORE_STATUS_SESSION_ERROR => {
            Err(CoreCommandError::OperationFailed {
                reason_code: result.reason_code,
            })
        }
        status => Err(CoreCommandError::UnknownStatus {
            status,
            reason_code: result.reason_code,
        }),
    }
}

pub(super) fn convert_status(status: bindings::vim_core_status_t) -> Result<(), CoreCommandError> {
    match status {
        value if value == bindings::vim_core_status_VIM_CORE_STATUS_OK => Ok(()),
        value if value == bindings::vim_core_status_VIM_CORE_STATUS_COMMAND_ERROR => {
            Err(CoreCommandError::InvalidInput)
        }
        value if value == bindings::vim_core_status_VIM_CORE_STATUS_SESSION_ERROR => {
            Err(CoreCommandError::OperationFailed { reason_code: 0 })
        }
        status => Err(CoreCommandError::UnknownStatus {
            status,
            reason_code: 0,
        }),
    }
}

pub(super) fn convert_snapshot(snapshot: bindings::vim_core_snapshot_t) -> CoreSnapshot {
    let started_at = Instant::now();
    let buffers = convert_buffer_list(snapshot.buffers, snapshot.buffer_count);
    let buffers_ms = started_at.elapsed().as_millis();
    let windows_started_at = Instant::now();
    let windows = convert_window_list(snapshot.windows, snapshot.window_count);
    let windows_ms = windows_started_at.elapsed().as_millis();

    // Free the C-allocated arrays (the data has been copied into Rust Vecs)
    if !snapshot.buffers.is_null() {
        unsafe { libc_free(snapshot.buffers.cast()) };
    }
    if !snapshot.windows.is_null() {
        unsafe { libc_free(snapshot.windows.cast()) };
    }

    // ポップアップメニュー情報の変換とメモリ解放
    let pum_started_at = Instant::now();
    let pum = convert_pum_info(snapshot.pum);
    let pum_ms = pum_started_at.elapsed().as_millis();
    let text_started_at = Instant::now();
    let text = string_from_parts(snapshot.text_ptr, snapshot.text_len);
    let text_ms = text_started_at.elapsed().as_millis();
    debug_log!(
        "[PERF][vim_core_rs] convert_snapshot text_len={} buffer_count={} window_count={} text_ms={} buffers_ms={} windows_ms={} pum_ms={} total_ms={}",
        text.len(),
        buffers.len(),
        windows.len(),
        text_ms,
        buffers_ms,
        windows_ms,
        pum_ms,
        started_at.elapsed().as_millis()
    );

    CoreSnapshot {
        text,
        revision: snapshot.revision,
        dirty: snapshot.dirty,
        mode: convert_mode(snapshot.mode),
        pending_input: CorePendingInput::none(),
        cursor_row: snapshot.cursor_row,
        cursor_col: snapshot.cursor_col,
        pending_host_actions: snapshot.pending_host_actions,
        buffers,
        windows,
        pum,
    }
}

pub(super) fn convert_light_snapshot(snapshot: bindings::vim_core_snapshot_t) -> CoreLightSnapshot {
    let started_at = Instant::now();
    let buffers = convert_buffer_list(snapshot.buffers, snapshot.buffer_count);
    let buffers_ms = started_at.elapsed().as_millis();
    let windows_started_at = Instant::now();
    let windows = convert_window_list(snapshot.windows, snapshot.window_count);
    let windows_ms = windows_started_at.elapsed().as_millis();

    if !snapshot.buffers.is_null() {
        unsafe { libc_free(snapshot.buffers.cast()) };
    }
    if !snapshot.windows.is_null() {
        unsafe { libc_free(snapshot.windows.cast()) };
    }

    let pum_started_at = Instant::now();
    let pum = convert_pum_info(snapshot.pum);
    let pum_ms = pum_started_at.elapsed().as_millis();
    debug_log!(
        "[PERF][vim_core_rs] convert_light_snapshot buffer_count={} window_count={} buffers_ms={} windows_ms={} pum_ms={} total_ms={}",
        buffers.len(),
        windows.len(),
        buffers_ms,
        windows_ms,
        pum_ms,
        started_at.elapsed().as_millis()
    );

    CoreLightSnapshot {
        revision: snapshot.revision,
        dirty: snapshot.dirty,
        mode: convert_mode(snapshot.mode),
        pending_input: CorePendingInput::none(),
        cursor_row: snapshot.cursor_row,
        cursor_col: snapshot.cursor_col,
        pending_host_actions: snapshot.pending_host_actions,
        buffers,
        windows,
        pum,
    }
}

/// C側のポップアップメニュー情報をRust型に変換し、C側メモリを解放する
fn convert_pum_info(pum_ptr: *mut bindings::vim_core_pum_info_t) -> Option<CorePumInfo> {
    if pum_ptr.is_null() {
        return None;
    }

    let pum = unsafe { &*pum_ptr };

    debug_log!(
        "[DEBUG] convert_pum_info: row={} col={} width={} height={} selected={} item_count={}",
        pum.row,
        pum.col,
        pum.width,
        pum.height,
        pum.selected_index,
        pum.item_count
    );

    // 候補配列を走査し、各候補のC文字列をRustのStringに変換
    let items = if !pum.items.is_null() && pum.item_count > 0 {
        let slice = unsafe { std::slice::from_raw_parts(pum.items, pum.item_count) };
        slice
            .iter()
            .map(|item| CorePumItem {
                word: c_str_to_string(item.word),
                abbr: c_str_to_string(item.abbr),
                menu: c_str_to_string(item.menu),
                kind: c_str_to_string(item.kind),
                info: c_str_to_string(item.info),
            })
            .collect()
    } else {
        Vec::new()
    };

    // 未選択状態 (selected_index == -1) は None にマッピング
    let selected_index = if pum.selected_index < 0 {
        None
    } else {
        Some(pum.selected_index as usize)
    };

    let result = CorePumInfo {
        row: pum.row,
        col: pum.col,
        width: pum.width,
        height: pum.height,
        selected_index,
        items,
    };

    // C側メモリを専用解放関数で解放
    unsafe {
        bindings::vim_bridge_free_pum_info(pum_ptr);
    }

    debug_log!(
        "[DEBUG] convert_pum_info: conversion complete, {} items, selected={:?}",
        result.items.len(),
        result.selected_index
    );

    Some(result)
}

/// NULLセーフなCストリング→Rust String変換
fn c_str_to_string(ptr: *const ::std::os::raw::c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() }
}

unsafe extern "C" {
    fn free(ptr: *mut std::ffi::c_void);
}

unsafe fn libc_free(ptr: *mut std::ffi::c_void) {
    unsafe { free(ptr) }
}

fn convert_buffer_list(
    ptr: *mut bindings::vim_core_buffer_info_t,
    count: usize,
) -> Vec<CoreBufferInfo> {
    if ptr.is_null() || count == 0 {
        return Vec::new();
    }

    let slice = unsafe { slice::from_raw_parts(ptr, count) };
    slice
        .iter()
        .map(|info| CoreBufferInfo {
            id: info.id,
            name: string_from_parts(info.name_ptr, info.name_len),
            source_revision: CoreBufferRevision {
                value: info.source_revision,
            },
            dirty: info.dirty,
            is_active: info.is_active,
            source_kind: CoreBufferSourceKind::Local,
            document_id: None,
            pending_vfs_operation: None,
            deferred_close: None,
            last_vfs_error: None,
        })
        .collect()
}

fn convert_window_list(
    ptr: *mut bindings::vim_core_window_info_t,
    count: usize,
) -> Vec<CoreWindowInfo> {
    if ptr.is_null() || count == 0 {
        return Vec::new();
    }

    let slice = unsafe { slice::from_raw_parts(ptr, count) };
    slice
        .iter()
        .map(|info| CoreWindowInfo {
            id: info.id,
            buf_id: info.buf_id,
            row: info.row,
            col: info.col,
            width: info.width,
            height: info.height,
            topline: info.topline,
            botline: info.botline,
            leftcol: info.leftcol,
            skipcol: info.skipcol,
            cursor_row: info.cursor_row,
            cursor_col: info.cursor_col,
            is_active: info.is_active,
        })
        .collect()
}

fn convert_mode(mode: bindings::vim_core_mode_t) -> CoreMode {
    match mode {
        value if value == bindings::vim_core_mode_VIM_CORE_MODE_INSERT => CoreMode::Insert,
        value if value == bindings::vim_core_mode_VIM_CORE_MODE_VISUAL => CoreMode::Visual,
        value if value == bindings::vim_core_mode_VIM_CORE_MODE_VISUAL_LINE => CoreMode::VisualLine,
        value if value == bindings::vim_core_mode_VIM_CORE_MODE_VISUAL_BLOCK => {
            CoreMode::VisualBlock
        }
        value if value == bindings::vim_core_mode_VIM_CORE_MODE_REPLACE => CoreMode::Replace,
        value if value == bindings::vim_core_mode_VIM_CORE_MODE_SELECT => CoreMode::Select,
        value if value == bindings::vim_core_mode_VIM_CORE_MODE_SELECT_LINE => CoreMode::SelectLine,
        value if value == bindings::vim_core_mode_VIM_CORE_MODE_SELECT_BLOCK => {
            CoreMode::SelectBlock
        }
        value if value == bindings::vim_core_mode_VIM_CORE_MODE_COMMAND_LINE => {
            CoreMode::CommandLine
        }
        value if value == bindings::vim_core_mode_VIM_CORE_MODE_OPERATOR_PENDING => {
            CoreMode::OperatorPending
        }
        _ => CoreMode::Normal,
    }
}

pub(super) fn convert_native_pending_argument(
    pending_input: bindings::vim_core_pending_input_t,
) -> Option<CorePendingArgumentKind> {
    match pending_input {
        value if value == bindings::vim_core_pending_input_VIM_CORE_PENDING_INPUT_CHAR => {
            Some(CorePendingArgumentKind::Char)
        }
        value if value == bindings::vim_core_pending_input_VIM_CORE_PENDING_INPUT_REPLACE => {
            Some(CorePendingArgumentKind::ReplaceChar)
        }
        value if value == bindings::vim_core_pending_input_VIM_CORE_PENDING_INPUT_MARK_SET => {
            Some(CorePendingArgumentKind::MarkSet)
        }
        value if value == bindings::vim_core_pending_input_VIM_CORE_PENDING_INPUT_MARK_JUMP => {
            Some(CorePendingArgumentKind::MarkJump)
        }
        value if value == bindings::vim_core_pending_input_VIM_CORE_PENDING_INPUT_REGISTER => {
            Some(CorePendingArgumentKind::Register)
        }
        _ => None,
    }
}

pub(super) fn convert_mark_position(mark: bindings::vim_core_mark_position_t) -> CoreMarkPosition {
    CoreMarkPosition {
        buf_id: mark.buf_id,
        row: mark.row,
        col: mark.col,
    }
}

pub(super) fn convert_jumplist(jumplist: bindings::vim_core_jumplist_t) -> CoreJumpList {
    let entries = convert_jumplist_entries(jumplist.entries, jumplist.entry_count);

    unsafe {
        bindings::vim_bridge_free_jumplist(jumplist);
    }

    CoreJumpList {
        current_index: if jumplist.has_current_index {
            jumplist.current_index
        } else {
            0
        },
        entries,
    }
}

fn convert_jumplist_entries(
    ptr: *mut bindings::vim_core_jumplist_entry_t,
    count: usize,
) -> Vec<CoreJumpListEntry> {
    if ptr.is_null() || count == 0 {
        return Vec::new();
    }

    let slice = unsafe { slice::from_raw_parts(ptr, count) };
    slice
        .iter()
        .map(|entry| CoreJumpListEntry {
            buf_id: entry.buf_id,
            row: entry.row,
            col: entry.col,
        })
        .collect()
}

pub(super) fn convert_undo_tree(tree: bindings::vim_core_undo_tree_t) -> CoreUndoTree {
    let mut nodes = Vec::new();
    if !tree.nodes.is_null() && tree.length > 0 {
        let slice = unsafe { slice::from_raw_parts(tree.nodes, tree.length) };
        for node in slice {
            nodes.push(CoreUndoNode {
                seq: node.seq as i32,
                time: node.time,
                save_nr: node.save_nr as i32,
                prev_seq: if node.prev_seq > 0 {
                    Some(node.prev_seq as i32)
                } else {
                    None
                },
                next_seq: if node.next_seq > 0 {
                    Some(node.next_seq as i32)
                } else {
                    None
                },
                alt_next_seq: if node.alt_next_seq > 0 {
                    Some(node.alt_next_seq as i32)
                } else {
                    None
                },
                alt_prev_seq: if node.alt_prev_seq > 0 {
                    Some(node.alt_prev_seq as i32)
                } else {
                    None
                },
                is_newhead: node.is_newhead,
                is_curhead: node.is_curhead,
            });
        }
    }

    let result = CoreUndoTree {
        nodes,
        synced: tree.synced,
        seq_last: tree.seq_last as i32,
        save_last: tree.save_last as i32,
        seq_cur: tree.seq_cur as i32,
        time_cur: tree.time_cur,
        save_cur: tree.save_cur as i32,
    };

    unsafe {
        bindings::vim_bridge_free_undo_tree(tree);
    }

    result
}

pub(super) fn convert_host_action(action: bindings::vim_host_action_t) -> Option<CoreHostAction> {
    match action.kind {
        value if value == bindings::VIM_HOST_ACTION_NONE => None,
        value if value == bindings::VIM_HOST_ACTION_WRITE => Some(CoreHostAction::Write {
            path: string_from_parts(action.primary_text_ptr, action.primary_text_len),
            force: action.force,
            issued_after_revision: action.issued_after_revision,
        }),
        value if value == bindings::VIM_HOST_ACTION_QUIT => Some(CoreHostAction::Quit {
            force: action.force,
            issued_after_revision: action.issued_after_revision,
        }),
        value if value == bindings::VIM_HOST_ACTION_REDRAW => Some(CoreHostAction::Redraw {
            full: true,
            clear_before_draw: action.redraw_force,
        }),
        value if value == bindings::VIM_HOST_ACTION_REQUEST_INPUT => {
            Some(CoreHostAction::RequestInput {
                prompt: string_from_parts(action.primary_text_ptr, action.primary_text_len),
                input_kind: convert_input_kind(action.input_kind),
                correlation_id: action.correlation_id,
            })
        }
        value if value == bindings::VIM_HOST_ACTION_BELL => Some(CoreHostAction::Bell),
        value if value == bindings::VIM_HOST_ACTION_BUF_ADD => None,
        value if value == bindings::VIM_HOST_ACTION_WIN_NEW => None,
        value if value == bindings::VIM_HOST_ACTION_LAYOUT_CHANGED => None,
        value if value == bindings::VIM_HOST_ACTION_JOB_START => {
            let req = action.job_start_request;
            crate::vfd::get_manager().register_job(
                req.job_id,
                req.vfd_in,
                req.vfd_out,
                req.vfd_err,
            );
            let mut argv = Vec::new();
            if !req.argv_buf.is_null() && req.argv_len > 0 {
                let slice =
                    unsafe { std::slice::from_raw_parts(req.argv_buf as *const u8, req.argv_len) };
                for arg_slice in slice.split(|&b| b == 0) {
                    if !arg_slice.is_empty()
                        && let Ok(s) = std::str::from_utf8(arg_slice)
                    {
                        argv.push(s.to_owned());
                    }
                }
                unsafe { bindings::vim_bridge_free_string(req.argv_buf) };
            }
            let cwd = if !req.cwd.is_null() {
                let s = unsafe { std::ffi::CStr::from_ptr(req.cwd) }
                    .to_string_lossy()
                    .into_owned();
                unsafe { bindings::vim_bridge_free_string(req.cwd) };
                Some(s)
            } else {
                None
            };
            Some(CoreHostAction::JobStart(CoreJobStartRequest {
                job_id: req.job_id,
                argv,
                cwd,
                vfd_in: req.vfd_in,
                vfd_out: req.vfd_out,
                vfd_err: req.vfd_err,
            }))
        }
        value if value == bindings::VIM_HOST_ACTION_JOB_STOP => Some(CoreHostAction::JobStop {
            job_id: action.job_start_request.job_id,
        }),
        _ => None,
    }
}

pub(super) fn convert_event(event: bindings::vim_core_event_t) -> Option<CoreEvent> {
    match event.kind {
        value if value == bindings::vim_core_event_kind_VIM_CORE_EVENT_NONE => None,
        value if value == bindings::vim_core_event_kind_VIM_CORE_EVENT_MESSAGE => {
            let severity = match event.message_severity {
                value
                    if value
                        == bindings::vim_core_message_severity_VIM_CORE_MESSAGE_SEVERITY_ERROR =>
                {
                    CoreMessageSeverity::Error
                }
                value
                    if value
                        == bindings::vim_core_message_severity_VIM_CORE_MESSAGE_SEVERITY_WARNING =>
                {
                    CoreMessageSeverity::Warning
                }
                _ => CoreMessageSeverity::Info,
            };
            let category = match event.message_category {
                value
                    if value
                        == bindings::vim_core_message_category_VIM_CORE_MESSAGE_CATEGORY_COMMAND_FEEDBACK =>
                {
                    CoreMessageCategory::CommandFeedback
                }
                _ => CoreMessageCategory::UserVisible,
            };
            Some(CoreEvent::Message(CoreMessageEvent {
                severity,
                category,
                content: string_from_parts(event.text_ptr, event.text_len),
            }))
        }
        value if value == bindings::vim_core_event_kind_VIM_CORE_EVENT_PAGER_PROMPT => {
            let kind = match event.pager_prompt_kind {
                value
                    if value
                        == bindings::vim_core_pager_prompt_kind_VIM_CORE_PAGER_PROMPT_HIT_RETURN =>
                {
                    CorePagerPromptKind::HitReturn
                }
                _ => CorePagerPromptKind::More,
            };
            Some(CoreEvent::PagerPrompt(kind))
        }
        value if value == bindings::vim_core_event_kind_VIM_CORE_EVENT_BELL => {
            Some(CoreEvent::Bell)
        }
        value if value == bindings::vim_core_event_kind_VIM_CORE_EVENT_REDRAW => {
            Some(CoreEvent::Redraw {
                full: event.full,
                clear_before_draw: event.clear_before_draw,
            })
        }
        value if value == bindings::vim_core_event_kind_VIM_CORE_EVENT_BUF_ADD => {
            Some(CoreEvent::BufferAdded {
                buf_id: event.buf_id,
            })
        }
        value if value == bindings::vim_core_event_kind_VIM_CORE_EVENT_WIN_NEW => {
            Some(CoreEvent::WindowCreated {
                win_id: event.win_id,
            })
        }
        value if value == bindings::vim_core_event_kind_VIM_CORE_EVENT_LAYOUT_CHANGED => {
            Some(CoreEvent::LayoutChanged)
        }
        _ => None,
    }
}

fn convert_input_kind(kind: bindings::vim_core_input_request_kind_t) -> CoreInputRequestKind {
    match kind {
        value
            if value
                == bindings::vim_core_input_request_kind_VIM_CORE_INPUT_REQUEST_CONFIRMATION =>
        {
            CoreInputRequestKind::Confirmation
        }
        value if value == bindings::vim_core_input_request_kind_VIM_CORE_INPUT_REQUEST_SECRET => {
            CoreInputRequestKind::Secret
        }
        _ => CoreInputRequestKind::CommandLine,
    }
}

pub(super) fn convert_option_scope(scope: CoreOptionScope) -> bindings::vim_core_option_scope_t {
    match scope {
        CoreOptionScope::Default => bindings::vim_core_option_scope_VIM_CORE_OPTION_SCOPE_DEFAULT,
        CoreOptionScope::Global => bindings::vim_core_option_scope_VIM_CORE_OPTION_SCOPE_GLOBAL,
        CoreOptionScope::Local => bindings::vim_core_option_scope_VIM_CORE_OPTION_SCOPE_LOCAL,
    }
}

fn convert_option_type(option_type: bindings::vim_core_option_type_t) -> Option<CoreOptionType> {
    match option_type {
        value if value == bindings::vim_core_option_type_VIM_CORE_OPTION_TYPE_BOOL => {
            Some(CoreOptionType::Bool)
        }
        value if value == bindings::vim_core_option_type_VIM_CORE_OPTION_TYPE_NUMBER => {
            Some(CoreOptionType::Number)
        }
        value if value == bindings::vim_core_option_type_VIM_CORE_OPTION_TYPE_STRING => {
            Some(CoreOptionType::String)
        }
        value if value == bindings::vim_core_option_type_VIM_CORE_OPTION_TYPE_UNKNOWN => None,
        _ => None,
    }
}

pub(super) fn convert_option_get_result(
    name: &str,
    scope: CoreOptionScope,
    expected: CoreOptionType,
    result: bindings::vim_core_option_get_result_t,
) -> Result<ConvertedOptionValue, CoreOptionError> {
    let actual = convert_option_type(result.option_type);
    debug_log!(
        "[DEBUG] convert_option_get_result: name={} scope={:?} status={} expected={:?} actual={:?}",
        name,
        scope,
        result.status,
        expected,
        actual
    );

    match result.status {
        value if value == bindings::vim_core_status_VIM_CORE_STATUS_OK => {}
        value if value == bindings::vim_core_status_VIM_CORE_STATUS_COMMAND_ERROR => {
            return match actual {
                None => Err(CoreOptionError::UnknownOption {
                    name: name.to_string(),
                }),
                Some(_) if scope == CoreOptionScope::Local => {
                    Err(CoreOptionError::ScopeNotSupported {
                        name: name.to_string(),
                        scope,
                    })
                }
                Some(actual_type) => Err(CoreOptionError::InternalError {
                    name: name.to_string(),
                    detail: format!(
                        "option command error for {:?} without local scope support path",
                        actual_type
                    ),
                }),
            };
        }
        value if value == bindings::vim_core_status_VIM_CORE_STATUS_SESSION_ERROR => {
            return Err(CoreOptionError::InternalError {
                name: name.to_string(),
                detail: "option get bridge returned session error".to_string(),
            });
        }
        status => {
            return Err(CoreOptionError::InternalError {
                name: name.to_string(),
                detail: format!("unknown option get status: {}", status),
            });
        }
    }

    let Some(actual_type) = actual else {
        return Err(CoreOptionError::InternalError {
            name: name.to_string(),
            detail: "option get succeeded with unknown type".to_string(),
        });
    };

    if actual_type != expected {
        return Err(CoreOptionError::TypeMismatch {
            name: name.to_string(),
            expected,
            actual: actual_type,
        });
    }

    match actual_type {
        CoreOptionType::Bool => Ok(ConvertedOptionValue::Bool(result.number_value != 0)),
        CoreOptionType::Number => Ok(ConvertedOptionValue::Number(result.number_value)),
        CoreOptionType::String => {
            let value = string_from_parts(result.string_value_ptr, result.string_value_len);
            if !result.string_value_ptr.is_null() {
                unsafe { bindings::vim_bridge_free_string(result.string_value_ptr.cast_mut()) };
            }
            Ok(ConvertedOptionValue::String(value))
        }
    }
}

pub(super) fn convert_option_set_result(
    name: &str,
    result: bindings::vim_core_option_set_result_t,
) -> Result<(), CoreOptionError> {
    debug_log!(
        "[DEBUG] convert_option_set_result: name={} status={} error_len={}",
        name,
        result.status,
        result.error_message_len
    );

    let error_message = string_from_parts(result.error_message_ptr, result.error_message_len);
    if !result.error_message_ptr.is_null() {
        unsafe { bindings::vim_bridge_free_string(result.error_message_ptr.cast_mut()) };
    }

    match result.status {
        value if value == bindings::vim_core_status_VIM_CORE_STATUS_OK => Ok(()),
        value if value == bindings::vim_core_status_VIM_CORE_STATUS_COMMAND_ERROR => {
            Err(CoreOptionError::SetFailed {
                name: name.to_string(),
                reason: error_message,
            })
        }
        value if value == bindings::vim_core_status_VIM_CORE_STATUS_SESSION_ERROR => {
            Err(CoreOptionError::InternalError {
                name: name.to_string(),
                detail: if error_message.is_empty() {
                    "option set bridge returned session error".to_string()
                } else {
                    error_message
                },
            })
        }
        status => Err(CoreOptionError::InternalError {
            name: name.to_string(),
            detail: format!("unknown option set status: {}", status),
        }),
    }
}

#[cfg(test)]
mod option_conversion_tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn option_get_result_returns_unknown_option_for_unknown_type() {
        let result = bindings::vim_core_option_get_result_t {
            status: bindings::vim_core_status_VIM_CORE_STATUS_COMMAND_ERROR,
            option_type: bindings::vim_core_option_type_VIM_CORE_OPTION_TYPE_UNKNOWN,
            number_value: 0,
            string_value_ptr: std::ptr::null(),
            string_value_len: 0,
        };

        assert_eq!(
            convert_option_get_result(
                "missing",
                CoreOptionScope::Default,
                CoreOptionType::Number,
                result,
            ),
            Err(CoreOptionError::UnknownOption {
                name: "missing".to_string(),
            })
        );
    }

    #[test]
    fn option_get_result_returns_type_mismatch_when_actual_type_differs() {
        let result = bindings::vim_core_option_get_result_t {
            status: bindings::vim_core_status_VIM_CORE_STATUS_OK,
            option_type: bindings::vim_core_option_type_VIM_CORE_OPTION_TYPE_STRING,
            number_value: 0,
            string_value_ptr: std::ptr::null(),
            string_value_len: 0,
        };

        assert_eq!(
            convert_option_get_result(
                "tabstop",
                CoreOptionScope::Default,
                CoreOptionType::Number,
                result,
            ),
            Err(CoreOptionError::TypeMismatch {
                name: "tabstop".to_string(),
                expected: CoreOptionType::Number,
                actual: CoreOptionType::String,
            })
        );
    }

    #[test]
    fn option_get_result_returns_scope_not_supported_for_local_known_option() {
        let result = bindings::vim_core_option_get_result_t {
            status: bindings::vim_core_status_VIM_CORE_STATUS_COMMAND_ERROR,
            option_type: bindings::vim_core_option_type_VIM_CORE_OPTION_TYPE_BOOL,
            number_value: 0,
            string_value_ptr: std::ptr::null(),
            string_value_len: 0,
        };

        assert_eq!(
            convert_option_get_result(
                "number",
                CoreOptionScope::Local,
                CoreOptionType::Bool,
                result,
            ),
            Err(CoreOptionError::ScopeNotSupported {
                name: "number".to_string(),
                scope: CoreOptionScope::Local,
            })
        );
    }

    #[test]
    fn option_get_result_copies_and_returns_string_values() {
        let value = CString::new("rust").expect("cstring");
        let len = value.as_bytes().len();
        let ptr = value.into_raw();
        let result = bindings::vim_core_option_get_result_t {
            status: bindings::vim_core_status_VIM_CORE_STATUS_OK,
            option_type: bindings::vim_core_option_type_VIM_CORE_OPTION_TYPE_STRING,
            number_value: 0,
            string_value_ptr: ptr,
            string_value_len: len,
        };

        assert_eq!(
            convert_option_get_result(
                "filetype",
                CoreOptionScope::Default,
                CoreOptionType::String,
                result,
            ),
            Ok(ConvertedOptionValue::String("rust".to_string()))
        );
    }

    #[test]
    fn option_set_result_returns_set_failed_with_reason() {
        let reason = CString::new("E487").expect("cstring");
        let len = reason.as_bytes().len();
        let ptr = reason.into_raw();
        let result = bindings::vim_core_option_set_result_t {
            status: bindings::vim_core_status_VIM_CORE_STATUS_COMMAND_ERROR,
            error_message_ptr: ptr,
            error_message_len: len,
        };

        assert_eq!(
            convert_option_set_result("tabstop", result),
            Err(CoreOptionError::SetFailed {
                name: "tabstop".to_string(),
                reason: "E487".to_string(),
            })
        );
    }

    #[test]
    fn option_scope_converts_to_ffi_values() {
        assert_eq!(
            convert_option_scope(CoreOptionScope::Default),
            bindings::vim_core_option_scope_VIM_CORE_OPTION_SCOPE_DEFAULT
        );
        assert_eq!(
            convert_option_scope(CoreOptionScope::Global),
            bindings::vim_core_option_scope_VIM_CORE_OPTION_SCOPE_GLOBAL
        );
        assert_eq!(
            convert_option_scope(CoreOptionScope::Local),
            bindings::vim_core_option_scope_VIM_CORE_OPTION_SCOPE_LOCAL
        );
    }
}

#[cfg(test)]
mod undo_conversion_tests {
    use super::*;

    #[test]
    fn convert_undo_tree_handles_empty_tree() {
        let tree = bindings::vim_core_undo_tree_t {
            nodes: std::ptr::null_mut(),
            length: 0,
            synced: true,
            seq_last: 0,
            save_last: 0,
            seq_cur: 0,
            time_cur: 0,
            save_cur: 0,
        };

        let core_tree = convert_undo_tree(tree);
        assert_eq!(core_tree.nodes.len(), 0);
        assert!(core_tree.synced);
        assert_eq!(core_tree.seq_last, 0);
    }

    #[test]
    fn convert_undo_tree_handles_populated_tree() {
        let raw_nodes = [
            bindings::vim_core_undo_node_t {
                seq: 1,
                time: 12345,
                save_nr: 0,
                prev_seq: 0,
                next_seq: 2,
                alt_next_seq: 0,
                alt_prev_seq: 0,
                is_newhead: true,
                is_curhead: false,
            },
            bindings::vim_core_undo_node_t {
                seq: 2,
                time: 12346,
                save_nr: 0,
                prev_seq: 1,
                next_seq: 0,
                alt_next_seq: 0,
                alt_prev_seq: 0,
                is_newhead: false,
                is_curhead: true,
            },
        ];

        unsafe extern "C" {
            fn malloc(size: usize) -> *mut std::ffi::c_void;
        }

        let ptr = unsafe {
            malloc(std::mem::size_of::<bindings::vim_core_undo_node_t>() * 2)
                as *mut bindings::vim_core_undo_node_t
        };
        unsafe {
            std::ptr::copy_nonoverlapping(raw_nodes.as_ptr(), ptr, 2);
        }

        let tree = bindings::vim_core_undo_tree_t {
            nodes: ptr,
            length: 2,
            synced: false,
            seq_last: 2,
            save_last: 0,
            seq_cur: 2,
            time_cur: 12346,
            save_cur: 0,
        };

        let core_tree = convert_undo_tree(tree);
        assert_eq!(core_tree.nodes.len(), 2);
        assert_eq!(core_tree.seq_last, 2);
        assert_eq!(core_tree.seq_cur, 2);

        let node1 = &core_tree.nodes[0];
        assert_eq!(node1.seq, 1);
        assert_eq!(node1.time, 12345);
        assert_eq!(node1.prev_seq, None);
        assert_eq!(node1.next_seq, Some(2));
        assert!(node1.is_newhead);

        let node2 = &core_tree.nodes[1];
        assert_eq!(node2.seq, 2);
        assert_eq!(node2.prev_seq, Some(1));
        assert_eq!(node2.next_seq, None);
        assert!(node2.is_curhead);
    }
}
