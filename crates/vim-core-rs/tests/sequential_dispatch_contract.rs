use std::sync::{Mutex, MutexGuard, OnceLock};
use vim_core_rs::{
    CoreCommandOutcome, CoreEvent, CoreHostAction, CoreMode, CoreOptionScope,
    CorePendingArgumentKind, CorePendingInput, CoreSnapshot, VimCoreSession,
};

fn session_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn acquire_session_test_lock() -> MutexGuard<'static, ()> {
    session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

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

fn assert_pending_state(session: &VimCoreSession, expected: CorePendingInput) {
    assert_eq!(session.pending_input(), expected);
    assert_eq!(session.snapshot().pending_input, expected);
}

fn dispatch_sequence(
    session: &mut VimCoreSession,
    sequence: &str,
) -> Result<(), vim_core_rs::CoreCommandError> {
    for ch in sequence.chars() {
        session.dispatch_key(&ch.to_string())?;
    }
    Ok(())
}

fn run_direct_and_sequential_with_setup<F, E, T>(
    initial_text: &str,
    sequence: &str,
    setup: F,
    extract: E,
) -> (CoreSnapshot, T, VimCoreSession)
where
    F: Fn(&mut VimCoreSession),
    E: Fn(&VimCoreSession) -> T,
{
    let mut direct = VimCoreSession::new(initial_text).expect("direct session should initialize");
    setup(&mut direct);
    direct
        .execute_normal_command(sequence)
        .expect("direct command should succeed");
    let direct_snapshot = direct.snapshot();
    let direct_extra = extract(&direct);
    drop(direct);

    let mut sequential =
        VimCoreSession::new(initial_text).expect("sequential session should initialize");
    setup(&mut sequential);
    dispatch_sequence(&mut sequential, sequence).expect("sequential dispatch should succeed");

    (direct_snapshot, direct_extra, sequential)
}

fn assert_sessions_match(direct_snapshot: &CoreSnapshot, sequential: &VimCoreSession) {
    let sequential_snapshot = sequential.snapshot();

    assert_eq!(sequential_snapshot.text, direct_snapshot.text);
    assert_eq!(sequential_snapshot.cursor_row, direct_snapshot.cursor_row);
    assert_eq!(sequential_snapshot.cursor_col, direct_snapshot.cursor_col);
    assert_eq!(sequential_snapshot.mode, direct_snapshot.mode);
    assert_eq!(
        sequential_snapshot.pending_input,
        direct_snapshot.pending_input
    );
    assert_eq!(sequential_snapshot.revision, direct_snapshot.revision);
    assert_eq!(sequential.pending_input(), CorePendingInput::none());
}

fn assert_contains_message(events: &[CoreEvent], expected: &str) {
    assert!(
        events.iter().any(|event| matches!(
            event,
            CoreEvent::Message(message) if message.content.contains(expected)
        )),
        "expected message {expected:?} in events: {events:?}"
    );
}

fn eval_required(session: &mut VimCoreSession, expr: &str) -> String {
    session
        .eval_string(expr)
        .unwrap_or_else(|| panic!("expected eval result for {expr}"))
}

#[test]
fn sequential_j_preserves_desired_column_after_short_line_clamp() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("abcdef\nx\nabcdef\n").expect("session should initialize");

    for key in ["l", "l", "l", "j"] {
        session.dispatch_key(key).expect("key should dispatch");
    }

    let clamped = session.snapshot();
    assert_eq!(clamped.cursor_row, 1);
    assert_eq!(clamped.cursor_col, 0);
    assert_eq!(
        eval_required(&mut session, "string(winsaveview().curswant)"),
        "3"
    );

    session.dispatch_key("j").expect("second j should dispatch");

    let restored = session.snapshot();
    assert_eq!(restored.cursor_row, 2);
    assert_eq!(
        restored.cursor_col, 3,
        "vertical movement should preserve Vim's desired column across a short-line clamp"
    );
}

