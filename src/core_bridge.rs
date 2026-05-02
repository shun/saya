use std::fmt;
use std::path::Path;

use vim_core_rs::{
    CoreCommandOutcome, CoreEvent, CoreHostAction, CoreInputResponse, CoreInputResponseError,
    CoreMatchType, CoreMessageCategory, CoreMessageEvent, CoreMessageSeverity, CoreOptionScope,
    CoreSearchHighlightMode, CoreSearchQueryError, CoreSessionError, CoreSnapshot, CoreVfsResponse,
    JobStatus, VimCoreSession,
};

use crate::core_outcome::{
    NormalizedCoreOutcome, NormalizedHostDirective, NormalizedNotification, NormalizedOutcomeBatch,
    NormalizedOutcomeQueue, NormalizedPrompt, NormalizedStructuralOutcome, OutcomeOrigin,
    OutcomeTrace, PromptResponseDisposition, core_event_raw_kind, core_host_action_raw_kind,
    normalize_core_event, normalize_host_action,
};
use crate::core_prompt::{PromptResponseCommand, PromptResponseError, PromptResponseRejection};
use crate::option_registry::{SayaOptionName, SayaOptionValue};
use crate::search_capability::SearchCapabilityContract;
use crate::search_query::{
    SearchMatch, SearchMatchKind, SearchQueryMode, SearchStateError, SearchVisibleQuery,
    SearchVisibleRows, SearchVisibleState,
};

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
}

impl fmt::Debug for CoreBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoreBridge")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

