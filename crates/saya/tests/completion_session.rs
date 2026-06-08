use saya::features::completion::session::{
    CompletionKeyBindingsRequest, CompletionPosition, CompletionRange, CompletionSessionManager,
    CompletionShowRequest, HostCompletionCandidate,
};

fn range(start: usize, end: usize) -> CompletionRange {
    CompletionRange {
        start: CompletionPosition {
            line: 0,
            character: start,
        },
        end: CompletionPosition {
            line: 0,
            character: end,
        },
    }
}

fn candidate(label: &str, insert_text: &str) -> HostCompletionCandidate {
    HostCompletionCandidate {
        label: label.to_string(),
        insert_text: Some(insert_text.to_string()),
        kind: Some("Function".to_string()),
        detail: None,
        documentation: Vec::new(),
        source: Some("test".to_string()),
    }
}

#[test]
fn completion_session_rejects_stale_results_by_request_generation() {
    let mut manager = CompletionSessionManager::default();
    let first = CompletionShowRequest {
        session_id: "session-1".to_string(),
        request_id: 1,
        replace_range: range(0, 3),
        candidates: vec![candidate("old", "old")],
        selected_index: 0,
        max_visible_items: 8,
        documentation_max_width: 72,
        documentation_max_height: 12,
        keys: None,
    };
    let second = CompletionShowRequest {
        request_id: 2,
        candidates: vec![candidate("new", "new")],
        ..first.clone()
    };

    assert!(manager.accept_show_request(second).is_ok());
    let stale = manager
        .accept_show_request(first)
        .expect_err("older request for same session must be rejected");

    assert_eq!(stale.session_id, "session-1");
    assert_eq!(stale.request_id, 1);
    assert_eq!(stale.current_request_id, 2);
}

#[test]
fn completion_confirm_applies_insert_text_to_replace_range() {
    let mut manager = CompletionSessionManager::default();
    let accepted = manager
        .accept_show_request(CompletionShowRequest {
            session_id: "session-1".to_string(),
            request_id: 1,
            replace_range: range(4, 7),
            candidates: vec![candidate("println!", "println!($0);")],
            selected_index: 0,
            max_visible_items: 8,
            documentation_max_width: 72,
            documentation_max_height: 12,
            keys: None,
        })
        .expect("request should be accepted");

    let edit = accepted
        .confirm(0, "let pri = 1;\n")
        .expect("selected candidate should produce an edit");

    assert_eq!(edit.replacement_text, "println!($0);");
    assert_eq!(edit.updated_text, "let println!($0); = 1;\n");
}

#[test]
fn completion_session_carries_request_scoped_keys_to_float_request() {
    let mut manager = CompletionSessionManager::default();
    let accepted = manager
        .accept_show_request(CompletionShowRequest {
            session_id: "session-1".to_string(),
            request_id: 1,
            replace_range: range(0, 3),
            candidates: vec![candidate("println!", "println!($0);")],
            selected_index: 0,
            max_visible_items: 8,
            documentation_max_width: 72,
            documentation_max_height: 12,
            keys: Some(CompletionKeyBindingsRequest {
                confirm: Some(vec!["<Tab>".to_string()]),
                close: Some(vec!["<Esc>".to_string()]),
                next: Some(vec!["j".to_string()]),
                previous: Some(vec!["k".to_string()]),
                page_next: Some(vec!["<C-f>".to_string()]),
                page_previous: Some(vec!["<C-b>".to_string()]),
            }),
        })
        .expect("request should be accepted");

    let float_request = accepted.to_float_request(7, 1, 2);

    assert_eq!(
        float_request.keys,
        Some(CompletionKeyBindingsRequest {
            confirm: Some(vec!["<Tab>".to_string()]),
            close: Some(vec!["<Esc>".to_string()]),
            next: Some(vec!["j".to_string()]),
            previous: Some(vec!["k".to_string()]),
            page_next: Some(vec!["<C-f>".to_string()]),
            page_previous: Some(vec!["<C-b>".to_string()]),
        })
    );
}
