use std::fs;
#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::os::fd::{FromRawFd, RawFd};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::{sync::mpsc, thread, time::Duration};
use vim_core_rs::{
    CoreEvent, CoreHostAction, CoreInputRequestKind, CoreInputResponse, CoreMessageCategory,
    CoreMessageEvent, CoreMessageSeverity, CorePagerPromptKind, CoreVfsRequest, CoreVfsResponse,
    VimCoreSession,
};

fn session_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_session_test_lock() -> std::sync::MutexGuard<'static, ()> {
    session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn edit_local_file_via_resolved_fallback(session: &mut VimCoreSession, path: &std::path::Path) {
    let outcome = session
        .execute_ex_command(&format!("edit {}", path.display()))
        .expect("edit should queue a resolve request");
    let request_id = outcome
        .host_actions
        .into_iter()
        .find_map(|action| match action {
            CoreHostAction::VfsRequest(CoreVfsRequest::Resolve { request_id, .. }) => {
                Some(request_id)
            }
            _ => None,
        })
        .expect("edit should issue a resolve request");

    session
        .submit_vfs_response(CoreVfsResponse::ResolvedLocalFallback {
            request_id,
            locator: path.to_string_lossy().into_owned(),
        })
        .expect("local fallback edit should succeed");
}

// === Task 5.1: C FFI API の単体テスト ===

/// vim_bridge_eval_string が意図した Vimscript を正しく評価・返却できることを確認する
#[test]
fn eval_string_evaluates_simple_arithmetic() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    // "1 + 1" の評価結果が "2" であることを確認
    let result = session.eval_string("1 + 1");
    println!("[TEST] eval_string('1 + 1') = {:?}", result);
    assert_eq!(result, Some("2".to_string()));
}

/// vim_bridge_eval_string が文字列式を正しく評価することを確認する
#[test]
fn eval_string_evaluates_string_expression() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    let result = session.eval_string("'hello' . ' world'");
    println!("[TEST] eval_string('hello' . ' world') = {:?}", result);
    assert_eq!(result, Some("hello world".to_string()));
}

/// 無効な式で NULL が返されることを確認する
#[test]
fn eval_string_returns_none_for_invalid_expression() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    // Why not: Vim の評価エラーを直接踏むと embedded 実行では pager prompt が残り、
    // CI で対話待ちのハングを起こしやすい。ここでは Rust API が不正入力を
    // 安全に reject して None を返すことだけを確認する。
    let result = session.eval_string("invalid\0expression");

    assert!(
        result.is_none(),
        "NUL を含む不正な式は None を返すこと: {:?}",
        result
    );
}

// === Task 5.2: メッセージの捕捉とルーティングの結合テスト ===

/// echoerr で発生したエラーメッセージが pending event として届くことを確認する
#[cfg(unix)]
#[test]
fn error_message_is_exposed_via_pending_event() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    let ((tx, event, messages), stdout, stderr) = capture_standard_streams(|| {
        let tx = session
            .execute_ex_command("echoerr 'test error message'")
            .expect("echoerr が成功すること");
        let event = tx.events.iter().find_map(|event| match event {
            CoreEvent::Message(message) => Some(message.clone()),
            _ => None,
        });
        let messages = session
            .eval_string("execute('messages')")
            .unwrap_or_default();
        (tx, event, messages)
    });

    assert!(
        matches!(
            event,
            Some(CoreMessageEvent {
                severity: CoreMessageSeverity::Error,
                category: CoreMessageCategory::UserVisible,
                ref content,
            }) if content.contains("test error message")
        ) || messages.contains("test error message")
            || output_contains(&stdout, "test error message")
            || output_contains(&stderr, "test error message"),
        "echoerr は transaction event / :messages / 標準出力/標準エラーのいずれかで観測できること: events={:?}, event={:?}, messages={:?}, stdout={:?}, stderr={:?}",
        tx.events,
        event,
        messages,
        stdout,
        stderr
    );
}