impl CoreBridge {
    pub fn new(initial_text: &str) -> Result<Self, CoreSessionError> {
        log::debug!(
            "[core_bridge] initializing vim-core-rs session: initial_text_len={}",
            initial_text.len()
        );
        let mut session = VimCoreSession::new(initial_text)?;
        configure_message_suppression(&mut session).map_err(CoreSessionError::CommandFailed)?;
        log::debug!("[core_bridge] vim-core-rs session initialized");
        Ok(Self {
            session,
            pending_outcomes: NormalizedOutcomeQueue::default(),
            next_outcome_sequence: 1,
            active_input_correlation_id: None,
            pending_transport_key: None,
        })
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
        let snapshot = self.session.snapshot();
        log::debug!(
            "[core_bridge] snapshot captured: revision={}, dirty={}, pending_host_actions={}",
            snapshot.revision,
            snapshot.dirty,
            snapshot.pending_host_actions
        );
        snapshot
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
        log::debug!(
            "[core_bridge] dispatching key: {:?} (len={})",
            key,
            key.len()
        );
        let outcome = if self.should_handle_ctrl_c_interrupt(&key) {
            self.handle_ctrl_c_interrupt()?
        } else {
            self.dispatch_session_key(&key)?
        };
        log::debug!(
            "[core_bridge] dispatch result: {:?}, pending_input={:?}",
            outcome,
            self.session.snapshot().pending_input
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
        log::debug!("[core_bridge] ex command result: {:?}", outcome.outcome);
        Ok(outcome.outcome)
    }

    pub fn set_core_option(
        &mut self,
        name: SayaOptionName,
        value: SayaOptionValue,
    ) -> Result<(), CoreSessionError> {
        log::debug!(
            "[core_bridge] setting core-owned option through typed API: name={}, value={:?}",
            name,
            value
        );
        let result = match value {
            SayaOptionValue::Boolean(value) => {
                self.session
                    .set_option_bool(name.canonical(), value, CoreOptionScope::Default)
            }
            SayaOptionValue::Number(value) => {
                self.session
                    .set_option_number(name.canonical(), value, CoreOptionScope::Default)
            }
            SayaOptionValue::String(value) => {
                self.session
                    .set_option_string(name.canonical(), &value, CoreOptionScope::Default)
            }
        };
        result.map_err(|error| {
            log::debug!(
                "[core_bridge] core option update failed: name={}, error={:?}",
                name,
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
        let snapshot = self.session.snapshot();
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
        let swapped = self.session.snapshot();
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
}

fn map_match_kind(match_type: CoreMatchType) -> SearchMatchKind {
    match match_type {
        CoreMatchType::Regular => SearchMatchKind::Regular,
        CoreMatchType::IncSearch => SearchMatchKind::Incremental,
        CoreMatchType::CurSearch => SearchMatchKind::Current,
    }
}

fn map_search_mode(mode: CoreSearchHighlightMode) -> SearchQueryMode {
    match mode {
        CoreSearchHighlightMode::Disabled => SearchQueryMode::Disabled,
        CoreSearchHighlightMode::HlSearch => SearchQueryMode::Hlsearch,
        CoreSearchHighlightMode::IncSearch => SearchQueryMode::IncsearchPreview,
    }
}

fn map_search_query_error(error: CoreSearchQueryError) -> SearchStateError {
    match error {
        CoreSearchQueryError::NoActiveWindow => SearchStateError::ActiveWindowMissing,
        CoreSearchQueryError::InvalidViewport { start_row, end_row } => {
            SearchStateError::InvalidViewport {
                start_row: start_row.max(0) as usize,
                end_row: end_row.max(0) as usize,
            }
        }
        CoreSearchQueryError::WindowNotFound { window_id } => {
            SearchStateError::WindowNotFound { window_id }
        }
    }
}

impl CoreBridge {
    fn search_prompt_is_active(&self) -> bool {
        self.session.get_search_input_pattern().is_some()
            || self.session.is_incsearch_active()
            || matches!(
                self.session.snapshot().mode,
                vim_core_rs::CoreMode::CommandLine
            )
    }

    fn should_buffer_transport_prefix(&self, key: &str) -> bool {
        key == "\u{17}"
            && self.pending_transport_key.is_none()
            && matches!(
                self.session.snapshot().mode,
                vim_core_rs::CoreMode::Normal
                    | vim_core_rs::CoreMode::Visual
                    | vim_core_rs::CoreMode::VisualLine
                    | vim_core_rs::CoreMode::VisualBlock
            )
    }

    fn take_pending_transport_key(&mut self) -> Option<String> {
        self.pending_transport_key.take()
    }

    fn should_handle_ctrl_c_interrupt(&self, key: &str) -> bool {
        key == "\u{3}"
            && matches!(
                self.session.snapshot().mode,
                vim_core_rs::CoreMode::Normal
                    | vim_core_rs::CoreMode::Visual
                    | vim_core_rs::CoreMode::VisualLine
                    | vim_core_rs::CoreMode::VisualBlock
            )
    }

    fn handle_ctrl_c_interrupt(&mut self) -> Result<CoreCommandOutcome, CoreSessionError> {
        let snapshot = self.session.snapshot();
        let has_pending_input = snapshot.pending_input.is_pending();
        log::debug!(
            "[core_bridge] handling ctrl-c interrupt: dirty={}, mode={:?}, has_pending_input={}",
            snapshot.dirty,
            snapshot.mode,
            has_pending_input
        );

        if has_pending_input {
            return self.dispatch_session_key("\x1b");
        }

        let content = if snapshot.dirty {
            "Type  :qa!  and press <Enter> to abandon all changes and exit Vim"
        } else {
            "Type  :qa  and press <Enter> to exit Vim"
        };
        let event = CoreMessageEvent {
            severity: CoreMessageSeverity::Info,
            category: CoreMessageCategory::UserVisible,
            content: content.to_string(),
        };
        let trace = self.next_outcome_trace(
            OutcomeOrigin::PendingSessionEvent,
            "CoreBridge::CtrlCExitGuidance",
        );
        self.enqueue_normalized_outcome(NormalizedCoreOutcome::Notification(
            NormalizedNotification::Message { event, trace },
        ));

        Ok(CoreCommandOutcome::NoChange)
    }

    fn dispatch_session_key(&mut self, key: &str) -> Result<CoreCommandOutcome, CoreSessionError> {
        let outcome = self
            .session
            .dispatch_key(key)
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&outcome);
        Ok(outcome.outcome)
    }

    fn queue_transaction_artifacts(&mut self, tx: &vim_core_rs::CoreCommandTransaction) {
        for action in &tx.host_actions {
            self.queue_host_action(action, OutcomeOrigin::TransactionHostAction);
        }

        for event in &tx.events {
            self.queue_core_event(event, OutcomeOrigin::TransactionEvent);
        }
    }

    fn collect_transaction_artifacts_into(
        &mut self,
        tx: &vim_core_rs::CoreCommandTransaction,
        outcomes: &mut Vec<NormalizedCoreOutcome>,
    ) {
        for action in &tx.host_actions {
            let raw_kind = core_host_action_raw_kind(action);
            let trace = self.next_outcome_trace(OutcomeOrigin::TransactionHostAction, raw_kind);
            log::debug!(
                "[core_bridge] normalized prompt-response transaction host action: raw_kind={}, action={:?}",
                raw_kind,
                action
            );
            outcomes.push(normalize_host_action(action, trace));
            if let CoreHostAction::RequestInput { correlation_id, .. } = action {
                log::debug!(
                    "[core_bridge] recorded active input prompt correlation from response transaction: correlation_id={}",
                    correlation_id
                );
                self.active_input_correlation_id = Some(*correlation_id);
            }
        }

        for event in &tx.events {
            let raw_kind = core_event_raw_kind(event);
            let trace = self.next_outcome_trace(OutcomeOrigin::TransactionEvent, raw_kind);
            log::debug!(
                "[core_bridge] normalized prompt-response transaction event: raw_kind={}, event={:?}",
                raw_kind,
                event
            );
            outcomes.push(normalize_core_event(event, trace));
        }
    }

    fn drain_pending_host_actions_from_session(&mut self) {
        while let Some(action) = self.session.take_pending_host_action() {
            self.queue_host_action(&action, OutcomeOrigin::PendingSessionHostAction);
        }
    }

    fn drain_pending_events_from_session(&mut self) {
        while let Some(event) = self.session.take_pending_event() {
            self.queue_core_event(&event, OutcomeOrigin::PendingSessionEvent);
        }
    }

    fn queue_host_action(&mut self, action: &CoreHostAction, origin: OutcomeOrigin) {
        let raw_kind = core_host_action_raw_kind(action);
        log::debug!(
            "[core_bridge] queued host action: origin={:?}, raw_kind={}, action={:?}",
            origin,
            raw_kind,
            action
        );
        let trace = self.next_outcome_trace(origin, raw_kind);
        self.enqueue_normalized_outcome(normalize_host_action(action, trace));
        if let CoreHostAction::RequestInput { correlation_id, .. } = action {
            log::debug!(
                "[core_bridge] recorded active input prompt correlation: correlation_id={}",
                correlation_id
            );
            self.active_input_correlation_id = Some(*correlation_id);
        }
    }

    fn queue_core_event(&mut self, event: &CoreEvent, origin: OutcomeOrigin) {
        let raw_kind = core_event_raw_kind(event);
        let trace = self.next_outcome_trace(origin, raw_kind);
        self.enqueue_normalized_outcome(normalize_core_event(event, trace));
        match event {
            CoreEvent::Message(message) => {
                log::debug!(
                    "[core_bridge] queued core message: origin={:?}, severity={:?}, category={:?}, content={:?}",
                    origin,
                    message.severity,
                    message.category,
                    message.content
                );
            }
            CoreEvent::Redraw {
                full,
                clear_before_draw,
            } => {
                log::debug!(
                    "[core_bridge] queued redraw request: origin={:?}, full={}, clear_before_draw={}",
                    origin,
                    full,
                    clear_before_draw
                );
            }
            other => {
                log::debug!(
                    "[core_bridge] queued normalized-only core event: origin={:?}, raw_kind={}, event={:?}",
                    origin,
                    raw_kind,
                    other
                );
            }
        }
    }

    fn next_outcome_trace(
        &mut self,
        origin: OutcomeOrigin,
        raw_kind: &'static str,
    ) -> OutcomeTrace {
        let trace = OutcomeTrace {
            sequence: self.next_outcome_sequence,
            origin,
            raw_kind,
        };
        self.next_outcome_sequence += 1;
        trace
    }

    fn enqueue_normalized_outcome(&mut self, outcome: NormalizedCoreOutcome) {
        let trace = *outcome.trace();
        log::debug!(
            "[core_bridge] append normalized outcome: sequence={}, origin={:?}, raw_kind={}",
            trace.sequence,
            trace.origin,
            trace.raw_kind
        );
        self.pending_outcomes.push_back(outcome);
    }
}

fn configure_message_suppression(
    session: &mut VimCoreSession,
) -> Result<(), vim_core_rs::CoreCommandError> {
    log::debug!("[core_bridge] configuring Vim message suppression: report=999999, shortmess+=F");
    session.execute_ex_command(":set report=999999 shortmess+=F")?;
    Ok(())
}

fn is_visual_mode(mode: vim_core_rs::CoreMode) -> bool {
    matches!(
        mode,
        vim_core_rs::CoreMode::Visual
            | vim_core_rs::CoreMode::VisualLine
            | vim_core_rs::CoreMode::VisualBlock
    )
}

fn normalize_selection_bounds(
    anchor: (usize, usize),
    cursor: (usize, usize),
) -> ((usize, usize), (usize, usize)) {
    if anchor <= cursor {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    }
}

fn map_input_response_error(error: CoreInputResponseError) -> PromptResponseError {
    match error {
        CoreInputResponseError::NoPendingInput => {
            PromptResponseError::CoreRejected(PromptResponseRejection::NoPendingInput)
        }
        CoreInputResponseError::CorrelationMismatch { expected, actual } => {
            PromptResponseError::CoreRejected(PromptResponseRejection::CoreCorrelationMismatch {
                expected,
                actual,
            })
        }
        CoreInputResponseError::Command(error) => PromptResponseError::CoreRejected(
            PromptResponseRejection::CommandRejected(format!("{error:?}")),
        ),
        CoreInputResponseError::EvalFailed => {
            PromptResponseError::CoreRejected(PromptResponseRejection::CommandRejected(
                "core eval failed after input response".to_string(),
            ))
        }
    }
}

fn legacy_host_action_from_normalized(outcome: NormalizedCoreOutcome) -> Option<CoreHostAction> {
    match outcome {
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Write {
            path,
            force,
            issued_after_revision,
            trace,
        }) => {
            log::debug!(
                "[core_bridge] projected legacy write host action: sequence={}, revision={}",
                trace.sequence,
                issued_after_revision
            );
            Some(CoreHostAction::Write {
                path,
                force,
                issued_after_revision,
            })
        }
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Quit {
            force,
            issued_after_revision,
            trace,
        }) => {
            log::debug!(
                "[core_bridge] projected legacy quit host action: sequence={}, revision={}",
                trace.sequence,
                issued_after_revision
            );
            Some(CoreHostAction::Quit {
                force,
                issued_after_revision,
            })
        }
        NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::VfsRequest {
            request,
            trace,
        }) => {
            log::debug!(
                "[core_bridge] projected legacy vfs host action: sequence={}",
                trace.sequence
            );
            Some(CoreHostAction::VfsRequest(request))
        }
        _ => None,
    }
}

fn legacy_message_from_normalized(outcome: NormalizedCoreOutcome) -> Option<CoreMessageEvent> {
    match outcome {
        NormalizedCoreOutcome::Notification(NormalizedNotification::Message { event, trace }) => {
            log::debug!(
                "[core_bridge] projected legacy message: sequence={}, severity={:?}, category={:?}",
                trace.sequence,
                event.severity,
                event.category
            );
            Some(event)
        }
        _ => None,
    }
}

fn legacy_redraw_request_from_normalized(
    outcome: NormalizedCoreOutcome,
) -> Option<PendingRedrawRequest> {
    match outcome {
        NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
            full,
            clear_before_draw,
            trace,
        }) => {
            log::debug!(
                "[core_bridge] projected legacy redraw request: sequence={}, full={}, clear_before_draw={}",
                trace.sequence,
                full,
                clear_before_draw
            );
            Some(PendingRedrawRequest {
                full,
                clear_before_draw,
            })
        }
        _ => None,
    }
}

