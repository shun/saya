//! テーマ・構文ハイライトのスタイル解決とインライン/端末スタイル変換、表示幅ユーティリティ。

use super::*;

/// 行を `FloatingInlineStyle` に従って Span に分割する。Span 自体は
/// Style::default() のままにして、Paragraph 全体に適用される `base_style`
/// に対し inline スタイル領域だけが Modifier を patch する設計。
/// これで Block::bordered() の border が Span の style に上書きされない。
pub(super) fn line_to_styled_spans<'a>(
    line: &'a str,
    line_index: usize,
    inline_styles: &[FloatingInlineStyle],
) -> Vec<Span<'a>> {
    let line_bytes = line.len();
    let mut applicable: Vec<&FloatingInlineStyle> = inline_styles
        .iter()
        .filter(|style| style.line == line_index)
        .collect();
    applicable.sort_by_key(|style| (style.column_start, style.column_end));

    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut cursor = 0usize;
    for style in applicable {
        let start = style.column_start.min(line_bytes);
        let end = style.column_end.min(line_bytes);
        if start < cursor || start >= end {
            continue;
        }
        if start > cursor {
            spans.push(Span::raw(&line[cursor..start]));
        }
        spans.push(Span::styled(
            &line[start..end],
            inline_kind_modifier_style(style.kind),
        ));
        cursor = end;
    }
    if cursor < line_bytes {
        spans.push(Span::raw(&line[cursor..]));
    }
    if spans.is_empty() {
        spans.push(Span::raw(line));
    }
    spans
}

/// inline スタイル種別ごとの追加 Modifier。背景・前景の色は触らず
/// modifier だけを足すことで、Paragraph 全体の base_style と合成される。
pub(super) fn inline_kind_modifier_style(kind: FloatingInlineStyleKind) -> Style {
    use ratatui::style::Modifier;
    match kind {
        FloatingInlineStyleKind::Selection => Style::default().add_modifier(Modifier::REVERSED),
        FloatingInlineStyleKind::Match => Style::default()
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::UNDERLINED),
        FloatingInlineStyleKind::Code => Style::default().add_modifier(Modifier::BOLD),
        FloatingInlineStyleKind::Emphasis => Style::default().add_modifier(Modifier::ITALIC),
        FloatingInlineStyleKind::Heading { .. } => Style::default().add_modifier(Modifier::BOLD),
        FloatingInlineStyleKind::LinkText => Style::default().add_modifier(Modifier::UNDERLINED),
        FloatingInlineStyleKind::LinkUrl => Style::default()
            .add_modifier(Modifier::UNDERLINED)
            .add_modifier(Modifier::DIM),
        FloatingInlineStyleKind::TerminalCell(style) => terminal_cell_style(style),
    }
}

pub(super) fn terminal_cell_style(style: TerminalCellStyle) -> Style {
    use ratatui::style::Modifier;
    let mut tui_style = Style::default();
    if let Some(foreground) = style.foreground {
        tui_style = tui_style.fg(terminal_color(foreground));
    }
    if let Some(background) = style.background {
        tui_style = tui_style.bg(terminal_color(background));
    }
    if style.bold {
        tui_style = tui_style.add_modifier(Modifier::BOLD);
    }
    if style.underline {
        tui_style = tui_style.add_modifier(Modifier::UNDERLINED);
    }
    if style.inverse {
        tui_style = tui_style.add_modifier(Modifier::REVERSED);
    }
    tui_style
}

pub(super) fn terminal_color(color: TerminalColor) -> ratatui::style::Color {
    match color {
        TerminalColor::Indexed(index) => ratatui::style::Color::Indexed(index),
        TerminalColor::Rgb(red, green, blue) => ratatui::style::Color::Rgb(red, green, blue),
    }
}

pub(super) fn style_for_markdown(style: ResolvedTextStyle, text_mode: RenderTextMode) -> Style {
    style_for_text(style, text_mode)
}

