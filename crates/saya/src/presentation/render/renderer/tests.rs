use super::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::app::bootstrap::prepare_launch;
use crate::app::cli::{ConfigSource, InputSource, LaunchRequest};
use crate::app::session::EditorSessionState;
use crate::core::notification_prompt::{
    BellIndication, InputPromptStatus, InputPromptView, MessageLineCandidate, MessageLineSource,
    PagerPromptView, PromptHintSuppressionReason, SuppressedPromptHint,
    resolve_workspace_message_line,
};
use crate::features::search::query::SearchMatchKind;
use crate::presentation::floating_window::{
    FloatingBorder, FloatingChrome, FloatingContentRef, FloatingCursor, FloatingScreenModel,
    FloatingWindowId,
};
use crate::presentation::markdown::structure::MarkdownDocumentMap;
use crate::presentation::screen_model::{
    ProjectionInput, ScreenLineProjection, ScreenSearchOverlay, project,
};
use crate::presentation::screen_model::{
    ScreenMarkdownStyleRange, ScreenSelection, ScreenSyntaxChunk,
};
use crate::presentation::theme::{ThemeRegistry, ThemeTextStyleDeclaration};
use crate::runtime::config::{StartupRegistry, StartupRegistryEntry};
use crate::support::session_guard::test_lock as session_test_lock;
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::{Position, Rect};
use ratatui::{TerminalOptions, Viewport};
use vim_core_rs::{CoreInputRequestKind, CorePagerPromptKind};

#[derive(Clone, Default)]
struct CaptureWriter(Rc<RefCell<Vec<u8>>>);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl CaptureWriter {
    fn bytes(&self) -> Vec<u8> {
        self.0.borrow().clone()
    }
}

fn unique_renderer_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-renderer-{name}-{nanos}"))
}

fn screen_model_with_message(message_line: Option<&str>) -> ScreenModel {
    ScreenModel {
        window_id: 1,
        buffer_id: 1,
        rect: PaneRect {
            x: 0,
            y: 0,
            width: 40,
            height: 3,
        },
        file_name: "test.txt".to_string(),
        mode_label: "NORMAL".to_string(),
        status_line: "test.txt | NORMAL | [+]!".to_string(),
        cursor_style: ScreenCursorStyle::Block,
        dirty: true,
        lines: vec!["hello".to_string()],
        line_projections: vec![],
        cursor_row: 0,
        cursor_col: 0,
        visual_selection: Some(ScreenSelection {
            start_row: 0,
            start_col: 0,
            line_start_col: 0,
            end_row: 0,
            end_col_exclusive: 1,
        }),
        search_overlays: vec![],
        syntax_chunks: vec![],
        markdown_style_ranges: vec![],
        filer_style_ranges: vec![],
        resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
        message_line: message_line.map(ToString::to_string),
        command_cursor_col: None,
        is_active: true,
    }
}

fn workspace_with_typed_message(message_line: Option<&str>) -> WorkspaceScreenModel {
    WorkspaceScreenModel {
        panes: vec![screen_model_with_message(None)],
        floats: vec![],
        active_window_id: 1,
        message_line: message_line.map_or_else(
            || resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
            |message| {
                resolve_workspace_message_line(vec![MessageLineCandidate::legacy(
                    MessageLineSource::CoreNotification,
                    message,
                )])
            },
        ),
        prompt_line: None,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: None,
        command_line: None,
        message_area_height: 5,
        message_scroll_offset: 0,
    }
}

fn projection(raw_text: &str, display_text: &str, line_start_col: u16) -> ScreenLineProjection {
    ScreenLineProjection {
        absolute_row: 0,
        raw_text: raw_text.to_string(),
        display_text: display_text.to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col,
    }
}

fn rendered_text_line(text: &Text<'_>, index: usize) -> String {
    text.lines[index]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn theme_from_entries(
    entries: Vec<StartupRegistryEntry>,
) -> crate::presentation::theme::ResolvedTheme {
    ThemeRegistry::from_startup_registry(&StartupRegistry::from_entries(entries)).resolve()
}

fn color_env_test_lock() -> &'static Mutex<()> {
    session_test_lock()
}

struct NoColorGuard {
    previous: Option<std::ffi::OsString>,
}

impl NoColorGuard {
    fn set() -> Self {
        let previous = std::env::var_os("NO_COLOR");
        unsafe {
            std::env::set_var("NO_COLOR", "1");
        }
        style::force_color_output(false);
        Self { previous }
    }
}

impl Drop for NoColorGuard {
    fn drop(&mut self) {
        match self.previous.as_ref() {
            Some(value) => unsafe {
                std::env::set_var("NO_COLOR", value);
            },
            None => unsafe {
                std::env::remove_var("NO_COLOR");
            },
        }
        style::force_color_output(
            std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty()),
        );
    }
}

#[test]
fn status_line_does_not_embed_message_line() {
    let model = screen_model_with_message(Some("保存しました"));

    assert_eq!(render_status_line(&model), "test.txt | NORMAL | [+]!");
}

#[test]
fn message_line_uses_transient_message_area() {
    let model = screen_model_with_message(Some("保存しました"));

    assert_eq!(render_message_line(&model), "保存しました");
}

#[test]
fn message_line_is_empty_when_no_message_exists() {
    let model = screen_model_with_message(None);

    assert_eq!(render_message_line(&model), "");
}

#[test]
fn message_line_with_whitespace_is_not_rendered() {
    let model = screen_model_with_message(Some("   "));

    assert_eq!(render_message_line(&model), "");
}

#[test]
fn message_area_uses_default_five_rows_for_multiline_messages() {
    let model = workspace_with_typed_message(Some("one\ntwo\nthree\nfour\nfive\nsix"));

    let layout = compute_workspace_layout(
        Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 8,
        },
        &model,
    );

    assert_eq!(
        layout.message_rect,
        Some(Rect {
            x: 0,
            y: 3,
            width: 20,
            height: 5,
        })
    );
    assert_eq!(layout.panes[0].rect.height, 3);
}

#[test]
fn message_area_height_can_be_configured_for_workspace_layout() {
    let mut model = workspace_with_typed_message(Some("one\ntwo\nthree"));
    model.message_area_height = 2;
    model.panes[0].rect.height = 6;

    let layout = compute_workspace_layout(
        Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 6,
        },
        &model,
    );

    assert_eq!(
        layout.message_rect,
        Some(Rect {
            x: 0,
            y: 4,
            width: 20,
            height: 2,
        })
    );
    assert_eq!(layout.panes[0].rect.height, 4);
}

#[test]
fn message_area_scroll_offset_selects_visible_tail() {
    let mut model = workspace_with_typed_message(Some("one\ntwo\nthree\nfour"));
    model.message_area_height = 2;
    model.message_scroll_offset = 1;

    let message = message_area_text(&model, 20).expect("message area should render");
    let rendered = message
        .lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(rendered, vec!["two", "three"]);
}

#[test]
fn message_area_wraps_single_long_message_to_workspace_width() {
    let model =
        workspace_with_typed_message(Some("unsupported startup option: saya.options.lineNumbers"));

    let message = message_area_text(&model, 20).expect("message area should render");
    let rendered = message
        .lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        rendered,
        vec![
            "unsupported startup ",
            "option: saya.options",
            ".lineNumbers"
        ]
    );
}

#[test]
fn system_warning_message_uses_warning_msg_style_by_default() {
    let mut model = workspace_with_typed_message(None);
    model.message_line = resolve_workspace_message_line(vec![MessageLineCandidate::legacy(
        MessageLineSource::SystemWarning,
        "warning",
    )]);

    let style = message_area_style(
        &model,
        &ResolvedTheme::default(),
        RenderTextMode::StyledTrueColor,
    );

    assert_eq!(style, Style::default().fg(Color::Yellow));
}

#[test]
fn search_overlay_precedence_prefers_current_over_incremental_and_regular() {
    let model = ScreenModel {
        window_id: 1,
        buffer_id: 1,
        rect: PaneRect {
            x: 0,
            y: 0,
            width: 6,
            height: 3,
        },
        file_name: "test.txt".to_string(),
        mode_label: "NORMAL".to_string(),
        status_line: "test.txt | NORMAL".to_string(),
        cursor_style: ScreenCursorStyle::Block,
        dirty: false,
        lines: vec!["abcdef".to_string()],
        line_projections: vec![],
        cursor_row: 0,
        cursor_col: 0,
        visual_selection: None,
        search_overlays: vec![
            ScreenSearchOverlay {
                row: 0,
                start_col: 0,
                end_col_exclusive: 6,
                kind: SearchMatchKind::Regular,
            },
            ScreenSearchOverlay {
                row: 0,
                start_col: 1,
                end_col_exclusive: 5,
                kind: SearchMatchKind::Incremental,
            },
            ScreenSearchOverlay {
                row: 0,
                start_col: 2,
                end_col_exclusive: 4,
                kind: SearchMatchKind::Current,
            },
        ],
        syntax_chunks: vec![],
        markdown_style_ranges: vec![],
        filer_style_ranges: vec![],
        resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
        message_line: None,
        command_cursor_col: None,
        is_active: true,
    };

    let text = render_buffer_text(&model, 6, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans.len(), 5);
    assert_eq!(line.spans[0].content.as_ref(), "a");
    assert_eq!(line.spans[1].content.as_ref(), "b");
    assert_eq!(line.spans[2].content.as_ref(), "cd");
    assert_eq!(line.spans[3].content.as_ref(), "e");
    assert_eq!(line.spans[4].content.as_ref(), "f");
    assert_eq!(
        line.spans[0].style,
        Style::default().fg(Color::Black).bg(Color::Yellow)
    );
    assert_eq!(
        line.spans[1].style,
        Style::default().fg(Color::White).bg(Color::Blue)
    );
    assert_eq!(
        line.spans[2].style,
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    );
    assert_eq!(
        line.spans[3].style,
        Style::default().fg(Color::White).bg(Color::Blue)
    );
    assert_eq!(
        line.spans[4].style,
        Style::default().fg(Color::Black).bg(Color::Yellow)
    );
}