#[test]
fn sequential_k_preserves_desired_column_after_short_line_clamp() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("abcdef\nx\nabcdef\n").expect("session should initialize");

    for key in ["j", "j", "l", "l", "l", "k"] {
        session.dispatch_key(key).expect("key should dispatch");
    }

    let clamped = session.snapshot();
    assert_eq!(clamped.cursor_row, 1);
    assert_eq!(clamped.cursor_col, 0);
    assert_eq!(
        eval_required(&mut session, "string(winsaveview().curswant)"),
        "3"
    );

    session.dispatch_key("k").expect("second k should dispatch");

    let restored = session.snapshot();
    assert_eq!(restored.cursor_row, 0);
    assert_eq!(
        restored.cursor_col, 3,
        "upward movement should preserve Vim's desired column across a short-line clamp"
    );
}

#[test]
fn sequential_dispatch_handles_gg_and_reports_pending_prefix() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("one\ntwo\nthree\n").expect("session should initialize");

    session.dispatch_key("g").expect("first g should succeed");
    assert_pending_state(&session, pending("g", None, None));

    session.dispatch_key("g").expect("second g should succeed");
    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert_eq!(session.snapshot().cursor_row, 0);
}

#[test]
fn sequential_dispatch_repeated_u_stays_at_oldest_without_toggling_to_redo() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("base").expect("session should initialize");

    let cpoptions = session
        .get_option_string("cpoptions", CoreOptionScope::Default)
        .expect("cpoptions should be readable");
    assert!(
        !cpoptions.contains('u'),
        "embedded editing sessions should not enable Vi-compatible undo toggling: {cpoptions:?}"
    );

    for key in ["A", "1", "\u{1b}", "A", "2", "\u{1b}"] {
        session.dispatch_key(key).expect("edit key should succeed");
    }
    assert_eq!(session.snapshot().text, "base12\n");

    session
        .dispatch_key("u")
        .expect("first undo should succeed");
    assert_eq!(session.snapshot().text, "base1\n");

    session
        .dispatch_key("u")
        .expect("second undo should succeed");
    assert_eq!(session.snapshot().text, "base\n");

    let oldest_tx = session
        .dispatch_key("u")
        .expect("oldest undo should succeed");
    assert_eq!(session.snapshot().text, "base\n");
    assert_contains_message(&oldest_tx.events, "Already at oldest change");

    let repeated_oldest_tx = session
        .dispatch_key("u")
        .expect("repeated oldest undo should succeed");
    assert_eq!(session.snapshot().text, "base\n");
    assert_contains_message(&repeated_oldest_tx.events, "Already at oldest change");
}

#[test]
fn sequential_dispatch_handles_dd_and_reports_operator_pending() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("first line\nsecond line\nthird line\n")
        .expect("session should initialize");

    session.dispatch_key("d").expect("d should succeed");
    assert_pending_state(
        &session,
        pending("d", None, Some(CorePendingArgumentKind::MotionOrTextObject)),
    );

    session.dispatch_key("d").expect("second d should succeed");
    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert_eq!(
        session.snapshot().text.trim_end_matches('\n'),
        "second line\nthird line"
    );
}

#[test]
fn sequential_dispatch_handles_ciw_and_reports_each_pending_transition() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("alpha beta gamma\n").expect("session should initialize");

    session.dispatch_key("c").expect("c should succeed");
    assert_pending_state(
        &session,
        pending("c", None, Some(CorePendingArgumentKind::MotionOrTextObject)),
    );

    session.dispatch_key("i").expect("i should succeed");
    assert_pending_state(
        &session,
        pending(
            "ci",
            None,
            Some(CorePendingArgumentKind::MotionOrTextObject),
        ),
    );

    session.dispatch_key("w").expect("w should succeed");
    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert_eq!(session.snapshot().mode, CoreMode::Insert);
}

