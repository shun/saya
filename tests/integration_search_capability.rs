use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::bootstrap::{prepare_launch, BootstrapOutcome};
use saya::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::search_capability::SearchCapabilityContract;
use saya::search_query::{SearchMatchKind, SearchQueryMode, SearchStateError, SearchVisibleQuery};

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-integ-search-capability-{name}-{nanos}"))
}

fn launch_with_content(content: &str) -> BootstrapOutcome {
    let target_path = unique_path("search-content");
    std::fs::write(&target_path, content).expect("テストファイルの作成");

    prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path),
        config_source: ConfigSource::Default,
        ..LaunchRequest::default()
    })
    .expect("テスト用の起動が成功すること")
}

fn full_viewport_query() -> SearchVisibleQuery {
    SearchVisibleQuery {
        start_row: 1,
        end_row: 3,
    }
}

#[test]
fn search_capability_contract_reports_live_state_query_available() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let outcome = launch_with_content("alpha\nbeta alpha\ngamma alpha\n");

    let contract = outcome.core_bridge.search_capability_contract();

    assert_eq!(contract, SearchCapabilityContract::baseline_ready_contract());
    assert!(contract.live_state_query_available);
    assert!(contract.visible_rows_only);
    assert!(contract.start_col_inclusive);
    assert!(contract.end_col_exclusive);
}

#[test]
fn query_visible_search_state_returns_typed_highlight_data() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("zero\nalpha one\nalpha two\nomega\n");

    outcome
        .core_bridge
        .apply_ex_command(":set hlsearch")
        .expect("hlsearch should be enabled in core before query");
    outcome.core_bridge.dispatch_key("/alpha\r").unwrap();

    let state = outcome
        .core_bridge
        .query_visible_search_state(SearchVisibleQuery {
            start_row: 2,
            end_row: 3,
        })
        .expect("search query should succeed");

    assert_eq!(
        state.capability,
        outcome.core_bridge.search_capability_contract()
    );
    assert_eq!(state.window_id, outcome.core_bridge.snapshot().windows[0].id);
    assert_eq!(state.visible_rows.start_row, 2);
    assert_eq!(state.visible_rows.end_row, 3);
    assert_eq!(state.mode, SearchQueryMode::Hlsearch);
    assert_eq!(state.pattern.as_deref(), Some("alpha"));
    assert_eq!(state.input_pattern, None);
    assert!(state.hlsearch_enabled);
    assert!(!state.hlsearch_suspended);
    assert!(!state.incsearch_active);
    assert_eq!(state.matches.len(), 2);
    assert_eq!(state.matches[0].kind, SearchMatchKind::Current);
    assert_eq!(state.matches[0].start_row, 2);
    assert_eq!(state.matches[0].start_col, 0);
    assert_eq!(state.matches[0].end_col, 5);
    assert_eq!(state.matches[1].kind, SearchMatchKind::Regular);
    assert_eq!(state.matches[1].start_row, 3);
}

#[test]
fn query_visible_search_state_keeps_search_start_at_the_first_matched_character() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("zero\nxvimx\nomega\n");

    outcome
        .core_bridge
        .apply_ex_command(":set hlsearch")
        .expect("hlsearch should be enabled in core before query");
    outcome.core_bridge.dispatch_key("/vim\r").unwrap();

    let state = outcome
        .core_bridge
        .query_visible_search_state(full_viewport_query())
        .expect("search query should succeed");

    assert_eq!(state.matches.len(), 1);
    assert_eq!(
        state.matches[0].start_col, 1,
        "search highlight should start at the 'v' in xvimx, not the preceding character"
    );
    assert_eq!(state.matches[0].end_col, 4);
}

#[test]
fn query_visible_search_state_handles_full_width_match_bounds() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("zero\nxあx\nomega\n");

    outcome
        .core_bridge
        .apply_ex_command(":set hlsearch")
        .expect("hlsearch should be enabled in core before query");
    outcome.core_bridge.dispatch_key("/あ\r").unwrap();

    let state = outcome
        .core_bridge
        .query_visible_search_state(full_viewport_query())
        .expect("search query should succeed");

    assert_eq!(state.matches.len(), 1);
    assert_eq!(state.matches[0].start_row, 2);
    assert_eq!(state.matches[0].start_col, 1);
    assert_eq!(
        state.matches[0].end_col, 4,
        "full-width matches should keep the core exclusive end column"
    );
}

#[test]
fn query_visible_search_state_reports_incsearch_preview_when_active() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("alpha\nhello world hello\nomega\n");

    outcome
        .core_bridge
        .apply_ex_command(":set incsearch hlsearch")
        .expect("incsearch should be enabled");
    outcome.core_bridge.sync_search_input("hello").unwrap();

    let state = outcome
        .core_bridge
        .query_visible_search_state(full_viewport_query())
        .expect("incsearch preview should be queryable");

    assert_eq!(state.mode, SearchQueryMode::IncsearchPreview);
    assert!(state.incsearch_active);
    assert_eq!(state.input_pattern.as_deref(), Some("hello"));
    assert_eq!(state.pattern.as_deref(), Some("hello"));
    assert!(
        state
            .matches
            .iter()
            .any(|range| range.kind == SearchMatchKind::Current)
    );
    assert!(
        state
            .matches
            .iter()
            .any(|range| range.kind == SearchMatchKind::Incremental)
    );
}

