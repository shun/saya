use std::fs;

use saya::core_notification_prompt::{
    BellIndication, MessageLineCandidate, MessageLineSource, resolve_workspace_message_line,
};
use saya::core_outcome::{RedrawEffect, StructuralEffectSet};
use saya::optional_graphics::{OptionalGraphicsAdapter, RecordingOverlayWriter};
use saya::overlay_asset_store::OverlayAssetStore;
use saya::screen_model::{
    CommandLineModel, PaneRect, ScreenCursorStyle, ScreenModel, WorkspaceProjectionError,
    WorkspaceScreenModel,
};
use saya::structural_refresh::{
    ProjectionFailureDiagnostic, ProjectionStatus, RedrawPlan, RedrawPlanSource, StructuralRefresh,
    ViewportRefreshStatus,
};
use saya::terminal_capability::{
    InlineGraphicsProbeResult, TerminalCapabilityObservation, TerminalCapabilityProbe,
    TerminalCapabilityProbeService, TerminalCapabilityProfile, TerminalSessionKind,
};
use saya::tui_render_coordinator::TuiRenderCoordinator;
use saya::tui_renderer::RenderFrameOptions;

fn capabilities_without_graphics() -> TerminalCapabilityProfile {
    TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: false,
            truecolor: false,
        },
        InlineGraphicsProbeResult::Disabled,
    )
    .detect()
}

fn workspace(window_id: i32, buffer_id: i32, line: &str, message: &str) -> WorkspaceScreenModel {
    WorkspaceScreenModel {
        panes: vec![ScreenModel {
            window_id,
            buffer_id,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 20,
                height: 4,
            },
            file_name: format!("buffer-{buffer_id}.txt"),
            mode_label: "NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec![line.to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![],
            syntax_chunks: vec![],
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        }],
        active_window_id: window_id,
        message_line: resolve_workspace_message_line(vec![MessageLineCandidate::legacy(
            MessageLineSource::CoreNotification,
            message,
        )]),
        prompt_line: None,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: None,
        command_line: None,
    }
}

#[test]
fn render_workspace_applies_active_cursor_style_to_writer() {
    let capabilities = capabilities_without_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut workspace = workspace(1, 101, "insert projection", "message");
    workspace.panes[0].cursor_style = ScreenCursorStyle::SteadyBar;
    let mut writer = RecordingOverlayWriter::default();

    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace),
            &capabilities,
            &[],
            Some(&mut writer),
        )
        .expect("render should apply cursor style");

    assert_eq!(writer.cursor_styles, vec![ScreenCursorStyle::SteadyBar]);
}

#[test]
fn render_workspace_prefers_command_line_cursor_style() {
    let capabilities = capabilities_without_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut workspace = workspace(1, 101, "normal projection", "message");
    workspace.panes[0].cursor_style = ScreenCursorStyle::Block;
    workspace.command_line = Some(saya::screen_model::CommandLineModel {
        text: ":write".to_string(),
        cursor_col: 6,
    });
    let mut writer = RecordingOverlayWriter::default();

    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace),
            &capabilities,
            &[],
            Some(&mut writer),
        )
        .expect("render should apply command line cursor style");

    assert_eq!(writer.cursor_styles, vec![ScreenCursorStyle::SteadyBar]);
}

#[test]
fn command_line_overlay_render_applies_command_cursor_style_without_workspace_render() {
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut writer = RecordingOverlayWriter::default();

    coordinator
        .render_command_line_overlay(
            &CommandLineModel {
                text: ":write".to_string(),
                cursor_col: 6,
            },
            Some(&mut writer),
        )
        .expect("command-line-only overlay should render through the lightweight path");

    assert_eq!(writer.cursor_styles, vec![ScreenCursorStyle::SteadyBar]);
}

#[test]
fn repeated_command_line_overlay_does_not_rewrite_unchanged_cursor_style() {
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut writer = RecordingOverlayWriter::default();

    for text in [":syntax o", ":syntax on"] {
        coordinator
            .render_command_line_overlay(
                &CommandLineModel {
                    text: text.to_string(),
                    cursor_col: u16::try_from(text.len()).unwrap(),
                },
                Some(&mut writer),
            )
            .expect("command-line-only overlay should render");
    }

    assert_eq!(writer.cursor_styles, vec![ScreenCursorStyle::SteadyBar]);
}