pub(super) fn style_for_buffer_base_text(
    mut style: ResolvedTextStyle,
    text_mode: RenderTextMode,
) -> Style {
    if style.bg.is_some() {
        log::debug!(
            "[tui_renderer] ignoring ui.text.bg for buffer text cells so text, tabs, and padding keep the terminal background"
        );
        style.bg = None;
    }
    style_for_text(style, text_mode)
}

pub(super) fn style_for_text(style: ResolvedTextStyle, text_mode: RenderTextMode) -> Style {
    let mut rendered = Style::default();
    if colors_enabled(text_mode) {
        if let Some(fg) = style.fg {
            rendered = rendered.fg(color_for_resolved_theme_color(&fg));
        }
        if let Some(bg) = style.bg {
            rendered = rendered.bg(color_for_resolved_theme_color(&bg));
        }
    }
    if style.bold {
        rendered = rendered.add_modifier(Modifier::BOLD);
    }
    if style.italic {
        rendered = rendered.add_modifier(Modifier::ITALIC);
    }
    if style.underline {
        rendered = rendered.add_modifier(Modifier::UNDERLINED);
    }
    if style.strikethrough {
        rendered = rendered.add_modifier(Modifier::CROSSED_OUT);
    }
    rendered
}

pub(super) fn colors_enabled(text_mode: RenderTextMode) -> bool {
    matches!(
        text_mode,
        RenderTextMode::StyledAnsi | RenderTextMode::StyledTrueColor
    )
}

pub(super) fn color_for_resolved_theme_color(color: &ResolvedThemeColor) -> Color {
    let hex = color.0.trim_start_matches('#');
    if hex.len() == 6 {
        if let (Ok(red), Ok(green), Ok(blue)) = (
            u8::from_str_radix(&hex[0..2], 16),
            u8::from_str_radix(&hex[2..4], 16),
            u8::from_str_radix(&hex[4..6], 16),
        ) {
            return Color::Rgb(red, green, blue);
        }
    }
    log::debug!(
        "[tui_renderer] unresolved renderer color fallback used for theme color: {:?}",
        color
    );
    Color::Reset
}

pub(super) fn syntax_style(
    chunk: &crate::presentation::screen_model::ScreenSyntaxChunk,
) -> RenderSyntaxStyle {
    RenderSyntaxStyle {
        vim_family: syntax_family(chunk.name.as_deref()),
        language: chunk.language.clone(),
        tree_sitter: chunk.tree_sitter.as_ref().map(tree_sitter_syntax_style),
    }
}

pub(super) fn tree_sitter_syntax_style(
    syntax: &ScreenTreeSitterSyntax,
) -> RenderTreeSitterSyntaxStyle {
    RenderTreeSitterSyntaxStyle {
        category: syntax.category,
        definition: syntax.modifiers.contains(&ScreenSyntaxModifier::Definition),
        documentation: syntax
            .modifiers
            .contains(&ScreenSyntaxModifier::Documentation),
        deprecated: syntax.modifiers.contains(&ScreenSyntaxModifier::Deprecated),
    }
}

pub(super) fn style_for_syntax(
    style: RenderSyntaxStyle,
    text_mode: RenderTextMode,
    theme: &ResolvedTheme,
) -> Style {
    if let Some(tree_sitter) = style.tree_sitter {
        return style_for_tree_sitter_syntax(
            tree_sitter,
            style.language.as_deref(),
            text_mode,
            theme,
        );
    }
    style_for_syntax_family(
        style.vim_family,
        style.language.as_deref(),
        text_mode,
        theme,
    )
}