#[test]
fn sequential_dispatch_tracks_register_prefix_until_command_executes() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("paste target\n").expect("session should initialize");
    session.set_register('a', "from-a");

    session.dispatch_key("\"").expect("quote should succeed");
    assert_pending_state(
        &session,
        pending("\"", None, Some(CorePendingArgumentKind::Register)),
    );

    session
        .dispatch_key("a")
        .expect("register name should succeed");
    assert_pending_state(
        &session,
        pending("\"a", None, Some(CorePendingArgumentKind::NormalCommand)),
    );

    session.dispatch_key("p").expect("paste should succeed");
    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert!(session.snapshot().text.contains("from-a"));
}

#[test]
fn sequential_dispatch_tracks_mark_jump_prefix_until_mark_name_arrives() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("one\ntwo\nthree\n").expect("session should initialize");
    let current_buf_id = session.buffers()[0].id;
    session
        .set_mark('a', current_buf_id, 1, 0)
        .expect("mark setup should succeed");
    session
        .execute_normal_command("gg")
        .expect("cursor reset should succeed");

    session
        .dispatch_key("'")
        .expect("mark jump prefix should succeed");
    assert_pending_state(
        &session,
        pending("'", None, Some(CorePendingArgumentKind::MarkJump)),
    );

    session
        .dispatch_key("a")
        .expect("mark jump target should succeed");
    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert_eq!(session.snapshot().cursor_row, 1);
}

#[test]
fn sequential_dispatch_respects_insert_mode_literal_prefix_keys() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").expect("session should initialize");

    session
        .dispatch_key("i")
        .expect("i should enter insert mode");
    assert_eq!(session.snapshot().mode, CoreMode::Insert);
    assert_eq!(session.pending_input(), CorePendingInput::none());

    session
        .dispatch_key("g")
        .expect("g should insert literally");
    assert_eq!(session.snapshot().mode, CoreMode::Insert);
    assert_eq!(session.snapshot().text, "g\n");
    assert_eq!(session.pending_input(), CorePendingInput::none());

    session
        .dispatch_key("\u{1b}")
        .expect("escape should succeed");
    assert_eq!(session.snapshot().mode, CoreMode::Normal);
    assert_eq!(session.snapshot().text, "g\n");
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_respects_insert_mode_literal_digits() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").expect("session should initialize");

    session
        .dispatch_key("i")
        .expect("i should enter insert mode");
    session
        .dispatch_key("2")
        .expect("2 should insert literally");

    assert_eq!(session.snapshot().mode, CoreMode::Insert);
    assert_eq!(session.snapshot().text, "2\n");
    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert_eq!(session.snapshot().pending_input, CorePendingInput::none());
}

