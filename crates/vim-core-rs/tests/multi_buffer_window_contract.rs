use std::sync::{Mutex, OnceLock};
use vim_core_rs::{CoreBufferInfo, CoreCommandError, CoreEvent, CoreWindowInfo, VimCoreSession};

fn session_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_session_test_lock() -> std::sync::MutexGuard<'static, ()> {
    session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn drain_events(session: &mut VimCoreSession) -> Vec<CoreEvent> {
    let mut events = Vec::new();
    while let Some(event) = session.take_pending_event() {
        events.push(event);
    }
    events
}

// =============================================================================
// Task 1.1: FFI 構造体の定義確認テスト
// バッファおよびウィンドウ情報の FFI 構造体が正しく定義されていること
// =============================================================================

#[test]
fn snapshot_contains_buffer_list_with_at_least_one_entry() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("hello world").expect("session should initialize");
    let snapshot = session.snapshot();

    // 初期状態では少なくとも1つのバッファが存在する
    assert!(
        !snapshot.buffers.is_empty(),
        "初期状態でバッファリストは空であってはならない"
    );

    // 最初のバッファはアクティブであること
    let active_buf = snapshot.buffers.iter().find(|b| b.is_active);
    assert!(
        active_buf.is_some(),
        "少なくとも1つのアクティブバッファが存在すること"
    );

    // バッファIDは正の値であること（Vimの b_fnum は 1-based）
    let first_buf = &snapshot.buffers[0];
    assert!(
        first_buf.id > 0,
        "バッファIDは正の値であること: got {}",
        first_buf.id
    );
}

#[test]
fn snapshot_contains_window_list_with_at_least_one_entry() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("hello world").expect("session should initialize");
    let snapshot = session.snapshot();

    // 初期状態では少なくとも1つのウィンドウが存在する
    assert!(
        !snapshot.windows.is_empty(),
        "初期状態でウィンドウリストは空であってはならない"
    );

    // 最初のウィンドウはアクティブであること
    let active_win = snapshot.windows.iter().find(|w| w.is_active);
    assert!(
        active_win.is_some(),
        "少なくとも1つのアクティブウィンドウが存在すること"
    );

    // ウィンドウIDは正の値であること（Vimの w_id は正）
    let first_win = &snapshot.windows[0];
    assert!(
        first_win.id > 0,
        "ウィンドウIDは正の値であること: got {}",
        first_win.id
    );

    // ウィンドウはバッファを表示していること
    assert!(
        first_win.buf_id > 0,
        "ウィンドウのbuf_idは正の値であること: got {}",
        first_win.buf_id
    );
}

#[test]
fn buffer_info_contains_name_and_dirty_flag() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("initial text").expect("session should initialize");

    // 初期状態ではdirtyフラグはfalse
    let snapshot = session.snapshot();
    let active_buf = snapshot
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("アクティブバッファが存在すること");
    assert!(
        !active_buf.dirty,
        "初期状態ではdirtyフラグはfalseであること"
    );

    // バッファを変更するとdirtyフラグがtrueになる
    session
        .execute_normal_command("dd")
        .expect("dd should succeed");
    let snapshot = session.snapshot();
    let active_buf = snapshot
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("アクティブバッファが存在すること");
    assert!(
        active_buf.dirty,
        "バッファ変更後はdirtyフラグがtrueであること"
    );
}

#[test]
fn buffer_info_exposes_buffer_local_source_revision() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer one").expect("session should initialize");

    let initial_snapshot = session.snapshot();
    let first_buffer = initial_snapshot
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("アクティブバッファが存在すること");
    let first_buffer_id = first_buffer.id;
    let first_initial_revision = first_buffer.source_revision;

    let repeated_snapshot = session.snapshot();
    let repeated_first_revision = repeated_snapshot
        .buffers
        .iter()
        .find(|b| b.id == first_buffer_id)
        .expect("最初のバッファが存在すること")
        .source_revision;
    assert_eq!(
        repeated_first_revision, first_initial_revision,
        "変更がないスナップショット取得では source_revision は進まないこと"
    );

    session
        .execute_ex_command(":enew")
        .expect("enew should succeed");
    let second_snapshot = session.snapshot();
    let first_after_new_buffer_revision = second_snapshot
        .buffers
        .iter()
        .find(|b| b.id == first_buffer_id)
        .expect("最初のバッファが存在すること")
        .source_revision;
    let second_buffer = second_snapshot
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("2つ目のアクティブバッファが存在すること");
    let second_buffer_id = second_buffer.id;
    let second_initial_revision = second_buffer.source_revision;

    session
        .execute_ex_command("call setline(1, 'buffer two changed')")
        .expect("setline should change the second buffer");
    let edited_snapshot = session.snapshot();
    let first_after_second_edit = edited_snapshot
        .buffers
        .iter()
        .find(|b| b.id == first_buffer_id)
        .expect("最初のバッファが存在すること");
    let second_after_edit = edited_snapshot
        .buffers
        .iter()
        .find(|b| b.id == second_buffer_id)
        .expect("2つ目のバッファが存在すること");

    assert_eq!(
        first_after_second_edit.source_revision, first_after_new_buffer_revision,
        "別バッファの編集では source_revision は進まないこと"
    );
    assert!(
        second_after_edit.source_revision > second_initial_revision,
        "編集したバッファの source_revision は前進すること"
    );
}

#[test]
fn window_info_contains_geometry() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("hello world").expect("session should initialize");
    let snapshot = session.snapshot();

    let win = &snapshot.windows[0];
    // ウィンドウの幾何情報が取得できること（サイズは正の値）
    // ヘッドレス環境では小さいかもしれないが、0以上であること
    // width/height は usize なので常に >= 0。
    // ヘッドレス環境でも値が取得できていることだけ確認する。
    assert!(
        win.width > 0,
        "ウィンドウ幅は正であること: got {}",
        win.width
    );
    assert!(
        win.height > 0,
        "ウィンドウ高さは正であること: got {}",
        win.height
    );
}

// =============================================================================
// Task 1.2: バッファとウィンドウの一覧取得ロジック
// =============================================================================

#[test]
fn window_references_correct_buffer() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("hello world").expect("session should initialize");
    let snapshot = session.snapshot();

    // ウィンドウが表示しているバッファIDがバッファリストに存在すること
    let win = &snapshot.windows[0];
    let buf_exists = snapshot.buffers.iter().any(|b| b.id == win.buf_id);
    assert!(
        buf_exists,
        "ウィンドウのbuf_id({})がバッファリストに存在すること",
        win.buf_id
    );
}