pub(super) fn style_for_tree_sitter_syntax(
    syntax: RenderTreeSitterSyntaxStyle,
    language: Option<&str>,
    text_mode: RenderTextMode,
    theme: &ResolvedTheme,
) -> Style {
    if text_mode == RenderTextMode::Plain {
        return Style::default();
    }
    let key = syntax_key_for_tree_sitter(syntax.category);
    let mut style = theme
        .syntax_style_for_language(language, key)
        .map(|style| style_for_text(style, text_mode))
        .unwrap_or_else(|| fallback_style_for_tree_sitter_syntax(syntax.category, text_mode));
    if syntax.definition || syntax.documentation {
        style = style.add_modifier(Modifier::BOLD);
    }
    if syntax.deprecated {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
}

pub(super) fn syntax_key_for_tree_sitter(category: ScreenSyntaxCategory) -> SyntaxSemanticStyleKey {
    match category {
        ScreenSyntaxCategory::Comment => SyntaxSemanticStyleKey::Comment,
        ScreenSyntaxCategory::String => SyntaxSemanticStyleKey::String,
        ScreenSyntaxCategory::Constant | ScreenSyntaxCategory::Number => {
            SyntaxSemanticStyleKey::Constant
        }
        ScreenSyntaxCategory::Keyword | ScreenSyntaxCategory::Operator => {
            SyntaxSemanticStyleKey::Statement
        }
        ScreenSyntaxCategory::Function | ScreenSyntaxCategory::Constructor => {
            SyntaxSemanticStyleKey::Function
        }
        ScreenSyntaxCategory::Type => SyntaxSemanticStyleKey::Type,
        ScreenSyntaxCategory::Punctuation => SyntaxSemanticStyleKey::Punctuation,
        ScreenSyntaxCategory::Markup | ScreenSyntaxCategory::Tag | ScreenSyntaxCategory::Label => {
            SyntaxSemanticStyleKey::Markup
        }
        ScreenSyntaxCategory::Variable
        | ScreenSyntaxCategory::Property
        | ScreenSyntaxCategory::Attribute => SyntaxSemanticStyleKey::Identifier,
        ScreenSyntaxCategory::Module
        | ScreenSyntaxCategory::Text
        | ScreenSyntaxCategory::Unknown => SyntaxSemanticStyleKey::Default,
    }
}

pub(super) fn fallback_style_for_tree_sitter_syntax(
    category: ScreenSyntaxCategory,
    text_mode: RenderTextMode,
) -> Style {
    if !colors_enabled(text_mode) {
        return Style::default();
    }
    match category {
        ScreenSyntaxCategory::Comment => Style::default().fg(Color::DarkGray),
        ScreenSyntaxCategory::String => Style::default().fg(Color::Green),
        ScreenSyntaxCategory::Constant | ScreenSyntaxCategory::Number => {
            Style::default().fg(Color::Magenta)
        }
        ScreenSyntaxCategory::Keyword | ScreenSyntaxCategory::Operator => {
            Style::default().fg(Color::Cyan)
        }
        ScreenSyntaxCategory::Function
        | ScreenSyntaxCategory::Constructor
        | ScreenSyntaxCategory::Type
        | ScreenSyntaxCategory::Variable
        | ScreenSyntaxCategory::Property
        | ScreenSyntaxCategory::Attribute => Style::default().fg(Color::Yellow),
        ScreenSyntaxCategory::Markup | ScreenSyntaxCategory::Tag | ScreenSyntaxCategory::Label => {
            Style::default().fg(Color::Blue)
        }
        ScreenSyntaxCategory::Module
        | ScreenSyntaxCategory::Punctuation
        | ScreenSyntaxCategory::Text
        | ScreenSyntaxCategory::Unknown => Style::default().fg(Color::White),
    }
}

pub(super) fn syntax_family(name: Option<&str>) -> Option<&'static str> {
    let name = name?;
    let normalized = name.to_ascii_lowercase();
    if normalized.contains("comment") || normalized.contains("todo") {
        Some("comment")
    } else if normalized.contains("string") || normalized.contains("character") {
        Some("string")
    } else if normalized.contains("number")
        || normalized.contains("float")
        || normalized.contains("boolean")
        || normalized.contains("constant")
        || normalized.contains("char")
    {
        Some("constant")
    } else if normalized.contains("operator")
        || normalized.contains("punctuation")
        || normalized.contains("delimiter")
        || normalized.contains("separator")
        || normalized.contains("sigil")
        || normalized.contains("arrow")
        || normalized.contains("modpathsep")
    {
        Some("punctuation")
    } else if normalized.contains("statement")
        || normalized.contains("keyword")
        || normalized.contains("conditional")
        || normalized.contains("repeat")
        || normalized.contains("storage")
        || normalized.contains("storageclass")
        || normalized.contains("visibility")
        || normalized.contains("preproc")
        || normalized.contains("include")
        || normalized.contains("define")
        || normalized.contains("exception")
    {
        Some("statement")
    } else if normalized.contains("function")
        || normalized.contains("func")
        || normalized.contains("method")
        || normalized.contains("macro")
    {
        Some("function")
    } else if normalized.contains("type")
        || normalized.contains("struct")
        || normalized.contains("enum")
        || normalized.contains("trait")
        || normalized.contains("typedef")
        || normalized.contains("modpath")
    {
        Some("type")
    } else if normalized.contains("identifier") {
        Some("identifier")
    } else {
        Some("default")
    }
}

