/// エディタセッションの保存・終了を統括するモジュール。
///
/// CoreBridge から取得した buffer 情報と対象パスを組み合わせて、
/// 保存要求の生成、保存結果の反映、終了判定を行う。
use std::path::PathBuf;

use crate::host_io::SaveRequest;

/// 保存要求の生成に失敗した理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveRequestError {
    /// 保存先パスが未設定
    NoTargetPath,
}

/// 終了要求の判定結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuitDecision {
    /// 即時終了可能
    Allow,
    /// 未保存変更があるため警告
    WarnUnsaved,
    /// 強制終了（未保存でも終了）
    ForceQuit,
}

/// エディタセッションの状態。保存と終了の判定に使用する。
#[derive(Debug)]
pub struct EditorSessionState {
    /// 対象ファイルパス（新規バッファの場合は None）
    target_path: Option<PathBuf>,
    /// 描画時のタブ幅
    tab_size: u16,
    /// 行番号表示の初期状態
    line_numbers: bool,
    /// 現在 dirty 状態かどうか
    dirty: bool,
    /// 直近の保存失敗メッセージ
    last_save_error: Option<String>,
}

impl EditorSessionState {
    /// 新しいセッション状態を作成する。
    pub fn new(target_path: Option<PathBuf>) -> Self {
        Self::new_with_tab_size_and_line_numbers(target_path, 8, false)
    }

    /// タブ幅を指定して新しいセッション状態を作成する。
    pub fn new_with_tab_size(target_path: Option<PathBuf>, tab_size: u16) -> Self {
        Self::new_with_tab_size_and_line_numbers(target_path, tab_size, false)
    }

    /// タブ幅と行番号表示を指定して新しいセッション状態を作成する。
    pub fn new_with_tab_size_and_line_numbers(
        target_path: Option<PathBuf>,
        tab_size: u16,
        line_numbers: bool,
    ) -> Self {
        let tab_size = tab_size.max(1);
        log::debug!(
            "[editor_session] new session state: target_path={:?}, tab_size={}, line_numbers={}",
            target_path,
            tab_size,
            line_numbers
        );
        Self {
            target_path,
            tab_size,
            line_numbers,
            dirty: false,
            last_save_error: None,
        }
    }

    /// 現在の buffer 内容から保存要求を生成する。
    /// target_path が未設定の場合は SaveRequestError::NoTargetPath を返す。
    pub fn build_save_request(
        &self,
        buffer_contents: &str,
    ) -> Result<SaveRequest, SaveRequestError> {
        log::debug!(
            "[editor_session] building save request: target_path={:?}, contents_len={}",
            self.target_path,
            buffer_contents.len()
        );
        match &self.target_path {
            Some(path) => {
                let request = SaveRequest {
                    path: path.clone(),
                    contents: buffer_contents.to_string(),
                };
                log::debug!(
                    "[editor_session] save request built: path={}",
                    path.display()
                );
                Ok(request)
            }
            None => {
                log::debug!("[editor_session] save request failed: no target path");
                Err(SaveRequestError::NoTargetPath)
            }
        }
    }

    /// dirty 状態を更新する（CoreBridge の snapshot から反映する想定）。
    pub fn update_dirty(&mut self, dirty: bool) {
        log::debug!(
            "[editor_session] dirty state updated: {} -> {}",
            self.dirty,
            dirty
        );
        self.dirty = dirty;
    }

    /// 現在の dirty 状態を返す。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 直近の保存失敗メッセージを返す。
    pub fn last_save_error(&self) -> Option<&str> {
        self.last_save_error.as_deref()
    }

    /// 保存成功を記録し、dirty 状態を解除する。
    pub fn record_save_success(&mut self) {
        log::debug!("[editor_session] save success recorded: dirty -> false");
        self.dirty = false;
        self.last_save_error = None;
    }

