//! core outcome の消費パイプライン。正規化済み outcome の畳み込みと
//! dispatch effect の適用、構造リフレッシュの蓄積を担う。

use crate::core::notification_prompt::{
    NotificationPromptProjectionState, ProjectionFrame, record_prompt_response_error,
};
use crate::core::outcome::{
    ApplicationDispatchEffects, ApplicationOutcomeState, NormalizedHostDirective,
    NormalizedOutcomeBatch, StructuralEffectSet, fold_normalized_outcomes,
};
use crate::core::prompt::PromptResponseCommand;
use crate::presentation::structural_refresh::{StructuralRefresh, StructuralRefreshOutcome};

use crate::presentation::render::redraw_trace::trace_redraw_diagnostic;

#[derive(Debug, Default)]
pub struct MainOutcomeAccumulator {
    pub state: ApplicationOutcomeState,
    pub host_directives: Vec<NormalizedHostDirective>,
    pub projection: NotificationPromptProjectionState,
    pub last_projection_frame: Option<ProjectionFrame>,
    pub last_structural_refresh: Option<StructuralRefreshOutcome>,
    pub suspend_requested: bool,
}

pub fn consume_core_outcomes_from_core(
    core_bridge: &mut crate::core::bridge::CoreBridge,
    accumulator: &mut MainOutcomeAccumulator,
    need_redraw: &mut bool,
) {
    let batch = core_bridge.take_normalized_outcomes();
    if batch.is_empty() {
        let neutral_refresh =
            StructuralRefresh::from_folded_effects(&StructuralEffectSet::default());
        trace_redraw_diagnostic(format_args!(
            "normalized batch was empty; switching to neutral structural refresh: redraw_requested={}, full={}, clear_before_draw={}, source={:?}, coalesced_count={}, invalidated_buffers={:?}, invalidated_windows={:?}, layout_dirty={}",
            neutral_refresh.redraw_plan.requested,
            neutral_refresh.redraw_plan.full,
            neutral_refresh.redraw_plan.clear_before_draw,
            neutral_refresh.redraw_plan.source,
            neutral_refresh.redraw_plan.coalesced_count,
            neutral_refresh.invalidation.buffer_ids,
            neutral_refresh.invalidation.window_ids,
            neutral_refresh.invalidation.layout_dirty
        ));
        accumulator.last_structural_refresh = Some(neutral_refresh);
        return;
    }

    consume_normalized_batch(batch, accumulator, need_redraw);
}

pub fn consume_normalized_batch(
    batch: NormalizedOutcomeBatch,
    accumulator: &mut MainOutcomeAccumulator,
    need_redraw: &mut bool,
) {
    let current = std::mem::take(&mut accumulator.state);
    let folded = fold_normalized_outcomes(batch, current);
    let projection_frame = accumulator
        .projection
        .apply_seam(folded.downstream_consume_seam());
    let effects = folded.effects;
    accumulator.state = folded.state;
    accumulator.last_projection_frame = Some(projection_frame.clone());
    let structural_refresh = StructuralRefresh::from_folded_effects(&effects.structural);
    trace_redraw_diagnostic(format_args!(
        "normalized batch folded into structural refresh: redraw_requested={}, full={}, clear_before_draw={}, source={:?}, coalesced_count={}, invalidated_buffers={:?}, invalidated_windows={:?}, layout_dirty={}",
        structural_refresh.redraw_plan.requested,
        structural_refresh.redraw_plan.full,
        structural_refresh.redraw_plan.clear_before_draw,
        structural_refresh.redraw_plan.source,
        structural_refresh.redraw_plan.coalesced_count,
        structural_refresh.invalidation.buffer_ids,
        structural_refresh.invalidation.window_ids,
        structural_refresh.invalidation.layout_dirty
    ));
    if structural_refresh.redraw_plan.requested {
        log::debug!(
            "[main] deriving redraw scheduling hint from structural RedrawPlan: full={}, clear_before_draw={}, source={:?}, coalesced_count={}",
            structural_refresh.redraw_plan.full,
            structural_refresh.redraw_plan.clear_before_draw,
            structural_refresh.redraw_plan.source,
            structural_refresh.redraw_plan.coalesced_count
        );
        *need_redraw = true;
    }
    accumulator.last_structural_refresh = Some(structural_refresh);
    apply_core_dispatch_effects(effects, &projection_frame, accumulator, need_redraw);
}

