//! 描画専用モデルと投影ロジック。
//!
//! CoreSnapshot と EditorSessionState から描画に必要な情報だけを
//! 抽出し、ScreenModel として TuiRenderer に渡す。
//! 描画側は ScreenModel だけを入力とし、CoreSnapshot に直接依存しない。

use unicode_width::UnicodeWidthChar;
use vim_core_rs::{CoreMode, CoreSnapshot};

use crate::core_bridge::VisualSelection;
use crate::editor_session::EditorSessionState;

/// 描画専用 view model。
///
/// TuiRenderer はこの型だけを入力とし、CoreSnapshot や
/// EditorSessionState を直接参照しない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenModel {
    /// 現在のファイル名（未設定なら "[新規]"）
    pub file_name: String,
    /// 現在のモードラベル（例: "NORMAL", "INSERT"）
    pub mode_label: String,
    /// バッファが変更済みかどうか
    pub dirty: bool,
    /// 表示用の行データ
    pub lines: Vec<String>,
    /// カーソル行（0-indexed）
    pub cursor_row: u16,
    /// カーソル列（0-indexed）
    pub cursor_col: u16,
    /// Visual mode の選択範囲（表示セル座標）
    pub visual_selection: Option<ScreenSelection>,
    /// メッセージ欄に表示する通知（エラーやガイダンス）
    pub message_line: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenSelection {
    pub start_row: u16,
    pub start_col: u16,
    pub end_row: u16,
    pub end_col_exclusive: u16,
}

/// 投影の入力をまとめた構造体。
///
/// CoreSnapshot と EditorSessionState から描画に必要な情報を選択して渡す。
pub struct ProjectionInput<'a> {
    pub snapshot: &'a CoreSnapshot,
    pub session_state: &'a EditorSessionState,
    pub visual_selection: Option<&'a VisualSelection>,
    pub transient_message: Option<&'a str>,
    pub viewport_top: usize,
    pub body_height: usize,
}

impl<'a> ProjectionInput<'a> {
    pub fn new(
        snapshot: &'a CoreSnapshot,
        session_state: &'a EditorSessionState,
        transient_message: Option<&'a str>,
    ) -> Self {
        Self {
            snapshot,
            session_state,
            visual_selection: None,
            transient_message,
            viewport_top: 0,
            body_height: usize::MAX,
        }
    }

    pub fn with_viewport(mut self, viewport_top: usize, body_height: usize) -> Self {
        self.viewport_top = viewport_top;
        self.body_height = body_height.max(1);
        self
    }

    pub fn with_visual_selection(mut self, visual_selection: Option<&'a VisualSelection>) -> Self {
        self.visual_selection = visual_selection;
        self
    }
}

/// CoreSnapshot と EditorSessionState から ScreenModel を生成する。
///
/// 描画側はこの関数の戻り値だけを使い、CoreSnapshot に直接依存しない。
pub fn project(input: &ProjectionInput<'_>) -> ScreenModel {
    log::debug!(
        "[screen_model] projecting: mode={:?}, dirty={}, cursor=({},{}), transient_message={:?}",
        input.snapshot.mode,
        input.snapshot.dirty,
        input.snapshot.cursor_row,
        input.snapshot.cursor_col,
        input.transient_message,
    );

    let file_name = resolve_file_name(input.snapshot, input.session_state);
    let mode_label = mode_to_label(input.snapshot.mode);
    let dirty = input.snapshot.dirty;
    let full_lines = apply_line_number_prefix(
        split_text_to_lines(&input.snapshot.text, input.session_state.tab_size()),
        input.session_state.line_numbers(),
        input.session_state.number_width(),
    );
    let lines = slice_visible_lines(&full_lines, input.viewport_top, input.body_height);
    let cursor_row = resolve_cursor_row(
        input.snapshot.cursor_row,
        input.viewport_top,
        input.body_height,
    );
    let cursor_col = resolve_cursor_col(
        &input.snapshot.text,
        input.snapshot.cursor_row,
        input.snapshot.cursor_col,
        input.session_state.tab_size(),
        input.session_state.line_numbers(),
        input.session_state.number_width(),
    );
    let visual_selection = resolve_visual_selection(input);
    let message_line = resolve_message_line(input.session_state, input.transient_message);

    log::debug!(
        "[screen_model] projected: file_name={:?}, mode_label={:?}, dirty={}, lines_count={}, cursor=({},{}), message_line={:?}",
        file_name,
        mode_label,
        dirty,
        lines.len(),
        cursor_row,
        cursor_col,
        message_line,
    );

    ScreenModel {
        file_name,
        mode_label,
        dirty,
        lines,
        cursor_row,
        cursor_col,
        visual_selection,
        message_line,
    }
}

