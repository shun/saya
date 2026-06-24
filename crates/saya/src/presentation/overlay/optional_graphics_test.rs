use super::*;
use crate::core::notification_prompt::{MessageLineCandidate, resolve_workspace_message_line};
use crate::presentation::overlay::asset_store::{OverlayAssetMedia, OverlayAssetRef};
use crate::presentation::overlay::effect::{
    OverlayContentKey, OverlayTarget, PresentationOverlayIntent,
};
use crate::presentation::screen_model::{PaneRect, ScreenCursorStyle, ScreenModel};
use crate::terminal::capability::{
    InlineGraphicsProbeResult, TerminalCapabilityObservation, TerminalCapabilityProbe,
    TerminalCapabilityProbeService, TerminalSessionKind,
};

#[test]
fn adapter_negotiates_graphics_only_when_profile_supports_it() {
    let capabilities = TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: true,
            color_text: true,
            truecolor: true,
        },
        InlineGraphicsProbeResult::Supported(InlineGraphicsProtocol::Kitty),
    )
    .detect();

    assert_eq!(
        OptionalGraphicsAdapter::default().negotiate(&capabilities),
        Some(InlineGraphicsProtocol::Kitty)
    );
}

#[test]
fn adapter_projects_overlay_request_from_semantic_target() {
    let workspace = WorkspaceScreenModel {
        panes: vec![ScreenModel {
            window_id: 3,
            buffer_id: 1,
            rect: PaneRect {
                x: 2,
                y: 1,
                width: 20,
                height: 4,
            },
            file_name: "sample".to_string(),
            mode_label: "NORMAL".to_string(),
            status_line: "sample | NORMAL".to_string(),
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
        active_window_id: 3,
        message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
        message_area_height: 5,
        message_scroll_offset: 0,
        prompt_line: None,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: None,
        command_line: None,
    };
    let asset_ref = OverlayAssetRef {
        id: "overlay-1".to_string(),
    };
    let media = OverlayAssetMedia::png("preview", 12, 8, b"png".to_vec());
    let intent = PresentationOverlayIntent {
        content_key: OverlayContentKey::RuntimeRegistered {
            id: "runtime.preview".to_string(),
        },
        target: OverlayTarget::ActivePaneCorner,
        source_rect: None,
        fallback_text: "preview unavailable".to_string(),
    };
    let request = OptionalGraphicsAdapter::default()
        .project_request(
            &intent,
            OverlayAssetSnapshot {
                asset_ref: &asset_ref,
                metadata: &media.metadata,
                bytes: &media.bytes,
            },
            &workspace,
        )
        .expect("active pane corner should project");

    assert_eq!(request.cell_y, 1);
    assert_eq!(request.cell_height, 1);
    assert!(request.cell_x >= 2);
}

#[test]
fn kitty_encoder_uses_base64_payload_and_positions_cursor_before_image() {
    let asset_ref = OverlayAssetRef {
        id: "overlay-1".to_string(),
    };
    let media = OverlayAssetMedia::png("preview", 2, 1, b"png".to_vec());
    let request = GraphicsOverlayRequest {
        asset: OverlayAssetSnapshot {
            asset_ref: &asset_ref,
            metadata: &media.metadata,
            bytes: &media.bytes,
        },
        cell_x: 4,
        cell_y: 2,
        cell_width: 8,
        cell_height: 3,
        source_rect: None,
    };

    let encoded = encode_kitty_payload(&request);

    assert!(
        encoded.starts_with("\u{1b}[3;5H\u{1b}_G"),
        "kitty image should be emitted after cursor positioning, got {encoded:?}"
    );
    assert!(
        encoded.contains("a=T,f=100,s=2,v=1,c=8,r=3,m=0;"),
        "kitty control data should include PNG metadata and final chunk marker: {encoded:?}"
    );
    assert!(
        encoded.contains(";cG5n\u{1b}\\"),
        "kitty payload must be base64 PNG bytes, not hex: {encoded:?}"
    );
    assert!(
        !encoded.contains(";706e67"),
        "kitty payload must not use hex encoding"
    );
}

#[test]
fn kitty_clear_payload_deletes_visible_images() {
    assert_eq!(encode_kitty_clear_visible_payload(), "\u{1b}_Ga=d\u{1b}\\");
}
