//! 継ぎ目（テスト監査 高深刻度 #2）テスト: focused なフロート/補完/パネルへ
//! キーを振り分ける **ラッパ** を実マネージャで駆動する統合テスト。
//!
//! leaf マネージャ（`FloatingWindowManager` / `CompletionFloatManager` /
//! `PanelManager`）の単体テストは緑だが、
//!
//! - `dispatch_floating_window_key`（`app::runtime_dispatch` ラッパ）
//! - `handle_completion_float_key`（`dispatch_completion_float_key` が呼ぶ leaf 接続関数）
//! - `begin_command_line_from_focused_panel` / `handle_terminal_panel_key`（パネル経路）
//! - `focused_input_target_for_key`（main.rs の振り分け判断を抽出した純粋関数）
//!
//! という **継ぎ目** にテストが無かった。継ぎ目が切れると「フロート/補完/
//! パネルは開くがキーが一切効かない（Esc で閉じない・Enter で確定しない・
//! j/k で動かない）」回帰を全件すり抜ける。本ファイルは leaf を直叩きせず、
//! 必ずラッパ経由で実マネージャを駆動し、`handled` / `need_redraw` /
//! `workspace_projection_dirty` 等のフラグ遷移まで検証する。

mod support;

use std::sync::MutexGuard;

use saya::app::runtime_dispatch::dispatch_floating_window_key;
use saya::core::bridge::CoreBridge;
use saya::features::completion::float::{
    CompletionCandidate, CompletionFloatInputOutcome, CompletionFloatManager,
    CompletionMenuFloatRequest,
};
use saya::features::completion::session::CompletionKeyBindingsRequest;
use saya::input::router::KeyInput;
use saya::presentation::floating_input::{
    FloatingWindowKeyHandling, FocusedInputTarget, begin_command_line_from_focused_panel,
    focused_input_target_for_key, handle_completion_float_key, handle_floating_window_key,
    handle_terminal_panel_key,
};
use saya::presentation::floating_window::{
    FloatingChrome, FloatingPlacement, FloatingSize, FloatingWindowManager, FloatingZIndex,
    WorkspaceFocus,
};
use saya::presentation::panel::{
    PanelCloseBehavior, PanelContent, PanelManager, PanelOpenRequest, PanelPosition, PanelSize,
};
use saya::terminal::float::TerminalFloatManager;
use support::session::launch_serial_lock;
use vim_core_rs::CoreMode;

// ============================================================================
// ヘルパー
// ============================================================================

