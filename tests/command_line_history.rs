use saya::command_line_history::{
    CommandLineHistories, CommandLineHistory, CommandLineHistoryDirection,
};

#[test]
fn up_and_ctrl_p_recall_recent_commands_like_vim_command_line_history() {
    let mut history = CommandLineHistory::default();
    history.record(":write");
    history.record(":quit");

    assert_eq!(history.previous(""), Some(":quit"));
    assert_eq!(history.previous(":quit"), Some(":write"));
}

#[test]
fn down_and_ctrl_n_return_toward_newer_entries_and_restore_draft() {
    let mut history = CommandLineHistory::default();
    history.record(":edit one");
    history.record(":edit two");

    assert_eq!(history.previous(":edit "), Some(":edit two"));
    assert_eq!(history.previous(":edit two"), Some(":edit one"));
    assert_eq!(history.next(":edit one"), Some(":edit two"));
    assert_eq!(history.next(":edit two"), Some(":edit "));
}

#[test]
fn previous_and_next_keep_using_the_original_command_line_prefix() {
    let mut history = CommandLineHistory::default();
    history.record(":write");
    history.record(":quit");
    history.record(":wall");

    assert_eq!(history.previous(":w"), Some(":wall"));
    assert_eq!(history.previous(":wall"), Some(":write"));
    assert_eq!(history.previous(":write"), None);
    assert_eq!(history.next(":write"), Some(":wall"));
    assert_eq!(history.next(":wall"), Some(":w"));
}

#[test]
fn recording_command_resets_navigation_and_skips_empty_or_adjacent_duplicate_entries() {
    let mut history = CommandLineHistory::default();
    history.record(":write");
    history.record(":write");
    history.record(":");

    assert_eq!(history.previous(""), Some(":write"));
    assert_eq!(history.previous(":write"), None);

    history.record(":quit");

    assert_eq!(history.previous(""), Some(":quit"));
    assert_eq!(history.next(":quit"), Some(""));
}

#[test]
fn command_and_search_histories_are_kept_separate_and_return_promptless_buffers() {
    let mut histories = CommandLineHistories::default();
    histories.record(':', "write");
    histories.record('/', "needle");

    assert_eq!(
        histories.navigate(':', "", CommandLineHistoryDirection::Previous),
        Some("write".to_string())
    );
    assert_eq!(
        histories.navigate('/', "", CommandLineHistoryDirection::Previous),
        Some("needle".to_string())
    );
}
