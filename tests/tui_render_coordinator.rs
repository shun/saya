use std::fs;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

use saya::core::notification_prompt::{
    BellIndication, MessageLineCandidate, MessageLineSource, resolve_workspace_message_line,
};
use saya::core::outcome::{RedrawEffect, StructuralEffectSet};
use saya::presentation::floating_window::{
    FloatingBorder, FloatingChrome, FloatingContentRef, FloatingImage, FloatingImageSource,
    FloatingImageView, FloatingScreenModel, FloatingWindowId,
};
use saya::presentation::markdown::render::MermaidDiagramRenderer;
use saya::presentation::overlay::asset_store::OverlayAssetMedia;
use saya::presentation::overlay::asset_store::OverlayAssetStore;
use saya::presentation::overlay::optional_graphics::{
    OptionalGraphicsAdapter, OverlayRenderResult, RecordingOverlayWriter,
};
use saya::presentation::render::coordinator::TuiRenderCoordinator;
use saya::presentation::render::renderer::{RenderFrameOptions, RenderTextMode};
use saya::presentation::screen_model::{
    CommandLineModel, PaneRect, ScreenCursorStyle, ScreenModel, ScreenSyntaxChunk,
    WorkspaceProjectionError, WorkspaceScreenModel,
};
use saya::presentation::structural_refresh::{
    ProjectionFailureDiagnostic, ProjectionStatus, RedrawPlan, RedrawPlanSource, StructuralRefresh,
    ViewportRefreshStatus,
};
use saya::terminal::capability::{
    InlineGraphicsProbeResult, TerminalCapabilityObservation, TerminalCapabilityProbe,
    TerminalCapabilityProbeService, TerminalCapabilityProfile, TerminalSessionKind,
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

fn capabilities_with_kitty_graphics() -> TerminalCapabilityProfile {
    TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: true,
            color_text: true,
            truecolor: true,
        },
        InlineGraphicsProbeResult::Supported(
            saya::terminal::capability::InlineGraphicsProtocol::Kitty,
        ),
    )
    .detect()
}

#[derive(Debug)]
struct FakeMermaidRenderer;

impl MermaidDiagramRenderer for FakeMermaidRenderer {
    fn render_png(&self, _source: &str, _background: &str) -> Result<OverlayAssetMedia, String> {
        panic!("floating Mermaid popup must not call synchronous render_png")
    }

    fn render_png_async(
        &self,
        source: String,
        background: String,
    ) -> Option<mpsc::Receiver<Result<OverlayAssetMedia, String>>> {
        assert_eq!(
            source, "graph TD\n  A-->B",
            "coordinator must pass only the mermaid fenced body to the renderer"
        );
        assert_eq!(background, "transparent");
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(OverlayAssetMedia::png(
                "mermaid diagram",
                320,
                180,
                b"fake-png".to_vec(),
            )))
            .expect("send fake mermaid render");
        Some(receiver)
    }
}

#[derive(Debug)]
struct SyncOnlyMermaidRenderer;

impl MermaidDiagramRenderer for SyncOnlyMermaidRenderer {
    fn render_png(&self, _source: &str, _background: &str) -> Result<OverlayAssetMedia, String> {
        panic!("floating Mermaid popup must not call synchronous render_png")
    }
}

#[derive(Debug)]
struct FailingMermaidRenderer;

impl MermaidDiagramRenderer for FailingMermaidRenderer {
    fn render_png(&self, _source: &str, _background: &str) -> Result<OverlayAssetMedia, String> {
        panic!("floating Mermaid popup must not call synchronous render_png")
    }

    fn render_png_async(
        &self,
        _source: String,
        _background: String,
    ) -> Option<mpsc::Receiver<Result<OverlayAssetMedia, String>>> {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Err(
                "mmdc exited with exit status: 1: stderr=Error: Parse error on line 3:\n...classDef正常 fill:#d4edda\n----------------------^\nExpecting 'SPACE', got 'UNICODE_TEXT'".to_string(),
            ))
            .expect("send fake mermaid failure");
        Some(receiver)
    }
}

