use super::*;

fn window(cursor_row: usize) -> CoreWindowInfo {
    CoreWindowInfo {
        id: 7,
        buf_id: 3,
        row: 0,
        col: 0,
        width: 80,
        height: 10,
        topline: 1,
        botline: 3,
        leftcol: 0,
        skipcol: 0,
        cursor_row,
        cursor_col: 0,
        is_active: true,
    }
}

fn line_range(start_row: usize, lines: &[&str]) -> CoreBufferLineRange {
    CoreBufferLineRange {
        buffer_id: 3,
        source_revision: vim_core_rs::CoreBufferRevision { value: 0 },
        start_row,
        line_count: lines.len(),
        total_line_count: lines.len(),
        lines: lines.iter().map(|line| line.to_string()).collect(),
    }
}

#[test]
fn parses_percent_substitute_as_visible_line_live_preview() {
    let preview = build_substitute_preview_render(
        &window(0),
        &line_range(0, &["foo foo", "bar foo"]),
        Some(':'),
        "%s/foo/baz",
    )
    .expect("substitute preview should be built");

    assert_eq!(preview.search_state.mode, SearchQueryMode::IncsearchPreview);
    assert_eq!(preview.search_state.pattern.as_deref(), Some("foo"));
    assert_eq!(preview.line_range.lines, vec!["baz foo", "bar baz"]);
    assert_eq!(
        preview
            .search_state
            .matches
            .iter()
            .map(|m| (m.start_row, m.start_col, m.end_col))
            .collect::<Vec<_>>(),
        vec![(1, 0, 3), (2, 4, 7)]
    );
}

#[test]
fn global_flag_previews_every_match_on_each_visible_line() {
    let preview = build_substitute_preview_render(
        &window(0),
        &line_range(0, &["foo foo", "bar foo"]),
        Some(':'),
        "%s/foo/baz/g",
    )
    .expect("substitute preview should be built");

    assert_eq!(preview.line_range.lines, vec!["baz baz", "bar baz"]);
    assert_eq!(
        preview
            .search_state
            .matches
            .iter()
            .map(|m| (m.start_row, m.start_col, m.end_col))
            .collect::<Vec<_>>(),
        vec![(1, 0, 3), (1, 4, 7), (2, 4, 7)]
    );
}

#[test]
fn unqualified_substitute_previews_only_the_cursor_line() {
    let preview = build_substitute_preview_render(
        &window(1),
        &line_range(0, &["foo", "foo foo", "foo"]),
        Some(':'),
        "s/foo/bar",
    )
    .expect("substitute preview should be built");

    assert_eq!(preview.line_range.lines, vec!["foo", "bar foo", "foo"]);
    assert_eq!(
        preview
            .search_state
            .matches
            .iter()
            .map(|m| (m.start_row, m.start_col, m.end_col))
            .collect::<Vec<_>>(),
        vec![(2, 0, 3)]
    );
}

#[test]
fn open_pattern_without_replacement_highlights_matches_without_mutating_text() {
    let preview = build_substitute_preview_render(
        &window(0),
        &line_range(0, &["fill fish", "no match"]),
        Some(':'),
        "%s/fi",
    )
    .expect("pattern-only substitute preview should be built");

    assert_eq!(preview.search_state.mode, SearchQueryMode::IncsearchPreview);
    assert_eq!(preview.search_state.pattern.as_deref(), Some("fi"));
    // テキストは置換フィールド未入力なので変更しない。
    assert_eq!(preview.line_range.lines, vec!["fill fish", "no match"]);
    // g フラグが無い substitute は行ごとに最初のマッチだけが置換対象になるため、
    // ライブプレビューでも最初のマッチだけをハイライトする（neovim の inccommand 準拠）。
    assert_eq!(
        preview
            .search_state
            .matches
            .iter()
            .map(|m| (m.start_row, m.start_col, m.end_col))
            .collect::<Vec<_>>(),
        vec![(1, 0, 2)]
    );
}

#[test]
fn open_pattern_respects_current_line_scope() {
    let preview = build_substitute_preview_render(
        &window(1),
        &line_range(0, &["foo", "foo foo", "foo"]),
        Some(':'),
        "s/fo",
    )
    .expect("pattern-only substitute preview should be built");

    assert_eq!(preview.line_range.lines, vec!["foo", "foo foo", "foo"]);
    // g フラグが無いので、カーソル行の最初のマッチだけハイライトする。
    assert_eq!(
        preview
            .search_state
            .matches
            .iter()
            .map(|m| (m.start_row, m.start_col, m.end_col))
            .collect::<Vec<_>>(),
        vec![(2, 0, 2)]
    );
}

#[test]
fn empty_open_pattern_does_not_build_preview() {
    assert!(
        build_substitute_preview_render(&window(0), &line_range(0, &["fill"]), Some(':'), "%s/",)
            .is_none()
    );
}
