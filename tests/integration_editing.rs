/// 統合テスト: 編集フローの検証
///
/// モード遷移、画面投影、入力、dirty 状態が一連で動くことを確認する。
/// 代表的な editing-flow smoke のみを残す。
/// Requirements: 2.1, 2.2, 2.3, 2.4, 2.5
use std::path::PathBuf;
use std::sync::MutexGuard;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::app::bootstrap::{BootstrapOutcome, launch_test_lock, prepare_launch};
use saya::app::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::app::session::EditorSessionState;
use saya::input::router::{EditorIntent, KeyInput, resolve_intent};
use saya::presentation::screen_model::{ProjectionInput, project};
use saya::presentation::viewport::{ViewportState, ViewportSyncMode, WindowViewportStore};

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-integ-edit-{name}-{nanos}"))
}

fn test_lock() -> MutexGuard<'static, ()> {
    launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// テスト用に起動済みセッションを生成するヘルパー。
fn launch_with_content(content: &str) -> BootstrapOutcome {
    let target_path = unique_path("edit-content");
    std::fs::write(&target_path, content).expect("テストファイルの作成");

    prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path),
        config_source: ConfigSource::Default,
        ..LaunchRequest::default()
    })
    .expect("テスト用の起動が成功すること")
}

/// テスト用に新規バッファで起動するヘルパー。
fn launch_empty() -> BootstrapOutcome {
    prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::Default,
        ..LaunchRequest::default()
    })
    .expect("テスト用の新規バッファ起動が成功すること")
}

// host-integration: input routing, core bridge, and screen projection as a
// representative smoke flow.
// ---- 9.2.1: モード遷移が一連で動くことを確認する ----

/// 起動 → i でインサート → テキスト入力 → Esc でノーマル復帰の流れが
/// CoreBridge + InputRouter + ScreenModel を横断して成立する。
#[test]
fn mode_transition_flow_through_input_router_to_screen_model() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("hello\n");

    // 起動直後はノーマルモード
    let session_state = EditorSessionState::new(outcome.target_path.clone());
    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));
    assert_eq!(model.mode_label, "NORMAL");

    // InputRouter で 'i' キーを intent 変換
    let intent = resolve_intent(&KeyInput::Char('i'));
    assert_eq!(intent, EditorIntent::EditKey("i".to_string()));

    // CoreBridge で dispatch
    outcome
        .core_bridge
        .dispatch_key("i")
        .expect("i キーの dispatch");
    let snapshot = outcome.core_bridge.snapshot();

    // ScreenModel 投影
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));
    assert_eq!(model.mode_label, "INSERT");

    // Esc でノーマルモードに復帰
    let esc_intent = resolve_intent(&KeyInput::Escape);
    assert_eq!(esc_intent, EditorIntent::EditKey("\x1b".to_string()));

    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Escape dispatch");
    let snapshot = outcome.core_bridge.snapshot();

    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));
    assert_eq!(model.mode_label, "NORMAL");
}

// ---- 9.2.2: 画面投影が ScreenModel に追随する ----

/// viewport と visual selection の投影が ScreenModel に反映される。
// host-integration: viewport projection and terminal-visible cursor handling
// are application responsibilities.
#[test]
fn viewport_auto_scroll_keeps_cursor_visible_during_vertical_motion() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("line1\nline2\nline3\nline4\nline5\nline6\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());
    let mut viewport = ViewportState::new();
    let body_height = 3usize;

    for _ in 0..4 {
        outcome.core_bridge.dispatch_key("j").expect("j dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport.ensure_cursor_visible(
            snapshot.cursor_row,
            body_height,
            snapshot.text.lines().count(),
        );
    }

    let snapshot = outcome.core_bridge.snapshot();
    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_viewport(viewport.top_line(), body_height),
    );

    assert_eq!(
        viewport.top_line(),
        2,
        "4 行目移動時に viewport が追従すること"
    );
    assert_eq!(model.lines, vec!["line3", "line4", "line5"]);
    assert_eq!(model.cursor_row, 2, "カーソルが本文領域内へ保たれること");
}

