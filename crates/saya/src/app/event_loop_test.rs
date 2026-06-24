use super::*;
use crate::input::router::KeyInput;

// ---- タスク 7.1: 単一ループで順序制御するテスト ----

#[tokio::test]
async fn input_event_dispatches_through_coordinator() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender
        .send(UiEvent::Input(KeyInput::Char('j')))
        .await
        .expect("send should succeed");

    let action = coordinator.next_action().await;
    assert_eq!(
        action,
        LoopAction::NeedRedraw,
        "入力イベントは NeedRedraw を返すこと"
    );
}

#[tokio::test]
async fn redraw_event_dispatches_through_coordinator() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender
        .send(UiEvent::Redraw)
        .await
        .expect("send should succeed");

    let action = coordinator.next_action().await;
    assert_eq!(
        action,
        LoopAction::NeedRedraw,
        "再描画イベントは NeedRedraw を返すこと"
    );
}

#[tokio::test]
async fn resize_event_dispatches_through_coordinator() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender
        .send(UiEvent::Resize {
            columns: 120,
            rows: 40,
        })
        .await
        .expect("send should succeed");

    let action = coordinator.next_action().await;
    assert_eq!(
        action,
        LoopAction::NeedRedraw,
        "リサイズイベントは NeedRedraw を返すこと"
    );
}

#[tokio::test]
async fn mouse_click_event_dispatches_through_coordinator() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender
        .send(UiEvent::MouseClick { column: 3, row: 4 })
        .await
        .expect("send should succeed");

    let action = coordinator.next_action().await;
    assert_eq!(
        action,
        LoopAction::NeedRedraw,
        "マウスクリックイベントは NeedRedraw を返すこと"
    );
}

#[tokio::test]
async fn pasted_text_event_dispatches_through_coordinator() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender
        .send(UiEvent::PastedText("ab".to_string()))
        .await
        .expect("send should succeed");

    let action = coordinator.next_action().await;
    assert_eq!(
        action,
        LoopAction::NeedRedraw,
        "ペーストイベントは NeedRedraw を返すこと"
    );
}

#[tokio::test]
async fn terminal_resume_event_requests_redraw_and_preserves_size_payload() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender
        .send(UiEvent::TerminalResumed {
            columns: 132,
            rows: 43,
        })
        .await
        .expect("send should succeed");

    let action = coordinator.next_action().await;
    assert_eq!(
        action,
        LoopAction::NeedRedraw,
        "foreground 復帰は redraw を要求すること"
    );
    let drained = coordinator.drain_pending();
    assert_eq!(
        drained,
        vec![UiEvent::TerminalResumed {
            columns: 132,
            rows: 43,
        }],
        "復帰時の terminal size は main loop へ渡すこと"
    );
}

#[tokio::test]
async fn terminal_suspend_request_is_preserved_without_preemptive_redraw() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender
        .send(UiEvent::TerminalSuspendRequested)
        .await
        .expect("send should succeed");

    let action = coordinator.next_action().await;
    assert_eq!(
        action,
        LoopAction::Continue,
        "suspend 前は redraw ではなく terminal release を優先すること"
    );
    let drained = coordinator.drain_pending();
    assert_eq!(drained, vec![UiEvent::TerminalSuspendRequested]);
    assert!(!coordinator.take_redraw_pending());
}

#[tokio::test]
async fn shutdown_event_returns_exit_action() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender
        .send(UiEvent::Shutdown(ShutdownReason::UserQuit))
        .await
        .expect("send should succeed");

    let action = coordinator.next_action().await;
    assert_eq!(
        action,
        LoopAction::Exit(ShutdownReason::UserQuit),
        "終了イベントは Exit を返すこと"
    );
}