#[derive(Debug)]
struct CountingMermaidRenderer {
    calls: Arc<AtomicUsize>,
}

impl MermaidDiagramRenderer for CountingMermaidRenderer {
    fn render_png(&self, _source: &str, _background: &str) -> Result<OverlayAssetMedia, String> {
        panic!("floating Mermaid popup must not call synchronous render_png")
    }

    fn render_png_async(
        &self,
        source: String,
        background: String,
    ) -> Option<mpsc::Receiver<Result<OverlayAssetMedia, String>>> {
        assert_eq!(source, "graph TD\n  A-->B");
        assert_eq!(background, "transparent");
        self.calls.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(OverlayAssetMedia::png(
                "mermaid diagram",
                320,
                180,
                b"fake-png".to_vec(),
            )))
            .expect("send counted mermaid render");
        Some(receiver)
    }
}

#[derive(Debug)]
struct AsyncMermaidRenderer {
    sender: Arc<Mutex<Option<mpsc::Sender<Result<OverlayAssetMedia, String>>>>>,
}

impl MermaidDiagramRenderer for AsyncMermaidRenderer {
    fn render_png(&self, _source: &str, _background: &str) -> Result<OverlayAssetMedia, String> {
        panic!("async-capable Mermaid renderer must not block through render_png")
    }

    fn render_png_async(
        &self,
        source: String,
        background: String,
    ) -> Option<mpsc::Receiver<Result<OverlayAssetMedia, String>>> {
        assert_eq!(source, "graph TD\n  A-->B");
        assert_eq!(background, "transparent");
        let (sender, receiver) = mpsc::channel();
        *self.sender.lock().expect("sender lock") = Some(sender);
        Some(receiver)
    }
}

#[derive(Debug)]
struct BackgroundAssertingMermaidRenderer;

impl MermaidDiagramRenderer for BackgroundAssertingMermaidRenderer {
    fn render_png(&self, _source: &str, _background: &str) -> Result<OverlayAssetMedia, String> {
        panic!("floating Mermaid popup must not call synchronous render_png")
    }

    fn render_png_async(
        &self,
        source: String,
        background: String,
    ) -> Option<mpsc::Receiver<Result<OverlayAssetMedia, String>>> {
        assert_eq!(source, "graph TD\n  A-->B");
        assert_eq!(background, "#ffffff");
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(OverlayAssetMedia::png(
                "mermaid diagram",
                320,
                180,
                b"fake-png".to_vec(),
            )))
            .expect("send background mermaid render");
        Some(receiver)
    }
}

fn markdown_mermaid_popup_workspace() -> WorkspaceScreenModel {
    let mut workspace = workspace_without_message(1, 101, "```mermaid");
    workspace.floats = vec![FloatingScreenModel {
        id: FloatingWindowId(9001),
        content: FloatingContentRef::StaticLines { content_id: 9001 },
        rect: PaneRect {
            x: 20,
            y: 2,
            width: 24,
            height: 8,
        },
        lines: vec!["Mermaid preview".to_string(), "".to_string()],
        inline_styles: Vec::new(),
        images: vec![FloatingImage {
            line: 1,
            column: 0,
            max_width: 22,
            max_height: 6,
            view: FloatingImageView::fit(),
            source: FloatingImageSource::Mermaid {
                buffer_id: 101,
                row: 4,
                alt_text: "mermaid diagram".to_string(),
                background: "transparent".to_string(),
                source: "graph TD\n  A-->B".to_string(),
            },
        }],
        cursor: None,
        focusable: false,
        mouse: false,
        chrome: FloatingChrome {
            border: FloatingBorder::Single,
        },
        zindex: 40,
        creation_order: 1,
    }];
    workspace
}

