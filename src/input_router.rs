//! キー入力を editor intent へ変換するモジュール。
//!
//! terminal のキーイベントを EditorIntent に正規化することで、
//! 後続の event loop が入力元に依存しない設計を実現する。

/// 入力元に依存しないキー入力の抽象表現。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationKey {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// 入力元に依存しないキー入力の抽象表現。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyInput {
    /// 印字可能な文字入力
    Char(char),
    /// Ctrl + 文字の組み合わせ
    Ctrl(char),
    /// Tab キー
    Tab,
    /// Shift+Tab キー
    BackTab,
    /// 左矢印キー
    Left,
    /// 右矢印キー
    Right,
    /// 上矢印キー
    Up,
    /// 下矢印キー
    Down,
    /// Home キー
    Home,
    /// End キー
    End,
    /// PageUp キー
    PageUp,
    /// PageDown キー
    PageDown,
    /// Delete キー
    Delete,
    /// Insert キー
    Insert,
    /// Escape キー
    Escape,
    /// Enter キー
    Enter,
    /// Backspace キー
    Backspace,
    /// F1-F12 ファンクションキー
    F(u8),
    /// Alt + 文字の組み合わせ
    Alt(char),
    /// Shift + navigation key
    ShiftedNav(NavigationKey),
    /// Ctrl + navigation key
    CtrlNav(NavigationKey),
}

/// エディタが処理すべき意図の分類。
///
/// InputRouter は raw なキー入力をこの型に変換し、
/// application 層がモードに応じた処理を行えるようにする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorIntent {
    /// vim-core-rs へ直接渡す編集キー入力（モード遷移、移動、入力、削除含む）
    EditKey(String),
    /// 保存要求（:w 相当）
    Save,
    /// 通常終了要求（:q 相当）
    Quit { force: bool },
}

/// キー入力を EditorIntent へ変換する。
///
/// 現在のモードに関係なく、特殊なキーバインド（Ctrl+S で保存、
/// Ctrl+Q で終了）をアプリケーションコマンドとして切り出す。
/// それ以外のキー入力はすべて EditKey として vim-core-rs へ委譲する。
pub fn resolve_intent(key: &KeyInput) -> EditorIntent {
    log::debug!("[input_router] resolving intent for key: {:?}", key);
    let intent = match key {
        KeyInput::Ctrl('s') => EditorIntent::Save,
        KeyInput::Ctrl('q') => EditorIntent::Quit { force: false },
        KeyInput::Ctrl('Q') => EditorIntent::Quit { force: true },
        _ => {
            let key_str = key_input_to_vim_key(key);
            EditorIntent::EditKey(key_str)
        }
    };
    log::debug!("[input_router] resolved intent: {:?}", intent);
    intent
}

/// KeyInput を vim-core-rs が解釈可能なキー文字列に変換する。
fn key_input_to_vim_key(key: &KeyInput) -> String {
    match key {
        KeyInput::Char(ch) => ch.to_string(),
        KeyInput::Ctrl(ch) => {
            // Ctrl+文字は ASCII 制御コードに変換
            let ctrl_code = (*ch as u8) & 0x1f;
            String::from(ctrl_code as char)
        }
        KeyInput::Tab => "\t".to_string(),
        KeyInput::BackTab => "\x1b[Z".to_string(),
        KeyInput::Left => "\x1b[D".to_string(),
        KeyInput::Right => "\x1b[C".to_string(),
        KeyInput::Up => "\x1b[A".to_string(),
        KeyInput::Down => "\x1b[B".to_string(),
        KeyInput::Home => "\x1b[H".to_string(),
        KeyInput::End => "\x1b[F".to_string(),
        KeyInput::PageUp => "\x1b[5~".to_string(),
        KeyInput::PageDown => "\x1b[6~".to_string(),
        KeyInput::Delete => "\x1b[3~".to_string(),
        KeyInput::Insert => "\x1b[2~".to_string(),
        KeyInput::Escape => "\x1b".to_string(),
        KeyInput::Enter => "\r".to_string(),
        KeyInput::Backspace => "\x08".to_string(),
        KeyInput::F(number) => function_key_sequence(*number).to_string(),
        KeyInput::Alt(ch) => format!("\x1b{ch}"),
        KeyInput::ShiftedNav(nav) => modified_navigation_sequence(*nav, 2).to_string(),
        KeyInput::CtrlNav(nav) => modified_navigation_sequence(*nav, 5).to_string(),
    }
}

fn function_key_sequence(number: u8) -> &'static str {
    match number {
        1 => "\x1bOP",
        2 => "\x1bOQ",
        3 => "\x1bOR",
        4 => "\x1bOS",
        5 => "\x1b[15~",
        6 => "\x1b[17~",
        7 => "\x1b[18~",
        8 => "\x1b[19~",
        9 => "\x1b[20~",
        10 => "\x1b[21~",
        11 => "\x1b[23~",
        12 => "\x1b[24~",
        _ => "",
    }
}

fn modified_navigation_sequence(nav: NavigationKey, modifier: u8) -> &'static str {
    match (nav, modifier) {
        (NavigationKey::Up, 2) => "\x1b[1;2A",
        (NavigationKey::Down, 2) => "\x1b[1;2B",
        (NavigationKey::Right, 2) => "\x1b[1;2C",
        (NavigationKey::Left, 2) => "\x1b[1;2D",
        (NavigationKey::Home, 2) => "\x1b[1;2H",
        (NavigationKey::End, 2) => "\x1b[1;2F",
        (NavigationKey::PageUp, 2) => "\x1b[5;2~",
        (NavigationKey::PageDown, 2) => "\x1b[6;2~",
        (NavigationKey::Up, 5) => "\x1b[1;5A",
        (NavigationKey::Down, 5) => "\x1b[1;5B",
        (NavigationKey::Right, 5) => "\x1b[1;5C",
        (NavigationKey::Left, 5) => "\x1b[1;5D",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
