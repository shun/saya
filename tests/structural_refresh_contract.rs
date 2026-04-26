use std::fs;

use saya::core_outcome::{RedrawEffect, StructuralEffectSet};
use saya::screen_model::{PaneProjectionGeometry, PaneRect, WorkspaceProjectionSummary};
use saya::structural_refresh::{
    ProjectionFailureDiagnostic, ProjectionStaleReason, ProjectionStatus, RedrawPlanSource,
    StructuralRefresh, ViewportRefreshStatus, ViewportSyncDiagnostic,
};
use saya::viewport::ViewportSyncSummary;

fn folded_effects(
    redraw: Option<RedrawEffect>,
    buffers: Vec<i32>,
    windows: Vec<i32>,
    layout_dirty: bool,
) -> StructuralEffectSet {
    StructuralEffectSet {
        redraw,
        invalidate_buffers: buffers,
        invalidate_windows: windows,
        layout_dirty,
    }
}

fn explicit_redraw(full: bool, clear_before_draw: bool, coalesced_count: usize) -> RedrawEffect {
    RedrawEffect {
        full,
        clear_before_draw,
        required_by_structure_change: false,
        coalesced_count,
    }
}

fn viewport_sync_summary() -> ViewportSyncSummary {
    let mut summary = ViewportSyncSummary::default();
    summary.live_window_ids.extend([20, 21]);
    summary.synced_window_ids.extend([20, 21]);
    summary.pruned_window_ids.insert(19);
    summary.reevaluated_window_ids.insert(20);
    summary.invalidated_missing_window_ids.insert(99);
    summary
}

fn projection_summary() -> WorkspaceProjectionSummary {
    WorkspaceProjectionSummary {
        window_ids: vec![20, 21],
        active_window_id: 21,
        pane_geometry: vec![
            PaneProjectionGeometry {
                window_id: 20,
                rect: PaneRect {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 10,
                },
            },
            PaneProjectionGeometry {
                window_id: 21,
                rect: PaneRect {
                    x: 40,
                    y: 0,
                    width: 40,
                    height: 10,
                },
            },
        ],
        visible_buffer_ids: vec![10, 11],
    }
}

#[test]
fn refresh_exposes_redraw_plan_and_structural_invalidations_independently() {
    let effects = folded_effects(
        Some(RedrawEffect {
            full: true,
            clear_before_draw: true,
            required_by_structure_change: true,
            coalesced_count: 4,
        }),
        vec![10, 11],
        vec![20],
        true,
    );

    let outcome = StructuralRefresh::from_folded_effects(&effects);

    assert!(outcome.redraw_plan.requested);
    assert!(outcome.redraw_plan.full);
    assert!(outcome.redraw_plan.clear_before_draw);
    assert!(outcome.redraw_plan.required_by_structure_change);
    assert_eq!(
        outcome.redraw_plan.source,
        RedrawPlanSource::ExplicitAndStructural
    );
    assert_eq!(outcome.redraw_plan.coalesced_count, 4);

    assert_eq!(outcome.invalidation.buffer_ids, vec![10, 11]);
    assert_eq!(outcome.invalidation.window_ids, vec![20]);
    assert!(outcome.invalidation.layout_dirty);
    assert!(outcome.invalidation.has_buffer_invalidation());
    assert!(outcome.invalidation.has_window_invalidation());
    assert!(outcome.invalidation.has_layout_invalidation());

    assert_eq!(outcome.projection.status, ProjectionStatus::Stale);
    assert_eq!(
        outcome.projection.stale_reason,
        Some(ProjectionStaleReason::StructuralInvalidation)
    );
}

#[test]
fn structural_invalidation_without_explicit_redraw_still_marks_projection_stale() {
    let effects = folded_effects(None, vec![3], vec![], false);

    let outcome = StructuralRefresh::from_folded_effects(&effects);

    assert!(outcome.redraw_plan.requested);
    assert!(outcome.redraw_plan.full);
    assert!(!outcome.redraw_plan.clear_before_draw);
    assert!(outcome.redraw_plan.required_by_structure_change);
    assert_eq!(
        outcome.redraw_plan.source,
        RedrawPlanSource::StructuralInvalidation
    );
    assert_eq!(outcome.redraw_plan.coalesced_count, 0);

    assert_eq!(outcome.projection.status, ProjectionStatus::Stale);
    assert_eq!(
        outcome.projection.stale_reason,
        Some(ProjectionStaleReason::StructuralInvalidation)
    );
}

#[test]
fn explicit_redraw_without_structural_invalidation_keeps_projection_fresh() {
    let effects = folded_effects(Some(explicit_redraw(false, true, 2)), vec![], vec![], false);

    let outcome = StructuralRefresh::from_folded_effects(&effects);

    assert!(outcome.redraw_plan.requested);
    assert!(!outcome.redraw_plan.full);
    assert!(outcome.redraw_plan.clear_before_draw);
    assert_eq!(outcome.redraw_plan.source, RedrawPlanSource::ExplicitCore);
    assert_eq!(outcome.projection.status, ProjectionStatus::Fresh);
    assert_eq!(outcome.projection.stale_reason, None);
}

