use super::*;

#[test]
fn vim_style_history_keys_map_to_navigation_directions() {
    assert_eq!(
        history_direction_for_key(&KeyInput::Up),
        Some(CommandLineHistoryDirection::Previous)
    );
    assert_eq!(
        history_direction_for_key(&KeyInput::Ctrl('p')),
        Some(CommandLineHistoryDirection::Previous)
    );
    assert_eq!(
        history_direction_for_key(&KeyInput::Down),
        Some(CommandLineHistoryDirection::Next)
    );
    assert_eq!(
        history_direction_for_key(&KeyInput::Ctrl('n')),
        Some(CommandLineHistoryDirection::Next)
    );
    assert_eq!(history_direction_for_key(&KeyInput::Char('p')), None);
}