// =============================================================================
// Task 1.3: バッファおよびウィンドウ操作の Bridge 関数
// =============================================================================

#[test]
fn switch_to_buffer_changes_active_buffer() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer one").expect("session should initialize");

    // 新しいバッファを作成
    session
        .execute_ex_command(":enew")
        .expect("enew should succeed");

    let snapshot = session.snapshot();
    assert!(
        snapshot.buffers.len() >= 2,
        ":enew 後にバッファが2つ以上存在すること: got {}",
        snapshot.buffers.len()
    );

    // 最初のバッファに切り替え
    let first_buf_id = snapshot
        .buffers
        .iter()
        .find(|b| !b.is_active)
        .expect("非アクティブバッファが存在すること")
        .id;

    session
        .switch_to_buffer(first_buf_id)
        .expect("switch_to_buffer should succeed");

    let snapshot = session.snapshot();
    let active_buf = snapshot
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("アクティブバッファが存在すること");
    assert_eq!(
        active_buf.id, first_buf_id,
        "switch_to_buffer後にアクティブバッファが変更されること"
    );
}

#[test]
fn switch_to_invalid_buffer_returns_error() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let result = session.switch_to_buffer(99999);
    assert!(
        result.is_err(),
        "存在しないバッファIDへの切り替えはエラーを返すこと"
    );
}

#[test]
fn switch_to_window_changes_active_window() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("window test").expect("session should initialize");

    // ヘッドレス環境ではスクリーンサイズを設定しないとE36エラーになる
    session.set_screen_size(24, 80);

    // ウィンドウを分割
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let snapshot = session.snapshot();
    assert!(
        snapshot.windows.len() >= 2,
        ":split 後にウィンドウが2つ以上存在すること: got {}",
        snapshot.windows.len()
    );

    // 非アクティブウィンドウに切り替え
    let inactive_win_id = snapshot
        .windows
        .iter()
        .find(|w| !w.is_active)
        .expect("非アクティブウィンドウが存在すること")
        .id;

    session
        .switch_to_window(inactive_win_id)
        .expect("switch_to_window should succeed");

    let snapshot = session.snapshot();
    let active_win = snapshot
        .windows
        .iter()
        .find(|w| w.is_active)
        .expect("アクティブウィンドウが存在すること");
    assert_eq!(
        active_win.id, inactive_win_id,
        "switch_to_window後にアクティブウィンドウが変更されること"
    );
}

#[test]
fn switch_to_invalid_window_returns_error() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer").expect("session should initialize");

    let result = session.switch_to_window(99999);
    assert!(
        result.is_err(),
        "存在しないウィンドウIDへの切り替えはエラーを返すこと"
    );
}

#[test]
fn buffer_text_returns_content_of_specific_buffer() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("original buffer text").expect("session should initialize");

    // バッファIDを取得
    let snapshot = session.snapshot();
    let buf_id = snapshot
        .buffers
        .iter()
        .find(|b| b.is_active)
        .expect("アクティブバッファが存在すること")
        .id;

    // 特定バッファのテキストを取得
    let text = session.buffer_text(buf_id);
    assert!(
        text.is_some(),
        "存在するバッファIDに対してテキストが取得できること"
    );
    assert_eq!(
        text.unwrap(),
        "original buffer text",
        "バッファテキストが正しく取得できること"
    );
}

#[test]
fn buffer_text_returns_none_for_invalid_buffer() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("buffer").expect("session should initialize");

    let text = session.buffer_text(99999);
    assert!(text.is_none(), "存在しないバッファIDに対してNoneを返すこと");
}

#[test]
fn split_creates_additional_window() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("split test").expect("session should initialize");

    // ヘッドレス環境ではスクリーンサイズを設定しないとE36エラーになる
    session.set_screen_size(24, 80);

    let initial_windows = session.snapshot().windows.len();

    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let after_split = session.snapshot().windows.len();
    assert_eq!(
        after_split,
        initial_windows + 1,
        ":split後にウィンドウ数が1つ増えること"
    );
}

#[test]
fn vsplit_creates_additional_window() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("vsplit test").expect("session should initialize");

    // ヘッドレス環境ではスクリーンサイズを設定しないとE36エラーになる
    session.set_screen_size(24, 80);

    let initial_windows = session.snapshot().windows.len();

    session
        .execute_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let after_vsplit = session.snapshot().windows.len();
    assert_eq!(
        after_vsplit,
        initial_windows + 1,
        ":vsplit後にウィンドウ数が1つ増えること"
    );
}

#[test]
fn split_and_vsplit_preserve_window_cursor_state() {
    let _guard = acquire_session_test_lock();

    for command in [":split", ":vsplit"] {
        let mut session = VimCoreSession::new("alphaaaaa\nbetaBBBBBB\ncharlieCCC")
            .expect("session should initialize");
        session.set_screen_size(24, 80);
        session
            .execute_normal_command("2G4l")
            .expect("cursor positioning should succeed");

        let before = session.snapshot();
        let expected_cursor = (before.cursor_row, before.cursor_col);

        session
            .execute_ex_command(command)
            .expect("split command should succeed");

        let after = session.snapshot();
        assert_eq!(
            after.windows.len(),
            2,
            "{command} should create two windows"
        );
        for window in &after.windows {
            assert_eq!(
                window_cursor(window),
                expected_cursor,
                "{command} should preserve the cursor position in each window"
            );
        }
        assert_eq!(
            window_cursor(&active_window(&session)),
            expected_cursor,
            "{command} should keep the active window cursor in sync with the snapshot"
        );
    }
}

fn send_ctrl_w_command(session: &mut VimCoreSession, suffix: &str) {
    session
        .execute_normal_command(&format!("\x17{suffix}"))
        .expect("Ctrl-W command should succeed");
}

fn active_window(session: &VimCoreSession) -> CoreWindowInfo {
    session
        .windows()
        .into_iter()
        .find(|window| window.is_active)
        .expect("active window should exist")
}

fn window_cursor(window: &CoreWindowInfo) -> (usize, usize) {
    (window.cursor_row, window.cursor_col)
}