pub fn mark_structural_refresh_rendered(accumulator: &mut MainOutcomeAccumulator) {
    let neutral_refresh = StructuralRefresh::from_folded_effects(&StructuralEffectSet::default());
    trace_redraw_diagnostic(format_args!(
        "structural refresh marked rendered; switching to neutral state: previous_present={}, redraw_requested=false",
        accumulator.last_structural_refresh.is_some()
    ));
    accumulator.last_structural_refresh = Some(neutral_refresh);
}

pub fn apply_core_dispatch_effects(
    effects: ApplicationDispatchEffects,
    projection_frame: &ProjectionFrame,
    accumulator: &mut MainOutcomeAccumulator,
    need_redraw: &mut bool,
) {
    if let Some(message) = effects.notification.latest_user_visible_message {
        log::debug!(
            "[main] replacing core message from normalized notification effect: {:?}",
            message
        );
    }
    if let Some(message) = effects.notification.latest_non_user_message {
        log::debug!(
            "[main] observed non-user core message from normalized notification effect: {:?}",
            message
        );
    }
    if effects.notification.bell_count > 0 {
        log::debug!(
            "[main] observed bell notification effect: count={}",
            effects.notification.bell_count
        );
    }
    if let Some(prompt) = effects.prompt.pager_prompt {
        log::debug!("[main] observed pager prompt effect: kind={:?}", prompt);
    }
    if let Some(transition) = effects.prompt.input_transition {
        log::debug!(
            "[main] observed input prompt transition effect: {:?}",
            transition
        );
    }
    if let Some(prompt) = projection_frame.input_prompt.as_ref() {
        log::debug!(
            "[main] retained prompt projection is active: correlation_id={}, input_kind={:?}, buffer_len={}, status={:?}",
            prompt.correlation_id,
            prompt.input_kind,
            prompt.input.len(),
            prompt.status
        );
    }
    if let Some(error) = projection_frame.response_error.as_ref() {
        log::debug!(
            "[main] prompt projection recorded response error: sequence={}, error={}",
            projection_frame.sequence,
            error
        );
    }
    if let Some(redraw) = effects.structural.redraw {
        log::debug!(
            "[main] applying structural redraw effect: full={}, clear_before_draw={}, required_by_structure_change={}",
            redraw.full,
            redraw.clear_before_draw,
            redraw.required_by_structure_change
        );
        *need_redraw = true;
    }
    if !effects.structural.invalidate_buffers.is_empty()
        || !effects.structural.invalidate_windows.is_empty()
        || effects.structural.layout_dirty
    {
        log::debug!(
            "[main] observed structural invalidation effect: buffers={:?}, windows={:?}, layout_dirty={}",
            effects.structural.invalidate_buffers,
            effects.structural.invalidate_windows,
            effects.structural.layout_dirty
        );
    }
    for diagnostic in &effects.diagnostics {
        log::debug!(
            "[main] observed normalized diagnostic effect: {:?}",
            diagnostic
        );
    }
    accumulator.host_directives.extend(effects.host_directives);
}

pub fn dispatch_prompt_response_command(
    core_bridge: &mut crate::core::bridge::CoreBridge,
    accumulator: &mut MainOutcomeAccumulator,
    command: PromptResponseCommand,
    need_redraw: &mut bool,
) {
    log::debug!(
        "[main] routing prompt response through core bridge: correlation_id={}",
        command.correlation_id()
    );
    match core_bridge.respond_to_prompt(command) {
        Ok(batch) => {
            consume_normalized_batch(batch, accumulator, need_redraw);
        }
        Err(error) => {
            log::debug!("[main] prompt response rejected: {}", error);
            record_prompt_response_error(&mut accumulator.projection, error);
        }
    }
    *need_redraw = true;
}
