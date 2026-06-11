//! CoreBridge 内部の outcome 正規化・キーディスパッチ補助。

use super::*;

pub(super) fn map_match_kind(match_type: CoreMatchType) -> SearchMatchKind {
    match match_type {
        CoreMatchType::Regular => SearchMatchKind::Regular,
        CoreMatchType::IncSearch => SearchMatchKind::Incremental,
        CoreMatchType::CurSearch => SearchMatchKind::Current,
    }
}

pub(super) fn map_search_mode(mode: CoreSearchHighlightMode) -> SearchQueryMode {
    match mode {
        CoreSearchHighlightMode::Disabled => SearchQueryMode::Disabled,
        CoreSearchHighlightMode::HlSearch => SearchQueryMode::Hlsearch,
        CoreSearchHighlightMode::IncSearch => SearchQueryMode::IncsearchPreview,
    }
}

pub(super) fn map_search_query_error(error: CoreSearchQueryError) -> SearchStateError {
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
    pub(super) fn search_prompt_is_active(&self) -> bool {
        self.session.get_search_input_pattern().is_some()
            || self.session.is_incsearch_active()
            || matches!(
                self.session.light_snapshot().mode,
                vim_core_rs::CoreMode::CommandLine
            )
    }

    pub(super) fn should_buffer_transport_prefix(&self, key: &str) -> bool {
        key == "\u{17}"
            && self.pending_transport_key.is_none()
            && matches!(
                self.session.light_snapshot().mode,
                vim_core_rs::CoreMode::Normal
                    | vim_core_rs::CoreMode::Visual
                    | vim_core_rs::CoreMode::VisualLine
                    | vim_core_rs::CoreMode::VisualBlock
            )
    }

    pub(super) fn take_pending_transport_key(&mut self) -> Option<String> {
        self.pending_transport_key.take()
    }

    pub(super) fn should_handle_ctrl_c_interrupt(&self, key: &str) -> bool {
        key == "\u{3}"
            && matches!(
                self.session.light_snapshot().mode,
                vim_core_rs::CoreMode::Normal
                    | vim_core_rs::CoreMode::Visual
                    | vim_core_rs::CoreMode::VisualLine
                    | vim_core_rs::CoreMode::VisualBlock
            )
    }

    pub(super) fn handle_ctrl_c_interrupt(
        &mut self,
    ) -> Result<CoreCommandOutcome, CoreSessionError> {
        let snapshot = self.session.light_snapshot();
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

    pub(super) fn dispatch_session_key(
        &mut self,
        key: &str,
    ) -> Result<CoreCommandOutcome, CoreSessionError> {
        let outcome = self
            .session
            .dispatch_key(key)
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&outcome);
        Ok(outcome.outcome)
    }

    pub(super) fn queue_transaction_artifacts(&mut self, tx: &vim_core_rs::CoreCommandTransaction) {
        for action in &tx.host_actions {
            self.queue_host_action(action, OutcomeOrigin::TransactionHostAction);
        }

        for event in &tx.events {
            self.queue_core_event(event, OutcomeOrigin::TransactionEvent);
        }
    }

    pub(super) fn collect_transaction_artifacts_into(
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

    pub(super) fn drain_pending_host_actions_from_session(&mut self) {
        while let Some(action) = self.session.take_pending_host_action() {
            self.queue_host_action(&action, OutcomeOrigin::PendingSessionHostAction);
        }
    }

    pub(super) fn drain_pending_events_from_session(&mut self) {
        while let Some(event) = self.session.take_pending_event() {
            self.queue_core_event(&event, OutcomeOrigin::PendingSessionEvent);
        }
    }

    pub(super) fn queue_host_action(&mut self, action: &CoreHostAction, origin: OutcomeOrigin) {
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

    pub(super) fn queue_core_event(&mut self, event: &CoreEvent, origin: OutcomeOrigin) {
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

    pub(super) fn next_outcome_trace(
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

    pub(super) fn enqueue_normalized_outcome(&mut self, outcome: NormalizedCoreOutcome) {
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
