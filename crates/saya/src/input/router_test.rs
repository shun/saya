use super::*;

use vim_core_rs::CoreMode;

// ---- command_line_entry_for_key の純粋判定テスト ----

#[test]
fn command_line_entry_for_key_enters_ex_and_search_on_unambiguous_single_press() {
    // Normal モードで host に何も保留がない単打 `:` / `/` は入口へ確定する。
    assert_eq!(
        command_line_entry_for_key(&KeyInput::Char(':'), CoreMode::Normal, &None, &None, ""),
        Some(':')
    );
    assert_eq!(
        command_line_entry_for_key(&KeyInput::Char('/'), CoreMode::Normal, &None, &None, ""),
        Some('/')
    );
}

#[test]
fn command_line_entry_for_key_ignores_non_command_keys() {
    for key in [
        KeyInput::Char('a'),
        KeyInput::Char('j'),
        KeyInput::Char(';'),
        KeyInput::Escape,
        KeyInput::Enter,
    ] {
        assert_eq!(
            command_line_entry_for_key(&key, CoreMode::Normal, &None, &None, ""),
            None,
            "key {key:?} must not enter command-line"
        );
    }
}

#[test]
fn command_line_entry_for_key_does_not_intercept_when_host_has_pending() {
    // keymap prefix 保留中は横取りしない（prefix を落とさないため）。
    assert_eq!(
        command_line_entry_for_key(
            &KeyInput::Char(':'),
            CoreMode::Normal,
            &Some("g".to_string()),
            &None,
            "",
        ),
        None
    );
    // count 入力中も横取りしない。
    assert_eq!(
        command_line_entry_for_key(&KeyInput::Char('/'), CoreMode::Normal, &None, &Some(3), "",),
        None
    );
    // operator passthrough 中も横取りしない。
    assert_eq!(
        command_line_entry_for_key(&KeyInput::Char(':'), CoreMode::Normal, &None, &None, "d"),
        None
    );
}

#[test]
fn command_line_entry_for_key_only_in_normal_mode() {
    for mode in [CoreMode::Insert, CoreMode::Visual] {
        assert_eq!(
            command_line_entry_for_key(&KeyInput::Char(':'), mode, &None, &None, ""),
            None,
            "mode {mode:?} must not enter command-line on ':'"
        );
    }
}

// ---- タスク 4.5: キー入力の intent 変換テスト ----

#[test]
fn char_input_resolves_to_edit_key() {
    let intent = resolve_intent(&KeyInput::Char('i'));

    assert_eq!(
        intent,
        EditorIntent::EditKey("i".to_string()),
        "通常の文字入力は EditKey に変換されること"
    );
}

#[test]
fn hjkl_movement_resolves_to_edit_key() {
    for key in ['h', 'j', 'k', 'l'] {
        let intent = resolve_intent(&KeyInput::Char(key));
        assert_eq!(
            intent,
            EditorIntent::EditKey(key.to_string()),
            "{} キーは EditKey に変換されること",
            key
        );
    }
}

#[test]
fn escape_resolves_to_edit_key_with_escape_sequence() {
    let intent = resolve_intent(&KeyInput::Escape);

    assert_eq!(
        intent,
        EditorIntent::EditKey("\x1b".to_string()),
        "Escape は vim-core-rs の ESC 文字列として EditKey に変換されること"
    );
}

#[test]
fn enter_resolves_to_edit_key_with_cr() {
    let intent = resolve_intent(&KeyInput::Enter);

    assert_eq!(
        intent,
        EditorIntent::EditKey("\r".to_string()),
        "Enter は CR として EditKey に変換されること"
    );
}

#[test]
fn shift_enter_resolves_to_modified_enter_sequence() {
    let intent = resolve_intent(&KeyInput::ShiftEnter);

    assert_eq!(
        intent,
        EditorIntent::EditKey("\x1b[13;2u".to_string()),
        "Shift+Enter は modified Enter として EditKey に変換されること"
    );
}

#[test]
fn backspace_resolves_to_edit_key() {
    let intent = resolve_intent(&KeyInput::Backspace);

    assert_eq!(
        intent,
        EditorIntent::EditKey("\x08".to_string()),
        "Backspace は BS として EditKey に変換されること"
    );
}

#[test]
fn x_key_resolves_to_edit_key_for_normal_mode_delete() {
    let intent = resolve_intent(&KeyInput::Char('x'));

    assert_eq!(
        intent,
        EditorIntent::EditKey("x".to_string()),
        "x キー（削除操作）は EditKey として vim-core-rs に委譲されること"
    );
}

#[test]
fn printable_characters_resolve_to_edit_key() {
    for ch in ['a', 'Z', '0', ' ', '.', ':', '/'] {
        let intent = resolve_intent(&KeyInput::Char(ch));
        assert_eq!(
            intent,
            EditorIntent::EditKey(ch.to_string()),
            "印字可能文字 '{}' は EditKey に変換されること",
            ch
        );
    }
}

