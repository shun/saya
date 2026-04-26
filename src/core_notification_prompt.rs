use crate::core_outcome::{
    DownstreamOutcomeConsumeSeam, NormalizedInputPromptSession, PromptInputTransition,
};
use crate::core_prompt::{PromptResponseCommand, PromptResponseError};
use crate::input_router::KeyInput;

use vim_core_rs::{
    CoreInputRequestKind, CoreMessageCategory, CoreMessageEvent, CoreMessageSeverity,
    CorePagerPromptKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageLineSource {
    CommandPreview,
    SystemWarning,
    CoreNotification,
    RenderProjectionError,
    RuntimeOverlayFallback,
    TransientInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLineCandidate {
    pub source: MessageLineSource,
    pub text: String,
}

impl MessageLineCandidate {
    pub fn legacy(source: MessageLineSource, text: impl Into<String>) -> Self {
        Self {
            source,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceMessageLineState {
    pub visible: Option<MessageLineCandidate>,
    pub suppressed: Vec<MessageLineCandidate>,
}

impl WorkspaceMessageLineState {
    pub fn visible_text(&self) -> Option<&str> {
        self.visible
            .as_ref()
            .map(|candidate| candidate.text.as_str())
    }

    pub fn visible_source(&self) -> Option<MessageLineSource> {
        self.visible.as_ref().map(|candidate| candidate.source)
    }

    pub fn suppressed_sources(&self) -> Vec<MessageLineSource> {
        self.suppressed
            .iter()
            .map(|candidate| candidate.source)
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.visible.is_none() && self.suppressed.is_empty()
    }
}

fn source_rank(source: MessageLineSource) -> u8 {
    match source {
        MessageLineSource::CommandPreview => 0,
        MessageLineSource::SystemWarning => 1,
        MessageLineSource::CoreNotification => 2,
        MessageLineSource::RenderProjectionError => 3,
        MessageLineSource::RuntimeOverlayFallback => 4,
        MessageLineSource::TransientInfo => 5,
    }
}

pub fn resolve_workspace_message_line(
    candidates: impl IntoIterator<Item = MessageLineCandidate>,
) -> WorkspaceMessageLineState {
    let mut eligible = Vec::new();
    let mut suppressed = Vec::new();

    for candidate in candidates {
        let trimmed = candidate.text.trim();
        if trimmed.is_empty() {
            log::debug!(
                "[core_notification_prompt] suppressed empty message candidate: source={:?}, text_len={}",
                candidate.source,
                candidate.text.len()
            );
            continue;
        }
        eligible.push(MessageLineCandidate {
            source: candidate.source,
            text: trimmed.to_string(),
        });
    }

    eligible.sort_by_key(|candidate| source_rank(candidate.source));
    let visible = eligible.first().cloned();
    suppressed.extend(eligible.into_iter().skip(1));

    log::debug!(
        "[core_notification_prompt] resolved message line: visible_source={:?}, suppressed_sources={:?}, candidate_count={}",
        visible.as_ref().map(|candidate| candidate.source),
        suppressed
            .iter()
            .map(|candidate| candidate.source)
            .collect::<Vec<_>>(),
        visible.as_ref().map(|_| 1).unwrap_or(0) + suppressed.len()
    );

    WorkspaceMessageLineState {
        visible,
        suppressed,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BellIndication {
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagerPromptView {
    pub kind: CorePagerPromptKind,
    pub one_shot: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptHintSuppressionReason {
    ActiveInputPrompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppressedPromptHint {
    pub pager_prompt: PagerPromptView,
    pub reason: PromptHintSuppressionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptResponseDispositionView {
    Submit,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPromptStatus {
    Active,
    AwaitingCore {
        disposition: PromptResponseDispositionView,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputPromptView {
    pub prompt: String,
    pub input: String,
    pub correlation_id: u64,
    pub input_kind: CoreInputRequestKind,
    pub status: InputPromptStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptTransitionKind {
    Requested,
    Superseded,
    Submitted,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTransitionRecord {
    pub kind: PromptTransitionKind,
    pub correlation_id: u64,
    pub input_kind: CoreInputRequestKind,
    pub prompt_text: String,
    pub input_len: usize,
    pub previous_correlation_id: Option<u64>,
    pub next_correlation_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetainedPromptState {
    pub active_input: Option<InputPromptView>,
    pub last_transition: Option<PromptTransitionRecord>,
    pub last_response_error: Option<String>,
}

impl RetainedPromptState {
    pub fn active_input(&self) -> Option<&InputPromptView> {
        self.active_input.as_ref()
    }

    pub fn last_transition(&self) -> Option<&PromptTransitionRecord> {
        self.last_transition.as_ref()
    }

    pub fn last_response_error(&self) -> Option<&str> {
        self.last_response_error.as_deref()
    }

    fn set_active_input(&mut self, view: InputPromptView) {
        self.active_input = Some(view);
    }

    fn restore_active_after_error(&mut self, error: PromptResponseError) {
        let message = error.to_string();
        let kind = prompt_response_error_kind(&error);
        match self.active_input.as_mut() {
            Some(view) => {
                view.status = InputPromptStatus::Active;
                log::debug!(
                    "[core_notification_prompt] prompt response error: kind={}, correlation_id={}, input_kind={:?}, buffer_len={}",
                    kind,
                    view.correlation_id,
                    view.input_kind,
                    view.input.len()
                );
            }
            None => {
                log::debug!(
                    "[core_notification_prompt] prompt response error without active prompt: kind={}",
                    kind
                );
            }
        }
        self.last_response_error = Some(message);
    }

    pub fn requested(&mut self, view: InputPromptView) -> PromptTransitionRecord {
        let record = PromptTransitionRecord {
            kind: PromptTransitionKind::Requested,
            correlation_id: view.correlation_id,
            input_kind: view.input_kind,
            prompt_text: view.prompt.clone(),
            input_len: view.input.len(),
            previous_correlation_id: None,
            next_correlation_id: None,
        };
        log_transition(&record);
        self.active_input = Some(view);
        self.last_transition = Some(record.clone());
        self.last_response_error = None;
        record
    }

    pub fn superseded(&mut self, next: InputPromptView) -> PromptTransitionRecord {
        let previous = self.active_input.replace(next.clone());
        let record = PromptTransitionRecord {
            kind: PromptTransitionKind::Superseded,
            correlation_id: next.correlation_id,
            input_kind: next.input_kind,
            prompt_text: next.prompt.clone(),
            input_len: next.input.len(),
            previous_correlation_id: previous.as_ref().map(|view| view.correlation_id),
            next_correlation_id: Some(next.correlation_id),
        };
        log_transition(&record);
        self.last_transition = Some(record.clone());
        self.last_response_error = None;
        record
    }

    pub fn submitted(
        &mut self,
        correlation_id: u64,
        submitted_input: &str,
    ) -> PromptTransitionRecord {
        self.transition_outcome(
            PromptTransitionKind::Submitted,
            correlation_id,
            PromptResponseDispositionView::Submit,
            submitted_input.len(),
        )
    }

    pub fn cancelled(&mut self, correlation_id: u64) -> PromptTransitionRecord {
        self.transition_outcome(
            PromptTransitionKind::Cancelled,
            correlation_id,
            PromptResponseDispositionView::Cancel,
            0,
        )
    }

    fn transition_outcome(
        &mut self,
        kind: PromptTransitionKind,
        correlation_id: u64,
        disposition: PromptResponseDispositionView,
        submitted_input_len: usize,
    ) -> PromptTransitionRecord {
        let active = self.active_input.as_ref();
        let record = PromptTransitionRecord {
            kind,
            correlation_id,
            input_kind: active
                .map(|view| view.input_kind)
                .unwrap_or(CoreInputRequestKind::CommandLine),
            prompt_text: active.map(|view| view.prompt.clone()).unwrap_or_default(),
            input_len: submitted_input_len,
            previous_correlation_id: active.map(|view| view.correlation_id),
            next_correlation_id: None,
        };

        match active {
            Some(view) if view.correlation_id == correlation_id => {
                log::debug!(
                    "[core_notification_prompt] prompt transition: kind={:?}, correlation_id={}, input_kind={:?}, prompt_len={}, input_len={}, disposition={:?}",
                    record.kind,
                    record.correlation_id,
                    record.input_kind,
                    record.prompt_text.len(),
                    record.input_len,
                    disposition
                );
                self.active_input = None;
                self.last_transition = Some(record.clone());
                self.last_response_error = None;
            }
            Some(view) => {
                let error = format!(
                    "prompt correlation mismatch: expected={}, actual={}",
                    view.correlation_id, correlation_id
                );
                log::debug!(
                    "[core_notification_prompt] prompt transition rejected: kind={:?}, expected={}, actual={}, input_kind={:?}, prompt_len={}, input_len={}",
                    kind,
                    view.correlation_id,
                    correlation_id,
                    view.input_kind,
                    view.prompt.len(),
                    view.input.len()
                );
                self.last_response_error = Some(error);
            }
            None => {
                let error = format!("no active prompt for correlation_id={correlation_id}");
                log::debug!(
                    "[core_notification_prompt] prompt transition rejected: kind={:?}, correlation_id={}, input_len={}, reason=no_active_prompt",
                    kind,
                    correlation_id,
                    submitted_input_len
                );
                self.last_response_error = Some(error);
            }
        }

        record
    }
}

fn log_transition(record: &PromptTransitionRecord) {
    log::debug!(
        "[core_notification_prompt] prompt transition: kind={:?}, correlation_id={}, input_kind={:?}, prompt_len={}, input_len={}, previous_correlation_id={:?}, next_correlation_id={:?}",
        record.kind,
        record.correlation_id,
        record.input_kind,
        record.prompt_text.len(),
        record.input_len,
        record.previous_correlation_id,
        record.next_correlation_id
    );
}

fn prompt_response_error_kind(error: &PromptResponseError) -> &'static str {
    match error {
        PromptResponseError::NoActivePrompt => "NoActivePrompt",
        PromptResponseError::CorrelationMismatch { .. } => "CorrelationMismatch",
        PromptResponseError::CoreRejected(_) => "CoreRejected",
        PromptResponseError::Core(_) => "Core",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationMessageView {
    pub text: String,
    pub severity: CoreMessageSeverity,
    pub category: CoreMessageCategory,
}

impl From<&CoreMessageEvent> for NotificationMessageView {
    fn from(event: &CoreMessageEvent) -> Self {
        Self {
            text: event.content.trim().to_string(),
            severity: event.severity.clone(),
            category: event.category.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceNotificationPromptView {
    pub notification_message: Option<NotificationMessageView>,
    pub bell: Option<BellIndication>,
    pub pager_prompt: Option<PagerPromptView>,
    pub suppressed_prompt_hints: Vec<SuppressedPromptHint>,
    pub input_prompt: Option<InputPromptView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionFrame {
    pub sequence: u64,
    pub message_line: WorkspaceMessageLineState,
    pub bell: Option<BellIndication>,
    pub prompt: RetainedPromptState,
    pub pager_prompt: Option<PagerPromptView>,
    pub suppressed_prompt_hints: Vec<SuppressedPromptHint>,
    pub input_prompt: Option<InputPromptView>,
    pub replayed: bool,
    pub response_error: Option<String>,
}

impl ProjectionFrame {
    pub fn workspace_view(&self) -> WorkspaceNotificationPromptView {
        WorkspaceNotificationPromptView {
            notification_message: self.message_line.visible.as_ref().map(|candidate| {
                NotificationMessageView {
                    text: candidate.text.clone(),
                    severity: CoreMessageSeverity::Info,
                    category: CoreMessageCategory::UserVisible,
                }
            }),
            bell: self.bell,
            pager_prompt: self.pager_prompt,
            suppressed_prompt_hints: self.suppressed_prompt_hints.clone(),
            input_prompt: self.input_prompt.clone(),
        }
    }

    pub fn log_metadata(&self) {
        let transition_kind = self
            .prompt
            .last_transition()
            .map(|record| format!("{:?}", record.kind))
            .unwrap_or_else(|| "None".to_string());
        log::debug!(
            "[core_notification_prompt] projection frame: sequence={}, replayed={}, bell_count={}, visible_source={:?}, suppressed_sources={:?}, response_error_present={}, prompt_active={}, transition_kind={}",
            self.sequence,
            self.replayed,
            self.bell.map(|bell| bell.count).unwrap_or(0),
            self.message_line.visible_source(),
            self.message_line.suppressed_sources(),
            self.response_error.is_some(),
            self.prompt.active_input.is_some(),
            transition_kind
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotificationPromptProjectionState {
    next_sequence: u64,
    prompt: RetainedPromptState,
    observed_bell_count: usize,
}

impl NotificationPromptProjectionState {
    pub fn prompt(&self) -> &RetainedPromptState {
        &self.prompt
    }

    pub fn apply_seam(&mut self, seam: DownstreamOutcomeConsumeSeam<'_>) -> ProjectionFrame {
        let sequence = self.next_sequence + 1;
        self.next_sequence = sequence;

        let notification = seam.notification();
        let prompt_effect = seam.prompt_effect();
        let retained_prompt = seam.prompt_state();

        let message_line = resolve_notification_message_line(sequence, notification);
        let bell = if notification.bell_count > 0 {
            self.observed_bell_count += notification.bell_count;
            log::debug!(
                "[core_notification_prompt] bell projection: sequence={}, dispatch_bell_count={}, observed_bell_count={}",
                sequence,
                notification.bell_count,
                self.observed_bell_count
            );
            Some(BellIndication {
                count: notification.bell_count,
            })
        } else {
            None
        };

        apply_prompt_transition(&mut self.prompt, prompt_effect.input_transition.as_ref());
        reconcile_retained_prompt_state(&mut self.prompt, retained_prompt.active_input.as_ref());

        let mut pager_prompt = prompt_effect.pager_prompt.map(|kind| PagerPromptView {
            kind,
            one_shot: true,
        });
        let mut suppressed_prompt_hints = Vec::new();
        if let Some(pager) = pager_prompt.take() {
            log::debug!(
                "[core_notification_prompt] pager prompt projection: sequence={}, kind={:?}, prompt_active={}",
                sequence,
                pager.kind,
                self.prompt.active_input.is_some()
            );
            if self.prompt.active_input.is_some() {
                suppressed_prompt_hints.push(SuppressedPromptHint {
                    pager_prompt: pager,
                    reason: PromptHintSuppressionReason::ActiveInputPrompt,
                });
            } else {
                pager_prompt = Some(pager);
            }
        }

        let frame = ProjectionFrame {
            sequence,
            message_line,
            bell,
            prompt: self.prompt.clone(),
            pager_prompt,
            suppressed_prompt_hints,
            input_prompt: self.prompt.active_input.clone(),
            replayed: false,
            response_error: self.prompt.last_response_error.clone(),
        };
        frame.log_metadata();
        frame
    }
}

fn resolve_notification_message_line(
    sequence: u64,
    notification: &crate::core_outcome::NotificationEffect,
) -> WorkspaceMessageLineState {
    let mut candidates = Vec::new();

    if let Some(message) = notification.latest_user_visible_message.as_ref() {
        let text = message.content.trim();
        if text.is_empty() {
            log::debug!(
                "[core_notification_prompt] empty message candidate observed: sequence={}, severity={:?}, category={:?}, text_len={}",
                sequence,
                message.severity,
                message.category,
                message.content.len()
            );
        } else {
            log::debug!(
                "[core_notification_prompt] user-visible notification projection: sequence={}, severity={:?}, category={:?}, text_len={}",
                sequence,
                message.severity,
                message.category,
                text.len()
            );
            candidates.push(MessageLineCandidate::legacy(
                MessageLineSource::CoreNotification,
                text,
            ));
        }
    }

    if let Some(message) = notification.latest_non_user_message.as_ref() {
        log::debug!(
            "[core_notification_prompt] non-user notification observed: sequence={}, severity={:?}, category={:?}, text_len={}",
            sequence,
            message.severity,
            message.category,
            message.content.len()
        );
    }

    resolve_workspace_message_line(candidates)
}

fn input_prompt_view_from_session(
    session: &NormalizedInputPromptSession,
    input: String,
    status: InputPromptStatus,
) -> InputPromptView {
    InputPromptView {
        prompt: session.prompt.clone(),
        input,
        correlation_id: session.correlation_id,
        input_kind: session.input_kind,
        status,
    }
}

fn apply_prompt_transition(
    prompt: &mut RetainedPromptState,
    transition: Option<&PromptInputTransition>,
) {
    let Some(transition) = transition else {
        return;
    };

    match transition {
        PromptInputTransition::Requested { session } => {
            let _ = prompt.requested(input_prompt_view_from_session(
                session,
                String::new(),
                InputPromptStatus::Active,
            ));
        }
        PromptInputTransition::Superseded { next, .. } => {
            let _ = prompt.superseded(input_prompt_view_from_session(
                next,
                String::new(),
                InputPromptStatus::Active,
            ));
        }
        PromptInputTransition::Submitted { correlation_id } => {
            let submitted_input = prompt
                .active_input()
                .map(|view| view.input.clone())
                .unwrap_or_default();
            let _ = prompt.submitted(*correlation_id, &submitted_input);
        }
        PromptInputTransition::Cancelled { correlation_id } => {
            let _ = prompt.cancelled(*correlation_id);
        }
    }
}

fn reconcile_retained_prompt_state(
    prompt: &mut RetainedPromptState,
    retained_input: Option<&NormalizedInputPromptSession>,
) {
    match retained_input {
        Some(session) => {
            let next = match prompt.active_input.take() {
                Some(mut current) if current.correlation_id == session.correlation_id => {
                    current.prompt = session.prompt.clone();
                    current.input_kind = session.input_kind;
                    current.status = InputPromptStatus::Active;
                    current
                }
                _ => input_prompt_view_from_session(
                    session,
                    String::new(),
                    InputPromptStatus::Active,
                ),
            };
            prompt.set_active_input(next);
        }
        None if prompt.last_transition().is_some_and(|transition| {
            matches!(
                transition.kind,
                PromptTransitionKind::Submitted | PromptTransitionKind::Cancelled
            )
        }) =>
        {
            prompt.active_input = None;
        }
        None => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptInputAction {
    Consumed,
    AwaitingCore,
    Submit(PromptResponseCommand),
    Cancel(PromptResponseCommand),
    NotPromptInput,
}

pub fn handle_prompt_key(
    state: &mut NotificationPromptProjectionState,
    key: &KeyInput,
) -> PromptInputAction {
    let Some(active_prompt) = state.prompt.active_input.as_mut() else {
        return PromptInputAction::NotPromptInput;
    };

    match active_prompt.status {
        InputPromptStatus::AwaitingCore { .. } => {
            log::debug!(
                "[core_notification_prompt] prompt key consumed while awaiting core: correlation_id={}, key={:?}",
                active_prompt.correlation_id,
                key
            );
            PromptInputAction::AwaitingCore
        }
        InputPromptStatus::Active => match key {
            KeyInput::Char(ch) => {
                active_prompt.input.push(*ch);
                log::debug!(
                    "[core_notification_prompt] prompt input appended: correlation_id={}, input_kind={:?}, buffer_len={}",
                    active_prompt.correlation_id,
                    active_prompt.input_kind,
                    active_prompt.input.len()
                );
                PromptInputAction::Consumed
            }
            KeyInput::Backspace => {
                let _ = active_prompt.input.pop();
                log::debug!(
                    "[core_notification_prompt] prompt input backspace: correlation_id={}, input_kind={:?}, buffer_len={}",
                    active_prompt.correlation_id,
                    active_prompt.input_kind,
                    active_prompt.input.len()
                );
                PromptInputAction::Consumed
            }
            KeyInput::Enter => {
                let correlation_id = active_prompt.correlation_id;
                let value = active_prompt.input.clone();
                active_prompt.status = InputPromptStatus::AwaitingCore {
                    disposition: PromptResponseDispositionView::Submit,
                };
                log::debug!(
                    "[core_notification_prompt] prompt submit requested: correlation_id={}, input_kind={:?}, buffer_len={}",
                    correlation_id,
                    active_prompt.input_kind,
                    value.len()
                );
                PromptInputAction::Submit(PromptResponseCommand::Submit {
                    correlation_id,
                    value,
                })
            }
            KeyInput::Escape => {
                let correlation_id = active_prompt.correlation_id;
                active_prompt.status = InputPromptStatus::AwaitingCore {
                    disposition: PromptResponseDispositionView::Cancel,
                };
                log::debug!(
                    "[core_notification_prompt] prompt cancel requested: correlation_id={}, input_kind={:?}, buffer_len={}",
                    correlation_id,
                    active_prompt.input_kind,
                    active_prompt.input.len()
                );
                PromptInputAction::Cancel(PromptResponseCommand::Cancel { correlation_id })
            }
            KeyInput::Ctrl(_) => {
                log::debug!(
                    "[core_notification_prompt] prompt control key consumed locally: correlation_id={}, key={:?}",
                    active_prompt.correlation_id,
                    key
                );
                PromptInputAction::Consumed
            }
            _ => {
                log::debug!(
                    "[core_notification_prompt] prompt non-text key consumed locally: correlation_id={}, key={:?}",
                    active_prompt.correlation_id,
                    key
                );
                PromptInputAction::Consumed
            }
        },
    }
}

pub fn record_prompt_response_error(
    state: &mut NotificationPromptProjectionState,
    error: PromptResponseError,
) {
    state.prompt.restore_active_after_error(error);
}
