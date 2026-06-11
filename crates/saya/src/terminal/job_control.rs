use crate::app::event_loop::{EventSender, UiEvent};
use std::io;

#[cfg(unix)]
use signal_hook::consts::signal::{SIGCONT, SIGTSTP, SIGWINCH};

#[cfg(unix)]
pub struct JobControlSignalWatcher {
    handle: signal_hook::iterator::Handle,
    join: Option<std::thread::JoinHandle<()>>,
}

#[cfg(not(unix))]
pub struct JobControlSignalWatcher;

#[cfg(unix)]
impl Drop for JobControlSignalWatcher {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(join) = self.join.take() {
            if let Err(error) = join.join() {
                log::debug!("[job_control] signal watcher join failed: {:?}", error);
            }
        }
    }
}

pub fn start_job_control_signal_watcher(
    sender: EventSender,
) -> io::Result<Option<JobControlSignalWatcher>> {
    start_job_control_signal_watcher_inner(sender)
}

#[cfg(unix)]
fn start_job_control_signal_watcher_inner(
    sender: EventSender,
) -> io::Result<Option<JobControlSignalWatcher>> {
    let mut signals = signal_hook::iterator::Signals::new([SIGTSTP, SIGCONT, SIGWINCH])?;
    let handle = signals.handle();
    let join = std::thread::Builder::new()
        .name("saya-job-control-signal-watcher".to_string())
        .spawn(move || {
            log::debug!("[job_control] signal watcher started");
            for signal in signals.forever() {
                let event = match signal {
                    SIGTSTP => {
                        log::debug!("[job_control] SIGTSTP observed; requesting terminal suspend");
                        UiEvent::TerminalSuspendRequested
                    }
                    SIGCONT => {
                        let (columns, rows) = current_terminal_size();
                        log::debug!(
                            "[job_control] SIGCONT observed; requesting terminal resume: columns={}, rows={}",
                            columns,
                            rows
                        );
                        UiEvent::TerminalResumed { columns, rows }
                    }
                    SIGWINCH => {
                        let (columns, rows) = current_terminal_size();
                        log::debug!(
                            "[job_control] SIGWINCH observed; forwarding resize: columns={}, rows={}",
                            columns,
                            rows
                        );
                        UiEvent::Resize { columns, rows }
                    }
                    other => {
                        log::debug!("[job_control] ignoring unexpected signal: {}", other);
                        continue;
                    }
                };

                if sender.blocking_send(event).is_err() {
                    log::debug!("[job_control] receiver closed; stopping signal watcher");
                    break;
                }
            }
            log::debug!("[job_control] signal watcher stopped");
        })?;

    Ok(Some(JobControlSignalWatcher {
        handle,
        join: Some(join),
    }))
}

#[cfg(not(unix))]
fn start_job_control_signal_watcher_inner(
    _sender: EventSender,
) -> io::Result<Option<JobControlSignalWatcher>> {
    Ok(None)
}

#[cfg(unix)]
pub fn suspend_current_process_for_job_control() -> io::Result<()> {
    log::debug!("[job_control] emulating default SIGTSTP handler");
    signal_hook::low_level::emulate_default_handler(SIGTSTP)
}

#[cfg(not(unix))]
pub fn suspend_current_process_for_job_control() -> io::Result<()> {
    log::debug!("[job_control] job-control suspend requested on unsupported platform");
    Ok(())
}

fn current_terminal_size() -> (u16, u16) {
    crossterm::terminal::size().unwrap_or((80, 24))
}

/// ジョブコントロール関連の診断トレースを出力する。`SAYA_TRACE_JOB_CONTROL`
/// が設定されている場合は標準エラーにも出す。
pub fn trace_job_control_diagnostic(args: std::fmt::Arguments<'_>) {
    let message = args.to_string();
    log::debug!("[job_control_diagnostic] {message}");
    if std::env::var_os("SAYA_TRACE_JOB_CONTROL").is_some() {
        eprintln!("[saya-trace][job-control] {message}");
    }
}
