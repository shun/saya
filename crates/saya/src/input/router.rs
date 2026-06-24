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
    /// Shift+Enter キー
    ShiftEnter,
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

/// command-line（ex / search）入口に入るべきキーかを判定する純粋関数。
///
/// ADR 0006 回帰修正: `:` / `/` の command-line 入口は host の責務
/// （architecture.md: `src/input/` が command-line editing / ex-command
/// routing を持つ）。単一パイプラインに載せると `predict_input_completeness`
/// が「完成 builtin」と判定して backend へ越境し、host の command-line 入口が
/// バイパスされてコマンドモードに入れなくなる回帰が起きる。
///
/// 次の条件をすべて満たす「曖昧でない単打」のときに限り、パイプラインより前で
/// host 入口へ確定させる対象として `Some(prompt)` を返す。
///
/// - mode が Normal
/// - host 側に keymap prefix / count / passthrough のいずれも保留されていない
/// - key が `:`（ex）または `/`（search）の単打
///
/// 上記以外（pending 中・Insert モード・count 中・他キー）は `None` を返し、
/// 従来どおりパイプラインで解決させる（count や keymap prefix を落とさない）。
///
/// この判断を副作用のない関数として切り出すことで、本番イベントループ
/// （`main.rs`）と端末経路の統合テストが同一の判断ロジックを駆動でき、
/// 入力ルーティングの配線が leaf の再実装ではなく実コードで検証される。
pub fn command_line_entry_for_key(
    key: &KeyInput,
    mode: vim_core_rs::CoreMode,
    host_pending: &Option<String>,
    host_count: &Option<usize>,
    host_passthrough: &str,
) -> Option<char> {
    if mode != vim_core_rs::CoreMode::Normal {
        return None;
    }
    if host_pending.is_some() || host_count.is_some() || !host_passthrough.is_empty() {
        return None;
    }
    match key {
        KeyInput::Char(c @ (':' | '/')) => Some(*c),
        _ => None,
    }
}

/// `KeyInput` を vim-core-rs が dispatch / 完成度予測で解釈する「実キー文字列」へ
/// 変換する公開 API。
///
/// 重要（ADR 0006 回帰修正・A-1）:
/// keymap 照合に使う lhs 表記（`<C-f>` のような Vim notation, `startup_keymap_lhs_from_input`）
/// と、core が実際に解釈するキー文字列（`Ctrl-f` なら ASCII 制御コード `\u{6}`）は
/// **別物**である。`Char` キーは両者が一致するため見過ごされていたが、`Ctrl(_)` 系は
/// lhs 表記 `<C-f>` が core へ素通しされると core はこれを Ctrl-f と認識できず no-op に
/// なる（Ctrl-f/Ctrl-b/Ctrl-d/Ctrl-u のページ送り回帰）。
/// 単一入力パイプラインの `Passthrough` は lhs 表記ではなくこの実キー文字列を core へ
/// 渡さなければならない。
pub fn vim_key_for_input(key: &KeyInput) -> String {
    key_input_to_vim_key(key)
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
        KeyInput::ShiftEnter => "\x1b[13;2u".to_string(),
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
#[path = "router_test.rs"]
mod tests;
