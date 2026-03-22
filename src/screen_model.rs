//! 描画専用モデルと投影ロジック。
//!
//! CoreSnapshot と EditorSessionState から描画に必要な情報だけを
//! 抽出し、ScreenModel として TuiRenderer に渡す。
//! 描画側は ScreenModel だけを入力とし、CoreSnapshot に直接依存しない。

use unicode_width::UnicodeWidthChar;
use vim_core_rs::{CoreMode, CoreSnapshot};

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
    /// ステータスメッセージ（エラーや通知）
    pub status_message: Option<String>,
}

/// 投影の入力をまとめた構造体。
///
/// CoreSnapshot と EditorSessionState から描画に必要な情報を選択して渡す。
pub struct ProjectionInput<'a> {
    pub snapshot: &'a CoreSnapshot,
    pub session_state: &'a EditorSessionState,
    pub transient_message: Option<&'a str>,
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
    let lines = split_text_to_lines(&input.snapshot.text, input.session_state.tab_size());
    let cursor_row = input.snapshot.cursor_row as u16;
    let cursor_col = resolve_cursor_col(
        &input.snapshot.text,
        input.snapshot.cursor_row,
        input.snapshot.cursor_col,
        input.session_state.tab_size(),
    );
    let status_message = resolve_status_message(input.session_state, input.transient_message);

    log::debug!(
        "[screen_model] projected: file_name={:?}, mode_label={:?}, dirty={}, lines_count={}, cursor=({},{}), status_message={:?}",
        file_name,
        mode_label,
        dirty,
        lines.len(),
        cursor_row,
        cursor_col,
        status_message,
    );

    ScreenModel {
        file_name,
        mode_label,
        dirty,
        lines,
        cursor_row,
        cursor_col,
        status_message,
    }
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

/// vim-core-rs のバイト列ベースカーソル位置を terminal の表示セル列へ変換する。
fn resolve_cursor_col(text: &str, cursor_row: usize, cursor_col: usize, tab_size: u16) -> u16 {
    let line = text.split('\n').nth(cursor_row).unwrap_or("");
    let clamped_col = cursor_col.min(line.len());
    let boundary_col = clamp_to_char_boundary(line, clamped_col);
    let display_col = display_width(&line[..boundary_col], usize::from(tab_size.max(1)));
    let display_col = u16::try_from(display_col).unwrap_or(u16::MAX);

    log::debug!(
        "[screen_model] resolved cursor col: row={}, raw_col={}, boundary_col={}, display_col={}",
        cursor_row,
        cursor_col,
        boundary_col,
        display_col
    );

    display_col
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

/// ステータスメッセージを解決する。
///
/// transient_message が指定されていればそれを優先し、
/// なければ session_state の last_save_error を表示する。
fn resolve_status_message(
    session_state: &EditorSessionState,
    transient_message: Option<&str>,
) -> Option<String> {
    if let Some(msg) = transient_message {
        log::debug!("[screen_model] status message from transient: {:?}", msg);
        return Some(msg.to_string());
    }

    if let Some(error) = session_state.last_save_error() {
        log::debug!("[screen_model] status message from save error: {:?}", error);
        return Some(format!("保存失敗: {}", error));
    }

    log::debug!("[screen_model] no status message");
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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

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
        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

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
        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

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
        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

        assert_eq!(model.cursor_row, 1, "カーソル行が移動後に反映されること");
        assert_eq!(model.cursor_col, 2, "カーソル列が移動後に反映されること");
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
        let model1 = project(&ProjectionInput {
            snapshot: &snapshot1,
            session_state: &session_state,
            transient_message: None,
        });
        assert!(!model1.dirty);
        assert_eq!(model1.cursor_row, 0);

        // 編集操作後に再投影
        bridge.dispatch_key("j").expect("move down");
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("Z").expect("insert Z");
        bridge.dispatch_key("\x1b").expect("normal mode");

        let snapshot2 = bridge.snapshot();
        let model2 = project(&ProjectionInput {
            snapshot: &snapshot2,
            session_state: &session_state,
            transient_message: None,
        });

        assert!(model2.dirty, "編集後の再描画では dirty=true");
        assert_eq!(model2.cursor_row, 1, "カーソル行が再描画で追随すること");
    }

    // ---- タスク 6.3: status message を描画モデルへ取り込むテスト ----

    #[test]
    fn projects_no_status_message_in_normal_state() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let session_state = EditorSessionState::new(None);

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

        assert_eq!(
            model.status_message, None,
            "通常状態ではステータスメッセージなし"
        );
    }

    #[test]
    fn projects_save_failure_as_status_message() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let mut session_state = EditorSessionState::new(None);
        session_state.record_save_failure("disk full".to_string());

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

        assert_eq!(
            model.status_message,
            Some("保存失敗: disk full".to_string()),
            "保存失敗メッセージが status_message に反映されること"
        );
    }

    #[test]
    fn projects_transient_message_over_save_error() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let mut session_state = EditorSessionState::new(None);
        session_state.record_save_failure("old error".to_string());

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: Some("未保存の変更があります"),
        });

        assert_eq!(
            model.status_message,
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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: Some("未保存の変更があります。:q! で強制終了できます"),
        });

        assert_eq!(
            model.status_message,
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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: Some("設定の読み込みに失敗しました"),
        });

        assert_eq!(
            model.status_message,
            Some("設定の読み込みに失敗しました".to_string()),
            "設定失敗メッセージが投影されること"
        );
    }

    #[test]
    fn clears_status_message_after_save_success() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("text\n").expect("core bridge");
        let snapshot = bridge.snapshot();
        let mut session_state = EditorSessionState::new(None);

        // 保存失敗を記録
        session_state.record_save_failure("error".to_string());
        let model1 = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });
        assert!(model1.status_message.is_some());

        // 保存成功を記録
        session_state.record_save_success();
        let model2 = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });
        assert_eq!(
            model2.status_message, None,
            "保存成功後はステータスメッセージがクリアされること"
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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: Some("保存しました"),
        });

        assert_eq!(
            model.status_message,
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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

        assert_eq!(
            model.lines,
            vec!["line1", "line2", "line3"],
            "行データが snapshot から正しく分割されること"
        );
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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

        assert_eq!(model.cursor_row, 0, "行位置はそのまま反映されること");
        assert_eq!(
            model.cursor_col, 2,
            "全角 1 文字ぶんは terminal 上で 2 セルとして描画されること"
        );
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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: None,
        });

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

        let model = project(&ProjectionInput {
            snapshot: &snapshot,
            session_state: &session_state,
            transient_message: Some("テストメッセージ"),
        });

        // 全フィールドがまとめて draw に必要なデータを持つこと
        assert!(!model.file_name.is_empty(), "ファイル名は空でないこと");
        assert!(!model.mode_label.is_empty(), "モードラベルは空でないこと");
        assert!(model.dirty, "編集後は dirty=true であること");
        assert!(!model.lines.is_empty(), "行データは空でないこと");
        assert!(
            model.status_message.is_some(),
            "ステータスメッセージが存在すること"
        );
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
            status_message: None,
        };

        // ScreenModel の各フィールドにアクセスできること（コンパイル時検証）
        let _ = &model.file_name;
        let _ = &model.mode_label;
        let _ = model.dirty;
        let _ = &model.lines;
        let _ = model.cursor_row;
        let _ = model.cursor_col;
        let _ = &model.status_message;

        // CoreSnapshot への直接参照は不要（型の独立性）
        assert_eq!(
            model.mode_label, "NORMAL",
            "ScreenModel だけで描画に必要な全情報が揃うこと"
        );
    }
}
