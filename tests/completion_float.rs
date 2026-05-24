use saya::features::completion::float::{
    CompletionCandidate, CompletionFloatInputOutcome, CompletionFloatManager,
    CompletionMenuFloatRequest,
};
use saya::features::completion::session::CompletionKeyBindingsRequest;
use saya::input::router::KeyInput;
use saya::presentation::floating_window::{
    FloatingAnchor, FloatingContentRef, FloatingFit, FloatingInputOutcome, FloatingPlacement,
    FloatingRelativeTo, FloatingWindowManager, FloatingZIndex, WorkspaceFocus,
};
use saya::presentation::screen_model::PaneRect;

fn request() -> CompletionMenuFloatRequest {
    CompletionMenuFloatRequest {
        window_id: 7,
        cursor_row: 3,
        cursor_col: 5,
        candidates: vec![
            CompletionCandidate {
                label: "println!".to_string(),
                insert_text: None,
                detail: Some("macro".to_string()),
                kind: Some("Function".to_string()),
                documentation: vec!["Prints to stdout.".to_string()],
                source: None,
                replace_range: None,
            },
            CompletionCandidate {
                label: "print!".to_string(),
                insert_text: None,
                detail: Some("macro".to_string()),
                kind: Some("Function".to_string()),
                documentation: vec!["Prints without a newline.".to_string()],
                source: None,
                replace_range: None,
            },
            CompletionCandidate {
                label: "process".to_string(),
                insert_text: None,
                detail: Some("module".to_string()),
                kind: Some("Module".to_string()),
                documentation: vec!["Process control APIs.".to_string()],
                source: None,
                replace_range: None,
            },
        ],
        selected_index: 1,
        max_visible_items: 2,
        documentation_max_width: 40,
        documentation_max_height: 5,
        keys: Some(default_completion_keys()),
    }
}

fn default_completion_keys() -> CompletionKeyBindingsRequest {
    CompletionKeyBindingsRequest {
        confirm: Some(vec![
            "<Enter>".to_string(),
            "<Tab>".to_string(),
            "<C-y>".to_string(),
        ]),
        close: Some(vec!["<C-e>".to_string()]),
        next: Some(vec!["<Down>".to_string(), "<C-n>".to_string()]),
        previous: Some(vec!["<Up>".to_string(), "<C-p>".to_string()]),
        page_next: Some(vec!["<PageDown>".to_string()]),
        page_previous: Some(vec!["<PageUp>".to_string()]),
    }
}

#[test]
fn completion_menu_opens_structured_candidate_owner_and_documentation_float() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();

    let opened = completion
        .open_menu(&mut floats, request())
        .expect("completion candidates should open a menu");

    let menu = floats
        .debug_window(opened.menu_id)
        .expect("menu float should be tracked");
    assert_eq!(
        menu.content,
        FloatingContentRef::CompletionMenu {
            menu_id: opened.menu_id.0
        }
    );
    assert_eq!(menu.zindex, FloatingZIndex::Completion.value());
    assert_eq!(
        menu.placement,
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Cursor { window_id: 7 },
            anchor: FloatingAnchor::NorthWest,
            row: 1,
            col: 0,
            fit: FloatingFit::TruncateToGrid,
        }
    );

    let docs_id = opened
        .documentation_id
        .expect("selected candidate documentation should open");
    let docs = floats
        .debug_window(docs_id)
        .expect("documentation float should be tracked");
    assert_eq!(docs.zindex, FloatingZIndex::CompletionDocumentation.value());
    assert_eq!(docs.lines, vec!["Prints without a newline."]);

    let rendered = floats.resolve_screen_models_with_cursors(
        80,
        24,
        &[(
            7,
            PaneRect {
                x: 10,
                y: 2,
                width: 60,
                height: 20,
            },
        )],
        &[(7, 3, 5)],
        Some(7),
    );
    let menu_model = rendered
        .iter()
        .find(|float| float.id == opened.menu_id)
        .expect("menu should resolve");
    assert_eq!(
        menu_model.lines,
        vec![
            "  [Function] println! - macro",
            "> [Function] print! - macro"
        ]
    );
}