    /// 保存失敗を記録する。dirty 状態は維持される。
    pub fn record_save_failure(&mut self, message: String) {
        log::debug!(
            "[editor_session] save failure recorded: message={}, dirty={}",
            message,
            self.dirty
        );
        self.last_save_error = Some(message);
    }

    /// 終了要求を判定する。
    pub fn evaluate_quit(&self, force: bool) -> QuitDecision {
        log::debug!(
            "[editor_session] evaluating quit: force={}, dirty={}",
            force,
            self.dirty
        );
        if force {
            log::debug!("[editor_session] quit decision: ForceQuit");
            QuitDecision::ForceQuit
        } else if self.dirty {
            log::debug!("[editor_session] quit decision: WarnUnsaved");
            QuitDecision::WarnUnsaved
        } else {
            log::debug!("[editor_session] quit decision: Allow");
            QuitDecision::Allow
        }
    }

    /// 対象パスの参照を返す。
    pub fn target_path(&self) -> Option<&PathBuf> {
        self.target_path.as_ref()
    }

    /// 描画時のタブ幅を返す。
    pub fn tab_size(&self) -> u16 {
        self.tab_size
    }

    /// 行番号表示が有効かを返す。
    pub fn line_numbers(&self) -> bool {
        self.line_numbers
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    // ---- タスク 5.1: 保存要求の生成テスト ----

    #[test]
    fn build_save_request_returns_request_with_path_and_contents() {
        let state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        let buffer_contents = "hello world\n";

        let request = state
            .build_save_request(buffer_contents)
            .expect("保存要求の生成に成功すること");

        assert_eq!(request.path, PathBuf::from("/tmp/test.txt"));
        assert_eq!(request.contents, "hello world\n");
    }

    #[test]
    fn build_save_request_fails_when_no_target_path() {
        let state = EditorSessionState::new(None);

        let result = state.build_save_request("data");

        assert_eq!(
            result,
            Err(SaveRequestError::NoTargetPath),
            "target_path が未設定の場合は NoTargetPath エラーになること"
        );
    }

    #[test]
    fn build_save_request_extracts_current_buffer_contents() {
        let state = EditorSessionState::new(Some(PathBuf::from("/tmp/file.txt")));
        let contents = "line1\nline2\nline3\n";

        let request = state.build_save_request(contents).unwrap();

        assert_eq!(
            request.contents, contents,
            "buffer の現在内容がそのまま保存要求に含まれること"
        );
    }

    // ---- タスク 5.2: 保存成功時の clean 状態テスト ----

    #[test]
    fn record_save_success_clears_dirty_state() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);
        assert!(state.is_dirty(), "保存前は dirty であること");

        state.record_save_success();