#[tokio::test]
async fn force_quit_shutdown_returns_force_quit_exit() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender
        .send(UiEvent::Shutdown(ShutdownReason::UserForceQuit))
        .await
        .expect("send should succeed");

    let action = coordinator.next_action().await;
    assert_eq!(
        action,
        LoopAction::Exit(ShutdownReason::UserForceQuit),
        "強制終了イベントは UserForceQuit の Exit を返すこと"
    );
}

#[tokio::test]
async fn all_event_types_pass_through_same_dispatch_path() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    // 全イベント種別を送信
    sender
        .send(UiEvent::Input(KeyInput::Char('a')))
        .await
        .unwrap();
    sender.send(UiEvent::Redraw).await.unwrap();
    sender
        .send(UiEvent::Resize {
            columns: 80,
            rows: 24,
        })
        .await
        .unwrap();
    sender
        .send(UiEvent::Shutdown(ShutdownReason::UserQuit))
        .await
        .unwrap();

    // 全て同じ coordinator.next_action() で処理される
    let action1 = coordinator.next_action().await;
    let action2 = coordinator.next_action().await;
    let action3 = coordinator.next_action().await;
    let action4 = coordinator.next_action().await;

    assert_eq!(action1, LoopAction::NeedRedraw);
    assert_eq!(action2, LoopAction::NeedRedraw);
    assert_eq!(action3, LoopAction::NeedRedraw);
    assert_eq!(
        action4,
        LoopAction::Exit(ShutdownReason::UserQuit),
        "全イベント種別が同じ dispatch 経路を通ること"
    );
}

#[tokio::test]
async fn channel_close_triggers_shutdown() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    // sender を drop して channel を閉じる
    drop(sender);

    let action = coordinator.next_action().await;
    assert_eq!(
        action,
        LoopAction::Exit(ShutdownReason::UserQuit),
        "channel が閉じたら shutdown になること"
    );
}

#[tokio::test]
async fn coordinator_does_not_require_multiple_mutable_owners() {
    // coordinator は &mut self だけで動作する（複数の mutable owner を持たない）
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender
        .send(UiEvent::Input(KeyInput::Char('x')))
        .await
        .unwrap();
    // coordinator の &mut self のみで全操作が完結する
    let _action = coordinator.next_action().await;
    let _pending = coordinator.take_redraw_pending();

    // sender は clone 可能（複数 producer）
    let sender2 = sender.clone();
    sender2.send(UiEvent::Redraw).await.unwrap();
    let _action2 = coordinator.next_action().await;
}

#[tokio::test]
async fn events_maintain_order_in_channel() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    // 順序通りに送信
    sender
        .send(UiEvent::Input(KeyInput::Char('a')))
        .await
        .unwrap();
    sender
        .send(UiEvent::Input(KeyInput::Char('b')))
        .await
        .unwrap();
    sender
        .send(UiEvent::Shutdown(ShutdownReason::UserQuit))
        .await
        .unwrap();

    // 受信も同じ順序であること
    let a1 = coordinator.next_action().await;
    let a2 = coordinator.next_action().await;
    let a3 = coordinator.next_action().await;

    assert_eq!(a1, LoopAction::NeedRedraw, "1 番目: input 'a'");
    assert_eq!(a2, LoopAction::NeedRedraw, "2 番目: input 'b'");
    assert_eq!(
        a3,
        LoopAction::Exit(ShutdownReason::UserQuit),
        "3 番目: shutdown"
    );
}

#[tokio::test]
async fn bounded_channel_has_limited_capacity() {
    let (coordinator, sender) = EventLoopCoordinator::with_capacity(2);

    // 2 個まで即座に送信できる
    sender.send(UiEvent::Redraw).await.unwrap();
    sender.send(UiEvent::Redraw).await.unwrap();

    // 3 個目は bounded なので try_send が失敗する
    let result = sender.try_send(UiEvent::Redraw);
    assert!(
        result.is_err(),
        "bounded channel の容量超過で try_send が失敗すること"
    );

    drop(coordinator);
}

// ---- タスク 7.2: redraw 要求の集約テスト ----

