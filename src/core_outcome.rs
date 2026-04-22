use std::collections::VecDeque;

use vim_core_rs::{
    CoreEvent, CoreHostAction, CoreInputRequestKind, CoreMessageCategory, CoreMessageEvent,
    CorePagerPromptKind, CoreVfsRequest,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeOrigin {
    TransactionHostAction,
    TransactionEvent,
    PendingSessionHostAction,
    PendingSessionEvent,
    BridgePromptResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutcomeTrace {
    pub sequence: u64,
    pub origin: OutcomeOrigin,
    pub raw_kind: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedCoreOutcome {
    HostDirective(NormalizedHostDirective),
    Notification(NormalizedNotification),
    Prompt(NormalizedPrompt),
    Structural(NormalizedStructuralOutcome),
    Diagnostic(NormalizedDiagnosticOutcome),
}

impl NormalizedCoreOutcome {
    pub fn trace(&self) -> &OutcomeTrace {
        match self {
            Self::HostDirective(outcome) => outcome.trace(),
            Self::Notification(outcome) => outcome.trace(),
            Self::Prompt(outcome) => outcome.trace(),
            Self::Structural(outcome) => outcome.trace(),
            Self::Diagnostic(outcome) => outcome.trace(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedHostDirective {
    Write {
        path: String,
        force: bool,
        issued_after_revision: u64,
        trace: OutcomeTrace,
    },
    Quit {
        force: bool,
        issued_after_revision: u64,
        trace: OutcomeTrace,
    },
    VfsRequest {
        request: CoreVfsRequest,
        trace: OutcomeTrace,
    },
}

impl NormalizedHostDirective {
    pub fn trace(&self) -> &OutcomeTrace {
        match self {
            Self::Write { trace, .. }
            | Self::Quit { trace, .. }
            | Self::VfsRequest { trace, .. } => trace,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedNotification {
    Message {
        event: CoreMessageEvent,
        trace: OutcomeTrace,
    },
    Bell {
        trace: OutcomeTrace,
    },
}

impl NormalizedNotification {
    pub fn trace(&self) -> &OutcomeTrace {
        match self {
            Self::Message { trace, .. } | Self::Bell { trace } => trace,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedPrompt {
    PagerPrompt {
        kind: CorePagerPromptKind,
        trace: OutcomeTrace,
    },
    RequestInput {
        prompt: String,
        input_kind: CoreInputRequestKind,
        correlation_id: u64,
        trace: OutcomeTrace,
    },
    InputResponseAccepted {
        correlation_id: u64,
        disposition: PromptResponseDisposition,
        trace: OutcomeTrace,
    },
}

impl NormalizedPrompt {
    pub fn trace(&self) -> &OutcomeTrace {
        match self {
            Self::PagerPrompt { trace, .. }
            | Self::RequestInput { trace, .. }
            | Self::InputResponseAccepted { trace, .. } => trace,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptResponseDisposition {
    Submitted,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedStructuralOutcome {
    RedrawRequested {
        full: bool,
        clear_before_draw: bool,
        trace: OutcomeTrace,
    },
    BufferAdded {
        buf_id: i32,
        trace: OutcomeTrace,
    },
    WindowCreated {
        win_id: i32,
        trace: OutcomeTrace,
    },
    LayoutChanged {
        trace: OutcomeTrace,
    },
}

impl NormalizedStructuralOutcome {
    pub fn trace(&self) -> &OutcomeTrace {
        match self {
            Self::RedrawRequested { trace, .. }
            | Self::BufferAdded { trace, .. }
            | Self::WindowCreated { trace, .. }
            | Self::LayoutChanged { trace } => trace,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedDiagnosticOutcome {
    JobOutOfScope {
        raw_kind: &'static str,
        trace: OutcomeTrace,
    },
    Unknown {
        raw_kind: String,
        trace: OutcomeTrace,
    },
}

pub fn core_host_action_raw_kind(action: &CoreHostAction) -> &'static str {
    match action {
        CoreHostAction::Write { .. } => "CoreHostAction::Write",
        CoreHostAction::Quit { .. } => "CoreHostAction::Quit",
        CoreHostAction::VfsRequest(_) => "CoreHostAction::VfsRequest",
        CoreHostAction::Redraw { .. } => "CoreHostAction::Redraw",
        CoreHostAction::RequestInput { .. } => "CoreHostAction::RequestInput",
        CoreHostAction::Bell => "CoreHostAction::Bell",
        CoreHostAction::JobStart(_) => "CoreHostAction::JobStart",
        CoreHostAction::JobWrite { .. } => "CoreHostAction::JobWrite",
        CoreHostAction::JobStop { .. } => "CoreHostAction::JobStop",
    }
}

pub fn core_event_raw_kind(event: &CoreEvent) -> &'static str {
    match event {
        CoreEvent::Message(_) => "CoreEvent::Message",
        CoreEvent::PagerPrompt(_) => "CoreEvent::PagerPrompt",
        CoreEvent::Bell => "CoreEvent::Bell",
        CoreEvent::Redraw { .. } => "CoreEvent::Redraw",
        CoreEvent::BufferAdded { .. } => "CoreEvent::BufferAdded",
        CoreEvent::WindowCreated { .. } => "CoreEvent::WindowCreated",
        CoreEvent::LayoutChanged => "CoreEvent::LayoutChanged",
    }
}

pub fn normalize_host_action(
    action: &CoreHostAction,
    trace: OutcomeTrace,
) -> NormalizedCoreOutcome {
    log::debug!(
        "[core_outcome] normalizing host action: sequence={}, origin={:?}, raw_kind={}",
        trace.sequence,
        trace.origin,
        trace.raw_kind
    );
    match action {
        CoreHostAction::Write {
            path,
            force,
            issued_after_revision,
        } => NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Write {
            path: path.clone(),
            force: *force,
            issued_after_revision: *issued_after_revision,
            trace,
        }),
        CoreHostAction::Quit {
            force,
            issued_after_revision,
        } => NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::Quit {
            force: *force,
            issued_after_revision: *issued_after_revision,
            trace,
        }),
        CoreHostAction::VfsRequest(request) => {
            NormalizedCoreOutcome::HostDirective(NormalizedHostDirective::VfsRequest {
                request: request.clone(),
                trace,
            })
        }
        CoreHostAction::Redraw {
            full,
            clear_before_draw,
        } => NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
            full: *full,
            clear_before_draw: *clear_before_draw,
            trace,
        }),
        CoreHostAction::RequestInput {
            prompt,
            input_kind,
            correlation_id,
        } => NormalizedCoreOutcome::Prompt(NormalizedPrompt::RequestInput {
            prompt: prompt.clone(),
            input_kind: *input_kind,
            correlation_id: *correlation_id,
            trace,
        }),
        CoreHostAction::Bell => {
            NormalizedCoreOutcome::Notification(NormalizedNotification::Bell { trace })
        }
        CoreHostAction::JobStart(_) => {
            NormalizedCoreOutcome::Diagnostic(NormalizedDiagnosticOutcome::JobOutOfScope {
                raw_kind: "CoreHostAction::JobStart",
                trace,
            })
        }
        CoreHostAction::JobWrite { .. } => {
            NormalizedCoreOutcome::Diagnostic(NormalizedDiagnosticOutcome::JobOutOfScope {
                raw_kind: "CoreHostAction::JobWrite",
                trace,
            })
        }
        CoreHostAction::JobStop { .. } => {
            NormalizedCoreOutcome::Diagnostic(NormalizedDiagnosticOutcome::JobOutOfScope {
                raw_kind: "CoreHostAction::JobStop",
                trace,
            })
        }
    }
}

pub fn normalize_core_event(event: &CoreEvent, trace: OutcomeTrace) -> NormalizedCoreOutcome {
    log::debug!(
        "[core_outcome] normalizing core event: sequence={}, origin={:?}, raw_kind={}",
        trace.sequence,
        trace.origin,
        trace.raw_kind
    );
    match event {
        CoreEvent::Message(event) => {
            NormalizedCoreOutcome::Notification(NormalizedNotification::Message {
                event: event.clone(),
                trace,
            })
        }
        CoreEvent::PagerPrompt(kind) => {
            NormalizedCoreOutcome::Prompt(NormalizedPrompt::PagerPrompt { kind: *kind, trace })
        }
        CoreEvent::Bell => {
            NormalizedCoreOutcome::Notification(NormalizedNotification::Bell { trace })
        }
        CoreEvent::Redraw {
            full,
            clear_before_draw,
        } => NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::RedrawRequested {
            full: *full,
            clear_before_draw: *clear_before_draw,
            trace,
        }),
        CoreEvent::BufferAdded { buf_id } => {
            NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::BufferAdded {
                buf_id: *buf_id,
                trace,
            })
        }
        CoreEvent::WindowCreated { win_id } => {
            NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::WindowCreated {
                win_id: *win_id,
                trace,
            })
        }
        CoreEvent::LayoutChanged => {
            NormalizedCoreOutcome::Structural(NormalizedStructuralOutcome::LayoutChanged { trace })
        }
    }
}

impl NormalizedDiagnosticOutcome {
    pub fn trace(&self) -> &OutcomeTrace {
        match self {
            Self::JobOutOfScope { trace, .. } | Self::Unknown { trace, .. } => trace,
        }
    }

    pub fn raw_kind(&self) -> &str {
        match self {
            Self::JobOutOfScope { raw_kind, .. } => raw_kind,
            Self::Unknown { raw_kind, .. } => raw_kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NormalizedOutcomeBatch {
    outcomes: Vec<NormalizedCoreOutcome>,
}

impl NormalizedOutcomeBatch {
    pub fn new(outcomes: Vec<NormalizedCoreOutcome>) -> Self {
        Self { outcomes }
    }

    pub fn outcomes(&self) -> &[NormalizedCoreOutcome] {
        &self.outcomes
    }

    pub fn into_outcomes(self) -> Vec<NormalizedCoreOutcome> {
        self.outcomes
    }

    pub fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NormalizedOutcomeQueue {
    items: VecDeque<NormalizedCoreOutcome>,
}

impl NormalizedOutcomeQueue {
    pub fn push_back(&mut self, outcome: NormalizedCoreOutcome) {
        let trace = *outcome.trace();
        log::debug!(
            "[core_outcome] queued normalized outcome: sequence={}, origin={:?}, raw_kind={}",
            trace.sequence,
            trace.origin,
            trace.raw_kind
        );
        self.items.push_back(outcome);
    }

    pub fn drain(&mut self) -> NormalizedOutcomeBatch {
        let outcomes = self.items.drain(..).collect::<Vec<_>>();
        log::debug!(
            "[core_outcome] drained normalized outcome batch: count={}",
            outcomes.len()
        );
        NormalizedOutcomeBatch::new(outcomes)
    }

    pub fn take_matching<F>(&mut self, mut predicate: F) -> Vec<NormalizedCoreOutcome>
    where
        F: FnMut(&NormalizedCoreOutcome) -> bool,
    {
        let original_len = self.items.len();
        let mut selected = Vec::new();
        let mut retained = VecDeque::new();

        while let Some(outcome) = self.items.pop_front() {
            if predicate(&outcome) {
                selected.push(outcome);
            } else {
                retained.push_back(outcome);
            }
        }

        self.items = retained;
        log::debug!(
            "[core_outcome] projected normalized outcomes for legacy adapter: original_count={}, selected_count={}, retained_count={}",
            original_len,
            selected.len(),
            self.items.len()
        );
        selected
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApplicationOutcomeState {
    pub prompt: PromptState,
    pub diagnostics: DiagnosticHistory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldedCoreOutcomes {
    pub state: ApplicationOutcomeState,
    pub effects: ApplicationDispatchEffects,
}

impl FoldedCoreOutcomes {
    pub fn downstream_consume_seam(&self) -> DownstreamOutcomeConsumeSeam<'_> {
        log::debug!(
            "[core_outcome] exposing downstream consume seam: has_user_message={}, bell_count={}, has_prompt_transition={}, has_active_prompt={}, has_redraw={}",
            self.effects
                .notification
                .latest_user_visible_message
                .is_some(),
            self.effects.notification.bell_count,
            self.effects.prompt.input_transition.is_some(),
            self.state.prompt.active_input.is_some(),
            self.effects.structural.redraw.is_some()
        );
        DownstreamOutcomeConsumeSeam {
            effects: &self.effects,
            state: &self.state,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DownstreamOutcomeConsumeSeam<'a> {
    effects: &'a ApplicationDispatchEffects,
    state: &'a ApplicationOutcomeState,
}

impl<'a> DownstreamOutcomeConsumeSeam<'a> {
    pub fn notification(&self) -> &'a NotificationEffect {
        &self.effects.notification
    }

    pub fn prompt_effect(&self) -> &'a PromptEffect {
        &self.effects.prompt
    }

    pub fn prompt_state(&self) -> &'a PromptState {
        &self.state.prompt
    }

    pub fn structural(&self) -> &'a StructuralEffectSet {
        &self.effects.structural
    }

    pub fn diagnostics(&self) -> &'a [NormalizedDiagnosticOutcome] {
        &self.effects.diagnostics
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApplicationDispatchEffects {
    pub host_directives: Vec<NormalizedHostDirective>,
    pub notification: NotificationEffect,
    pub prompt: PromptEffect,
    pub structural: StructuralEffectSet,
    pub diagnostics: Vec<NormalizedDiagnosticOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotificationEffect {
    pub latest_user_visible_message: Option<CoreMessageEvent>,
    pub latest_non_user_message: Option<CoreMessageEvent>,
    pub bell_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PromptState {
    pub active_input: Option<NormalizedInputPromptSession>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PromptEffect {
    pub pager_prompt: Option<CorePagerPromptKind>,
    pub input_transition: Option<PromptInputTransition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedInputPromptSession {
    pub prompt: String,
    pub input_kind: CoreInputRequestKind,
    pub correlation_id: u64,
    pub status: PromptSessionStatus,
    pub trace: OutcomeTrace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptSessionStatus {
    WaitingForUser,
    Submitted,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptInputTransition {
    Requested {
        session: NormalizedInputPromptSession,
    },
    Superseded {
        previous: NormalizedInputPromptSession,
        next: NormalizedInputPromptSession,
    },
    Submitted {
        correlation_id: u64,
    },
    Cancelled {
        correlation_id: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StructuralEffectSet {
    pub redraw: Option<RedrawEffect>,
    pub invalidate_buffers: Vec<i32>,
    pub invalidate_windows: Vec<i32>,
    pub layout_dirty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedrawEffect {
    pub full: bool,
    pub clear_before_draw: bool,
    pub required_by_structure_change: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiagnosticHistory {
    pub retained: Vec<NormalizedDiagnosticOutcome>,
}

pub fn fold_normalized_outcomes(
    batch: NormalizedOutcomeBatch,
    current: ApplicationOutcomeState,
) -> FoldedCoreOutcomes {
    let mut state = current;
    let mut effects = ApplicationDispatchEffects::default();

    for outcome in batch.into_outcomes() {
        let trace = *outcome.trace();
        log::debug!(
            "[core_outcome] folding normalized outcome: sequence={}, origin={:?}, raw_kind={}",
            trace.sequence,
            trace.origin,
            trace.raw_kind
        );

        match outcome {
            NormalizedCoreOutcome::HostDirective(directive) => {
                effects.host_directives.push(directive);
            }
            NormalizedCoreOutcome::Notification(notification) => {
                fold_notification(notification, &mut effects.notification);
            }
            NormalizedCoreOutcome::Prompt(prompt) => {
                fold_prompt(prompt, &mut state, &mut effects);
            }
            NormalizedCoreOutcome::Structural(structural) => {
                fold_structural(structural, &mut effects.structural);
            }
            NormalizedCoreOutcome::Diagnostic(diagnostic) => {
                record_diagnostic(diagnostic, &mut state, &mut effects);
            }
        }
    }

    FoldedCoreOutcomes { state, effects }
}

fn fold_notification(notification: NormalizedNotification, effect: &mut NotificationEffect) {
    match notification {
        NormalizedNotification::Message { event, trace } => {
            log::debug!(
                "[core_outcome] folding notification message: sequence={}, category={:?}, severity={:?}",
                trace.sequence,
                event.category,
                event.severity
            );
            match event.category {
                CoreMessageCategory::UserVisible => {
                    effect.latest_user_visible_message = Some(event);
                }
                CoreMessageCategory::CommandFeedback => {
                    effect.latest_non_user_message = Some(event);
                }
            }
        }
        NormalizedNotification::Bell { trace } => {
            log::debug!(
                "[core_outcome] folding notification bell: sequence={}",
                trace.sequence
            );
            effect.bell_count += 1;
        }
    }
}

fn fold_prompt(
    prompt: NormalizedPrompt,
    state: &mut ApplicationOutcomeState,
    effects: &mut ApplicationDispatchEffects,
) {
    match prompt {
        NormalizedPrompt::PagerPrompt { kind, trace } => {
            log::debug!(
                "[core_outcome] folding pager prompt: sequence={}, kind={:?}",
                trace.sequence,
                kind
            );
            effects.prompt.pager_prompt = Some(kind);
        }
        NormalizedPrompt::RequestInput {
            prompt,
            input_kind,
            correlation_id,
            trace,
        } => {
            log::debug!(
                "[core_outcome] folding input prompt request: sequence={}, correlation_id={}, input_kind={:?}",
                trace.sequence,
                correlation_id,
                input_kind
            );
            let next = NormalizedInputPromptSession {
                prompt,
                input_kind,
                correlation_id,
                status: PromptSessionStatus::WaitingForUser,
                trace,
            };
            effects.prompt.input_transition =
                if let Some(previous) = state.prompt.active_input.replace(next.clone()) {
                    Some(PromptInputTransition::Superseded { previous, next })
                } else {
                    Some(PromptInputTransition::Requested { session: next })
                };
        }
        NormalizedPrompt::InputResponseAccepted {
            correlation_id,
            disposition,
            trace,
        } => {
            log::debug!(
                "[core_outcome] folding input prompt response: sequence={}, correlation_id={}, disposition={:?}",
                trace.sequence,
                correlation_id,
                disposition
            );
            match state.prompt.active_input.as_ref() {
                Some(active) if active.correlation_id == correlation_id => {
                    state.prompt.active_input = None;
                    effects.prompt.input_transition = Some(match disposition {
                        PromptResponseDisposition::Submitted => {
                            PromptInputTransition::Submitted { correlation_id }
                        }
                        PromptResponseDisposition::Cancelled => {
                            PromptInputTransition::Cancelled { correlation_id }
                        }
                    });
                }
                Some(active) => {
                    let diagnostic = NormalizedDiagnosticOutcome::Unknown {
                        raw_kind: format!(
                            "PromptResponseCorrelationMismatch(expected={}, actual={})",
                            active.correlation_id, correlation_id
                        ),
                        trace,
                    };
                    record_diagnostic(diagnostic, state, effects);
                }
                None => {
                    let diagnostic = NormalizedDiagnosticOutcome::Unknown {
                        raw_kind: format!(
                            "PromptResponseWithoutActivePrompt(actual={})",
                            correlation_id
                        ),
                        trace,
                    };
                    record_diagnostic(diagnostic, state, effects);
                }
            }
        }
    }
}

fn fold_structural(structural: NormalizedStructuralOutcome, effect: &mut StructuralEffectSet) {
    match structural {
        NormalizedStructuralOutcome::RedrawRequested {
            full,
            clear_before_draw,
            trace,
        } => {
            log::debug!(
                "[core_outcome] folding redraw request: sequence={}, full={}, clear_before_draw={}",
                trace.sequence,
                full,
                clear_before_draw
            );
            merge_redraw(effect, full, clear_before_draw, false);
        }
        NormalizedStructuralOutcome::BufferAdded { buf_id, trace } => {
            log::debug!(
                "[core_outcome] folding buffer invalidation: sequence={}, buf_id={}",
                trace.sequence,
                buf_id
            );
            effect.invalidate_buffers.push(buf_id);
            merge_redraw(effect, true, false, true);
        }
        NormalizedStructuralOutcome::WindowCreated { win_id, trace } => {
            log::debug!(
                "[core_outcome] folding window invalidation: sequence={}, win_id={}",
                trace.sequence,
                win_id
            );
            effect.invalidate_windows.push(win_id);
            merge_redraw(effect, true, false, true);
        }
        NormalizedStructuralOutcome::LayoutChanged { trace } => {
            log::debug!(
                "[core_outcome] folding layout invalidation: sequence={}",
                trace.sequence
            );
            effect.layout_dirty = true;
            merge_redraw(effect, true, false, true);
        }
    }
}

fn merge_redraw(
    effect: &mut StructuralEffectSet,
    full: bool,
    clear_before_draw: bool,
    required_by_structure_change: bool,
) {
    match effect.redraw.as_mut() {
        Some(redraw) => {
            redraw.full |= full;
            redraw.clear_before_draw |= clear_before_draw;
            redraw.required_by_structure_change |= required_by_structure_change;
        }
        None => {
            effect.redraw = Some(RedrawEffect {
                full,
                clear_before_draw,
                required_by_structure_change,
            });
        }
    }
}

fn record_diagnostic(
    diagnostic: NormalizedDiagnosticOutcome,
    state: &mut ApplicationOutcomeState,
    effects: &mut ApplicationDispatchEffects,
) {
    let trace = *diagnostic.trace();
    log::debug!(
        "[core_outcome] retaining diagnostic outcome: sequence={}, origin={:?}, raw_kind={}, diagnostic_kind={}",
        trace.sequence,
        trace.origin,
        trace.raw_kind,
        diagnostic.raw_kind()
    );
    effects.diagnostics.push(diagnostic.clone());
    state.diagnostics.retained.push(diagnostic);
}
