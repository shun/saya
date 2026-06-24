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
    Input(crate::input::router::KeyInput),
    /// 再描画要求
    Redraw,
    /// 画面リサイズ
    Resize { columns: u16, rows: u16 },
    /// 終了要求
    Shutdown(ShutdownReason),
    /// 左クリック入力
    MouseClick { column: u16, row: u16 },
    /// mouse wheel 入力
    MouseWheel {
        column: u16,
        row: u16,
        delta_x: i16,
        delta_y: i16,
    },
    /// ブラケットペースト入力
    PastedText(String),
    /// job control による suspend 要求
    TerminalSuspendRequested,
    /// foreground 復帰後の terminal 再初期化要求
    TerminalResumed { columns: u16, rows: u16 },
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
                    columns,
                    rows
                );
                self.redraw_pending = true;
                LoopAction::NeedRedraw
            }
            UiEvent::MouseClick { column, row } => {
                log::debug!(
                    "[event_loop] mouse click event processed: column={}, row={}",
                    column,
                    row
                );
                self.redraw_pending = true;
                LoopAction::NeedRedraw
            }
            UiEvent::MouseWheel {
                column,
                row,
                delta_x,
                delta_y,
            } => {
                log::debug!(
                    "[event_loop] mouse wheel event processed: column={}, row={}, delta=({}, {})",
                    column,
                    row,
                    delta_x,
                    delta_y
                );
                self.redraw_pending = true;
                LoopAction::NeedRedraw
            }
            UiEvent::PastedText(text) => {
                log::debug!(
                    "[event_loop] pasted text event processed: chars={}",
                    text.chars().count()
                );
                self.redraw_pending = true;
                LoopAction::NeedRedraw
            }
            UiEvent::TerminalSuspendRequested => {
                log::debug!("[event_loop] terminal suspend request processed");
                LoopAction::Continue
            }
            UiEvent::TerminalResumed { columns, rows } => {
                log::debug!(
                    "[event_loop] terminal resume event processed: columns={}, rows={}",
                    columns,
                    rows
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

impl Default for ShutdownSequence {
    fn default() -> Self {
        Self::new()
    }
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
#[path = "event_loop_test.rs"]
mod tests;
