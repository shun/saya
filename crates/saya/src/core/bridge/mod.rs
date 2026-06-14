use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use vim_core_rs::{
    CoreBufferLineRange, CoreCommandError, CoreCommandOutcome, CoreEvent, CoreHostAction,
    CoreInputResponse, CoreInputResponseError, CoreLightSnapshot, CoreMatchType,
    CoreMessageCategory, CoreMessageEvent, CoreMessageSeverity, CoreOptionScope,
    CoreSearchHighlightMode, CoreSearchQueryError, CoreSessionError, CoreSessionOptions,
    CoreSnapshot, CoreSyntaxChunk, CoreVfsResponse, JobStatus, VimCoreSession,
};

use crate::core::outcome::{
    NormalizedCoreOutcome, NormalizedHostDirective, NormalizedNotification, NormalizedOutcomeBatch,
    NormalizedOutcomeQueue, NormalizedPrompt, NormalizedStructuralOutcome, OutcomeOrigin,
    OutcomeTrace, PromptResponseDisposition, core_event_raw_kind, core_host_action_raw_kind,
    normalize_core_event, normalize_host_action,
};
use crate::core::prompt::{PromptResponseCommand, PromptResponseError, PromptResponseRejection};
use crate::features::completion::session::{CompletionRange, apply_replace_range};
use crate::features::search::capability::SearchCapabilityContract;
use crate::features::search::query::{
    SearchMatch, SearchMatchKind, SearchQueryMode, SearchStateError, SearchVisibleQuery,
    SearchVisibleRows, SearchVisibleState,
};
use crate::runtime::options::{SayaOptionName, SayaOptionValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualSelection {
    pub mode: vim_core_rs::CoreMode,
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingRedrawRequest {
    pub full: bool,
    pub clear_before_draw: bool,
}

pub struct CoreBridge {
    session: VimCoreSession,
    pending_outcomes: NormalizedOutcomeQueue,
    next_outcome_sequence: u64,
    active_input_correlation_id: Option<u64>,
    pending_transport_key: Option<String>,
    syntax_enabled: bool,
    /// ADR 0006 Phase 3: `dispatch_key` が core に渡された回数。
    /// 「pending 中は backend を呼ばない／完成時のみ 1 回呼ぶ」をログ・テストで
    /// 観測するための seam。transport prefix のバッファリングで実際の dispatch を
    /// 行わなかった回も含めず、core へ実投入した回数のみ数える。
    dispatch_key_count: u64,
}

fn core_session_options_from_env() -> CoreSessionOptions {
    let debug_log_path = std::env::var_os("SAYA_VIM_CORE_LOG_FILE")
        .or_else(|| std::env::var_os("SAYA_LOG_FILE"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    CoreSessionOptions {
        debug_log_path,
        ..CoreSessionOptions::default()
    }
}

impl fmt::Debug for CoreBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoreBridge")
            .field("pending_outcomes_len", &self.pending_outcomes.len())
            .field("next_outcome_sequence", &self.next_outcome_sequence)
            .field(
                "active_input_correlation_id",
                &self.active_input_correlation_id,
            )
            .field("pending_transport_key", &self.pending_transport_key)
            .field("syntax_enabled", &self.syntax_enabled)
            .finish()
    }
}

impl CoreBridge {
    pub fn new(initial_text: &str) -> Result<Self, CoreSessionError> {
        let started_at = Instant::now();
        log::debug!(
            "[core_bridge] initializing vim-core-rs session: initial_text_len={}",
            initial_text.len()
        );
        let mut session =
            VimCoreSession::new_with_options(initial_text, core_session_options_from_env())?;
        log::debug!(
            "[PERF][core_bridge] vim-core-rs session constructed: initial_text_len={}, elapsed_ms={}",
            initial_text.len(),
            started_at.elapsed().as_millis()
        );
        let configure_started_at = Instant::now();
        configure_utf8_encoding(&mut session).map_err(CoreSessionError::CommandFailed)?;
        configure_message_suppression(&mut session).map_err(CoreSessionError::CommandFailed)?;
        configure_initial_syntax_state(&mut session).map_err(CoreSessionError::CommandFailed)?;
        log::debug!(
            "[PERF][core_bridge] vim-core-rs session initialized: initial_text_len={}, configure_ms={}, total_ms={}",
            initial_text.len(),
            configure_started_at.elapsed().as_millis(),
            started_at.elapsed().as_millis()
        );
        Ok(Self {
            session,
            pending_outcomes: NormalizedOutcomeQueue::default(),
            next_outcome_sequence: 1,
            active_input_correlation_id: None,
            pending_transport_key: None,
            syntax_enabled: false,
            dispatch_key_count: 0,
        })
    }

    /// ADR 0006 Phase 3: これまでに core へ実投入した `dispatch_key` の回数。
    /// transport prefix のバッファリングで実投入しなかった呼び出しは数えない。
    pub fn dispatch_key_count(&self) -> u64 {
        self.dispatch_key_count
    }

    pub fn new_with_target_path(
        target_path: &Path,
        initial_text: &str,
    ) -> Result<Self, CoreSessionError> {
        let mut bridge = Self::new(initial_text)?;
        bridge.attach_target_path(target_path)?;
        Ok(bridge)
    }

    pub fn snapshot(&self) -> CoreSnapshot {
        let started_at = Instant::now();
        let snapshot = self.session.snapshot();
        log::debug!(
            "[core_bridge] snapshot captured: revision={}, dirty={}, pending_host_actions={}",
            snapshot.revision,
            snapshot.dirty,
            snapshot.pending_host_actions
        );
        log::debug!(
            "[PERF][core_bridge] snapshot captured: text_len={}, buffers={}, windows={}, elapsed_ms={}",
            snapshot.text.len(),
            snapshot.buffers.len(),
            snapshot.windows.len(),
            started_at.elapsed().as_millis()
        );
        snapshot
    }

    pub fn revision(&self) -> u64 {
        self.light_snapshot().revision
    }

    pub fn dirty(&self) -> bool {
        self.light_snapshot().dirty
    }

    pub fn mode(&self) -> vim_core_rs::CoreMode {
        self.light_snapshot().mode
    }

    pub fn pending_input_is_pending(&self) -> bool {
        self.light_snapshot().pending_input.is_pending()
    }

    /// ADR 0006 Phase 3: キー列 `sequence` を現在のモードで core に dispatch したとき、
    /// 完成コマンドになるか pending（追加キー待ち）になるかを非破壊に予測する。
    ///
    /// core の純粋予測器 `predict_input_completeness` をそのまま委譲する薄いクエリ。
    /// session 状態を一切変更しないため、host 入力パイプラインが「pending の間は
    /// backend を呼ばず、完成時のみ完成キー列を 1 回 dispatch する」越境制御に使える。
    pub fn classify_input_completeness(
        &self,
        sequence: &str,
    ) -> vim_core_rs::CoreInputCompleteness {
        let mode = self.light_snapshot().mode;
        let completeness = vim_core_rs::predict_input_completeness(sequence, mode);
        log::debug!(
            "[core_bridge] classify_input_completeness: sequence={:?}, mode={:?}, completeness={:?}",
            sequence,
            mode,
            completeness
        );
        completeness
    }

    pub fn light_snapshot(&self) -> CoreLightSnapshot {
        let started_at = Instant::now();
        let snapshot = self.session.light_snapshot();
        log::debug!(
            "[core_bridge] light snapshot captured: revision={}, dirty={}, pending_host_actions={}",
            snapshot.revision,
            snapshot.dirty,
            snapshot.pending_host_actions
        );
        log::debug!(
            "[PERF][core_bridge] light snapshot captured: buffers={}, windows={}, elapsed_ms={}",
            snapshot.buffers.len(),
            snapshot.windows.len(),
            started_at.elapsed().as_millis()
        );
        snapshot
    }

    pub fn buffer_line_range(
        &self,
        buf_id: i32,
        start_row: usize,
        line_count: usize,
    ) -> Option<CoreBufferLineRange> {
        let started_at = Instant::now();
        let range = self
            .session
            .buffer_line_range(buf_id, start_row, line_count);
        log::debug!(
            "[PERF][core_bridge] buffer line range fetched: buf_id={}, start_row={}, requested_lines={}, returned_lines={}, total_line_count={:?}, elapsed_ms={}",
            buf_id,
            start_row,
            line_count,
            range
                .as_ref()
                .map(|range| range.lines.len())
                .unwrap_or_default(),
            range.as_ref().map(|range| range.total_line_count),
            started_at.elapsed().as_millis()
        );
        range
    }

    pub fn switch_to_window(&mut self, window_id: i32) -> Result<(), CoreSessionError> {
        log::debug!("[core_bridge] switching active window: window_id={window_id}");
        self.session
            .switch_to_window(window_id)
            .map_err(CoreSessionError::CommandFailed)?;
        self.drain_pending_host_actions_from_session();
        self.drain_pending_events_from_session();
        Ok(())
    }

    pub fn switch_to_buffer(&mut self, buffer_id: i32) -> Result<(), CoreSessionError> {
        log::debug!("[core_bridge] switching active buffer: buffer_id={buffer_id}");
        self.session
            .switch_to_buffer(buffer_id)
            .map_err(CoreSessionError::CommandFailed)?;
        self.drain_pending_host_actions_from_session();
        self.drain_pending_events_from_session();
        Ok(())
    }

    /// core がページスクロールの基準にする screen size を host 側で同期する。
    pub fn set_screen_size(&mut self, rows: i32, cols: i32) {
        log::debug!(
            "[core_bridge] setting screen size: rows={}, cols={}",
            rows,
            cols
        );
        self.session.set_screen_size(rows, cols);
    }

    pub fn take_pending_host_actions(&mut self) -> Vec<CoreHostAction> {
        self.drain_pending_host_actions_from_session();
        let actions = self
            .pending_outcomes
            .take_matching(|outcome| matches!(outcome, NormalizedCoreOutcome::HostDirective(_)))
            .into_iter()
            .filter_map(legacy_host_action_from_normalized)
            .collect::<Vec<_>>();
        log::debug!(
            "[core_bridge] projected pending host actions from normalized outcomes: count={}",
            actions.len()
        );
        actions
    }

    pub fn take_pending_messages(&mut self) -> Vec<CoreMessageEvent> {
        self.drain_pending_events_from_session();
        let messages = self
            .pending_outcomes
            .take_matching(|outcome| {
                matches!(
                    outcome,
                    NormalizedCoreOutcome::Notification(NormalizedNotification::Message { .. })
                )
            })
            .into_iter()
            .filter_map(legacy_message_from_normalized)
            .collect::<Vec<_>>();
        log::debug!(
            "[core_bridge] projected pending core messages from normalized outcomes: count={}",
            messages.len()
        );
        messages
    }

    pub fn take_pending_redraw_requests(&mut self) -> Vec<PendingRedrawRequest> {
        self.drain_pending_events_from_session();
        let redraw_requests = self
            .pending_outcomes
            .take_matching(|outcome| {
                matches!(
                    outcome,
                    NormalizedCoreOutcome::Structural(
                        NormalizedStructuralOutcome::RedrawRequested { .. }
                    )
                )
            })
            .into_iter()
            .filter_map(legacy_redraw_request_from_normalized)
            .collect::<Vec<_>>();
        log::debug!(
            "[core_bridge] projected pending redraw requests from normalized outcomes: count={}",
            redraw_requests.len()
        );
        redraw_requests
    }

    pub fn take_normalized_outcomes(&mut self) -> NormalizedOutcomeBatch {
        self.drain_pending_host_actions_from_session();
        self.drain_pending_events_from_session();
        let batch = self.pending_outcomes.drain();
        log::debug!(
            "[core_bridge] drained normalized core outcomes: count={}",
            batch.outcomes().len()
        );
        batch
    }

    pub fn respond_to_prompt(
        &mut self,
        command: PromptResponseCommand,
    ) -> Result<NormalizedOutcomeBatch, PromptResponseError> {
        let actual = command.correlation_id();
        let Some(expected) = self.active_input_correlation_id else {
            log::debug!(
                "[core_bridge] prompt response rejected before core call: no active prompt, actual={}",
                actual
            );
            return Err(PromptResponseError::NoActivePrompt);
        };
        if expected != actual {
            log::debug!(
                "[core_bridge] prompt response rejected before core call: expected={}, actual={}",
                expected,
                actual
            );
            return Err(PromptResponseError::CorrelationMismatch { expected, actual });
        }

        let (response, disposition, raw_kind) = match command {
            PromptResponseCommand::Submit {
                correlation_id,
                value,
            } => (
                CoreInputResponse::Submitted {
                    correlation_id,
                    value,
                },
                PromptResponseDisposition::Submitted,
                "PromptResponse::Submit",
            ),
            PromptResponseCommand::Cancel { correlation_id } => (
                CoreInputResponse::Cancelled { correlation_id },
                PromptResponseDisposition::Cancelled,
                "PromptResponse::Cancel",
            ),
        };

        log::debug!(
            "[core_bridge] submitting prompt response to core: correlation_id={}, disposition={:?}",
            actual,
            disposition
        );
        let tx = self
            .session
            .submit_input_response(response)
            .map_err(map_input_response_error)?;

        let mut outcomes = Vec::new();
        let trace = self.next_outcome_trace(OutcomeOrigin::BridgePromptResponse, raw_kind);
        outcomes.push(NormalizedCoreOutcome::Prompt(
            NormalizedPrompt::InputResponseAccepted {
                correlation_id: actual,
                disposition,
                trace,
            },
        ));
        self.active_input_correlation_id = None;
        self.collect_transaction_artifacts_into(&tx, &mut outcomes);

        log::debug!(
            "[core_bridge] prompt response normalized batch ready: correlation_id={}, count={}",
            actual,
            outcomes.len()
        );
        Ok(NormalizedOutcomeBatch::new(outcomes))
    }

    pub fn submit_vfs_response(
        &mut self,
        response: CoreVfsResponse,
    ) -> Result<CoreCommandOutcome, CoreSessionError> {
        log::debug!("[core_bridge] submitting VFS response: {:?}", response);
        let outcome = self
            .session
            .submit_vfs_response(response)
            .map_err(CoreSessionError::CommandFailed)?;
        self.drain_pending_host_actions_from_session();
        self.drain_pending_events_from_session();
        log::debug!("[core_bridge] VFS response result: {:?}", outcome);
        Ok(outcome)
    }

    pub fn inject_vfd_data(&mut self, vfd: i32, data: &[u8]) -> Result<(), CoreSessionError> {
        log::debug!(
            "[core_bridge] injecting VFD data into core: vfd={}, bytes={}",
            vfd,
            data.len()
        );
        self.session
            .inject_vfd_data(vfd, data)
            .map_err(CoreSessionError::CommandFailed)?;
        self.drain_pending_host_actions_from_session();
        self.drain_pending_events_from_session();
        Ok(())
    }

    pub fn notify_job_status(
        &mut self,
        job_id: i32,
        status: JobStatus,
        exit_code: i32,
    ) -> Result<(), CoreSessionError> {
        log::debug!(
            "[core_bridge] notifying job status: job_id={}, status={:?}, exit_code={}",
            job_id,
            status,
            exit_code
        );
        self.session
            .notify_job_status(job_id, status, exit_code)
            .map_err(CoreSessionError::CommandFailed)?;
        self.drain_pending_host_actions_from_session();
        self.drain_pending_events_from_session();
        Ok(())
    }

    /// キー入力を vim-core-rs セッションに適用する。
    /// ノーマルモードコマンドとして解釈し、結果を返す。
    pub fn dispatch_key(&mut self, key: &str) -> Result<CoreCommandOutcome, CoreSessionError> {
        if self.should_buffer_transport_prefix(key) {
            log::debug!(
                "[core_bridge] buffering transport-level key prefix for next dispatch: {:?}",
                key
            );
            self.pending_transport_key = Some(key.to_string());
            return Ok(CoreCommandOutcome::NoChange);
        }

        let key = if let Some(prefix) = self.take_pending_transport_key() {
            let combined = format!("{prefix}{key}");
            log::debug!(
                "[core_bridge] coalesced buffered transport prefix with incoming key: prefix={:?}, key={:?}, combined={:?}",
                prefix,
                key,
                combined
            );
            combined
        } else {
            key.to_string()
        };
        self.dispatch_key_count += 1;
        log::debug!(
            "[core_bridge] dispatching key: {:?} (len={}, dispatch_key_count={})",
            key,
            key.len(),
            self.dispatch_key_count
        );
        let outcome = if self.should_handle_ctrl_c_interrupt(&key) {
            self.handle_ctrl_c_interrupt()?
        } else {
            self.dispatch_session_key(&key)?
        };
        log::debug!(
            "[core_bridge] dispatch result: {:?}, pending_input={:?}",
            outcome,
            self.session.pending_input()
        );
        Ok(outcome)
    }

    /// ex コマンドを vim-core-rs セッションに適用する。
    /// `:w`, `:q`, `:q!` などの実行に使用する。
    pub fn apply_ex_command(
        &mut self,
        command: &str,
    ) -> Result<CoreCommandOutcome, CoreSessionError> {
        log::debug!("[core_bridge] applying ex command: {:?}", command);
        let outcome = self
            .session
            .execute_ex_command(command)
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&outcome);
        self.record_syntax_command_state(command);
        log::debug!("[core_bridge] ex command result: {:?}", outcome.outcome);
        Ok(outcome.outcome)
    }

    pub fn set_core_option(
        &mut self,
        name: SayaOptionName,
        value: SayaOptionValue,
    ) -> Result<(), CoreSessionError> {
        self.set_core_option_with_scope(name, value, CoreOptionScope::Default)
    }

    pub fn set_core_option_with_scope(
        &mut self,
        name: SayaOptionName,
        value: SayaOptionValue,
        scope: CoreOptionScope,
    ) -> Result<(), CoreSessionError> {
        log::debug!(
            "[core_bridge] setting core-owned option through typed API: name={}, value={:?}, scope={:?}",
            name,
            value,
            scope
        );
        let result = match value {
            SayaOptionValue::Boolean(value) => {
                self.session.set_option_bool(name.canonical(), value, scope)
            }
            SayaOptionValue::Number(value) => {
                self.session
                    .set_option_number(name.canonical(), value, scope)
            }
            SayaOptionValue::String(value) => {
                self.session
                    .set_option_string(name.canonical(), &value, scope)
            }
        };
        result.map_err(|error| {
            log::debug!(
                "[core_bridge] core option update failed: name={}, scope={:?}, error={:?}",
                name,
                scope,
                error
            );
            CoreSessionError::CommandFailed(vim_core_rs::CoreCommandError::OperationFailed {
                reason_code: 1,
            })
        })
    }

    /// buffer のテキスト内容を返す。保存要求の生成に使用する。
    pub fn buffer_text(&self) -> String {
        let snapshot = self.session.snapshot();
        log::debug!(
            "[core_bridge] buffer text retrieved: len={}",
            snapshot.text.len()
        );
        snapshot.text
    }

    pub fn replace_buffer_text(&mut self, text: &str) -> Result<(), CoreSessionError> {
        self.replace_buffer_text_inner(text, true)
    }

    pub fn apply_completion_replace_range(
        &mut self,
        range: &CompletionRange,
        replacement_text: &str,
    ) -> Result<(), CoreSessionError> {
        let before = self.buffer_text();
        let Some(after) = apply_replace_range(&before, range, replacement_text) else {
            log::debug!(
                "[core_bridge] completion replace range rejected: start=({}:{}), end=({}:{}), replacement_len={}, text_len={}",
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character,
                replacement_text.len(),
                before.len()
            );
            return Err(CoreSessionError::CommandFailed(
                vim_core_rs::CoreCommandError::OperationFailed { reason_code: 1 },
            ));
        };
        log::debug!(
            "[core_bridge] applying completion replace range: start=({}:{}), end=({}:{}), replacement_len={}, before_len={}, after_len={}",
            range.start.line,
            range.start.character,
            range.end.line,
            range.end.character,
            replacement_text.len(),
            before.len(),
            after.len()
        );
        self.replace_buffer_text_inner(&after, false)?;
        let (cursor_row, cursor_col) = completion_replacement_end(range, replacement_text);
        let cursor = self
            .session
            .execute_ex_command(&format!(
                "call cursor({}, {})",
                cursor_row + 1,
                cursor_col + 1
            ))
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&cursor);
        log::debug!(
            "[core_bridge] moved completion cursor to replacement end: row={}, col={}",
            cursor_row,
            cursor_col
        );
        Ok(())
    }

    fn replace_buffer_text_inner(
        &mut self,
        text: &str,
        clear_modified: bool,
    ) -> Result<(), CoreSessionError> {
        let lines = text
            .strip_suffix('\n')
            .unwrap_or(text)
            .split('\n')
            .collect::<Vec<_>>();
        let lines = if lines.is_empty() { vec![""] } else { lines };
        let list_expr = lines
            .iter()
            .map(|line| format!("'{}'", line.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(", ");
        log::debug!(
            "[core_bridge] replacing active buffer text through setline: line_count={}, text_len={}",
            lines.len(),
            text.len()
        );
        let setline = self
            .session
            .execute_ex_command(&format!("call setline(1, [{list_expr}])"))
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&setline);
        let snapshot = self.session.light_snapshot();
        let active_buffer_id = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .map(|buffer| buffer.id)
            .unwrap_or(1);
        let current_line_count = self
            .session
            .buffer_line_range(active_buffer_id, 0, 1)
            .map(|range| range.total_line_count)
            .unwrap_or(lines.len());
        if current_line_count > lines.len() {
            let delete = self
                .session
                .execute_ex_command(&format!("{},$delete _", lines.len() + 1))
                .map_err(CoreSessionError::CommandFailed)?;
            self.queue_transaction_artifacts(&delete);
        }
        if clear_modified {
            let nomodified = self
                .session
                .execute_ex_command("set nomodified")
                .map_err(CoreSessionError::CommandFailed)?;
            self.queue_transaction_artifacts(&nomodified);
        }
        Ok(())
    }

    pub fn attach_target_path(&mut self, target_path: &Path) -> Result<(), CoreSessionError> {
        let escaped_path = escape_path_for_file_command(target_path);
        log::debug!(
            "[core_bridge] attaching target path to active buffer: {}",
            target_path.display()
        );
        let tx = self
            .session
            .execute_ex_command(&format!(":file {}", escaped_path))
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&tx);
        Ok(())
    }

    pub fn current_visual_selection(&mut self) -> Option<VisualSelection> {
        let snapshot = self.session.light_snapshot();
        if !is_visual_mode(snapshot.mode) {
            return None;
        }
        let current = (snapshot.cursor_row, snapshot.cursor_col);
        let first_swap = self
            .session
            .execute_normal_command("o")
            .map_err(CoreSessionError::CommandFailed)
            .ok()?;
        self.queue_transaction_artifacts(&first_swap);
        let swapped = self.session.light_snapshot();
        let anchor = (swapped.cursor_row, swapped.cursor_col);
        let second_swap = self
            .session
            .execute_normal_command("o")
            .map_err(CoreSessionError::CommandFailed)
            .ok()?;
        self.queue_transaction_artifacts(&second_swap);
        let ((start_row, start_col), (end_row, end_col)) =
            normalize_selection_bounds(anchor, current);
        Some(VisualSelection {
            mode: snapshot.mode,
            start_row,
            start_col,
            end_row,
            end_col,
        })
    }

    pub fn sync_search_input(
        &mut self,
        pattern: &str,
    ) -> Result<CoreCommandOutcome, CoreSessionError> {
        if self.search_prompt_is_active() {
            let _ = self.dispatch_session_key("\x1b")?;
        }

        if pattern.is_empty() {
            log::debug!("[core_bridge] search prompt synced with empty pattern");
            return Ok(CoreCommandOutcome::NoChange);
        }

        log::debug!(
            "[core_bridge] syncing search prompt through core-owned state: pattern={:?}",
            pattern
        );
        let tx = self
            .session
            .execute_normal_command(&format!("/{}", pattern))
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&tx);
        Ok(tx.outcome)
    }

    pub fn commit_search_input(
        &mut self,
        pattern: &str,
    ) -> Result<CoreCommandOutcome, CoreSessionError> {
        if self.search_prompt_is_active() {
            let _ = self.dispatch_session_key("\x1b")?;
        }

        let tx = self
            .session
            .execute_normal_command(&format!("/{}\r", pattern))
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&tx);
        Ok(tx.outcome)
    }

    pub fn cancel_search_input(&mut self) -> Result<CoreCommandOutcome, CoreSessionError> {
        if !self.search_prompt_is_active() {
            return Ok(CoreCommandOutcome::NoChange);
        }
        self.dispatch_session_key("\x1b")
    }

    pub fn search_capability_contract(&self) -> SearchCapabilityContract {
        let contract = VimCoreSession::search_capability_contract();
        let contract = SearchCapabilityContract {
            live_state_query_available: contract.live_state_query_available,
            visible_rows_only: contract.visible_rows_only,
            start_col_inclusive: contract.start_col_inclusive,
            end_col_exclusive: contract.end_col_exclusive,
        };
        log::debug!(
            "[core_bridge] search capability contract resolved: live_state_query_available={}, visible_rows_only={}, start_col_inclusive={}, end_col_exclusive={}",
            contract.live_state_query_available,
            contract.visible_rows_only,
            contract.start_col_inclusive,
            contract.end_col_exclusive
        );
        contract
    }

    pub fn query_visible_search_state(
        &mut self,
        query: SearchVisibleQuery,
    ) -> Result<SearchVisibleState, SearchStateError> {
        if query.start_row == 0 || query.end_row < query.start_row {
            log::debug!(
                "[core_bridge] rejecting search query because viewport is invalid: query={:?}",
                query
            );
            return Err(SearchStateError::InvalidViewport {
                start_row: query.start_row,
                end_row: query.end_row,
            });
        }

        let capability = self.search_capability_contract();
        let core_state = self
            .session
            .query_visible_search_state(query.start_row as i32, query.end_row as i32)
            .map_err(map_search_query_error)?;
        let matches = core_state
            .ranges
            .into_iter()
            .map(|range| SearchMatch {
                kind: map_match_kind(range.match_type),
                start_row: range.start_row,
                start_col: range.start_col,
                end_row: range.end_row,
                end_col: range.end_col,
            })
            .collect::<Vec<_>>();
        let mut matches = matches;
        matches.sort_by_key(|range| {
            let kind_rank = match range.kind {
                SearchMatchKind::Current => 0usize,
                SearchMatchKind::Incremental => 1usize,
                SearchMatchKind::Regular => 2usize,
            };
            (
                kind_rank,
                range.start_row,
                range.start_col,
                range.end_row,
                range.end_col,
            )
        });
        log::debug!(
            "[core_bridge] resolved visible search state: query={:?}, window_id={}, mode={:?}, hlsearch_enabled={}, hlsearch_suspended={}, incsearch_active={}, pattern={:?}, input_pattern={:?}, matches={}",
            query,
            core_state.window_id,
            core_state.mode,
            core_state.hlsearch_enabled,
            core_state.hlsearch_suspended,
            core_state.incsearch_active,
            core_state.pattern,
            core_state.input_pattern,
            matches.len(),
        );

        Ok(SearchVisibleState {
            capability,
            visible_rows: SearchVisibleRows {
                start_row: core_state.start_row,
                end_row: core_state.end_row,
            },
            window_id: core_state.window_id,
            mode: map_search_mode(core_state.mode),
            pattern: core_state.pattern,
            input_pattern: core_state.input_pattern,
            hlsearch_enabled: core_state.hlsearch_enabled,
            hlsearch_suspended: core_state.hlsearch_suspended,
            incsearch_active: core_state.incsearch_active,
            matches,
        })
    }

    pub fn query_visible_search_state_for_window(
        &mut self,
        window_id: i32,
        query: SearchVisibleQuery,
    ) -> Result<SearchVisibleState, SearchStateError> {
        if query.start_row == 0 || query.end_row < query.start_row {
            log::debug!(
                "[core_bridge] rejecting window search query because viewport is invalid: window_id={}, query={:?}",
                window_id,
                query
            );
            return Err(SearchStateError::InvalidViewport {
                start_row: query.start_row,
                end_row: query.end_row,
            });
        }

        let capability = self.search_capability_contract();
        let core_state = self
            .session
            .query_visible_search_state_for_window(
                window_id,
                query.start_row as i32,
                query.end_row as i32,
            )
            .map_err(map_search_query_error)?;
        let matches = core_state
            .ranges
            .into_iter()
            .map(|range| SearchMatch {
                kind: map_match_kind(range.match_type),
                start_row: range.start_row,
                start_col: range.start_col,
                end_row: range.end_row,
                end_col: range.end_col,
            })
            .collect::<Vec<_>>();
        let mut matches = matches;
        matches.sort_by_key(|range| {
            let kind_rank = match range.kind {
                SearchMatchKind::Current => 0usize,
                SearchMatchKind::Incremental => 1usize,
                SearchMatchKind::Regular => 2usize,
            };
            (
                kind_rank,
                range.start_row,
                range.start_col,
                range.end_row,
                range.end_col,
            )
        });

        Ok(SearchVisibleState {
            capability,
            visible_rows: SearchVisibleRows {
                start_row: core_state.start_row,
                end_row: core_state.end_row,
            },
            window_id: core_state.window_id,
            mode: map_search_mode(core_state.mode),
            pattern: core_state.pattern,
            input_pattern: core_state.input_pattern,
            hlsearch_enabled: core_state.hlsearch_enabled,
            hlsearch_suspended: core_state.hlsearch_suspended,
            incsearch_active: core_state.incsearch_active,
            matches,
        })
    }

    pub fn has_search_highlight_activity(&self) -> bool {
        let active = self.session.is_incsearch_active() || self.session.is_hlsearch_active();
        log::debug!(
            "[core_bridge] resolved search highlight activity without snapshot: active={}",
            active
        );
        active
    }

    pub fn get_line_syntax(
        &self,
        window_id: i32,
        lnum: i64,
    ) -> Result<Vec<CoreSyntaxChunk>, CoreCommandError> {
        let chunks = self.session.get_line_syntax(window_id, lnum)?;
        log::debug!(
            "[core_bridge] resolved line syntax: window_id={}, lnum={}, chunks={}",
            window_id,
            lnum,
            chunks.len()
        );
        Ok(chunks)
    }

    pub fn is_syntax_enabled(&mut self) -> bool {
        log::debug!(
            "[core_bridge] resolved syntax enabled state from host command state: enabled={}",
            self.syntax_enabled
        );
        self.syntax_enabled
    }

    fn record_syntax_command_state(&mut self, command: &str) {
        match normalize_ex_command(command).as_deref() {
            Some("syntax on") => {
                log::debug!(
                    "[core_bridge] syntax command state updated: {} -> true",
                    self.syntax_enabled
                );
                self.syntax_enabled = true;
            }
            Some("syntax off") => {
                log::debug!(
                    "[core_bridge] syntax command state updated: {} -> false",
                    self.syntax_enabled
                );
                self.syntax_enabled = false;
            }
            _ => {}
        }
    }

    #[cfg(feature = "tree-sitter-syntax")]
    pub fn request_tree_sitter_syntax_preparation(
        &mut self,
        request: vim_core_rs::CoreTreeSitterPreparationRequest,
    ) -> Result<vim_core_rs::CoreTreeSitterPreparation, CoreCommandError> {
        log::debug!(
            "[core_bridge] requesting Tree-sitter syntax preparation: buffer_id={}, source_revision={:?}, range=({:?}..{:?}), buffer_name={:?}",
            request.buffer_id,
            request.source_revision,
            request.range.start,
            request.range.end,
            request.buffer_name
        );
        let preparation = self
            .session
            .request_tree_sitter_syntax_preparation(request)?;
        log::debug!(
            "[core_bridge] Tree-sitter syntax preparation accepted: request_id={}, buffer_id={}, source_revision={:?}, status={:?}",
            preparation.request_id.value,
            preparation.buffer_id,
            preparation.source_revision,
            preparation.status
        );
        Ok(preparation)
    }

    #[cfg(feature = "tree-sitter-syntax")]
    pub fn poll_tree_sitter_preparation(
        &mut self,
    ) -> Option<vim_core_rs::CoreTreeSitterPreparationResult> {
        let result = self.session.poll_tree_sitter_preparation();
        if let Some(result) = &result {
            log::debug!(
                "[core_bridge] Tree-sitter preparation completed: request_id={}, buffer_id={}, source_revision={:?}, status={:?}, chunks={}, embedded_regions={}",
                result.request_id.value,
                result.syntax.buffer_id,
                result.syntax.source_revision,
                result.syntax.status,
                result.syntax.chunks.len(),
                result.syntax.embedded_regions.len()
            );
        }
        result
    }

    #[cfg(feature = "tree-sitter-syntax")]
    pub fn query_tree_sitter_syntax_range(
        &self,
        buffer_id: i32,
        source_revision: vim_core_rs::CoreBufferRevision,
        range: vim_core_rs::CoreTextRange,
    ) -> Option<vim_core_rs::CoreTreeSitterRangeSyntax> {
        let syntax = self
            .session
            .query_tree_sitter_syntax_range(buffer_id, source_revision, range);
        log::debug!(
            "[core_bridge] queried Tree-sitter syntax range: buffer_id={}, source_revision={:?}, range=({:?}..{:?}), found={}, status={:?}, chunks={}",
            buffer_id,
            source_revision,
            range.start,
            range.end,
            syntax.is_some(),
            syntax.as_ref().map(|syntax| &syntax.status),
            syntax
                .as_ref()
                .map(|syntax| syntax.chunks.len())
                .unwrap_or_default()
        );
        syntax
    }
}

mod outcome_normalization;
mod session_support;

use outcome_normalization::*;
use session_support::*;

#[cfg(test)]
mod tests;
