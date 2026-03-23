use std::fmt;
use std::path::Path;

use vim_core_rs::{
    CoreCommandOutcome, CoreHostAction, CoreSessionError, CoreSnapshot, VimCoreSession,
};

pub struct CoreBridge {
    session: VimCoreSession,
    preferred_column: Option<usize>,
    pending_normal_operator: Option<String>,
}

impl fmt::Debug for CoreBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoreBridge")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

impl CoreBridge {
    pub fn new(initial_text: &str) -> Result<Self, CoreSessionError> {
        log::debug!(
            "[core_bridge] initializing vim-core-rs session: initial_text_len={}",
            initial_text.len()
        );
        let session = VimCoreSession::new(initial_text)?;
        log::debug!("[core_bridge] vim-core-rs session initialized");
        Ok(Self {
            session,
            preferred_column: None,
            pending_normal_operator: None,
        })
    }

    pub fn new_with_target_path(
        target_path: &Path,
        initial_text: &str,
    ) -> Result<Self, CoreSessionError> {
        let mut bridge = Self::new(initial_text)?;
        bridge.attach_target_path(target_path)?;
        Ok(bridge)
    }

    pub fn snapshot(&self) -> CoreSnapshot {
        let snapshot = self.session.snapshot();
        log::debug!(
            "[core_bridge] snapshot captured: revision={}, dirty={}, pending_host_actions={}",
            snapshot.revision,
            snapshot.dirty,
            snapshot.pending_host_actions
        );
        snapshot
    }

    pub fn take_pending_host_actions(&mut self) -> Vec<CoreHostAction> {
        let mut actions = Vec::new();
        while let Some(action) = self.session.take_pending_host_action() {
            actions.push(action);
        }
        log::debug!(
            "[core_bridge] drained pending host actions: count={}",
            actions.len()
        );
        actions
    }

    /// キー入力を vim-core-rs セッションに適用する。
    /// ノーマルモードコマンドとして解釈し、結果を返す。
    pub fn dispatch_key(&mut self, key: &str) -> Result<CoreCommandOutcome, CoreSessionError> {
        log::debug!(
            "[core_bridge] dispatching key: {:?} (len={})",
            key,
            key.len()
        );
        let outcome = if self.should_route_through_single_key_normal_path(key) {
            self.dispatch_single_normal_key(key)?
        } else if self.should_preserve_preferred_column(key) {
            self.dispatch_vertical_motion_with_preferred_column(key)?
        } else {
            self.execute_normal_command(key)?
        };
        log::debug!(
            "[core_bridge] dispatch result: {:?}, preferred_column={:?}, pending_normal_operator={:?}",
            outcome,
            self.preferred_column,
            self.pending_normal_operator
        );
        Ok(outcome)
    }

    /// ex コマンドを vim-core-rs セッションに適用する。
    /// `:w`, `:q`, `:q!` などの実行に使用する。
    pub fn apply_ex_command(
        &mut self,
        command: &str,
    ) -> Result<CoreCommandOutcome, CoreSessionError> {
        log::debug!("[core_bridge] applying ex command: {:?}", command);
        let outcome = self
            .session
            .apply_ex_command(command)
            .map_err(CoreSessionError::CommandFailed)?;
        log::debug!("[core_bridge] ex command result: {:?}", outcome);
        Ok(outcome)
    }

    /// buffer のテキスト内容を返す。保存要求の生成に使用する。
    pub fn buffer_text(&self) -> String {
        let snapshot = self.session.snapshot();
        log::debug!(
            "[core_bridge] buffer text retrieved: len={}",
            snapshot.text.len()
        );
        snapshot.text
    }

    pub fn attach_target_path(&mut self, target_path: &Path) -> Result<(), CoreSessionError> {
        let escaped_path = escape_path_for_file_command(target_path);
        log::debug!(
            "[core_bridge] attaching target path to active buffer: {}",
            target_path.display()
        );
        self.session
            .apply_ex_command(&format!(":file {}", escaped_path))
            .map_err(CoreSessionError::CommandFailed)?;
        Ok(())
    }
}

impl CoreBridge {
    fn should_route_through_single_key_normal_path(&self, key: &str) -> bool {
        self.session.snapshot().mode == vim_core_rs::CoreMode::Normal
            && key.chars().count() == 1
            && !matches!(key, "\x1b")
    }

