use crate::core::outcome::StructuralEffectSet;
use crate::presentation::screen_model::WorkspaceProjectionSummary;
use crate::presentation::viewport::ViewportSyncSummary;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralRefreshOutcome {
    pub redraw_plan: RedrawPlan,
    pub invalidation: InvalidationSummary,
    pub projection: ProjectionIntent,
    pub viewport_status: ViewportRefreshStatus,
}

impl StructuralRefreshOutcome {
    pub fn projection_failure(
        &self,
        reason: impl Into<String>,
        viewport_status: ViewportRefreshStatus,
    ) -> ProjectionFailureDiagnostic {
        let reason = reason.into();
        log::debug!(
            "[structural_refresh] projection failure: reason={}, redraw_requested={}, redraw_full={}, clear_before_draw={}, buffer_invalidations={}, window_invalidations={}, layout_dirty={}, viewport_status={:?}, projection_status={:?}",
            reason,
            self.redraw_plan.requested,
            self.redraw_plan.full,
            self.redraw_plan.clear_before_draw,
            self.invalidation.buffer_ids.len(),
            self.invalidation.window_ids.len(),
            self.invalidation.layout_dirty,
            viewport_status,
            ProjectionStatus::Failed
        );
        ProjectionFailureDiagnostic {
            reason,
            redraw_plan: self.redraw_plan.clone(),
            invalidation: self.invalidation.clone(),
            viewport_status,
            projection_status: ProjectionStatus::Failed,
            projection_summary: None,
        }
    }

    pub fn with_viewport_sync_summary(mut self, summary: &ViewportSyncSummary) -> Self {
        let diagnostic = ViewportSyncDiagnostic::from_summary(summary);
        log::debug!(
            "[structural_refresh] viewport sync evaluated: live={}, synced={}, pruned={}, reevaluated={}, invalidated_missing={}",
            diagnostic.live_window_count,
            diagnostic.synced_window_count,
            diagnostic.pruned_window_count,
            diagnostic.reevaluated_window_count,
            diagnostic.invalidated_missing_window_count
        );
        self.viewport_status = ViewportRefreshStatus::Evaluated(diagnostic);
        self
    }