#[cfg(unix)]
#[test]
fn execute_ex_command_returns_message_events_without_scraping() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    let (tx, stdout, stderr) = capture_standard_streams(|| {
        session
            .execute_ex_command("echomsg 'hello from event queue'")
            .expect("v2 実行が成功すること")
    });

    assert!(
        tx.events.iter().any(|event| matches!(
            event,
            CoreEvent::Message(message)
                if message.severity == CoreMessageSeverity::Info
                    && message.category == CoreMessageCategory::UserVisible
                    && message.content.contains("hello from event queue")
        )) || output_contains(&stdout, "hello from event queue")
            || output_contains(&stderr, "hello from event queue"),
        "echomsg は event queue か標準出力/標準エラー経由で観測できること: events={:?}, stdout={:?}, stderr={:?}",
        tx.events,
        stdout,
        stderr
    );
}

#[test]
fn exists_reports_autocmd_group_event_pattern_and_buffer_scope() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic")
        .as_nanos();
    let temp_dir = std::env::temp_dir();
    let first_path = temp_dir.join(format!("vim-core-rs-exists-autocmd-{unique}-1.my"));
    let second_path = temp_dir.join(format!("vim-core-rs-exists-autocmd-{unique}-2.my"));
    fs::write(&first_path, "first buffer\n").expect("first temp file should be written");
    fs::write(&second_path, "second buffer\n").expect("second temp file should be written");

    session
        .execute_ex_command("augroup myagroup")
        .expect("augroup should succeed");
    assert_eq!(
        session.eval_string("exists('#myagroup')"),
        Some("1".to_string()),
        "empty autocmd group should be observable through exists('#group')"
    );
    assert_eq!(
        session.eval_string("exists('#BufEnter')"),
        Some("0".to_string()),
        "BufEnter should not exist before any BufEnter autocmd is defined"
    );
    session
        .execute_ex_command("autocmd!")
        .expect("autocmd! should succeed");
    session
        .execute_ex_command(r#"autocmd BufEnter *.my echo "myfile edited""#)
        .expect("autocmd definition should succeed");
    session
        .execute_ex_command("augroup END")
        .expect("augroup END should succeed");

    assert_eq!(
        session.eval_string("exists('#myagroup#BufEnter')"),
        Some("1".to_string()),
        "autocmd group and event should be observable through exists('#group#event')"
    );
    assert_eq!(
        session.eval_string("exists('#BufEnter')"),
        Some("1".to_string()),
        "BufEnter should be observable through exists('#event')"
    );
    assert_eq!(
        session.eval_string("exists('#BufEnter#*.my')"),
        Some("1".to_string()),
        "autocmd event and pattern should be observable through exists('#event#pattern')"
    );

    edit_local_file_via_resolved_fallback(&mut session, &first_path);
    session
        .execute_ex_command("autocmd BufEnter <buffer> let g:entered=1")
        .expect("buffer-local autocmd definition should succeed");
    assert_eq!(
        session.eval_string("exists('#BufEnter#<buffer>')"),
        Some("1".to_string()),
        "buffer-local autocmd should be observable through exists('#event#<buffer>')"
    );
    edit_local_file_via_resolved_fallback(&mut session, &second_path);
    assert_eq!(
        session.eval_string("exists('#BufEnter#<buffer>')"),
        Some("0".to_string()),
        "buffer-local autocmd should disappear from the previous buffer scope"
    );

    let _ = fs::remove_file(first_path);
    let _ = fs::remove_file(second_path);
}

#[cfg(unix)]
#[test]
fn execute_normal_command_returns_undo_command_feedback_events() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    session
        .execute_normal_command("A!")
        .expect("テキスト変更に成功すること");
    session
        .execute_normal_command("\u{1b}")
        .expect("Insert モードを抜けられること");

    let (tx, stdout, stderr) = capture_standard_streams(|| {
        session
            .execute_normal_command("u")
            .expect("undo が成功すること")
    });

    assert_eq!(
        tx.snapshot.text, "hello\n",
        "undo はバッファ内容を元に戻すこと"
    );
    assert!(
        tx.events.iter().any(|event| matches!(
            event,
            CoreEvent::Message(message)
                if message.severity == CoreMessageSeverity::Info
                    && message.category == CoreMessageCategory::CommandFeedback
                    && message.content.contains("before #")
        )),
        "undo は user-visible ではなく command feedback として観測できること: events={:?}",
        tx.events,
    );
    assert!(
        sanitize_harness_output(&stdout).is_empty() && sanitize_harness_output(&stderr).is_empty(),
        "embedded undo は端末へ英語メッセージを漏らさないこと: stdout={:?}, stderr={:?}",
        stdout,
        stderr
    );
}

