use super::*;

#[test]
fn maps_selector_navigation_keys_to_controller_commands() {
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Char('j')),
        Some(RuntimeSelectorControllerCommand::CursorNext)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Down),
        Some(RuntimeSelectorControllerCommand::CursorNext)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Ctrl('n')),
        Some(RuntimeSelectorControllerCommand::CursorNext)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Ctrl('N')),
        Some(RuntimeSelectorControllerCommand::CursorNext)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Char('k')),
        Some(RuntimeSelectorControllerCommand::CursorPrevious)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Up),
        Some(RuntimeSelectorControllerCommand::CursorPrevious)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Ctrl('p')),
        Some(RuntimeSelectorControllerCommand::CursorPrevious)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Ctrl('P')),
        Some(RuntimeSelectorControllerCommand::CursorPrevious)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::PageDown),
        Some(RuntimeSelectorControllerCommand::PageDown)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Ctrl('d')),
        Some(RuntimeSelectorControllerCommand::PageDown)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Ctrl('f')),
        Some(RuntimeSelectorControllerCommand::PageDown)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::PageUp),
        Some(RuntimeSelectorControllerCommand::PageUp)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Ctrl('u')),
        Some(RuntimeSelectorControllerCommand::PageUp)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Ctrl('b')),
        Some(RuntimeSelectorControllerCommand::PageUp)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Char('g')),
        Some(RuntimeSelectorControllerCommand::CursorFirst)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Char('G')),
        Some(RuntimeSelectorControllerCommand::CursorLast)
    );
    assert_eq!(
        selector_control_command_for_key(&KeyInput::Escape),
        Some(RuntimeSelectorControllerCommand::Cancel)
    );
    assert_eq!(selector_control_command_for_key(&KeyInput::Enter), None);
}

#[test]
fn maps_printable_keys_and_backspace_to_query_edits() {
    assert_eq!(
        selector_query_edit_for_key(&KeyInput::Char('a')),
        Some(SelectorQueryEdit::Insert('a'))
    );
    assert_eq!(
        selector_query_edit_for_key(&KeyInput::Backspace),
        Some(SelectorQueryEdit::Backspace)
    );
    assert_eq!(SelectorQueryEdit::Insert('x').apply_to("ab"), "abx");
    assert_eq!(SelectorQueryEdit::Backspace.apply_to("ab"), "a");
    assert_eq!(SelectorQueryEdit::Backspace.apply_to(""), "");
    assert_eq!(selector_query_edit_for_key(&KeyInput::Enter), None);
}