fn open_focused_static_lines(
    manager: &mut FloatingWindowManager,
) -> saya::presentation::floating_window::FloatingWindowId {
    let id = manager.open_static_lines(
        vec![
            "line-1".to_string(),
            "line-2".to_string(),
            "line-3".to_string(),
            "line-4".to_string(),
        ],
        FloatingPlacement::editor_at(0, 0),
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    assert!(
        manager.focus_float(id),
        "static-lines float must take focus"
    );
    id
}

fn completion_request() -> CompletionMenuFloatRequest {
    CompletionMenuFloatRequest {
        window_id: 7,
        cursor_row: 3,
        cursor_col: 5,
        candidates: vec![
            CompletionCandidate {
                label: "alpha".to_string(),
                insert_text: None,
                detail: None,
                kind: None,
                documentation: vec!["first".to_string()],
                source: None,
                metadata: None,
                replace_range: None,
            },
            CompletionCandidate {
                label: "beta".to_string(),
                insert_text: None,
                detail: None,
                kind: None,
                documentation: vec!["second".to_string()],
                source: None,
                metadata: None,
                replace_range: None,
            },
        ],
        selected_index: 0,
        max_visible_items: 4,
        documentation_max_width: 40,
        documentation_max_height: 5,
        keys: Some(default_completion_keys()),
    }
}

/// CoreBridge は単一のグローバルコアセッションを掴むため、`CoreBridge::new`
/// を使うテストは launch_serial_lock で直列化する（並列実行で
/// `SessionAlreadyActive` を避けるため）。
fn core_test_lock() -> MutexGuard<'static, ()> {
    launch_serial_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn default_completion_keys() -> CompletionKeyBindingsRequest {
    CompletionKeyBindingsRequest {
        confirm: Some(vec!["<Enter>".to_string(), "<Tab>".to_string()]),
        close: Some(vec!["<C-e>".to_string()]),
        next: Some(vec!["<Down>".to_string(), "<C-n>".to_string()]),
        previous: Some(vec!["<Up>".to_string(), "<C-p>".to_string()]),
        page_next: Some(vec!["<PageDown>".to_string()]),
        page_previous: Some(vec!["<PageUp>".to_string()]),
    }
}

// ============================================================================
// 1. dispatch_floating_window_key（実ラッパ・sync）
// ----------------------------------------------------------------------------
// main.rs:665-679 のブロックが呼ぶ実ラッパ。leaf
// `handle_focused_static_lines_key_with_restore` の outcome を
// `FloatingWindowKeyHandling` へ写し、handled/need_redraw/
// workspace_projection_dirty を遷移させる継ぎ目。
// ============================================================================

#[test]
fn dispatch_floating_window_key_consumes_scroll_on_focused_static_lines_float() {
    let mut manager = FloatingWindowManager::default();
    let _id = open_focused_static_lines(&mut manager);

    // Down / j / k はメニュー（スクロール）を動かして Consumed になる。
    for key in [KeyInput::Down, KeyInput::Char('j'), KeyInput::Char('k')] {
        let mut handled = false;
        let mut need_redraw = false;
        let mut workspace_projection_dirty = false;
        dispatch_floating_window_key(
            &key,
            7,
            &mut manager,
            &mut handled,
            &mut need_redraw,
            &mut workspace_projection_dirty,
        );
        assert!(
            handled,
            "focused static-lines scroll key must be handled: {key:?}"
        );
        assert!(need_redraw, "scroll must request redraw: {key:?}");
        assert!(
            workspace_projection_dirty,
            "scroll must mark workspace projection dirty: {key:?}"
        );
    }
}

#[test]
fn dispatch_floating_window_key_closes_focused_static_lines_float_on_escape() {
    let mut manager = FloatingWindowManager::default();
    let _id = open_focused_static_lines(&mut manager);

    let mut handled = false;
    let mut need_redraw = false;
    let mut workspace_projection_dirty = false;

    dispatch_floating_window_key(
        &KeyInput::Escape,
        7,
        &mut manager,
        &mut handled,
        &mut need_redraw,
        &mut workspace_projection_dirty,
    );

    assert!(
        handled,
        "Esc on focused float must be handled by the wrapper"
    );
    assert!(need_redraw, "closing a float must request redraw");
    assert!(
        workspace_projection_dirty,
        "closing a float must dirty projection"
    );
    assert!(
        manager.resolve_screen_models(80, 24, &[], None).is_empty(),
        "Esc must actually close the focused float through the wrapper"
    );
    assert_eq!(
        manager.focus(),
        Some(WorkspaceFocus::Pane { window_id: 7 }),
        "closing must restore pane focus to the provided active window id"
    );
}

#[test]
fn dispatch_floating_window_key_is_noop_without_focused_float() {
    let mut manager = FloatingWindowManager::default();
    // フォーカス無し: 何もせず handled は false のまま。
    let mut handled = false;
    let mut need_redraw = false;
    let mut workspace_projection_dirty = false;

    dispatch_floating_window_key(
        &KeyInput::Char('j'),
        0,
        &mut manager,
        &mut handled,
        &mut need_redraw,
        &mut workspace_projection_dirty,
    );

    assert!(!handled, "no focused float must leave the key unhandled");
    assert!(!need_redraw);
    assert!(!workspace_projection_dirty);
}

// `handle_floating_window_key`（dispatch_floating_window_key が内部で呼ぶ
// presentation 層のラッパ）も直接駆動して outcome マッピングを固定する。
#[test]
fn handle_floating_window_key_maps_leaf_outcomes_to_handling_enum() {
    let mut manager = FloatingWindowManager::default();
    let id = open_focused_static_lines(&mut manager);

    assert_eq!(
        handle_floating_window_key(&mut manager, &KeyInput::Down, 7),
        Some(FloatingWindowKeyHandling::Consumed),
        "scroll must map to Consumed"
    );
    assert_eq!(
        handle_floating_window_key(&mut manager, &KeyInput::Escape, 7),
        Some(FloatingWindowKeyHandling::Closed { id }),
        "Esc must map to Closed with the float id"
    );
    assert_eq!(
        handle_floating_window_key(&mut manager, &KeyInput::Down, 7),
        None,
        "after close, no focused float -> Ignored maps to None"
    );
}

// ============================================================================
// 2. handle_completion_float_key（dispatch_completion_float_key が呼ぶ実関数）
// ----------------------------------------------------------------------------
// 重い async ラッパ dispatch_completion_float_key の中核は、この leaf 接続
// 関数（CoreBridge を取り、CompletionFloatInputOutcome を
// FloatingWindowKeyHandling へ写す）。ここを実マネージャ + 実 CoreBridge で
// 駆動して、選択(Consumed)・確定(Closed)・閉じる(Closed) の継ぎ目を検証する。
// ============================================================================

#[test]
fn handle_completion_float_key_selects_with_down_through_wrapper() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    let _lock = core_test_lock();
    let mut bridge = CoreBridge::new("").expect("core bridge");
    completion
        .open_menu(&mut floats, completion_request())
        .expect("completion menu should open");
    assert!(
        completion.has_active_menu(),
        "menu must be active after open"
    );

    let effect = handle_completion_float_key(
        &mut completion,
        &mut floats,
        &mut bridge,
        &KeyInput::Down,
        7,
    );
    assert_eq!(
        effect,
        Some(FloatingWindowKeyHandling::Consumed),
        "Down on focused completion menu must be consumed by the wrapper"
    );
}