    fn should_preserve_preferred_column(&self, key: &str) -> bool {
        matches!(key, "j" | "k") && self.session.snapshot().mode == vim_core_rs::CoreMode::Normal
    }

    fn dispatch_single_normal_key(
        &mut self,
        key: &str,
    ) -> Result<CoreCommandOutcome, CoreSessionError> {
        if let Some(prefix) = self.pending_normal_operator.take() {
            let command = format!("{prefix}{key}");
            log::debug!(
                "[core_bridge] completing pending normal operator: prefix={:?}, key={:?}, command={:?}",
                prefix,
                key,
                command
            );
            return self.execute_normal_command(&command);
        }

        if is_pending_normal_operator(key) {
            log::debug!(
                "[core_bridge] storing pending normal operator: key={:?}",
                key
            );
            self.pending_normal_operator = Some(key.to_string());
            return Ok(CoreCommandOutcome::NoChange);
        }

        if self.should_preserve_preferred_column(key) {
            return self.dispatch_vertical_motion_with_preferred_column(key);
        }

        self.execute_normal_command(key)
    }

    fn execute_normal_command(
        &mut self,
        command: &str,
    ) -> Result<CoreCommandOutcome, CoreSessionError> {
        let outcome = self
            .session
            .apply_normal_command(command)
            .map_err(CoreSessionError::CommandFailed)?;
        self.update_preferred_column_from_snapshot(command);
        Ok(outcome)
    }

    fn dispatch_vertical_motion_with_preferred_column(
        &mut self,
        key: &str,
    ) -> Result<CoreCommandOutcome, CoreSessionError> {
        let before = self.session.snapshot();
        let desired_col = self.preferred_column.unwrap_or(before.cursor_col);
        log::debug!(
            "[core_bridge] vertical motion with preferred column: key={:?}, row={}, col={}, desired_col={}",
            key,
            before.cursor_row,
            before.cursor_col,
            desired_col
        );

        self.session
            .apply_normal_command(key)
            .map_err(CoreSessionError::CommandFailed)?;
        let moved = self.session.snapshot();

        if moved.cursor_col != desired_col {
            let restore_command = format!("{}|", desired_col.saturating_add(1));
            log::debug!(
                "[core_bridge] restoring preferred column after vertical motion: command={:?}, current_col={}, desired_col={}",
                restore_command,
                moved.cursor_col,
                desired_col
            );
            self.session
                .apply_normal_command(&restore_command)
                .map_err(CoreSessionError::CommandFailed)?;
        }

        self.preferred_column = Some(desired_col);
        let snapshot = self.session.snapshot();
        Ok(CoreCommandOutcome::CursorChanged {
            row: snapshot.cursor_row,
            col: snapshot.cursor_col,
        })
    }

    fn update_preferred_column_from_snapshot(&mut self, key: &str) {
        if matches!(key, "j" | "k") {
            return;
        }

        let snapshot = self.session.snapshot();
        self.preferred_column = Some(snapshot.cursor_col);
        log::debug!(
            "[core_bridge] preferred column updated from snapshot: key={:?}, preferred_column={}",
            key,
            snapshot.cursor_col
        );
    }
}

fn is_pending_normal_operator(key: &str) -> bool {
    matches!(key, "d" | "y" | "c" | ">" | "<" | "=")
}