#[test]
fn snapshot_window_helpers_resolve_active_and_target_windows_without_host_guessing() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("helper contract").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let snapshot = session.snapshot();
    let active_window = snapshot
        .active_window()
        .expect("active window helper should resolve the active window");
    let active_window_id = snapshot
        .active_window_id()
        .expect("active window id helper should resolve the active window id");

    assert!(active_window.is_active);
    assert_eq!(active_window.id, active_window_id);
    assert_eq!(
        snapshot.window(active_window_id).map(|window| window.id),
        Some(active_window_id),
        "window lookup should resolve by canonical window_id"
    );
    assert!(
        snapshot.window(999_999).is_none(),
        "unknown window ids should not resolve through snapshot helpers"
    );
}

#[test]
fn ctrl_w_h_and_l_move_between_horizontal_neighbors() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("ctrl-w h/l").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let windows = session.windows();
    assert_eq!(windows.len(), 2, "vsplit should create two windows");
    let left_id = windows
        .iter()
        .min_by_key(|window| window.col)
        .expect("left window should exist")
        .id;
    let right_id = windows
        .iter()
        .max_by_key(|window| window.col)
        .expect("right window should exist")
        .id;

    session
        .switch_to_window(left_id)
        .expect("switch to left window should succeed");
    send_ctrl_w_command(&mut session, "l");
    assert_eq!(
        active_window(&session).id,
        right_id,
        "Ctrl-w l should move to the window on the right"
    );

    send_ctrl_w_command(&mut session, "h");
    assert_eq!(
        active_window(&session).id,
        left_id,
        "Ctrl-w h should move back to the window on the left"
    );
}

#[test]
fn ctrl_w_j_and_k_move_between_vertical_neighbors() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("ctrl-w j/k").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let windows = session.windows();
    assert_eq!(windows.len(), 2, "split should create two windows");
    let top_id = windows
        .iter()
        .min_by_key(|window| window.row)
        .expect("top window should exist")
        .id;
    let bottom_id = windows
        .iter()
        .max_by_key(|window| window.row)
        .expect("bottom window should exist")
        .id;

    session
        .switch_to_window(top_id)
        .expect("switch to top window should succeed");
    send_ctrl_w_command(&mut session, "j");
    assert_eq!(
        active_window(&session).id,
        bottom_id,
        "Ctrl-w j should move to the window below"
    );

    send_ctrl_w_command(&mut session, "k");
    assert_eq!(
        active_window(&session).id,
        top_id,
        "Ctrl-w k should move back to the window above"
    );
}

#[test]
fn ctrl_w_c_closes_the_active_window() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("ctrl-w c").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let before = session.windows().len();
    assert_eq!(before, 2, "split should create two windows");

    send_ctrl_w_command(&mut session, "c");

    let after = session.windows();
    assert_eq!(after.len(), 1, "Ctrl-w c should close one window");
    assert!(
        after.iter().any(|window| window.is_active),
        "one remaining window should stay active"
    );
}

#[test]
fn ctrl_w_equalize_restores_balanced_horizontal_layout() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("ctrl-w =").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let windows = session.windows();
    let left_id = windows
        .iter()
        .min_by_key(|window| window.col)
        .expect("left window should exist")
        .id;

    session
        .switch_to_window(left_id)
        .expect("switch to left window should succeed");
    session
        .execute_ex_command(":vertical resize 50")
        .expect("vertical resize should succeed");

    let before = session.windows();
    let active_before = before
        .iter()
        .find(|window| window.is_active)
        .expect("active window should exist");
    let other_before = before
        .iter()
        .find(|window| !window.is_active)
        .expect("inactive window should exist");
    assert!(
        active_before.width != other_before.width,
        "test setup should create an uneven vertical layout"
    );

    send_ctrl_w_command(&mut session, "=");

    let after = session.windows();
    let active_after = after
        .iter()
        .find(|window| window.is_active)
        .expect("active window should exist after equalize");
    let other_after = after
        .iter()
        .find(|window| !window.is_active)
        .expect("inactive window should exist after equalize");
    assert!(
        active_after.width.abs_diff(other_after.width) <= 1,
        "Ctrl-w = should equalize widths: active={}, other={}",
        active_after.width,
        other_after.width
    );
}

#[test]
fn ctrl_w_focus_changes_expose_window_local_cursor_state() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("alphaaaaa\nbetaBBBBBB\ncharlieCCC")
        .expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let windows = session.windows();
    let left_id = windows
        .iter()
        .min_by_key(|window| window.col)
        .expect("left window should exist")
        .id;
    let right_id = windows
        .iter()
        .max_by_key(|window| window.col)
        .expect("right window should exist")
        .id;

    session
        .switch_to_window(left_id)
        .expect("switch to left window should succeed");
    session
        .execute_normal_command("2G3l")
        .expect("cursor movement in left window should succeed");
    let left_cursor = window_cursor(&active_window(&session));

    session
        .switch_to_window(right_id)
        .expect("switch to right window should succeed");
    session
        .execute_normal_command("G0")
        .expect("cursor movement in right window should succeed");
    let right_cursor = window_cursor(&active_window(&session));

    session
        .switch_to_window(left_id)
        .expect("switch back to left window should succeed");
    assert_eq!(
        window_cursor(&active_window(&session)),
        left_cursor,
        "active snapshot cursor should follow the left window"
    );

    send_ctrl_w_command(&mut session, "l");
    assert_eq!(
        active_window(&session).id,
        right_id,
        "Ctrl-w l should move focus to the right window"
    );
    assert_eq!(
        window_cursor(&active_window(&session)),
        right_cursor,
        "Ctrl-w l should expose the right window's cursor state"
    );
}

#[test]
fn ctrl_w_plus_and_minus_resize_the_active_window_height() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("ctrl-w height resize").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let before = active_window(&session);

    send_ctrl_w_command(&mut session, "+");
    let after_plus = active_window(&session);
    assert!(
        after_plus.height > before.height,
        "Ctrl-w + should increase the active window height: before={}, after={}",
        before.height,
        after_plus.height
    );

    send_ctrl_w_command(&mut session, "-");
    let after_minus = active_window(&session);
    assert!(
        after_minus.height <= after_plus.height,
        "Ctrl-w - should not increase the active window height: after_plus={}, after_minus={}",
        after_plus.height,
        after_minus.height
    );
}

#[test]
fn ctrl_w_angle_brackets_resize_the_active_window_width() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("ctrl-w width resize").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let before = active_window(&session);

    send_ctrl_w_command(&mut session, ">");
    let after_right = active_window(&session);
    assert!(
        after_right.width > before.width,
        "Ctrl-w > should increase the active window width: before={}, after={}",
        before.width,
        after_right.width
    );

    send_ctrl_w_command(&mut session, "<");
    let after_left = active_window(&session);
    assert!(
        after_left.width <= after_right.width,
        "Ctrl-w < should not increase the active window width: after_right={}, after_left={}",
        after_right.width,
        after_left.width
    );
}