#[test]
fn projection_failure_rollback_applies_retained_cursor_style() {
    let capabilities = capabilities_without_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut first_valid = workspace(1, 101, "replace projection", "message");
    first_valid.panes[0].cursor_style = ScreenCursorStyle::UnderScore;

    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(first_valid),
            &capabilities,
            &[],
            None,
        )
        .expect("first render should succeed");

    let mut writer = RecordingOverlayWriter::default();
    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Err(WorkspaceProjectionError::ActiveWindowMissing),
            &capabilities,
            &[],
            Some(&mut writer),
        )
        .expect("rollback render should apply retained style");

    assert_eq!(writer.cursor_styles, vec![ScreenCursorStyle::UnderScore]);
}

fn workspace_without_message(window_id: i32, buffer_id: i32, line: &str) -> WorkspaceScreenModel {
    let mut workspace = workspace(window_id, buffer_id, line, "");
    workspace.message_line = resolve_workspace_message_line(Vec::<MessageLineCandidate>::new());
    workspace
}

fn structural_effects_for_projection_failure() -> StructuralEffectSet {
    StructuralEffectSet {
        redraw: Some(RedrawEffect {
            full: true,
            clear_before_draw: true,
            required_by_structure_change: true,
            coalesced_count: 3,
        }),
        invalidate_buffers: vec![101],
        invalidate_windows: vec![9],
        layout_dirty: true,
    }
}

#[test]
fn projection_failure_diagnostic_has_refresh_context_and_retains_last_valid_screen() {
    let refresh =
        StructuralRefresh::from_folded_effects(&structural_effects_for_projection_failure());
    let diagnostic = refresh.projection_failure(
        "window not found: window_id=9",
        ViewportRefreshStatus::Deferred,
    );

    assert_eq!(
        diagnostic,
        ProjectionFailureDiagnostic {
            reason: "window not found: window_id=9".to_string(),
            redraw_plan: refresh.redraw_plan.clone(),
            invalidation: refresh.invalidation.clone(),
            viewport_status: ViewportRefreshStatus::Deferred,
            projection_status: ProjectionStatus::Failed,
            projection_summary: None,
        }
    );
    assert_eq!(
        diagnostic.redraw_plan.source,
        RedrawPlanSource::ExplicitAndStructural
    );
    assert!(diagnostic.redraw_plan.full);
    assert!(diagnostic.redraw_plan.clear_before_draw);
    assert_eq!(diagnostic.invalidation.buffer_ids, vec![101]);
    assert_eq!(diagnostic.invalidation.window_ids, vec![9]);
    assert!(diagnostic.invalidation.layout_dirty);

    let capabilities = capabilities_without_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let first_valid = workspace(1, 101, "valid before failure", "initial projection");

    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(first_valid.clone()),
            &capabilities,
            &[],
            None,
        )
        .expect("first valid workspace should render");

    let rollback = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Err(WorkspaceProjectionError::WindowNotFound { window_id: 9 }),
            &capabilities,
            &[],
            None,
        )
        .expect("projection failure should render the retained workspace");

    assert_eq!(rollback.rendered_workspace.panes, first_valid.panes);
    assert_eq!(
        rollback.rendered_workspace.visible_message_text(),
        Some("initial projection")
    );
    assert_eq!(
        rollback.rendered_workspace.suppressed_message_sources(),
        vec![MessageLineSource::RenderProjectionError]
    );
}

