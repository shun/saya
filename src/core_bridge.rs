use std::collections::VecDeque;
use std::fmt;
use std::path::Path;

use vim_core_rs::{
    CoreCommandOutcome, CoreEvent, CoreHostAction, CoreMatchType, CoreMessageCategory,
    CoreMessageEvent, CoreMessageSeverity, CoreSearchHighlightMode, CoreSearchQueryError,
    CoreSessionError, CoreSnapshot, VimCoreSession,
};

use crate::search_capability::SearchCapabilityContract;
use crate::search_query::{
    SearchMatch, SearchMatchKind, SearchQueryMode, SearchStateError, SearchVisibleQuery,
    SearchVisibleRows, SearchVisibleState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualSelection {
    pub mode: vim_core_rs::CoreMode,
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

pub struct CoreBridge {
    session: VimCoreSession,
    pending_host_actions: VecDeque<CoreHostAction>,
    pending_messages: VecDeque<CoreMessageEvent>,
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
        let mut session = VimCoreSession::new(initial_text)?;
        configure_message_suppression(&mut session).map_err(CoreSessionError::CommandFailed)?;
        log::debug!("[core_bridge] vim-core-rs session initialized");
        Ok(Self {
            session,
            pending_host_actions: VecDeque::new(),
            pending_messages: VecDeque::new(),
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

    /// core がページスクロールの基準にする screen size を host 側で同期する。
    pub fn set_screen_size(&mut self, rows: i32, cols: i32) {
        log::debug!(
            "[core_bridge] setting screen size: rows={}, cols={}",
            rows,
            cols
        );
        self.session.set_screen_size(rows, cols);
    }

    pub fn take_pending_host_actions(&mut self) -> Vec<CoreHostAction> {
        self.drain_pending_host_actions_from_session();
        let actions = self.pending_host_actions.drain(..).collect::<Vec<_>>();
        log::debug!(
            "[core_bridge] drained pending host actions: count={}",
            actions.len()
        );
        actions
    }

    pub fn take_pending_messages(&mut self) -> Vec<CoreMessageEvent> {
        self.drain_pending_messages_from_session();
        let messages = self.pending_messages.drain(..).collect::<Vec<_>>();
        log::debug!(
            "[core_bridge] drained pending core messages: count={}",
            messages.len()
        );
        messages
    }

    /// キー入力を vim-core-rs セッションに適用する。
    /// ノーマルモードコマンドとして解釈し、結果を返す。
    pub fn dispatch_key(&mut self, key: &str) -> Result<CoreCommandOutcome, CoreSessionError> {
        log::debug!(
            "[core_bridge] dispatching key: {:?} (len={})",
            key,
            key.len()
        );
        let outcome = if self.should_handle_ctrl_c_interrupt(key) {
            self.handle_ctrl_c_interrupt()?
        } else {
            self.dispatch_session_key(key)?
        };
        log::debug!(
            "[core_bridge] dispatch result: {:?}, pending_input={:?}",
            outcome,
            self.session.snapshot().pending_input
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
            .execute_ex_command(command)
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&outcome);
        log::debug!("[core_bridge] ex command result: {:?}", outcome.outcome);
        Ok(outcome.outcome)
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
        let tx = self
            .session
            .execute_ex_command(&format!(":file {}", escaped_path))
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&tx);
        Ok(())
    }

    pub fn current_visual_selection(&mut self) -> Option<VisualSelection> {
        let snapshot = self.session.snapshot();
        if !is_visual_mode(snapshot.mode) {
            return None;
        }
        let current = (snapshot.cursor_row, snapshot.cursor_col);
        let first_swap = self
            .session
            .execute_normal_command("o")
            .map_err(CoreSessionError::CommandFailed)
            .ok()?;
        self.queue_transaction_artifacts(&first_swap);
        let swapped = self.session.snapshot();
        let anchor = (swapped.cursor_row, swapped.cursor_col);
        let second_swap = self
            .session
            .execute_normal_command("o")
            .map_err(CoreSessionError::CommandFailed)
            .ok()?;
        self.queue_transaction_artifacts(&second_swap);
        let ((start_row, start_col), (end_row, end_col)) =
            normalize_selection_bounds(anchor, current);
        Some(VisualSelection {
            mode: snapshot.mode,
            start_row,
            start_col,
            end_row,
            end_col,
        })
    }

    pub fn sync_search_input(
        &mut self,
        pattern: &str,
    ) -> Result<CoreCommandOutcome, CoreSessionError> {
        if self.search_prompt_is_active() {
            let _ = self.dispatch_session_key("\x1b")?;
        }

        if pattern.is_empty() {
            log::debug!("[core_bridge] search prompt synced with empty pattern");
            return Ok(CoreCommandOutcome::NoChange);
        }

        log::debug!(
            "[core_bridge] syncing search prompt through core-owned state: pattern={:?}",
            pattern
        );
        let tx = self
            .session
            .execute_normal_command(&format!("/{}", pattern))
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&tx);
        Ok(tx.outcome)
    }

    pub fn commit_search_input(
        &mut self,
        pattern: &str,
    ) -> Result<CoreCommandOutcome, CoreSessionError> {
        if self.search_prompt_is_active() {
            let _ = self.dispatch_session_key("\x1b")?;
        }

        let tx = self
            .session
            .execute_normal_command(&format!("/{}\r", pattern))
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&tx);
        Ok(tx.outcome)
    }

    pub fn cancel_search_input(&mut self) -> Result<CoreCommandOutcome, CoreSessionError> {
        if !self.search_prompt_is_active() {
            return Ok(CoreCommandOutcome::NoChange);
        }
        self.dispatch_session_key("\x1b")
    }

    pub fn search_capability_contract(&self) -> SearchCapabilityContract {
        let contract = VimCoreSession::search_capability_contract();
        let contract = SearchCapabilityContract {
            live_state_query_available: contract.live_state_query_available,
            visible_rows_only: contract.visible_rows_only,
            start_col_inclusive: contract.start_col_inclusive,
            end_col_exclusive: contract.end_col_exclusive,
        };
        log::debug!(
            "[core_bridge] search capability contract resolved: live_state_query_available={}, visible_rows_only={}, start_col_inclusive={}, end_col_exclusive={}",
            contract.live_state_query_available,
            contract.visible_rows_only,
            contract.start_col_inclusive,
            contract.end_col_exclusive
        );
        contract
    }

    pub fn query_visible_search_state(
        &mut self,
        query: SearchVisibleQuery,
    ) -> Result<SearchVisibleState, SearchStateError> {
        if query.start_row == 0 || query.end_row < query.start_row {
            log::debug!(
                "[core_bridge] rejecting search query because viewport is invalid: query={:?}",
                query
            );
            return Err(SearchStateError::InvalidViewport {
                start_row: query.start_row,
                end_row: query.end_row,
            });
        }

        let capability = self.search_capability_contract();
        let core_state = self
            .session
            .query_visible_search_state(query.start_row as i32, query.end_row as i32)
            .map_err(map_search_query_error)?;
        let matches = core_state
            .ranges
            .into_iter()
            .map(|range| SearchMatch {
                kind: map_match_kind(range.match_type),
                start_row: range.start_row,
                start_col: range.start_col,
                end_row: range.end_row,
                end_col: range.end_col,
            })
            .collect::<Vec<_>>();
        let mut matches = matches;
        matches.sort_by_key(|range| {
            let kind_rank = match range.kind {
                SearchMatchKind::Current => 0usize,
                SearchMatchKind::Incremental => 1usize,
                SearchMatchKind::Regular => 2usize,
            };
            (
                kind_rank,
                range.start_row,
                range.start_col,
                range.end_row,
                range.end_col,
            )
        });
        log::debug!(
            "[core_bridge] resolved visible search state: query={:?}, window_id={}, mode={:?}, hlsearch_enabled={}, hlsearch_suspended={}, incsearch_active={}, pattern={:?}, input_pattern={:?}, matches={}",
            query,
            core_state.window_id,
            core_state.mode,
            core_state.hlsearch_enabled,
            core_state.hlsearch_suspended,
            core_state.incsearch_active,
            core_state.pattern,
            core_state.input_pattern,
            matches.len(),
        );

        Ok(SearchVisibleState {
            capability,
            visible_rows: SearchVisibleRows {
                start_row: core_state.start_row,
                end_row: core_state.end_row,
            },
            window_id: core_state.window_id,
            mode: map_search_mode(core_state.mode),
            pattern: core_state.pattern,
            input_pattern: core_state.input_pattern,
            hlsearch_enabled: core_state.hlsearch_enabled,
            hlsearch_suspended: core_state.hlsearch_suspended,
            incsearch_active: core_state.incsearch_active,
            matches,
        })
    }
}

fn map_match_kind(match_type: CoreMatchType) -> SearchMatchKind {
    match match_type {
        CoreMatchType::Regular => SearchMatchKind::Regular,
        CoreMatchType::IncSearch => SearchMatchKind::Incremental,
        CoreMatchType::CurSearch => SearchMatchKind::Current,
    }
}

fn map_search_mode(mode: CoreSearchHighlightMode) -> SearchQueryMode {
    match mode {
        CoreSearchHighlightMode::Disabled => SearchQueryMode::Disabled,
        CoreSearchHighlightMode::HlSearch => SearchQueryMode::Hlsearch,
        CoreSearchHighlightMode::IncSearch => SearchQueryMode::IncsearchPreview,
    }
}

fn map_search_query_error(error: CoreSearchQueryError) -> SearchStateError {
    match error {
        CoreSearchQueryError::NoActiveWindow => SearchStateError::ActiveWindowMissing,
        CoreSearchQueryError::InvalidViewport { start_row, end_row } => {
            SearchStateError::InvalidViewport {
                start_row: start_row.max(0) as usize,
                end_row: end_row.max(0) as usize,
            }
        }
        CoreSearchQueryError::WindowNotFound { window_id } => {
            SearchStateError::WindowNotFound { window_id }
        }
    }
}

impl CoreBridge {
    fn search_prompt_is_active(&self) -> bool {
        self.session.get_search_input_pattern().is_some()
            || self.session.is_incsearch_active()
            || matches!(
                self.session.snapshot().mode,
                vim_core_rs::CoreMode::CommandLine
            )
    }

    fn should_handle_ctrl_c_interrupt(&self, key: &str) -> bool {
        key == "\u{3}"
            && matches!(
                self.session.snapshot().mode,
                vim_core_rs::CoreMode::Normal
                    | vim_core_rs::CoreMode::Visual
                    | vim_core_rs::CoreMode::VisualLine
                    | vim_core_rs::CoreMode::VisualBlock
            )
    }

    fn handle_ctrl_c_interrupt(&mut self) -> Result<CoreCommandOutcome, CoreSessionError> {
        let snapshot = self.session.snapshot();
        let has_pending_input = snapshot.pending_input.is_pending();
        log::debug!(
            "[core_bridge] handling ctrl-c interrupt: dirty={}, mode={:?}, has_pending_input={}",
            snapshot.dirty,
            snapshot.mode,
            has_pending_input
        );

        if has_pending_input {
            return self.dispatch_session_key("\x1b");
        }

        let content = if snapshot.dirty {
            "Type  :qa!  and press <Enter> to abandon all changes and exit Vim"
        } else {
            "Type  :qa  and press <Enter> to exit Vim"
        };
        self.pending_messages.push_back(CoreMessageEvent {
            severity: CoreMessageSeverity::Info,
            category: CoreMessageCategory::UserVisible,
            content: content.to_string(),
        });

        Ok(CoreCommandOutcome::NoChange)
    }

    fn dispatch_session_key(&mut self, key: &str) -> Result<CoreCommandOutcome, CoreSessionError> {
        let outcome = self
            .session
            .dispatch_key(key)
            .map_err(CoreSessionError::CommandFailed)?;
        self.queue_transaction_artifacts(&outcome);
        Ok(outcome.outcome)
    }

    fn queue_transaction_artifacts(&mut self, tx: &vim_core_rs::CoreCommandTransaction) {
        for action in &tx.host_actions {
            log::debug!(
                "[core_bridge] queued host action from transaction: {:?}",
                action
            );
            self.pending_host_actions.push_back(action.clone());
        }

        for event in &tx.events {
            if let CoreEvent::Message(message) = event {
                log::debug!(
                    "[core_bridge] queued core message from transaction: severity={:?}, category={:?}, content={:?}",
                    message.severity,
                    message.category,
                    message.content
                );
                self.pending_messages.push_back(message.clone());
            }
        }
    }

    fn drain_pending_host_actions_from_session(&mut self) {
        while let Some(action) = self.session.take_pending_host_action() {
            log::debug!(
                "[core_bridge] queued host action from pending session state: {:?}",
                action
            );
            self.pending_host_actions.push_back(action);
        }
    }

    fn drain_pending_messages_from_session(&mut self) {
        while let Some(event) = self.session.take_pending_event() {
            if let CoreEvent::Message(message) = event {
                log::debug!(
                    "[core_bridge] queued core message from pending session state: severity={:?}, category={:?}, content={:?}",
                    message.severity,
                    message.category,
                    message.content
                );
                self.pending_messages.push_back(message);
            }
        }
    }
}

fn configure_message_suppression(
    session: &mut VimCoreSession,
) -> Result<(), vim_core_rs::CoreCommandError> {
    log::debug!("[core_bridge] configuring Vim message suppression: report=999999, shortmess+=F");
    session.execute_ex_command(":set report=999999 shortmess+=F")?;
    Ok(())
}

fn is_visual_mode(mode: vim_core_rs::CoreMode) -> bool {
    matches!(
        mode,
        vim_core_rs::CoreMode::Visual
            | vim_core_rs::CoreMode::VisualLine
            | vim_core_rs::CoreMode::VisualBlock
    )
}

fn normalize_selection_bounds(
    anchor: (usize, usize),
    cursor: (usize, usize),
) -> ((usize, usize), (usize, usize)) {
    if anchor <= cursor {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    }
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
    use std::time::{SystemTime, UNIX_EPOCH};

    use vim_core_rs::{
        CoreCommandOutcome, CoreMessageCategory, CoreMessageSeverity, CoreMode, CorePendingInput,
    };

    use super::CoreBridge;

    use crate::session_guard::test_lock as session_test_lock;

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
    fn starts_with_no_pending_core_messages() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("buffer text").expect("core bridge should initialize");

        assert!(bridge.take_pending_messages().is_empty());
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

    #[test]
    fn message_handler_captures_echoerr_messages() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");

        bridge
            .apply_ex_command("echoerr 'test error message'")
            .expect("echoerr command should complete");
        let messages = bridge.take_pending_messages();

        assert!(
            messages.iter().any(|message| {
                message.severity == CoreMessageSeverity::Error
                    && message.category == CoreMessageCategory::UserVisible
                    && message.content.contains("test error message")
            }),
            "echoerr message should be queued: {:?}",
            messages
        );
    }

    #[test]
    fn dirty_ctrl_c_queues_upstream_exit_guidance_message() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("hello\n").expect("core bridge should initialize");
        bridge.dispatch_key("i").expect("insert mode");
        bridge.dispatch_key("X").expect("typed input");
        bridge.dispatch_key("\x1b").expect("normal mode");

        bridge
            .dispatch_key("\u{3}")
            .expect("ctrl-c should dispatch");
        let messages = bridge.take_pending_messages();

        assert!(
            messages
                .iter()
                .any(|message| message.content.contains(":qa!")),
            "ctrl-c guidance should mention :qa!: {:?}",
            messages
        );
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
    fn sequential_multi_key_pending_input_is_forwarded_through_core_dispatch() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");

        let first = bridge
            .dispatch_key("y")
            .expect("first key should be forwarded to core");
        assert_eq!(first, CoreCommandOutcome::NoChange);
        assert_eq!(
            bridge.snapshot().pending_input.pending_keys,
            "y",
            "pending input state should come from vim-core-rs"
        );

        bridge
            .dispatch_key("y")
            .expect("second key should complete the sequence");

        let snapshot = bridge.snapshot();
        assert_eq!(
            snapshot.text, "first\nsecond\nthird\n",
            "yy itself should not change the buffer"
        );
        assert_eq!(
            snapshot.pending_input,
            CorePendingInput::none(),
            "completed sequence should leave no bridge-side pending parser state"
        );
    }

    #[test]
    fn ctrl_c_cancels_core_owned_pending_input_without_showing_exit_guidance() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("first\nsecond\n").expect("core bridge should initialize");

        bridge.dispatch_key("d").expect("enter operator pending");
        assert!(
            bridge.snapshot().pending_input.is_pending(),
            "pending input should be reported by vim-core-rs before ctrl-c"
        );

        let outcome = bridge
            .dispatch_key("\u{3}")
            .expect("ctrl-c should cancel pending input");
        assert_eq!(outcome, CoreCommandOutcome::NoChange);
        assert_eq!(
            bridge.snapshot().pending_input,
            CorePendingInput::none(),
            "ctrl-c should clear core pending input via escape dispatch"
        );
        assert!(
            bridge.take_pending_messages().is_empty(),
            "canceling pending input should not enqueue exit guidance"
        );
    }

    #[test]
    fn new_configures_high_report_threshold_to_suppress_bulk_edit_messages() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge =
            CoreBridge::new("first\nsecond\nthird\n").expect("core bridge should initialize");
        assert!(
            bridge
                .session
                .eval_string("&report")
                .as_deref()
                .is_some_and(|report| report.trim() == "999999"),
            "複数行操作の報告メッセージ抑制のため report が引き上げられていること"
        );
    }

    #[test]
    fn sequential_dd_deletes_current_line_via_core_owned_pending_input() {
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
    fn insert_mode_keeps_literal_text_literal_at_bridge_boundary() {
        let _lock = session_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut bridge = CoreBridge::new("").expect("core bridge should initialize");

        bridge.dispatch_key("i").expect("enter insert mode");
        bridge
            .dispatch_key("2")
            .expect("insert literal count digit");
        bridge
            .dispatch_key("d")
            .expect("insert literal operator key");
        bridge
            .dispatch_key("g")
            .expect("insert literal normal prefix key");
        bridge.dispatch_key("\x1b").expect("leave insert mode");

        let snapshot = bridge.snapshot();
        assert_eq!(snapshot.mode, CoreMode::Normal);
        assert_eq!(snapshot.text, "2dg\n");
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
