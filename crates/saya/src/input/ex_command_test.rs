use crate::app::test_support::launch_serial_lock as session_test_lock;
use crate::presentation::screen_model::{ProjectionInput, project};

use super::*;

#[test]
fn apply_local_ex_command_enables_line_numbers_for_set_number() {
    let mut session_state = EditorSessionState::new(None);
    let message = apply_local_ex_command(&mut session_state, ":set number");

    assert_eq!(message, Some("line numbers: on".to_string()));
    assert!(session_state.line_numbers());
}

#[test]
fn apply_local_ex_command_enables_line_numbers_for_set_nu() {
    let mut session_state = EditorSessionState::new(None);
    let message = apply_local_ex_command(&mut session_state, "set nu");

    assert_eq!(message, Some("line numbers: on".to_string()));
    assert!(session_state.line_numbers());
}

#[test]
fn apply_local_ex_command_disables_line_numbers_for_set_nonumber() {
    let mut session_state = EditorSessionState::new_with_tab_size_and_line_numbers(None, 8, true);
    let message = apply_local_ex_command(&mut session_state, ":set nonumber");

    assert_eq!(message, Some("line numbers: off".to_string()));
    assert!(!session_state.line_numbers());
}

#[test]
fn apply_local_ex_command_disables_line_numbers_for_set_nonu() {
    let mut session_state = EditorSessionState::new_with_tab_size_and_line_numbers(None, 8, true);
    let message = apply_local_ex_command(&mut session_state, "set nonu");

    assert_eq!(message, Some("line numbers: off".to_string()));
    assert!(!session_state.line_numbers());
}

#[test]
fn apply_local_ex_command_updates_screen_projection() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let snapshot = vim_core_rs::VimCoreSession::new("alpha\nbeta\n")
        .expect("session")
        .snapshot();
    let mut session_state = EditorSessionState::new(None);

    let before = project(&ProjectionInput::new(&snapshot, &session_state, None));
    assert_eq!(before.lines, vec!["alpha".to_string(), "beta".to_string()]);

    apply_local_ex_command(&mut session_state, ":set number").expect("command handled");

    let after = project(&ProjectionInput::new(&snapshot, &session_state, None));
    assert_eq!(
        after.lines,
        vec!["   1 alpha".to_string(), "   2 beta".to_string()]
    );
}

#[test]
fn apply_local_ex_command_updates_number_width_for_set_numberwidth() {
    let mut session_state = EditorSessionState::new_with_tab_size_and_line_numbers(None, 8, true);

    let message = apply_local_ex_command(&mut session_state, ":set numberwidth=6");

    assert_eq!(message, Some("numberwidth=6".to_string()));
    assert_eq!(session_state.number_width(), 6);
}

#[test]
fn apply_local_ex_command_updates_number_width_for_set_nuw() {
    let mut session_state = EditorSessionState::new(None);

    let message = apply_local_ex_command(&mut session_state, "set nuw=0");

    assert_eq!(message, Some("numberwidth=1".to_string()));
    assert_eq!(session_state.number_width(), 1);
}

#[test]
fn apply_local_ex_command_toggles_markdown_render_projection() {
    let mut session_state = EditorSessionState::new(None);

    let off_message = apply_local_ex_command(&mut session_state, ":set nomarkdownrender");
    assert_eq!(off_message, Some("markdownrender: off".to_string()));
    assert!(!session_state.markdown_render());

    let toggle_message = apply_local_ex_command(&mut session_state, ":set markdownrender!");
    assert_eq!(toggle_message, Some("markdownrender: on".to_string()));
    assert!(session_state.markdown_render());
}

#[test]
fn apply_local_ex_command_cancels_pending_directory_operation_preview() {
    let root_path = std::env::temp_dir().join(format!(
        "saya-ex-command-dired-cancel-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos()
    ));
    let alpha_path = root_path.join("alpha.md");
    std::fs::create_dir_all(&root_path).expect("test directory");
    std::fs::write(&alpha_path, "alpha\n").expect("alpha file");
    let mut session_state = EditorSessionState::new(Some(root_path.clone()));
    let preview = session_state
        .prepare_directory_buffer_operation_preview("")
        .expect("delete preview should be prepared");

    let message = apply_local_ex_command(&mut session_state, ":dired-cancel");

    assert_eq!(
        message,
        Some(format!(
            "Directory operation preview cancelled: 1 operation(s), preview_id={}",
            preview.id
        ))
    );
    assert!(
        session_state
            .pending_directory_operation_preview()
            .is_none()
    );
    assert!(alpha_path.exists(), "cancel must not delete files");

    std::fs::remove_dir_all(root_path).expect("cleanup directory");
}

#[test]
fn route_ex_command_routes_dired_cancel_to_host_local_handler() {
    assert_eq!(
        route_ex_command(":dired-cancel"),
        ExCommandRoute::PresentationLocal
    );
}