#[test]
fn valid_refresh_after_projection_failure_replaces_the_retained_screen() {
    let capabilities = capabilities_without_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );

    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace(1, 101, "old valid projection", "old message")),
            &capabilities,
            &[],
            None,
        )
        .expect("first valid workspace should render");
    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Err(WorkspaceProjectionError::ActiveWindowMissing),
            &capabilities,
            &[],
            None,
        )
        .expect("projection failure should render the retained workspace");

    let latest = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace(
                2,
                202,
                "latest valid projection",
                "latest message",
            )),
            &capabilities,
            &[],
            None,
        )
        .expect("next valid workspace should replace retained state");

    assert_eq!(latest.rendered_workspace.active_window_id, 2);
    assert_eq!(latest.rendered_workspace.panes[0].buffer_id, 202);
    assert_eq!(
        latest.rendered_workspace.panes[0].lines,
        vec!["latest valid projection".to_string()]
    );
    assert_eq!(
        latest.rendered_workspace.visible_message_text(),
        Some("latest message")
    );
}

#[test]
fn projection_failure_render_does_not_replace_last_successful_workspace_state() {
    let capabilities = capabilities_without_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );

    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace_without_message(
                1,
                101,
                "valid projection before failures",
            )),
            &capabilities,
            &[],
            None,
        )
        .expect("first valid workspace should render");

    let first_failure = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Err(WorkspaceProjectionError::WindowNotFound { window_id: 9 }),
            &capabilities,
            &[],
            None,
        )
        .expect("first projection failure should render rollback workspace");
    assert!(
        first_failure
            .rendered_workspace
            .visible_message_text()
            .is_some_and(|message| message.contains("window_id=9")),
        "first failure should be operator-visible: {:?}",
        first_failure.rendered_workspace.visible_message_text()
    );

    let second_failure = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Err(WorkspaceProjectionError::WindowNotFound { window_id: 10 }),
            &capabilities,
            &[],
            None,
        )
        .expect("second projection failure should still use the original retained workspace");

    assert_eq!(second_failure.rendered_workspace.panes[0].window_id, 1);
    assert_eq!(
        second_failure.rendered_workspace.panes[0].lines,
        vec!["valid projection before failures".to_string()]
    );
    assert!(
        second_failure
            .rendered_workspace
            .visible_message_text()
            .is_some_and(|message| message.contains("window_id=10")),
        "second failure must be derived from the new failure, not a prior rollback render: {:?}",
        second_failure.rendered_workspace.visible_message_text()
    );
}

fn redraw_plan_for_full_clear() -> RedrawPlan {
    StructuralRefresh::from_folded_effects(&structural_effects_for_projection_failure()).redraw_plan
}

#[test]
fn renderer_option_propagation_preserves_full_and_clear_before_draw() {
    let capabilities = capabilities_without_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let redraw_plan = redraw_plan_for_full_clear();

    let outcome = coordinator
        .render_workspace_result_with_redraw_plan::<WorkspaceProjectionError>(
            Ok(workspace(1, 101, "valid projection", "message")),
            &capabilities,
            &[],
            None,
            redraw_plan.clone(),
        )
        .expect("render should succeed with task 4 redraw options");

    assert_eq!(outcome.redraw_plan, redraw_plan);
    assert_eq!(
        outcome.frame_options,
        RenderFrameOptions {
            full_redraw: true,
            clear_before_draw: true,
        }
    );
}

#[test]
fn projection_failure_outcome_exposes_diagnostic_and_retained_state() {
    let capabilities = capabilities_without_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let refresh =
        StructuralRefresh::from_folded_effects(&structural_effects_for_projection_failure());

    coordinator
        .render_workspace_result_with_structural_refresh::<WorkspaceProjectionError>(
            Ok(workspace(
                1,
                101,
                "valid before failure",
                "initial projection",
            )),
            &capabilities,
            &[],
            None,
            Some(&refresh),
        )
        .expect("initial render should succeed");

    let rollback = coordinator
        .render_workspace_result_with_structural_refresh::<WorkspaceProjectionError>(
            Err(WorkspaceProjectionError::WindowNotFound { window_id: 9 }),
            &capabilities,
            &[],
            None,
            Some(&refresh),
        )
        .expect("projection failure should render retained workspace");

    assert_eq!(rollback.rendered_workspace.panes[0].window_id, 1);
    let diagnostic = rollback
        .projection_failure
        .as_ref()
        .expect("projection failure diagnostic should be retained on rollback outcome");
    assert!(diagnostic.reason.contains("window_id=9"));
    assert_eq!(diagnostic.redraw_plan, refresh.redraw_plan);
    assert_eq!(diagnostic.invalidation, refresh.invalidation);
    assert_eq!(diagnostic.projection_status, ProjectionStatus::Failed);
    assert_eq!(
        rollback.frame_options,
        RenderFrameOptions {
            full_redraw: true,
            clear_before_draw: true,
        }
    );
}