#[test]
fn ctrl_w_h_and_l_move_the_active_window_to_horizontal_edges() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("ctrl-w H/L").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let windows = session.windows();
    let left_id = windows
        .iter()
        .min_by_key(|window| window.col)
        .expect("left window should exist")
        .id;
    let right_id = windows
        .iter()
        .max_by_key(|window| window.col)
        .expect("right window should exist")
        .id;

    session
        .switch_to_window(right_id)
        .expect("switch to right window should succeed");

    send_ctrl_w_command(&mut session, "H");
    let after_h = active_window(&session);
    assert_eq!(
        after_h.id, right_id,
        "Ctrl-w H should keep the same window active while moving it to the left edge"
    );
    assert_eq!(
        after_h.col, 0,
        "Ctrl-w H should place the active window at the left edge"
    );
    assert!(
        session
            .windows()
            .iter()
            .find(|window| window.id == left_id)
            .map(|window| window.col > after_h.col)
            .unwrap_or(false),
        "the former left window should remain to the right of the moved window"
    );

    send_ctrl_w_command(&mut session, "L");
    let after_l = active_window(&session);
    let rightmost_col = session
        .windows()
        .iter()
        .map(|window| window.col)
        .max()
        .expect("rightmost window should exist");
    assert_eq!(
        after_l.id, right_id,
        "Ctrl-w L should keep the same window active while moving it to the right edge"
    );
    assert_eq!(
        after_l.col, rightmost_col,
        "Ctrl-w L should place the active window at the right edge"
    );
}

#[test]
fn ctrl_w_j_and_k_move_the_active_window_to_vertical_edges() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("ctrl-w J/K").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let windows = session.windows();
    let top_id = windows
        .iter()
        .min_by_key(|window| window.row)
        .expect("top window should exist")
        .id;
    let bottom_id = windows
        .iter()
        .max_by_key(|window| window.row)
        .expect("bottom window should exist")
        .id;

    session
        .switch_to_window(bottom_id)
        .expect("switch to bottom window should succeed");

    send_ctrl_w_command(&mut session, "K");
    let after_k = active_window(&session);
    assert_eq!(
        after_k.id, bottom_id,
        "Ctrl-w K should keep the same window active while moving it to the top edge"
    );
    assert_eq!(
        after_k.row, 0,
        "Ctrl-w K should place the active window at the top edge"
    );
    assert!(
        session
            .windows()
            .iter()
            .find(|window| window.id == top_id)
            .map(|window| window.row > after_k.row)
            .unwrap_or(false),
        "the former top window should remain below the moved window"
    );

    send_ctrl_w_command(&mut session, "J");
    let after_j = active_window(&session);
    let bottommost_row = session
        .windows()
        .iter()
        .map(|window| window.row)
        .max()
        .expect("bottommost window should exist");
    assert_eq!(
        after_j.id, bottom_id,
        "Ctrl-w J should keep the same window active while moving it to the bottom edge"
    );
    assert_eq!(
        after_j.row, bottommost_row,
        "Ctrl-w J should place the active window at the bottom edge"
    );
}

#[test]
fn query_visible_search_state_accepts_inactive_window_ids() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello\nworld\n").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let inactive_window_id = session
        .windows()
        .iter()
        .find(|window| !window.is_active)
        .expect("inactive window should exist")
        .id;

    let state = session
        .query_visible_search_state_for_window(inactive_window_id, 1, 2)
        .expect("inactive window should be queryable");

    assert_eq!(state.window_id, inactive_window_id);
    assert_eq!(state.mode, vim_core_rs::CoreSearchHighlightMode::Disabled);
    assert!(state.ranges.is_empty());
}

#[test]
fn window_id_stays_canonical_across_resize_move_and_close() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("window identity contract").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":split")
        .expect("split should succeed");
    session
        .execute_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let initial_snapshot = session.snapshot();
    let tracked_window_id = initial_snapshot
        .active_window_id()
        .expect("active window id should exist");
    let tracked_window_before = initial_snapshot
        .window(tracked_window_id)
        .expect("tracked window should exist before layout changes")
        .clone();

    eprintln!(
        "[window-id-contract] before: tracked_id={}, geometry=({}, {}) {}x{}",
        tracked_window_before.id,
        tracked_window_before.row,
        tracked_window_before.col,
        tracked_window_before.width,
        tracked_window_before.height
    );

    session
        .execute_ex_command(":resize 5")
        .expect("resize should succeed");
    send_ctrl_w_command(&mut session, "H");
    send_ctrl_w_command(&mut session, "=");

    let after_layout_snapshot = session.snapshot();
    let tracked_window_after = after_layout_snapshot
        .window(tracked_window_id)
        .expect("tracked window should still exist after resize and move");
    assert_eq!(
        tracked_window_after.id, tracked_window_id,
        "window_id must remain the canonical identity across layout changes"
    );

    eprintln!(
        "[window-id-contract] after layout: tracked_id={}, geometry=({}, {}) {}x{}",
        tracked_window_after.id,
        tracked_window_after.row,
        tracked_window_after.col,
        tracked_window_after.width,
        tracked_window_after.height
    );

    session
        .switch_to_window(tracked_window_id)
        .expect("switch to tracked window should succeed");
    session
        .execute_ex_command(":close")
        .expect("close should succeed");

    let after_close_snapshot = session.snapshot();
    assert!(
        after_close_snapshot.window(tracked_window_id).is_none(),
        "closed window_id should disappear from the snapshot"
    );
    assert!(
        after_close_snapshot.active_window_id().is_some(),
        "a surviving active window should remain resolvable after close"
    );
}

// =============================================================================
// Task 2.1: Rust ドメインモデルの定義
// CoreBufferInfo および CoreWindowInfo 構造体の検証
// Bridge 層からのデータが正しく Rust 型に変換されること
// =============================================================================

