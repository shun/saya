use super::*;

/// ADR 0006 回帰修正・A-1: Ctrl 系 notation が core の実キー文字列（ASCII 制御コード）
/// へ復元されること。これが壊れると Ctrl-f/b/d/u のページ送りが core で no-op になる。
#[test]
fn ctrl_notation_decodes_to_ascii_control_codes() {
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("<C-f>"), "\u{6}");
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("<C-b>"), "\u{2}");
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("<C-d>"), "\u{4}");
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("<C-u>"), "\u{15}");
}

/// Char のみの combined lhs（例 `gg`）は変換不変で従来挙動を壊さない。
#[test]
fn plain_chars_pass_through_unchanged() {
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("gg"), "gg");
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("d"), "d");
    assert_eq!(startup_keymap_lhs_notation_to_core_keys(""), "");
}

/// prefix + Ctrl の連結（例 `g<C-f>`）も各トークンが正しく復元される。
#[test]
fn mixed_prefix_and_ctrl_decodes_each_token() {
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("g<C-f>"), "g\u{6}");
}

/// 特殊キー notation（Tab/Enter/Esc/BS/S-Tab）も実キー列へ復元される。
#[test]
fn special_key_notation_decodes() {
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("<Tab>"), "\t");
    assert_eq!(
        startup_keymap_lhs_notation_to_core_keys("<S-Tab>"),
        "\x1b[Z"
    );
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("<Enter>"), "\r");
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("<Esc>"), "\x1b");
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("<BS>"), "\x08");
}

/// 未知の notation・閉じない `<` は情報を捨てずに原文を維持する。
#[test]
fn unknown_or_unclosed_notation_is_preserved() {
    assert_eq!(
        startup_keymap_lhs_notation_to_core_keys("<Unknown>"),
        "<Unknown>"
    );
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("<C-"), "<C-");
    assert_eq!(startup_keymap_lhs_notation_to_core_keys("a<b"), "a<b");
}
