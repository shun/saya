//! `focused_input_target_for_key` の純粋分類テスト。
//!
//! 巨大イベントループの `if !handled { ... }` 連鎖と同じ優先順位を表現する
//! ことを保証し、「フロート/補完/パネルは開くがキーが効かない」回帰
//! （振り分け順序の断線）を構造的に検出する。

use super::{FocusedInputTarget, focused_input_target_for_key};
use crate::input::router::KeyInput;
use vim_core_rs::CoreMode;

fn classify(
    key: &KeyInput,
    mode: CoreMode,
    panel_terminal: bool,
    float_terminal: bool,
    float_core_window: bool,
    completion: bool,
    float_static: bool,
) -> FocusedInputTarget {
    focused_input_target_for_key(
        key,
        mode,
        panel_terminal,
        float_terminal,
        float_core_window,
        completion,
        float_static,
    )
}

#[test]
fn no_focused_subsystem_routes_to_none() {
    assert_eq!(
        classify(
            &KeyInput::Char('j'),
            CoreMode::Normal,
            false,
            false,
            false,
            false,
            false,
        ),
        FocusedInputTarget::None
    );
}

#[test]
fn focused_completion_menu_routes_navigation_and_confirm_to_completion() {
    for key in [
        KeyInput::Down,
        KeyInput::Up,
        KeyInput::Enter,
        KeyInput::Escape,
    ] {
        assert_eq!(
            classify(&key, CoreMode::Insert, false, false, false, true, false),
            FocusedInputTarget::CompletionFloat,
            "completion menu must take navigation/confirm/close keys: {key:?}"
        );
    }
}

#[test]
fn focused_static_lines_float_routes_to_floating_window() {
    for key in [
        KeyInput::Char('j'),
        KeyInput::Char('k'),
        KeyInput::Down,
        KeyInput::Escape,
    ] {
        assert_eq!(
            classify(&key, CoreMode::Normal, false, false, false, false, true),
            FocusedInputTarget::FloatingWindow,
            "focused static-lines float must take scroll/close keys: {key:?}"
        );
    }
}

#[test]
fn focused_panel_terminal_promotes_colon_and_slash_to_command_line() {
    assert_eq!(
        classify(
            &KeyInput::Char(':'),
            CoreMode::Normal,
            true,
            false,
            false,
            false,
            false,
        ),
        FocusedInputTarget::PanelCommandLineEntry
    );
    assert_eq!(
        classify(
            &KeyInput::Char('/'),
            CoreMode::Normal,
            true,
            false,
            false,
            false,
            false,
        ),
        FocusedInputTarget::PanelCommandLineEntry
    );
}

#[test]
fn focused_panel_terminal_writes_other_keys_to_panel() {
    assert_eq!(
        classify(
            &KeyInput::Char('a'),
            CoreMode::Normal,
            true,
            false,
            false,
            false,
            false,
        ),
        FocusedInputTarget::TerminalPanel
    );
    // Insert モードの `:` は command-line 昇格対象外なのでパネルへ書き込む。
    assert_eq!(
        classify(
            &KeyInput::Char(':'),
            CoreMode::Insert,
            true,
            false,
            false,
            false,
            false,
        ),
        FocusedInputTarget::TerminalPanel
    );
}

#[test]
fn precedence_panel_then_terminal_float_then_core_window_then_completion_then_static() {
    // 全 focus 述語が true でも、優先順位の先頭（panel）が選ばれる。
    assert_eq!(
        classify(
            &KeyInput::Char('x'),
            CoreMode::Normal,
            true,
            true,
            true,
            true,
            true,
        ),
        FocusedInputTarget::TerminalPanel
    );
    // panel を外すと terminal float。
    assert_eq!(
        classify(
            &KeyInput::Char('x'),
            CoreMode::Normal,
            false,
            true,
            true,
            true,
            true,
        ),
        FocusedInputTarget::TerminalFloat
    );
    // terminal float を外すと core-window float。
    assert_eq!(
        classify(
            &KeyInput::Char('x'),
            CoreMode::Normal,
            false,
            false,
            true,
            true,
            true,
        ),
        FocusedInputTarget::CoreWindowFloat
    );
    // core-window float を外すと completion。
    assert_eq!(
        classify(
            &KeyInput::Char('x'),
            CoreMode::Normal,
            false,
            false,
            false,
            true,
            true,
        ),
        FocusedInputTarget::CompletionFloat
    );
    // completion を外すと static-lines float。
    assert_eq!(
        classify(
            &KeyInput::Char('x'),
            CoreMode::Normal,
            false,
            false,
            false,
            false,
            true,
        ),
        FocusedInputTarget::FloatingWindow
    );
}
