use super::*;
use crate::core::notification_prompt::{
    BellIndication, MessageLineCandidate, MessageLineSource, resolve_workspace_message_line,
};
use crate::presentation::screen_model::{
    PaneRect, ScreenCursorStyle, ScreenModel, WorkspaceProjectionError,
};
use crate::terminal::capability::{
    InlineGraphicsProbeResult, TerminalCapabilityObservation, TerminalCapabilityProbe,
    TerminalCapabilityProbeService, TerminalSessionKind,
};

fn capabilities_without_graphics() -> TerminalCapabilityProfile {
    TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: false,
            color_text: false,
            truecolor: false,
        },
        InlineGraphicsProbeResult::Disabled,
    )
    .detect()
}

fn workspace() -> WorkspaceScreenModel {
    WorkspaceScreenModel {
        panes: vec![ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 20,
                height: 4,
            },
            file_name: "sample.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            status_line: "sample.txt | NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["alpha".to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            filer_style_ranges: vec![],
            resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        }],
        floats: vec![],
        active_window_id: 1,
        message_line: resolve_workspace_message_line(vec![MessageLineCandidate::legacy(
            MessageLineSource::CoreNotification,
            "core note",
        )]),
        message_area_height: 5,
        message_scroll_offset: 0,
        prompt_line: None,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: Some(BellIndication { count: 1 }),
        command_line: None,
    }
}

#[test]
fn render_workspace_result_keeps_core_message_visible_when_projection_rolls_back() {
    let mut coordinator = TuiRenderCoordinator::new_headless(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let capabilities = capabilities_without_graphics();

    let first = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace()),
            &capabilities,
            &[],
            None,
        )
        .expect("initial render should succeed");
    assert_eq!(
        first.rendered_workspace.visible_message_source(),
        Some(MessageLineSource::CoreNotification)
    );

    let second = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Err(WorkspaceProjectionError::ActiveWindowMissing),
            &capabilities,
            &[],
            None,
        )
        .expect("rollback render should succeed");

    assert_eq!(
        second.rendered_workspace.visible_message_text(),
        Some("core note")
    );
    assert_eq!(
        second.rendered_workspace.visible_message_source(),
        Some(MessageLineSource::CoreNotification)
    );
    assert_eq!(
        second.rendered_workspace.suppressed_message_sources(),
        vec![MessageLineSource::RenderProjectionError]
    );
}

#[test]
fn render_workspace_result_retains_runtime_fallback_as_suppressed_when_core_message_exists() {
    let mut coordinator = TuiRenderCoordinator::new_headless(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let capabilities = capabilities_without_graphics();
    let outcome = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace()),
            &capabilities,
            &[RuntimePresentationIntent {
                content_key:
                    crate::presentation::overlay::effect::OverlayContentKey::RuntimeRegistered {
                        id: "runtime.preview".to_string(),
                    },
                target: crate::presentation::overlay::effect::OverlayTarget::StatusArea,
                fallback_text: "preview unavailable".to_string(),
            }],
            None,
        )
        .expect("render should succeed");

    assert_eq!(
        outcome.rendered_workspace.visible_message_source(),
        Some(MessageLineSource::CoreNotification)
    );
    assert_eq!(
        outcome.rendered_workspace.suppressed_message_sources(),
        vec![MessageLineSource::RuntimeOverlayFallback]
    );
}

#[test]
fn markdown_image_cell_height_preserves_tall_png_as_multiple_rows() {
    let media = OverlayAssetMedia::png("mermaid diagram", 86, 174, b"png".to_vec());

    assert_eq!(estimate_markdown_image_cell_height(&media, 17, 40), 18);
}

#[test]
fn markdown_image_cell_height_is_capped_by_remaining_pane_height() {
    let media = OverlayAssetMedia::png("mermaid diagram", 86, 174, b"png".to_vec());

    assert_eq!(estimate_markdown_image_cell_height(&media, 17, 8), 8);
}