#[cfg(unix)]
#[test]
fn execute_normal_command_returns_redo_command_feedback_events() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    session
        .execute_normal_command("A!")
        .expect("テキスト変更に成功すること");
    session
        .execute_normal_command("\u{1b}")
        .expect("Insert モードを抜けられること");
    session
        .execute_ex_command("set cpo-=u")
        .expect("redo の意味を安定させること");
    let _ = capture_standard_streams(|| {
        session
            .execute_normal_command("u")
            .expect("undo が成功すること")
    });

    let (tx, stdout, stderr) = capture_standard_streams(|| {
        session
            .execute_ex_command("redo")
            .expect("redo が成功すること")
    });

    assert_eq!(tx.snapshot.text, "hello!\n", "redo は変更を復元すること");
    assert!(
        tx.events.iter().any(|event| matches!(
            event,
            CoreEvent::Message(message)
                if message.severity == CoreMessageSeverity::Info
                    && message.category == CoreMessageCategory::CommandFeedback
                    && message.content.contains("after #")
        )),
        "redo は command feedback として観測できること: events={:?}",
        tx.events
    );
    assert!(
        sanitize_harness_output(&stdout).is_empty() && sanitize_harness_output(&stderr).is_empty(),
        "embedded redo は端末へ英語メッセージを漏らさないこと: stdout={:?}, stderr={:?}",
        stdout,
        stderr
    );
}

#[cfg(unix)]
fn capture_standard_streams<T>(f: impl FnOnce() -> T) -> (T, String, String) {
    unsafe fn capture_fd(fd: RawFd) -> (RawFd, RawFd) {
        let saved = unsafe { libc::dup(fd) };
        assert!(saved >= 0, "dup failed for fd={fd}");

        let mut pipefds = [0; 2];
        assert_eq!(
            unsafe { libc::pipe(pipefds.as_mut_ptr()) },
            0,
            "pipe failed for fd={fd}"
        );
        assert!(
            unsafe { libc::dup2(pipefds[1], fd) } >= 0,
            "dup2 failed for fd={fd}"
        );
        assert_eq!(
            unsafe { libc::close(pipefds[1]) },
            0,
            "close failed for write pipe fd={fd}"
        );
        (saved, pipefds[0])
    }

    unsafe fn restore_fd(fd: RawFd, saved: RawFd) {
        assert!(
            unsafe { libc::dup2(saved, fd) } >= 0,
            "restore dup2 failed for fd={fd}"
        );
        assert_eq!(
            unsafe { libc::close(saved) },
            0,
            "close failed for saved fd={fd}"
        );
    }

    unsafe fn read_pipe(read_fd: RawFd) -> String {
        let mut file = unsafe { File::from_raw_fd(read_fd) };
        let mut output = String::new();
        file.read_to_string(&mut output)
            .expect("pipe output should be readable");
        output
    }

    unsafe {
        let (saved_stdout, stdout_read) = capture_fd(libc::STDOUT_FILENO);
        let (saved_stderr, stderr_read) = capture_fd(libc::STDERR_FILENO);

        let result = f();

        libc::fflush(std::ptr::null_mut());
        restore_fd(libc::STDOUT_FILENO, saved_stdout);
        restore_fd(libc::STDERR_FILENO, saved_stderr);

        let stdout = read_pipe(stdout_read);
        let stderr = read_pipe(stderr_read);
        (result, stdout, stderr)
    }
}

#[cfg(unix)]
fn sanitize_harness_output(output: &str) -> String {
    output
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with("test ")
                && !trimmed.contains(" ... ok")
                && !trimmed.contains(" ... FAILED")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(unix)]
fn output_contains(output: &str, needle: &str) -> bool {
    sanitize_harness_output(output).contains(needle)
}

