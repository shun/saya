use saya::input::command_line_history::{
    CommandLineHistories, CommandLineHistory, CommandLineHistoryDirection,
    load_histories_from_path, record_history_and_save_to_path, save_histories_to_path,
};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

fn temp_history_path(test_name: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "saya-command-history-{test_name}-{}-{unique}.json",
        std::process::id()
    ))
}

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

#[test]
fn command_and_search_histories_round_trip_through_persistent_cache_file() {
    let path = temp_history_path("round-trip");
    let mut histories = CommandLineHistories::default();
    histories.record(':', "write");
    histories.record(':', "quit");
    histories.record('/', "needle");

    save_histories_to_path(&histories, &path).expect("history cache save should succeed");
    let mut restored = load_histories_from_path(&path).expect("history cache load should succeed");

    assert_eq!(
        restored.navigate(':', "", CommandLineHistoryDirection::Previous),
        Some("quit".to_string())
    );
    assert_eq!(
        restored.navigate(':', "quit", CommandLineHistoryDirection::Previous),
        Some("write".to_string())
    );
    assert_eq!(
        restored.navigate('/', "", CommandLineHistoryDirection::Previous),
        Some("needle".to_string())
    );

    let _ = fs::remove_file(path);
}

#[test]
fn recording_command_history_immediately_persists_cache_file() {
    let path = temp_history_path("record-save");
    let mut histories = CommandLineHistories::default();

    record_history_and_save_to_path(&mut histories, ':', "write", &path)
        .expect("recorded command history should be saved immediately");

    assert!(
        path.exists(),
        "history cache file should exist immediately after command record"
    );
    let mut restored = load_histories_from_path(&path).expect("saved command history loads");
    assert_eq!(
        restored.navigate(':', "", CommandLineHistoryDirection::Previous),
        Some("write".to_string())
    );

    let _ = fs::remove_file(path);
}

#[test]
fn loading_missing_history_cache_starts_with_empty_histories() {
    let path = temp_history_path("missing");

    let mut restored = load_histories_from_path(&path).expect("missing cache should be allowed");

    assert_eq!(
        restored.navigate(':', "", CommandLineHistoryDirection::Previous),
        None
    );
}
