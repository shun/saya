//! 行単位のレンダリングとオーバーレイ（検索・選択・構文）の合成。

use super::*;

pub(super) fn render_line(
    model: &ScreenModel,
    index: usize,
    line: &str,
    width: u16,
    text_mode: RenderTextMode,
) -> Line<'static> {
    let row = u16::try_from(index).unwrap_or(u16::MAX);
    let overlays = collect_render_overlays(model, row, line);
    let base_style = model
        .resolved_theme
        .ui_style(UiStyleKey::Text)
        .cloned()
        .unwrap_or_default();
    if overlays.is_empty() {
        let style = style_for_buffer_base_text(base_style, text_mode);
        let line = if style == Style::default() {
            Line::from(line.to_string())
        } else {
            Line::from(Span::styled(line.to_string(), style))
        };
        return pad_line_to_width(line, width);
    }

    render_layered_line(
        line,
        &overlays,
        width,
        text_mode,
        &model.resolved_theme,
        base_style,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RenderOverlayKind {
    Ui(UiStyleKey),
    Filer(ResolvedTextStyle),
    Markdown(ResolvedTextStyle),
    Syntax(RenderSyntaxStyle),
    VisualSelection,
    Search(crate::features::search::query::SearchMatchKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RenderSyntaxStyle {
    pub(super) vim_family: Option<&'static str>,
    pub(super) language: Option<String>,
    pub(super) tree_sitter: Option<RenderTreeSitterSyntaxStyle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RenderTreeSitterSyntaxStyle {
    pub(super) category: ScreenSyntaxCategory,
    pub(super) definition: bool,
    pub(super) documentation: bool,
    pub(super) deprecated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RenderOverlayRange {
    start_col: usize,
    end_col_exclusive: usize,
    kind: RenderOverlayKind,
}

pub(super) fn collect_render_overlays(
    model: &ScreenModel,
    row: u16,
    line: &str,
) -> Vec<RenderOverlayRange> {
    let mut overlays = Vec::new();

    if let Some(end_col_exclusive) = line_number_gutter_end_col(line) {
        overlays.push(RenderOverlayRange {
            start_col: 0,
            end_col_exclusive,
            kind: RenderOverlayKind::Ui(UiStyleKey::Gutter),
        });
    }

    overlays.extend(
        model
            .filer_style_ranges
            .iter()
            .filter(|range| range.row == row)
            .filter(|range| range.end_col_exclusive > range.start_col)
            .map(|range| RenderOverlayRange {
                start_col: usize::from(range.start_col),
                end_col_exclusive: usize::from(range.end_col_exclusive),
                kind: RenderOverlayKind::Filer(range.style.clone()),
            }),
    );

    overlays.extend(
        model
            .markdown_style_ranges
            .iter()
            .filter(|range| range.row == row)
            .filter(|range| range.end_col_exclusive > range.start_col)
            .map(|range| RenderOverlayRange {
                start_col: usize::from(range.start_col),
                end_col_exclusive: usize::from(range.end_col_exclusive),
                kind: RenderOverlayKind::Markdown(range.style.clone()),
            }),
    );

    overlays.extend(
        model
            .syntax_chunks
            .iter()
            .filter(|chunk| chunk.row == row)
            .filter(|chunk| chunk.end_col_exclusive > chunk.start_col)
            .map(|chunk| RenderOverlayRange {
                start_col: usize::from(chunk.start_col),
                end_col_exclusive: usize::from(chunk.end_col_exclusive),
                kind: RenderOverlayKind::Syntax(syntax_style(chunk)),
            }),
    );

    if let Some(selection) = model.visual_selection {
        if row >= selection.start_row && row <= selection.end_row {
            let start_col = if row == selection.start_row {
                usize::from(selection.start_col)
            } else {
                usize::from(selection.line_start_col)
            };
            let end_col_exclusive = if row == selection.end_row {
                usize::from(selection.end_col_exclusive)
            } else {
                display_width(line)
            };
            if end_col_exclusive > start_col {
                overlays.push(RenderOverlayRange {
                    start_col,
                    end_col_exclusive,
                    kind: RenderOverlayKind::VisualSelection,
                });
            }
        }
    }

    overlays.extend(
        model
            .search_overlays
            .iter()
            .filter(|overlay| overlay.row == row)
            .filter(|overlay| overlay.end_col_exclusive > overlay.start_col)
            .map(|overlay| RenderOverlayRange {
                start_col: usize::from(overlay.start_col),
                end_col_exclusive: usize::from(overlay.end_col_exclusive),
                kind: RenderOverlayKind::Search(overlay.kind),
            }),
    );

    overlays.sort_by_key(|overlay| {
        (
            overlay.start_col,
            overlay.end_col_exclusive,
            overlay_kind_rank(&overlay.kind),
        )
    });
    overlays
}

pub(super) fn render_layered_line(
    line: &str,
    overlays: &[RenderOverlayRange],
    width: u16,
    text_mode: RenderTextMode,
    theme: &ResolvedTheme,
    base_style: ResolvedTextStyle,
) -> Line<'static> {
    let line_width = display_width(line);
    let mut boundaries = vec![0usize, line_width];
    for overlay in overlays {
        boundaries.push(overlay.start_col.min(line_width));
        boundaries.push(overlay.end_col_exclusive.min(line_width));
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut spans = Vec::new();
    for window in boundaries.windows(2) {
        let start_col = window[0];
        let end_col_exclusive = window[1];
        if end_col_exclusive <= start_col {
            continue;
        }
        let text = slice_line_by_display_columns(line, start_col, end_col_exclusive);
        let mut style = style_for_buffer_base_text(base_style.clone(), text_mode);
        let mut matching = overlays
            .iter()
            .filter(|overlay| {
                overlay.start_col < end_col_exclusive && overlay.end_col_exclusive > start_col
            })
            .collect::<Vec<_>>();
        matching.sort_by_key(|overlay| overlay_kind_rank(&overlay.kind));
        for overlay in matching {
            if overlay_replaces_lower_layers(&overlay.kind) {
                style = Style::default();
            }
            style = style.patch(style_for_overlay_kind(
                overlay.kind.clone(),
                text_mode,
                theme,
            ));
        }
        if style == Style::default() {
            spans.push(Span::raw(text));
        } else {
            spans.push(Span::styled(text, style));
        }
    }

    pad_line_to_width(Line::from(spans), width)
}

pub(super) fn overlay_replaces_lower_layers(kind: &RenderOverlayKind) -> bool {
    matches!(
        kind,
        RenderOverlayKind::VisualSelection | RenderOverlayKind::Search(_)
    )
}

pub(super) fn overlay_kind_rank(kind: &RenderOverlayKind) -> usize {
    match kind {
        RenderOverlayKind::Ui(_) => 0,
        RenderOverlayKind::Syntax(style) if syntax_is_base_markdown(style) => 1,
        RenderOverlayKind::Syntax(style) if style.language.is_none() => 1,
        RenderOverlayKind::Filer(_) => 2,
        RenderOverlayKind::Markdown(_) => 2,
        RenderOverlayKind::Syntax(_) => 3,
        RenderOverlayKind::Search(crate::features::search::query::SearchMatchKind::Regular) => 4,
        RenderOverlayKind::Search(crate::features::search::query::SearchMatchKind::Incremental) => {
            5
        }
        RenderOverlayKind::Search(crate::features::search::query::SearchMatchKind::Current) => 6,
        RenderOverlayKind::VisualSelection => 7,
    }
}

pub(super) fn syntax_is_base_markdown(style: &RenderSyntaxStyle) -> bool {
    style
        .language
        .as_deref()
        .map(|language| {
            let normalized = language.trim().to_ascii_lowercase();
            normalized == "markdown" || normalized == "md"
        })
        .unwrap_or(false)
}

pub(super) fn style_for_overlay_kind(
    kind: RenderOverlayKind,
    text_mode: RenderTextMode,
    theme: &ResolvedTheme,
) -> Style {
    if text_mode == RenderTextMode::Plain {
        return Style::default();
    }
    match kind {
        RenderOverlayKind::Ui(key) => theme
            .ui_style(key)
            .cloned()
            .map(|style| style_for_text(style, text_mode))
            .unwrap_or_default(),
        RenderOverlayKind::Filer(style) => style_for_text(style, text_mode),
        RenderOverlayKind::Markdown(style) => style_for_markdown(style, text_mode),
        RenderOverlayKind::Syntax(style) => style_for_syntax(style, text_mode, theme),
        RenderOverlayKind::VisualSelection => Style::default().add_modifier(Modifier::REVERSED),
        RenderOverlayKind::Search(crate::features::search::query::SearchMatchKind::Current) => {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        }
        RenderOverlayKind::Search(crate::features::search::query::SearchMatchKind::Incremental) => {
            Style::default().fg(Color::White).bg(Color::Blue)
        }
        RenderOverlayKind::Search(crate::features::search::query::SearchMatchKind::Regular) => {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        }
    }
}

pub(super) fn line_number_gutter_end_col(line: &str) -> Option<usize> {
    let mut saw_digit = false;
    let mut width = 0usize;
    for ch in line.chars() {
        if ch == ' ' && !saw_digit {
            width += 1;
            continue;
        }
        if ch.is_ascii_digit() {
            saw_digit = true;
            width += 1;
            continue;
        }
        if ch == ' ' && saw_digit {
            return Some(width + 1);
        }
        return None;
    }
    None
}