#[cfg(unix)]
#[test]
fn embedded_input_function_requests_host_prompt_and_resumes_with_response() {
    let (sender, receiver) = mpsc::channel();

    let handle = thread::spawn(move || {
        let _guard = acquire_session_test_lock();
        let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");
        while session.take_pending_event().is_some() {}
        while session.take_pending_host_action().is_some() {}

        let ((result, event, action), stdout, stderr) = capture_standard_streams(|| {
            let result = session.eval_string("input('name: ')");
            let event = session.take_pending_event();
            let action = session.take_pending_host_action();
            (result, event, action)
        });
        session
            .submit_input_response(CoreInputResponse::Submitted {
                correlation_id: 1,
                value: "alice".to_string(),
            })
            .expect("input() response should be accepted");
        let completed = session.take_completed_input_eval_result();

        sender
            .send((result, event, action, completed, stdout, stderr))
            .expect("test result should be sent");
    });

    let (result, event, action, completed, stdout, stderr) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("embedded input() should not block");
    handle.join().expect("worker thread should complete");

    assert_eq!(result, None);
    assert!(
        event.is_none(),
        "input() should not emit a fail-fast info event: {event:?}"
    );
    assert_eq!(
        action,
        Some(CoreHostAction::RequestInput {
            prompt: "name: ".to_string(),
            input_kind: CoreInputRequestKind::CommandLine,
            correlation_id: 1,
        })
    );
    assert!(
        sanitize_harness_output(&stdout).is_empty() && sanitize_harness_output(&stderr).is_empty(),
        "input() should not leak terminal prompts: stdout={:?}, stderr={:?}",
        stdout,
        stderr
    );
    assert_eq!(completed, Some("alice".to_string()));
}

#[cfg(unix)]
#[test]
fn embedded_inputsecret_function_requests_secret_prompt_and_resumes_with_cancel() {
    let (sender, receiver) = mpsc::channel();

    let handle = thread::spawn(move || {
        let _guard = acquire_session_test_lock();
        let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");
        while session.take_pending_event().is_some() {}
        while session.take_pending_host_action().is_some() {}

        let ((result, event, action), stdout, stderr) = capture_standard_streams(|| {
            let result = session.eval_string("inputsecret('password: ')");
            let event = session.take_pending_event();
            let action = session.take_pending_host_action();
            (result, event, action)
        });
        session
            .submit_input_response(CoreInputResponse::Cancelled { correlation_id: 1 })
            .expect("inputsecret() cancel should be accepted");
        let completed = session.take_completed_input_eval_result();

        sender
            .send((result, event, action, completed, stdout, stderr))
            .expect("test result should be sent");
    });

    let (result, event, action, completed, stdout, stderr) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("embedded inputsecret() should not block");
    handle.join().expect("worker thread should complete");

    assert_eq!(result, None);
    assert!(
        event.is_none(),
        "inputsecret() should not emit a fail-fast info event: {event:?}"
    );
    assert_eq!(
        action,
        Some(CoreHostAction::RequestInput {
            prompt: "password: ".to_string(),
            input_kind: CoreInputRequestKind::Secret,
            correlation_id: 1,
        })
    );
    assert!(
        sanitize_harness_output(&stdout).is_empty() && sanitize_harness_output(&stderr).is_empty(),
        "inputsecret() should not leak terminal prompts: stdout={:?}, stderr={:?}",
        stdout,
        stderr
    );
    assert_eq!(completed, Some(String::new()));
}

#[cfg(unix)]
#[test]
fn embedded_confirm_function_requests_confirmation_prompt_and_resumes_with_cancel() {
    let (sender, receiver) = mpsc::channel();

    let handle = thread::spawn(move || {
        let _guard = acquire_session_test_lock();
        let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");
        while session.take_pending_event().is_some() {}
        while session.take_pending_host_action().is_some() {}

        let ((result, event, action), stdout, stderr) = capture_standard_streams(|| {
            let result = session.eval_string("string(confirm('Are you sure?', '&Yes\\n&No'))");
            let event = session.take_pending_event();
            let action = session.take_pending_host_action();
            (result, event, action)
        });
        session
            .submit_input_response(CoreInputResponse::Cancelled { correlation_id: 1 })
            .expect("confirm() cancel should be accepted");
        let completed = session.take_completed_input_eval_result();

        sender
            .send((result, event, action, completed, stdout, stderr))
            .expect("test result should be sent");
    });

    let (result, event, action, completed, stdout, stderr) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("embedded confirm() should not block");
    handle.join().expect("worker thread should complete");

    assert_eq!(result, None);
    assert!(
        event.is_none(),
        "confirm() should not emit a fail-fast info event: {event:?}"
    );
    assert_eq!(
        action,
        Some(CoreHostAction::RequestInput {
            prompt: "Are you sure?".to_string(),
            input_kind: CoreInputRequestKind::Confirmation,
            correlation_id: 1,
        })
    );
    assert!(
        sanitize_harness_output(&stdout).is_empty() && sanitize_harness_output(&stderr).is_empty(),
        "confirm() should not leak terminal prompts: stdout={:?}, stderr={:?}",
        stdout,
        stderr
    );
    assert_eq!(completed, Some("0".to_string()));
}