fn resolve_visual_selection(input: &ProjectionInput<'_>) -> Option<ScreenSelection> {
    let selection = input.visual_selection?;
    let viewport_bottom = input
        .viewport_top
        .saturating_add(input.body_height.max(1))
        .saturating_sub(1);
    if selection.end_row < input.viewport_top || selection.start_row > viewport_bottom {
        return None;
    }

    let start_row = selection.start_row.max(input.viewport_top);
    let end_row = selection.end_row.min(viewport_bottom);
    let start_col = if start_row == selection.start_row {
        resolve_display_col_for_position(
            &input.snapshot.text,
            start_row,
            selection.start_col,
            input.session_state.tab_size(),
            input.session_state.line_numbers(),
            input.session_state.number_width(),
        )
    } else {
        line_number_offset(
            &input.snapshot.text,
            input.session_state.line_numbers(),
            input.session_state.number_width(),
        )
    };
    let end_col_exclusive = if end_row == selection.end_row {
        resolve_display_col_after_inclusive_position(
            &input.snapshot.text,
            end_row,
            selection.end_col,
            input.session_state.tab_size(),
            input.session_state.line_numbers(),
            input.session_state.number_width(),
        )
    } else {
        u16::MAX
    };

    Some(ScreenSelection {
        start_row: resolve_cursor_row(start_row, input.viewport_top, input.body_height),
        start_col,
        end_row: resolve_cursor_row(end_row, input.viewport_top, input.body_height),
        end_col_exclusive,
    })
}

fn slice_visible_lines(lines: &[String], viewport_top: usize, body_height: usize) -> Vec<String> {
    let body_height = body_height.max(1);
    let start = viewport_top.min(lines.len());
    let end = start.saturating_add(body_height).min(lines.len());
    let visible = lines[start..end].to_vec();

    log::debug!(
        "[screen_model] sliced visible lines: viewport_top={}, body_height={}, total_lines={}, visible_lines={}",
        viewport_top,
        body_height,
        lines.len(),
        visible.len()
    );

    visible
}

/// アクティブバッファのファイル名を解決する。
///
/// session_state に target_path がある場合はそのファイル名部分を使い、
/// ない場合はデフォルトの "[新規]" を返す。
fn resolve_file_name(_snapshot: &CoreSnapshot, session_state: &EditorSessionState) -> String {
    // session_state の target_path を唯一のソースとする。
    // snapshot のバッファ名はセッション再利用時に汚染されうるため参照しない。
    if let Some(path) = session_state.target_path() {
        log::debug!(
            "[screen_model] file name from session target path: {}",
            path.display()
        );
        return path.display().to_string();
    }

    log::debug!("[screen_model] file name defaulting to [新規]");
    "[新規]".to_string()
}

/// CoreMode をユーザー向けのラベル文字列に変換する。
fn mode_to_label(mode: CoreMode) -> String {
    let label = match mode {
        CoreMode::Normal => "NORMAL",
        CoreMode::Insert => "INSERT",
        CoreMode::Visual => "VISUAL",
        CoreMode::VisualLine => "V-LINE",
        CoreMode::VisualBlock => "V-BLOCK",
        CoreMode::Replace => "REPLACE",
        CoreMode::Select => "SELECT",
        CoreMode::SelectLine => "S-LINE",
        CoreMode::SelectBlock => "S-BLOCK",
        CoreMode::CommandLine => "COMMAND",
        CoreMode::OperatorPending => "OP PENDING",
    };
    log::debug!("[screen_model] mode {:?} -> label {:?}", mode, label);
    label.to_string()
}

/// テキストを行に分割する。
fn split_text_to_lines(text: &str, tab_size: u16) -> Vec<String> {
    let lines: Vec<String> = text
        .lines()
        .map(|line| expand_tabs(line, usize::from(tab_size.max(1))))
        .collect();
    log::debug!("[screen_model] split text to {} lines", lines.len());
    lines
}