fn markdown_mermaid_zoomed_popup_workspace() -> WorkspaceScreenModel {
    let mut workspace = markdown_mermaid_popup_workspace();
    workspace.floats[0].images[0].view = FloatingImageView {
        zoom_percent: Some(200),
        pan_x_px: 32,
        pan_y_px: 24,
    };
    workspace
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
            status_line: "test.txt | NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec![line.to_string()],
            line_projections: vec![],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            search_overlays: vec![],
            syntax_chunks: vec![],
            markdown_style_ranges: vec![],
            filer_style_ranges: vec![],
            resolved_theme: saya::presentation::theme::ResolvedTheme::default(),
            message_line: None,
            command_cursor_col: None,
            is_active: true,
        }],
        floats: vec![],
        active_window_id: window_id,
        message_line: resolve_workspace_message_line(vec![MessageLineCandidate::legacy(
            MessageLineSource::CoreNotification,
            message,
        )]),
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
    workspace.command_line = Some(saya::presentation::screen_model::CommandLineModel {
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
fn render_workspace_registers_floating_mermaid_png_and_calls_kitty_overlay() {
    let capabilities = capabilities_with_kitty_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    )
    .with_mermaid_renderer_for_tests(Box::new(FakeMermaidRenderer));
    let mut writer = RecordingOverlayWriter::default();

    let outcome = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(markdown_mermaid_popup_workspace()),
            &capabilities,
            &[],
            Some(&mut writer),
        )
        .expect("floating markdown mermaid overlay should render through kitty");

    assert_eq!(outcome.overlay_results, vec![OverlayRenderResult::Rendered]);
    assert_eq!(
        outcome.rendered_workspace.panes[0].lines,
        vec!["```mermaid".to_string()],
        "Mermaid source text must remain in the body; popup preview must not reserve body rows"
    );
    assert!(
        writer.writes.iter().any(|write| {
            let text = String::from_utf8_lossy(write);
            text.starts_with("\u{1b}[5;22H")
                && text.contains(",c=20,r=6,")
                && text.contains(";ZmFrZS1wbmc=")
        }),
        "kitty overlay write should target the float content cell without stretching to the full popup width: {:?}",
        writer.writes
    );
    assert_eq!(
        writer
            .writes
            .last()
            .map(|write| String::from_utf8_lossy(write).into_owned()),
        Some("\u{1b}[1;1H".to_string()),
        "coordinator must restore the body cursor after writing popup kitty graphics"
    );
}

#[test]
fn render_workspace_zoomed_mermaid_popup_uses_kitty_source_rectangle() {
    let capabilities = capabilities_with_kitty_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    )
    .with_mermaid_renderer_for_tests(Box::new(FakeMermaidRenderer));
    let mut writer = RecordingOverlayWriter::default();

    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(markdown_mermaid_zoomed_popup_workspace()),
            &capabilities,
            &[],
            Some(&mut writer),
        )
        .expect("zoomed floating markdown mermaid overlay should render through kitty");

    assert!(
        writer.writes.iter().any(|write| {
            let text = String::from_utf8_lossy(write);
            text.contains(",x=32,y=24,")
                && text.contains(",w=")
                && text.contains(",h=")
                && text.contains(",c=22,r=6,")
        }),
        "zoomed kitty overlay should crop the source image instead of stretching the whole image: {:?}",
        writer.writes
    );
}

#[test]
fn render_workspace_passes_mermaid_background_to_renderer() {
    let capabilities = capabilities_with_kitty_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    )
    .with_mermaid_renderer_for_tests(Box::new(BackgroundAssertingMermaidRenderer));
    let mut writer = RecordingOverlayWriter::default();
    let mut workspace = markdown_mermaid_popup_workspace();
    let FloatingImageSource::Mermaid { background, .. } = &mut workspace.floats[0].images[0].source;
    *background = "#ffffff".to_string();

    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace),
            &capabilities,
            &[],
            Some(&mut writer),
        )
        .expect("floating markdown mermaid overlay should render with configured background");

    assert!(
        writer.writes.iter().any(|write| !write.is_empty()),
        "renderer success should reach kitty overlay"
    );
}

