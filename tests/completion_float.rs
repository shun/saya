use saya::features::completion::float::{
    CompletionCandidate, CompletionFloatInputOutcome, CompletionFloatManager,
    CompletionMenuFloatRequest,
};
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
            menu_id: opened.menu_id
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