#[test]
fn render_buffer_text_uses_line_projection_display_text_when_present() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["# Heading".to_string()];
    model.is_active = false;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "# Heading".to_string(),
        display_text: "Heading".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 0,
    }];

    let text = render_buffer_text(&model, 10, RenderTextMode::Plain);
    let rendered = text.lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(rendered, "Heading   ");
}

#[test]
fn render_buffer_text_applies_overlays_to_projected_display_text() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["# Heading".to_string()];
    model.is_active = false;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "# Heading".to_string(),
        display_text: "Heading".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 0,
    }];
    model.visual_selection = None;
    model.search_overlays = vec![ScreenSearchOverlay {
        row: 0,
        start_col: 0,
        end_col_exclusive: 7,
        kind: SearchMatchKind::Regular,
    }];

    let text = render_buffer_text(&model, 10, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "Heading");
    assert_eq!(
        line.spans[0].style,
        Style::default().fg(Color::Black).bg(Color::Yellow)
    );
    assert_eq!(line.spans[1].content.as_ref(), "   ");
}

#[test]
fn render_buffer_text_applies_resolved_markdown_style_ranges() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["## Heading".to_string()];
    model.is_active = false;
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "## Heading".to_string(),
        display_text: "Heading".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 0,
    }];
    model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
        row: 0,
        start_col: 0,
        end_col_exclusive: 7,
        style: ResolvedTextStyle {
            fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
            underline: true,
            bold: true,
            ..ResolvedTextStyle::default()
        },
    }];

    let text = render_buffer_text(&model, 10, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "Heading");
    assert_eq!(
        line.spans[0].style,
        Style::default()
            .fg(Color::Rgb(0x9e, 0xce, 0x6a))
            .add_modifier(Modifier::UNDERLINED)
            .add_modifier(Modifier::BOLD)
    );
}

#[test]
fn render_buffer_text_applies_ui_text_fg_without_painting_base_bg() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["let value = 1;".to_string()];
    model.visual_selection = None;
    model.resolved_theme = theme_from_entries(vec![
        StartupRegistryEntry::ThemePalette {
            name: "fg".to_string(),
            value: "#c0caf5".to_string(),
        },
        StartupRegistryEntry::ThemePalette {
            name: "bg".to_string(),
            value: "#24283b".to_string(),
        },
        StartupRegistryEntry::ThemeUiStyle {
            key: UiStyleKey::Text,
            style: ThemeTextStyleDeclaration {
                fg: Some("fg".to_string()),
                bg: Some("bg".to_string()),
                ..ThemeTextStyleDeclaration::default()
            },
        },
    ]);

    let text = render_buffer_text(&model, 16, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "let value = 1;");
    assert_eq!(
        line.spans[0].style,
        Style::default().fg(Color::Rgb(0xc0, 0xca, 0xf5))
    );
    assert_eq!(line.spans[1].content.as_ref(), "  ");
    assert_eq!(
        line.spans[1].style,
        Style::default(),
        "line padding should keep the terminal background instead of inheriting ui.text.bg"
    );
}

#[test]
fn render_buffer_text_applies_ui_gutter_style_to_line_number_prefix() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["   1 let value = 1;".to_string()];
    model.visual_selection = None;
    model.resolved_theme = theme_from_entries(vec![
        StartupRegistryEntry::ThemePalette {
            name: "gutter".to_string(),
            value: "#3b4261".to_string(),
        },
        StartupRegistryEntry::ThemePalette {
            name: "fg".to_string(),
            value: "#c0caf5".to_string(),
        },
        StartupRegistryEntry::ThemeUiStyle {
            key: UiStyleKey::Gutter,
            style: ThemeTextStyleDeclaration {
                fg: Some("gutter".to_string()),
                ..ThemeTextStyleDeclaration::default()
            },
        },
        StartupRegistryEntry::ThemeUiStyle {
            key: UiStyleKey::Text,
            style: ThemeTextStyleDeclaration {
                fg: Some("fg".to_string()),
                ..ThemeTextStyleDeclaration::default()
            },
        },
    ]);

    let text = render_buffer_text(&model, 20, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "   1 ");
    assert_eq!(
        line.spans[0].style,
        Style::default().fg(Color::Rgb(0x3b, 0x42, 0x61))
    );
    assert_eq!(line.spans[1].content.as_ref(), "let value = 1;");
    assert_eq!(
        line.spans[1].style,
        Style::default().fg(Color::Rgb(0xc0, 0xca, 0xf5))
    );
}

#[test]
fn render_buffer_text_keeps_markdown_projection_with_line_numbers_and_ui_theme() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["   1 ## Heading".to_string()];
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "## Heading".to_string(),
        display_text: "Heading".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 5,
    }];
    model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
        row: 0,
        start_col: 5,
        end_col_exclusive: 12,
        style: ResolvedTextStyle {
            fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
            bold: true,
            ..ResolvedTextStyle::default()
        },
    }];
    model.resolved_theme = theme_from_entries(vec![
        StartupRegistryEntry::ThemePalette {
            name: "fg".to_string(),
            value: "#c0caf5".to_string(),
        },
        StartupRegistryEntry::ThemePalette {
            name: "bg".to_string(),
            value: "#24283b".to_string(),
        },
        StartupRegistryEntry::ThemePalette {
            name: "gutter".to_string(),
            value: "#3b4261".to_string(),
        },
        StartupRegistryEntry::ThemeUiStyle {
            key: UiStyleKey::Text,
            style: ThemeTextStyleDeclaration {
                fg: Some("fg".to_string()),
                bg: Some("bg".to_string()),
                ..ThemeTextStyleDeclaration::default()
            },
        },
        StartupRegistryEntry::ThemeUiStyle {
            key: UiStyleKey::Gutter,
            style: ThemeTextStyleDeclaration {
                fg: Some("gutter".to_string()),
                bg: Some("bg".to_string()),
                ..ThemeTextStyleDeclaration::default()
            },
        },
    ]);

    let text = render_buffer_text(&model, 16, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(rendered_text_line(&text, 0), "   1 Heading    ");
    assert_eq!(line.spans[0].content.as_ref(), "   1 ");
    assert_eq!(line.spans[1].content.as_ref(), "Heading");
    assert_eq!(
        line.spans[1].style,
        Style::default()
            .fg(Color::Rgb(0x9e, 0xce, 0x6a))
            .add_modifier(Modifier::BOLD)
    );
}

#[test]
fn render_buffer_text_applies_theme_syntax_style() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["// comment".to_string()];
    model.visual_selection = None;
    model.syntax_chunks = vec![ScreenSyntaxChunk {
        row: 0,
        start_col: 0,
        end_col_exclusive: 10,
        syn_id: 1,
        name: Some("Comment".to_string()),
        language: None,
        tree_sitter: None,
    }];
    model.resolved_theme = theme_from_entries(vec![
        StartupRegistryEntry::ThemePalette {
            name: "comment".to_string(),
            value: "#565f89".to_string(),
        },
        StartupRegistryEntry::ThemeSyntaxStyle {
            key: SyntaxSemanticStyleKey::Comment,
            style: ThemeTextStyleDeclaration {
                fg: Some("comment".to_string()),
                italic: Some(true),
                ..ThemeTextStyleDeclaration::default()
            },
        },
    ]);

    let text = render_buffer_text(&model, 12, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "// comment");
    assert_eq!(
        line.spans[0].style,
        Style::default()
            .fg(Color::Rgb(0x56, 0x5f, 0x89))
            .add_modifier(Modifier::ITALIC)
    );
}

#[test]
fn render_buffer_text_applies_language_specific_syntax_over_markdown_fence_style() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["func main() {}".to_string()];
    model.visual_selection = None;
    model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
        row: 0,
        start_col: 0,
        end_col_exclusive: 14,
        style: ResolvedTextStyle {
            bg: Some(ResolvedThemeColor("#111827".to_string())),
            ..ResolvedTextStyle::default()
        },
    }];
    model.syntax_chunks = vec![ScreenSyntaxChunk {
        row: 0,
        start_col: 0,
        end_col_exclusive: 4,
        syn_id: 1,
        name: Some("Function".to_string()),
        language: Some("go".to_string()),
        tree_sitter: None,
    }];
    model.resolved_theme = theme_from_entries(vec![
        StartupRegistryEntry::ThemeSyntaxStyle {
            key: SyntaxSemanticStyleKey::Function,
            style: ThemeTextStyleDeclaration {
                fg: Some("#bb9af7".to_string()),
                ..ThemeTextStyleDeclaration::default()
            },
        },
        StartupRegistryEntry::ThemeLanguageSyntaxStyle {
            language: "go".to_string(),
            key: SyntaxSemanticStyleKey::Function,
            style: ThemeTextStyleDeclaration {
                fg: Some("#7aa2f7".to_string()),
                bold: Some(true),
                ..ThemeTextStyleDeclaration::default()
            },
        },
    ]);

    let text = render_buffer_text(&model, 16, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "func");
    assert_eq!(
        line.spans[0].style,
        Style::default()
            .fg(Color::Rgb(0x7a, 0xa2, 0xf7))
            .bg(Color::Rgb(0x11, 0x18, 0x27))
            .add_modifier(Modifier::BOLD),
        "language-specific syntax should win over the fenced code Markdown presentation"
    );
    assert_eq!(line.spans[1].content.as_ref(), " main() {}");
    assert_eq!(
        line.spans[1].style,
        Style::default().bg(Color::Rgb(0x11, 0x18, 0x27)),
        "non-token cells should keep the fenced code Markdown presentation"
    );
}