#[test]
fn render_workspace_does_not_block_on_async_mermaid_cache_miss() {
    let capabilities = capabilities_with_kitty_graphics();
    let sender = Arc::new(Mutex::new(None));
    let redraws = Arc::new(AtomicUsize::new(0));
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    )
    .with_mermaid_renderer_for_tests(Box::new(AsyncMermaidRenderer {
        sender: sender.clone(),
    }));
    {
        let redraws = redraws.clone();
        coordinator.set_mermaid_redraw_callback(move || {
            redraws.fetch_add(1, Ordering::SeqCst);
        });
    }
    let mut first_writer = RecordingOverlayWriter::default();
    let mut second_writer = RecordingOverlayWriter::default();

    let first = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(markdown_mermaid_popup_workspace()),
            &capabilities,
            &[],
            Some(&mut first_writer),
        )
        .expect("first frame should not block on Mermaid conversion");

    assert_eq!(
        first.overlay_results,
        vec![OverlayRenderResult::FallbackToText]
    );
    assert!(
        first.rendered_workspace.floats[0]
            .lines
            .iter()
            .any(|line| line.contains("Rendering Mermaid")),
        "async cache miss frame should show a non-empty pending message in the popup"
    );
    assert!(
        first_writer
            .writes
            .iter()
            .all(|write| !String::from_utf8_lossy(write).contains(";ZmFrZS1wbmc=")),
        "cache miss frame must not wait for or write a PNG payload"
    );

    sender
        .lock()
        .expect("sender lock")
        .take()
        .expect("async renderer should expose sender")
        .send(Ok(OverlayAssetMedia::png(
            "mermaid diagram",
            320,
            180,
            b"fake-png".to_vec(),
        )))
        .expect("send async render result");
    for _ in 0..50 {
        if redraws.load(Ordering::SeqCst) > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        redraws.load(Ordering::SeqCst),
        1,
        "async Mermaid completion should request one redraw"
    );

    let second = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(markdown_mermaid_popup_workspace()),
            &capabilities,
            &[],
            Some(&mut second_writer),
        )
        .expect("second frame should consume async Mermaid result");

    assert_eq!(second.overlay_results, vec![OverlayRenderResult::Rendered]);
    assert_eq!(
        second.rendered_workspace.floats[0].lines[1],
        " ".repeat(22),
        "completed image frames should clear fallback text behind transparent PNGs"
    );
    assert!(
        second_writer
            .writes
            .iter()
            .any(|write| String::from_utf8_lossy(write).contains(";ZmFrZS1wbmc=")),
        "completed async render should be emitted on the next frame"
    );
}

#[test]
fn render_workspace_never_uses_synchronous_mermaid_renderer_for_popup() {
    let capabilities = capabilities_with_kitty_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    )
    .with_mermaid_renderer_for_tests(Box::new(SyncOnlyMermaidRenderer));
    let mut writer = RecordingOverlayWriter::default();

    let outcome = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(markdown_mermaid_popup_workspace()),
            &capabilities,
            &[],
            Some(&mut writer),
        )
        .expect("sync-only Mermaid renderer should fall back without blocking");

    assert_eq!(
        outcome.overlay_results,
        vec![OverlayRenderResult::FallbackToText]
    );
    assert!(
        writer
            .writes
            .iter()
            .all(|write| !String::from_utf8_lossy(write).contains(";ZmFrZS1wbmc=")),
        "sync-only renderer must not emit a PNG payload"
    );
}