#[test]
fn projection_failure_diagnostic_carries_refresh_context_without_rollback_state() {
    let effects = folded_effects(
        Some(RedrawEffect {
            full: true,
            clear_before_draw: false,
            required_by_structure_change: true,
            coalesced_count: 2,
        }),
        vec![7],
        vec![9],
        false,
    );

    let outcome = StructuralRefresh::from_folded_effects(&effects);
    let diagnostic = outcome.projection_failure(
        "active window 9 is missing",
        ViewportRefreshStatus::Deferred,
    );

    assert_eq!(
        diagnostic,
        ProjectionFailureDiagnostic {
            reason: "active window 9 is missing".to_string(),
            redraw_plan: outcome.redraw_plan.clone(),
            invalidation: outcome.invalidation.clone(),
            viewport_status: ViewportRefreshStatus::Deferred,
            projection_status: ProjectionStatus::Failed,
            projection_summary: None,
        }
    );
}

#[test]
fn refresh_enriches_viewport_status_and_single_projection_summary_after_main_sync() {
    let effects = folded_effects(
        Some(RedrawEffect {
            full: true,
            clear_before_draw: true,
            required_by_structure_change: true,
            coalesced_count: 3,
        }),
        vec![10],
        vec![20, 99],
        true,
    );

    let outcome = StructuralRefresh::from_folded_effects(&effects)
        .with_viewport_sync_summary(&viewport_sync_summary())
        .with_projection_summary(projection_summary());

    assert_eq!(
        outcome.viewport_status,
        ViewportRefreshStatus::Evaluated(ViewportSyncDiagnostic {
            live_window_count: 2,
            synced_window_count: 2,
            pruned_window_count: 1,
            reevaluated_window_count: 1,
            invalidated_missing_window_count: 1,
        })
    );
    assert_eq!(outcome.projection.status, ProjectionStatus::Fresh);
    assert_eq!(outcome.projection.stale_reason, None);
    let summary = outcome
        .projection
        .summary
        .expect("successful workspace projection should enrich structural diagnostics");
    assert_eq!(summary.window_ids, vec![20, 21]);
    assert_eq!(summary.active_window_id, 21);
    assert_eq!(summary.visible_buffer_ids, vec![10, 11]);
    assert_eq!(summary.pane_geometry.len(), 2);
}

#[test]
fn projection_failure_diagnostic_keeps_failure_reason_without_broken_summary() {
    let effects = folded_effects(None, vec![], vec![20], true);
    let outcome = StructuralRefresh::from_folded_effects(&effects)
        .with_viewport_sync_summary(&viewport_sync_summary());

    let diagnostic = outcome.projection_failure(
        "active window could not be resolved",
        outcome.viewport_status,
    );

    assert_eq!(diagnostic.projection_status, ProjectionStatus::Failed);
    assert_eq!(diagnostic.projection_summary, None);
    assert_eq!(diagnostic.reason, "active window could not be resolved");
    assert_eq!(
        diagnostic.viewport_status,
        ViewportRefreshStatus::Evaluated(ViewportSyncDiagnostic {
            live_window_count: 2,
            synced_window_count: 2,
            pruned_window_count: 1,
            reevaluated_window_count: 1,
            invalidated_missing_window_count: 1,
        })
    );
}

#[test]
fn projection_failure_debug_log_names_required_diagnostic_fields() {
    let source = fs::read_to_string("src/structural_refresh.rs")
        .expect("structural refresh source should be readable");

    let required_log_format = "[structural_refresh] projection failure: reason={}, redraw_requested={}, redraw_full={}, clear_before_draw={}, buffer_invalidations={}, window_invalidations={}, layout_dirty={}, viewport_status={:?}, projection_status={:?}";

    assert!(
        source.contains(required_log_format),
        "projection failure diagnostic log must name redraw intent, invalidation kind, viewport status, and projection status"
    );
}

#[test]
fn structural_refresh_boundary_excludes_prompt_notification_and_raw_core_decisions() {
    let source = fs::read_to_string("src/structural_refresh.rs")
        .expect("structural refresh source should be readable");

    for forbidden in [
        "NormalizedCoreOutcome",
        "CoreHostAction",
        "CoreEvent",
        "NormalizedPrompt",
        "NormalizedNotification",
        "PromptEffect",
        "NotificationEffect",
        "prompt_lifecycle",
        "notification_visibility",
        "rollback_state",
        "last_successful",
    ] {
        assert!(
            !source.contains(forbidden),
            "structural refresh must not own or import {forbidden}"
        );
    }
}
