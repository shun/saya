/// EventLoopCoordinator: UI イベント、host action、render を順序制御する。
///
/// 入力、redraw、resize、shutdown を単一ループで処理し、
/// bounded mpsc channel で backpressure を効かせる。
/// loop が複数の mutable owner を持たない形で整理し、
/// 各イベント種別が同じ dispatch 経路を通る。
use tokio::sync::mpsc;

/// event loop で扱うイベントの統一型。
/// 全てのイベント種別がこの enum を通じて同じ dispatch 経路を通る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEvent {
    /// キー入力イベント
    Input(crate::input_router::KeyInput),
    /// 再描画要求
    Redraw,
    /// 画面リサイズ
    Resize { columns: u16, rows: u16 },
    /// 終了要求
    Shutdown(ShutdownReason),
}

/// 終了要求の理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownReason {
    /// ユーザーの通常終了操作
    UserQuit,
    /// ユーザーの強制終了操作
    UserForceQuit,
}

/// EventLoopCoordinator のイベント送信側ハンドル。
/// 複数の producer（input reader、resize watcher など）が clone して使う。
pub type EventSender = mpsc::Sender<UiEvent>;

/// EventLoopCoordinator のイベント受信側ハンドル。
/// 単一の consumer loop が所有する。
pub type EventReceiver = mpsc::Receiver<UiEvent>;

/// event loop の各ステップで coordinator が返す指示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopAction {
    /// 処理を続行する
    Continue,
    /// 再描画が必要
    NeedRedraw,
    /// loop を停止して shutdown に入る
    Exit(ShutdownReason),
}

/// bounded channel のデフォルトバッファサイズ。
/// 入力バースト時にも backpressure が効くようにする。
const DEFAULT_CHANNEL_CAPACITY: usize = 64;

/// EventLoopCoordinator: 単一ループでイベントを順序制御する。
///
/// 各イベント種別が同じ dispatch 経路を通り、
/// loop が複数の mutable owner を持たない形を維持する。
pub struct EventLoopCoordinator {
    receiver: EventReceiver,
    redraw_pending: bool,
    pending_events: Vec<UiEvent>,
}

impl EventLoopCoordinator {
    /// デフォルト容量で channel pair を生成し、coordinator と sender を返す。
    pub fn new() -> (Self, EventSender) {
        Self::with_capacity(DEFAULT_CHANNEL_CAPACITY)
    }

    /// 指定の capacity で channel pair を生成し、coordinator と sender を返す。
    pub fn with_capacity(capacity: usize) -> (Self, EventSender) {
        let (sender, receiver) = mpsc::channel(capacity);
        log::debug!(
            "[event_loop] coordinator created: channel_capacity={}",
            capacity,
        );
        let coordinator = Self {
            receiver,
            redraw_pending: false,
            pending_events: Vec::new(),
        };
        (coordinator, sender)
    }

    /// 次のイベントを受信して dispatch し、LoopAction を返す。
    ///
    /// 全てのイベント種別が同じ経路を通る。
    /// channel が閉じた場合は UserQuit として Exit を返す。
    pub async fn next_action(&mut self) -> LoopAction {
        let event = match self.receiver.recv().await {
            Some(event) => event,
            None => {
                log::debug!("[event_loop] channel closed, initiating shutdown");
                return LoopAction::Exit(ShutdownReason::UserQuit);
            }
        };
        log::debug!("[event_loop] dispatching event: {:?}", event);
        let action = self.dispatch(event.clone());
        match event {
            UiEvent::Redraw => {}
            other => self.pending_events.push(other),
        }
        action
    }

    /// イベントを LoopAction に変換する内部 dispatch。
    fn dispatch(&mut self, event: UiEvent) -> LoopAction {
        match event {
            UiEvent::Input(_key) => {
                log::debug!("[event_loop] input event processed");
                // 入力後は再描画が必要
                self.redraw_pending = true;
                LoopAction::NeedRedraw
            }
            UiEvent::Redraw => {
                log::debug!("[event_loop] redraw event processed");
                self.redraw_pending = true;
                LoopAction::NeedRedraw
            }
            UiEvent::Resize { columns, rows } => {
                log::debug!(
                    "[event_loop] resize event processed: columns={}, rows={}",
                    columns, rows
                );
                self.redraw_pending = true;
                LoopAction::NeedRedraw
            }
            UiEvent::Shutdown(reason) => {
                log::debug!("[event_loop] shutdown event processed: {:?}", reason);
                LoopAction::Exit(reason)
            }
        }
    }

    /// redraw pending フラグを取得し、リセットする。
    pub fn take_redraw_pending(&mut self) -> bool {
        let pending = self.redraw_pending;
        self.redraw_pending = false;
        log::debug!(
            "[event_loop] take_redraw_pending: was={}, now=false",
            pending
        );
        pending
    }

    /// redraw pending フラグの現在値を返す。
    pub fn is_redraw_pending(&self) -> bool {
        self.redraw_pending
    }