#[test]
fn render_workspace_reuses_cached_mermaid_png_across_repeated_frames() {
    let capabilities = capabilities_with_kitty_graphics();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    )
    .with_mermaid_renderer_for_tests(Box::new(CountingMermaidRenderer {
        calls: calls.clone(),
    }));
    let mut first_writer = RecordingOverlayWriter::default();
    let mut second_writer = RecordingOverlayWriter::default();

    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(markdown_mermaid_popup_workspace()),
            &capabilities,
            &[],
            Some(&mut first_writer),
        )
        .expect("first markdown mermaid frame should render");
    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(markdown_mermaid_popup_workspace()),
            &capabilities,
            &[],
            Some(&mut second_writer),
        )
        .expect("second markdown mermaid frame should reuse cached PNG");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "scroll/cursor redraws must not invoke Mermaid conversion again for unchanged source"
    );
    assert!(
        second_writer
            .writes
            .iter()
            .any(|write| String::from_utf8_lossy(write).contains(";ZmFrZS1wbmc=")),
        "cached PNG should still be emitted as a kitty overlay on the second frame"
    );
}

#[test]
fn render_workspace_clears_stale_kitty_image_when_next_frame_has_no_overlay() {
    let capabilities = capabilities_with_kitty_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    )
    .with_mermaid_renderer_for_tests(Box::new(FakeMermaidRenderer));
    let mut first_writer = RecordingOverlayWriter::default();
    let mut second_writer = RecordingOverlayWriter::default();

    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(markdown_mermaid_popup_workspace()),
            &capabilities,
            &[],
            Some(&mut first_writer),
        )
        .expect("first frame should render kitty image");
    coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace_without_message(1, 101, "plain text after scroll")),
            &capabilities,
            &[],
            Some(&mut second_writer),
        )
        .expect("next frame should clear stale kitty image");

    assert!(
        second_writer
            .writes
            .iter()
            .any(|write| write == b"\x1b_Ga=d\x1b\\"),
        "frame without overlays must delete visible kitty images from prior frames: {:?}",
        second_writer.writes
    );
}

#[test]
fn render_workspace_restores_mermaid_text_when_kitty_graphics_are_disabled() {
    let capabilities = capabilities_without_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    )
    .with_mermaid_renderer_for_tests(Box::new(FakeMermaidRenderer));
    let mut writer = RecordingOverlayWriter::default();

    let outcome = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(markdown_mermaid_popup_workspace()),
            &capabilities,
            &[],
            Some(&mut writer),
        )
        .expect("markdown mermaid popup should skip graphics on unsupported terminals");

    assert!(outcome.overlay_results.is_empty());
    assert_eq!(
        outcome.rendered_workspace.panes[0].lines,
        vec!["```mermaid".to_string()],
        "non-graphics terminals should keep the raw markdown body unchanged"
    );
    assert!(
        outcome.rendered_workspace.floats[0]
            .lines
            .iter()
            .any(|line| line.contains("graph TD")),
        "non-graphics terminals should show Mermaid source inside the popup instead of an empty body"
    );
    assert!(
        writer.writes.is_empty(),
        "graphics-disabled fallback must not write kitty payloads"
    );
}

#[test]
fn render_workspace_restores_mermaid_text_when_png_conversion_fails() {
    let capabilities = capabilities_with_kitty_graphics();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    )
    .with_mermaid_renderer_for_tests(Box::new(FailingMermaidRenderer));
    let mut writer = RecordingOverlayWriter::default();

    let outcome = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(markdown_mermaid_popup_workspace()),
            &capabilities,
            &[],
            Some(&mut writer),
        )
        .expect("markdown mermaid conversion failure should fall back to text");

    assert_eq!(
        outcome.overlay_results,
        vec![OverlayRenderResult::FallbackToText]
    );
    assert_eq!(
        outcome.rendered_workspace.panes[0].lines,
        vec!["```mermaid".to_string()],
        "conversion failure must not rewrite markdown body lines"
    );
    assert!(
        outcome.rendered_workspace.floats[0]
            .lines
            .iter()
            .any(|line| line.contains("Mermaid render failed")),
        "conversion failure should show the reason inside the popup instead of only source text"
    );
    assert!(
        outcome.rendered_workspace.floats[0]
            .lines
            .iter()
            .any(|line| line.contains("Mermaid line 3")),
        "conversion failure should expose the mmdc Mermaid body line in the popup"
    );
    assert_eq!(
        outcome.rendered_workspace.message_line.visible_text(),
        Some("Mermaid render failed: Error: Parse error on line 3:"),
        "conversion failure should be visible without checking logs"
    );
    assert!(writer.writes.is_empty());
}