#[test]
fn completion_menu_without_request_keys_uses_default_operation_key_bindings() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    let mut menu_request = request();
    menu_request.keys = None;
    let opened = completion
        .open_menu(&mut floats, menu_request)
        .expect("completion candidates should open a menu");
    assert!(floats.focus_float(opened.menu_id));

    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Down, Some(7)),
        CompletionFloatInputOutcome::Selected {
            menu_id: opened.menu_id,
            selected_index: 2
        }
    );
    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Enter, Some(7)),
        CompletionFloatInputOutcome::Accepted {
            menu_id: opened.menu_id,
            candidate: request().candidates[2].clone()
        }
    );
    assert!(
        floats.debug_window(opened.menu_id).is_none(),
        "standard bindings should work when request keys are omitted"
    );
}

#[test]
fn completion_selection_keys_update_selected_row_scroll_and_documentation() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    let opened = completion
        .open_menu(&mut floats, request())
        .expect("completion candidates should open a menu");
    assert!(floats.focus_float(opened.menu_id));

    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Down, Some(7)),
        CompletionFloatInputOutcome::Selected {
            menu_id: opened.menu_id,
            selected_index: 2
        }
    );

    let menu = floats
        .debug_window(opened.menu_id)
        .expect("menu should remain open after selection");
    assert_eq!(
        menu.lines,
        vec!["  [Function] print! - macro", "> [Module] process - module"]
    );
    let docs_id = completion
        .active_documentation_id()
        .expect("documentation float should remain active");
    assert_eq!(
        floats
            .debug_window(docs_id)
            .expect("updated documentation should exist")
            .lines,
        vec!["Process control APIs."]
    );

    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Up, Some(7)),
        CompletionFloatInputOutcome::Selected {
            menu_id: opened.menu_id,
            selected_index: 1
        }
    );
    assert_eq!(
        floats
            .debug_window(opened.menu_id)
            .expect("menu should remain open")
            .lines,
        vec!["> [Function] print! - macro", "  [Module] process - module"]
    );
}

#[test]
fn completion_enter_accepts_candidate_and_escape_closes_menu_and_docs() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    let opened = completion
        .open_menu(&mut floats, request())
        .expect("completion candidates should open a menu");
    assert!(floats.focus_float(opened.menu_id));

    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Enter, Some(7)),
        CompletionFloatInputOutcome::Accepted {
            menu_id: opened.menu_id,
            candidate: request().candidates[1].clone()
        }
    );
    assert_eq!(floats.focus(), Some(WorkspaceFocus::Pane { window_id: 7 }));
    assert!(floats.debug_window(opened.menu_id).is_none());
    assert!(
        opened
            .documentation_id
            .and_then(|id| floats.debug_window(id))
            .is_none()
    );

    let opened = completion
        .open_menu(&mut floats, request())
        .expect("completion candidates should open another menu");
    assert!(floats.focus_float(opened.menu_id));
    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Escape, Some(7)),
        CompletionFloatInputOutcome::Closed {
            menu_id: opened.menu_id,
            editor_key: Some(KeyInput::Escape)
        }
    );
    assert_eq!(floats.focus(), Some(WorkspaceFocus::Pane { window_id: 7 }));
    assert!(floats.debug_window(opened.menu_id).is_none());
}

#[test]
fn completion_ctrl_e_closes_menu_without_editor_dispatch() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    let opened = completion
        .open_menu(&mut floats, request())
        .expect("completion candidates should open a menu");
    assert!(floats.focus_float(opened.menu_id));

    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Ctrl('e'), Some(7)),
        CompletionFloatInputOutcome::Closed {
            menu_id: opened.menu_id,
            editor_key: None
        }
    );
    assert_eq!(floats.focus(), Some(WorkspaceFocus::Pane { window_id: 7 }));
    assert!(floats.debug_window(opened.menu_id).is_none());
}

#[test]
fn completion_keys_drive_active_menu_without_float_focus() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    let opened = completion
        .open_menu(&mut floats, request())
        .expect("completion candidates should open a menu");

    assert_eq!(floats.focus(), None);
    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Down, Some(7)),
        CompletionFloatInputOutcome::Selected {
            menu_id: opened.menu_id,
            selected_index: 2
        }
    );
    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Enter, Some(7)),
        CompletionFloatInputOutcome::Accepted {
            menu_id: opened.menu_id,
            candidate: request().candidates[2].clone()
        }
    );
    assert_eq!(floats.focus(), Some(WorkspaceFocus::Pane { window_id: 7 }));
    assert!(floats.debug_window(opened.menu_id).is_none());
}