#[test]
fn sequential_dispatch_supports_counted_line_motion() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("one\ntwo\nthree\nfour\n").expect("session should initialize");

    session.dispatch_key("2").expect("count should succeed");
    assert_pending_state(&session, pending("2", Some(2), None));

    session.dispatch_key("j").expect("motion should succeed");
    assert_eq!(session.snapshot().cursor_row, 2);
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_supports_counted_goto_line() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("one\ntwo\nthree\nfour\n").expect("session should initialize");

    session.dispatch_key("3").expect("count should succeed");
    assert_pending_state(&session, pending("3", Some(3), None));

    session.dispatch_key("G").expect("G should succeed");
    assert_eq!(session.snapshot().cursor_row, 2);
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_supports_counted_gg_prefix_sequences() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("one\ntwo\nthree\nfour\n").expect("session should initialize");
    session
        .execute_normal_command("G")
        .expect("move to last line");
    assert_eq!(session.snapshot().cursor_row, 3);

    session.dispatch_key("2").expect("count should succeed");
    assert_pending_state(&session, pending("2", Some(2), None));

    session
        .dispatch_key("g")
        .expect("prefix should stay pending");
    assert_pending_state(&session, pending("2g", Some(2), None));

    session.dispatch_key("g").expect("sequence should execute");
    assert_eq!(session.snapshot().cursor_row, 1);
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_supports_counted_operator_sequences() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("one two three four\n").expect("session should initialize");

    session.dispatch_key("2").expect("count should succeed");
    assert_pending_state(&session, pending("2", Some(2), None));

    session
        .dispatch_key("d")
        .expect("operator should stay pending");
    assert_pending_state(
        &session,
        pending(
            "2d",
            Some(2),
            Some(CorePendingArgumentKind::MotionOrTextObject),
        ),
    );

    session.dispatch_key("w").expect("motion should execute");
    assert_eq!(session.snapshot().text, "three four\n");
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_supports_operator_followed_by_counted_motion() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("one two three four\n").expect("session should initialize");

    session
        .dispatch_key("d")
        .expect("operator should stay pending");
    assert_pending_state(
        &session,
        pending("d", None, Some(CorePendingArgumentKind::MotionOrTextObject)),
    );

    session
        .dispatch_key("2")
        .expect("motion count should stay pending");
    assert_pending_state(
        &session,
        pending(
            "d2",
            Some(2),
            Some(CorePendingArgumentKind::MotionOrTextObject),
        ),
    );

    session.dispatch_key("w").expect("motion should execute");
    assert_eq!(session.snapshot().text, "three four\n");
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_supports_register_prefixed_counted_operator_sequences() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("one two three four\n").expect("session should initialize");
    session.set_register('a', "baseline");

    session
        .dispatch_key("\"")
        .expect("quote should stay pending");
    assert_pending_state(
        &session,
        pending("\"", None, Some(CorePendingArgumentKind::Register)),
    );

    session
        .dispatch_key("a")
        .expect("register name should stay pending");
    assert_pending_state(
        &session,
        pending("\"a", None, Some(CorePendingArgumentKind::NormalCommand)),
    );

    session
        .dispatch_key("2")
        .expect("counted normal command should stay pending");
    assert_pending_state(
        &session,
        pending(
            "\"a2",
            Some(2),
            Some(CorePendingArgumentKind::NormalCommand),
        ),
    );

    session
        .dispatch_key("d")
        .expect("delete should stay pending");
    assert_pending_state(
        &session,
        pending(
            "\"a2d",
            Some(2),
            Some(CorePendingArgumentKind::MotionOrTextObject),
        ),
    );

    session.dispatch_key("w").expect("motion should execute");
    assert_eq!(session.snapshot().text, "three four\n");
    assert_eq!(session.pending_input(), CorePendingInput::none());
    assert_ne!(session.register('a').as_deref(), Some("baseline"));
}

#[test]
fn sequential_dispatch_supports_counted_find_sequences() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("xaxax\n").expect("session should initialize");

    session
        .dispatch_key("2")
        .expect("count should stay pending");
    assert_pending_state(&session, pending("2", Some(2), None));

    session.dispatch_key("f").expect("find should await target");
    assert_pending_state(
        &session,
        pending("2f", Some(2), Some(CorePendingArgumentKind::Char)),
    );

    session
        .dispatch_key("a")
        .expect("find target should execute");
    assert_eq!(session.snapshot().cursor_col, 3);
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_reports_all_single_key_argument_pending_families() {
    let _guard = acquire_session_test_lock();
    let cases = [
        ("f", pending("f", None, Some(CorePendingArgumentKind::Char))),
        ("F", pending("F", None, Some(CorePendingArgumentKind::Char))),
        ("t", pending("t", None, Some(CorePendingArgumentKind::Char))),
        ("T", pending("T", None, Some(CorePendingArgumentKind::Char))),
        (
            "r",
            pending("r", None, Some(CorePendingArgumentKind::ReplaceChar)),
        ),
        (
            "m",
            pending("m", None, Some(CorePendingArgumentKind::MarkSet)),
        ),
        (
            "'",
            pending("'", None, Some(CorePendingArgumentKind::MarkJump)),
        ),
        (
            "`",
            pending("`", None, Some(CorePendingArgumentKind::MarkJump)),
        ),
    ];

    for (key, expected) in cases {
        let mut session =
            VimCoreSession::new("alpha beta gamma\n").expect("session should initialize");
        session.dispatch_key(key).expect("prefix should succeed");
        assert_pending_state(&session, expected);
    }
}