#[test]
fn render_buffer_text_applies_filer_entry_kind_and_marked_styles() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["src/".to_string()];
    model.visual_selection = None;
    model.filer_style_ranges = vec![
        crate::presentation::screen_model::ScreenFilerStyleRange {
            row: 0,
            start_col: 0,
            end_col_exclusive: 4,
            key: crate::presentation::theme::FilerSemanticStyleKey::Directory,
            style: ResolvedTextStyle {
                fg: Some(ResolvedThemeColor("#7aa2f7".to_string())),
                bold: true,
                ..ResolvedTextStyle::default()
            },
        },
        crate::presentation::screen_model::ScreenFilerStyleRange {
            row: 0,
            start_col: 0,
            end_col_exclusive: 4,
            key: crate::presentation::theme::FilerSemanticStyleKey::Marked,
            style: ResolvedTextStyle {
                bg: Some(ResolvedThemeColor("#33467c".to_string())),
                ..ResolvedTextStyle::default()
            },
        },
    ];

    let text = render_buffer_text(&model, 8, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "src/");
    assert_eq!(
        line.spans[0].style,
        Style::default()
            .fg(Color::Rgb(0x7a, 0xa2, 0xf7))
            .bg(Color::Rgb(0x33, 0x46, 0x7c))
            .add_modifier(Modifier::BOLD),
        "marked overlay is a filer presentation layer and should not depend on syntax names"
    );
}

#[test]
fn render_buffer_text_maps_vim_syntax_groups_to_type_and_function_theme_styles() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["const std::PathBuf macro".to_string()];
    model.visual_selection = None;
    model.syntax_chunks = vec![
        ScreenSyntaxChunk {
            row: 0,
            start_col: 0,
            end_col_exclusive: 5,
            syn_id: 1,
            name: Some("rustStorage".to_string()),
            language: None,
            tree_sitter: None,
        },
        ScreenSyntaxChunk {
            row: 0,
            start_col: 6,
            end_col_exclusive: 9,
            syn_id: 2,
            name: Some("rustModPath".to_string()),
            language: None,
            tree_sitter: None,
        },
        ScreenSyntaxChunk {
            row: 0,
            start_col: 9,
            end_col_exclusive: 11,
            syn_id: 3,
            name: Some("rustModPathSep".to_string()),
            language: None,
            tree_sitter: None,
        },
        ScreenSyntaxChunk {
            row: 0,
            start_col: 11,
            end_col_exclusive: 18,
            syn_id: 4,
            name: Some("rustType".to_string()),
            language: None,
            tree_sitter: None,
        },
        ScreenSyntaxChunk {
            row: 0,
            start_col: 19,
            end_col_exclusive: 24,
            syn_id: 5,
            name: Some("rustMacro".to_string()),
            language: None,
            tree_sitter: None,
        },
    ];
    model.resolved_theme = theme_from_entries(vec![
        StartupRegistryEntry::ThemePalette {
            name: "statement".to_string(),
            value: "#bb9af7".to_string(),
        },
        StartupRegistryEntry::ThemePalette {
            name: "function".to_string(),
            value: "#7aa2f7".to_string(),
        },
        StartupRegistryEntry::ThemePalette {
            name: "type".to_string(),
            value: "#2ac3de".to_string(),
        },
        StartupRegistryEntry::ThemePalette {
            name: "punctuation".to_string(),
            value: "#737aa2".to_string(),
        },
        StartupRegistryEntry::ThemeSyntaxStyle {
            key: SyntaxSemanticStyleKey::Statement,
            style: ThemeTextStyleDeclaration {
                fg: Some("statement".to_string()),
                ..ThemeTextStyleDeclaration::default()
            },
        },
        StartupRegistryEntry::ThemeSyntaxStyle {
            key: SyntaxSemanticStyleKey::Function,
            style: ThemeTextStyleDeclaration {
                fg: Some("function".to_string()),
                ..ThemeTextStyleDeclaration::default()
            },
        },
        StartupRegistryEntry::ThemeSyntaxStyle {
            key: SyntaxSemanticStyleKey::Type,
            style: ThemeTextStyleDeclaration {
                fg: Some("type".to_string()),
                ..ThemeTextStyleDeclaration::default()
            },
        },
        StartupRegistryEntry::ThemeSyntaxStyle {
            key: SyntaxSemanticStyleKey::Punctuation,
            style: ThemeTextStyleDeclaration {
                fg: Some("punctuation".to_string()),
                ..ThemeTextStyleDeclaration::default()
            },
        },
    ]);

    let text = render_buffer_text(&model, 24, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(rendered_text_line(&text, 0), "const std::PathBuf macro");
    assert_eq!(line.spans[0].content.as_ref(), "const");
    assert_eq!(
        line.spans[0].style,
        Style::default().fg(Color::Rgb(0xbb, 0x9a, 0xf7))
    );
    assert_eq!(line.spans[2].content.as_ref(), "std");
    assert_eq!(
        line.spans[2].style,
        Style::default().fg(Color::Rgb(0x2a, 0xc3, 0xde))
    );
    assert_eq!(line.spans[3].content.as_ref(), "::");
    assert_eq!(
        line.spans[3].style,
        Style::default().fg(Color::Rgb(0x73, 0x7a, 0xa2))
    );
    assert_eq!(line.spans[4].content.as_ref(), "PathBuf");
    assert_eq!(
        line.spans[4].style,
        Style::default().fg(Color::Rgb(0x2a, 0xc3, 0xde))
    );
    assert_eq!(line.spans[6].content.as_ref(), "macro");
    assert_eq!(
        line.spans[6].style,
        Style::default().fg(Color::Rgb(0x7a, 0xa2, 0xf7))
    );
}

#[test]
fn markdown_style_ranges_override_syntax_highlight_on_same_cells() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["## Heading".to_string()];
    model.is_active = false;
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "## Heading".to_string(),
        display_text: "Heading".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 0,
    }];
    model.syntax_chunks = vec![ScreenSyntaxChunk {
        row: 0,
        start_col: 0,
        end_col_exclusive: 7,
        syn_id: 9,
        name: Some("Title".to_string()),
        language: None,
        tree_sitter: None,
    }];
    model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
        row: 0,
        start_col: 0,
        end_col_exclusive: 7,
        style: ResolvedTextStyle {
            fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
            underline: true,
            ..ResolvedTextStyle::default()
        },
    }];

    let text = render_buffer_text(&model, 10, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "Heading");
    assert_eq!(
        line.spans[0].style,
        Style::default()
            .fg(Color::Rgb(0x9e, 0xce, 0x6a))
            .add_modifier(Modifier::UNDERLINED),
        "Markdown semantic theme should win over core syntax style on projected Markdown cells"
    );
}

#[test]
fn markdown_style_ranges_override_markdown_tree_sitter_syntax_on_same_cells() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["## Heading".to_string()];
    model.is_active = false;
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "## Heading".to_string(),
        display_text: "Heading".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 0,
    }];
    model.syntax_chunks = vec![ScreenSyntaxChunk {
        row: 0,
        start_col: 0,
        end_col_exclusive: 7,
        syn_id: 9,
        name: None,
        language: Some("markdown".to_string()),
        tree_sitter: Some(ScreenTreeSitterSyntax {
            category: ScreenSyntaxCategory::Markup,
            modifiers: Vec::new(),
            capture_name: "markup.heading".to_string(),
        }),
    }];
    model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
        row: 0,
        start_col: 0,
        end_col_exclusive: 7,
        style: ResolvedTextStyle {
            fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
            underline: true,
            ..ResolvedTextStyle::default()
        },
    }];

    let text = render_buffer_text(&model, 10, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "Heading");
    assert_eq!(
        line.spans[0].style,
        Style::default()
            .fg(Color::Rgb(0x9e, 0xce, 0x6a))
            .add_modifier(Modifier::UNDERLINED),
        "Markdown semantic heading levels should win over Markdown parser syntax"
    );
}

#[test]
fn markdown_semantic_ranges_override_base_markdown_tree_sitter_across_styles() {
    let mut model = screen_model_with_message(None);
    model.lines = vec![
        "Heading".to_string(),
        "code".to_string(),
        "link".to_string(),
    ];
    model.is_active = false;
    model.visual_selection = None;
    model.line_projections = vec![
        ScreenLineProjection {
            absolute_row: 0,
            raw_text: "# Heading".to_string(),
            display_text: "Heading".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        },
        ScreenLineProjection {
            absolute_row: 1,
            raw_text: "`code`".to_string(),
            display_text: "code".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        },
        ScreenLineProjection {
            absolute_row: 2,
            raw_text: "[link](target)".to_string(),
            display_text: "link".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        },
    ];
    model.syntax_chunks = vec![
        ScreenSyntaxChunk {
            row: 0,
            start_col: 0,
            end_col_exclusive: 7,
            syn_id: 1,
            name: None,
            language: Some("markdown".to_string()),
            tree_sitter: Some(ScreenTreeSitterSyntax {
                category: ScreenSyntaxCategory::Markup,
                modifiers: Vec::new(),
                capture_name: "markup.heading".to_string(),
            }),
        },
        ScreenSyntaxChunk {
            row: 1,
            start_col: 0,
            end_col_exclusive: 4,
            syn_id: 2,
            name: None,
            language: Some("markdown".to_string()),
            tree_sitter: Some(ScreenTreeSitterSyntax {
                category: ScreenSyntaxCategory::String,
                modifiers: Vec::new(),
                capture_name: "markup.raw.inline".to_string(),
            }),
        },
        ScreenSyntaxChunk {
            row: 2,
            start_col: 0,
            end_col_exclusive: 4,
            syn_id: 3,
            name: None,
            language: Some("markdown".to_string()),
            tree_sitter: Some(ScreenTreeSitterSyntax {
                category: ScreenSyntaxCategory::Tag,
                modifiers: Vec::new(),
                capture_name: "markup.link.label".to_string(),
            }),
        },
    ];
    model.markdown_style_ranges = vec![
        ScreenMarkdownStyleRange {
            row: 0,
            start_col: 0,
            end_col_exclusive: 7,
            style: ResolvedTextStyle {
                fg: Some(ResolvedThemeColor("#bb9af7".to_string())),
                bold: true,
                ..ResolvedTextStyle::default()
            },
        },
        ScreenMarkdownStyleRange {
            row: 1,
            start_col: 0,
            end_col_exclusive: 4,
            style: ResolvedTextStyle {
                fg: Some(ResolvedThemeColor("#ff9e64".to_string())),
                bg: Some(ResolvedThemeColor("#292e42".to_string())),
                ..ResolvedTextStyle::default()
            },
        },
        ScreenMarkdownStyleRange {
            row: 2,
            start_col: 0,
            end_col_exclusive: 4,
            style: ResolvedTextStyle {
                fg: Some(ResolvedThemeColor("#2ac3de".to_string())),
                underline: true,
                ..ResolvedTextStyle::default()
            },
        },
    ];

    let text = render_buffer_text(&model, 10, RenderTextMode::StyledTrueColor);

    assert_eq!(
        text.lines[0].spans[0].style,
        Style::default()
            .fg(Color::Rgb(0xbb, 0x9a, 0xf7))
            .add_modifier(Modifier::BOLD)
    );
    assert_eq!(
        text.lines[1].spans[0].style,
        Style::default()
            .fg(Color::Rgb(0xff, 0x9e, 0x64))
            .bg(Color::Rgb(0x29, 0x2e, 0x42))
    );
    assert_eq!(
        text.lines[2].spans[0].style,
        Style::default()
            .fg(Color::Rgb(0x2a, 0xc3, 0xde))
            .add_modifier(Modifier::UNDERLINED)
    );
}