    pub fn with_projection_summary(mut self, summary: WorkspaceProjectionSummary) -> Self {
        log::debug!(
            "[structural_refresh] projection summary integrated: windows={}, active_window_id={}, pane_geometry={}, visible_buffers={}",
            summary.window_ids.len(),
            summary.active_window_id,
            summary.pane_geometry.len(),
            summary.visible_buffer_ids.len()
        );
        self.projection.status = ProjectionStatus::Fresh;
        self.projection.stale_reason = None;
        self.projection.summary = Some(summary);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedrawPlan {
    pub requested: bool,
    pub full: bool,
    pub clear_before_draw: bool,
    pub required_by_structure_change: bool,
    pub source: RedrawPlanSource,
    pub coalesced_count: usize,
}

impl Default for RedrawPlan {
    fn default() -> Self {
        Self {
            requested: false,
            full: false,
            clear_before_draw: false,
            required_by_structure_change: false,
            source: RedrawPlanSource::None,
            coalesced_count: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedrawPlanSource {
    None,
    ExplicitCore,
    StructuralInvalidation,
    ExplicitAndStructural,
    TerminalDisplayInvalidation,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InvalidationSummary {
    pub buffer_ids: Vec<i32>,
    pub window_ids: Vec<i32>,
    pub layout_dirty: bool,
}

impl InvalidationSummary {
    pub fn has_any(&self) -> bool {
        self.has_buffer_invalidation()
            || self.has_window_invalidation()
            || self.has_layout_invalidation()
    }

    pub fn has_buffer_invalidation(&self) -> bool {
        !self.buffer_ids.is_empty()
    }

    pub fn has_window_invalidation(&self) -> bool {
        !self.window_ids.is_empty()
    }

    pub fn has_layout_invalidation(&self) -> bool {
        self.layout_dirty
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionIntent {
    pub status: ProjectionStatus,
    pub stale_reason: Option<ProjectionStaleReason>,
    pub summary: Option<WorkspaceProjectionSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionStatus {
    Fresh,
    Stale,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionStaleReason {
    StructuralInvalidation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewportRefreshStatus {
    NotEvaluated,
    Deferred,
    Evaluated(ViewportSyncDiagnostic),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportSyncDiagnostic {
    pub live_window_count: usize,
    pub synced_window_count: usize,
    pub pruned_window_count: usize,
    pub reevaluated_window_count: usize,
    pub invalidated_missing_window_count: usize,
}

impl ViewportSyncDiagnostic {
    fn from_summary(summary: &ViewportSyncSummary) -> Self {
        Self {
            live_window_count: summary.live_window_ids.len(),
            synced_window_count: summary.synced_window_ids.len(),
            pruned_window_count: summary.pruned_window_ids.len(),
            reevaluated_window_count: summary.reevaluated_window_ids.len(),
            invalidated_missing_window_count: summary.invalidated_missing_window_ids.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionFailureDiagnostic {
    pub reason: String,
    pub redraw_plan: RedrawPlan,
    pub invalidation: InvalidationSummary,
    pub viewport_status: ViewportRefreshStatus,
    pub projection_status: ProjectionStatus,
    pub projection_summary: Option<WorkspaceProjectionSummary>,
}

pub struct StructuralRefresh;

impl StructuralRefresh {
    pub fn from_folded_effects(effects: &StructuralEffectSet) -> StructuralRefreshOutcome {
        let invalidation = InvalidationSummary {
            buffer_ids: effects.invalidate_buffers.clone(),
            window_ids: effects.invalidate_windows.clone(),
            layout_dirty: effects.layout_dirty,
        };
        let redraw_plan = build_redraw_plan(effects, invalidation.has_any());
        let projection = build_projection_intent(invalidation.has_any());

        log::debug!(
            "[structural_refresh] folded effects consumed: redraw_requested={}, redraw_full={}, clear_before_draw={}, source={:?}, coalesced_count={}, buffer_invalidations={}, window_invalidations={}, layout_dirty={}, projection_status={:?}",
            redraw_plan.requested,
            redraw_plan.full,
            redraw_plan.clear_before_draw,
            redraw_plan.source,
            redraw_plan.coalesced_count,
            invalidation.buffer_ids.len(),
            invalidation.window_ids.len(),
            invalidation.layout_dirty,
            projection.status
        );

        StructuralRefreshOutcome {
            redraw_plan,
            invalidation,
            projection,
            viewport_status: ViewportRefreshStatus::NotEvaluated,
        }
    }
}

fn build_redraw_plan(
    effects: &StructuralEffectSet,
    has_structural_invalidation: bool,
) -> RedrawPlan {
    match effects.redraw {
        Some(redraw) => RedrawPlan {
            requested: true,
            full: redraw.full,
            clear_before_draw: redraw.clear_before_draw,
            required_by_structure_change: redraw.required_by_structure_change,
            source: redraw_source(
                redraw.required_by_structure_change,
                has_structural_invalidation,
            ),
            coalesced_count: redraw.coalesced_count,
        },
        None if has_structural_invalidation => RedrawPlan {
            requested: true,
            full: true,
            clear_before_draw: false,
            required_by_structure_change: true,
            source: RedrawPlanSource::StructuralInvalidation,
            coalesced_count: 0,
        },
        None => RedrawPlan::default(),
    }
}

fn redraw_source(
    required_by_structure_change: bool,
    has_structural_invalidation: bool,
) -> RedrawPlanSource {
    match (required_by_structure_change, has_structural_invalidation) {
        (true, true) => RedrawPlanSource::ExplicitAndStructural,
        (true, false) => RedrawPlanSource::StructuralInvalidation,
        (false, _) => RedrawPlanSource::ExplicitCore,
    }
}

fn build_projection_intent(has_structural_invalidation: bool) -> ProjectionIntent {
    if has_structural_invalidation {
        ProjectionIntent {
            status: ProjectionStatus::Stale,
            stale_reason: Some(ProjectionStaleReason::StructuralInvalidation),
            summary: None,
        }
    } else {
        ProjectionIntent {
            status: ProjectionStatus::Fresh,
            stale_reason: None,
            summary: None,
        }
    }
}