#[test]
fn handle_completion_float_key_accepts_with_enter_and_closes_menu() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    let _lock = core_test_lock();
    let mut bridge = CoreBridge::new("").expect("core bridge");
    completion
        .open_menu(&mut floats, completion_request())
        .expect("completion menu should open");

    let effect = handle_completion_float_key(
        &mut completion,
        &mut floats,
        &mut bridge,
        &KeyInput::Enter,
        7,
    );
    assert!(
        matches!(effect, Some(FloatingWindowKeyHandling::Closed { .. })),
        "Enter must accept the candidate and close the menu through the wrapper, got {effect:?}"
    );
    assert!(
        !completion.has_active_menu(),
        "accepting a candidate must clear the active menu"
    );
}

#[test]
fn handle_completion_float_key_closes_with_escape() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    let _lock = core_test_lock();
    let mut bridge = CoreBridge::new("").expect("core bridge");
    completion
        .open_menu(&mut floats, completion_request())
        .expect("completion menu should open");

    let effect = handle_completion_float_key(
        &mut completion,
        &mut floats,
        &mut bridge,
        &KeyInput::Escape,
        7,
    );
    assert!(
        matches!(effect, Some(FloatingWindowKeyHandling::Closed { .. })),
        "Esc must close the completion menu through the wrapper, got {effect:?}"
    );
    assert!(
        !completion.has_active_menu(),
        "closing the menu must clear the active menu"
    );
}

// leaf `handle_key` がラッパ経由で確かに駆動されていることを、別系統
// （直接 outcome）でも固定する: 同じキーで Selected/Accepted が返る。
#[test]
fn completion_leaf_handle_key_reports_expected_outcomes() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    completion
        .open_menu(&mut floats, completion_request())
        .expect("completion menu should open");

    assert!(matches!(
        completion.handle_key(&mut floats, &KeyInput::Down, Some(7)),
        CompletionFloatInputOutcome::Selected { .. }
    ));
    assert!(matches!(
        completion.handle_key(&mut floats, &KeyInput::Enter, Some(7)),
        CompletionFloatInputOutcome::Accepted { .. }
    ));
}

// ============================================================================
// 3. パネル経路（begin_command_line_from_focused_panel / handle_terminal_panel_key）
// ----------------------------------------------------------------------------
// dispatch_floating_ui_key の最初の 2 ブロックが呼ぶ実関数。focused な端末
// パネルに対し `:` / `/` が command-line 入口へ昇格し、Ctrl-w で unfocus
// されることを検証する（write_key は live PTY を要するため対象外）。
// ============================================================================

fn open_focused_terminal_panel() -> PanelManager {
    let mut manager = PanelManager::default();
    manager.open(PanelOpenRequest {
        id: "agent".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Cells(30),
        content: PanelContent::Terminal {
            terminal_id: 42,
            close_behavior: PanelCloseBehavior::Detach,
        },
        focus: true,
    });
    assert_eq!(
        manager.focused_terminal_id(),
        Some(42),
        "panel terminal must be focused for the seam test"
    );
    manager
}