#[test]
fn generic_static_line_key_handler_ignores_completion_menu_content() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    let opened = completion
        .open_menu(&mut floats, request())
        .expect("completion candidates should open a menu");
    assert!(floats.focus_float(opened.menu_id));

    assert_eq!(
        floats.handle_focused_static_lines_key_with_restore(&KeyInput::Down, Some(7)),
        FloatingInputOutcome::Ignored,
        "completion-specific key semantics must stay outside the generic static-lines handler"
    );
}

#[test]
fn completion_menu_uses_request_scoped_key_bindings_for_confirm_and_close() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    let mut custom = request();
    custom.keys = Some(CompletionKeyBindingsRequest {
        confirm: Some(vec!["<Tab>".to_string()]),
        close: Some(vec!["<Esc>".to_string()]),
        next: None,
        previous: None,
        page_next: None,
        page_previous: None,
    });
    let opened = completion
        .open_menu(&mut floats, custom)
        .expect("completion candidates should open a menu");
    assert!(floats.focus_float(opened.menu_id));

    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Enter, Some(7)),
        CompletionFloatInputOutcome::Ignored,
        "Enter must not confirm when the popup overrides confirm keys to Tab only"
    );
    assert!(floats.debug_window(opened.menu_id).is_some());

    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Tab, Some(7)),
        CompletionFloatInputOutcome::Accepted {
            menu_id: opened.menu_id,
            candidate: request().candidates[1].clone()
        }
    );
    assert!(floats.debug_window(opened.menu_id).is_none());
}

#[test]
fn completion_menu_allows_empty_key_groups_to_disable_operations() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    let mut custom = request();
    custom.keys = Some(CompletionKeyBindingsRequest {
        confirm: Some(Vec::new()),
        close: Some(Vec::new()),
        next: None,
        previous: None,
        page_next: None,
        page_previous: None,
    });
    let opened = completion
        .open_menu(&mut floats, custom)
        .expect("completion candidates should open a menu");
    assert!(floats.focus_float(opened.menu_id));

    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Enter, Some(7)),
        CompletionFloatInputOutcome::Ignored
    );
    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Ctrl('e'), Some(7)),
        CompletionFloatInputOutcome::Ignored
    );
    assert!(floats.debug_window(opened.menu_id).is_some());
    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Escape, Some(7)),
        CompletionFloatInputOutcome::Closed {
            menu_id: opened.menu_id,
            editor_key: Some(KeyInput::Escape)
        },
        "Esc is an editor escape key, not a disableable completion close key"
    );
    assert!(floats.debug_window(opened.menu_id).is_none());
}

#[test]
fn completion_menu_uses_request_scoped_navigation_keys() {
    let mut floats = FloatingWindowManager::default();
    let mut completion = CompletionFloatManager::default();
    let mut custom = request();
    custom.keys = Some(CompletionKeyBindingsRequest {
        confirm: None,
        close: None,
        next: Some(vec!["j".to_string()]),
        previous: Some(vec!["k".to_string()]),
        page_next: Some(vec!["<C-f>".to_string()]),
        page_previous: Some(vec!["<C-b>".to_string()]),
    });
    let opened = completion
        .open_menu(&mut floats, custom)
        .expect("completion candidates should open a menu");
    assert!(floats.focus_float(opened.menu_id));

    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Down, Some(7)),
        CompletionFloatInputOutcome::Ignored,
        "Down must not navigate when the popup overrides next keys"
    );
    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Char('j'), Some(7)),
        CompletionFloatInputOutcome::Selected {
            menu_id: opened.menu_id,
            selected_index: 2
        }
    );
    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Char('k'), Some(7)),
        CompletionFloatInputOutcome::Selected {
            menu_id: opened.menu_id,
            selected_index: 1
        }
    );
    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Ctrl('f'), Some(7)),
        CompletionFloatInputOutcome::Selected {
            menu_id: opened.menu_id,
            selected_index: 2
        }
    );
    assert_eq!(
        completion.handle_key(&mut floats, &KeyInput::Ctrl('B'), Some(7)),
        CompletionFloatInputOutcome::Selected {
            menu_id: opened.menu_id,
            selected_index: 0
        }
    );
}
