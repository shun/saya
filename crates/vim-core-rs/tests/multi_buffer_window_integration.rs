// =============================================================================
// Task 4.1 & 4.2: 統合テスト
// マルチバッファ・マルチウィンドウの統合動作を検証する
// =============================================================================

use std::sync::{Mutex, OnceLock};
use vim_core_rs::{CoreEvent, VimCoreSession};

fn session_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_session_test_lock() -> std::sync::MutexGuard<'static, ()> {
    session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Event キューをすべて消費してVecとして返すヘルパー
fn drain_events(session: &mut VimCoreSession) -> Vec<CoreEvent> {
    let mut events = Vec::new();
    while let Some(event) = session.take_pending_event() {
        events.push(event);
    }
    events
}

// =============================================================================
// Task 4.1: マルチバッファ動作の統合テスト
// 複数バッファの作成、削除、切り替えが期待通りに動作するか検証
// 非アクティブなバッファのテキストが正しく取得できることを確認
// Requirements: 1.1, 1.2, 1.3, 3.1
// =============================================================================

#[test]
fn integration_create_multiple_buffers_and_verify_listing() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("buffer 1 content").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.1] === 複数バッファの作成と一覧取得の統合テスト ===");

    // 初期状態: 1バッファ
    let initial = session.buffers();
    eprintln!("[Task 4.1] 初期バッファ数: {}", initial.len());
    assert_eq!(initial.len(), 1, "初期状態ではバッファは1つ");

    // バッファ2を作成（ウィンドウを分割してからenewすることでmemlineを保持）
    session.execute_ex_command(":split").expect("split成功");
    session.execute_ex_command(":enew").expect("enew成功");
    session
        .execute_ex_command(":call setline(1, 'buffer 2 content')")
        .expect("setline成功");

    let buf2_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;
    eprintln!("[Task 4.1] バッファ2のID: {}", buf2_id);

    // バッファ3を作成（バッファ2が変更済みなので :enew! で強制）
    session.execute_ex_command(":enew!").expect("enew!成功");
    session
        .execute_ex_command(":call setline(1, 'buffer 3 content')")
        .expect("setline成功");

    let buf3_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;
    eprintln!("[Task 4.1] バッファ3のID: {}", buf3_id);

    // 3バッファが存在すること
    let buffers = session.buffers();
    eprintln!(
        "[Task 4.1] 全バッファ: {:?}",
        buffers
            .iter()
            .map(|b| (b.id, &b.name, b.is_active))
            .collect::<Vec<_>>()
    );
    assert!(
        buffers.len() >= 3,
        "3つ以上のバッファが存在すること: got {}",
        buffers.len()
    );

    // 各バッファのIDがユニークであること
    let mut ids: Vec<i32> = buffers.iter().map(|b| b.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), buffers.len(), "バッファIDはすべてユニーク");

    // アクティブなバッファは1つだけであること
    let active_count = buffers.iter().filter(|b| b.is_active).count();
    assert_eq!(active_count, 1, "アクティブバッファは常に1つ");
}