/// page scroll は cursor 位置ではなく core window の topline を信頼して投影する。
#[test]
fn page_scroll_uses_core_window_topline_for_forward_and_backward_motion() {
    let _lock = test_lock();
    let content = (1..=40)
        .map(|line| format!("line{line}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut outcome = launch_with_content(&content);
    let session_state = EditorSessionState::new(outcome.target_path.clone());
    let mut viewport = ViewportState::new();
    let body_height = 10usize;

    outcome.core_bridge.set_screen_size(12, 80);

    let initial_snapshot = outcome.core_bridge.snapshot();
    let initial_window = initial_snapshot
        .windows
        .iter()
        .find(|window| window.is_active)
        .or_else(|| initial_snapshot.windows.first())
        .expect("active window should exist");
    viewport.sync_from_core_topline(
        initial_window.topline,
        body_height,
        initial_snapshot.text.lines().count(),
    );
    let initial_top_line = viewport.top_line();

    outcome
        .core_bridge
        .dispatch_key("\u{6}")
        .expect("Ctrl+F dispatch");
    let forward_snapshot = outcome.core_bridge.snapshot();
    let forward_window = forward_snapshot
        .windows
        .iter()
        .find(|window| window.is_active)
        .or_else(|| forward_snapshot.windows.first())
        .expect("active window should exist after Ctrl+F");
    viewport.sync_from_core_topline(
        forward_window.topline,
        body_height,
        forward_snapshot.text.lines().count(),
    );
    let forward_top_line = viewport.top_line();

    assert!(
        forward_top_line > initial_top_line,
        "Ctrl+F should advance the viewport: initial={}, forward={}",
        initial_top_line,
        forward_top_line
    );

    outcome
        .core_bridge
        .dispatch_key("\u{2}")
        .expect("Ctrl+B dispatch");
    let backward_snapshot = outcome.core_bridge.snapshot();
    let backward_window = backward_snapshot
        .windows
        .iter()
        .find(|window| window.is_active)
        .or_else(|| backward_snapshot.windows.first())
        .expect("active window should exist after Ctrl+B");
    assert_eq!(
        backward_window.topline,
        initial_window.topline,
        "Ctrl+B should restore the core topline to the initial page: initial={}, forward={}, backward={}, initial_cursor=({},{}), forward_cursor=({},{}), backward_cursor=({}, {})",
        initial_window.topline,
        forward_window.topline,
        backward_window.topline,
        initial_snapshot.cursor_row,
        initial_snapshot.cursor_col,
        forward_snapshot.cursor_row,
        forward_snapshot.cursor_col,
        backward_snapshot.cursor_row,
        backward_snapshot.cursor_col
    );
    viewport.sync_from_core_topline(
        backward_window.topline,
        body_height,
        backward_snapshot.text.lines().count(),
    );

    assert_eq!(
        viewport.top_line(),
        initial_top_line,
        "Ctrl+B should restore the viewport to the previous page"
    );

    let model = project(
        &ProjectionInput::new(&backward_snapshot, &session_state, None)
            .with_viewport(viewport.top_line(), body_height),
    );
    assert_eq!(
        model.lines.first().map(String::as_str),
        Some("line1"),
        "Ctrl+B で元の先頭行が再び表示されること"
    );
}

#[test]
fn ctrl_f_to_last_page_then_k_scrolls_viewport_one_line() {
    let _lock = test_lock();
    let content = (1..=120)
        .map(|line| format!("line{line}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut outcome = launch_with_content(&content);
    let mut viewport_store = WindowViewportStore::new();
    let mut line_counts = std::collections::BTreeMap::new();
    line_counts.insert(1, content.lines().count());

    outcome.core_bridge.set_screen_size(12, 80);

    for _ in 0..20 {
        outcome
            .core_bridge
            .dispatch_key("\u{6}")
            .expect("Ctrl+F dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport_store.sync_from_windows_for_render(
            &snapshot.windows,
            &std::collections::BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::Core,
        );
        let window = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .expect("active window should exist after Ctrl+F");
        if window.botline >= content.lines().count() {
            break;
        }
    }

    let before_snapshot = outcome.core_bridge.snapshot();
    let before_window = before_snapshot
        .windows
        .iter()
        .find(|window| window.is_active)
        .or_else(|| before_snapshot.windows.first())
        .expect("active window should exist on last page");
    let before_viewport_top = viewport_store
        .get(before_window.id)
        .expect("viewport should be synced")
        .top_line();
    assert!(
        before_window.botline >= content.lines().count(),
        "test setup should reach the last page: topline={}, botline={}, cursor_row={}",
        before_window.topline,
        before_window.botline,
        before_window.cursor_row
    );

    outcome.core_bridge.dispatch_key("k").expect("k dispatch");

    let after_snapshot = outcome.core_bridge.snapshot();
    viewport_store.sync_from_windows_for_render(
        &after_snapshot.windows,
        &std::collections::BTreeSet::new(),
        &line_counts,
        ViewportSyncMode::SmoothLineMotion,
    );
    let after_window = after_snapshot
        .windows
        .iter()
        .find(|window| window.is_active)
        .or_else(|| after_snapshot.windows.first())
        .expect("active window should exist after k");
    let after_viewport_top = viewport_store
        .get(after_window.id)
        .expect("viewport should be synced after k")
        .top_line();

    assert_eq!(
        after_viewport_top,
        before_viewport_top.saturating_sub(1),
        "k after Ctrl+F reaches the last page should scroll the viewport up by one line: before_top={}, after_top={}, before_cursor={}, after_cursor={}",
        before_viewport_top,
        after_viewport_top,
        before_window.cursor_row,
        after_window.cursor_row
    );
}

#[test]
fn ctrl_f_to_last_page_then_repeated_k_scrolls_viewport_one_line_per_key() {
    let _lock = test_lock();
    let content = (1..=120)
        .map(|line| format!("line{line}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut outcome = launch_with_content(&content);
    let mut viewport_store = WindowViewportStore::new();
    let mut line_counts = std::collections::BTreeMap::new();
    line_counts.insert(1, content.lines().count());

    outcome.core_bridge.set_screen_size(12, 80);

    for _ in 0..20 {
        outcome
            .core_bridge
            .dispatch_key("\u{6}")
            .expect("Ctrl+F dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport_store.sync_from_windows_for_render(
            &snapshot.windows,
            &std::collections::BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::Core,
        );
        let window = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .expect("active window should exist after Ctrl+F");
        if window.botline >= content.lines().count() {
            break;
        }
    }

    let before_snapshot = outcome.core_bridge.snapshot();
    let before_window = before_snapshot
        .windows
        .iter()
        .find(|window| window.is_active)
        .or_else(|| before_snapshot.windows.first())
        .expect("active window should exist on last page");
    assert!(
        before_window.botline >= content.lines().count(),
        "test setup should reach the last page: topline={}, botline={}, cursor_row={}",
        before_window.topline,
        before_window.botline,
        before_window.cursor_row
    );

    let mut observed = Vec::new();
    for _ in 0..11 {
        outcome.core_bridge.dispatch_key("k").expect("k dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport_store.sync_from_windows_for_render(
            &snapshot.windows,
            &std::collections::BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );
        let window = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .expect("active window should exist after k");
        let viewport_top = viewport_store
            .get(window.id)
            .expect("viewport should be synced after k")
            .top_line();
        observed.push((
            window.topline,
            window.cursor_row,
            viewport_top,
            window.cursor_row.saturating_sub(viewport_top),
        ));
    }

    assert_eq!(
        observed,
        vec![
            (108, 117, 107, 10),
            (107, 116, 106, 10),
            (106, 115, 105, 10),
            (105, 114, 104, 10),
            (104, 113, 103, 10),
            (103, 112, 102, 10),
            (102, 111, 101, 10),
            (101, 110, 100, 10),
            (100, 109, 99, 10),
            (99, 108, 98, 10),
            (98, 107, 97, 10),
        ],
        "k after Ctrl+F reaches the last page should scroll the viewport up one line per key while preserving the cursor screen row"
    );
}

#[test]
fn ctrl_f_to_last_page_then_repeated_k_should_scroll_viewport_one_line_per_key() {
    let _lock = test_lock();
    let content = (1..=120)
        .map(|line| format!("line{line}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut outcome = launch_with_content(&content);
    let mut viewport_store = WindowViewportStore::new();
    let mut line_counts = std::collections::BTreeMap::new();
    line_counts.insert(1, content.lines().count());

    outcome.core_bridge.set_screen_size(12, 80);

    for _ in 0..20 {
        outcome
            .core_bridge
            .dispatch_key("\u{6}")
            .expect("Ctrl+F dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport_store.sync_from_windows_for_render(
            &snapshot.windows,
            &std::collections::BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::Core,
        );
        let window = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .expect("active window should exist after Ctrl+F");
        if window.botline >= content.lines().count() {
            break;
        }
    }

    let before_snapshot = outcome.core_bridge.snapshot();
    let before_window = before_snapshot
        .windows
        .iter()
        .find(|window| window.is_active)
        .or_else(|| before_snapshot.windows.first())
        .expect("active window should exist on last page");
    let before_viewport_top = viewport_store
        .get(before_window.id)
        .expect("viewport should be synced")
        .top_line();
    let expected_relative = before_window.cursor_row.saturating_sub(before_viewport_top);
    assert!(
        before_window.botline >= content.lines().count(),
        "test setup should reach the last page: topline={}, botline={}, cursor_row={}",
        before_window.topline,
        before_window.botline,
        before_window.cursor_row
    );

    for step in 1..=3 {
        outcome.core_bridge.dispatch_key("k").expect("k dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport_store.sync_from_windows_for_render(
            &snapshot.windows,
            &std::collections::BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );
        let window = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .expect("active window should exist after k");
        let viewport_top = viewport_store
            .get(window.id)
            .expect("viewport should be synced after k")
            .top_line();
        assert_eq!(
            viewport_top,
            before_viewport_top.saturating_sub(step),
            "k after Ctrl+F reaches the last page should scroll the viewport up by one line per key: step={step}, before_top={before_viewport_top}, actual_top={viewport_top}, cursor_row={}",
            window.cursor_row
        );
        assert_eq!(
            window.cursor_row.saturating_sub(viewport_top),
            expected_relative,
            "k after Ctrl+F reaches the last page should preserve cursor screen position while scrolling: step={step}, expected_relative={expected_relative}, actual_relative={}, cursor_row={}, viewport_top={viewport_top}",
            window.cursor_row.saturating_sub(viewport_top),
            window.cursor_row
        );
    }
}

#[test]
fn ctrl_f_to_one_line_last_page_then_k_scrolls_viewport_one_line_per_key() {
    let _lock = test_lock();
    let content = (1..=59)
        .map(|line| format!("line{line}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut outcome = launch_with_content(&content);
    let mut viewport_store = WindowViewportStore::new();
    let mut line_counts = std::collections::BTreeMap::new();
    line_counts.insert(1, content.lines().count());

    outcome.core_bridge.set_screen_size(55, 96);

    for _ in 0..5 {
        outcome
            .core_bridge
            .dispatch_key("\u{6}")
            .expect("Ctrl+F dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport_store.sync_from_windows_for_render(
            &snapshot.windows,
            &std::collections::BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::Core,
        );
        let window = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .expect("active window should exist after Ctrl+F");
        if window.topline >= content.lines().count() {
            break;
        }
    }

    let before_snapshot = outcome.core_bridge.snapshot();
    let before_window = before_snapshot
        .windows
        .iter()
        .find(|window| window.is_active)
        .or_else(|| before_snapshot.windows.first())
        .expect("active window should exist on one-line last page");
    let before_viewport_top = viewport_store
        .get(before_window.id)
        .expect("viewport should be synced")
        .top_line();
    assert_eq!(
        before_viewport_top, 58,
        "test setup should render the one-line last page: core_topline={}, viewport_top={}, cursor_row={}",
        before_window.topline, before_viewport_top, before_window.cursor_row
    );

    let mut observed = Vec::new();
    for _ in 0..3 {
        outcome.core_bridge.dispatch_key("k").expect("k dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport_store.sync_from_windows_for_render(
            &snapshot.windows,
            &std::collections::BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );
        let window = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .expect("active window should exist after k");
        let viewport_top = viewport_store
            .get(window.id)
            .expect("viewport should be synced after k")
            .top_line();
        observed.push((
            window.topline,
            window.cursor_row,
            viewport_top,
            window.cursor_row.saturating_sub(viewport_top),
        ));
    }

    assert_eq!(
        observed,
        vec![(58, 57, 57, 0), (57, 56, 56, 0), (56, 55, 55, 0)],
        "k after Ctrl+F reaches a one-line last page should move the viewport up one line per key"
    );
}

#[test]
fn ctrl_b_to_first_page_then_repeated_j_scrolls_viewport_one_line_per_key() {
    let _lock = test_lock();
    let content = (1..=120)
        .map(|line| format!("line{line}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut outcome = launch_with_content(&content);
    let mut viewport_store = WindowViewportStore::new();
    let mut line_counts = std::collections::BTreeMap::new();
    line_counts.insert(1, content.lines().count());

    outcome.core_bridge.set_screen_size(12, 80);

    for _ in 0..6 {
        outcome
            .core_bridge
            .dispatch_key("\u{6}")
            .expect("Ctrl+F dispatch");
    }
    for _ in 0..20 {
        outcome
            .core_bridge
            .dispatch_key("\u{2}")
            .expect("Ctrl+B dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport_store.sync_from_windows_for_render(
            &snapshot.windows,
            &std::collections::BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::Core,
        );
        let window = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .expect("active window should exist after Ctrl+B");
        if window.topline == 1 {
            break;
        }
    }

    let before_snapshot = outcome.core_bridge.snapshot();
    let before_window = before_snapshot
        .windows
        .iter()
        .find(|window| window.is_active)
        .or_else(|| before_snapshot.windows.first())
        .expect("active window should exist on first page");
    assert_eq!(
        before_window.topline, 1,
        "test setup should reach the first page: topline={}, botline={}, cursor_row={}",
        before_window.topline, before_window.botline, before_window.cursor_row
    );

    let mut observed = Vec::new();
    for _ in 0..11 {
        outcome.core_bridge.dispatch_key("j").expect("j dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport_store.sync_from_windows_for_render(
            &snapshot.windows,
            &std::collections::BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );
        let window = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .expect("active window should exist after j");
        let viewport_top = viewport_store
            .get(window.id)
            .expect("viewport should be synced after j")
            .top_line();
        observed.push((
            window.topline,
            window.cursor_row,
            viewport_top,
            window.cursor_row.saturating_sub(viewport_top),
        ));
    }

    assert_eq!(
        observed,
        vec![
            (2, 1, 1, 0),
            (3, 2, 2, 0),
            (4, 3, 3, 0),
            (5, 4, 4, 0),
            (6, 5, 5, 0),
            (7, 6, 6, 0),
            (8, 7, 7, 0),
            (9, 8, 8, 0),
            (10, 9, 9, 0),
            (11, 10, 10, 0),
            (12, 11, 11, 0),
        ],
        "j after Ctrl+B reaches the first page should scroll the viewport down one line per key while preserving the cursor screen row"
    );
}

#[test]
fn ctrl_b_to_first_page_then_repeated_j_should_scroll_viewport_one_line_per_key() {
    let _lock = test_lock();
    let content = (1..=120)
        .map(|line| format!("line{line}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut outcome = launch_with_content(&content);
    let mut viewport_store = WindowViewportStore::new();
    let mut line_counts = std::collections::BTreeMap::new();
    line_counts.insert(1, content.lines().count());

    outcome.core_bridge.set_screen_size(12, 80);

    for _ in 0..6 {
        outcome
            .core_bridge
            .dispatch_key("\u{6}")
            .expect("Ctrl+F dispatch");
    }
    for _ in 0..20 {
        outcome
            .core_bridge
            .dispatch_key("\u{2}")
            .expect("Ctrl+B dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport_store.sync_from_windows_for_render(
            &snapshot.windows,
            &std::collections::BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::Core,
        );
        let window = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .expect("active window should exist after Ctrl+B");
        if window.topline == 1 {
            break;
        }
    }

    let before_snapshot = outcome.core_bridge.snapshot();
    let before_window = before_snapshot
        .windows
        .iter()
        .find(|window| window.is_active)
        .or_else(|| before_snapshot.windows.first())
        .expect("active window should exist on first page");
    let before_viewport_top = viewport_store
        .get(before_window.id)
        .expect("viewport should be synced")
        .top_line();
    let expected_relative = before_window.cursor_row.saturating_sub(before_viewport_top);
    assert_eq!(
        before_window.topline, 1,
        "test setup should reach the first page: topline={}, botline={}, cursor_row={}",
        before_window.topline, before_window.botline, before_window.cursor_row
    );

    for step in 1..=3 {
        outcome.core_bridge.dispatch_key("j").expect("j dispatch");
        let snapshot = outcome.core_bridge.snapshot();
        viewport_store.sync_from_windows_for_render(
            &snapshot.windows,
            &std::collections::BTreeSet::new(),
            &line_counts,
            ViewportSyncMode::SmoothLineMotion,
        );
        let window = snapshot
            .windows
            .iter()
            .find(|window| window.is_active)
            .or_else(|| snapshot.windows.first())
            .expect("active window should exist after j");
        let viewport_top = viewport_store
            .get(window.id)
            .expect("viewport should be synced after j")
            .top_line();
        assert_eq!(
            viewport_top,
            before_viewport_top.saturating_add(step),
            "j after Ctrl+B reaches the first page should scroll the viewport down by one line per key: step={step}, before_top={before_viewport_top}, actual_top={viewport_top}, cursor_row={}",
            window.cursor_row
        );
        assert_eq!(
            window.cursor_row.saturating_sub(viewport_top),
            expected_relative,
            "j after Ctrl+B reaches the first page should preserve cursor screen position while scrolling: step={step}, expected_relative={expected_relative}, actual_relative={}, cursor_row={}, viewport_top={viewport_top}",
            window.cursor_row.saturating_sub(viewport_top),
            window.cursor_row
        );
    }
}

// host-integration: visual selection projection for rendering is host-side
// coverage.
#[test]
fn visual_selection_is_projected_for_rendering() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("alpha beta gamma\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    outcome.core_bridge.dispatch_key("v").expect("v dispatch");
    outcome.core_bridge.dispatch_key("l").expect("l dispatch");
    outcome.core_bridge.dispatch_key("l").expect("l dispatch");

    let snapshot = outcome.core_bridge.snapshot();
    let visual_selection = outcome.core_bridge.current_visual_selection();
    let model = project(
        &ProjectionInput::new(&snapshot, &session_state, None)
            .with_visual_selection(visual_selection.as_ref()),
    );

    assert!(
        visual_selection.is_some(),
        "host smoke should confirm that a core-owned visual selection can be handed off"
    );
    assert_eq!(model.mode_label, "VISUAL");
    assert!(
        model.visual_selection.is_some(),
        "visual selection should be projected once the host receives it"
    );
}

// ---- 9.2.3: テキスト入力が ScreenModel の行データに反映される ----

/// インサートモードで入力した文字が ScreenModel の lines に反映される。
// host-integration: basic input-to-screen-model smoke coverage.
#[test]
fn text_input_reflected_in_screen_model_lines() {
    let _lock = test_lock();
    let mut outcome = launch_empty();
    let session_state = EditorSessionState::new(None);

    // i でインサート → "Hi" を入力 → Esc でノーマル復帰
    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    outcome.core_bridge.dispatch_key("H").expect("H dispatch");
    outcome.core_bridge.dispatch_key("i").expect("i input");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");

    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert!(
        model.lines.iter().any(|line| line.contains("Hi")),
        "入力した 'Hi' が行データに含まれること: {:?}",
        model.lines
    );
}

#[test]
fn insert_mode_uppercase_sequence_does_not_duplicate_previous_character() {
    let _lock = test_lock();
    let mut outcome = launch_empty();
    let session_state = EditorSessionState::new(None);

    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    for key in ["A", "G", "E", "N", "T", "S"] {
        outcome
            .core_bridge
            .dispatch_key(key)
            .expect("insert edit key should dispatch");
    }
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");

    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert!(
        model.lines.iter().any(|line| line == "AGENTS"),
        "one terminal key event per uppercase character should insert exactly once: {:?}",
        model.lines
    );
}

#[test]
fn insert_mode_backspace_and_ctrl_h_delete_previous_character() {
    let _lock = test_lock();
    let mut outcome = launch_empty();
    let session_state = EditorSessionState::new(None);

    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    for key in ["A", "G", "E", "N", "T", "S", "\x08", "S", "\x08"] {
        outcome
            .core_bridge
            .dispatch_key(key)
            .expect("insert edit key should dispatch");
    }
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");

    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert!(
        model.lines.iter().any(|line| line == "AGENT"),
        "Backspace and Ctrl-H-compatible BS should delete the previous inserted character: {:?}",
        model.lines
    );
}

// host-integration: tab-size driven projection is an application-layer
// presentation concern.
#[test]
fn tab_size_setting_changes_screen_projection_for_tabs() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("\ta\n");
    let session_state = EditorSessionState::new_with_tab_size(outcome.target_path.clone(), 4);

    outcome.core_bridge.dispatch_key("l").expect("l dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

    assert_eq!(model.lines[0], "    a");
    assert_eq!(model.cursor_col, 4);
}

// host-integration: dirty projection is part of the application state the UI
// renders.
// ---- 9.2.5: dirty 状態が編集結果に追随する ----

/// 編集操作を通じて dirty 状態が ScreenModel に正しく追随する。
#[test]
fn dirty_state_follows_editing_in_screen_model() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("initial\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    // 起動直後は clean
    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));
    assert!(!model.dirty, "起動直後は dirty=false");

    // 文字入力で dirty になる
    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    outcome.core_bridge.dispatch_key("X").expect("X input");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");

    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));
    assert!(model.dirty, "編集後は dirty=true");
}

/// 削除操作でも dirty 状態になる。
// host-integration: dirty projection remains the application concern even for
// delete-driven edits.
#[test]
fn dirty_state_set_after_delete_operation() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("hello\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());

    assert!(!outcome.initial_snapshot.dirty, "起動直後は clean");

    // x で文字削除
    outcome.core_bridge.dispatch_key("x").expect("x dispatch");
    let snapshot = outcome.core_bridge.snapshot();
    let model = project(&ProjectionInput::new(&snapshot, &session_state, None));
    assert!(model.dirty, "削除操作後は dirty=true");
}

// host-integration: representative end-to-end editing flow smoke coverage.
// ---- 9.2.6: モード遷移 → 移動 → 入力 → 削除の一連フロー ----

/// 完全な編集フロー: モード遷移、移動、入力、削除が連続して
/// 正しく反映されることを確認する。
#[test]
fn full_editing_flow_mode_move_insert_delete() {
    let _lock = test_lock();
    let mut outcome = launch_with_content("line1\nline2\nline3\n");
    let session_state = EditorSessionState::new(outcome.target_path.clone());
    let initial_model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));

    // Step 1: 起動直後の projection は clean
    assert!(!initial_model.dirty);
    assert!(!initial_model.lines.is_empty());

    // Step 2: j で別行へ移動してから編集する
    outcome.core_bridge.dispatch_key("j").expect("j dispatch");

    // Step 3: i で編集開始 → テキスト入力
    outcome.core_bridge.dispatch_key("i").expect("i dispatch");
    outcome.core_bridge.dispatch_key("X").expect("X input");
    outcome.core_bridge.dispatch_key("Y").expect("Y input");
    outcome
        .core_bridge
        .dispatch_key("\x1b")
        .expect("Esc dispatch");

    // Step 4: ScreenModel に編集後の状態が反映されることを確認
    let edited_snapshot = outcome.core_bridge.snapshot();
    let edited_model = project(&ProjectionInput::new(
        &edited_snapshot,
        &session_state,
        None,
    ));
    assert!(edited_model.dirty);
    assert!(
        edited_model.lines != initial_model.lines,
        "integrated edit flow should change the projected lines: initial={:?}, edited={:?}",
        initial_model.lines,
        edited_model.lines
    );

    // Step 5: dd で行削除
    outcome.core_bridge.dispatch_key("dd").expect("dd dispatch");
    let final_snapshot = outcome.core_bridge.snapshot();
    let final_model = project(&ProjectionInput::new(&final_snapshot, &session_state, None));
    assert!(final_model.dirty);
    assert!(
        final_model.lines != edited_model.lines,
        "delete step should trigger another projected update: edited={:?}, final={:?}",
        edited_model.lines,
        final_model.lines
    );

    // 削除後の行データの検証
    eprintln!(
        "[integ-test] host smoke projection changed across edit flow: initial={:?} edited={:?} final={:?}",
        initial_model.lines, edited_model.lines, final_model.lines
    );
}