#[test]
fn plain_text_mode_preserves_markdown_text_while_removing_style() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["`code`".to_string()];
    model.is_active = false;
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "`code`".to_string(),
        display_text: "code".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 0,
    }];
    model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
        row: 0,
        start_col: 0,
        end_col_exclusive: 4,
        style: ResolvedTextStyle {
            fg: Some(ResolvedThemeColor("#ff9e64".to_string())),
            ..ResolvedTextStyle::default()
        },
    }];

    let text = render_buffer_text(&model, 6, RenderTextMode::Plain);

    assert_eq!(rendered_text_line(&text, 0), "code  ");
    assert!(
        text.lines[0]
            .spans
            .iter()
            .all(|span| span.style == Style::default())
    );
}

#[test]
fn monochrome_text_mode_preserves_markdown_modifiers_while_removing_colors() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["## Heading".to_string()];
    model.is_active = false;
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "## Heading".to_string(),
        display_text: "Heading".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 0,
    }];
    model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
        row: 0,
        start_col: 0,
        end_col_exclusive: 7,
        style: ResolvedTextStyle {
            fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
            bold: true,
            underline: true,
            ..ResolvedTextStyle::default()
        },
    }];

    let text = render_buffer_text(&model, 10, RenderTextMode::StyledMonochrome);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "Heading");
    assert_eq!(
        line.spans[0].style,
        Style::default()
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::UNDERLINED),
        "NO_COLOR mode should remove theme colors without dropping text modifiers"
    );
}

#[test]
fn crossterm_backend_emits_bold_sgr_for_markdown_heading_theme() {
    let writer = CaptureWriter::default();
    let backend = CrosstermBackend::new(writer.clone());
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 24, 4)),
        },
    )
    .expect("crossterm test terminal should initialize");
    let mut model = screen_model_with_message(None);
    model.lines = vec!["## プロジェクト概要".to_string()];
    model.is_active = false;
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "## プロジェクト概要".to_string(),
        display_text: "プロジェクト概要".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 0,
    }];
    model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
        row: 0,
        start_col: 0,
        end_col_exclusive: 16,
        style: ResolvedTextStyle {
            fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
            bold: true,
            underline: true,
            ..ResolvedTextStyle::default()
        },
    }];

    draw_editor_frame(&mut terminal, &model, true).expect("markdown heading should render");
    let bytes = writer.bytes();
    let output = String::from_utf8_lossy(&bytes);

    assert!(
        output.contains("\u{1b}[1m"),
        "Crossterm output should include SGR 1 for bold: {output:?}"
    );
    assert!(
        output.contains("\u{1b}[4m"),
        "Crossterm output should include SGR 4 for underline: {output:?}"
    );
}

#[test]
fn crossterm_backend_forces_color_sgr_for_syntax_highlight_even_when_no_color_is_set() {
    let _lock = color_env_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _no_color_guard = NoColorGuard::set();
    let writer = CaptureWriter::default();
    let backend = CrosstermBackend::new(writer.clone());
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 24, 4)),
        },
    )
    .expect("crossterm test terminal should initialize");
    let mut model = screen_model_with_message(None);
    model.lines = vec!["let value = 1;".to_string()];
    model.is_active = true;
    model.visual_selection = None;
    model.syntax_chunks = vec![ScreenSyntaxChunk {
        row: 0,
        start_col: 0,
        end_col_exclusive: 3,
        syn_id: 1,
        name: Some("rustKeyword".to_string()),
        language: None,
        tree_sitter: None,
    }];
    model.resolved_theme = theme_from_entries(vec![
        StartupRegistryEntry::ThemePalette {
            name: "keyword".to_string(),
            value: "#bb9af7".to_string(),
        },
        StartupRegistryEntry::ThemeSyntaxStyle {
            key: SyntaxSemanticStyleKey::Statement,
            style: ThemeTextStyleDeclaration {
                fg: Some("keyword".to_string()),
                ..ThemeTextStyleDeclaration::default()
            },
        },
    ]);

    draw_workspace_frame(
        &mut terminal,
        &WorkspaceScreenModel {
            panes: vec![model.clone()],
            floats: vec![],
            active_window_id: model.window_id,
            message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
            message_area_height: 5,
            message_scroll_offset: 0,
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: None,
        },
        true,
        RenderTextMode::StyledTrueColor,
    )
    .expect("syntax highlight should render with forced color");
    let bytes = writer.bytes();
    let output = String::from_utf8_lossy(&bytes);

    assert!(
        output.contains("\u{1b}[38;2;187;154;247"),
        "syntax on should emit color SGR even when NO_COLOR is set: {output:?}"
    );
}

#[test]
fn crossterm_backend_omits_bold_sgr_when_heading_level_disables_bold() {
    let writer = CaptureWriter::default();
    let backend = CrosstermBackend::new(writer.clone());
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 24, 4)),
        },
    )
    .expect("crossterm test terminal should initialize");
    let mut model = screen_model_with_message(None);
    model.lines = vec!["## プロジェクト概要".to_string()];
    model.is_active = false;
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "## プロジェクト概要".to_string(),
        display_text: "プロジェクト概要".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 0,
    }];
    model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
        row: 0,
        start_col: 0,
        end_col_exclusive: 16,
        style: ResolvedTextStyle {
            fg: Some(ResolvedThemeColor("#9ece6a".to_string())),
            bold: false,
            underline: true,
            ..ResolvedTextStyle::default()
        },
    }];

    draw_editor_frame(&mut terminal, &model, true).expect("markdown heading should render");
    let bytes = writer.bytes();
    let output = String::from_utf8_lossy(&bytes);

    assert!(
        !output.contains("\u{1b}[1m"),
        "Crossterm output should not include SGR 1 when bold=false: {output:?}"
    );
    assert!(
        output.contains("\u{1b}[4m"),
        "Crossterm output should still include SGR 4 for underline: {output:?}"
    );
}

#[test]
fn crossterm_backend_emits_bold_sgr_for_startup_config_heading1_with_line_numbers() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_renderer_path("heading1-target.md");
    let config_path = unique_renderer_path("heading1-init.ts");
    let markdown_source = "# AGENTS.md\n\nbody\n";
    std::fs::write(&target_path, markdown_source).expect("target file");
    std::fs::write(
        &config_path,
        r##"
            saya.options.number = true;
            saya.options.syntax = true;
            saya.theme.palette = {
                accent: "#7aa2f7",
                heading2: "#9ece6a",
            };
            saya.theme.markdown = {
                heading: { fg: "accent", bold: true },
                heading2: { fg: "heading2", underline: true, bold: false },
            };
        "##,
    )
    .expect("config file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with typescript theme config");
    let markdown_map = MarkdownDocumentMap::parse(markdown_source);
    let session_state = outcome.editor_session_state();
    let model = project(
        &ProjectionInput::new(&outcome.initial_snapshot, &session_state, None)
            .with_markdown_document_map(Some(&markdown_map)),
    );
    let heading = model
        .markdown_style_ranges
        .iter()
        .find(|range| range.row == 0)
        .expect("heading1 range should be projected");

    assert_eq!(model.line_projections[0].display_text, "# AGENTS.md");
    assert_eq!(
        heading.start_col, model.line_projections[0].line_start_col,
        "active raw heading1 should style the heading after the line-number gutter"
    );
    assert!(
        heading.style.bold,
        "startup heading theme should keep heading1 bold before renderer output"
    );

    let writer = CaptureWriter::default();
    let backend = CrosstermBackend::new(writer.clone());
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 32, 4)),
        },
    )
    .expect("crossterm test terminal should initialize");

    draw_editor_frame(&mut terminal, &model, true).expect("heading1 should render");
    let bytes = writer.bytes();
    let output = String::from_utf8_lossy(&bytes);

    assert!(
        output.contains("\u{1b}[1m"),
        "startup-configured heading1 should emit SGR 1 for bold: {output:?}"
    );

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn crossterm_backend_emits_bold_sgr_for_markdown_heading_in_monochrome_mode() {
    let writer = CaptureWriter::default();
    let backend = CrosstermBackend::new(writer.clone());
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 24, 4)),
        },
    )
    .expect("crossterm test terminal should initialize");
    let mut model = screen_model_with_message(None);
    model.lines = vec!["   1 # AGENTS.md".to_string()];
    model.is_active = true;
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "# AGENTS.md".to_string(),
        display_text: "# AGENTS.md".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 5,
    }];
    model.markdown_style_ranges = vec![ScreenMarkdownStyleRange {
        row: 0,
        start_col: 5,
        end_col_exclusive: 16,
        style: ResolvedTextStyle {
            fg: Some(ResolvedThemeColor("#7aa2f7".to_string())),
            bold: true,
            ..ResolvedTextStyle::default()
        },
    }];

    draw_workspace_frame(
        &mut terminal,
        &WorkspaceScreenModel {
            panes: vec![model.clone()],
            floats: vec![],
            active_window_id: model.window_id,
            message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
            message_area_height: 5,
            message_scroll_offset: 0,
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: None,
        },
        true,
        RenderTextMode::StyledMonochrome,
    )
    .expect("monochrome heading should render");
    let bytes = writer.bytes();
    let output = String::from_utf8_lossy(&bytes);

    assert!(
        output.contains("\u{1b}[1m"),
        "monochrome mode should still emit SGR 1 for bold: {output:?}"
    );
    assert!(
        !output.contains("38;2"),
        "monochrome mode should drop theme color SGR while keeping bold: {output:?}"
    );
}