#[test]
fn core_buffer_info_has_required_fields() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("domain model test").expect("session should initialize");
    let snapshot = session.snapshot();

    // CoreBufferInfo が id, name, dirty, is_active フィールドを持つこと
    let buf: &CoreBufferInfo = &snapshot.buffers[0];
    eprintln!(
        "[Task 2.1] CoreBufferInfo 検証: id={}, name='{}', dirty={}, is_active={}",
        buf.id, buf.name, buf.dirty, buf.is_active
    );

    // id は Vim の b_fnum（1-based の正の値）
    assert!(
        buf.id > 0,
        "バッファIDはVimのb_fnumに由来する正の値であること"
    );
    // name は String 型（空文字列も許容、無名バッファの場合）
    // dirty は bool 型
    assert!(!buf.dirty, "初期状態のバッファは未変更であること");
    // is_active は bool 型
    assert!(buf.is_active, "初期バッファはアクティブであること");
}

#[test]
fn core_window_info_has_required_fields() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("window model test").expect("session should initialize");
    session.set_screen_size(24, 80);

    let snapshot = session.snapshot();

    // CoreWindowInfo が id, buf_id, row, col, width, height, cursor_row, cursor_col, is_active フィールドを持つこと
    let win: &CoreWindowInfo = &snapshot.windows[0];
    eprintln!(
        "[Task 2.1] CoreWindowInfo 検証: id={}, buf_id={}, row={}, col={}, width={}, height={}, cursor_row={}, cursor_col={}, is_active={}",
        win.id,
        win.buf_id,
        win.row,
        win.col,
        win.width,
        win.height,
        win.cursor_row,
        win.cursor_col,
        win.is_active
    );

    assert!(
        win.id > 0,
        "ウィンドウIDはVimのw_idに由来する正の値であること"
    );
    assert!(
        win.buf_id > 0,
        "ウィンドウが表示するバッファIDは正の値であること"
    );
    assert_eq!(
        (win.cursor_row, win.cursor_col),
        (snapshot.cursor_row, snapshot.cursor_col),
        "アクティブウィンドウのcursorはsnapshot全体のcursorと一致すること"
    );
    // スクリーンサイズ設定後は幾何情報が正しく取得できること
    assert!(
        win.width > 0,
        "スクリーン設定後のウィンドウ幅は正の値であること: got {}",
        win.width
    );
    assert!(
        win.height > 0,
        "スクリーン設定後のウィンドウ高さは正の値であること: got {}",
        win.height
    );
    assert!(win.is_active, "初期ウィンドウはアクティブであること");
}

#[test]
fn core_buffer_info_is_clone_and_debug() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("clone test").expect("session should initialize");
    let snapshot = session.snapshot();

    // CoreBufferInfo が Clone および Debug を実装していること
    let buf = snapshot.buffers[0].clone();
    let debug_str = format!("{:?}", buf);
    eprintln!("[Task 2.1] CoreBufferInfo Debug出力: {}", debug_str);
    assert!(!debug_str.is_empty(), "Debug出力が空でないこと");
}

#[test]
fn core_window_info_is_clone_and_debug() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("clone test").expect("session should initialize");
    let snapshot = session.snapshot();

    // CoreWindowInfo が Clone および Debug を実装していること
    let win = snapshot.windows[0].clone();
    let debug_str = format!("{:?}", win);
    eprintln!("[Task 2.1] CoreWindowInfo Debug出力: {}", debug_str);
    assert!(!debug_str.is_empty(), "Debug出力が空でないこと");
}

#[test]
fn buffer_ids_use_vim_fnum_which_is_positive() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("id test").expect("session should initialize");

    // 複数バッファ作成後もIDは常に正の値（Vimのb_fnum由来）
    session
        .execute_ex_command(":enew")
        .expect("enew should succeed");
    session
        .execute_ex_command(":enew")
        .expect("enew should succeed");

    let buffers = session.buffers();
    eprintln!(
        "[Task 2.1] バッファID一覧: {:?}",
        buffers.iter().map(|b| b.id).collect::<Vec<_>>()
    );

    for buf in &buffers {
        assert!(
            buf.id > 0,
            "すべてのバッファIDは正の値であること: got {}",
            buf.id
        );
    }

    // IDはユニークであること
    let mut ids: Vec<i32> = buffers.iter().map(|b| b.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), buffers.len(), "バッファIDはユニークであること");
}

#[test]
fn window_ids_use_vim_wid_which_is_positive() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("win id test").expect("session should initialize");
    session.set_screen_size(24, 80);

    session
        .execute_ex_command(":split")
        .expect("split should succeed");
    session
        .execute_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let windows = session.windows();
    eprintln!(
        "[Task 2.1] ウィンドウID一覧: {:?}",
        windows.iter().map(|w| w.id).collect::<Vec<_>>()
    );

    for win in &windows {
        assert!(
            win.id > 0,
            "すべてのウィンドウIDは正の値であること: got {}",
            win.id
        );
    }

    // IDはユニークであること
    let mut ids: Vec<i32> = windows.iter().map(|w| w.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), windows.len(), "ウィンドウIDはユニークであること");
}

// =============================================================================
// Task 2.2: VimCoreSession への問い合わせ API の実装
// buffers() および windows() メソッド、アクティブ状態の追跡
// =============================================================================

#[test]
fn buffers_method_returns_all_buffers() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffers api test").expect("session should initialize");

    // 初期状態: 1バッファ
    let initial_buffers = session.buffers();
    eprintln!("[Task 2.2] 初期バッファ数: {}", initial_buffers.len());
    assert_eq!(
        initial_buffers.len(),
        1,
        "初期状態では1つのバッファが存在すること"
    );

    // バッファ追加後: 2バッファ
    session
        .execute_ex_command(":enew")
        .expect("enew should succeed");
    let after_buffers = session.buffers();
    eprintln!("[Task 2.2] enew後バッファ数: {}", after_buffers.len());
    assert_eq!(
        after_buffers.len(),
        2,
        ":enew後に2つのバッファが存在すること"
    );

    // 現在のバッファを変更してからさらに追加（Vim は未変更の無名バッファで :enew するとバッファを再利用する）
    session
        .execute_ex_command(":call setline(1, 'some content')")
        .expect("setline should succeed");
    session
        .execute_ex_command(":enew!")
        .expect("enew should succeed");
    let final_buffers = session.buffers();
    eprintln!(
        "[Task 2.2] 変更後enew!のバッファ数: {}",
        final_buffers.len()
    );
    assert_eq!(
        final_buffers.len(),
        3,
        "変更後:enew!で3つのバッファが存在すること"
    );
}

