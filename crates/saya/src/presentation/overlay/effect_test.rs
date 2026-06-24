use super::*;
use crate::core::notification_prompt::{
    MessageLineCandidate, MessageLineSource, resolve_workspace_message_line,
};
use crate::presentation::screen_model::{PaneRect, ScreenCursorStyle, ScreenModel};
use crate::terminal::capability::{
    InlineGraphicsProbeResult, TerminalCapabilityObservation, TerminalCapabilityProbe,
    TerminalCapabilityProbeService, TerminalSessionKind,
};

fn workspace_model() -> WorkspaceScreenModel {
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
        message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
        message_area_height: 5,
        message_scroll_offset: 0,
        prompt_line: None,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: None,
        command_line: None,
    }
}

#[test]
fn projector_promotes_runtime_fallback_into_message_line_when_graphics_are_disabled() {
    let workspace = workspace_model();
    let capabilities = TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: false,
            color_text: false,
            truecolor: false,
        },
        InlineGraphicsProbeResult::Disabled,
    )
    .detect();
    let projector = PresentationEffectProjector;
    let presentation = projector.project(
        &workspace,
        &[RuntimePresentationIntent {
            content_key: OverlayContentKey::RuntimeRegistered {
                id: "runtime.preview".to_string(),
            },
            target: OverlayTarget::StatusArea,
            fallback_text: "preview unavailable".to_string(),
        }],
        &capabilities,
    );

    assert_eq!(
        presentation.visible_message_text(),
        Some("preview unavailable")
    );
    assert!(presentation.overlays.is_empty());
}

#[test]
fn projector_keeps_core_message_visible_and_retains_runtime_fallback_as_suppressed() {
    let mut workspace = workspace_model();
    workspace.message_line = resolve_workspace_message_line(vec![MessageLineCandidate::legacy(
        MessageLineSource::CoreNotification,
        "core note",
    )]);
    let capabilities = TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: false,
            color_text: false,
            truecolor: false,
        },
        InlineGraphicsProbeResult::Disabled,
    )
    .detect();
    let projector = PresentationEffectProjector;

    let presentation = projector.project(
        &workspace,
        &[RuntimePresentationIntent {
            content_key: OverlayContentKey::RuntimeRegistered {
                id: "runtime.preview".to_string(),
            },
            target: OverlayTarget::StatusArea,
            fallback_text: "preview unavailable".to_string(),
        }],
        &capabilities,
    );

    assert_eq!(presentation.visible_message_text(), Some("core note"));
    assert_eq!(
        presentation.message_line.visible_source(),
        Some(MessageLineSource::CoreNotification)
    );
    assert_eq!(
        presentation.message_line.suppressed_sources(),
        vec![MessageLineSource::RuntimeOverlayFallback]
    );
}
