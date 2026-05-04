/// エディタセッションの保存・終了を統括するモジュール。
///
/// CoreBridge から取得した buffer 情報と対象パスを組み合わせて、
/// 保存要求の生成、保存結果の反映、終了判定を行う。
use std::path::PathBuf;

use crate::host_io::SaveRequest;
use crate::option_registry::{SayaOptionName, SayaOptionValue};

/// 保存要求の生成に失敗した理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveRequestError {
    /// 保存先パスが未設定
    NoTargetPath,
    /// read-only 起動のため保存不可
    ReadOnly,
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
    /// 行番号欄の最小幅
    number_width: u16,
    relative_number: bool,
    cursorline: bool,
    scrolloff: u16,
    sidescrolloff: u16,
    wrap: bool,
    laststatus: u8,
    list: bool,
    listchars: String,
    markdown_render: bool,
    foldmethod: String,
    foldlevel: u16,
    /// read-only 起動かどうか
    read_only: bool,
    /// 現在 dirty 状態かどうか
    dirty: bool,
    /// 直近の保存失敗メッセージ
    last_save_error: Option<String>,
}

impl EditorSessionState {
    /// 新しいセッション状態を作成する。
    pub fn new(target_path: Option<PathBuf>) -> Self {
        Self::new_with_tab_size_and_line_numbers_and_number_width(target_path, 8, false, 4)
    }

    /// タブ幅を指定して新しいセッション状態を作成する。
    pub fn new_with_tab_size(target_path: Option<PathBuf>, tab_size: u16) -> Self {
        Self::new_with_tab_size_and_line_numbers_and_number_width(target_path, tab_size, false, 4)
    }

    /// タブ幅と行番号表示を指定して新しいセッション状態を作成する。
    pub fn new_with_tab_size_and_line_numbers(
        target_path: Option<PathBuf>,
        tab_size: u16,
        line_numbers: bool,
    ) -> Self {
        Self::new_with_tab_size_and_line_numbers_and_number_width(
            target_path,
            tab_size,
            line_numbers,
            4,
        )
    }

    /// タブ幅、行番号表示、行番号欄幅を指定して新しいセッション状態を作成する。
    pub fn new_with_tab_size_and_line_numbers_and_number_width(
        target_path: Option<PathBuf>,
        tab_size: u16,
        line_numbers: bool,
        number_width: u16,
    ) -> Self {
        Self::new_with_options(target_path, tab_size, line_numbers, number_width, false)
    }