#[tokio::test]
async fn coalesce_drains_multiple_redraw_events_into_single_flag() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    // 高頻度で Redraw を連続送信
    sender.send(UiEvent::Redraw).await.unwrap();
    sender.send(UiEvent::Redraw).await.unwrap();
    sender.send(UiEvent::Redraw).await.unwrap();

    // 最初の recv で 1 つ処理
    let action = coordinator.next_action().await;
    assert_eq!(action, LoopAction::NeedRedraw);

    // drain_pending で残りを集約
    let drained = coordinator.drain_pending();
    log::debug!("[test] drained events: {:?}", drained);

    // redraw_pending は true（集約済み）
    assert!(
        coordinator.is_redraw_pending(),
        "drain 後に redraw_pending が立っていること"
    );

    // take_redraw_pending で 1 回分として消費
    let pending = coordinator.take_redraw_pending();
    assert!(pending, "集約された redraw が 1 回として取得できること");
    assert!(
        !coordinator.is_redraw_pending(),
        "take 後は redraw_pending が解除されること"
    );
}

#[tokio::test]
async fn drain_pending_keeps_mouse_click_and_pasted_text_events() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender.send(UiEvent::Redraw).await.unwrap();
    sender
        .send(UiEvent::MouseClick { column: 1, row: 2 })
        .await
        .unwrap();
    sender
        .send(UiEvent::PastedText("xy".to_string()))
        .await
        .unwrap();

    let action = coordinator.next_action().await;
    assert_eq!(action, LoopAction::NeedRedraw);

    let drained = coordinator.drain_pending();

    assert_eq!(
        drained,
        vec![
            UiEvent::MouseClick { column: 1, row: 2 },
            UiEvent::PastedText("xy".to_string()),
        ],
        "Redraw 以外の非キーイベントは drain 後も保持されること"
    );
    assert!(coordinator.is_redraw_pending());
}

#[tokio::test]
async fn coalesce_preserves_non_redraw_events_during_drain() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    // Redraw の間に Input が混在
    sender.send(UiEvent::Redraw).await.unwrap();
    sender
        .send(UiEvent::Input(KeyInput::Char('a')))
        .await
        .unwrap();
    sender.send(UiEvent::Redraw).await.unwrap();

    // 最初の Redraw を処理
    let action = coordinator.next_action().await;
    assert_eq!(action, LoopAction::NeedRedraw);

    // drain で残りを集約
    let drained = coordinator.drain_pending();

    // Input イベントは drain 結果に含まれる（消えない）
    assert!(
        drained.iter().any(|e| matches!(e, UiEvent::Input(_))),
        "drain 中に Input イベントが失われないこと: {:?}",
        drained
    );
}

#[tokio::test]
async fn coalesce_shutdown_takes_priority_during_drain() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender.send(UiEvent::Redraw).await.unwrap();
    sender
        .send(UiEvent::Shutdown(ShutdownReason::UserQuit))
        .await
        .unwrap();
    sender.send(UiEvent::Redraw).await.unwrap();

    // 最初の Redraw を処理
    coordinator.next_action().await;

    // drain で残りを集約
    let drained = coordinator.drain_pending();

    // Shutdown が drain 結果に含まれる
    assert!(
        drained.iter().any(|e| matches!(e, UiEvent::Shutdown(_))),
        "drain 中に Shutdown イベントが失われないこと: {:?}",
        drained
    );
}

#[tokio::test]
async fn redraw_does_not_accumulate_infinitely_in_channel() {
    let (mut coordinator, sender) = EventLoopCoordinator::with_capacity(4);

    // 4 個の Redraw を送信（channel 容量 = 4）
    for _ in 0..4 {
        sender.send(UiEvent::Redraw).await.unwrap();
    }

    // 1 回 recv してから drain
    coordinator.next_action().await;
    let drained = coordinator.drain_pending();

    // drain で全部消化され、redraw_pending は 1 回分
    assert!(
        coordinator.is_redraw_pending(),
        "drain 後に redraw_pending が立っていること"
    );

    // channel は空になっている
    assert!(
        drained.len() <= 3,
        "drain で最大 3 個のイベントが取り出されること（最初の 1 個は recv 済み）: {}",
        drained.len()
    );
}