#[test]
fn render_buffer_text_keeps_line_number_gutter_with_projected_display_text() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["   1 # Heading".to_string()];
    model.is_active = false;
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "# Heading".to_string(),
        display_text: "Heading".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 5,
    }];

    let text = render_buffer_text(&model, 14, RenderTextMode::Plain);
    let rendered = text.lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(rendered, "   1 Heading  ");
}

#[test]
fn render_buffer_text_uses_raw_projection_display_text_on_active_cursor_row() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["# Heading".to_string()];
    model.is_active = true;
    model.cursor_row = 0;
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "# Heading".to_string(),
        display_text: "# Heading".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 0,
    }];

    let text = render_buffer_text(&model, 10, RenderTextMode::Plain);
    let rendered = text.lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(rendered, "# Heading ");
}

#[test]
fn render_buffer_text_uses_projection_display_text_even_for_active_cursor_row() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["# Heading".to_string()];
    model.is_active = true;
    model.cursor_row = 0;
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "# Heading".to_string(),
        display_text: "Heading".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 0,
    }];

    let text = render_buffer_text(&model, 10, RenderTextMode::Plain);
    let rendered = text.lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(rendered, "Heading   ");
}

#[test]
fn render_buffer_text_does_not_reinterpret_projection_for_active_or_inactive_rows() {
    let mut active_model = screen_model_with_message(None);
    active_model.lines = vec!["# Active".to_string()];
    active_model.is_active = true;
    active_model.cursor_row = 0;
    active_model.visual_selection = None;
    active_model.line_projections = vec![projection("# Active", "Active", 0)];

    let active_text = render_buffer_text(&active_model, 10, RenderTextMode::Plain);

    assert_eq!(rendered_text_line(&active_text, 0), "Active    ");

    let mut inactive_model = screen_model_with_message(None);
    inactive_model.lines = vec!["# Inactive".to_string()];
    inactive_model.is_active = false;
    inactive_model.cursor_row = 0;
    inactive_model.visual_selection = None;
    inactive_model.line_projections = vec![projection("# Inactive", "# Inactive", 0)];

    let inactive_text = render_buffer_text(&inactive_model, 12, RenderTextMode::Plain);

    assert_eq!(rendered_text_line(&inactive_text, 0), "# Inactive  ");
}

#[test]
fn render_buffer_text_renders_raw_block_rows_from_projection_display_text() {
    let mut model = screen_model_with_message(None);
    model.lines = vec![
        "# Title".to_string(),
        "- [x] done".to_string(),
        "tail".to_string(),
    ];
    model.is_active = true;
    model.cursor_row = 1;
    model.visual_selection = None;
    model.line_projections = vec![
        ScreenLineProjection {
            absolute_row: 0,
            raw_text: "# Title".to_string(),
            display_text: "# Title".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        },
        ScreenLineProjection {
            absolute_row: 1,
            raw_text: "- [x] done".to_string(),
            display_text: "- [x] done".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        },
        ScreenLineProjection {
            absolute_row: 2,
            raw_text: "tail".to_string(),
            display_text: "tail".to_string(),
            spans: vec![],
            cells: vec![],
            line_start_col: 0,
        },
    ];

    let text = render_buffer_text(&model, 12, RenderTextMode::Plain);
    let rendered = text
        .lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        rendered,
        vec![
            "# Title     ".to_string(),
            "- [x] done  ".to_string(),
            "tail        ".to_string(),
        ]
    );
}

#[test]
fn render_buffer_text_preserves_gutter_and_overlay_for_raw_projection_row() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["   1 # Heading".to_string()];
    model.is_active = true;
    model.cursor_row = 0;
    model.visual_selection = None;
    model.line_projections = vec![ScreenLineProjection {
        absolute_row: 0,
        raw_text: "# Heading".to_string(),
        display_text: "# Heading".to_string(),
        spans: vec![],
        cells: vec![],
        line_start_col: 5,
    }];
    model.search_overlays = vec![ScreenSearchOverlay {
        row: 0,
        start_col: 5,
        end_col_exclusive: 14,
        kind: SearchMatchKind::Regular,
    }];

    let text = render_buffer_text(&model, 16, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "   1 ");
    assert_eq!(line.spans[0].style, Style::default());
    assert_eq!(line.spans[1].content.as_ref(), "# Heading");
    assert_eq!(
        line.spans[1].style,
        Style::default().fg(Color::Black).bg(Color::Yellow)
    );
    assert_eq!(line.spans[2].content.as_ref(), "  ");
}

#[test]
fn render_buffer_text_applies_gutter_and_overlays_in_projected_display_space() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["   1 # Heading".to_string()];
    model.is_active = false;
    model.cursor_row = 0;
    model.visual_selection = Some(ScreenSelection {
        start_row: 0,
        start_col: 9,
        line_start_col: 5,
        end_row: 0,
        end_col_exclusive: 11,
    });
    model.search_overlays = vec![ScreenSearchOverlay {
        row: 0,
        start_col: 6,
        end_col_exclusive: 8,
        kind: SearchMatchKind::Regular,
    }];
    model.syntax_chunks = vec![ScreenSyntaxChunk {
        row: 0,
        start_col: 5,
        end_col_exclusive: 12,
        syn_id: 7,
        name: Some("Keyword".to_string()),
        language: None,
        tree_sitter: None,
    }];
    model.line_projections = vec![projection("# Heading", "Heading", 5)];

    let text = render_buffer_text(&model, 14, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans.len(), 7);
    assert_eq!(line.spans[0].content.as_ref(), "   1 ");
    assert_eq!(line.spans[0].style, Style::default());
    assert_eq!(line.spans[1].content.as_ref(), "H");
    assert_eq!(line.spans[1].style, Style::default().fg(Color::Cyan));
    assert_eq!(line.spans[2].content.as_ref(), "ea");
    assert_eq!(
        line.spans[2].style,
        Style::default().fg(Color::Black).bg(Color::Yellow)
    );
    assert_eq!(line.spans[3].content.as_ref(), "d");
    assert_eq!(line.spans[3].style, Style::default().fg(Color::Cyan));
    assert_eq!(line.spans[4].content.as_ref(), "in");
    assert_eq!(
        line.spans[4].style,
        Style::default().add_modifier(Modifier::REVERSED)
    );
    assert_eq!(line.spans[5].content.as_ref(), "g");
    assert_eq!(line.spans[5].style, Style::default().fg(Color::Cyan));
    assert_eq!(line.spans[6].content.as_ref(), "  ");
}

#[test]
fn render_buffer_text_falls_back_to_lines_when_line_projections_are_empty() {
    let mut model = screen_model_with_message(None);
    model.lines = vec!["# Heading".to_string()];
    model.line_projections = vec![];
    model.visual_selection = None;

    let text = render_buffer_text(&model, 10, RenderTextMode::Plain);
    let rendered = text.lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(rendered, "# Heading ");
}

#[test]
fn syntax_chunks_style_spans_without_changing_line_text() {
    let model = ScreenModel {
        window_id: 1,
        buffer_id: 1,
        rect: PaneRect {
            x: 0,
            y: 0,
            width: 12,
            height: 3,
        },
        file_name: "test.rs".to_string(),
        mode_label: "NORMAL".to_string(),
        status_line: "test.txt | NORMAL".to_string(),
        cursor_style: ScreenCursorStyle::Block,
        dirty: false,
        lines: vec!["let value".to_string()],
        line_projections: vec![],
        cursor_row: 0,
        cursor_col: 0,
        visual_selection: None,
        search_overlays: vec![],
        syntax_chunks: vec![ScreenSyntaxChunk {
            row: 0,
            start_col: 0,
            end_col_exclusive: 3,
            syn_id: 7,
            name: Some("Keyword".to_string()),
            language: None,
            tree_sitter: None,
        }],
        markdown_style_ranges: vec![],
        filer_style_ranges: vec![],
        resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
        message_line: None,
        command_cursor_col: None,
        is_active: true,
    };

    let text = render_buffer_text(&model, 12, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "let");
    assert_eq!(line.spans[0].style, Style::default().fg(Color::Cyan));
    assert_eq!(line.spans[1].content.as_ref(), " value");
    assert_eq!(
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>(),
        "let value   ",
        "syntax styling must not alter rendered line text"
    );
}

#[test]
fn tree_sitter_syntax_styles_use_category_and_modifier_not_capture_name() {
    use crate::presentation::screen_model::{
        ScreenSyntaxCategory, ScreenSyntaxModifier, ScreenTreeSitterSyntax,
    };

    let model = ScreenModel {
        window_id: 1,
        buffer_id: 1,
        rect: PaneRect {
            x: 0,
            y: 0,
            width: 12,
            height: 3,
        },
        file_name: "test.rs".to_string(),
        mode_label: "NORMAL".to_string(),
        status_line: "test.txt | NORMAL".to_string(),
        cursor_style: ScreenCursorStyle::Block,
        dirty: false,
        lines: vec!["fn value".to_string()],
        line_projections: vec![],
        cursor_row: 0,
        cursor_col: 0,
        visual_selection: None,
        search_overlays: vec![],
        syntax_chunks: vec![ScreenSyntaxChunk {
            row: 0,
            start_col: 0,
            end_col_exclusive: 2,
            syn_id: 0,
            name: Some("ignored.capture".to_string()),
            language: None,
            tree_sitter: Some(ScreenTreeSitterSyntax {
                category: ScreenSyntaxCategory::Keyword,
                modifiers: vec![ScreenSyntaxModifier::Definition],
                capture_name: "ignored.capture".to_string(),
            }),
        }],
        markdown_style_ranges: vec![],
        filer_style_ranges: vec![],
        resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
        message_line: None,
        command_cursor_col: None,
        is_active: true,
    };

    let text = render_buffer_text(&model, 12, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans[0].content.as_ref(), "fn");
    assert_eq!(
        line.spans[0].style,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        "Tree-sitter styling must use normalized category/modifier data"
    );
    assert_eq!(line.spans[1].content.as_ref(), " value");
}