    /// タブ幅、行番号表示、行番号欄幅、read-only を指定して新しいセッション状態を作成する。
    pub fn new_with_options(
        target_path: Option<PathBuf>,
        tab_size: u16,
        line_numbers: bool,
        number_width: u16,
        read_only: bool,
    ) -> Self {
        let tab_size = tab_size.max(1);
        let number_width = number_width.max(1);
        log::debug!(
            "[editor_session] new session state: target_path={:?}, tab_size={}, line_numbers={}, number_width={}, read_only={}",
            target_path,
            tab_size,
            line_numbers,
            number_width,
            read_only
        );
        Self {
            target_path,
            tab_size,
            line_numbers,
            number_width,
            relative_number: false,
            cursorline: false,
            scrolloff: 0,
            sidescrolloff: 0,
            wrap: true,
            laststatus: 2,
            list: false,
            listchars: "tab:>-,trail:-".to_string(),
            markdown_render: true,
            foldmethod: "manual".to_string(),
            foldlevel: 0,
            read_only,
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
            "[editor_session] building save request: target_path={:?}, contents_len={}, read_only={}",
            self.target_path,
            buffer_contents.len(),
            self.read_only
        );
        if self.read_only {
            log::debug!("[editor_session] save request failed: session is read-only");
            return Err(SaveRequestError::ReadOnly);
        }
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

    /// 行番号欄の最小幅を返す。
    pub fn number_width(&self) -> u16 {
        self.number_width
    }

    pub fn relative_number(&self) -> bool {
        self.relative_number
    }

    pub fn cursorline(&self) -> bool {
        self.cursorline
    }

    pub fn scrolloff(&self) -> u16 {
        self.scrolloff
    }

    pub fn sidescrolloff(&self) -> u16 {
        self.sidescrolloff
    }

    pub fn wrap(&self) -> bool {
        self.wrap
    }

    pub fn laststatus(&self) -> u8 {
        self.laststatus
    }

    pub fn list(&self) -> bool {
        self.list
    }

    pub fn listchars(&self) -> &str {
        &self.listchars
    }

    pub fn markdown_render(&self) -> bool {
        self.markdown_render
    }

    pub fn foldmethod(&self) -> &str {
        &self.foldmethod
    }

    pub fn foldlevel(&self) -> u16 {
        self.foldlevel
    }

    /// 行番号表示の有効/無効を更新する。
    pub fn set_line_numbers(&mut self, enabled: bool) {
        log::debug!(
            "[editor_session] line number visibility updated: {} -> {}",
            self.line_numbers,
            enabled
        );
        self.line_numbers = enabled;
    }

    /// 行番号欄の最小幅を更新する。
    pub fn set_number_width(&mut self, width: u16) {
        let width = width.max(1);
        log::debug!(
            "[editor_session] number width updated: {} -> {}",
            self.number_width,
            width
        );
        self.number_width = width;
    }

    pub fn apply_presentation_option(
        &mut self,
        name: SayaOptionName,
        value: SayaOptionValue,
    ) -> Result<(), String> {
        log::debug!(
            "[editor_session] applying presentation option: name={}, value={:?}",
            name,
            value
        );
        match (name, value) {
            (SayaOptionName::LineNumbers, SayaOptionValue::Boolean(value)) => {
                self.set_line_numbers(value);
                Ok(())
            }
            (SayaOptionName::RelativeNumber, SayaOptionValue::Boolean(value)) => {
                self.relative_number = value;
                Ok(())
            }
            (SayaOptionName::CursorLine, SayaOptionValue::Boolean(value)) => {
                self.cursorline = value;
                Ok(())
            }
            (SayaOptionName::ScrollOff, SayaOptionValue::Number(value)) => {
                self.scrolloff = u16::try_from(value.max(0)).unwrap_or(u16::MAX);
                Ok(())
            }
            (SayaOptionName::SidescrollOff, SayaOptionValue::Number(value)) => {
                self.sidescrolloff = u16::try_from(value.max(0)).unwrap_or(u16::MAX);
                Ok(())
            }
            (SayaOptionName::Wrap, SayaOptionValue::Boolean(value)) => {
                self.wrap = value;
                Ok(())
            }
            (SayaOptionName::NumberWidth, SayaOptionValue::Number(value)) => {
                self.set_number_width(u16::try_from(value.max(1)).unwrap_or(u16::MAX));
                Ok(())
            }
            (SayaOptionName::LastStatus, SayaOptionValue::Number(value)) => {
                self.laststatus = u8::try_from(value.clamp(0, 3)).unwrap_or(2);
                Ok(())
            }
            (SayaOptionName::List, SayaOptionValue::Boolean(value)) => {
                self.list = value;
                Ok(())
            }
            (SayaOptionName::ListChars, SayaOptionValue::String(value)) => {
                self.listchars = value;
                Ok(())
            }
            (SayaOptionName::MarkdownRender, SayaOptionValue::Boolean(value)) => {
                log::debug!(
                    "[editor_session] markdown render projection updated: {} -> {}",
                    self.markdown_render,
                    value
                );
                self.markdown_render = value;
                Ok(())
            }
            (SayaOptionName::FoldMethod, SayaOptionValue::String(value)) => {
                self.foldmethod = value;
                Ok(())
            }
            (SayaOptionName::FoldLevel, SayaOptionValue::Number(value)) => {
                self.foldlevel = u16::try_from(value.max(0)).unwrap_or(u16::MAX);
                Ok(())
            }
            (name, value) => Err(format!(
                "presentation option type mismatch: name={name}, value={value:?}"
            )),
        }
    }

    pub fn read_only(&self) -> bool {
        self.read_only
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

    #[test]
    fn new_with_number_width_defaults_to_four() {
        let state = EditorSessionState::new(None);

        assert_eq!(state.number_width(), 4);
    }

    #[test]
    fn set_line_numbers_enables_projection_flag() {
        let mut state = EditorSessionState::new(None);
        assert!(!state.line_numbers(), "既定値は false であること");

        state.set_line_numbers(true);

        assert!(state.line_numbers(), "行番号表示が有効になること");
    }

    #[test]
    fn set_line_numbers_disables_projection_flag() {
        let mut state = EditorSessionState::new_with_tab_size_and_line_numbers(None, 8, true);
        assert!(state.line_numbers(), "初期状態は true であること");

        state.set_line_numbers(false);

        assert!(!state.line_numbers(), "行番号表示が無効になること");
    }

    #[test]
    fn set_number_width_updates_projection_width() {
        let mut state = EditorSessionState::new(None);

        state.set_number_width(6);

        assert_eq!(state.number_width(), 6);
    }

    #[test]
    fn set_number_width_clamps_zero_to_one() {
        let mut state = EditorSessionState::new(None);

        state.set_number_width(0);

        assert_eq!(state.number_width(), 1);
    }

    #[test]
    fn build_save_request_fails_when_session_is_read_only() {
        let state = EditorSessionState::new_with_options(None, 8, false, 4, true);

        let result = state.build_save_request("content");

        assert_eq!(result, Err(SaveRequestError::ReadOnly));
        assert!(state.read_only());
    }
}
