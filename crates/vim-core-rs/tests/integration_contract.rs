use std::sync::{Mutex, OnceLock};
use vim_core_rs::{
    CoreCommandOutcome, CoreEvent, CoreHostAction, CoreMode, CorePendingArgumentKind,
    CorePendingInput, VimCoreSession,
};

fn pending(
    keys: &str,
    count: Option<usize>,
    awaited_argument: Option<CorePendingArgumentKind>,
) -> CorePendingInput {
    CorePendingInput {
        pending_keys: keys.to_string(),
        count,
        awaited_argument,
    }
}

fn session_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_session_test_lock() -> std::sync::MutexGuard<'static, ()> {
    session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn assert_host_action<F>(session: &mut VimCoreSession, command: &str, predicate: F)
where
    F: FnOnce(&CoreHostAction),
{
    let outcome = session
        .execute_ex_command(command)
        .unwrap_or_else(|_| panic!("Failed to apply command: {}", command));

    match outcome.outcome {
        CoreCommandOutcome::HostActionQueued => {
            let action = outcome
                .host_actions
                .into_iter()
                .next()
                .unwrap_or_else(|| panic!("Expected host action for command: {}", command));
            predicate(&action);
        }
        _ => panic!(
            "Expected HostActionQueued for command: {}, got {:?}",
            command, outcome.outcome
        ),
    }
}

fn assert_write_action(session: &mut VimCoreSession, command: &str, force: bool) {
    assert_host_action(session, command, |action| {
        if let CoreHostAction::Write {
            force: actual_force,
            ..
        } = action
        {
            assert_eq!(
                *actual_force, force,
                "unexpected force flag for command {command}"
            );
        } else {
            panic!("Expected Write action for {command}, got {:?}", action);
        }
    });
}

fn assert_quit_action(session: &mut VimCoreSession, command: &str, force: bool) {
    assert_host_action(session, command, |action| {
        if let CoreHostAction::Quit {
            force: actual_force,
            ..
        } = action
        {
            assert_eq!(
                *actual_force, force,
                "unexpected force flag for command {command}"
            );
        } else {
            panic!("Expected Quit action for {command}, got {:?}", action);
        }
    });
}

/// `:wq`/`:xit` 等の Write→Quit 連結アクションを検証する
fn assert_write_then_quit_actions(session: &mut VimCoreSession, command: &str, force: bool) {
    let outcome = session
        .execute_ex_command(command)
        .unwrap_or_else(|_| panic!("Failed to apply command: {}", command));

    match outcome.outcome {
        CoreCommandOutcome::HostActionQueued => {
            let actions: Vec<_> = outcome.host_actions.into_iter().collect();
            assert_eq!(
                actions.len(),
                2,
                "Expected 2 host actions (Write + Quit) for {command}, got {:?}",
                actions
            );
            assert!(
                matches!(&actions[0], CoreHostAction::Write { force: f, .. } if *f == force),
                "First action should be Write(force={force}) for {command}, got {:?}",
                actions[0]
            );
            assert!(
                matches!(&actions[1], CoreHostAction::Quit { force: f, .. } if *f == force),
                "Second action should be Quit(force={force}) for {command}, got {:?}",
                actions[1]
            );
        }
        _ => panic!(
            "Expected HostActionQueued for command: {}, got {:?}",
            command, outcome.outcome
        ),
    }
}

#[test]
fn side_effect_convergence_matrix() {
    let _guard = acquire_session_test_lock();

    let mut session = VimCoreSession::new("Original text").expect("Failed to initialize session");

    for command in ["write", "write!", "update", "up!"] {
        assert_write_action(&mut session, command, command.ends_with('!'));
    }

    for command in ["quit", "q", "quit!", "qa", "qa!"] {
        assert_quit_action(&mut session, command, command.ends_with('!'));
    }

    // :wq は常に Write + Quit を返す
    for command in ["wq", "wq!"] {
        assert_write_then_quit_actions(&mut session, command, command.ends_with('!'));
    }

    // :x/:xit はバッファが変更されていない場合は Quit のみ（dirty時のみ Write + Quit）
    for command in ["x", "xit"] {
        assert_quit_action(&mut session, command, command.ends_with('!'));
    }

    let redraw_tx = session
        .execute_ex_command("redraw")
        .expect("redraw should succeed");
    assert!(matches!(
        redraw_tx.events.as_slice(),
        [CoreEvent::Redraw {
            clear_before_draw: false,
            ..
        }]
    ));
    assert!(redraw_tx.host_actions.is_empty());

    let redraw_bang_tx = session
        .execute_ex_command("redraw!")
        .expect("redraw! should succeed");
    assert!(matches!(
        redraw_bang_tx.events.as_slice(),
        [CoreEvent::Redraw {
            clear_before_draw: true,
            ..
        }]
    ));
    assert!(redraw_bang_tx.host_actions.is_empty());

    let bell_tx = session
        .execute_ex_command("bell")
        .expect("bell should succeed");
    assert!(matches!(bell_tx.events.as_slice(), [CoreEvent::Bell]));
    assert!(bell_tx.host_actions.is_empty());

    assert_host_action(&mut session, "input Hello", |action| {
        if let CoreHostAction::RequestInput { prompt, .. } = action {
            assert_eq!(prompt, "Hello");
        } else {
            panic!("Expected RequestInput action, got {:?}", action);
        }
    });
}