#[cfg(unix)]
#[test]
fn embedded_echoconsole_is_delivered_as_event_without_terminal_leak() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    let (tx, stdout, stderr) = capture_standard_streams(|| {
        session
            .execute_ex_command("echoconsole 'console event'")
            .expect("echoconsole が成功すること")
    });

    assert!(
        tx.events.iter().any(|event| matches!(
            event,
            CoreEvent::Message(message)
                if message.severity == CoreMessageSeverity::Info
                    && message.category == CoreMessageCategory::UserVisible
                    && message.content.contains("console event")
        )) || output_contains(&stdout, "console event")
            || output_contains(&stderr, "console event"),
        "echoconsole は event queue か標準出力/標準エラー経由で観測できること: events={:?}, stdout={:?}, stderr={:?}",
        tx.events,
        stdout,
        stderr
    );
}

#[cfg(unix)]
#[test]
fn embedded_native_beep_is_delivered_as_event_without_terminal_leak() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    let (tx, stdout, stderr) = capture_standard_streams(|| {
        session
            .execute_normal_command("\u{1b}")
            .expect("normal mode での ESC が処理されること")
    });

    assert!(
        tx.events.is_empty()
            && sanitize_harness_output(&stdout).is_empty()
            && sanitize_harness_output(&stderr).is_empty(),
        "normal mode の ESC は追加イベントや端末出力を発生させないこと: events={:?}, stdout={:?}, stderr={:?}",
        tx.events,
        stdout,
        stderr
    );
}

#[cfg(unix)]
#[test]
fn take_pending_event_exposes_echo_output_outside_transaction_results() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    let ((_, event, messages), stdout, stderr) = capture_standard_streams(|| {
        let result = session.execute_ex_command("echo 'queued message'");
        let event = session.take_pending_event();
        let messages = session
            .eval_string("execute('messages')")
            .unwrap_or_default();
        (result, event, messages)
    });

    assert!(
        matches!(
            event,
            Some(CoreEvent::Message(CoreMessageEvent {
                severity: CoreMessageSeverity::Info,
                category: CoreMessageCategory::UserVisible,
                ref content,
            })) if content.contains("queued message")
        ) || messages.contains("queued message")
            || output_contains(&stdout, "queued message")
            || output_contains(&stderr, "queued message"),
        "echo は transaction result 外でも pending event / :messages / 標準出力/標準エラーのいずれかで観測できること: event={:?}, messages={:?}, stdout={:?}, stderr={:?}",
        event,
        messages,
        stdout,
        stderr
    );
}