#[test]
fn windows_method_returns_all_windows() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("windows api test").expect("session should initialize");
    session.set_screen_size(24, 80);

    // 初期状態: 1ウィンドウ
    let initial_windows = session.windows();
    eprintln!("[Task 2.2] 初期ウィンドウ数: {}", initial_windows.len());
    assert_eq!(
        initial_windows.len(),
        1,
        "初期状態では1つのウィンドウが存在すること"
    );

    // split後: 2ウィンドウ
    session
        .execute_ex_command(":split")
        .expect("split should succeed");
    let after_windows = session.windows();
    eprintln!("[Task 2.2] split後ウィンドウ数: {}", after_windows.len());
    assert_eq!(
        after_windows.len(),
        2,
        ":split後に2つのウィンドウが存在すること"
    );
}

#[test]
fn exactly_one_active_buffer_exists() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("active buf test").expect("session should initialize");

    // 複数バッファ作成後もアクティブバッファは常に1つ
    session
        .execute_ex_command(":enew")
        .expect("enew should succeed");
    session
        .execute_ex_command(":enew")
        .expect("enew should succeed");

    let buffers = session.buffers();
    let active_count = buffers.iter().filter(|b| b.is_active).count();
    eprintln!(
        "[Task 2.2] バッファ数={}, アクティブ数={}",
        buffers.len(),
        active_count
    );
    assert_eq!(
        active_count, 1,
        "アクティブバッファは常に1つであること: got {}",
        active_count
    );
}

#[test]
fn exactly_one_active_window_exists() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("active win test").expect("session should initialize");
    session.set_screen_size(24, 80);

    // 複数ウィンドウ作成後もアクティブウィンドウは常に1つ
    session
        .execute_ex_command(":split")
        .expect("split should succeed");
    session
        .execute_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let windows = session.windows();
    let active_count = windows.iter().filter(|w| w.is_active).count();
    eprintln!(
        "[Task 2.2] ウィンドウ数={}, アクティブ数={}",
        windows.len(),
        active_count
    );
    assert_eq!(
        active_count, 1,
        "アクティブウィンドウは常に1つであること: got {}",
        active_count
    );
}

#[test]
fn active_buffer_matches_snapshot_text() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("snapshot text match").expect("session should initialize");

    let snapshot = session.snapshot();
    let active_buf = snapshot.buffers.iter().find(|b| b.is_active).unwrap();

    // アクティブバッファのテキストがスナップショットのテキストと一致すること
    // 注: snapshot.text は Vim が最終行に付加する改行を含む場合がある。
    //     buffer_text() は getline() ベースのため末尾改行を含まない。
    //     両者の内容が実質的に同じであることを検証する。
    let buf_text = session
        .buffer_text(active_buf.id)
        .expect("アクティブバッファのテキスト取得");
    eprintln!(
        "[Task 2.2] snapshot.text='{:?}', buffer_text='{:?}'",
        snapshot.text, buf_text
    );
    assert_eq!(
        snapshot.text.trim_end_matches('\n'),
        buf_text.trim_end_matches('\n'),
        "アクティブバッファのテキストがスナップショットのテキストと実質的に一致すること"
    );
}

#[test]
fn active_window_displays_active_buffer() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("win-buf link test").expect("session should initialize");

    let snapshot = session.snapshot();
    let active_win = snapshot.windows.iter().find(|w| w.is_active).unwrap();
    let active_buf = snapshot.buffers.iter().find(|b| b.is_active).unwrap();

    eprintln!(
        "[Task 2.2] アクティブウィンドウのbuf_id={}, アクティブバッファのid={}",
        active_win.buf_id, active_buf.id
    );
    assert_eq!(
        active_win.buf_id, active_buf.id,
        "アクティブウィンドウはアクティブバッファを表示していること"
    );
}

#[test]
fn buffers_and_windows_consistent_with_snapshot() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("consistency test").expect("session should initialize");
    session.set_screen_size(24, 80);
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    // buffers() と windows() が snapshot() と同一の結果を返すこと
    let snapshot = session.snapshot();
    let buffers = session.buffers();
    let windows = session.windows();

    eprintln!(
        "[Task 2.2] snapshot.buffers={}, buffers()={}, snapshot.windows={}, windows()={}",
        snapshot.buffers.len(),
        buffers.len(),
        snapshot.windows.len(),
        windows.len()
    );
    assert_eq!(
        snapshot.buffers.len(),
        buffers.len(),
        "buffers()はsnapshot().buffersと同じ数を返すこと"
    );
    assert_eq!(
        snapshot.windows.len(),
        windows.len(),
        "windows()はsnapshot().windowsと同じ数を返すこと"
    );
}

// =============================================================================
// Task 2.3: 操作用メソッドおよび非アクティブバッファへのアクセス
// switch_to_buffer, switch_to_window, buffer_text(id) の Rust API 検証
// =============================================================================

#[test]
fn switch_to_buffer_returns_ok_for_valid_id() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("switch buf ok").expect("session should initialize");

    session
        .execute_ex_command(":enew")
        .expect("enew should succeed");

    let first_buf_id = session.buffers().iter().find(|b| !b.is_active).unwrap().id;
    eprintln!("[Task 2.3] 切り替え先バッファID: {}", first_buf_id);

    let result: Result<(), CoreCommandError> = session.switch_to_buffer(first_buf_id);
    assert!(result.is_ok(), "有効なバッファIDへの切り替えはOkを返すこと");
}

#[test]
fn switch_to_buffer_returns_invalid_input_for_bad_id() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("switch buf err").expect("session should initialize");

    let result = session.switch_to_buffer(99999);
    eprintln!("[Task 2.3] 無効バッファID切り替え結果: {:?}", result);
    assert!(
        matches!(result, Err(CoreCommandError::InvalidInput)),
        "存在しないバッファIDへの切り替えはInvalidInputエラーを返すこと"
    );
}

#[test]
fn switch_to_window_returns_ok_for_valid_id() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("switch win ok").expect("session should initialize");
    session.set_screen_size(24, 80);

    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let inactive_win_id = session.windows().iter().find(|w| !w.is_active).unwrap().id;
    eprintln!("[Task 2.3] 切り替え先ウィンドウID: {}", inactive_win_id);

    let result: Result<(), CoreCommandError> = session.switch_to_window(inactive_win_id);
    assert!(
        result.is_ok(),
        "有効なウィンドウIDへの切り替えはOkを返すこと"
    );
}

#[test]
fn switch_to_window_returns_invalid_input_for_bad_id() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("switch win err").expect("session should initialize");

    let result = session.switch_to_window(99999);
    eprintln!("[Task 2.3] 無効ウィンドウID切り替え結果: {:?}", result);
    assert!(
        matches!(result, Err(CoreCommandError::InvalidInput)),
        "存在しないウィンドウIDへの切り替えはInvalidInputエラーを返すこと"
    );
}