#[test]
fn visual_selection_overrides_search_overlay_when_ranges_overlap() {
    let model = ScreenModel {
        window_id: 1,
        buffer_id: 1,
        rect: PaneRect {
            x: 0,
            y: 0,
            width: 6,
            height: 3,
        },
        file_name: "test.txt".to_string(),
        mode_label: "VISUAL".to_string(),
        status_line: "test.txt | VISUAL".to_string(),
        cursor_style: ScreenCursorStyle::Block,
        dirty: false,
        lines: vec!["abcdef".to_string()],
        line_projections: vec![],
        cursor_row: 0,
        cursor_col: 0,
        visual_selection: Some(ScreenSelection {
            start_row: 0,
            start_col: 2,
            line_start_col: 2,
            end_row: 0,
            end_col_exclusive: 4,
        }),
        search_overlays: vec![ScreenSearchOverlay {
            row: 0,
            start_col: 0,
            end_col_exclusive: 6,
            kind: SearchMatchKind::Regular,
        }],
        syntax_chunks: vec![],
        markdown_style_ranges: vec![],
        filer_style_ranges: vec![],
        resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
        message_line: None,
        command_cursor_col: None,
        is_active: true,
    };

    let text = render_buffer_text(&model, 6, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans.len(), 3);
    assert_eq!(line.spans[0].content.as_ref(), "ab");
    assert_eq!(line.spans[1].content.as_ref(), "cd");
    assert_eq!(line.spans[2].content.as_ref(), "ef");
    assert_eq!(
        line.spans[0].style,
        Style::default().fg(Color::Black).bg(Color::Yellow)
    );
    assert_eq!(
        line.spans[1].style,
        Style::default().add_modifier(Modifier::REVERSED)
    );
    assert_eq!(
        line.spans[2].style,
        Style::default().fg(Color::Black).bg(Color::Yellow)
    );
}

#[test]
fn search_overlay_renders_full_width_glyph_with_background_highlight() {
    let model = ScreenModel {
        window_id: 1,
        buffer_id: 1,
        rect: PaneRect {
            x: 0,
            y: 0,
            width: 6,
            height: 3,
        },
        file_name: "test.txt".to_string(),
        mode_label: "NORMAL".to_string(),
        status_line: "test.txt | NORMAL".to_string(),
        cursor_style: ScreenCursorStyle::Block,
        dirty: false,
        lines: vec!["xあx".to_string()],
        line_projections: vec![],
        cursor_row: 0,
        cursor_col: 0,
        visual_selection: None,
        search_overlays: vec![ScreenSearchOverlay {
            row: 0,
            start_col: 1,
            end_col_exclusive: 3,
            kind: SearchMatchKind::Regular,
        }],
        syntax_chunks: vec![],
        markdown_style_ranges: vec![],
        filer_style_ranges: vec![],
        resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
        message_line: None,
        command_cursor_col: None,
        is_active: true,
    };

    let text = render_buffer_text(&model, 6, RenderTextMode::StyledTrueColor);
    let line = &text.lines[0];

    assert_eq!(line.spans.len(), 4);
    assert_eq!(line.spans[0].content.as_ref(), "x");
    assert_eq!(line.spans[1].content.as_ref(), "あ");
    assert_eq!(line.spans[2].content.as_ref(), "x");
    assert!(
        line.spans[3].content.as_ref().chars().all(|ch| ch == ' '),
        "rendered line should keep trailing padding spaces"
    );
    assert_eq!(
        line.spans[1].style,
        Style::default().fg(Color::Black).bg(Color::Yellow)
    );
}

#[test]
fn multiline_selection_does_not_highlight_line_number_gutter() {
    let model = ScreenModel {
        window_id: 1,
        buffer_id: 1,
        rect: PaneRect {
            x: 0,
            y: 0,
            width: 20,
            height: 4,
        },
        file_name: "test.txt".to_string(),
        mode_label: "V-LINE".to_string(),
        status_line: "test.txt | V-LINE".to_string(),
        cursor_style: ScreenCursorStyle::Block,
        dirty: false,
        lines: vec![" 1 alpha".to_string(), " 2 beta".to_string()],
        line_projections: vec![],
        cursor_row: 1,
        cursor_col: 3,
        visual_selection: Some(ScreenSelection {
            start_row: 0,
            start_col: 3,
            line_start_col: 3,
            end_row: 1,
            end_col_exclusive: 7,
        }),
        search_overlays: vec![],
        syntax_chunks: vec![],
        markdown_style_ranges: vec![],
        filer_style_ranges: vec![],
        resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
        message_line: None,
        command_cursor_col: None,
        is_active: true,
    };

    let text = render_buffer_text(&model, 20, RenderTextMode::StyledTrueColor);
    let second_line = &text.lines[1];

    assert_eq!(second_line.spans.len(), 3);
    assert_eq!(second_line.spans[0].content.as_ref(), " 2 ");
    assert_eq!(second_line.spans[1].content.as_ref(), "beta");
    assert!(
        second_line.spans[2]
            .content
            .as_ref()
            .chars()
            .all(|ch| ch == ' '),
        "末尾はパディング空白で埋めること"
    );
}

#[test]
fn redraw_clears_stale_tail_when_line_becomes_shorter() {
    let mut terminal =
        Terminal::new(TestBackend::new(40, 4)).expect("test terminal should initialize");
    let mut long_model = screen_model_with_message(None);
    long_model.lines = vec!["## プロジェクト概要    13 seconds ago".to_string()];
    long_model.dirty = false;
    let mut short_model = screen_model_with_message(None);
    short_model.lines = vec!["## プロジェクト概要".to_string()];
    short_model.dirty = false;

    draw_editor_frame(&mut terminal, &long_model, true).expect("first draw should succeed");
    draw_editor_frame(&mut terminal, &short_model, false)
        .expect("short line redraw should succeed");

    let rendered = terminal.backend().buffer().content();
    let first_row: String = rendered.iter().take(40).map(|cell| cell.symbol()).collect();

    assert!(
        !first_row.contains("seconds ago"),
        "短い行への再描画で古い suffix が残らないこと: {:?}",
        first_row
    );
}

#[test]
fn redraw_clears_stale_tail_when_projected_display_becomes_shorter() {
    let mut terminal =
        Terminal::new(TestBackend::new(40, 4)).expect("test terminal should initialize");
    let mut long_model = screen_model_with_message(None);
    long_model.lines = vec!["# Long projected tail".to_string()];
    long_model.line_projections = vec![projection(
        "# Long projected tail",
        "Long projected tail",
        0,
    )];
    long_model.visual_selection = None;
    long_model.dirty = false;

    let mut short_model = screen_model_with_message(None);
    short_model.lines = vec!["# Short".to_string()];
    short_model.line_projections = vec![projection("# Short", "Short", 0)];
    short_model.visual_selection = None;
    short_model.dirty = false;

    draw_editor_frame(&mut terminal, &long_model, true).expect("first draw should succeed");
    draw_editor_frame(&mut terminal, &short_model, false)
        .expect("short projected redraw should succeed");

    let rendered = terminal.backend().buffer().content();
    let first_row: String = rendered.iter().take(40).map(|cell| cell.symbol()).collect();

    assert!(
        !first_row.contains("projected tail"),
        "shorter projected redraw must clear stale suffix: {:?}",
        first_row
    );
}

#[test]
fn integrated_update_cycle_keeps_message_status_and_cursor_in_sync() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut outcome = prepare_launch(LaunchRequest::default()).expect("launch should succeed");
    let mut session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.dispatch_key("i").expect("insert mode");
    outcome.core_bridge.dispatch_key("H").expect("insert text");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("leave insert mode");
    session_state.update_dirty(outcome.core_bridge.snapshot().dirty);

    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(
        &snapshot,
        &session_state,
        Some("Action failed"),
    ));

    let mut terminal =
        Terminal::new(TestBackend::new(40, 4)).expect("test terminal should initialize");
    draw_editor_frame(&mut terminal, &model, true).expect("render should succeed");

    let rendered = format!("{}", terminal.backend());
    let rows: Vec<&str> = rendered.lines().collect();

    assert!(
        rows.get(2).is_some_and(|row| row.contains(&model.file_name)
            && row.contains(&model.mode_label)
            && row.contains("[+]!")),
        "status line should reflect file name, mode, and dirty state: {:?}",
        rows.get(2)
    );
    assert!(
        rows.get(3).is_some_and(|row| row.contains("Action failed")),
        "message line should render the projected transient message: {:?}",
        rows.get(3)
    );
    assert!(
        rows.get(0).is_some_and(|row| row.contains('H')),
        "buffer area should include the edited content after the update cycle: {:?}",
        rows.get(0)
    );
    terminal
        .backend_mut()
        .assert_cursor_position(Position::new(model.cursor_col, model.cursor_row));
}