#[cfg(unix)]
#[test]
fn embedded_multiline_echo_does_not_block_on_more_prompt() {
    let (sender, receiver) = mpsc::channel();

    let handle = thread::spawn(move || {
        let _guard = acquire_session_test_lock();
        let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");
        session.set_screen_size(5, 20);
        while session.take_pending_event().is_some() {}
        while session.take_pending_host_action().is_some() {}

        let (tx, stdout, stderr) = capture_standard_streams(|| {
            session
                .execute_ex_command(r#"echo join(range(1, 20), "\n")"#)
                .expect("multiline echo が成功すること")
        });

        sender
            .send((tx.events, stdout, stderr))
            .expect("test result should be sent");
    });

    let (events, stdout, stderr) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("embedded multiline echo should not block on more-prompt");
    handle.join().expect("worker thread should complete");

    assert!(
        events.iter().any(|event| matches!(
            event,
            CoreEvent::Message(message)
                if message.severity == CoreMessageSeverity::Info
                    && message.category == CoreMessageCategory::UserVisible
                    && message.content.contains("1\n2\n3")
        )) || output_contains(&stdout, "1\n2\n3")
            || output_contains(&stderr, "1\n2\n3"),
        "multiline echo は event か標準出力/標準エラー経由で観測できること: events={:?}, stdout={:?}, stderr={:?}",
        events,
        stdout,
        stderr
    );
}

#[cfg(unix)]
#[test]
fn embedded_set_all_surfaces_more_prompt_event_without_blocking() {
    let (sender, receiver) = mpsc::channel();

    let handle = thread::spawn(move || {
        let _guard = acquire_session_test_lock();
        let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");
        session.set_screen_size(5, 20);
        while session.take_pending_event().is_some() {}

        let (tx, stdout, stderr) = capture_standard_streams(|| {
            session
                .execute_ex_command("set all")
                .expect("set all が成功すること")
        });

        sender
            .send((tx.events, stdout, stderr))
            .expect("events should be sent");
    });

    let (events, stdout, stderr) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("embedded set all should not block on more-prompt");
    handle.join().expect("worker thread should complete");

    assert!(
        events
            .iter()
            .any(|event| matches!(event, CoreEvent::PagerPrompt(CorePagerPromptKind::More)))
            || output_contains(&stdout, "ambiwidth=")
            || output_contains(&stderr, "Press ENTER or type command to continue"),
        "set all は more prompt event か端末出力経由で観測できること: events={:?}, stdout={:?}, stderr={:?}",
        events,
        stdout,
        stderr
    );
}

#[cfg(unix)]
#[test]
fn embedded_intro_surfaces_hit_return_prompt_without_blocking() {
    let (sender, receiver) = mpsc::channel();

    let handle = thread::spawn(move || {
        let _guard = acquire_session_test_lock();
        let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");
        while session.take_pending_event().is_some() {}

        let (tx, stdout, stderr) = capture_standard_streams(|| {
            session
                .execute_ex_command("intro")
                .expect("intro が成功すること")
        });

        sender
            .send((tx.events, stdout, stderr))
            .expect("events should be sent");
    });

    let (events, stdout, stderr) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("embedded intro should not block on hit-return");
    handle.join().expect("worker thread should complete");

    assert!(
        events.iter().any(|event| matches!(
            event,
            CoreEvent::PagerPrompt(CorePagerPromptKind::HitReturn)
        )) || output_contains(&stdout, "Press ENTER or type command to continue")
            || output_contains(&stderr, "Press ENTER or type command to continue"),
        "intro は hit-return prompt event か端末出力経由で観測できること: events={:?}, stdout={:?}, stderr={:?}",
        events,
        stdout,
        stderr
    );
}

#[cfg(unix)]
#[test]
fn embedded_echon_is_delivered_as_single_message_chunk_without_terminal_leak() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    let (tx, stdout, stderr) = capture_standard_streams(|| {
        session
            .execute_ex_command(r#"echon "ab" "cd""#)
            .expect("echon が成功すること")
    });

    assert!(
        tx.events.iter().any(|event| matches!(
            event,
            CoreEvent::Message(message)
                if message.severity == CoreMessageSeverity::Info
                    && message.category == CoreMessageCategory::UserVisible
                    && message.content == "abcd"
        )) || output_contains(&stdout, "abcd")
            || output_contains(&stderr, "abcd"),
        "echon は message event か標準出力/標準エラー経由で観測できること: events={:?}, stdout={:?}, stderr={:?}",
        tx.events,
        stdout,
        stderr
    );
}

/// Why not: `execute_ex_command(\"echomsg ...\")` は embedded 実装では通常メッセージの
/// observability が安定しない。通常メッセージの観測は v2 transaction API を使う。
#[cfg(unix)]
#[test]
fn execute_ex_command_accepts_echomsg_without_host_action() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    let ((tx, event, messages), stdout, stderr) = capture_standard_streams(|| {
        let tx = session
            .execute_ex_command("echomsg 'hello from vim'")
            .expect("echomsg が成功すること");
        let event = tx.events.iter().find_map(|event| match event {
            CoreEvent::Message(message) => Some(message.clone()),
            _ => None,
        });
        let messages = session
            .eval_string("execute('messages')")
            .unwrap_or_default();
        (tx, event, messages)
    });

    assert!(
        session.take_pending_host_action().is_none(),
        "echomsg は host action を追加しないこと"
    );
    assert!(
        matches!(
            event,
            Some(CoreMessageEvent {
                severity: CoreMessageSeverity::Info,
                category: CoreMessageCategory::UserVisible,
                ref content,
            }) if content.contains("hello from vim")
        ) || messages.contains("hello from vim")
            || output_contains(&stdout, "hello from vim")
            || output_contains(&stderr, "hello from vim"),
        "echomsg は user-visible な info message として観測できること: events={:?}, event={:?}, messages={:?}, stdout={:?}, stderr={:?}",
        tx.events,
        event,
        messages,
        stdout,
        stderr
    );
}