fn escape_path_for_file_command(target_path: &Path) -> String {
    let mut escaped = String::new();
    for ch in target_path.to_string_lossy().chars() {
        if matches!(ch, ' ' | '\\' | '|' | '"' | '%' | '#' | '<' | '>') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use vim_core_rs::{
        CoreCommandOutcome, CoreCommandTransaction, CoreEvent, CoreHostAction, CoreMessageCategory,
        CoreMessageEvent, CoreMessageSeverity, CoreMode, CorePendingInput,
    };

    use super::CoreBridge;
    use crate::core_outcome::{
        ApplicationOutcomeState, NormalizedCoreOutcome, NormalizedHostDirective,
        NormalizedNotification, NormalizedOutcomeBatch, NormalizedPrompt,
        NormalizedStructuralOutcome, OutcomeOrigin, PromptInputTransition,
        PromptResponseDisposition, fold_normalized_outcomes,
    };
    use crate::core_prompt::{PromptResponseCommand, PromptResponseError};

    use crate::session_guard::test_lock as session_test_lock;

    fn unique_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("saya-core-bridge-{name}-{nanos}.txt"))
    }

    #[test]
    fn initializes_vim_core_session_and_returns_initial_snapshot() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("").expect("core bridge should initialize");
        let snapshot = bridge.snapshot();

        assert_eq!(snapshot.text, "\n");
        assert_eq!(snapshot.revision, 0);
        assert!(!snapshot.dirty);
        assert_eq!(snapshot.mode, CoreMode::Normal);
    }

    #[test]
    fn starts_with_no_pending_host_actions() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");

        assert!(bridge.take_pending_host_actions().is_empty());
    }

    #[test]
    fn starts_with_no_pending_core_messages() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");

        assert!(bridge.take_pending_messages().is_empty());
    }

    #[test]
    fn starts_with_no_pending_redraw_requests() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");

        assert!(bridge.take_pending_redraw_requests().is_empty());
    }

    #[test]
    fn normalized_outcomes_preserve_transaction_total_order_and_sequence() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");
        let tx = CoreCommandTransaction {
            outcome: CoreCommandOutcome::NoChange,
            snapshot: bridge.snapshot(),
            host_actions: vec![
                CoreHostAction::Write {
                    path: "notes.txt".to_string(),
                    force: false,
                    issued_after_revision: 1,
                },
                CoreHostAction::Bell,
            ],
            events: vec![
                CoreEvent::Message(CoreMessageEvent {
                    severity: CoreMessageSeverity::Info,
                    category: CoreMessageCategory::UserVisible,
                    content: "written".to_string(),
                }),
                CoreEvent::Redraw {
                    full: false,
                    clear_before_draw: true,
                },
            ],
        };

        bridge.queue_transaction_artifacts(&tx);
        let batch = bridge.take_normalized_outcomes();

        let traces = batch
            .outcomes()
            .iter()
            .map(|outcome| *outcome.trace())
            .collect::<Vec<_>>();
        assert_eq!(
            traces
                .iter()
                .map(|trace| trace.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(
            traces.iter().map(|trace| trace.origin).collect::<Vec<_>>(),
            vec![
                OutcomeOrigin::TransactionHostAction,
                OutcomeOrigin::TransactionHostAction,
                OutcomeOrigin::TransactionEvent,
                OutcomeOrigin::TransactionEvent,
            ]
        );
        assert_eq!(
            traces
                .iter()
                .map(|trace| trace.raw_kind)
                .collect::<Vec<_>>(),
            vec![
                "CoreHostAction::Write",
                "CoreHostAction::Bell",
                "CoreEvent::Message",
                "CoreEvent::Redraw",
            ]
        );
        assert!(matches!(
            batch.outcomes()[0],
            NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Write { .. })
        ));
        assert!(matches!(
            batch.outcomes()[1],
            NormalizedCoreOutcome::Notification(NormalizedNotification::Bell { .. })
        ));
        assert!(matches!(
            batch.outcomes()[3],
            NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
                full: false,
                clear_before_draw: true,
                ..
            })
        ));
    }

    #[test]
    fn normalized_outcomes_append_pending_session_state_after_transaction_payload() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");
        let tx = CoreCommandTransaction {
            outcome: CoreCommandOutcome::NoChange,
            snapshot: bridge.snapshot(),
            host_actions: vec![CoreHostAction::Write {
                path: "notes.txt".to_string(),
                force: false,
                issued_after_revision: 1,
            }],
            events: vec![CoreEvent::Message(CoreMessageEvent {
                severity: CoreMessageSeverity::Info,
                category: CoreMessageCategory::UserVisible,
                content: "transaction message".to_string(),
            })],
        };

        bridge.queue_transaction_artifacts(&tx);
        bridge.queue_host_action(
            &CoreHostAction::Quit {
                force: true,
                issued_after_revision: 2,
            },
            OutcomeOrigin::PendingSessionHostAction,
        );
        bridge.queue_core_event(
            &CoreEvent::Redraw {
                full: true,
                clear_before_draw: false,
            },
            OutcomeOrigin::PendingSessionEvent,
        );

        let batch = bridge.take_normalized_outcomes();
        let traces = batch
            .outcomes()
            .iter()
            .map(|outcome| *outcome.trace())
            .collect::<Vec<_>>();

        assert_eq!(
            traces
                .iter()
                .map(|trace| trace.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(
            traces.iter().map(|trace| trace.origin).collect::<Vec<_>>(),
            vec![
                OutcomeOrigin::TransactionHostAction,
                OutcomeOrigin::TransactionEvent,
                OutcomeOrigin::PendingSessionHostAction,
                OutcomeOrigin::PendingSessionEvent,
            ]
        );
    }

    #[test]
    fn take_normalized_outcomes_moves_batch_and_clears_bridge_queue() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");
        let tx = CoreCommandTransaction {
            outcome: CoreCommandOutcome::NoChange,
            snapshot: bridge.snapshot(),
            host_actions: vec![CoreHostAction::Quit {
                force: true,
                issued_after_revision: 1,
            }],
            events: vec![],
        };

        bridge.queue_transaction_artifacts(&tx);

        assert_eq!(bridge.take_normalized_outcomes().outcomes().len(), 1);
        assert!(
            bridge.take_normalized_outcomes().is_empty(),
            "normalized outcome drain must clear the bridge queue"
        );
    }

    #[test]
    fn respond_to_prompt_returns_completion_batch_that_folds_active_prompt_closed() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");

        bridge
            .apply_ex_command(":input Name")
            .expect("input request should be queued");
        let request_batch = bridge.take_normalized_outcomes();
        assert!(matches!(
            request_batch.outcomes().first(),
            Some(NormalizedCoreOutcome::Prompt(
                NormalizedPrompt::RequestInput {
                    correlation_id: 1,
                    ..
                }
            ))
        ));
        let requested = fold_normalized_outcomes(request_batch, ApplicationOutcomeState::default());
        assert!(matches!(
            requested.effects.prompt.input_transition,
            Some(PromptInputTransition::Requested { .. })
        ));

        let response_batch = bridge
            .respond_to_prompt(PromptResponseCommand::Submit {
                correlation_id: 1,
                value: "alice".to_string(),
            })
            .expect("prompt response should be accepted");

        assert!(matches!(
            response_batch.outcomes().first(),
            Some(NormalizedCoreOutcome::Prompt(
                NormalizedPrompt::InputResponseAccepted {
                    correlation_id: 1,
                    disposition: PromptResponseDisposition::Submitted,
                    ..
                }
            ))
        ));

        let completed = fold_normalized_outcomes(response_batch, requested.state);
        assert!(completed.state.prompt.active_input.is_none());
        assert!(matches!(
            completed.effects.prompt.input_transition,
            Some(PromptInputTransition::Submitted { correlation_id: 1 })
        ));
        assert!(matches!(
            bridge.respond_to_prompt(PromptResponseCommand::Cancel { correlation_id: 1 }),
            Err(PromptResponseError::NoActivePrompt)
        ));
    }

    #[test]
    fn respond_to_prompt_rejects_missing_and_mismatched_active_prompt_without_clearing_it() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");

        assert!(matches!(
            bridge.respond_to_prompt(PromptResponseCommand::Cancel { correlation_id: 1 }),
            Err(PromptResponseError::NoActivePrompt)
        ));

        bridge
            .apply_ex_command(":input Name")
            .expect("input request should be queued");
        let _ = bridge.take_normalized_outcomes();

        assert!(matches!(
            bridge.respond_to_prompt(PromptResponseCommand::Cancel { correlation_id: 2 }),
            Err(PromptResponseError::CorrelationMismatch {
                expected: 1,
                actual: 2
            })
        ));

        assert!(
            bridge
                .respond_to_prompt(PromptResponseCommand::Cancel { correlation_id: 1 })
                .is_ok(),
            "mismatch must not clear the bridge active input correlation"
        );
    }

    #[test]
    fn legacy_message_projection_consumes_only_projected_normalized_outcome() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");
        let tx = CoreCommandTransaction {
            outcome: CoreCommandOutcome::NoChange,
            snapshot: bridge.snapshot(),
            host_actions: vec![CoreHostAction::Write {
                path: "notes.txt".to_string(),
                force: false,
                issued_after_revision: 1,
            }],
            events: vec![CoreEvent::Message(CoreMessageEvent {
                severity: CoreMessageSeverity::Info,
                category: CoreMessageCategory::UserVisible,
                content: "written".to_string(),
            })],
        };

        bridge.queue_transaction_artifacts(&tx);

        let messages = bridge.take_pending_messages();
        assert_eq!(messages.len(), 1);

        let remaining = bridge.take_normalized_outcomes();
        assert_eq!(
            remaining.outcomes().len(),
            1,
            "legacy message projection must not duplicate consumed messages or drop host directives"
        );
        assert!(matches!(
            remaining.outcomes()[0],
            NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Write { .. })
        ));
    }

    #[test]
    fn legacy_host_action_projection_consumes_host_directives_from_normalized_queue() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");
        let tx = CoreCommandTransaction {
            outcome: CoreCommandOutcome::NoChange,
            snapshot: bridge.snapshot(),
            host_actions: vec![CoreHostAction::Quit {
                force: true,
                issued_after_revision: 1,
            }],
            events: vec![CoreEvent::Redraw {
                full: true,
                clear_before_draw: false,
            }],
        };

        bridge.queue_transaction_artifacts(&tx);

        let actions = bridge.take_pending_host_actions();
        assert_eq!(actions.len(), 1);

        let remaining = bridge.take_normalized_outcomes();
        assert_eq!(
            remaining.outcomes().len(),
            1,
            "legacy host projection must leave non-host normalized outcomes queued"
        );
        assert!(matches!(
            remaining.outcomes()[0],
            NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
                full: true,
                ..
            })
        ));
    }

    #[test]
    fn headless_application_regression_folds_core_foundation_without_effect_replay() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");
        let mut folded_state = ApplicationOutcomeState::default();

        bridge
            .apply_ex_command(":input Name")
            .expect("input request should be queued");
        let requested = fold_normalized_outcomes(bridge.take_normalized_outcomes(), folded_state);
        folded_state = requested.state;
        assert!(matches!(
            requested.effects.prompt.input_transition,
            Some(PromptInputTransition::Requested { .. })
        ));
        assert_eq!(
            folded_state
                .prompt
                .active_input
                .as_ref()
                .map(|session| session.correlation_id),
            Some(1)
        );

        let response_batch = bridge
            .respond_to_prompt(PromptResponseCommand::Cancel { correlation_id: 1 })
            .expect("prompt cancel should be accepted");
        let cancelled = fold_normalized_outcomes(response_batch, folded_state);
        folded_state = cancelled.state;
        assert!(folded_state.prompt.active_input.is_none());
        assert!(matches!(
            cancelled.effects.prompt.input_transition,
            Some(PromptInputTransition::Cancelled { correlation_id: 1 })
        ));

        let tx = CoreCommandTransaction {
            outcome: CoreCommandOutcome::NoChange,
            snapshot: bridge.snapshot(),
            host_actions: vec![
                CoreHostAction::Write {
                    path: "notes.txt".to_string(),
                    force: false,
                    issued_after_revision: 1,
                },
                CoreHostAction::Quit {
                    force: false,
                    issued_after_revision: 1,
                },
            ],
            events: vec![
                CoreEvent::Message(CoreMessageEvent {
                    severity: CoreMessageSeverity::Info,
                    category: CoreMessageCategory::UserVisible,
                    content: "saved".to_string(),
                }),
                CoreEvent::Redraw {
                    full: false,
                    clear_before_draw: true,
                },
                CoreEvent::BufferAdded { buf_id: 3 },
                CoreEvent::LayoutChanged,
            ],
        };

        bridge.queue_transaction_artifacts(&tx);
        let folded = fold_normalized_outcomes(bridge.take_normalized_outcomes(), folded_state);

        assert!(matches!(
            folded.effects.host_directives.as_slice(),
            [
                NormalizedHostDirective::Write { .. },
                NormalizedHostDirective::Quit { .. }
            ]
        ));
        assert_eq!(
            folded
                .effects
                .notification
                .latest_user_visible_message
                .as_ref()
                .map(|message| message.content.as_str()),
            Some("saved")
        );
        assert_eq!(folded.effects.structural.invalidate_buffers, vec![3]);
        assert!(folded.effects.structural.layout_dirty);
        assert_eq!(
            folded.effects.structural.redraw.map(|redraw| {
                (
                    redraw.full,
                    redraw.clear_before_draw,
                    redraw.required_by_structure_change,
                )
            }),
            Some((true, true, true))
        );

        let replay = fold_normalized_outcomes(NormalizedOutcomeBatch::default(), folded.state);

        assert!(replay.effects.host_directives.is_empty());
        assert!(
            replay
                .effects
                .notification
                .latest_user_visible_message
                .is_none()
        );
        assert!(replay.effects.prompt.input_transition.is_none());
        assert!(replay.effects.structural.redraw.is_none());
        assert!(
            bridge.take_normalized_outcomes().is_empty(),
            "headless application regression must leave the bridge drain contract clear"
        );
    }

    #[test]
    fn initializes_empty_buffer_for_new_file_scenario() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("").expect("core bridge should initialize with empty text");
        let snapshot = bridge.snapshot();

        assert_eq!(snapshot.text, "\n");
        assert!(!snapshot.dirty);
        assert_eq!(snapshot.mode, CoreMode::Normal);
    }

    #[test]
    fn allows_attaching_target_path_to_empty_buffer_after_creation() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let save_path = unique_path("new-file-save");

        let mut bridge = CoreBridge::new("").expect("core bridge should initialize");
        bridge
            .attach_target_path(&save_path)
            .expect("should attach target path to empty buffer");
        let snapshot = bridge.snapshot();

        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .expect("active buffer should exist");

        assert_eq!(active_buffer.name, save_path.display().to_string());
        assert_eq!(snapshot.text, "\n");
        assert!(!snapshot.dirty);
    }

    #[test]
    fn initializes_existing_file_session_with_target_path_as_buffer_name() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("target file");

        let bridge = CoreBridge::new_with_target_path(&target_path, "buffer text\n")
            .expect("core bridge should initialize existing file session");
        let snapshot = bridge.snapshot();
        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .expect("active buffer should exist");

        assert_eq!(active_buffer.name, target_path.display().to_string());
        assert_eq!(snapshot.text, "buffer text\n");
        assert!(!snapshot.dirty);
        assert_eq!(snapshot.mode, CoreMode::Normal);
    }

    #[test]
    fn message_handler_captures_echoerr_messages() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        bridge
            .apply_ex_command("echoerr 'test error message'")
            .expect("echoerr command should complete");
        let messages = bridge.take_pending_messages();

        assert!(
            messages.iter().any(|message| {
                message.severity == CoreMessageSeverity::Error
                    && message.category == CoreMessageCategory::UserVisible
                    && message.content.contains("test error message")
            }),
            "echoerr message should be queued: {:?}",
            messages
        );
    }

    #[test]
    fn dispatch_key_queues_pending_redraw_request() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        bridge
            .apply_ex_command(":redraw")
            .expect(":redraw should succeed");
        let redraws = bridge.take_pending_redraw_requests();

        assert!(
            !redraws.is_empty(),
            ":redraw should enqueue at least one redraw request: {:?}",
            redraws
        );
    }

    #[test]
    fn dirty_ctrl_c_queues_upstream_exit_guidance_message() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("X").expect("typed input");
        bridge.dispatch_key("\x1b").expect("normal mode");

        bridge
            .dispatch_key("\u{3}")
            .expect("ctrl-c should dispatch");
        let messages = bridge.take_pending_messages();

        assert!(
            messages
                .iter()
                .any(|message| message.content.contains(":qa!")),
            "ctrl-c guidance should mention :qa!: {:?}",
            messages
        );
    }

    // ---- タスク 4.1: モード遷移テスト ----

    #[test]
    fn starts_in_normal_mode() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");
        let snapshot = bridge.snapshot();

        assert_eq!(
            snapshot.mode,
            CoreMode::Normal,
            "起動時はノーマルモードであること"
        );
    }

    #[test]
    fn transitions_to_insert_mode_with_i_key() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        let result = bridge.dispatch_key("i");
        assert!(result.is_ok(), "i キーの dispatch は成功すること");

        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.mode,
            CoreMode::Insert,
            "i キーでインサートモードに遷移すること"
        );
    }

    #[test]
    fn transitions_back_to_normal_mode_with_escape() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        bridge.dispatch_key("i").expect("i キーで insert 遷移");
        assert_eq!(bridge.snapshot().mode, CoreMode::Insert);

        bridge
            .dispatch_key("\x1b")
            .expect("Escape キーで normal 復帰");
        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.mode,
            CoreMode::Normal,
            "Escape でノーマルモードに復帰すること"
        );
    }

    #[test]
    fn current_mode_is_available_from_snapshot_after_dispatch() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("test\n").expect("core bridge should initialize");

        // ノーマルモード確認
        assert_eq!(bridge.snapshot().mode, CoreMode::Normal);

        // インサートモードへ
        bridge.dispatch_key("i").expect("insert mode");
        assert_eq!(bridge.snapshot().mode, CoreMode::Insert);

        // ノーマルモードへ戻る
        bridge.dispatch_key("\x1b").expect("normal mode");
        assert_eq!(bridge.snapshot().mode, CoreMode::Normal);
    }

    // ---- タスク 4.2: カーソル移動テスト ----

    #[test]
    fn cursor_moves_down_with_j_key() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first line\nsecond line\n").expect("core bridge should initialize");
        let initial = bridge.snapshot();
        assert_eq!(initial.cursor_row, 0, "初期カーソル行は 0");
        assert_eq!(initial.cursor_col, 0, "初期カーソル列は 0");

        bridge.dispatch_key("j").expect("j キーで下移動");
        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.cursor_row, 1,
            "j キーでカーソルが 1 行下に移動すること"
        );
        assert_eq!(snapshot.cursor_col, 0, "j キーで列は変わらないこと");
    }

    #[test]
    fn cursor_moves_right_with_l_key() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        bridge.dispatch_key("l").expect("l キーで右移動");
        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.cursor_col, 1, "l キーでカーソルが右に移動すること");
        assert_eq!(snapshot.cursor_row, 0, "l キーで行は変わらないこと");
    }

    #[test]
    fn cursor_moves_left_with_h_key() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        // まず右に移動してから左に戻る
        bridge.dispatch_key("ll").expect("l で右に 2 回移動");
        assert_eq!(bridge.snapshot().cursor_col, 2);

        bridge.dispatch_key("h").expect("h キーで左移動");
        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.cursor_col, 1, "h キーでカーソルが左に移動すること");
    }

    #[test]
    fn cursor_moves_up_with_k_key() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");

        bridge.dispatch_key("jj").expect("j で 2 行下に移動");
        assert_eq!(bridge.snapshot().cursor_row, 2);
        bridge.dispatch_key("ll").expect("l で 2 列右に移動");
        assert_eq!(bridge.snapshot().cursor_col, 2);

        bridge.dispatch_key("k").expect("k キーで上移動");
        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.cursor_row, 1, "k キーでカーソルが上に移動すること");
    }

    #[test]
    fn cursor_position_reflected_in_snapshot_after_multiple_moves() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("abcde\nfghij\nklmno\n").expect("core bridge should initialize");

        bridge
            .dispatch_key("jll")
            .expect("j で 1 行下、ll で 2 列右");
        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.cursor_row, 1, "複合移動後の行位置");
        assert_eq!(snapshot.cursor_col, 2, "複合移動後の列位置");
    }

    // ---- タスク 4.3: インサートモード文字入力テスト ----

    #[test]
    fn insert_mode_text_input_appears_in_buffer() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("").expect("core bridge should initialize");

        // i でインサートモードに入り、文字を入力して Esc で戻る
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("H").expect("H を入力");
        bridge.dispatch_key("i").expect("i を入力");
        bridge
            .dispatch_key("\x1b")
            .expect("Escape でノーマルモードに復帰");

        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.mode, CoreMode::Normal, "ノーマルモードに復帰");
        assert!(
            snapshot.text.contains("Hi"),
            "入力した文字 'Hi' がバッファに含まれること: actual={:?}",
            snapshot.text
        );
    }

    #[test]
    fn insert_mode_marks_buffer_dirty() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("").expect("core bridge should initialize");

        assert!(!bridge.snapshot().dirty, "初期状態は dirty でないこと");

        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("a").expect("a を入力");
        bridge.dispatch_key("\x1b").expect("normal mode");

        let snapshot = bridge.snapshot();
        assert!(
            snapshot.dirty,
            "インサートモードで文字入力後は dirty になること"
        );
    }

    #[test]
    fn insert_mode_text_input_updates_buffer_for_redraw() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("line1\n").expect("core bridge should initialize");

        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("X").expect("X を入力");
        bridge.dispatch_key("\x1b").expect("normal mode");

        let snapshot = bridge.snapshot();
        assert!(
            snapshot.text.starts_with("X"),
            "先頭に X が挿入されること: actual={:?}",
            snapshot.text
        );
        log::debug!(
            "[test] insert 後のバッファ内容（再描画用）: {:?}",
            snapshot.text
        );
    }

    // ---- タスク 4.4: 削除操作テスト ----

    #[test]
    fn x_key_deletes_character_at_cursor() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("abcde\n").expect("core bridge should initialize");

        bridge.dispatch_key("x").expect("x キーで文字削除");
        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.text, "bcde\n",
            "x キーで先頭の 'a' が削除されること"
        );
    }

    #[test]
    fn x_key_marks_buffer_dirty() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        assert!(!bridge.snapshot().dirty, "初期状態は dirty でないこと");

        bridge.dispatch_key("x").expect("x キーで削除");
        let snapshot = bridge.snapshot();
        assert!(snapshot.dirty, "削除後は dirty になること");
    }

    #[test]
    fn dd_deletes_entire_line() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");

        bridge.dispatch_key("dd").expect("dd で行削除");
        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.text, "second\nthird\n",
            "dd で最初の行が削除されること"
        );
        assert!(snapshot.dirty, "dd 後は dirty になること");
    }

    #[test]
    fn sequential_multi_key_pending_input_is_forwarded_through_core_dispatch() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");

        let first = bridge
            .dispatch_key("y")
            .expect("first key should be forwarded to core");
        assert_eq!(first, CoreCommandOutcome::NoChange);
        assert_eq!(
            bridge.snapshot().pending_input.pending_keys,
            "y",
            "pending input state should come from vim-core-rs"
        );

        bridge
            .dispatch_key("y")
            .expect("second key should complete the sequence");

        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.text, "first\nsecond\nthird\n",
            "yy itself should not change the buffer"
        );
        assert_eq!(
            snapshot.pending_input,
            CorePendingInput::none(),
            "completed sequence should leave no bridge-side pending parser state"
        );
    }

    #[test]
    fn ctrl_c_cancels_core_owned_pending_input_without_showing_exit_guidance() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("first\nsecond\n").expect("core bridge should initialize");

        bridge.dispatch_key("d").expect("enter operator pending");
        assert!(
            bridge.snapshot().pending_input.is_pending(),
            "pending input should be reported by vim-core-rs before ctrl-c"
        );

        let outcome = bridge
            .dispatch_key("\u{3}")
            .expect("ctrl-c should cancel pending input");
        assert_eq!(outcome, CoreCommandOutcome::NoChange);
        assert_eq!(
            bridge.snapshot().pending_input,
            CorePendingInput::none(),
            "ctrl-c should clear core pending input via escape dispatch"
        );
        assert!(
            bridge.take_pending_messages().is_empty(),
            "canceling pending input should not enqueue exit guidance"
        );
    }

    #[test]
    fn ctrl_w_prefix_is_coalesced_across_separate_dispatch_calls() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");
        bridge.set_screen_size(24, 80);

        let first = bridge
            .dispatch_key("\u{17}")
            .expect("ctrl-w prefix should be accepted");
        assert_eq!(first, CoreCommandOutcome::NoChange);
        assert_eq!(
            bridge.snapshot().windows.len(),
            1,
            "buffering the transport prefix alone should not mutate layout yet"
        );

        bridge
            .dispatch_key("s")
            .expect("second key should complete the ctrl-w sequence");

        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.windows.len(),
            2,
            "Ctrl-w followed by s in separate dispatch calls should create a split"
        );
    }

    #[test]
    fn new_configures_high_report_threshold_to_suppress_bulk_edit_messages() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");
        assert!(
            bridge
                .session
                .eval_string("&report")
                .as_deref()
                .is_some_and(|report| report.trim() == "999999"),
            "複数行操作の報告メッセージ抑制のため report が引き上げられていること"
        );
    }

    #[test]
    fn sequential_dd_deletes_current_line_via_core_owned_pending_input() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");

        bridge
            .dispatch_key("d")
            .expect("first d should enter operator pending");
        bridge
            .dispatch_key("d")
            .expect("second d should delete current line");

        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.text, "second\nthird\n",
            "dd を逐次入力しても現在行が削除されること"
        );
    }

    #[test]
    fn insert_mode_keeps_literal_text_literal_at_bridge_boundary() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("").expect("core bridge should initialize");

        bridge.dispatch_key("i").expect("enter insert mode");
        bridge
            .dispatch_key("2")
            .expect("insert literal count digit");
        bridge
            .dispatch_key("d")
            .expect("insert literal operator key");
        bridge
            .dispatch_key("g")
            .expect("insert literal normal prefix key");
        bridge.dispatch_key("\x1b").expect("leave insert mode");

        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.mode, CoreMode::Normal);
        assert_eq!(snapshot.text, "2dg\n");
    }

    #[test]
    fn delete_maintains_dirty_state_across_operations() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("abc\ndef\n").expect("core bridge should initialize");

        bridge.dispatch_key("x").expect("最初の x で削除");
        assert!(bridge.snapshot().dirty, "最初の削除後は dirty");

        bridge.dispatch_key("x").expect("2 回目の x で削除");
        let snapshot = bridge.snapshot();
        assert!(
            snapshot.dirty,
            "複数回の削除操作後も dirty 状態が維持されること"
        );
        assert_eq!(snapshot.text, "c\ndef\n", "2 文字削除後のバッファ内容");
    }

    // ---- タスク 5.1: 保存要求（:w）でホストアクション Write が発行されるテスト ----

    #[test]
    fn write_command_produces_write_host_action() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("write-host-action");

        let mut bridge = CoreBridge::new_with_target_path(&target_path, "content\n")
            .expect("core bridge should initialize");

        // :w を実行
        bridge
            .apply_ex_command(":w")
            .expect(":w コマンドは成功すること");

        let actions = bridge.take_pending_host_actions();
        log::debug!("[test] host actions after :w: {:?}", actions);
        let has_write = actions
            .iter()
            .any(|a| matches!(a, vim_core_rs::CoreHostAction::Write { .. }));
        assert!(
            has_write,
            ":w 実行後に Write ホストアクションが発行されること: actions={:?}",
            actions
        );
    }

    #[test]
    fn buffer_text_returns_current_contents() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        let text = bridge.buffer_text();
        assert_eq!(text, "hello\n", "buffer_text が現在の内容を返すこと");

        // 編集後もテキストが更新されること
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("X").expect("X を入力");
        bridge.dispatch_key("\x1b").expect("normal mode");

        let text_after = bridge.buffer_text();
        assert!(
            text_after.contains("X"),
            "編集後の buffer_text に入力文字が含まれること: {:?}",
            text_after
        );
    }

    // ---- タスク 5.4: 終了要求（:q, :q!）でホストアクション Quit が発行されるテスト ----

    #[test]
    fn quit_command_produces_quit_host_action() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("content\n").expect("core bridge should initialize");

        bridge
            .apply_ex_command(":q")
            .expect(":q コマンドは成功すること");

        let actions = bridge.take_pending_host_actions();
        log::debug!("[test] host actions after :q: {:?}", actions);
        let has_quit = actions
            .iter()
            .any(|a| matches!(a, vim_core_rs::CoreHostAction::Quit { force: false, .. }));
        assert!(
            has_quit,
            ":q 実行後に Quit(force=false) ホストアクションが発行されること: actions={:?}",
            actions
        );
    }

    #[test]
    fn force_quit_command_produces_force_quit_host_action() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("content\n").expect("core bridge should initialize");

        bridge
            .apply_ex_command(":q!")
            .expect(":q! コマンドは成功すること");

        let actions = bridge.take_pending_host_actions();
        log::debug!("[test] host actions after :q!: {:?}", actions);
        let has_force_quit = actions
            .iter()
            .any(|a| matches!(a, vim_core_rs::CoreHostAction::Quit { force: true, .. }));
        assert!(
            has_force_quit,
            ":q! 実行後に Quit(force=true) ホストアクションが発行されること: actions={:?}",
            actions
        );
    }
}