#[test]
fn workspace_render_uses_active_window_id_even_when_pane_flags_are_stale() {
    let mut terminal =
        Terminal::new(TestBackend::new(40, 8)).expect("test terminal should initialize");
    let model = WorkspaceScreenModel {
        panes: vec![
            ScreenModel {
                window_id: 10,
                buffer_id: 10,
                rect: PaneRect {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 4,
                },
                file_name: "left.txt".to_string(),
                mode_label: "NORMAL".to_string(),
                status_line: "test.txt | NORMAL".to_string(),
                cursor_style: ScreenCursorStyle::Block,
                dirty: false,
                lines: vec!["left".to_string()],
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
                is_active: false,
            },
            ScreenModel {
                window_id: 20,
                buffer_id: 20,
                rect: PaneRect {
                    x: 20,
                    y: 0,
                    width: 20,
                    height: 4,
                },
                file_name: "right.txt".to_string(),
                mode_label: "NORMAL".to_string(),
                status_line: "test.txt | NORMAL".to_string(),
                cursor_style: ScreenCursorStyle::Block,
                dirty: false,
                lines: vec!["right".to_string()],
                line_projections: vec![],
                cursor_row: 1,
                cursor_col: 2,
                visual_selection: None,
                search_overlays: vec![],
                syntax_chunks: vec![],
                markdown_style_ranges: vec![],
                filer_style_ranges: vec![],
                resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
                message_line: None,
                command_cursor_col: None,
                is_active: false,
            },
        ],
        floats: vec![],
        active_window_id: 20,
        message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
        message_area_height: 5,
        message_scroll_offset: 0,
        prompt_line: None,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: None,
        command_line: None,
    };

    draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::StyledTrueColor)
        .expect("workspace render should succeed");

    terminal
        .backend_mut()
        .assert_cursor_position(Position::new(22, 1));
}

#[test]
fn workspace_render_composes_floats_above_panes_by_zindex() {
    let mut terminal =
        Terminal::new(TestBackend::new(20, 6)).expect("test terminal should initialize");
    let mut model = workspace_with_typed_message(None);
    model.panes[0].rect = PaneRect {
        x: 0,
        y: 0,
        width: 20,
        height: 4,
    };
    model.panes[0].lines = vec!["underneath".to_string()];
    model.floats = vec![
        FloatingScreenModel {
            id: FloatingWindowId(1),
            content: FloatingContentRef::StaticLines { content_id: 1 },
            rect: PaneRect {
                x: 1,
                y: 0,
                width: 10,
                height: 2,
            },
            lines: vec!["low".to_string()],
            inline_styles: Vec::new(),
            images: Vec::new(),
            cursor: None,
            focusable: false,
            mouse: false,
            chrome: FloatingChrome {
                border: FloatingBorder::None,
            },
            zindex: 40,
            creation_order: 1,
        },
        FloatingScreenModel {
            id: FloatingWindowId(2),
            content: FloatingContentRef::StaticLines { content_id: 2 },
            rect: PaneRect {
                x: 1,
                y: 0,
                width: 10,
                height: 2,
            },
            lines: vec!["top".to_string()],
            inline_styles: Vec::new(),
            images: Vec::new(),
            cursor: None,
            focusable: false,
            mouse: false,
            chrome: FloatingChrome {
                border: FloatingBorder::None,
            },
            zindex: 100,
            creation_order: 2,
        },
    ];

    draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::Plain)
        .expect("workspace render should succeed");

    let rendered = terminal.backend().buffer().content();
    let first_row = rendered
        .iter()
        .take(20)
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert_eq!(
        &first_row[1..4],
        "top",
        "topmost float should overwrite the overlapping pane text: {first_row:?}"
    );
    assert!(
        !first_row.contains("under"),
        "pane text must not remain under the resolved float area: {first_row:?}"
    );
}

#[test]
fn workspace_render_draws_bordered_static_line_float() {
    let mut terminal =
        Terminal::new(TestBackend::new(20, 6)).expect("test terminal should initialize");
    let mut model = workspace_with_typed_message(None);
    model.floats = vec![FloatingScreenModel {
        id: FloatingWindowId(1),
        content: FloatingContentRef::StaticLines { content_id: 1 },
        rect: PaneRect {
            x: 1,
            y: 1,
            width: 10,
            height: 3,
        },
        lines: vec!["hover".to_string()],
        inline_styles: Vec::new(),
        images: Vec::new(),
        cursor: Some(FloatingCursor { line: 0, column: 5 }),
        focusable: false,
        mouse: false,
        chrome: FloatingChrome {
            border: FloatingBorder::Single,
        },
        zindex: 40,
        creation_order: 1,
    }];

    draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::Plain)
        .expect("workspace render should succeed");

    let rendered = format!("{}", terminal.backend());

    assert!(
        rendered.contains("┌────────┐"),
        "bordered float should draw a single border: {rendered:?}"
    );
    assert!(
        rendered.contains("│hover"),
        "bordered float should draw content inside the border: {rendered:?}"
    );
    terminal
        .backend_mut()
        .assert_cursor_position(Position::new(7, 2));
}

#[test]
fn workspace_render_applies_inline_styles_to_float_text_via_span_split() {
    use ratatui::style::Modifier;
    let mut terminal =
        Terminal::new(TestBackend::new(30, 6)).expect("test terminal should initialize");
    let mut model = workspace_with_typed_message(None);
    // 行内バイト 0..4 を Code (Bold), 5..9 を Emphasis (Italic)、
    // 10..16 を LinkText (Underlined) として宣言。border 無しで
    // float の最初の行をそのまま検証する。
    model.floats = vec![FloatingScreenModel {
        id: FloatingWindowId(1),
        content: FloatingContentRef::StaticLines { content_id: 1 },
        rect: PaneRect {
            x: 0,
            y: 0,
            width: 20,
            height: 1,
        },
        lines: vec!["abcd efgh ijklmn ".to_string()],
        inline_styles: vec![
            FloatingInlineStyle {
                kind: FloatingInlineStyleKind::Code,
                line: 0,
                column_start: 0,
                column_end: 4,
            },
            FloatingInlineStyle {
                kind: FloatingInlineStyleKind::Emphasis,
                line: 0,
                column_start: 5,
                column_end: 9,
            },
            FloatingInlineStyle {
                kind: FloatingInlineStyleKind::LinkText,
                line: 0,
                column_start: 10,
                column_end: 16,
            },
        ],
        images: Vec::new(),
        cursor: None,
        focusable: false,
        mouse: false,
        chrome: FloatingChrome {
            border: FloatingBorder::None,
        },
        zindex: 40,
        creation_order: 1,
    }];

    draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::StyledTrueColor)
        .expect("workspace render should succeed");

    let buffer = terminal.backend().buffer().clone();
    let cell = |x: u16| buffer[(x, 0u16)].clone();
    assert!(
        cell(0).modifier.contains(Modifier::BOLD),
        "Code range must apply BOLD to first 4 cells: got modifier={:?}",
        cell(0).modifier
    );
    assert!(
        cell(5).modifier.contains(Modifier::ITALIC),
        "Emphasis range must apply ITALIC: got modifier={:?}",
        cell(5).modifier
    );
    assert!(
        cell(10).modifier.contains(Modifier::UNDERLINED),
        "LinkText range must apply UNDERLINED: got modifier={:?}",
        cell(10).modifier
    );
    assert!(
        !cell(4).modifier.contains(Modifier::BOLD),
        "Cell outside the Code range must not be BOLD: got modifier={:?}",
        cell(4).modifier
    );
}

#[test]
fn workspace_render_applies_terminal_cell_styles_to_float_text() {
    use ratatui::style::{Color, Modifier};
    let mut terminal =
        Terminal::new(TestBackend::new(20, 4)).expect("test terminal should initialize");
    let mut model = workspace_with_typed_message(None);
    model.floats = vec![FloatingScreenModel {
        id: FloatingWindowId(1),
        content: FloatingContentRef::Terminal { terminal_id: 7 },
        rect: PaneRect {
            x: 0,
            y: 0,
            width: 12,
            height: 1,
        },
        lines: vec!["styled".to_string()],
        inline_styles: vec![FloatingInlineStyle {
            kind: FloatingInlineStyleKind::TerminalCell(TerminalCellStyle {
                foreground: Some(TerminalColor::Indexed(1)),
                background: Some(TerminalColor::Rgb(1, 2, 3)),
                bold: true,
                underline: true,
                inverse: true,
            }),
            line: 0,
            column_start: 0,
            column_end: 6,
        }],
        images: Vec::new(),
        cursor: Some(FloatingCursor { line: 0, column: 2 }),
        focusable: true,
        mouse: true,
        chrome: FloatingChrome {
            border: FloatingBorder::None,
        },
        zindex: 40,
        creation_order: 1,
    }];

    draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::StyledTrueColor)
        .expect("workspace render should succeed");

    let buffer = terminal.backend().buffer().clone();
    let styled = buffer[(0u16, 0u16)].clone();
    assert_eq!(styled.fg, Color::Indexed(1));
    assert_eq!(styled.bg, Color::Rgb(1, 2, 3));
    assert!(styled.modifier.contains(Modifier::BOLD));
    assert!(styled.modifier.contains(Modifier::UNDERLINED));
    assert!(styled.modifier.contains(Modifier::REVERSED));
    terminal
        .backend_mut()
        .assert_cursor_position(Position::new(2, 0));
}

#[test]
fn workspace_render_does_not_reserve_empty_global_message_row() {
    let mut terminal =
        Terminal::new(TestBackend::new(20, 4)).expect("test terminal should initialize");
    let model = WorkspaceScreenModel {
        panes: vec![ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 20,
                height: 4,
            },
            file_name: "alpha.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            status_line: "alpha.txt | NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()],
            line_projections: vec![],
            cursor_row: 2,
            cursor_col: 1,
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
    };

    draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::StyledTrueColor)
        .expect("workspace render should succeed");

    let rendered = format!("{}", terminal.backend());
    let rows: Vec<&str> = rendered.lines().collect();
    assert!(
        rows.get(3)
            .is_some_and(|row| row.contains("alpha.txt") && row.contains("NORMAL")),
        "message/command がない時は最下段まで local status line を使うこと: {:?}",
        rows
    );
}