#[test]
fn integration_switch_between_buffers_round_trip() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("alpha content").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.1] === バッファ切り替えラウンドトリップの統合テスト ===");

    let buf_alpha_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;
    eprintln!("[Task 4.1] バッファAlpha ID: {}", buf_alpha_id);

    // バッファBetaを作成
    session.execute_ex_command(":split").expect("split成功");
    session.execute_ex_command(":enew").expect("enew成功");
    session
        .execute_ex_command(":call setline(1, 'beta content')")
        .expect("setline成功");
    let buf_beta_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;
    eprintln!("[Task 4.1] バッファBeta ID: {}", buf_beta_id);

    // Alpha → Beta → Alpha の切り替えでテキストが保持されること
    // 現在Betaがアクティブ
    let text_alpha = session.buffer_text(buf_alpha_id);
    eprintln!("[Task 4.1] 非アクティブAlphaのテキスト: {:?}", text_alpha);
    assert_eq!(
        text_alpha.as_deref(),
        Some("alpha content"),
        "非アクティブAlphaのテキスト取得"
    );

    // Alphaに切り替え
    session
        .switch_to_buffer(buf_alpha_id)
        .expect("Alphaへの切り替え成功");
    let snapshot = session.snapshot();
    eprintln!(
        "[Task 4.1] Alpha切り替え後: text='{}', active_buf_id={}",
        snapshot.text.trim_end_matches('\n'),
        snapshot.buffers.iter().find(|b| b.is_active).unwrap().id
    );
    assert_eq!(
        snapshot.text.trim_end_matches('\n'),
        "alpha content",
        "Alpha切り替え後のスナップショットテキスト"
    );

    // 非アクティブBetaのテキストも保持されていること
    let text_beta = session.buffer_text(buf_beta_id);
    eprintln!("[Task 4.1] 非アクティブBetaのテキスト: {:?}", text_beta);
    assert_eq!(
        text_beta.as_deref(),
        Some("beta content"),
        "非アクティブBetaのテキスト保持"
    );

    // Betaに戻る
    session
        .switch_to_buffer(buf_beta_id)
        .expect("Betaへの切り替え成功");
    let snapshot = session.snapshot();
    assert_eq!(
        snapshot.text.trim_end_matches('\n'),
        "beta content",
        "Beta再切り替え後のスナップショットテキスト"
    );
}

#[test]
fn integration_delete_buffer_updates_listing() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("keep me").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.1] === バッファ削除後の一覧更新テスト ===");

    // バッファを追加
    session.execute_ex_command(":split").expect("split成功");
    session.execute_ex_command(":enew").expect("enew成功");
    session
        .execute_ex_command(":call setline(1, 'delete me')")
        .expect("setline成功");

    let buf_to_delete = session.buffers().iter().find(|b| b.is_active).unwrap().id;
    let count_before = session.buffers().len();
    eprintln!(
        "[Task 4.1] 削除前: バッファ数={}, 削除対象ID={}",
        count_before, buf_to_delete
    );

    // バッファを完全削除（bwipeout はバッファリストからも完全に除去する）
    let cmd = format!(":bwipeout! {}", buf_to_delete);
    session.execute_ex_command(&cmd).expect("bwipeout成功");

    let buffers_after = session.buffers();
    eprintln!(
        "[Task 4.1] 削除後: バッファ数={}, IDs={:?}",
        buffers_after.len(),
        buffers_after.iter().map(|b| b.id).collect::<Vec<_>>()
    );

    // 削除されたバッファがリストに含まれないこと
    let still_exists = buffers_after.iter().any(|b| b.id == buf_to_delete);
    assert!(
        !still_exists,
        "削除されたバッファ(id={})がリストに残っていないこと",
        buf_to_delete
    );

    // バッファ数が減少していること
    assert!(
        buffers_after.len() < count_before,
        "バッファ数が削除前({})より減少していること: got {}",
        count_before,
        buffers_after.len()
    );
}

#[test]
fn integration_inactive_buffer_text_after_edits() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("line one").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.1] === 編集後の非アクティブバッファテキスト取得テスト ===");

    // バッファ1を編集
    session
        .execute_ex_command(":call setline(1, 'edited line one')")
        .expect("setline成功");
    let buf1_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;

    // バッファ2を作成して編集
    session.execute_ex_command(":split").expect("split成功");
    session.execute_ex_command(":enew").expect("enew成功");
    session
        .execute_ex_command(":call setline(1, 'line two')")
        .expect("setline成功");

    // バッファ1のテキストが編集後の内容であること
    let text1 = session.buffer_text(buf1_id);
    eprintln!("[Task 4.1] バッファ1(非アクティブ)のテキスト: {:?}", text1);
    assert_eq!(
        text1.as_deref(),
        Some("edited line one"),
        "編集後の非アクティブバッファのテキストが正しいこと"
    );
}