#[test]
fn normal_mode_side_effects() {
    let _guard = acquire_session_test_lock();

    let mut session = VimCoreSession::new("Line 1\nLine 2").expect("Failed to initialize session");

    // ZZ is a normal command that saves and exits
    let outcome = session
        .execute_normal_command("ZZ")
        .expect("Failed to apply ZZ");

    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));
    let action = outcome.host_actions.first().expect("Expected action");
    assert!(matches!(action, CoreHostAction::Quit { .. }));

    let outcome = session
        .execute_normal_command("ZQ")
        .expect("Failed to apply ZQ");
    assert!(matches!(
        outcome.outcome,
        CoreCommandOutcome::HostActionQueued
    ));
    let action = outcome.host_actions.first().expect("Expected action");
    assert!(matches!(action, CoreHostAction::Quit { force: true, .. }));
}

#[test]
fn snapshot_and_session_state_apis_stay_consistent_within_one_session() {
    let _guard = acquire_session_test_lock();

    let mut session = VimCoreSession::new("alpha\nbravo\ncharlie\ndelta\n")
        .expect("Failed to initialize session");
    let current_buf_id = session
        .buffers()
        .into_iter()
        .find(|buffer| buffer.is_active)
        .expect("active buffer should exist")
        .id;

    session
        .execute_ex_command(":clearjumps")
        .expect("clearjumps should succeed");
    session
        .set_mark('a', current_buf_id, 3, 0)
        .expect("mark should be set from rust");

    let initial_snapshot = session.snapshot();
    assert_eq!(initial_snapshot.mode, CoreMode::Normal);
    assert_eq!(initial_snapshot.pending_input, CorePendingInput::none());
    assert_eq!(session.mode(), CoreMode::Normal);
    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert_eq!(session.mark('a').expect("mark should be readable").row, 3);
    assert!(session.jumplist().entries.is_empty());

    session
        .execute_normal_command("'a")
        .expect("mark jump should succeed within one injection");
    let jumped_snapshot = session.snapshot();
    assert_eq!(jumped_snapshot.mode, CoreMode::Normal);
    assert_eq!(jumped_snapshot.pending_input, CorePendingInput::none());
    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert_eq!(jumped_snapshot.cursor_row, 3);

    let jumplist_after_jump = session.jumplist();
    assert!(
        !jumplist_after_jump.entries.is_empty(),
        "mark jump should leave a navigable history entry: {jumplist_after_jump:?}"
    );
    assert_eq!(
        jumplist_after_jump.current_index,
        jumplist_after_jump.entries.len()
    );

    session
        .execute_normal_command("v")
        .expect("v should enter visual mode");
    let visual_snapshot = session.snapshot();
    assert_eq!(visual_snapshot.mode, CoreMode::Visual);
    assert_eq!(session.mode(), CoreMode::Visual);
    assert_eq!(visual_snapshot.pending_input, CorePendingInput::none());
    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert_eq!(
        session.mark('a').expect("mark should stay available").row,
        3
    );

    session
        .execute_normal_command("\x1bR")
        .expect("escape then R should enter replace mode");
    assert_eq!(session.mode(), CoreMode::Replace);
    assert_eq!(session.snapshot().mode, CoreMode::Replace);
    assert_eq!(session.pending_input(), CorePendingInput::none());

    session
        .execute_normal_command("\x1bf")
        .expect("escape then f should enter char-pending state");
    assert_eq!(session.mode(), CoreMode::Normal);
    assert_eq!(
        session.pending_input(),
        pending("f", None, Some(CorePendingArgumentKind::Char))
    );
    assert_eq!(
        session.snapshot().pending_input,
        pending("f", None, Some(CorePendingArgumentKind::Char))
    );
    assert_eq!(
        session
            .mark('a')
            .expect("mark should survive pending state")
            .row,
        3
    );
    assert_eq!(session.jumplist(), jumplist_after_jump);
}