#[test]
fn render_workspace_mermaid_parse_error_points_to_source_line_and_hint() {
    #[derive(Debug)]
    struct StyleSeparatorFailure;

    impl MermaidDiagramRenderer for StyleSeparatorFailure {
        fn render_png(
            &self,
            _source: &str,
            _background: &str,
        ) -> Result<OverlayAssetMedia, String> {
            panic!("floating Mermaid popup must not call synchronous render_png")
        }

        fn render_png_async(
            &self,
            _source: String,
            _background: String,
        ) -> Option<mpsc::Receiver<Result<OverlayAssetMedia, String>>> {
            let (sender, receiver) = mpsc::channel();
            sender
                .send(Err(
                    "mmdc exited with exit status: 1: stderr=Error: Parse error on line 2:\n\
                     ...A([start]) :::startEnd --> B\n\
                     -----------------------^\n\
                     Expecting 'SEMI', 'NEWLINE', 'SPACE', got 'STYLE_SEPARATOR'"
                        .to_string(),
                ))
                .expect("send style separator failure");
            Some(receiver)
        }
    }

    let mut workspace = markdown_mermaid_popup_workspace();
    let fence_row = 13;
    let expected_editor_line = fence_row + 3 + 1;
    let FloatingImageSource::Mermaid { row, .. } = &mut workspace.floats[0].images[0].source;
    *row = fence_row;
    let FloatingImageSource::Mermaid { source, .. } = &mut workspace.floats[0].images[0].source;
    *source = "flowchart TD\n\n  A([start]) :::startEnd --> B".to_string();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    )
    .with_mermaid_renderer_for_tests(Box::new(StyleSeparatorFailure));
    let mut writer = RecordingOverlayWriter::default();

    let outcome = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace),
            &capabilities_with_kitty_graphics(),
            &[],
            Some(&mut writer),
        )
        .expect("style separator parse failure should render diagnostics");

    let popup_lines = &outcome.rendered_workspace.floats[0].lines;
    assert!(
        popup_lines
            .iter()
            .any(|line| line.contains("Mermaid line 2")),
        "popup should keep the mmdc Mermaid body line for diagnostics: {popup_lines:?}"
    );
    assert!(
        popup_lines
            .iter()
            .any(|line| line.contains(&format!("Fix editor line {expected_editor_line}"))),
        "popup should show the editor line number, not only the Mermaid body line: {popup_lines:?}"
    );
    assert!(
        popup_lines.iter().any(|line| line.contains("A([start])")),
        "popup should show the offending Mermaid source line: {popup_lines:?}"
    );
    assert!(
        popup_lines
            .iter()
            .any(|line| line.contains("remove space before")),
        "popup should say what to edit: {popup_lines:?}"
    );
    assert!(
        popup_lines.iter().any(|line| line.contains("Node:::class")),
        "popup should suggest the concrete class syntax fix: {popup_lines:?}"
    );
    assert!(writer.writes.is_empty());
}