#[test]
fn begin_command_line_from_focused_panel_promotes_colon_and_slash() {
    let mut manager = open_focused_terminal_panel();
    assert_eq!(
        begin_command_line_from_focused_panel(&mut manager, &KeyInput::Char(':'), CoreMode::Normal),
        Some(':'),
        "`:` on a focused terminal panel must yield the ex command-line prompt"
    );
    assert_eq!(
        manager.focused_terminal_id(),
        None,
        "promoting to command-line must unfocus the panel"
    );

    let mut manager = open_focused_terminal_panel();
    assert_eq!(
        begin_command_line_from_focused_panel(&mut manager, &KeyInput::Char('/'), CoreMode::Normal),
        Some('/'),
        "`/` on a focused terminal panel must yield the search command-line prompt"
    );
}

#[test]
fn begin_command_line_from_focused_panel_ignores_non_command_keys() {
    let mut manager = open_focused_terminal_panel();
    assert_eq!(
        begin_command_line_from_focused_panel(&mut manager, &KeyInput::Char('a'), CoreMode::Normal),
        None,
        "non `:`/`/` keys must not be promoted to command-line"
    );
    // Insert モードでは昇格しない。
    assert_eq!(
        begin_command_line_from_focused_panel(&mut manager, &KeyInput::Char(':'), CoreMode::Insert),
        None,
        "`:` outside Normal mode must not be promoted"
    );
}

#[test]
fn handle_terminal_panel_key_unfocuses_panel_on_ctrl_w() {
    let mut manager = open_focused_terminal_panel();
    let mut terminals = TerminalFloatManager::default();

    let effect = handle_terminal_panel_key(&mut manager, &mut terminals, &KeyInput::Ctrl('w'));
    assert_eq!(
        effect,
        Some(FloatingWindowKeyHandling::Consumed),
        "Ctrl-w on a focused terminal panel must be consumed by the wrapper"
    );
    assert_eq!(
        manager.focused_terminal_id(),
        None,
        "Ctrl-w must unfocus the terminal panel"
    );
}

#[test]
fn handle_terminal_panel_key_is_noop_without_focused_panel() {
    let mut manager = PanelManager::default();
    let mut terminals = TerminalFloatManager::default();
    assert_eq!(
        handle_terminal_panel_key(&mut manager, &mut terminals, &KeyInput::Char('x')),
        None,
        "without a focused terminal panel the wrapper must not consume the key"
    );
}

// ============================================================================
// 4. ルーティング判断の純粋関数（focused_input_target_for_key）を、実マネージャ
//    の focus 述語から駆動して、main.rs の配線と一致することを固定する。
// ----------------------------------------------------------------------------
// floating_input.rs 内の単体テストはブール直入力だが、ここでは実マネージャの
// 公開述語（focused_static_lines_id / has_active_menu / focused_terminal_id 等）
// を実際に通すことで、main.rs が渡している引数と同じ経路を検証する。
// ============================================================================

#[test]
fn routing_classifier_matches_real_floating_manager_focus_state() {
    let mut manager = FloatingWindowManager::default();
    let _id = open_focused_static_lines(&mut manager);

    let target = focused_input_target_for_key(
        &KeyInput::Char('j'),
        CoreMode::Normal,
        false,
        manager.focused_terminal_id().is_some(),
        manager.focused_core_window_id().is_some(),
        false,
        manager.focused_static_lines_id().is_some(),
    );
    assert_eq!(
        target,
        FocusedInputTarget::FloatingWindow,
        "a real focused static-lines float must classify to FloatingWindow"
    );
}

#[test]
fn routing_classifier_matches_real_completion_and_panel_focus_state() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    completion
        .open_menu(&mut floats, completion_request())
        .expect("completion menu should open");
    let panel = open_focused_terminal_panel();

    // パネルが最優先（command-line 昇格対象外のキー）。
    let target = focused_input_target_for_key(
        &KeyInput::Char('x'),
        CoreMode::Normal,
        panel.focused_terminal_id().is_some(),
        false,
        false,
        completion.has_active_menu(),
        floats.focused_static_lines_id().is_some(),
    );
    assert_eq!(target, FocusedInputTarget::TerminalPanel);

    // パネルを除けば completion メニューが対象。
    let target = focused_input_target_for_key(
        &KeyInput::Down,
        CoreMode::Insert,
        false,
        false,
        false,
        completion.has_active_menu(),
        floats.focused_static_lines_id().is_some(),
    );
    assert_eq!(target, FocusedInputTarget::CompletionFloat);
}
