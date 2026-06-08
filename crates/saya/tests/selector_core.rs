use saya::features::selector::core::{
    CancellationToken, CollectProcessor, DefaultRenderer, HighlightKind, InMemoryResultStore,
    MatchProcessor, NoopPreviewer, PrefixAndMatcher, PreviewProcessor, RenderProcessor,
    ResultStore, SelectorController, SelectorControllerCommand, SelectorError, SelectorHighlight,
    SelectorItem, SelectorLimits, SelectorRenderer, SelectorSessionCore, SelectorViewState,
    SubstringAndMatcher, SuffixAndMatcher, WorkState, parse_ascii_space_and_query,
};

fn item(id: &str, value: &str) -> SelectorItem<()> {
    SelectorItem {
        id: id.to_string(),
        value: value.to_string(),
        kind: "test".to_string(),
        detail: (),
    }
}

#[test]
fn ascii_space_query_parsing_ignores_empty_terms_only() {
    let query = parse_ascii_space_and_query("  alpha   beta\tgamma  delta  ");

    assert_eq!(query.terms(), ["alpha", "beta\tgamma", "delta"]);
}

#[test]
fn prefix_substring_and_suffix_matchers_use_ascii_space_and_semantics() {
    let candidate = item("1", "src/main.rs parser query");

    let prefix = PrefixAndMatcher::default()
        .match_item(&candidate, &parse_ascii_space_and_query("src par"))
        .expect("both prefix terms should match token starts");
    assert_eq!(
        prefix.highlights,
        vec![
            SelectorHighlight {
                column: 0,
                width: 3,
                kind: HighlightKind::Match,
            },
            SelectorHighlight {
                column: 12,
                width: 3,
                kind: HighlightKind::Match,
            },
        ]
    );

    let substring = SubstringAndMatcher::default()
        .match_item(&candidate, &parse_ascii_space_and_query("main query"))
        .expect("both substring terms should match anywhere");
    assert_eq!(substring.highlights[0].column, 4);
    assert_eq!(substring.highlights[0].width, 4);
    assert_eq!(substring.highlights[1].column, 19);
    assert_eq!(substring.highlights[1].width, 5);

    let suffix = SuffixAndMatcher::default()
        .match_item(&candidate, &parse_ascii_space_and_query("rs query"))
        .expect("both suffix terms should match token ends");
    assert_eq!(suffix.highlights[0].column, 9);
    assert_eq!(suffix.highlights[0].width, 2);
    assert_eq!(suffix.highlights[1].column, 19);
    assert_eq!(suffix.highlights[1].width, 5);

    assert!(
        SubstringAndMatcher::default()
            .match_item(&candidate, &parse_ascii_space_and_query("main missing"))
            .is_none(),
        "AND semantics must reject candidates missing any term"
    );
}

#[test]
fn empty_or_repeated_space_query_matches_every_item_without_highlights() {
    let matched = SubstringAndMatcher::default()
        .match_item(&item("1", "anything"), &parse_ascii_space_and_query("    "))
        .expect("empty query should keep all items visible");

    assert!(matched.highlights.is_empty());
}

#[test]
fn memory_result_store_appends_scans_gets_and_counts_items() {
    let mut store = InMemoryResultStore::new();
    let token = CancellationToken::new();

    store.append(item("a", "alpha")).expect("append alpha");
    store.append(item("b", "beta")).expect("append beta");

    assert_eq!(store.count(), 2);
    assert_eq!(store.get("b").expect("get beta").unwrap().value, "beta");
    assert_eq!(store.status().total_stored, 2);
    assert_eq!(
        store
            .scan(&token)
            .expect("scan all")
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        ["a", "b"]
    );

    store.dispose().expect("dispose memory store");
    assert_eq!(store.count(), 0);
}

#[test]
fn collect_processor_stores_all_items_and_reports_completion() {
    let mut store = InMemoryResultStore::new();
    let token = CancellationToken::new();

    let output = CollectProcessor::new()
        .collect_items(
            &mut store,
            [item("a", "alpha"), item("b", "beta"), item("c", "gamma")],
            &token,
        )
        .expect("collect items");

    assert_eq!(output.status.state, WorkState::Completed);
    assert_eq!(output.status.total_seen, 3);
    assert_eq!(output.status.total_stored, 3);
    assert_eq!(store.count(), 3);
}

#[test]
fn preview_processor_supports_noop_headless_preview() {
    let token = CancellationToken::new();
    let processor = PreviewProcessor::new();

    let output = processor
        .preview(&item("a", "alpha"), &NoopPreviewer, &token)
        .expect("noop preview");

    assert_eq!(output.status.state, WorkState::Completed);
    assert!(output.content.is_none());
    assert_eq!(
        processor.status().expect("preview status").state,
        WorkState::Completed
    );
}

#[test]
fn max_rendered_items_limits_rendering_but_not_match_coverage() {
    let mut store = InMemoryResultStore::new();
    for index in 0..1500 {
        store
            .append(item(
                &format!("item-{index}"),
                &format!("row {index} needle"),
            ))
            .expect("append item");
    }

    let token = CancellationToken::new();
    let matches = MatchProcessor::new()
        .match_store(&store, &SubstringAndMatcher::default(), "needle", &token)
        .expect("match all stored items");
    assert_eq!(matches.status.state, WorkState::Completed);
    assert_eq!(matches.status.total_matched, 1500);

    let rendered = RenderProcessor::new(SelectorLimits {
        max_rendered_items: 10,
    })
    .render(&matches.items, &DefaultRenderer, &token)
    .expect("render first page");

    assert_eq!(rendered.status.total_matched, 1500);
    assert_eq!(rendered.status.total_rendered, 10);
    assert_eq!(rendered.items.len(), 10);
}