fn escape_path_for_file_command(target_path: &Path) -> String {
    let mut escaped = String::new();
    for ch in target_path.to_string_lossy().chars() {
        if matches!(ch, ' ' | '\\' | '|' | '"' | '%' | '#' | '<' | '>') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use vim_core_rs::CoreMode;

    use super::CoreBridge;

    fn session_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn unique_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("saya-core-bridge-{name}-{nanos}.txt"))
    }

    #[test]
    fn initializes_vim_core_session_and_returns_initial_snapshot() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("").expect("core bridge should initialize");
        let snapshot = bridge.snapshot();

        assert_eq!(snapshot.text, "\n");
        assert_eq!(snapshot.revision, 0);
        assert!(!snapshot.dirty);
        assert_eq!(snapshot.mode, CoreMode::Normal);
    }

    #[test]
    fn starts_with_no_pending_host_actions() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");

        assert!(bridge.take_pending_host_actions().is_empty());
    }

    #[test]
    fn initializes_empty_buffer_for_new_file_scenario() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("").expect("core bridge should initialize with empty text");
        let snapshot = bridge.snapshot();

        assert_eq!(snapshot.text, "\n");
        assert!(!snapshot.dirty);
        assert_eq!(snapshot.mode, CoreMode::Normal);
    }

    #[test]
    fn allows_attaching_target_path_to_empty_buffer_after_creation() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let save_path = unique_path("new-file-save");

        let mut bridge = CoreBridge::new("").expect("core bridge should initialize");
        bridge
            .attach_target_path(&save_path)
            .expect("should attach target path to empty buffer");
        let snapshot = bridge.snapshot();

        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .expect("active buffer should exist");

        assert_eq!(active_buffer.name, save_path.display().to_string());
        assert_eq!(snapshot.text, "\n");
        assert!(!snapshot.dirty);
    }

    #[test]
    fn initializes_existing_file_session_with_target_path_as_buffer_name() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("target file");

        let bridge = CoreBridge::new_with_target_path(&target_path, "buffer text\n")
            .expect("core bridge should initialize existing file session");
        let snapshot = bridge.snapshot();
        let active_buffer = snapshot
            .buffers
            .iter()
            .find(|buffer| buffer.is_active)
            .expect("active buffer should exist");

        assert_eq!(active_buffer.name, target_path.display().to_string());
        assert_eq!(snapshot.text, "buffer text\n");
        assert!(!snapshot.dirty);
        assert_eq!(snapshot.mode, CoreMode::Normal);
    }

    // ---- タスク 4.1: モード遷移テスト ----

    #[test]
    fn starts_in_normal_mode() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");
        let snapshot = bridge.snapshot();

        assert_eq!(
            snapshot.mode,
            CoreMode::Normal,
            "起動時はノーマルモードであること"
        );
    }

    #[test]
    fn transitions_to_insert_mode_with_i_key() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        let result = bridge.dispatch_key("i");
        assert!(result.is_ok(), "i キーの dispatch は成功すること");

        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.mode,
            CoreMode::Insert,
            "i キーでインサートモードに遷移すること"
        );
    }

    #[test]
    fn transitions_back_to_normal_mode_with_escape() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        bridge.dispatch_key("i").expect("i キーで insert 遷移");
        assert_eq!(bridge.snapshot().mode, CoreMode::Insert);

        bridge
            .dispatch_key("\x1b")
            .expect("Escape キーで normal 復帰");
        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.mode,
            CoreMode::Normal,
            "Escape でノーマルモードに復帰すること"
        );
    }

    #[test]
    fn current_mode_is_available_from_snapshot_after_dispatch() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("test\n").expect("core bridge should initialize");

        // ノーマルモード確認
        assert_eq!(bridge.snapshot().mode, CoreMode::Normal);

        // インサートモードへ
        bridge.dispatch_key("i").expect("insert mode");
        assert_eq!(bridge.snapshot().mode, CoreMode::Insert);

        // ノーマルモードへ戻る
        bridge.dispatch_key("\x1b").expect("normal mode");
        assert_eq!(bridge.snapshot().mode, CoreMode::Normal);
    }

    // ---- タスク 4.2: カーソル移動テスト ----

    #[test]
    fn cursor_moves_down_with_j_key() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first line\nsecond line\n").expect("core bridge should initialize");
        let initial = bridge.snapshot();
        assert_eq!(initial.cursor_row, 0, "初期カーソル行は 0");
        assert_eq!(initial.cursor_col, 0, "初期カーソル列は 0");

        bridge.dispatch_key("j").expect("j キーで下移動");
        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.cursor_row, 1,
            "j キーでカーソルが 1 行下に移動すること"
        );
        assert_eq!(snapshot.cursor_col, 0, "j キーで列は変わらないこと");
    }

    #[test]
    fn cursor_moves_right_with_l_key() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        bridge.dispatch_key("l").expect("l キーで右移動");
        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.cursor_col, 1, "l キーでカーソルが右に移動すること");
        assert_eq!(snapshot.cursor_row, 0, "l キーで行は変わらないこと");
    }

    #[test]
    fn cursor_moves_left_with_h_key() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        // まず右に移動してから左に戻る
        bridge.dispatch_key("ll").expect("l で右に 2 回移動");
        assert_eq!(bridge.snapshot().cursor_col, 2);

        bridge.dispatch_key("h").expect("h キーで左移動");
        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.cursor_col, 1, "h キーでカーソルが左に移動すること");
    }

    #[test]
    fn cursor_moves_up_with_k_key() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");

        bridge.dispatch_key("jj").expect("j で 2 行下に移動");
        assert_eq!(bridge.snapshot().cursor_row, 2);
        bridge.dispatch_key("ll").expect("l で 2 列右に移動");
        assert_eq!(bridge.snapshot().cursor_col, 2);

        bridge.dispatch_key("k").expect("k キーで上移動");
        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.cursor_row, 1, "k キーでカーソルが上に移動すること");
        assert_eq!(snapshot.cursor_col, 2, "k キーで現在列を維持すること");
    }

    #[test]
    fn vertical_motion_keeps_preferred_column_across_shorter_line() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("abcdef\nx\nuvwxyz\n").expect("core bridge should initialize");

        bridge.dispatch_key("llll").expect("l で 4 列右に移動");
        assert_eq!(bridge.snapshot().cursor_col, 4);

        bridge.dispatch_key("j").expect("短い行へ下移動");
        let short_line = bridge.snapshot();
        assert_eq!(short_line.cursor_row, 1);
        assert_eq!(short_line.cursor_col, 0, "短い行では行末へ丸められること");

        bridge.dispatch_key("j").expect("再び下移動");
        let restored = bridge.snapshot();
        assert_eq!(restored.cursor_row, 2);
        assert_eq!(
            restored.cursor_col, 4,
            "短い行を経由しても次の長い行で目標列へ戻ること"
        );
    }

    #[test]
    fn cursor_position_reflected_in_snapshot_after_multiple_moves() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("abcde\nfghij\nklmno\n").expect("core bridge should initialize");

        bridge
            .dispatch_key("jll")
            .expect("j で 1 行下、ll で 2 列右");
        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.cursor_row, 1, "複合移動後の行位置");
        assert_eq!(snapshot.cursor_col, 2, "複合移動後の列位置");
    }

    // ---- タスク 4.3: インサートモード文字入力テスト ----

    #[test]
    fn insert_mode_text_input_appears_in_buffer() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("").expect("core bridge should initialize");

        // i でインサートモードに入り、文字を入力して Esc で戻る
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("H").expect("H を入力");
        bridge.dispatch_key("i").expect("i を入力");
        bridge
            .dispatch_key("\x1b")
            .expect("Escape でノーマルモードに復帰");

        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.mode, CoreMode::Normal, "ノーマルモードに復帰");
        assert!(
            snapshot.text.contains("Hi"),
            "入力した文字 'Hi' がバッファに含まれること: actual={:?}",
            snapshot.text
        );
    }

    #[test]
    fn insert_mode_marks_buffer_dirty() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("").expect("core bridge should initialize");

        assert!(!bridge.snapshot().dirty, "初期状態は dirty でないこと");

        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("a").expect("a を入力");
        bridge.dispatch_key("\x1b").expect("normal mode");

        let snapshot = bridge.snapshot();
        assert!(
            snapshot.dirty,
            "インサートモードで文字入力後は dirty になること"
        );
    }

    #[test]
    fn insert_mode_text_input_updates_buffer_for_redraw() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("line1\n").expect("core bridge should initialize");

        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("X").expect("X を入力");
        bridge.dispatch_key("\x1b").expect("normal mode");

        let snapshot = bridge.snapshot();
        assert!(
            snapshot.text.starts_with("X"),
            "先頭に X が挿入されること: actual={:?}",
            snapshot.text
        );
        log::debug!(
            "[test] insert 後のバッファ内容（再描画用）: {:?}",
            snapshot.text
        );
    }

    // ---- タスク 4.4: 削除操作テスト ----

    #[test]
    fn x_key_deletes_character_at_cursor() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("abcde\n").expect("core bridge should initialize");

        bridge.dispatch_key("x").expect("x キーで文字削除");
        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.text, "bcde\n",
            "x キーで先頭の 'a' が削除されること"
        );
    }

    #[test]
    fn x_key_marks_buffer_dirty() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        assert!(!bridge.snapshot().dirty, "初期状態は dirty でないこと");

        bridge.dispatch_key("x").expect("x キーで削除");
        let snapshot = bridge.snapshot();
        assert!(snapshot.dirty, "削除後は dirty になること");
    }

    #[test]
    fn dd_deletes_entire_line() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");

        bridge.dispatch_key("dd").expect("dd で行削除");
        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.text, "second\nthird\n",
            "dd で最初の行が削除されること"
        );
        assert!(snapshot.dirty, "dd 後は dirty になること");
    }

    #[test]
    fn sequential_yyp_duplicates_current_line() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("first\nsecond\n").expect("core bridge should initialize");

        bridge
            .dispatch_key("y")
            .expect("first y should enter operator pending");
        bridge
            .dispatch_key("y")
            .expect("second y should yank current line");
        bridge
            .dispatch_key("p")
            .expect("p should paste yanked line");

        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.text, "first\nfirst\nsecond\n",
            "yyp で現在行が複製されること"
        );
    }

    #[test]
    fn sequential_dd_deletes_current_line() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");

        bridge
            .dispatch_key("d")
            .expect("first d should enter operator pending");
        bridge
            .dispatch_key("d")
            .expect("second d should delete current line");

        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.text, "second\nthird\n",
            "dd を逐次入力しても現在行が削除されること"
        );
    }

    #[test]
    fn delete_maintains_dirty_state_across_operations() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("abc\ndef\n").expect("core bridge should initialize");

        bridge.dispatch_key("x").expect("最初の x で削除");
        assert!(bridge.snapshot().dirty, "最初の削除後は dirty");

        bridge.dispatch_key("x").expect("2 回目の x で削除");
        let snapshot = bridge.snapshot();
        assert!(
            snapshot.dirty,
            "複数回の削除操作後も dirty 状態が維持されること"
        );
        assert_eq!(snapshot.text, "c\ndef\n", "2 文字削除後のバッファ内容");
    }

    // ---- タスク 5.1: 保存要求（:w）でホストアクション Write が発行されるテスト ----

    #[test]
    fn write_command_produces_write_host_action() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let target_path = unique_path("write-host-action");

        let mut bridge = CoreBridge::new_with_target_path(&target_path, "content\n")
            .expect("core bridge should initialize");

        // :w を実行
        bridge
            .apply_ex_command(":w")
            .expect(":w コマンドは成功すること");

        let actions = bridge.take_pending_host_actions();
        log::debug!("[test] host actions after :w: {:?}", actions);
        let has_write = actions
            .iter()
            .any(|a| matches!(a, vim_core_rs::CoreHostAction::Write { .. }));
        assert!(
            has_write,
            ":w 実行後に Write ホストアクションが発行されること: actions={:?}",
            actions
        );
    }

    #[test]
    fn buffer_text_returns_current_contents() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        let text = bridge.buffer_text();
        assert_eq!(text, "hello\n", "buffer_text が現在の内容を返すこと");

        // 編集後もテキストが更新されること
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("X").expect("X を入力");
        bridge.dispatch_key("\x1b").expect("normal mode");

        let text_after = bridge.buffer_text();
        assert!(
            text_after.contains("X"),
            "編集後の buffer_text に入力文字が含まれること: {:?}",
            text_after
        );
    }

    // ---- タスク 5.4: 終了要求（:q, :q!）でホストアクション Quit が発行されるテスト ----

    #[test]
    fn quit_command_produces_quit_host_action() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("content\n").expect("core bridge should initialize");

        bridge
            .apply_ex_command(":q")
            .expect(":q コマンドは成功すること");

        let actions = bridge.take_pending_host_actions();
        log::debug!("[test] host actions after :q: {:?}", actions);
        let has_quit = actions
            .iter()
            .any(|a| matches!(a, vim_core_rs::CoreHostAction::Quit { force: false, .. }));
        assert!(
            has_quit,
            ":q 実行後に Quit(force=false) ホストアクションが発行されること: actions={:?}",
            actions
        );
    }

    #[test]
    fn force_quit_command_produces_force_quit_host_action() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("content\n").expect("core bridge should initialize");

        bridge
            .apply_ex_command(":q!")
            .expect(":q! コマンドは成功すること");

        let actions = bridge.take_pending_host_actions();
        log::debug!("[test] host actions after :q!: {:?}", actions);
        let has_force_quit = actions
            .iter()
            .any(|a| matches!(a, vim_core_rs::CoreHostAction::Quit { force: true, .. }));
        assert!(
            has_force_quit,
            ":q! 実行後に Quit(force=true) ホストアクションが発行されること: actions={:?}",
            actions
        );
    }
}