#[test]
fn render_workspace_mermaid_subgraph_label_error_suggests_bracket_label() {
    #[derive(Debug)]
    struct SubgraphLabelFailure;

    impl MermaidDiagramRenderer for SubgraphLabelFailure {
        fn render_png(
            &self,
            _source: &str,
            _background: &str,
        ) -> Result<OverlayAssetMedia, String> {
            panic!("floating Mermaid popup must not call synchronous render_png")
        }

        fn render_png_async(
            &self,
            _source: String,
            _background: String,
        ) -> Option<mpsc::Receiver<Result<OverlayAssetMedia, String>>> {
            let (sender, receiver) = mpsc::channel();
            sender
                .send(Err(
                    "mmdc exited with exit status: 1: stderr=Error: Lexical error on line 3. Unrecognized text.\n\
                     ...A-->B  subgraph 配送・通知システム    C-->D\n\
                     ----------------------^"
                        .to_string(),
                ))
                .expect("send subgraph label failure");
            Some(receiver)
        }
    }

    let mut workspace = markdown_mermaid_popup_workspace();
    let FloatingImageSource::Mermaid { source, .. } = &mut workspace.floats[0].images[0].source;
    *source = "flowchart TD\n  A-->B\n  subgraph 配送・通知システム\n    C-->D\n  end".to_string();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    )
    .with_mermaid_renderer_for_tests(Box::new(SubgraphLabelFailure));
    let mut writer = RecordingOverlayWriter::default();

    let outcome = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace),
            &capabilities_with_kitty_graphics(),
            &[],
            Some(&mut writer),
        )
        .expect("subgraph label parse failure should render diagnostics");

    let popup_lines = &outcome.rendered_workspace.floats[0].lines;
    assert!(
        popup_lines.iter().any(|line| line.contains("subgraph id[")),
        "popup should suggest bracketed subgraph labels: {popup_lines:?}"
    );
    assert!(writer.writes.is_empty());
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
fn syntax_chunks_force_color_text_mode_even_when_terminal_profile_is_monochrome() {
    let monochrome_capabilities = TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: true,
            color_text: false,
            truecolor: false,
        },
        InlineGraphicsProbeResult::Unsupported,
    )
    .detect();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );
    let mut workspace = workspace(1, 101, "let value = 1;", "message");
    workspace.panes[0].syntax_chunks = vec![ScreenSyntaxChunk {
        row: 0,
        start_col: 0,
        end_col_exclusive: 3,
        syn_id: 1,
        name: Some("rustKeyword".to_string()),
        language: None,
        tree_sitter: None,
    }];

    let outcome = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace),
            &monochrome_capabilities,
            &[],
            None,
        )
        .expect("syntax-highlighted workspace should render");

    assert_eq!(
        outcome.text_mode,
        RenderTextMode::StyledTrueColor,
        "syntax on should keep colored highlighting even when NO_COLOR made the terminal profile monochrome"
    );
}

#[test]
fn empty_syntax_chunks_keep_monochrome_text_mode_for_syntax_off_fast_path() {
    let monochrome_capabilities = TerminalCapabilityProbe::new(
        TerminalCapabilityObservation {
            session_kind: TerminalSessionKind::Local,
            basic_terminal_control: true,
            styled_text: true,
            color_text: false,
            truecolor: false,
        },
        InlineGraphicsProbeResult::Unsupported,
    )
    .detect();
    let mut coordinator = TuiRenderCoordinator::new_for_tests(
        OverlayAssetStore::default(),
        OptionalGraphicsAdapter::default(),
    );

    let outcome = coordinator
        .render_workspace_result::<WorkspaceProjectionError>(
            Ok(workspace(1, 101, "let value = 1;", "message")),
            &monochrome_capabilities,
            &[],
            None,
        )
        .expect("plain workspace should render");

    assert_eq!(
        outcome.text_mode,
        RenderTextMode::StyledMonochrome,
        "syntax off should not opt into color rendering or syntax styling work"
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
    let coordinator_source = fs::read_to_string("src/presentation/render/coordinator.rs")
        .expect("render coordinator source should be readable");
    let renderer_source = fs::read_to_string("src/presentation/render/renderer.rs")
        .expect("renderer source should be readable");

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
fn optional_graphics_overlay_is_rendered_after_text_frame_draw() {
    let coordinator_source = fs::read_to_string("src/presentation/render/coordinator.rs")
        .expect("render coordinator source should be readable");
    let text_draw = coordinator_source
        .find("draw_with_mode_and_options(&rendered_workspace")
        .expect("coordinator should draw the text frame");
    let overlay_draw = coordinator_source
        .find(".render_overlay(&graphics_request")
        .expect("coordinator should render optional graphics overlays");

    assert!(
        text_draw < overlay_draw,
        "kitty overlays must be emitted after text draw so the TUI frame does not overwrite the image"
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