#[test]
fn sequential_dispatch_reports_all_operator_pending_families() {
    let _guard = acquire_session_test_lock();
    let cases = ["d", "y", "c", ">", "<", "="];

    for key in cases {
        let mut session = VimCoreSession::new("one two\n").expect("session should initialize");
        session.dispatch_key(key).expect("operator should succeed");
        assert_pending_state(
            &session,
            pending(key, None, Some(CorePendingArgumentKind::MotionOrTextObject)),
        );
    }
}

#[test]
fn sequential_dispatch_reports_all_g_prefixed_pending_families() {
    let _guard = acquire_session_test_lock();
    let cases = [
        ("g", pending("g", None, None)),
        (
            "gq",
            pending(
                "gq",
                None,
                Some(CorePendingArgumentKind::MotionOrTextObject),
            ),
        ),
        (
            "gu",
            pending(
                "gu",
                None,
                Some(CorePendingArgumentKind::MotionOrTextObject),
            ),
        ),
        (
            "gU",
            pending(
                "gU",
                None,
                Some(CorePendingArgumentKind::MotionOrTextObject),
            ),
        ),
        (
            "g~",
            pending(
                "g~",
                None,
                Some(CorePendingArgumentKind::MotionOrTextObject),
            ),
        ),
    ];

    for (sequence, expected) in cases {
        let mut session = VimCoreSession::new("one two\n").expect("session should initialize");
        for ch in sequence.chars() {
            session
                .dispatch_key(&ch.to_string())
                .expect("g family prefix should succeed");
        }
        assert_pending_state(&session, expected);
    }
}