fn apply_line_number_prefix(
    lines: Vec<String>,
    enabled: bool,
    configured_width: u16,
) -> Vec<String> {
    if !enabled {
        return lines;
    }

    let width = line_number_width(lines.len(), configured_width);
    let numbered_lines = lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| format!("{:>width$} {}", index + 1, line, width = width))
        .collect();
    log::debug!("[screen_model] applied line number prefix");
    numbered_lines
}

/// vim-core-rs のバイト列ベースカーソル位置を terminal の表示セル列へ変換する。
fn resolve_cursor_col(
    text: &str,
    cursor_row: usize,
    cursor_col: usize,
    tab_size: u16,
    line_numbers: bool,
    number_width: u16,
) -> u16 {
    let line = text.split('\n').nth(cursor_row).unwrap_or("");
    let clamped_col = cursor_col.min(line.len());
    let boundary_col = clamp_to_char_boundary(line, clamped_col);
    let base_display_col = display_width(&line[..boundary_col], usize::from(tab_size.max(1)));
    let line_number_offset = usize::from(line_number_offset(text, line_numbers, number_width));
    let display_col = base_display_col.saturating_add(line_number_offset);
    let display_col = u16::try_from(display_col).unwrap_or(u16::MAX);

    log::debug!(
        "[screen_model] resolved cursor col: row={}, raw_col={}, boundary_col={}, base_display_col={}, line_number_offset={}, display_col={}",
        cursor_row,
        cursor_col,
        boundary_col,
        base_display_col,
        line_number_offset,
        display_col
    );

    display_col
}

fn resolve_display_col_for_position(
    text: &str,
    cursor_row: usize,
    cursor_col: usize,
    tab_size: u16,
    line_numbers: bool,
    number_width: u16,
) -> u16 {
    resolve_cursor_col(
        text,
        cursor_row,
        cursor_col,
        tab_size,
        line_numbers,
        number_width,
    )
}

fn resolve_display_col_after_inclusive_position(
    text: &str,
    cursor_row: usize,
    cursor_col: usize,
    tab_size: u16,
    line_numbers: bool,
    number_width: u16,
) -> u16 {
    let line = text.split('\n').nth(cursor_row).unwrap_or("");
    if line.is_empty() {
        return resolve_display_col_for_position(
            text,
            cursor_row,
            cursor_col,
            tab_size,
            line_numbers,
            number_width,
        );
    }
    let clamped_col = clamp_to_char_boundary(line, cursor_col.min(line.len()));
    let next_col = line[clamped_col..]
        .chars()
        .next()
        .map(|ch| clamped_col + ch.len_utf8())
        .unwrap_or(clamped_col);
    resolve_display_col_for_position(
        text,
        cursor_row,
        next_col,
        tab_size,
        line_numbers,
        number_width,
    )
}

fn line_number_offset(text: &str, line_numbers: bool, number_width: u16) -> u16 {
    if line_numbers {
        u16::try_from(line_number_width(text.lines().count(), number_width) + 1).unwrap_or(u16::MAX)
    } else {
        0
    }
}

fn line_number_width(line_count: usize, configured_width: u16) -> usize {
    line_count
        .max(1)
        .to_string()
        .len()
        .max(usize::from(configured_width.max(1)))
}

fn resolve_cursor_row(cursor_row: usize, viewport_top: usize, body_height: usize) -> u16 {
    let body_height = body_height.max(1);
    let relative_row = cursor_row.saturating_sub(viewport_top).min(body_height - 1);
    let relative_row = u16::try_from(relative_row).unwrap_or(u16::MAX);

    log::debug!(
        "[screen_model] resolved cursor row: absolute_row={}, viewport_top={}, body_height={}, relative_row={}",
        cursor_row,
        viewport_top,
        body_height,
        relative_row
    );

    relative_row
}