#[test]
fn integration_buffer_dirty_flag_tracks_modifications() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("clean buffer").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.1] === バッファdirtyフラグの追跡テスト ===");

    let buf1_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;

    // 初期状態: dirty=false
    let buf1 = session
        .buffers()
        .iter()
        .find(|b| b.id == buf1_id)
        .unwrap()
        .clone();
    eprintln!("[Task 4.1] 初期状態: buf1 dirty={}", buf1.dirty);
    assert!(!buf1.dirty, "初期状態のバッファはdirty=false");

    // 編集後: dirty=true
    session.execute_normal_command("dd").expect("dd成功");
    let buf1 = session
        .buffers()
        .iter()
        .find(|b| b.id == buf1_id)
        .unwrap()
        .clone();
    eprintln!("[Task 4.1] 編集後: buf1 dirty={}", buf1.dirty);
    assert!(buf1.dirty, "編集後のバッファはdirty=true");

    // 新バッファ作成して確認
    session.execute_ex_command(":split").expect("split成功");
    session.execute_ex_command(":enew").expect("enew成功");

    // 新バッファはdirty=false
    let new_buf = session
        .buffers()
        .iter()
        .find(|b| b.is_active)
        .unwrap()
        .clone();
    eprintln!("[Task 4.1] 新バッファ: dirty={}", new_buf.dirty);
    assert!(!new_buf.dirty, "新規作成バッファはdirty=false");

    // 元のバッファはまだdirty=true
    let buf1_check = session
        .buffers()
        .iter()
        .find(|b| b.id == buf1_id)
        .unwrap()
        .clone();
    eprintln!(
        "[Task 4.1] 元バッファ(非アクティブ): dirty={}",
        buf1_check.dirty
    );
    assert!(
        buf1_check.dirty,
        "非アクティブでも編集済みバッファはdirty=true"
    );
}

#[test]
fn integration_buffer_creation_triggers_buf_add_events() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("event test").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.1] === バッファ作成時のBufAddイベント発火テスト ===");

    drain_events(&mut session);

    // 3つのバッファを連続作成
    let mut buf_add_count = 0;
    for i in 1..=3 {
        let tx = session.execute_ex_command(":enew").expect("enew成功");
        let events = tx.events;
        let buf_adds: Vec<&CoreEvent> = events
            .iter()
            .filter(|event| matches!(event, CoreEvent::BufferAdded { .. }))
            .collect();
        eprintln!(
            "[Task 4.1] バッファ{}作成後のBufAddイベント数: {}",
            i,
            buf_adds.len()
        );
        buf_add_count += buf_adds.len();
    }

    eprintln!("[Task 4.1] 合計BufAddイベント数: {}", buf_add_count);
    assert!(
        buf_add_count >= 3,
        "3回の:enewで少なくとも3回のBufAddイベントが発火されること: got {}",
        buf_add_count
    );
}