#[test]
fn buffer_text_for_inactive_buffer_returns_correct_content() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("first buffer content").expect("session should initialize");
    session.set_screen_size(24, 80);

    // 最初のバッファのIDを記録
    let first_buf_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;

    // :split + :enew で新しいウィンドウに新バッファを作成
    // （:enew 単体だと Vim が無名バッファの memline を unload する場合がある）
    session
        .execute_ex_command(":split")
        .expect("split should succeed");
    session
        .execute_ex_command(":enew")
        .expect("enew should succeed");

    // 新バッファが作成されていること
    let buffers = session.buffers();
    eprintln!(
        "[Task 2.3] バッファ一覧: {:?}",
        buffers
            .iter()
            .map(|b| (b.id, b.is_active))
            .collect::<Vec<_>>()
    );

    // 非アクティブバッファ（最初のバッファ）のテキストを取得
    let text = session.buffer_text(first_buf_id);
    eprintln!(
        "[Task 2.3] 非アクティブバッファ(id={})のテキスト: {:?}",
        first_buf_id, text
    );
    assert!(
        text.is_some(),
        "非アクティブバッファのテキストが取得できること"
    );
    assert_eq!(
        text.unwrap(),
        "first buffer content",
        "非アクティブバッファのテキスト内容が正しいこと"
    );
}

#[test]
fn buffer_text_returns_none_for_nonexistent_buffer() {
    let _guard = acquire_session_test_lock();
    let session = VimCoreSession::new("none test").expect("session should initialize");

    let text = session.buffer_text(99999);
    eprintln!("[Task 2.3] 存在しないバッファのテキスト: {:?}", text);
    assert!(text.is_none(), "存在しないバッファIDに対してNoneを返すこと");
}

#[test]
fn switch_buffer_and_verify_text_round_trip() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buffer A content").expect("session should initialize");
    session.set_screen_size(24, 80);

    let buf_a_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;

    // :split + :enew で新ウィンドウに新バッファを作成（memline が確実にロードされたままにする）
    session
        .execute_ex_command(":split")
        .expect("split should succeed");
    session
        .execute_ex_command(":enew")
        .expect("enew should succeed");
    let buf_b_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;

    // バッファBにテキストを挿入
    session
        .execute_ex_command(":call setline(1, 'buffer B content')")
        .expect("setline should succeed");

    eprintln!(
        "[Task 2.3] バッファA(id={}), バッファB(id={})",
        buf_a_id, buf_b_id
    );

    // バッファAのテキストを非アクティブ状態で取得
    let text_a = session.buffer_text(buf_a_id);
    assert_eq!(
        text_a.as_deref(),
        Some("buffer A content"),
        "非アクティブなバッファAのテキストが正しいこと"
    );

    // バッファBのテキストをアクティブ状態で取得
    let text_b = session.buffer_text(buf_b_id);
    assert_eq!(
        text_b.as_deref(),
        Some("buffer B content"),
        "アクティブなバッファBのテキストが正しいこと"
    );

    // バッファAに切り替えて確認
    session
        .switch_to_buffer(buf_a_id)
        .expect("switch to buf A should succeed");
    let snapshot = session.snapshot();
    // snapshot.text には末尾改行が含まれる場合がある
    assert_eq!(
        snapshot.text.trim_end_matches('\n'),
        "buffer A content",
        "バッファA切り替え後のスナップショットテキストが正しいこと"
    );

    // バッファBのテキストを非アクティブ状態で取得
    let text_b_after = session.buffer_text(buf_b_id);
    assert_eq!(
        text_b_after.as_deref(),
        Some("buffer B content"),
        "非アクティブなバッファBのテキストが正しいこと"
    );
}

#[test]
fn switch_window_preserves_buffer_association() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("window buf assoc").expect("session should initialize");
    session.set_screen_size(24, 80);

    // ウィンドウ分割
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let windows_before = session.windows();
    let active_win = windows_before.iter().find(|w| w.is_active).unwrap();
    let inactive_win = windows_before.iter().find(|w| !w.is_active).unwrap();

    eprintln!(
        "[Task 2.3] 分割直後: アクティブwin={} (buf={}), 非アクティブwin={} (buf={})",
        active_win.id, active_win.buf_id, inactive_win.id, inactive_win.buf_id
    );

    // 分割直後は両方同じバッファを表示
    assert_eq!(
        active_win.buf_id, inactive_win.buf_id,
        ":split直後は両ウィンドウが同じバッファを表示すること"
    );

    // 非アクティブウィンドウに切り替え
    let target_win_id = inactive_win.id;
    session
        .switch_to_window(target_win_id)
        .expect("switch_to_window should succeed");

    // 切り替え後のアクティブウィンドウを確認
    let windows_after = session.windows();
    let new_active = windows_after.iter().find(|w| w.is_active).unwrap();
    assert_eq!(
        new_active.id, target_win_id,
        "ウィンドウ切り替え後にアクティブウィンドウが変更されること"
    );
}

// =============================================================================
// Task 3.1: ホストイベントのインターセプト拡張
// バッファ生成やウィンドウ分割を HostAction として Rust 側に通知する仕組み
// =============================================================================

#[test]
fn enew_triggers_buffer_added_event() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("initial").expect("session should initialize");
    session.set_screen_size(24, 80);

    // :enew でバッファ生成 → HostBufAdd イベントが発火されること
    let outcome = session
        .execute_ex_command(":enew")
        .expect("enew should succeed");

    eprintln!("[Task 3.1] enew outcome: {:?}", outcome);

    let events = outcome.events;
    let found_buf_add = events
        .iter()
        .any(|event| matches!(event, CoreEvent::BufferAdded { .. }));

    assert!(
        found_buf_add,
        ":enew 後に BufferAdded event が発火されること。取得されたイベント: {:?}",
        events
    );
    assert!(outcome.host_actions.is_empty());
}

#[test]
fn split_triggers_window_created_event() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("split event test").expect("session should initialize");
    session.set_screen_size(24, 80);

    // :split でウィンドウ分割 → HostWinNew イベントが発火されること
    let outcome = session
        .execute_ex_command(":split")
        .expect("split should succeed");

    eprintln!("[Task 3.1] split outcome: {:?}", outcome);

    let events = outcome.events;
    let found_win_new = events
        .iter()
        .any(|event| matches!(event, CoreEvent::WindowCreated { .. }));

    assert!(
        found_win_new,
        ":split 後に WindowCreated event が発火されること。取得されたイベント: {:?}",
        events
    );
    assert!(outcome.host_actions.is_empty());
}