#[tokio::test]
async fn drain_returns_empty_when_no_pending_events() {
    let (mut coordinator, sender) = EventLoopCoordinator::new();

    sender.send(UiEvent::Redraw).await.unwrap();

    // 1 個だけ recv
    coordinator.next_action().await;

    // channel にはもう何もない。Redrawは pending_events に入らない
    let drained = coordinator.drain_pending();
    assert!(
        drained.is_empty(),
        "pending がない場合は空の Vec を返すこと"
    );
}

// ---- タスク 7.3: shutdown 順序固定テスト ----

#[test]
fn shutdown_sequence_records_steps_in_fixed_order() {
    let mut seq = ShutdownSequence::new();

    seq.record_quit_requested(ShutdownReason::UserQuit);
    seq.record_loop_stopped();
    seq.record_session_released();
    seq.record_terminal_restored(Ok(()));

    let steps = seq.steps();
    assert_eq!(
        steps,
        &[
            ShutdownStep::QuitRequested,
            ShutdownStep::LoopStopped,
            ShutdownStep::SessionReleased,
            ShutdownStep::TerminalRestored,
        ],
        "shutdown 手順が固定順序で記録されること"
    );
}

#[test]
fn shutdown_sequence_terminal_restore_runs_even_after_session_release_failure() {
    let mut seq = ShutdownSequence::new();

    seq.record_quit_requested(ShutdownReason::UserQuit);
    seq.record_loop_stopped();
    // session 解放は失敗を想定してスキップ（ここでは省略して順序テスト）
    seq.record_terminal_restored(Ok(()));

    let steps = seq.steps();
    // terminal restore が最後に実行されていること
    assert_eq!(
        steps.last(),
        Some(&ShutdownStep::TerminalRestored),
        "失敗時でも terminal restore が最後に実行されること"
    );
}

#[test]
fn shutdown_sequence_records_restore_error() {
    let mut seq = ShutdownSequence::new();

    seq.record_quit_requested(ShutdownReason::UserForceQuit);
    seq.record_loop_stopped();
    seq.record_session_released();
    seq.record_terminal_restored(Err("restore failed".to_string()));

    assert!(
        seq.has_restore_error(),
        "terminal restore エラーが記録されること"
    );
    assert_eq!(
        seq.restore_error(),
        Some("restore failed"),
        "restore エラーメッセージが取得できること"
    );
}

#[test]
fn shutdown_sequence_is_complete_only_when_all_steps_done() {
    let mut seq = ShutdownSequence::new();

    assert!(!seq.is_complete(), "空の時は未完了");

    seq.record_quit_requested(ShutdownReason::UserQuit);
    assert!(!seq.is_complete(), "quit だけでは未完了");

    seq.record_loop_stopped();
    assert!(!seq.is_complete(), "loop 停止だけでは未完了");

    seq.record_session_released();
    assert!(!seq.is_complete(), "session 解放だけでは未完了");

    seq.record_terminal_restored(Ok(()));
    assert!(seq.is_complete(), "全 4 ステップ完了で complete");
}

#[test]
fn shutdown_sequence_restore_always_last_even_with_force_quit() {
    let mut seq = ShutdownSequence::new();

    seq.record_quit_requested(ShutdownReason::UserForceQuit);
    seq.record_loop_stopped();
    seq.record_session_released();
    seq.record_terminal_restored(Ok(()));

    let steps = seq.steps();
    assert_eq!(
        steps.last(),
        Some(&ShutdownStep::TerminalRestored),
        "強制終了でも terminal restore が最後に来ること"
    );
    assert_eq!(
        steps.first(),
        Some(&ShutdownStep::QuitRequested),
        "終了要求が最初に来ること"
    );
}