#[test]
fn integration_multi_buffer_text_isolation() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("AAA").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.1] === マルチバッファ間のテキスト分離テスト ===");

    let buf_a_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;

    // バッファBを作成（split + enew でmemlineを保持）
    session.execute_ex_command(":split").expect("split成功");
    session.execute_ex_command(":enew").expect("enew成功");
    session
        .execute_ex_command(":call setline(1, 'BBB')")
        .expect("setline成功");
    let buf_b_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;

    // バッファCを作成（さらにsplit + enew で別ウィンドウに新バッファを開く）
    // enew! だとバッファBの変更が破棄されるため、split経由で別ウィンドウに新バッファを作る
    session.execute_ex_command(":split").expect("split成功");
    session.execute_ex_command(":enew").expect("enew成功");
    session
        .execute_ex_command(":call setline(1, 'CCC')")
        .expect("setline成功");
    let buf_c_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;

    // 各バッファのテキストが独立していること
    let text_a = session.buffer_text(buf_a_id);
    let text_b = session.buffer_text(buf_b_id);
    let text_c = session.buffer_text(buf_c_id);

    eprintln!("[Task 4.1] バッファA(id={}): {:?}", buf_a_id, text_a);
    eprintln!("[Task 4.1] バッファB(id={}): {:?}", buf_b_id, text_b);
    eprintln!("[Task 4.1] バッファC(id={}): {:?}", buf_c_id, text_c);

    assert_eq!(text_a.as_deref(), Some("AAA"), "バッファAのテキスト");
    assert_eq!(text_b.as_deref(), Some("BBB"), "バッファBのテキスト");
    assert_eq!(text_c.as_deref(), Some("CCC"), "バッファCのテキスト");

    // バッファCを編集してもAとBに影響しないこと
    session
        .execute_ex_command(":call setline(1, 'CCC modified')")
        .expect("setline成功");

    let text_a_after = session.buffer_text(buf_a_id);
    let text_b_after = session.buffer_text(buf_b_id);
    let text_c_after = session.buffer_text(buf_c_id);

    eprintln!("[Task 4.1] 編集後 バッファA: {:?}", text_a_after);
    eprintln!("[Task 4.1] 編集後 バッファB: {:?}", text_b_after);
    eprintln!("[Task 4.1] 編集後 バッファC: {:?}", text_c_after);

    assert_eq!(text_a_after.as_deref(), Some("AAA"), "バッファAは未変更");
    assert_eq!(text_b_after.as_deref(), Some("BBB"), "バッファBは未変更");
    assert_eq!(
        text_c_after.as_deref(),
        Some("CCC modified"),
        "バッファCのみ変更"
    );
}

// =============================================================================
// Task 4.2: マルチウィンドウレイアウトの検証テスト
// ウィンドウ分割、サイズ変更、フォーカス移動後のスナップショットの幾何情報が正確か検証
// イベント通知が適切なタイミングで Host に到達するかを確認
// Requirements: 2.1, 2.2, 3.2, 4.1, 4.2
// =============================================================================

#[test]
fn integration_horizontal_split_geometry() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hsplit geo").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.2] === 水平分割後の幾何情報テスト ===");

    // 分割前の幾何情報
    let before = session.windows();
    eprintln!(
        "[Task 4.2] 分割前: {:?}",
        before
            .iter()
            .map(|w| (w.id, w.row, w.col, w.width, w.height))
            .collect::<Vec<_>>()
    );

    session.execute_ex_command(":split").expect("split成功");

    let after = session.windows();
    eprintln!(
        "[Task 4.2] 分割後: {:?}",
        after
            .iter()
            .map(|w| (w.id, w.row, w.col, w.width, w.height, w.is_active))
            .collect::<Vec<_>>()
    );

    assert_eq!(after.len(), 2, "水平分割後は2ウィンドウ");

    // 両ウィンドウの幅はスクリーン幅と同じ
    for win in &after {
        assert!(
            win.width > 0,
            "各ウィンドウの幅は正の値: win_id={}, width={}",
            win.id,
            win.width
        );
    }

    // 水平分割: 上下に並ぶので、rowが異なること
    let rows: Vec<usize> = after.iter().map(|w| w.row).collect();
    eprintln!("[Task 4.2] ウィンドウのrow値: {:?}", rows);
    assert_ne!(
        rows[0], rows[1],
        "水平分割後の2ウィンドウは異なるrow位置を持つこと"
    );

    // 各ウィンドウの高さの合計 + 区切り線 <= スクリーン行数（ステータスライン等を考慮）
    let total_height: usize = after.iter().map(|w| w.height).sum();
    eprintln!(
        "[Task 4.2] ウィンドウ高さ合計: {} (スクリーン行数: 24)",
        total_height
    );
    // Vimはステータスラインやコマンドラインに1-2行使うため、厳密な等式は使わない
    assert!(
        total_height > 0 && total_height <= 24,
        "ウィンドウ高さ合計はスクリーン行数以下であること"
    );
}