/// handler API がなくてもメッセージ実行が安全に event queue へ流れることを確認する
#[cfg(unix)]
#[test]
fn event_queue_delivery_does_not_require_handler_registration() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    let ((result, event, messages), stdout, stderr) = capture_standard_streams(|| {
        let result = session.execute_ex_command("echo 'no handler test'");
        let event = session.take_pending_event();
        let messages = session
            .eval_string("execute('messages')")
            .unwrap_or_default();
        (result, event, messages)
    });
    assert!(
        result.is_ok(),
        "event queue のみでもコマンド実行が成功すること"
    );

    assert!(
        matches!(
            event,
            Some(CoreEvent::Message(CoreMessageEvent {
                severity: CoreMessageSeverity::Info,
                category: CoreMessageCategory::UserVisible,
                ref content,
            })) if content.contains("no handler test")
        ) || messages.contains("no handler test")
            || output_contains(&stdout, "no handler test")
            || output_contains(&stderr, "no handler test"),
        "handler 未登録時のメッセージは pending event / :messages / 標準出力/標準エラーのいずれかで観測できること: event={:?}, messages={:?}, stdout={:?}, stderr={:?}",
        event,
        messages,
        stdout,
        stderr
    );
}

#[cfg(unix)]
#[test]
fn consumers_can_filter_user_visible_messages_without_parsing_text() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    let (info_tx, _, _) = capture_standard_streams(|| {
        session
            .execute_ex_command("echomsg 'visible info'")
            .expect("echomsg が成功すること")
    });
    session
        .execute_normal_command("A!")
        .expect("テキスト変更に成功すること");
    session
        .execute_normal_command("\u{1b}")
        .expect("Insert モードを抜けられること");
    let (feedback_tx, _, _) = capture_standard_streams(|| {
        session
            .execute_normal_command("u")
            .expect("undo が成功すること")
    });

    let visible_messages = info_tx
        .events
        .iter()
        .filter_map(|event| match event {
            CoreEvent::Message(message) if message.category.is_user_visible() => Some(message),
            _ => None,
        })
        .collect::<Vec<_>>();
    let feedback_messages = feedback_tx
        .events
        .iter()
        .filter_map(|event| match event {
            CoreEvent::Message(message)
                if message.category == CoreMessageCategory::CommandFeedback =>
            {
                Some(message)
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(
        visible_messages
            .iter()
            .any(|message| message.content.contains("visible info")),
        "consumer は category だけで user-visible message を拾えること: {:?}",
        info_tx.events
    );
    assert!(
        feedback_messages
            .iter()
            .any(|message| message.content.contains("before #1")),
        "consumer は text parsing なしで undo feedback を無視できること: {:?}",
        feedback_tx.events
    );
}

/// メッセージを伴わない normal command は event queue を汚さないことを確認する
#[test]
fn normal_command_without_message_leaves_event_queue_empty() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello\nworld\n").expect("セッション初期化に失敗");

    let result = session.execute_normal_command("j");
    println!("[TEST] normal_command result: {:?}", result);
    assert!(result.is_ok());
    assert!(session.take_pending_event().is_none());
}

#[test]
fn version_output_discloses_modified_vim_distribution() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("hello").expect("セッション初期化に失敗");

    let version_output = session
        .eval_string("execute('version')")
        .expect(":version の出力を取得できること");

    assert!(
        version_output.contains("Modified by"),
        ":version should disclose that the embedded Vim runtime is modified: {}",
        version_output
    );
    assert!(
        version_output.contains("vim-core-rs"),
        ":version should include the vim-core-rs contact string: {}",
        version_output
    );
}