#[test]
fn route_ex_command_routes_empty_command_to_noop() {
    assert_eq!(route_ex_command(":"), ExCommandRoute::NoOp);
    assert_eq!(route_ex_command("   :   "), ExCommandRoute::NoOp);
}

#[test]
fn apply_local_ex_command_reopens_dismissed_message_pager_for_messages() {
    let mut session_state = EditorSessionState::new(None);
    assert!(session_state.sync_message_pager("one\ntwo\nthree\nfour", 2));
    assert!(session_state.handle_message_pager_key(&crate::input::router::KeyInput::Char('G')));
    assert!(session_state.handle_message_pager_key(&crate::input::router::KeyInput::Enter));
    assert!(!session_state.message_pager_active());

    let message = apply_local_ex_command(&mut session_state, ":messages");

    assert_eq!(message, Some("one\ntwo\nthree\nfour".to_string()));
    assert!(session_state.message_pager_active());
    assert_eq!(session_state.message_scroll_offset(), 0);
}

#[test]
fn route_ex_command_routes_messages_to_host_local_handler() {
    assert_eq!(
        route_ex_command(":messages"),
        ExCommandRoute::PresentationLocal
    );
    assert_eq!(route_ex_command(":mes"), ExCommandRoute::PresentationLocal);
}

#[test]
fn route_ex_command_routes_markdown_render_to_presentation_state() {
    assert_eq!(
        route_ex_command(":set markdownrender"),
        ExCommandRoute::PresentationLocal
    );
    assert_eq!(
        route_ex_command(":set nomarkdownrender"),
        ExCommandRoute::PresentationLocal
    );
    assert_eq!(
        route_ex_command(":set markdownrender!"),
        ExCommandRoute::PresentationLocal
    );
}

#[test]
fn apply_local_ex_command_returns_none_for_unknown_command() {
    let mut session_state = EditorSessionState::new(None);

    let result = apply_local_ex_command(&mut session_state, ":w");

    assert_eq!(result, None);
    assert!(!session_state.line_numbers());
}

#[test]
fn parse_search_option_command_routes_search_option_commands_to_core_owned_updates() {
    assert_eq!(
        parse_search_option_command(":set hlsearch"),
        Some(SearchOptionCommand::EnableHlSearch)
    );
    assert_eq!(
        parse_search_option_command("set nohls"),
        Some(SearchOptionCommand::DisableHlSearch)
    );
    assert_eq!(
        parse_search_option_command(":set hls!"),
        Some(SearchOptionCommand::ToggleHlSearch)
    );
    assert_eq!(
        parse_search_option_command("set incsearch"),
        Some(SearchOptionCommand::EnableIncSearch)
    );
    assert_eq!(
        parse_search_option_command(":nohlsearch"),
        Some(SearchOptionCommand::ClearHlSearch)
    );
}

#[test]
fn route_ex_command_routes_search_option_commands_to_core_owned_updates() {
    assert_eq!(
        route_ex_command(":set hlsearch"),
        ExCommandRoute::SearchOption(SearchOptionCommand::EnableHlSearch)
    );
    assert_eq!(
        route_ex_command("set nohls"),
        ExCommandRoute::SearchOption(SearchOptionCommand::DisableHlSearch)
    );
    assert_eq!(
        route_ex_command(":set hls!"),
        ExCommandRoute::SearchOption(SearchOptionCommand::ToggleHlSearch)
    );
    assert_eq!(
        route_ex_command("set incsearch"),
        ExCommandRoute::SearchOption(SearchOptionCommand::EnableIncSearch)
    );
    assert_eq!(
        route_ex_command(":nohlsearch"),
        ExCommandRoute::SearchOption(SearchOptionCommand::ClearHlSearch)
    );
}

#[test]
fn route_ex_command_routes_save_family_commands_to_core_owned_handlers() {
    assert_eq!(route_ex_command(":w"), ExCommandRoute::CoreOwned);
    assert_eq!(route_ex_command(":wq"), ExCommandRoute::CoreOwned);
    assert_eq!(route_ex_command(":x"), ExCommandRoute::CoreOwned);
    assert_eq!(route_ex_command(":xit"), ExCommandRoute::CoreOwned);
    assert_eq!(
        route_ex_command(":exit"),
        ExCommandRoute::CoreOwned,
        "exit alias should also stay core-owned"
    );
}

#[test]
fn apply_local_ex_command_ignores_search_option_commands() {
    let mut session_state = EditorSessionState::new(None);

    assert_eq!(
        apply_local_ex_command(&mut session_state, ":set hlsearch"),
        None
    );
    assert_eq!(
        apply_local_ex_command(&mut session_state, ":nohlsearch"),
        None
    );
    assert!(!session_state.line_numbers());
}