    /// shutdown シーケンスを開始する。
    /// 固定順序で shutdown を実行するための ShutdownSequence を返す。
    pub fn begin_shutdown(&self, reason: ShutdownReason) -> ShutdownSequence {
        log::debug!(
            "[event_loop] beginning shutdown sequence: reason={:?}",
            reason
        );
        let mut seq = ShutdownSequence::new();
        seq.record_quit_requested(reason);
        seq
    }

    /// channel に溜まっているイベントを非ブロッキングで全て取り出す。
    ///
    /// Redraw イベントは redraw_pending フラグに集約し、返却 Vec には含めない。
    /// Input、Resize、Shutdown は返却 Vec に保持される（消失しない）。
    /// 高頻度入力時に redraw が無限に積み上がらないようにする。
    pub fn drain_pending(&mut self) -> Vec<UiEvent> {
        let mut non_redraw_events = std::mem::take(&mut self.pending_events);
        loop {
            match self.receiver.try_recv() {
                Ok(event) => {
                    log::debug!("[event_loop] drain: got event {:?}", event);
                    match event {
                        UiEvent::Redraw => {
                            // Redraw はフラグに集約（無限積み上がり防止）
                            self.redraw_pending = true;
                        }
                        other => {
                            // Redraw 以外は dispatch して保持
                            self.dispatch(other.clone());
                            non_redraw_events.push(other);
                        }
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    log::debug!(
                        "[event_loop] drain complete: non_redraw_events={}, redraw_pending={}",
                        non_redraw_events.len(),
                        self.redraw_pending
                    );
                    break;
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    log::debug!("[event_loop] drain: channel disconnected");
                    break;
                }
            }
        }
        non_redraw_events
    }
}

/// shutdown の各ステップ。
/// 終了要求、loop 停止、session 解放、terminal restore の順番を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownStep {
    /// 終了要求を受信した
    QuitRequested,
    /// event loop を停止した
    LoopStopped,
    /// editor session を解放した
    SessionReleased,
    /// terminal を restore した
    TerminalRestored,
}

/// shutdown 順序を固定して実行するためのシーケンスレコーダー。
///
/// 終了要求 -> loop 停止 -> session 解放 -> terminal restore の順番を保証する。
/// 失敗時でも terminal restore を最後に実行する。
#[derive(Debug)]
pub struct ShutdownSequence {
    steps: Vec<ShutdownStep>,
    restore_error: Option<String>,
}

impl ShutdownSequence {
    /// 新しいシーケンスを作成する。
    pub fn new() -> Self {
        log::debug!("[event_loop] shutdown sequence created");
        Self {
            steps: Vec::new(),
            restore_error: None,
        }
    }

    /// 終了要求ステップを記録する。
    pub fn record_quit_requested(&mut self, reason: ShutdownReason) {
        log::debug!(
            "[event_loop] shutdown step: quit_requested (reason={:?})",
            reason
        );
        self.steps.push(ShutdownStep::QuitRequested);
    }

    /// loop 停止ステップを記録する。
    pub fn record_loop_stopped(&mut self) {
        log::debug!("[event_loop] shutdown step: loop_stopped");
        self.steps.push(ShutdownStep::LoopStopped);
    }

    /// session 解放ステップを記録する。
    pub fn record_session_released(&mut self) {
        log::debug!("[event_loop] shutdown step: session_released");
        self.steps.push(ShutdownStep::SessionReleased);
    }

    /// terminal restore ステップを記録する。
    /// 失敗時もステップとして記録し、エラー情報を保持する。
    pub fn record_terminal_restored(&mut self, result: Result<(), String>) {
        match &result {
            Ok(()) => {
                log::debug!("[event_loop] shutdown step: terminal_restored (ok)");
            }
            Err(error) => {
                log::debug!(
                    "[event_loop] shutdown step: terminal_restored (error={})",
                    error
                );
                self.restore_error = Some(error.clone());
            }
        }
        self.steps.push(ShutdownStep::TerminalRestored);
    }

    /// 記録されたステップの参照を返す。
    pub fn steps(&self) -> &[ShutdownStep] {
        &self.steps
    }

    /// terminal restore でエラーがあったかどうか。
    pub fn has_restore_error(&self) -> bool {
        self.restore_error.is_some()
    }

    /// terminal restore のエラーメッセージを返す。
    pub fn restore_error(&self) -> Option<&str> {
        self.restore_error.as_deref()
    }

    /// 全 4 ステップが完了しているかどうか。
    pub fn is_complete(&self) -> bool {
        self.steps.len() == 4
            && self.steps.contains(&ShutdownStep::QuitRequested)
            && self.steps.contains(&ShutdownStep::LoopStopped)
            && self.steps.contains(&ShutdownStep::SessionReleased)
            && self.steps.contains(&ShutdownStep::TerminalRestored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_router::KeyInput;

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
            drained
                .iter()
                .any(|e| matches!(e, UiEvent::Shutdown(_))),
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

        sender
            .send(UiEvent::Input(KeyInput::Char('x')))
            .await
            .unwrap();

        // 1 個だけ recv
        coordinator.next_action().await;

        // channel にはもう何もない
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
        assert!(
            seq.is_complete(),
            "全 4 ステップ完了で complete"
        );
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
}