fn assert_normal_redraw_removes_stale_message_area_after_dismiss(dismiss_key: &str) {
    let mut terminal =
        Terminal::new(TestBackend::new(20, 8)).expect("test terminal should initialize");
    let mut model = workspace_with_typed_message(Some("one\ntwo\nthree\nfour\nfive"));
    model.panes[0].lines = vec!["alpha".to_string()];

    draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::StyledTrueColor)
        .expect("initial workspace render should succeed");
    let with_message = format!("{}", terminal.backend());
    assert!(
        with_message.contains("four") && with_message.contains("five"),
        "initial render should draw message area: {:?}",
        with_message
    );

    model.message_line = resolve_workspace_message_line(Vec::<MessageLineCandidate>::new());
    model.message_scroll_offset = 0;

    draw_workspace_frame(
        &mut terminal,
        &model,
        false,
        RenderTextMode::StyledTrueColor,
    )
    .expect("dismissed workspace render should succeed");

    let dismissed = format!("{}", terminal.backend());
    assert!(
        !dismissed.contains("four") && !dismissed.contains("five"),
        "normal redraw after {dismiss_key} dismissal must erase stale message area rows without terminal.clear: {:?}",
        dismissed
    );
    assert!(
        dismissed.contains("test.txt") && dismissed.contains("NORMAL"),
        "{dismiss_key} dismissed workspace should reclaim the bottom rows for the buffer/status area: {:?}",
        dismissed
    );
}

#[test]
fn workspace_render_normal_redraw_removes_stale_message_area_after_enter_dismiss() {
    assert_normal_redraw_removes_stale_message_area_after_dismiss("Enter");
}

#[test]
fn workspace_render_normal_redraw_removes_stale_message_area_after_escape_dismiss() {
    assert_normal_redraw_removes_stale_message_area_after_dismiss("Escape");
}

#[test]
fn workspace_render_uses_single_bottom_row_for_command_line_without_message() {
    let mut terminal =
        Terminal::new(TestBackend::new(20, 4)).expect("test terminal should initialize");
    let model = WorkspaceScreenModel {
        panes: vec![ScreenModel {
            window_id: 1,
            buffer_id: 1,
            rect: PaneRect {
                x: 0,
                y: 0,
                width: 20,
                height: 3,
            },
            file_name: "alpha.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            status_line: "alpha.txt | NORMAL".to_string(),
            cursor_style: ScreenCursorStyle::Block,
            dirty: false,
            lines: vec!["alpha".to_string(), "beta".to_string()],
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
        command_line: Some(CommandLineModel {
            text: ":w".to_string(),
            cursor_col: 2,
        }),
    };

    draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::StyledTrueColor)
        .expect("workspace render should succeed");

    let rendered = format!("{}", terminal.backend());
    let rows: Vec<&str> = rendered.lines().collect();
    assert!(
        rows.get(2)
            .is_some_and(|row| row.contains("alpha.txt") && row.contains("NORMAL")),
        "command line だけの時は status line の直下 1 行だけを予約すること: {:?}",
        rows
    );
    assert!(
        rows.get(3).is_some_and(|row| row.contains(":w")),
        "最下段に command line を描画すること: {:?}",
        rows
    );
}

#[test]
fn command_line_overlay_update_appends_without_clearing_current_line() {
    let previous = CommandLineModel {
        text: ":syntax o".to_string(),
        cursor_col: 9,
    };
    let next = CommandLineModel {
        text: ":syntax on".to_string(),
        cursor_col: 10,
    };

    let update = command_line_overlay_update(Some(&previous), &next);

    assert_eq!(
        update,
        CommandLineOverlayUpdate {
            start_col: 9,
            cursor_col: 10,
            text: "n".to_string(),
            clear_current_line_first: false,
            clear_after_text: false,
        }
    );
}

#[test]
fn command_line_overlay_update_clears_tail_only_when_text_shrinks() {
    let previous = CommandLineModel {
        text: ":syntax on".to_string(),
        cursor_col: 10,
    };
    let next = CommandLineModel {
        text: ":syntax o".to_string(),
        cursor_col: 9,
    };

    let update = command_line_overlay_update(Some(&previous), &next);

    assert_eq!(
        update,
        CommandLineOverlayUpdate {
            start_col: 9,
            cursor_col: 9,
            text: String::new(),
            clear_current_line_first: false,
            clear_after_text: true,
        }
    );
}

#[test]
fn command_line_overlay_update_clears_current_line_only_for_first_overlay() {
    let next = CommandLineModel {
        text: ":".to_string(),
        cursor_col: 1,
    };

    let update = command_line_overlay_update(None, &next);

    assert_eq!(
        update,
        CommandLineOverlayUpdate {
            start_col: 0,
            cursor_col: 1,
            text: ":".to_string(),
            clear_current_line_first: true,
            clear_after_text: false,
        }
    );
}

#[test]
fn workspace_layout_exposes_no_global_rows_when_message_and_command_are_absent() {
    let model = workspace_with_typed_message(None);

    let layout = compute_workspace_layout(
        Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 4,
        },
        &model,
    );

    assert_eq!(layout.message_rect, None);
    assert_eq!(layout.command_rect, None);
    assert_eq!(layout.panes[0].rect.height, 3);
    assert_eq!(layout.cursor, Some((0, 0)));
}

#[test]
fn workspace_layout_exposes_single_command_row_without_empty_message_row() {
    let mut pane = screen_model_with_message(None);
    pane.rect.height = 3;
    let model = WorkspaceScreenModel {
        panes: vec![pane],
        floats: vec![],
        active_window_id: 1,
        message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
        message_area_height: 5,
        message_scroll_offset: 0,
        prompt_line: None,
        pager_prompt: None,
        suppressed_prompt_hints: vec![],
        bell: None,
        command_line: Some(CommandLineModel {
            text: ":w".to_string(),
            cursor_col: 2,
        }),
    };

    let layout = compute_workspace_layout(
        Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 4,
        },
        &model,
    );

    assert_eq!(layout.message_rect, None);
    assert_eq!(
        layout.command_rect,
        Some(Rect {
            x: 0,
            y: 3,
            width: 20,
            height: 1,
        })
    );
    assert_eq!(layout.panes[0].rect.height, 3);
    assert_eq!(layout.cursor, None);
}

#[test]
fn workspace_layout_does_not_reserve_row_for_empty_global_message() {
    let model = workspace_with_typed_message(Some(""));

    let layout = compute_workspace_layout(
        Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 4,
        },
        &model,
    );

    assert_eq!(
        layout.message_rect, None,
        "空の message line では global row を予約しないこと"
    );
    assert_eq!(
        layout.panes[0].rect.height, 3,
        "空 message で pane body/status の高さを削らないこと"
    );
}

#[test]
fn workspace_layout_stacks_message_above_command_without_overlap() {
    let mut model = workspace_with_typed_message(Some("saved"));
    model.command_line = Some(CommandLineModel {
        text: ":w".to_string(),
        cursor_col: 2,
    });

    let layout = compute_workspace_layout(
        Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 5,
        },
        &model,
    );

    assert_eq!(
        layout.message_rect,
        Some(Rect {
            x: 0,
            y: 3,
            width: 20,
            height: 1,
        })
    );
    assert_eq!(
        layout.command_rect,
        Some(Rect {
            x: 0,
            y: 4,
            width: 20,
            height: 1,
        })
    );
    assert_eq!(layout.panes[0].rect.height, 3);
}

#[test]
fn workspace_render_draws_message_prompt_pager_and_bell_rows_without_suppressed_hints() {
    let mut terminal =
        Terminal::new(TestBackend::new(32, 6)).expect("test terminal should initialize");
    let mut model = workspace_with_typed_message(Some("saved"));
    model.pager_prompt = Some(PagerPromptView {
        kind: CorePagerPromptKind::More,
        one_shot: true,
    });
    model.prompt_line = Some(InputPromptView {
        prompt: "Name:".to_string(),
        input: "abc".to_string(),
        correlation_id: 7,
        input_kind: CoreInputRequestKind::CommandLine,
        status: InputPromptStatus::Active,
    });
    model.suppressed_prompt_hints = vec![SuppressedPromptHint {
        pager_prompt: PagerPromptView {
            kind: CorePagerPromptKind::HitReturn,
            one_shot: true,
        },
        reason: PromptHintSuppressionReason::ActiveInputPrompt,
    }];
    model.bell = Some(BellIndication { count: 2 });

    draw_workspace_frame(&mut terminal, &model, true, RenderTextMode::StyledTrueColor)
        .expect("workspace render should succeed");

    let rendered = format!("{}", terminal.backend());
    let rows: Vec<&str> = rendered.lines().collect();
    assert!(
        rows.iter()
            .any(|row| row.contains("saved") && row.contains("[bell x2]")),
        "message row should include both visible message and bell marker: {:?}",
        rows
    );
    assert!(
        rows.iter().any(|row| row.contains("[pager: More]")),
        "pager hint should render as its own row: {:?}",
        rows
    );
    assert!(
        rows.iter().any(|row| row.contains("Name: abc")),
        "prompt line should render as its own row: {:?}",
        rows
    );
    assert!(
        !rows.iter().any(|row| row.contains("Confirm")),
        "suppressed prompt hints must stay headless-only: {:?}",
        rows
    );
}

#[test]
fn workspace_layout_saturates_prompt_rows_on_small_terminal_without_overlap() {
    let mut model = workspace_with_typed_message(Some("saved"));
    model.pager_prompt = Some(PagerPromptView {
        kind: CorePagerPromptKind::More,
        one_shot: true,
    });
    model.prompt_line = Some(InputPromptView {
        prompt: "Name:".to_string(),
        input: "abc".to_string(),
        correlation_id: 7,
        input_kind: CoreInputRequestKind::CommandLine,
        status: InputPromptStatus::Active,
    });
    model.command_line = Some(CommandLineModel {
        text: ":w".to_string(),
        cursor_col: 2,
    });

    let layout = compute_workspace_layout(
        Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 2,
        },
        &model,
    );

    let mut rows = Vec::new();
    rows.extend(layout.message_rect.map(|rect| rect.y));
    rows.extend(layout.pager_rect.map(|rect| rect.y));
    rows.extend(layout.prompt_rect.map(|rect| rect.y));
    rows.extend(layout.command_rect.map(|rect| rect.y));
    rows.sort_unstable();
    rows.dedup();

    assert_eq!(layout.command_rect.map(|rect| rect.y), Some(1));
    assert_eq!(layout.panes[0].rect.height, 1);
    assert_eq!(
        rows.len(),
        1,
        "small terminal must not overlap reserved rows"
    );
}