#[test]
fn sequential_dispatch_supports_counted_replace_target_with_digit_argument() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("abc\n").expect("session should initialize");

    session
        .dispatch_key("2")
        .expect("count should stay pending");
    assert_pending_state(&session, pending("2", Some(2), None));

    session
        .dispatch_key("r")
        .expect("replace should await target");
    assert_pending_state(
        &session,
        pending("2r", Some(2), Some(CorePendingArgumentKind::ReplaceChar)),
    );

    session
        .dispatch_key("9")
        .expect("replace target should execute");
    assert_eq!(session.snapshot().text, "99c\n");
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_supports_counted_mark_jump_sequences() {
    let _guard = acquire_session_test_lock();
    let mut session =
        VimCoreSession::new("one\ntwo\nthree\nfour\n").expect("session should initialize");
    let current_buf_id = session.buffers()[0].id;
    session
        .set_mark('a', current_buf_id, 2, 0)
        .expect("mark setup should succeed");

    session
        .dispatch_key("2")
        .expect("count should stay pending");
    assert_pending_state(&session, pending("2", Some(2), None));

    session
        .dispatch_key("'")
        .expect("mark jump should await target");
    assert_pending_state(
        &session,
        pending("2'", Some(2), Some(CorePendingArgumentKind::MarkJump)),
    );

    session
        .dispatch_key("a")
        .expect("mark jump target should execute");
    assert_eq!(session.snapshot().cursor_row, 2);
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_matches_upstream_charsearch_count_case() {
    let _guard = acquire_session_test_lock();
    let initial_text = "xaxax\n";
    let sequence = "2fa";

    let mut direct = VimCoreSession::new(initial_text).expect("direct session should initialize");
    direct
        .execute_normal_command(sequence)
        .expect("direct command should succeed");
    let direct_snapshot = direct.snapshot();
    drop(direct);

    let mut sequential =
        VimCoreSession::new(initial_text).expect("sequential session should initialize");
    dispatch_sequence(&mut sequential, sequence).expect("sequential dispatch should succeed");

    let sequential_snapshot = sequential.snapshot();
    assert_eq!(sequential_snapshot.text, direct_snapshot.text);
    assert_eq!(sequential_snapshot.cursor_row, direct_snapshot.cursor_row);
    assert_eq!(sequential_snapshot.cursor_col, direct_snapshot.cursor_col);
    assert_eq!(sequential.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_matches_upstream_textobject_delete_inside_quote_case() {
    let _guard = acquire_session_test_lock();
    let initial_text = "out \" in \"noXno\"\n";
    let sequence = "0fXdi\"";

    let mut direct = VimCoreSession::new(initial_text).expect("direct session should initialize");
    direct
        .execute_normal_command(sequence)
        .expect("direct command should succeed");
    let direct_snapshot = direct.snapshot();
    drop(direct);

    let mut sequential =
        VimCoreSession::new(initial_text).expect("sequential session should initialize");
    dispatch_sequence(&mut sequential, sequence).expect("sequential dispatch should succeed");

    let sequential_snapshot = sequential.snapshot();
    assert_eq!(sequential_snapshot.text, direct_snapshot.text);
    assert_eq!(sequential_snapshot.cursor_row, direct_snapshot.cursor_row);
    assert_eq!(sequential_snapshot.cursor_col, direct_snapshot.cursor_col);
    assert_eq!(sequential.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_matches_upstream_textobject_delete_around_backtick_case() {
    let _guard = acquire_session_test_lock();
    let initial_text = "bla bla `quote` blah\n";
    let sequence = "02f`da`";

    let mut direct = VimCoreSession::new(initial_text).expect("direct session should initialize");
    direct
        .execute_normal_command(sequence)
        .expect("direct command should succeed");
    let direct_snapshot = direct.snapshot();
    drop(direct);

    let mut sequential =
        VimCoreSession::new(initial_text).expect("sequential session should initialize");
    dispatch_sequence(&mut sequential, sequence).expect("sequential dispatch should succeed");

    let sequential_snapshot = sequential.snapshot();
    assert_eq!(sequential_snapshot.text, direct_snapshot.text);
    assert_eq!(sequential_snapshot.cursor_row, direct_snapshot.cursor_row);
    assert_eq!(sequential_snapshot.cursor_col, direct_snapshot.cursor_col);
    assert_eq!(sequential.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_matches_upstream_textobject_change_inner_word_case() {
    let _guard = acquire_session_test_lock();
    let initial_text = "alpha beta gamma\n";
    let sequence = "wciw";

    let mut direct = VimCoreSession::new(initial_text).expect("direct session should initialize");
    direct
        .execute_normal_command(sequence)
        .expect("direct command should succeed");
    let direct_snapshot = direct.snapshot();
    drop(direct);

    let mut sequential =
        VimCoreSession::new(initial_text).expect("sequential session should initialize");
    dispatch_sequence(&mut sequential, sequence).expect("sequential dispatch should succeed");

    let sequential_snapshot = sequential.snapshot();
    assert_eq!(sequential_snapshot.text, direct_snapshot.text);
    assert_eq!(sequential_snapshot.cursor_row, direct_snapshot.cursor_row);
    assert_eq!(sequential_snapshot.cursor_col, direct_snapshot.cursor_col);
    assert_eq!(sequential_snapshot.mode, direct_snapshot.mode);
    assert_eq!(sequential.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_matches_upstream_goto_line_variants() {
    let _guard = acquire_session_test_lock();
    let initial_text = "one\ntwo\nthree\nfour\n";

    // Derived from vendor/upstream/vim/src/testdir/test_goto.vim and
    // vendor/upstream/vim/src/testdir/test_normal.vim goto-line cases.
    for sequence in ["gg", "2gg"] {
        let (direct_snapshot, (), sequential) = run_direct_and_sequential_with_setup(
            initial_text,
            sequence,
            |session| {
                session
                    .execute_normal_command("G")
                    .expect("setup move to end should succeed");
            },
            |_| (),
        );
        assert_sessions_match(&direct_snapshot, &sequential);
    }

    let (direct_snapshot, (), sequential) =
        run_direct_and_sequential_with_setup(initial_text, "3G", |_| {}, |_| ());
    assert_sessions_match(&direct_snapshot, &sequential);
}

#[test]
fn sequential_dispatch_matches_upstream_operator_and_count_variants() {
    let _guard = acquire_session_test_lock();

    // Derived from vendor/upstream/vim/src/testdir/test_normal.vim count/operator
    // coverage and vendor/upstream/vim/src/testdir/test_registers.vim delete cases.
    let operator_cases = [
        ("first line\nsecond line\nthird line\n", "dd"),
        ("one two three four\n", "dw"),
        ("one two three four\n", "d2w"),
        ("one two three four\n", "2dw"),
    ];

    for (initial_text, sequence) in operator_cases {
        let (direct_snapshot, (), sequential) =
            run_direct_and_sequential_with_setup(initial_text, sequence, |_| {}, |_| ());
        assert_sessions_match(&direct_snapshot, &sequential);
    }
}

#[test]
fn sequential_dispatch_matches_upstream_word_textobject_variants() {
    let _guard = acquire_session_test_lock();
    let initial_text = "alpha beta gamma\n";

    // Derived from vendor/upstream/vim/src/testdir/test_textobjects.vim.
    for sequence in ["wciw", "diw", "yiw"] {
        let (direct_snapshot, direct_register, sequential) = run_direct_and_sequential_with_setup(
            initial_text,
            sequence,
            |_| {},
            |session| session.register('"'),
        );
        assert_sessions_match(&direct_snapshot, &sequential);
        assert_eq!(sequential.register('"'), direct_register);
    }
}

#[test]
fn sequential_dispatch_matches_upstream_register_prefixed_variants() {
    let _guard = acquire_session_test_lock();

    // Derived from vendor/upstream/vim/src/testdir/test_registers.vim.
    {
        let (direct_snapshot, direct_register, sequential) = run_direct_and_sequential_with_setup(
            "paste target\n",
            "\"ap",
            |s| {
                s.set_register('a', "from-a");
            },
            |session| session.register('a'),
        );
        assert_sessions_match(&direct_snapshot, &sequential);
        assert_eq!(sequential.register('a'), direct_register);
    }

    {
        let (direct_snapshot, direct_register, sequential) = run_direct_and_sequential_with_setup(
            "one two three four\n",
            "\"a2dw",
            |s| {
                s.set_register('a', "baseline");
            },
            |session| session.register('a'),
        );
        assert_sessions_match(&direct_snapshot, &sequential);
        assert_eq!(sequential.register('a'), direct_register);
    }
}

#[test]
fn sequential_dispatch_matches_upstream_mark_jump_variants() {
    let _guard = acquire_session_test_lock();
    let initial_text = "one\ntwo\nthree\nfour\n";

    // Derived from vendor/upstream/vim/src/testdir/test_marks.vim.
    for sequence in ["'a", "`a"] {
        let (direct_snapshot, (), sequential) = run_direct_and_sequential_with_setup(
            initial_text,
            sequence,
            |session| {
                let current_buf_id = session.buffers()[0].id;
                session
                    .set_mark('a', current_buf_id, 2, 2)
                    .expect("mark setup should succeed");
                session
                    .execute_normal_command("gg0")
                    .expect("setup reset should succeed");
            },
            |_| (),
        );
        assert_sessions_match(&direct_snapshot, &sequential);
    }
}

#[test]
fn sequential_dispatch_matches_upstream_charsearch_variants() {
    let _guard = acquire_session_test_lock();

    // Derived from vendor/upstream/vim/src/testdir/test_charsearch.vim.
    let charsearch_cases = [("xaxax\n", "fa"), ("xaxax\n", "2fa"), ("xxaxax\n", "ta")];

    for (initial_text, sequence) in charsearch_cases {
        let (direct_snapshot, (), sequential) =
            run_direct_and_sequential_with_setup(initial_text, sequence, |_| {}, |_| ());
        assert_sessions_match(&direct_snapshot, &sequential);
    }
}

#[test]
fn sequential_dispatch_matches_upstream_g_prefix_variants() {
    let _guard = acquire_session_test_lock();

    // Derived from vendor/upstream/vim/src/testdir/test_normal.vim.
    for (initial_text, sequence) in [
        ("This is a simple test\n", "guu"),
        ("This is a simple test\n", "gUgU"),
        ("Alpha beta gamma\n", "g~w"),
    ] {
        let (direct_snapshot, (), sequential) =
            run_direct_and_sequential_with_setup(initial_text, sequence, |_| {}, |_| ());
        assert_sessions_match(&direct_snapshot, &sequential);
    }

    let (direct_snapshot, (), sequential) = run_direct_and_sequential_with_setup(
        "alpha beta gamma delta\nepsilon zeta eta theta\n",
        "gqj",
        |session| {
            session
                .execute_ex_command(":set textwidth=12")
                .expect("textwidth setup should succeed");
        },
        |_| (),
    );
    assert_sessions_match(&direct_snapshot, &sequential);
}

#[test]
fn sequential_dispatch_keeps_normal_prefix_keys_literal_in_insert_mode() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").expect("session should initialize");

    session
        .dispatch_key("i")
        .expect("i should enter insert mode");

    for key in ["g", "2"] {
        session
            .dispatch_key(key)
            .expect("key should insert literally");
    }

    assert_eq!(session.snapshot().mode, CoreMode::Insert);
    assert_eq!(session.snapshot().text, "g2\n");
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_keeps_char_pending_prefixes_literal_in_insert_mode() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").expect("session should initialize");

    session
        .dispatch_key("i")
        .expect("i should enter insert mode");
    for key in ["A", "G", "E", "N", "T", "S"] {
        session
            .dispatch_key(key)
            .expect("insert-mode key should insert literally");
        assert_eq!(session.pending_input(), CorePendingInput::none());
    }
    session
        .dispatch_key("\u{1b}")
        .expect("escape should leave insert mode");

    assert_eq!(session.snapshot().mode, CoreMode::Normal);
    assert_eq!(session.snapshot().text, "AGENTS\n");
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn sequential_dispatch_accepts_ctrl_h_as_insert_mode_backspace() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("").expect("session should initialize");

    session
        .dispatch_key("i")
        .expect("i should enter insert mode");
    for key in ["A", "B", "\x08", "C"] {
        session
            .dispatch_key(key)
            .expect("insert-mode key should dispatch");
    }
    session
        .dispatch_key("\u{1b}")
        .expect("escape should leave insert mode");

    assert_eq!(session.snapshot().mode, CoreMode::Normal);
    assert_eq!(session.snapshot().text, "AC\n");
    assert_eq!(session.pending_input(), CorePendingInput::none());
}

#[test]
fn ctrl_z_queues_suspend_host_action_without_native_terminal_ownership() {
    let _guard = acquire_session_test_lock();
    let mut session = VimCoreSession::new("alpha\n").expect("session should initialize");

    let outcome = session
        .dispatch_key("\u{1a}")
        .expect("Ctrl-Z should be host-mediated");

    assert_eq!(outcome.outcome, CoreCommandOutcome::HostActionQueued);
    assert_eq!(outcome.host_actions, vec![CoreHostAction::Suspend]);
    assert_eq!(session.pending_input(), CorePendingInput::none());
}