#[test]
fn tab_and_navigation_keys_resolve_to_vim_special_keys() {
    let cases = [
        (KeyInput::Tab, "\t"),
        (KeyInput::BackTab, "\x1b[Z"),
        (KeyInput::Left, "\x1b[D"),
        (KeyInput::Right, "\x1b[C"),
        (KeyInput::Up, "\x1b[A"),
        (KeyInput::Down, "\x1b[B"),
        (KeyInput::Home, "\x1b[H"),
        (KeyInput::End, "\x1b[F"),
        (KeyInput::PageUp, "\x1b[5~"),
        (KeyInput::PageDown, "\x1b[6~"),
        (KeyInput::Delete, "\x1b[3~"),
        (KeyInput::Insert, "\x1b[2~"),
    ];

    for (key, expected) in cases {
        let intent = resolve_intent(&key);
        assert_eq!(
            intent,
            EditorIntent::EditKey(expected.to_string()),
            "{key:?} は Vim 互換の special key として扱われること",
        );
    }
}

#[test]
fn function_keys_resolve_to_vim_ansi_sequences() {
    let cases = [
        (1, "\x1bOP"),
        (2, "\x1bOQ"),
        (3, "\x1bOR"),
        (4, "\x1bOS"),
        (5, "\x1b[15~"),
        (6, "\x1b[17~"),
        (7, "\x1b[18~"),
        (8, "\x1b[19~"),
        (9, "\x1b[20~"),
        (10, "\x1b[21~"),
        (11, "\x1b[23~"),
        (12, "\x1b[24~"),
    ];

    for (number, expected) in cases {
        let intent = resolve_intent(&KeyInput::F(number));
        assert_eq!(intent, EditorIntent::EditKey(expected.to_string()));
    }
}

#[test]
fn modified_navigation_resolves_to_vim_csi_modifier_sequences() {
    let cases = [
        (KeyInput::ShiftedNav(NavigationKey::Up), "\x1b[1;2A"),
        (KeyInput::ShiftedNav(NavigationKey::Down), "\x1b[1;2B"),
        (KeyInput::ShiftedNav(NavigationKey::Right), "\x1b[1;2C"),
        (KeyInput::ShiftedNav(NavigationKey::Left), "\x1b[1;2D"),
        (KeyInput::ShiftedNav(NavigationKey::Home), "\x1b[1;2H"),
        (KeyInput::ShiftedNav(NavigationKey::End), "\x1b[1;2F"),
        (KeyInput::ShiftedNav(NavigationKey::PageUp), "\x1b[5;2~"),
        (KeyInput::ShiftedNav(NavigationKey::PageDown), "\x1b[6;2~"),
        (KeyInput::CtrlNav(NavigationKey::Up), "\x1b[1;5A"),
        (KeyInput::CtrlNav(NavigationKey::Down), "\x1b[1;5B"),
        (KeyInput::CtrlNav(NavigationKey::Right), "\x1b[1;5C"),
        (KeyInput::CtrlNav(NavigationKey::Left), "\x1b[1;5D"),
    ];

    for (key, expected) in cases {
        let intent = resolve_intent(&key);
        assert_eq!(intent, EditorIntent::EditKey(expected.to_string()));
    }
}

#[test]
fn alt_character_resolves_to_escape_prefixed_edit_key() {
    let ascii = resolve_intent(&KeyInput::Alt('x'));
    let multibyte = resolve_intent(&KeyInput::Alt('あ'));

    assert_eq!(ascii, EditorIntent::EditKey("\x1bx".to_string()));
    assert_eq!(multibyte, EditorIntent::EditKey("\x1bあ".to_string()));
}

#[test]
fn intent_is_independent_of_input_source() {
    // 同じ KeyInput からは常に同じ EditorIntent が返ることを検証
    let key = KeyInput::Char('i');
    let first = resolve_intent(&key);
    let second = resolve_intent(&key);

    assert_eq!(
        first, second,
        "同一入力からは決定的に同じ intent が返ること"
    );
}

// ---- 保存・終了 intent テスト ----

#[test]
fn ctrl_s_resolves_to_save_intent() {
    let intent = resolve_intent(&KeyInput::Ctrl('s'));

    assert_eq!(
        intent,
        EditorIntent::Save,
        "Ctrl+S は Save intent に変換されること"
    );
}

#[test]
fn ctrl_q_resolves_to_quit_intent() {
    let intent = resolve_intent(&KeyInput::Ctrl('q'));

    assert_eq!(
        intent,
        EditorIntent::Quit { force: false },
        "Ctrl+Q は通常終了の Quit intent に変換されること"
    );
}

#[test]
fn ctrl_shift_q_resolves_to_force_quit_intent() {
    let intent = resolve_intent(&KeyInput::Ctrl('Q'));

    assert_eq!(
        intent,
        EditorIntent::Quit { force: true },
        "Ctrl+Shift+Q は強制終了の Quit intent に変換されること"
    );
}

#[test]
fn non_special_ctrl_key_resolves_to_edit_key() {
    let intent = resolve_intent(&KeyInput::Ctrl('a'));

    assert!(
        matches!(intent, EditorIntent::EditKey(_)),
        "特殊バインドでない Ctrl+A は EditKey に変換されること"
    );
}

#[test]
fn save_and_quit_intents_are_distinct_from_edit_keys() {
    let save = resolve_intent(&KeyInput::Ctrl('s'));
    let quit = resolve_intent(&KeyInput::Ctrl('q'));
    let edit = resolve_intent(&KeyInput::Char('s'));

    assert_ne!(
        save, edit,
        "Ctrl+S の Save と通常の 's' EditKey は区別されること"
    );
    assert_ne!(save, quit, "Save と Quit は異なる intent であること");
}