#[test]
fn integration_vertical_split_geometry() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("vsplit geo").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.2] === 垂直分割後の幾何情報テスト ===");

    session.execute_ex_command(":vsplit").expect("vsplit成功");

    let windows = session.windows();
    eprintln!(
        "[Task 4.2] 垂直分割後: {:?}",
        windows
            .iter()
            .map(|w| (w.id, w.row, w.col, w.width, w.height, w.is_active))
            .collect::<Vec<_>>()
    );

    assert_eq!(windows.len(), 2, "垂直分割後は2ウィンドウ");

    // 垂直分割: 左右に並ぶので、colが異なること
    let cols: Vec<usize> = windows.iter().map(|w| w.col).collect();
    eprintln!("[Task 4.2] ウィンドウのcol値: {:?}", cols);
    assert_ne!(
        cols[0], cols[1],
        "垂直分割後の2ウィンドウは異なるcol位置を持つこと"
    );

    // 各ウィンドウの幅の合計 + 区切り線 <= スクリーン幅
    let total_width: usize = windows.iter().map(|w| w.width).sum();
    eprintln!(
        "[Task 4.2] ウィンドウ幅合計: {} (スクリーン幅: 80)",
        total_width
    );
    assert!(
        total_width > 0 && total_width <= 80,
        "ウィンドウ幅合計はスクリーン幅以下であること"
    );
}

#[test]
fn integration_resize_updates_geometry() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("resize geo").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.2] === リサイズ後の幾何情報更新テスト ===");

    session.execute_ex_command(":split").expect("split成功");

    let before = session.windows();
    let active_win_before = before.iter().find(|w| w.is_active).unwrap().clone();
    eprintln!(
        "[Task 4.2] リサイズ前のアクティブウィンドウ: height={}",
        active_win_before.height
    );

    // リサイズ実行
    session.execute_ex_command(":resize 5").expect("resize成功");

    let after = session.windows();
    let active_win_after = after.iter().find(|w| w.id == active_win_before.id).unwrap();
    eprintln!(
        "[Task 4.2] リサイズ後のアクティブウィンドウ: height={}",
        active_win_after.height
    );

    assert_eq!(
        active_win_after.height, 5,
        ":resize 5 後にウィンドウの高さが5になること"
    );
}

#[test]
fn integration_focus_move_updates_active_window() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("focus move").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.2] === フォーカス移動後のアクティブウィンドウ更新テスト ===");

    // 水平分割 + 垂直分割で3ウィンドウ
    session.execute_ex_command(":split").expect("split成功");
    session.execute_ex_command(":vsplit").expect("vsplit成功");

    let windows = session.windows();
    eprintln!(
        "[Task 4.2] 3ウィンドウ作成後: {:?}",
        windows
            .iter()
            .map(|w| (w.id, w.is_active))
            .collect::<Vec<_>>()
    );
    assert_eq!(windows.len(), 3, "3ウィンドウが存在すること");

    // 各ウィンドウに順番にフォーカスを移動
    let win_ids: Vec<i32> = windows.iter().map(|w| w.id).collect();
    for &target_id in &win_ids {
        session
            .switch_to_window(target_id)
            .expect("ウィンドウ切り替え成功");

        let current = session.windows();
        let active = current.iter().find(|w| w.is_active).unwrap();
        eprintln!(
            "[Task 4.2] フォーカス移動: target={}, active={}",
            target_id, active.id
        );
        assert_eq!(
            active.id, target_id,
            "switch_to_window後にアクティブウィンドウが更新されること"
        );

        // アクティブウィンドウは常に1つ
        let active_count = current.iter().filter(|w| w.is_active).count();
        assert_eq!(active_count, 1, "アクティブウィンドウは常に1つ");
    }
}