#[test]
fn query_visible_search_state_keeps_input_pattern_without_preview_when_noincsearch() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("alpha\nhello world hello\nomega\n");

    outcome
        .core_bridge
        .apply_ex_command(":let @/ = ''")
        .expect("search register reset should succeed");
    outcome
        .core_bridge
        .apply_ex_command(":set noincsearch nohlsearch")
        .expect("incsearch should be disabled");
    outcome.core_bridge.sync_search_input("hello").unwrap();

    let state = outcome
        .core_bridge
        .query_visible_search_state(full_viewport_query())
        .expect("disabled incsearch state should still be queryable");

    assert_eq!(state.mode, SearchQueryMode::Disabled);
    assert!(!state.incsearch_active);
    assert_eq!(state.input_pattern.as_deref(), Some("hello"));
    assert!(state.pattern.is_none());
    assert!(state.matches.is_empty());
}

#[test]
fn query_visible_search_state_clears_preview_after_escape() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("alpha\nhello world hello\nomega\n");

    outcome
        .core_bridge
        .apply_ex_command(":let @/ = ''")
        .expect("search register reset should succeed");
    outcome
        .core_bridge
        .apply_ex_command(":set incsearch nohlsearch")
        .expect("incsearch should be enabled");
    outcome.core_bridge.sync_search_input("hello").unwrap();
    outcome.core_bridge.cancel_search_input().unwrap();

    let state = outcome
        .core_bridge
        .query_visible_search_state(full_viewport_query())
        .expect("state after cancel should be queryable");

    assert_eq!(state.mode, SearchQueryMode::Disabled);
    assert!(!state.incsearch_active);
    assert!(state.input_pattern.is_none());
    assert!(state.pattern.is_none());
    assert!(state.matches.is_empty());
}

#[test]
fn query_visible_search_state_commits_preview_into_regular_search_after_enter() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("alpha\nhello world hello\nomega\n");

    outcome
        .core_bridge
        .apply_ex_command(":let @/ = ''")
        .expect("search register reset should succeed");
    outcome
        .core_bridge
        .apply_ex_command(":set incsearch hlsearch")
        .expect("incsearch should be enabled");
    outcome.core_bridge.sync_search_input("hello").unwrap();
    outcome.core_bridge.commit_search_input("hello").unwrap();

    let state = outcome
        .core_bridge
        .query_visible_search_state(full_viewport_query())
        .expect("committed search should be queryable");

    assert_eq!(state.mode, SearchQueryMode::Hlsearch);
    assert!(!state.incsearch_active);
    assert!(state.input_pattern.is_none());
    assert_eq!(state.pattern.as_deref(), Some("hello"));
    assert!(
        state
            .matches
            .iter()
            .all(|range| range.kind != SearchMatchKind::Incremental)
    );
}

#[test]
fn query_visible_search_state_marks_hlsearch_suspend_as_empty_overlay() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("zero\nalpha one\nalpha two\nomega\n");

    outcome
        .core_bridge
        .apply_ex_command(":set hlsearch")
        .expect("hlsearch should be enabled in core before suspend");
    outcome.core_bridge.dispatch_key("/alpha\r").unwrap();
    outcome
        .core_bridge
        .apply_ex_command(":nohlsearch")
        .expect("nohlsearch should be handled by core");

    let state = outcome
        .core_bridge
        .query_visible_search_state(SearchVisibleQuery {
            start_row: 1,
            end_row: 2,
        })
        .expect("suspended search query should still resolve");

    assert_eq!(state.mode, SearchQueryMode::Disabled);
    assert!(state.hlsearch_enabled);
    assert!(state.hlsearch_suspended);
    assert!(!state.incsearch_active);
    assert!(state.matches.is_empty());
}

#[test]
fn query_visible_search_state_rejects_invalid_viewport() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("zero\nalpha one\nalpha two\nomega\n");

    let error = outcome
        .core_bridge
        .query_visible_search_state(SearchVisibleQuery {
            start_row: 3,
            end_row: 2,
        })
        .expect_err("invalid viewport must be rejected");

    assert_eq!(
        error,
        SearchStateError::InvalidViewport {
            start_row: 3,
            end_row: 2,
        }
    );
}

#[test]
fn core_owned_search_option_updates_change_query_results() {
    let _lock = saya::bootstrap::launch_test_lock()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let mut outcome = launch_with_content("zero\nalpha one\nalpha two\nomega\n");

    outcome
        .core_bridge
        .apply_ex_command(":set hlsearch")
        .expect("core-owned hlsearch enable should succeed");
    outcome.core_bridge.dispatch_key("/alpha\r").unwrap();

    let query = SearchVisibleQuery {
        start_row: 1,
        end_row: 2,
    };

    let enabled_state = outcome
        .core_bridge
        .query_visible_search_state(query)
        .expect("enabled query should succeed");
    assert!(enabled_state.hlsearch_enabled);
    assert!(!enabled_state.matches.is_empty());

    outcome
        .core_bridge
        .apply_ex_command(":set nohlsearch")
        .expect("core-owned hlsearch disable should succeed");
    let disabled_state = outcome
        .core_bridge
        .query_visible_search_state(query)
        .expect("disabled query should succeed");
    assert!(!disabled_state.hlsearch_enabled);
    assert!(!disabled_state.hlsearch_suspended);
    assert!(disabled_state.matches.is_empty());

    outcome
        .core_bridge
        .apply_ex_command(":set hlsearch")
        .expect("core-owned hlsearch re-enable should succeed");
    let reenabled_state = outcome
        .core_bridge
        .query_visible_search_state(query)
        .expect("re-enabled query should succeed");
    assert!(reenabled_state.hlsearch_enabled);
    assert!(!reenabled_state.matches.is_empty());
}