pub(super) fn style_for_syntax_family(
    family: Option<&'static str>,
    language: Option<&str>,
    text_mode: RenderTextMode,
    theme: &ResolvedTheme,
) -> Style {
    if text_mode == RenderTextMode::Plain || !colors_enabled(text_mode) {
        return Style::default();
    }
    let key = match family {
        Some("comment") => SyntaxSemanticStyleKey::Comment,
        Some("string") => SyntaxSemanticStyleKey::String,
        Some("constant") => SyntaxSemanticStyleKey::Constant,
        Some("statement") => SyntaxSemanticStyleKey::Statement,
        Some("identifier") => SyntaxSemanticStyleKey::Identifier,
        Some("type") => SyntaxSemanticStyleKey::Type,
        Some("function") => SyntaxSemanticStyleKey::Function,
        Some("punctuation") => SyntaxSemanticStyleKey::Punctuation,
        Some("default") | None => SyntaxSemanticStyleKey::Default,
        Some(_) => SyntaxSemanticStyleKey::Default,
    };
    if let Some(style) = theme.syntax_style_for_language(language, key) {
        return style_for_text(style, text_mode);
    }
    match family {
        Some("comment") => Style::default().fg(Color::DarkGray),
        Some("string") => Style::default().fg(Color::Green),
        Some("constant") => Style::default().fg(Color::Magenta),
        Some("statement") => Style::default().fg(Color::Cyan),
        Some("identifier") => Style::default().fg(Color::Yellow),
        Some("type") => Style::default().fg(Color::Blue),
        Some("function") => Style::default().fg(Color::Yellow),
        Some("punctuation") => Style::default().fg(Color::White),
        Some("default") | None => Style::default().fg(Color::White),
        Some(_) => Style::default().fg(Color::White),
    }
}

pub(super) fn slice_line_by_display_columns(
    line: &str,
    start_col: usize,
    end_col_exclusive: usize,
) -> String {
    let mut result = String::new();
    let mut display_col = 0usize;

    for ch in line.chars() {
        let width = ch.width().unwrap_or(0);
        let next_col = display_col.saturating_add(width);
        if next_col <= start_col {
            display_col = next_col;
            continue;
        }
        if display_col >= end_col_exclusive {
            break;
        }
        result.push(ch);
        display_col = next_col;
    }

    result
}

pub(super) fn pad_line_to_width(mut line: Line<'static>, width: u16) -> Line<'static> {
    let rendered_width = line.width();
    let target_width = usize::from(width);
    if rendered_width < target_width {
        line.spans
            .push(Span::raw(" ".repeat(target_width - rendered_width)));
    }
    line
}

pub(super) fn display_width(text: &str) -> usize {
    text.chars().map(|ch| ch.width().unwrap_or(0)).sum()
}