#[test]
fn buffer_added_event_contains_buffer_id() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("buf id test").expect("session should initialize");
    session.set_screen_size(24, 80);

    // :enew でバッファ生成
    let tx = session
        .execute_ex_command(":enew")
        .expect("enew should succeed");

    // BufferAdded event に有効なバッファIDが含まれること
    for event in tx.events {
        if let CoreEvent::BufferAdded { buf_id } = event {
            eprintln!("[Task 3.1] BufAdd buf_id: {}", buf_id);
            assert!(
                buf_id > 0,
                "BufAdd のバッファIDは正の値であること: got {}",
                buf_id
            );
            // このバッファIDが現在のバッファリストに存在すること
            let buffers = session.buffers();
            let exists = buffers.iter().any(|b| b.id == buf_id);
            assert!(
                exists,
                "BufAdd のバッファID({})がバッファリストに存在すること",
                buf_id
            );
            return;
        }
    }
    panic!("BufferAdded event が見つからなかった");
}

#[test]
fn window_created_event_contains_window_id() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("win id test").expect("session should initialize");
    session.set_screen_size(24, 80);

    // :split でウィンドウ分割
    let tx = session
        .execute_ex_command(":split")
        .expect("split should succeed");

    // WindowCreated event に有効なウィンドウIDが含まれること
    for event in tx.events {
        if let CoreEvent::WindowCreated { win_id } = event {
            eprintln!("[Task 3.1] WinNew win_id: {}", win_id);
            assert!(
                win_id > 0,
                "WinNew のウィンドウIDは正の値であること: got {}",
                win_id
            );
            // このウィンドウIDが現在のウィンドウリストに存在すること
            let windows = session.windows();
            let exists = windows.iter().any(|w| w.id == win_id);
            assert!(
                exists,
                "WinNew のウィンドウID({})がウィンドウリストに存在すること",
                win_id
            );
            return;
        }
    }
    panic!("WindowCreated event が見つからなかった");
}

#[test]
fn vsplit_also_triggers_window_created_event() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("vsplit event test").expect("session should initialize");
    session.set_screen_size(24, 80);

    // :vsplit でもウィンドウ生成イベントが発火されること
    let tx = session
        .execute_ex_command(":vsplit")
        .expect("vsplit should succeed");

    let events = tx.events;
    let found_win_new = events
        .iter()
        .any(|event| matches!(event, CoreEvent::WindowCreated { .. }));

    assert!(
        found_win_new,
        ":vsplit 後に WindowCreated event が発火されること"
    );
    assert!(tx.host_actions.is_empty());
}

// =============================================================================
// Task 3.2: レイアウト変更の検知と通知
// ウィンドウのサイズ変更や位置移動を検知し、レイアウト再同期の通知を発火
// =============================================================================

#[test]
fn window_resize_triggers_layout_changed_event() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("layout change test").expect("session should initialize");
    session.set_screen_size(24, 80);

    // まずウィンドウを分割
    let split_tx = session
        .execute_ex_command(":split")
        .expect("split should succeed");

    assert!(!split_tx.events.is_empty());
    assert!(split_tx.host_actions.is_empty());

    // ウィンドウサイズを変更（:resize でアクティブウィンドウの高さを変更）
    let resize_tx = session
        .execute_ex_command(":resize 5")
        .expect("resize should succeed");

    let events = resize_tx.events;
    let found_layout_changed = events
        .iter()
        .any(|event| matches!(event, CoreEvent::LayoutChanged));

    assert!(
        found_layout_changed,
        ":resize 後に LayoutChanged event が発火されること。取得されたイベント: {:?}",
        events
    );
    assert!(resize_tx.host_actions.is_empty());
}

#[test]
fn vertical_resize_triggers_layout_changed_event() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("vresize test").expect("session should initialize");
    session.set_screen_size(24, 80);

    // 垂直分割
    let vsplit_tx = session
        .execute_ex_command(":vsplit")
        .expect("vsplit should succeed");

    assert!(!vsplit_tx.events.is_empty());
    assert!(vsplit_tx.host_actions.is_empty());

    // 垂直サイズ変更
    let resize_tx = session
        .execute_ex_command(":vertical resize 30")
        .expect("vertical resize should succeed");

    let events = resize_tx.events;
    let found_layout_changed = events
        .iter()
        .any(|event| matches!(event, CoreEvent::LayoutChanged));

    assert!(
        found_layout_changed,
        ":vertical resize 後に LayoutChanged event が発火されること。取得されたイベント: {:?}",
        events
    );
    assert!(resize_tx.host_actions.is_empty());
}

#[test]
fn screen_size_change_triggers_layout_changed_event() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("screen resize test").expect("session should initialize");
    session.set_screen_size(24, 80);

    // ウィンドウを分割してからスクリーンサイズを変更
    session
        .execute_ex_command(":split")
        .expect("split should succeed");

    while session.take_pending_event().is_some() {}
    while session.take_pending_host_action().is_some() {}

    // スクリーンサイズの変更
    session.set_screen_size(40, 120);

    let events = drain_events(&mut session);
    let found_layout_changed = events
        .iter()
        .any(|event| matches!(event, CoreEvent::LayoutChanged));

    assert!(
        found_layout_changed,
        "スクリーンサイズ変更後に LayoutChanged event が発火されること。取得されたイベント: {:?}",
        events
    );
    assert!(session.take_pending_host_action().is_none());
}

#[test]
fn split_triggers_both_window_created_and_layout_changed_events() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("combined event test").expect("session should initialize");
    session.set_screen_size(24, 80);

    // :split はウィンドウ生成とレイアウト変更の両方を発火すること
    let tx = session
        .execute_ex_command(":split")
        .expect("split should succeed");

    let events = tx.events;
    let found_win_new = events
        .iter()
        .any(|event| matches!(event, CoreEvent::WindowCreated { .. }));
    let found_layout_changed = events
        .iter()
        .any(|event| matches!(event, CoreEvent::LayoutChanged));

    assert!(
        found_win_new,
        ":split 後に WindowCreated が発火されること。取得されたイベント: {:?}",
        events
    );
    assert!(
        found_layout_changed,
        ":split 後に LayoutChanged が発火されること。取得されたイベント: {:?}",
        events
    );
    assert!(tx.host_actions.is_empty());
}