#[test]
fn integration_split_event_sequence() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("event seq").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.2] === 分割時のイベントシーケンステスト ===");

    drain_events(&mut session);

    // :split 実行 → WindowCreated + LayoutChanged が発火すること
    let tx = session.execute_ex_command(":split").expect("split成功");
    let events = tx.events;

    eprintln!("[Task 4.2] split後のイベント: {:?}", events);

    let has_win_new = events
        .iter()
        .any(|event| matches!(event, CoreEvent::WindowCreated { .. }));
    let has_layout_changed = events
        .iter()
        .any(|event| matches!(event, CoreEvent::LayoutChanged));

    assert!(has_win_new, ":split後にWinNewイベントが発火されること");
    assert!(
        has_layout_changed,
        ":split後にLayoutChangedイベントが発火されること"
    );
}

#[test]
fn integration_resize_event_timing() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("resize timing").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.2] === リサイズイベントのタイミングテスト ===");

    let split_tx = session.execute_ex_command(":split").expect("split成功");
    assert!(!split_tx.events.is_empty());

    // リサイズ → LayoutChanged が即座に発火
    let resize_tx = session.execute_ex_command(":resize 8").expect("resize成功");
    let events = resize_tx.events;

    eprintln!("[Task 4.2] resize後のイベント: {:?}", events);

    let has_layout_changed = events
        .iter()
        .any(|event| matches!(event, CoreEvent::LayoutChanged));
    assert!(
        has_layout_changed,
        ":resize後にLayoutChangedイベントが即座に発火されること"
    );

    // LayoutChanged後のスナップショットは新しいジオメトリを反映
    let windows = session.windows();
    let active = windows.iter().find(|w| w.is_active).unwrap();
    eprintln!(
        "[Task 4.2] LayoutChanged後のアクティブウィンドウ高さ: {}",
        active.height
    );
    assert_eq!(
        active.height, 8,
        "LayoutChanged後のジオメトリが新しいサイズを反映していること"
    );
}

#[test]
fn integration_window_close_updates_layout() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("close test").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.2] === ウィンドウクローズ後のレイアウト更新テスト ===");

    // 2ウィンドウに分割
    session.execute_ex_command(":split").expect("split成功");
    assert_eq!(session.windows().len(), 2, "分割後は2ウィンドウ");
    drain_events(&mut session);

    // アクティブウィンドウを閉じる
    session.execute_ex_command(":close").expect("close成功");

    let windows = session.windows();
    eprintln!(
        "[Task 4.2] ウィンドウ閉じ後: ウィンドウ数={}, {:?}",
        windows.len(),
        windows
            .iter()
            .map(|w| (w.id, w.is_active, w.height))
            .collect::<Vec<_>>()
    );
    assert_eq!(windows.len(), 1, "ウィンドウを閉じた後は1ウィンドウ");

    // 残ったウィンドウがアクティブであること
    assert!(windows[0].is_active, "残ったウィンドウはアクティブ");

    // 残ったウィンドウがフルスクリーンサイズを使用していること
    // （Vimはコマンドラインに1行使うため、height = screen_rows - 1）
    eprintln!(
        "[Task 4.2] 残ウィンドウの高さ: {} (スクリーン: 24)",
        windows[0].height
    );
    assert!(
        windows[0].height >= 20,
        "ウィンドウ閉じ後に残ったウィンドウがほぼフルスクリーンを使用すること: got {}",
        windows[0].height
    );
}

#[test]
fn integration_multi_split_geometry_consistency() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("multi split").expect("セッション初期化に成功すること");
    session.set_screen_size(30, 100);

    eprintln!("[Task 4.2] === 複数分割後のジオメトリ一貫性テスト ===");

    // 水平分割 × 2 + 垂直分割 × 1 で4ウィンドウ
    session.execute_ex_command(":split").expect("split成功");
    session.execute_ex_command(":split").expect("split成功");
    session.execute_ex_command(":vsplit").expect("vsplit成功");

    let windows = session.windows();
    eprintln!(
        "[Task 4.2] 複数分割後のウィンドウ: {:?}",
        windows
            .iter()
            .map(|w| (w.id, w.row, w.col, w.width, w.height))
            .collect::<Vec<_>>()
    );

    assert_eq!(windows.len(), 4, "4ウィンドウが存在すること");

    // すべてのウィンドウが正のサイズを持つこと
    for win in &windows {
        assert!(
            win.width > 0 && win.height > 0,
            "すべてのウィンドウが正のサイズを持つこと: win_id={}, width={}, height={}",
            win.id,
            win.width,
            win.height
        );
    }

    // ウィンドウが重ならないこと（簡易チェック: 同じrow,colの組み合わせがないこと）
    let windows_positions: Vec<(usize, usize)> = windows.iter().map(|w| (w.row, w.col)).collect();
    let mut unique_positions = windows_positions.clone();
    unique_positions.sort();
    unique_positions.dedup();
    assert_eq!(
        unique_positions.len(),
        windows_positions.len(),
        "すべてのウィンドウが異なる位置にあること"
    );
}