        assert!(!state.is_dirty(), "保存成功後は dirty が解除されること");
    }

    #[test]
    fn record_save_success_clears_last_save_error() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.record_save_failure("previous error".to_string());
        assert!(state.last_save_error().is_some());

        state.record_save_success();

        assert_eq!(
            state.last_save_error(),
            None,
            "保存成功後はエラーメッセージがクリアされること"
        );
    }

    #[test]
    fn quit_evaluates_to_allow_after_save_success() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);
        state.record_save_success();

        let decision = state.evaluate_quit(false);

        assert_eq!(
            decision,
            QuitDecision::Allow,
            "保存成功後の通常終了は Allow であること"
        );
    }

    // ---- タスク 5.3: 保存失敗時の編集継続テスト ----

    #[test]
    fn record_save_failure_preserves_dirty_state() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        state.record_save_failure("disk full".to_string());

        assert!(state.is_dirty(), "保存失敗後も dirty 状態が維持されること");
    }

    #[test]
    fn record_save_failure_stores_error_message_for_display() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));

        state.record_save_failure("permission denied".to_string());

        assert_eq!(
            state.last_save_error(),
            Some("permission denied"),
            "保存失敗メッセージが表示用に保持されること"
        );
    }

    #[test]
    fn save_failure_does_not_prevent_further_editing() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);
        state.record_save_failure("write error".to_string());

        // 保存失敗後も dirty 更新が可能であること
        state.update_dirty(true);
        assert!(
            state.is_dirty(),
            "保存失敗後も編集状態の更新が可能であること"
        );

        // 再度保存要求を生成できること
        let request = state.build_save_request("updated content");
        assert!(request.is_ok(), "保存失敗後も保存要求を再生成できること");
    }

    // ---- タスク 5.4: 通常終了と強制終了の分岐テスト ----

    #[test]
    fn evaluate_quit_allows_when_clean() {
        let state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));

        let decision = state.evaluate_quit(false);

        assert_eq!(
            decision,
            QuitDecision::Allow,
            "clean 状態での通常終了は Allow であること"
        );
    }

    #[test]
    fn evaluate_quit_warns_when_dirty_and_not_forced() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        let decision = state.evaluate_quit(false);

        assert_eq!(
            decision,
            QuitDecision::WarnUnsaved,
            "dirty 状態での通常終了は WarnUnsaved であること"
        );
    }

    #[test]
    fn evaluate_quit_force_quits_even_when_dirty() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        let decision = state.evaluate_quit(true);

        assert_eq!(
            decision,
            QuitDecision::ForceQuit,
            "dirty 状態でも force=true なら ForceQuit であること"
        );
    }

    #[test]
    fn evaluate_quit_force_quits_when_clean() {
        let state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));

        let decision = state.evaluate_quit(true);

        assert_eq!(
            decision,
            QuitDecision::ForceQuit,
            "clean 状態でも force=true なら ForceQuit であること"
        );
    }

    // ---- タスク 5.5: 未保存状態での終了警告テスト ----

    #[test]
    fn dirty_state_triggers_warn_unsaved_on_normal_quit() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        let decision = state.evaluate_quit(false);

        assert_eq!(
            decision,
            QuitDecision::WarnUnsaved,
            "dirty 状態の通常終了は警告に切り替わること"
        );
    }

    #[test]
    fn warn_unsaved_does_not_terminate_session() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        let decision = state.evaluate_quit(false);
        assert_eq!(decision, QuitDecision::WarnUnsaved);

        // 警告後もセッション状態は維持される
        assert!(
            state.is_dirty(),
            "警告後も dirty 状態が維持されること（即時終了しない）"
        );

        // 警告後も保存要求を生成できる
        let request = state.build_save_request("content");
        assert!(request.is_ok(), "警告後も保存操作が可能であること");
    }

    #[test]
    fn force_quit_bypasses_unsaved_warning() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        // 通常終了では警告になる
        assert_eq!(state.evaluate_quit(false), QuitDecision::WarnUnsaved);

        // 強制終了では即時終了
        assert_eq!(
            state.evaluate_quit(true),
            QuitDecision::ForceQuit,
            "強制終了は未保存警告をバイパスすること"
        );
    }

    #[test]
    fn clean_state_after_save_allows_normal_quit() {
        let mut state = EditorSessionState::new(Some(PathBuf::from("/tmp/test.txt")));
        state.update_dirty(true);

        // 保存前は警告
        assert_eq!(state.evaluate_quit(false), QuitDecision::WarnUnsaved);

        // 保存成功
        state.record_save_success();

        // 保存後は通常終了可能
        assert_eq!(
            state.evaluate_quit(false),
            QuitDecision::Allow,
            "保存成功後は通常終了が許可されること"
        );
    }

    #[test]
    fn new_with_tab_size_preserves_requested_value() {
        let state = EditorSessionState::new_with_tab_size(None, 4);

        assert_eq!(state.tab_size(), 4);
    }

    #[test]
    fn new_with_tab_size_clamps_zero_to_one() {
        let state = EditorSessionState::new_with_tab_size(None, 0);

        assert_eq!(state.tab_size(), 1);
    }
}