#[test]
fn unresolved_projection_failure_keeps_failure_diagnostic_separate_from_retained_projection_until_next_success()
 {
    let capabilities = capabilities_without_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let refresh =
        StructuralRefresh::from_folded_effects(&structural_effects_for_projection_failure());

    coordinator
        .render_workspace_result_with_structural_refresh::<WorkspaceProjectionError>(
            Ok(workspace(
                1,
                101,
                "retained valid projection",
                "retained message",
            )),
            &capabilities,
            &[],
            None,
            Some(&refresh),
        )
        .expect("initial render should establish a retained projection");

    let unresolved_failure = coordinator
        .render_workspace_result_with_structural_refresh::<WorkspaceProjectionError>(
            Err(WorkspaceProjectionError::WindowNotFound { window_id: 9 }),
            &capabilities,
            &[],
            None,
            Some(&refresh),
        )
        .expect("projection failure should render retained workspace");

    assert_eq!(unresolved_failure.rendered_workspace.active_window_id, 1);
    assert_eq!(
        unresolved_failure.rendered_workspace.panes[0].lines,
        vec!["retained valid projection".to_string()]
    );
    let diagnostic = unresolved_failure
        .projection_failure
        .as_ref()
        .expect("unresolved failure must expose diagnostic state separately");
    assert_eq!(diagnostic.projection_status, ProjectionStatus::Failed);
    assert_eq!(diagnostic.redraw_plan, refresh.redraw_plan);
    assert_eq!(diagnostic.invalidation, refresh.invalidation);
    assert_eq!(diagnostic.viewport_status, refresh.viewport_status);

    let latest_success = coordinator
        .render_workspace_result_with_structural_refresh::<WorkspaceProjectionError>(
            Ok(workspace(
                2,
                202,
                "latest valid projection",
                "latest message",
            )),
            &capabilities,
            &[],
            None,
            Some(&refresh),
        )
        .expect("next valid refresh should replace retained projection");

    assert_eq!(latest_success.rendered_workspace.active_window_id, 2);
    assert_eq!(latest_success.rendered_workspace.panes[0].buffer_id, 202);
    assert_eq!(latest_success.projection_failure, None);
    assert_eq!(
        latest_success.rendered_workspace.panes[0].lines,
        vec!["latest valid projection".to_string()]
    );
}

#[test]
fn renderer_option_contract_is_no_longer_source_only_future_guard() {
    let coordinator_source = fs::read_to_string("src/tui_render_coordinator.rs")
        .expect("render coordinator source should be readable");
    let renderer_source =
        fs::read_to_string("src/tui_renderer.rs").expect("renderer source should be readable");

    assert!(
        coordinator_source.contains("RedrawPlan"),
        "task 4 requires coordinator to keep RedrawPlan as render semantics"
    );
    assert!(
        coordinator_source.contains("draw_with_mode_and_options"),
        "task 4 requires coordinator to pass durable frame options to renderer"
    );
    assert!(
        renderer_source.contains("clear_before_draw"),
        "task 4 requires renderer to honor core-derived clear-before-draw"
    );
}

#[test]
fn render_workspace_emits_terminal_bell_signal_and_keeps_visible_marker() {
    let capabilities = capabilities_without_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut workspace = workspace(1, 1, "alpha", "saved");
    workspace.bell = Some(BellIndication { count: 2 });
    let mut writer = RecordingOverlayWriter::default();

    let outcome = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace),
            &capabilities,
            &[],
            Some(&mut writer),
        )
        .expect("workspace render should succeed");

    assert_eq!(writer.writes, vec![vec![b'\x07', b'\x07']]);
    assert_eq!(
        outcome.rendered_workspace.bell.map(|bell| bell.count),
        Some(2)
    );
}