#[test]
fn integration_window_buffer_association_after_split_and_enew() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("win buf assoc").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.2] === 分割+enew後のウィンドウ-バッファ紐付けテスト ===");

    let initial_buf_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;

    // :split → 両方同じバッファ
    session.execute_ex_command(":split").expect("split成功");
    let windows = session.windows();
    eprintln!(
        "[Task 4.2] split後: {:?}",
        windows.iter().map(|w| (w.id, w.buf_id)).collect::<Vec<_>>()
    );
    for win in &windows {
        assert_eq!(
            win.buf_id, initial_buf_id,
            "split直後は両ウィンドウが同じバッファを表示: win_id={}, buf_id={}",
            win.id, win.buf_id
        );
    }

    // アクティブウィンドウで :enew → 別バッファに
    session.execute_ex_command(":enew").expect("enew成功");
    let new_buf_id = session.buffers().iter().find(|b| b.is_active).unwrap().id;

    let windows = session.windows();
    let active_win = windows.iter().find(|w| w.is_active).unwrap();
    let inactive_win = windows.iter().find(|w| !w.is_active).unwrap();

    eprintln!(
        "[Task 4.2] enew後: active_win(id={}, buf_id={}), inactive_win(id={}, buf_id={})",
        active_win.id, active_win.buf_id, inactive_win.id, inactive_win.buf_id
    );

    assert_eq!(
        active_win.buf_id, new_buf_id,
        "アクティブウィンドウは新バッファを表示"
    );
    assert_eq!(
        inactive_win.buf_id, initial_buf_id,
        "非アクティブウィンドウは元のバッファを表示"
    );
}

#[test]
fn integration_screen_resize_updates_all_window_geometry() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("screen resize").expect("セッション初期化に成功すること");
    session.set_screen_size(24, 80);

    eprintln!("[Task 4.2] === スクリーンリサイズ後の全ウィンドウジオメトリ更新テスト ===");

    session.execute_ex_command(":split").expect("split成功");
    drain_events(&mut session);

    let before = session.windows();
    eprintln!(
        "[Task 4.2] リサイズ前: {:?}",
        before
            .iter()
            .map(|w| (w.id, w.width, w.height))
            .collect::<Vec<_>>()
    );

    // スクリーンサイズを大きく変更
    session.set_screen_size(40, 120);

    let after = session.windows();
    eprintln!(
        "[Task 4.2] リサイズ後: {:?}",
        after
            .iter()
            .map(|w| (w.id, w.width, w.height))
            .collect::<Vec<_>>()
    );

    // 幅が新スクリーンサイズに合わせて更新されていること
    for win in &after {
        assert!(
            win.width > 80,
            "スクリーン拡大後のウィンドウ幅が更新されていること: win_id={}, width={}",
            win.id,
            win.width
        );
    }

    // LayoutChangedイベントが発火されていること
    let events = drain_events(&mut session);
    let has_layout_changed = events
        .iter()
        .any(|event| matches!(event, CoreEvent::LayoutChanged));
    eprintln!("[Task 4.2] スクリーンリサイズ後のイベント: {:?}", events);
    assert!(
        has_layout_changed,
        "スクリーンリサイズ後にLayoutChangedイベントが発火されること"
    );
}