#[test]
fn autocmd_bufunload_event_order_matches_vim_slice() {
    let _guard = acquire_session_test_lock();

    let mut session = VimCoreSession::new("seed").expect("Failed to initialize session");

    session
        .execute_ex_command("let g:li = []")
        .expect("should initialize autocmd log");

    session
        .execute_ex_command("augroup test_bufunload_contract")
        .expect("augroup should open");
    session
        .execute_ex_command("autocmd!")
        .expect("augroup should clear prior autocmds");
    session
        .execute_ex_command("autocmd BufUnload * call add(g:li, 'bufunload')")
        .expect("BufUnload autocmd should define");
    session
        .execute_ex_command("autocmd BufDelete * call add(g:li, 'bufdelete')")
        .expect("BufDelete autocmd should define");
    session
        .execute_ex_command("autocmd BufWipeout * call add(g:li, 'bufwipeout')")
        .expect("BufWipeout autocmd should define");
    session
        .execute_ex_command("augroup END")
        .expect("augroup should close");

    session
        .execute_ex_command("new")
        .expect("new should create a buffer");
    session
        .execute_ex_command("setlocal bufhidden=")
        .expect("bufhidden should be configurable");
    session
        .execute_ex_command("bunload")
        .expect("bunload should succeed");

    assert_eq!(
        session.eval_string("string(g:li)"),
        Some("['bufunload', 'bufdelete']".to_string()),
        "bunload should trigger BufUnload then BufDelete"
    );

    session
        .execute_ex_command("new")
        .expect("new should create a second buffer");
    session
        .execute_ex_command("setlocal bufhidden=unload")
        .expect("bufhidden should accept unload");
    session
        .execute_ex_command("bwipeout")
        .expect("bwipeout should succeed");

    assert_eq!(
        session.eval_string("string(g:li)"),
        Some("['bufunload', 'bufdelete', 'bufunload', 'bufdelete', 'bufwipeout']".to_string()),
        "bwipeout should append BufUnload, BufDelete, and BufWipeout in order"
    );

    session
        .execute_ex_command("augroup test_bufunload_contract | autocmd! | augroup END")
        .expect("cleanup should succeed");
}

#[test]
fn autocmd_bufunload_can_tabnext_from_buffer_local_autocmd() {
    let _guard = acquire_session_test_lock();

    let mut session = VimCoreSession::new("seed").expect("Failed to initialize session");

    session
        .execute_ex_command("tabedit")
        .expect("tabedit should create a second tab");
    session
        .execute_ex_command("tabfirst")
        .expect("tabfirst should switch to the first tab");
    session
        .execute_ex_command("augroup test_autocmd_bufunload_tabnext")
        .expect("augroup should open");
    session
        .execute_ex_command("autocmd!")
        .expect("augroup should clear prior autocmds");
    session
        .execute_ex_command("autocmd BufUnload <buffer> tabnext")
        .expect("BufUnload autocmd should define");
    session
        .execute_ex_command("augroup END")
        .expect("augroup should close");

    session
        .execute_ex_command("quit")
        .expect("quit should succeed after BufUnload-triggered tabnext");

    assert_eq!(
        session.eval_string("tabpagenr('$')"),
        Some("2".to_string()),
        "quit should keep both tabs alive after tabnext from BufUnload"
    );

    session
        .execute_ex_command("tablast")
        .expect("tablast should switch to the last tab");
    session
        .execute_ex_command("quit")
        .expect("final quit should succeed");

    session
        .execute_ex_command("augroup test_autocmd_bufunload_tabnext | autocmd! | augroup END")
        .expect("cleanup should succeed");
}