#[test]
fn cancellation_and_stale_result_suppression_are_explicit() {
    let mut store = InMemoryResultStore::new();
    store.append(item("a", "alpha")).expect("append alpha");
    let cancelled = CancellationToken::new();
    cancelled.cancel();

    let result = MatchProcessor::new()
        .match_store(&store, &SubstringAndMatcher::default(), "alpha", &cancelled)
        .expect("cancelled matching reports cancelled status");
    assert_eq!(result.status.state, WorkState::Cancelled);
    assert!(result.items.is_empty());

    let mut session = SelectorSessionCore::new();
    let stale_run = session.begin_match_work();
    session.cancel_active_work();

    assert!(
        !session.is_active_work(stale_run),
        "cancelled work must not be able to update the active session"
    );
}

#[test]
fn selector_controller_moves_cursor_and_offset_within_rendered_items() {
    let mut view = SelectorViewState::new(3);
    let controller = SelectorController::new(5);

    controller.apply(&mut view, SelectorControllerCommand::CursorNext);
    controller.apply(&mut view, SelectorControllerCommand::CursorNext);
    controller.apply(&mut view, SelectorControllerCommand::CursorNext);
    assert_eq!(view.cursor, 2);
    assert_eq!(view.offset, 0);

    controller.apply(&mut view, SelectorControllerCommand::CursorLast);
    assert_eq!(view.cursor, 2);
    assert_eq!(view.offset, 0);

    let mut paged = SelectorViewState::new(25);
    controller.apply(&mut paged, SelectorControllerCommand::PageDown);
    assert_eq!(paged.cursor, 5);
    assert_eq!(paged.offset, 5);

    controller.apply(&mut paged, SelectorControllerCommand::PageDown);
    assert_eq!(paged.cursor, 10);
    assert_eq!(paged.offset, 10);

    controller.apply(&mut paged, SelectorControllerCommand::CursorPrevious);
    assert_eq!(paged.cursor, 9);
    assert_eq!(paged.offset, 9);

    controller.apply(&mut paged, SelectorControllerCommand::PageUp);
    assert_eq!(paged.cursor, 4);
    assert_eq!(paged.offset, 4);

    controller.apply(&mut paged, SelectorControllerCommand::CursorFirst);
    assert_eq!(paged.cursor, 0);
    assert_eq!(paged.offset, 0);
}

#[test]
fn selector_controller_keeps_cursor_moving_inside_last_page_when_going_up() {
    let mut view = SelectorViewState::new(25);
    let controller = SelectorController::new(5);

    controller.apply(&mut view, SelectorControllerCommand::CursorLast);
    assert_eq!(view.cursor, 24);
    assert_eq!(
        view.offset, 20,
        "last item should be shown at the bottom of the final page"
    );

    controller.apply(&mut view, SelectorControllerCommand::CursorPrevious);
    assert_eq!(view.cursor, 23);
    assert_eq!(
        view.offset, 20,
        "moving up inside the visible final page must not keep the cursor pinned to the bottom"
    );

    controller.apply(&mut view, SelectorControllerCommand::CursorPrevious);
    assert_eq!(view.cursor, 22);
    assert_eq!(view.offset, 20);
}

#[test]
fn selector_controller_distinguishes_hide_from_cancel_state() {
    let mut view = SelectorViewState::new(2);
    let controller = SelectorController::new(10);

    controller.apply(&mut view, SelectorControllerCommand::Hide);
    assert!(view.hidden);
    assert!(!view.cancelled);

    controller.apply(&mut view, SelectorControllerCommand::Show);
    assert!(!view.hidden);
    assert!(!view.cancelled);

    controller.apply(&mut view, SelectorControllerCommand::Cancel);
    assert!(view.hidden);
    assert!(view.cancelled);

    controller.apply(&mut view, SelectorControllerCommand::Show);
    assert!(
        view.hidden,
        "show must not resurrect a cancelled selector session"
    );
    assert!(view.cancelled);
}

struct FailingRenderer;

impl SelectorRenderer<()> for FailingRenderer {
    fn render(
        &self,
        _item: &saya::features::selector::core::MatchedItem<()>,
    ) -> Result<saya::features::selector::core::RenderedItem, SelectorError> {
        Err(SelectorError::Failed("render failed".to_string()))
    }
}

#[test]
fn processors_expose_failed_status_transitions() {
    let matched = SubstringAndMatcher::default()
        .match_item(&item("1", "alpha"), &parse_ascii_space_and_query("alpha"))
        .expect("fixture item matches");
    let processor = RenderProcessor::new(SelectorLimits::default());
    let token = CancellationToken::new();

    let result = processor.render(&[matched], &FailingRenderer, &token);

    assert!(result.is_err());
    let status = processor
        .status()
        .expect("failed render status is recorded");
    assert_eq!(status.state, WorkState::Failed);
    assert_eq!(status.error_message.as_deref(), Some("render failed"));
}
