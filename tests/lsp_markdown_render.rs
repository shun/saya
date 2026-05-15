//! markdown_render: LSP hover などで届く markdown 形式テキストを
//! float window が表示可能な「行 + インラインスタイル」へ変換する
//! 汎用レンダリングサービスの単体テスト。
//!
//! saya コアは markdown を解析する責務を `markdown_structure` に、
//! float 表示用整形を `markdown_render` に分離する。これにより
//! `lsp_float.rs` は markdown の存在を知らずに描画行を受け取れる。

use saya::presentation::markdown::render::{
    InlineStyleKind, RenderedFloatContent, render_markdown_to_float_content,
    render_plaintext_to_float_content,
};

fn line_strings(content: &RenderedFloatContent) -> Vec<&str> {
    content.lines.iter().map(String::as_str).collect()
}

fn styles_on_line(content: &RenderedFloatContent, line: usize) -> Vec<InlineStyleKind> {
    content
        .inline_styles
        .iter()
        .filter(|style| style.line == line)
        .map(|style| style.kind)
        .collect()
}

#[test]
fn render_markdown_strips_fence_marker_lines_and_keeps_code_body() {
    let source = "```rust\nfn foo(x: i32) -> i32 { x }\n```\n";
    let rendered = render_markdown_to_float_content(source);
    assert_eq!(
        line_strings(&rendered),
        vec!["fn foo(x: i32) -> i32 { x }"],
        "fenced code block markers must be removed, body preserved"
    );
}

#[test]
fn render_markdown_replaces_list_marker_with_bullet_glyph() {
    let source = "- alpha\n- beta\n* gamma\n+ delta\n";
    let rendered = render_markdown_to_float_content(source);
    assert_eq!(
        line_strings(&rendered),
        vec!["• alpha", "• beta", "• gamma", "• delta"],
        "list markers `-` `*` `+` must be normalized to `•`"
    );
}

#[test]
fn render_markdown_renders_link_as_visible_text_only_without_url_suffix() {
    let source = "See [Reference](https://example.com) for details.\n";
    let rendered = render_markdown_to_float_content(source);
    assert_eq!(
        line_strings(&rendered),
        vec!["See Reference for details."],
        "links should be rendered as visible text only; the URL is not shown inline"
    );
}

#[test]
fn render_markdown_preserves_inline_code_backticks_for_later_styling() {
    let source = "Call `foo()` to invoke.\n";
    let rendered = render_markdown_to_float_content(source);
    assert_eq!(
        line_strings(&rendered),
        vec!["Call `foo()` to invoke."],
        "inline code backticks must be kept for later styling"
    );
    assert!(
        styles_on_line(&rendered, 0).contains(&InlineStyleKind::Code),
        "inline_styles must record InlineCode range for the rendered line"
    );
}

#[test]
fn render_markdown_preserves_emphasis_markers_and_records_inline_style() {
    let source = "This is *important* and also **strong**.\n";
    let rendered = render_markdown_to_float_content(source);
    assert_eq!(
        line_strings(&rendered),
        vec!["This is *important* and also **strong**."],
        "emphasis markers must be preserved literally"
    );
    let styles = styles_on_line(&rendered, 0);
    assert!(
        styles.iter().any(|kind| *kind == InlineStyleKind::Emphasis),
        "inline_styles must record Emphasis ranges: {styles:?}"
    );
}

#[test]
fn render_markdown_keeps_heading_prefix_intact_for_visual_hierarchy() {
    let source = "# Title\n## Subtitle\nbody\n";
    let rendered = render_markdown_to_float_content(source);
    assert_eq!(
        line_strings(&rendered),
        vec!["# Title", "## Subtitle", "body"],
        "heading prefixes must be preserved for visual hierarchy"
    );
}

#[test]
fn render_markdown_records_link_text_style_range_on_rendered_line() {
    let source = "Open [Docs](https://example.com).\n";
    let rendered = render_markdown_to_float_content(source);
    let styles = styles_on_line(&rendered, 0);
    assert!(
        styles
            .iter()
            .any(|kind| matches!(kind, InlineStyleKind::LinkText)),
        "link rendering must record a LinkText range on the visible text: {styles:?}"
    );
    assert!(
        !styles
            .iter()
            .any(|kind| matches!(kind, InlineStyleKind::LinkUrl)),
        "URL is hidden, so no LinkUrl range should be emitted: {styles:?}"
    );
}

#[test]
fn render_markdown_trims_trailing_blank_lines_and_keeps_internal_blank_separators() {
    let source = "para one\n\npara two\n\n";
    let rendered = render_markdown_to_float_content(source);
    assert_eq!(
        line_strings(&rendered),
        vec!["para one", "", "para two"],
        "internal blank separator must remain, but trailing blanks must be trimmed"
    );
}

#[test]
fn render_markdown_expands_tab_to_four_spaces_to_avoid_terminal_render_glitches() {
    // gopls の hover は Go ソースの構造体 signature を fenced code block で
    // 返してくることが多く、行頭インデントが TAB になっている。ratatui の
    // Cell は単一文字を扱うため、TAB をそのまま流すと表示が崩れる。
    let source = "type Cfg struct {\n\tName string\n\tTags []string\n}\n";
    let rendered = render_markdown_to_float_content(source);
    assert_eq!(
        line_strings(&rendered),
        vec![
            "type Cfg struct {",
            "    Name string",
            "    Tags []string",
            "}",
        ],
        "TAB indentation must be expanded into 4 spaces for terminal-safe rendering"
    );
}

#[test]
fn render_markdown_unescapes_backslash_escaped_punctuation_for_visible_text() {
    // gopls は markdown 内の特殊文字（バッククォート等）を `\` でエスケープする。
    // markdown 仕様に従い、`\X` は X 単独の文字として表示する。
    let source = "use \\`backtick\\` and \\*literal-star\\*\n";
    let rendered = render_markdown_to_float_content(source);
    assert_eq!(
        line_strings(&rendered),
        vec!["use `backtick` and *literal-star*"],
        "backslash-escaped punctuation must collapse to the literal character"
    );
}

#[test]
fn render_markdown_does_not_unescape_backslash_when_followed_by_non_punctuation() {
    // 通常テキスト中の単独の `\X`（X が句読点でない）はそのまま残す。
    let source = "path is C:\\Users\\saya\n";
    let rendered = render_markdown_to_float_content(source);
    assert_eq!(
        line_strings(&rendered),
        vec!["path is C:\\Users\\saya"],
        "non-punctuation after backslash must be preserved as-is"
    );
}

#[test]
fn render_plaintext_passes_lines_through_without_markdown_transformations() {
    let source = "fn foo()\n- not a list\n[not](a link)\n";
    let rendered = render_plaintext_to_float_content(source);
    assert_eq!(
        line_strings(&rendered),
        vec!["fn foo()", "- not a list", "[not](a link)"],
        "plaintext rendering must not apply markdown transformations"
    );
    assert!(
        rendered.inline_styles.is_empty(),
        "plaintext rendering must produce no inline styles"
    );
}