#[test]
fn autocmd_bufunload_close_other_records_window_event_order() {
    let _guard = acquire_session_test_lock();

    let mut session = VimCoreSession::new("seed").expect("Failed to initialize session");

    session
        .execute_ex_command("tabnew Xb1")
        .expect("tabnew should create a new tab");
    session
        .execute_ex_command("let g:tab = tabpagenr()")
        .expect("tabpagenr should be readable");
    session
        .execute_ex_command("let g:w1 = win_getid()")
        .expect("first window id should be readable");
    session
        .execute_ex_command("new Xb2")
        .expect("new should create a second window");
    session
        .execute_ex_command("let g:w2 = win_getid()")
        .expect("second window id should be readable");
    session
        .execute_ex_command("let g:log = []")
        .expect("log should initialize");

    session
        .execute_ex_command("augroup test_autocmd_bufunload_close_other")
        .expect("augroup should open");
    session
        .execute_ex_command("autocmd!")
        .expect("augroup should clear prior autocmds");
    session
        .execute_ex_command("autocmd BufUnload * ++nested ++once bwipe! Xb1")
        .expect("BufUnload autocmd should define");
    for event in ["WinClosed", "BufLeave", "WinLeave", "TabLeave"] {
        session
            .execute_ex_command(&format!(
                "autocmd {event} * call add(g:log, '{event}:' .. expand('<afile>'))"
            ))
            .expect("window event autocmd should define");
    }
    session
        .execute_ex_command("augroup END")
        .expect("augroup should close");

    session
        .execute_ex_command("close")
        .expect("close should trigger BufUnload-driven cleanup");

    let w1 = session
        .eval_string("string(g:w1)")
        .expect("first window id should be readable");
    let w2 = session
        .eval_string("string(g:w2)")
        .expect("second window id should be readable");
    assert_eq!(
        session.eval_string("string(g:log)"),
        Some(format!(
            "['BufLeave:Xb2', 'WinLeave:Xb2', 'WinClosed:{w2}', 'WinClosed:{w1}', 'TabLeave:Xb2']"
        )),
        "close should record the expected sequence of window/tab events"
    );

    session
        .execute_ex_command("augroup test_autocmd_bufunload_close_other | autocmd! | augroup END")
        .expect("cleanup should succeed");
}

#[test]
fn autocmd_bufunload_can_switch_curbuf_without_leaking_a_new_buffer() {
    let _guard = acquire_session_test_lock();

    let mut session = VimCoreSession::new("asdf").expect("Failed to initialize session");

    session
        .execute_ex_command("let g:asdf_win = win_getid()")
        .expect("first window id should be readable");
    session
        .execute_ex_command("new")
        .expect("new should create a second buffer");
    session
        .execute_ex_command("let g:other_buf = bufnr()")
        .expect("other buffer id should be readable");
    session
        .execute_ex_command("let g:other_win = win_getid()")
        .expect("other window id should be readable");
    session
        .execute_ex_command("let g:triggered = 0")
        .expect("trigger flag should initialize");

    session
        .execute_ex_command("augroup test_autocmd_bufunload_switch_curbuf")
        .expect("augroup should open");
    session
        .execute_ex_command("autocmd!")
        .expect("augroup should clear prior autocmds");
    session
        .execute_ex_command(
            "autocmd BufUnload * ++once let g:triggered = 1 | call assert_fails('split', 'E1159:') | call win_gotoid(g:asdf_win)",
        )
        .expect("BufUnload autocmd should define");
    session
        .execute_ex_command("augroup END")
        .expect("augroup should close");

    session
        .execute_ex_command("enew")
        .expect("enew should trigger BufUnload");

    assert_eq!(
        session.eval_string("string(g:triggered)"),
        Some("1".to_string()),
        "BufUnload autocmd should run"
    );
    assert_eq!(
        session.eval_string("bufnr()"),
        session.eval_string("string(g:other_buf)"),
        "curbuf should be reused instead of leaking a new buffer"
    );
    assert_eq!(
        session.eval_string("win_getid()"),
        session.eval_string("string(g:other_win)"),
        "window focus should remain on the original other window"
    );
    assert_eq!(
        session.eval_string("len(win_findbuf(g:other_buf))"),
        Some("1".to_string()),
        "other buffer should remain displayed exactly once"
    );
    assert_eq!(
        session.eval_string("bufloaded(g:other_buf)"),
        Some("1".to_string()),
        "other buffer should stay loaded"
    );

    session
        .execute_ex_command("augroup test_autocmd_bufunload_switch_curbuf | autocmd! | augroup END")
        .expect("cleanup should succeed");
}