fn clamp_to_char_boundary(text: &str, col: usize) -> usize {
    let mut boundary = col.min(text.len());
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

fn expand_tabs(line: &str, tab_size: usize) -> String {
    let mut expanded = String::new();
    let mut display_col = 0usize;

    for ch in line.chars() {
        if ch == '\t' {
            let spaces = next_tab_stop(display_col, tab_size) - display_col;
            expanded.push_str(&" ".repeat(spaces));
            display_col += spaces;
            continue;
        }

        expanded.push(ch);
        display_col += char_display_width(ch);
    }

    expanded
}

fn display_width(text: &str, tab_size: usize) -> usize {
    let mut display_col = 0usize;

    for ch in text.chars() {
        if ch == '\t' {
            display_col = next_tab_stop(display_col, tab_size);
        } else {
            display_col += char_display_width(ch);
        }
    }

    display_col
}

fn next_tab_stop(display_col: usize, tab_size: usize) -> usize {
    let tab_size = tab_size.max(1);
    display_col + (tab_size - (display_col % tab_size)).min(tab_size)
}

fn char_display_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

/// メッセージ欄の内容を解決する。
///
/// transient_message が指定されていればそれを優先し、
/// なければ session_state の last_save_error を表示する。
fn resolve_message_line(
    session_state: &EditorSessionState,
    transient_message: Option<&str>,
) -> Option<String> {
    if let Some(msg) = transient_message {
        log::debug!("[screen_model] message line from transient: {:?}", msg);
        return Some(msg.to_string());
    }

    if let Some(error) = session_state.last_save_error() {
        log::debug!("[screen_model] message line from save error: {:?}", error);
        return Some(format!("保存失敗: {}", error));
    }

    log::debug!("[screen_model] no message line");
    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    use vim_core_rs::CoreMode;

    use super::*;
    use crate::core_bridge::CoreBridge;

    fn session_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    // ---- タスク 6.1: file name と mode を描画モデルへ投影するテスト ----

    #[test]
    fn projects_file_name_from_session_target_path() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("hello\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(Some(PathBuf::from("/tmp/hello.txt")));

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(
            model.file_name, "/tmp/hello.txt",
            "session の target_path がファイル名として投影されること"
        );
    }

    #[test]
    fn projects_default_file_name_when_no_target_path() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(
            model.file_name, "[新規]",
            "ターゲットパスなしの場合はデフォルト名が使われること"
        );
    }

    #[test]
    fn projects_normal_mode_label() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.mode, CoreMode::Normal);

        let session_state = EditorSessionState::new(None);
        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(
            model.mode_label, "NORMAL",
            "ノーマルモードのラベルが NORMAL であること"
        );
    }

    #[test]
    fn projects_insert_mode_label() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("text\n").expect("core bridge");
        bridge.dispatch_key("i").expect("insert mode");
        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.mode, CoreMode::Insert);

        let session_state = EditorSessionState::new(None);
        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(
            model.mode_label, "INSERT",
            "インサートモードのラベルが INSERT であること"
        );
    }

    #[test]
    fn file_name_and_mode_are_never_empty_at_startup() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert!(
            !model.file_name.is_empty(),
            "起動直後でもファイル名は空でないこと"
        );
        assert!(
            !model.mode_label.is_empty(),
            "起動直後でもモードラベルは空でないこと"
        );
    }

    // ---- タスク 6.2: dirty 状態とカーソル位置を描画モデルへ投影するテスト ----

    #[test]
    fn projects_dirty_false_for_clean_buffer() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("clean\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert!(!model.dirty, "未編集バッファは dirty=false であること");
    }

    #[test]
    fn projects_dirty_true_after_edit() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("text\n").expect("core bridge");
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("X").expect("insert X");
        bridge.dispatch_key("\x1b").expect("normal mode");
        let snapshot = bridge.snapshot();
        assert!(snapshot.dirty);

        let session_state = EditorSessionState::new(None);
        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert!(model.dirty, "編集後のバッファは dirty=true であること");
    }

    #[test]
    fn projects_cursor_position_at_origin() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("abc\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.cursor_row, 0, "初期カーソル行は 0");
        assert_eq!(model.cursor_col, 0, "初期カーソル列は 0");
    }

    #[test]
    fn projects_cursor_position_after_movement() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("abcde\nfghij\n").expect("core bridge");
        bridge.dispatch_key("jll").expect("j, ll for movement");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.cursor_row, 1, "カーソル行が移動後に反映されること");
        assert_eq!(model.cursor_col, 2, "カーソル列が移動後に反映されること");
    }

    #[test]
    fn projects_visible_slice_and_relative_cursor_row_when_viewport_applied() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("line1\nline2\nline3\nline4\nline5\n").expect("core bridge");
        bridge.dispatch_key("jjj").expect("move to fourth line");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model =
            project(&ProjectionInput::new(&snapshot, &session_state, None).with_viewport(2, 2));

        assert_eq!(model.lines, vec!["line3", "line4"]);
        assert_eq!(model.cursor_row, 1, "viewport 内の相対行へ変換されること");
    }

    #[test]
    fn dirty_and_cursor_update_on_redraw() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("line1\nline2\n").expect("core bridge");

        // 初回投影
        let snapshot1 = bridge.snapshot();
        let session_state = EditorSessionState::new(None);
        let model1 = project(&ProjectionInput::new(&snapshot1, &session_state, None));
        assert!(!model1.dirty);
        assert_eq!(model1.cursor_row, 0);

        // 編集操作後に再投影
        bridge.dispatch_key("j").expect("move down");
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("Z").expect("insert Z");
        bridge.dispatch_key("\x1b").expect("normal mode");

        let snapshot2 = bridge.snapshot();
        let model2 = project(&ProjectionInput::new(&snapshot2, &session_state, None));

        assert!(model2.dirty, "編集後の再描画では dirty=true");
        assert_eq!(model2.cursor_row, 1, "カーソル行が再描画で追随すること");
    }

    // ---- タスク 6.3: message line を描画モデルへ取り込むテスト ----

    #[test]
    fn projects_no_message_line_in_normal_state() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.message_line, None, "通常状態ではメッセージ欄は空");
    }

    #[test]
    fn projects_save_failure_as_message_line() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let mut session_state = EditorSessionState::new(None);
        session_state.record_save_failure("disk full".to_string());

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(
            model.message_line,
            Some("保存失敗: disk full".to_string()),
            "保存失敗メッセージが message_line に反映されること"
        );
    }

    #[test]
    fn projects_transient_message_over_save_error_in_message_line() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let mut session_state = EditorSessionState::new(None);
        session_state.record_save_failure("old error".to_string());

        let model = project(&ProjectionInput::new(
            &snapshot,
            &session_state,
            Some("未保存の変更があります"),
        ));

        assert_eq!(
            model.message_line,
            Some("未保存の変更があります".to_string()),
            "transient_message が save error より優先されること"
        );
    }

    #[test]
    fn projects_unsaved_warning_as_transient_message() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(
            &snapshot,
            &session_state,
            Some("未保存の変更があります。:q! で強制終了できます"),
        ));

        assert_eq!(
            model.message_line,
            Some("未保存の変更があります。:q! で強制終了できます".to_string()),
            "未保存警告が transient_message として投影されること"
        );
    }

    #[test]
    fn projects_config_failure_as_transient_message() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(
            &snapshot,
            &session_state,
            Some("設定の読み込みに失敗しました"),
        ));

        assert_eq!(
            model.message_line,
            Some("設定の読み込みに失敗しました".to_string()),
            "設定失敗メッセージが投影されること"
        );
    }

    #[test]
    fn clears_message_line_after_save_success() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let mut session_state = EditorSessionState::new(None);

        // 保存失敗を記録
        session_state.record_save_failure("error".to_string());
        let model1 = project(&ProjectionInput::new(&snapshot, &session_state, None));
        assert!(model1.message_line.is_some());

        // 保存成功を記録
        session_state.record_save_success();
        let model2 = project(&ProjectionInput::new(&snapshot, &session_state, None));
        assert_eq!(
            model2.message_line, None,
            "保存成功後はメッセージ欄がクリアされること"
        );
    }

    #[test]
    fn success_message_takes_priority_when_provided_as_transient() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(
            &snapshot,
            &session_state,
            Some("保存しました"),
        ));

        assert_eq!(
            model.message_line,
            Some("保存しました".to_string()),
            "成功メッセージが transient として投影されること"
        );
    }

    // ---- タスク 6.4: 行データとカーソルを terminal 描画へ流せるようにするテスト ----

    #[test]
    fn projects_text_lines_from_snapshot() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("line1\nline2\nline3\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(
            model.lines,
            vec!["line1", "line2", "line3"],
            "行データが snapshot から正しく分割されること"
        );
    }

    #[test]
    fn project_limits_visible_lines_to_body_height() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("line1\nline2\nline3\nline4\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model =
            project(&ProjectionInput::new(&snapshot, &session_state, None).with_viewport(1, 2));

        assert_eq!(model.lines, vec!["line2", "line3"]);
    }

    #[test]
    fn projects_cursor_coordinates_as_u16() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("abcdef\nghijkl\n").expect("core bridge");
        bridge.dispatch_key("jlll").expect("move to row=1, col=3");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.cursor_row, 1, "カーソル行が u16 として正しく変換");
        assert_eq!(model.cursor_col, 3, "カーソル列が u16 として正しく変換");
    }

    #[test]
    fn projects_display_cursor_col_for_multibyte_character() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("あa\n").expect("core bridge");
        bridge
            .dispatch_key("l")
            .expect("move right over multibyte char");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        assert_eq!(
            snapshot.cursor_col, 3,
            "vim-core-rs の cursor_col は UTF-8 バイト位置で進むこと"
        );

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.cursor_row, 0, "行位置はそのまま反映されること");
        assert_eq!(
            model.cursor_col, 2,
            "全角 1 文字ぶんは terminal 上で 2 セルとして描画されること"
        );
    }

    #[test]
    fn projects_cursor_col_with_line_number_prefix() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("alpha\nbeta\n").expect("core bridge");
        bridge.dispatch_key("jll").expect("move to row=1, col=2");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(
            None, 8, true, 4,
        );

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.lines[1], "   2 beta");
        assert_eq!(model.cursor_row, 1);
        assert_eq!(
            model.cursor_col, 7,
            "行番号と区切り分だけ右へ補正されること"
        );
    }

    #[test]
    fn projects_cursor_col_with_line_numbers_and_multibyte_text() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("あa\n").expect("core bridge");
        bridge
            .dispatch_key("l")
            .expect("move right over multibyte char");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(
            None, 8, true, 4,
        );

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.lines[0], "   1 あa");
        assert_eq!(
            model.cursor_col, 7,
            "全角表示幅に行番号オフセットが加算されること"
        );
    }

    #[test]
    fn projects_line_numbers_using_configured_minimum_width() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("alpha\nbeta\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new_with_tab_size_and_line_numbers_and_number_width(
            None, 8, true, 4,
        );

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.lines[0], "   1 alpha");
        assert_eq!(model.lines[1], "   2 beta");
    }

    #[test]
    fn projects_tab_as_spaces_using_default_tab_size() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("\ta\n").expect("core bridge");
        bridge.dispatch_key("l").expect("move right over tab");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.lines[0], "        a");
        assert_eq!(model.cursor_col, 8);
    }

    #[test]
    fn projects_tab_using_configured_tab_size() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("\ta\n").expect("core bridge");
        bridge.dispatch_key("l").expect("move right over tab");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new_with_tab_size(None, 4);

        let model = project(&ProjectionInput::new(&snapshot, &session_state, None));

        assert_eq!(model.lines[0], "    a");
        assert_eq!(model.cursor_col, 4);
    }

    #[test]
    fn screen_model_contains_all_draw_fields() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("first\nsecond\n").expect("core bridge");
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("X").expect("insert X");
        bridge.dispatch_key("\x1b").expect("normal mode");
        let snapshot = bridge.snapshot();

        let session_state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));

        let model = project(&ProjectionInput::new(
            &snapshot,
            &session_state,
            Some("テストメッセージ"),
        ));

        // 全フィールドがまとめて draw に必要なデータを持つこと
        assert!(!model.file_name.is_empty(), "ファイル名は空でないこと");
        assert!(!model.mode_label.is_empty(), "モードラベルは空でないこと");
        assert!(model.dirty, "編集後は dirty=true であること");
        assert!(!model.lines.is_empty(), "行データは空でないこと");
        assert!(model.message_line.is_some(), "メッセージ欄が存在すること");
    }

    #[test]
    fn redraw_input_limited_to_screen_model_only() {
        // ScreenModel だけで描画に必要な全情報が揃うことを型レベルで検証
        let model = ScreenModel {
            file_name: "test.txt".to_string(),
            mode_label: "NORMAL".to_string(),
            dirty: false,
            lines: vec!["hello".to_string()],
            cursor_row: 0,
            cursor_col: 0,
            visual_selection: None,
            message_line: None,
        };

        // ScreenModel の各フィールドにアクセスできること（コンパイル時検証）
        let _ = &model.file_name;
        let _ = &model.mode_label;
        let _ = model.dirty;
        let _ = &model.lines;
        let _ = model.cursor_row;
        let _ = model.cursor_col;
        let _ = &model.visual_selection;
        let _ = &model.message_line;

        // CoreSnapshot への直接参照は不要（型の独立性）
        assert_eq!(
            model.mode_label, "NORMAL",
            "ScreenModel だけで描画に必要な全情報が揃うこと"
        );
    }
}
