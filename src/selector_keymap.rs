use crate::input_router::KeyInput;
use crate::selector_runtime::RuntimeSelectorControllerCommand;
use crate::selector_tui_state::SelectorTuiViewModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorKeyRoute {
    Control {
        session_id: u64,
        command: RuntimeSelectorControllerCommand,
    },
    Action {
        session_id: u64,
        action: SelectorAction,
    },
    Inactive,
    Unmapped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorAction {
    AcceptSelected,
}

pub fn selector_key_route_for_model(
    model: Option<&SelectorTuiViewModel>,
    key: &KeyInput,
) -> SelectorKeyRoute {
    let Some(model) = model else {
        return SelectorKeyRoute::Inactive;
    };
    if !selector_model_accepts_key(model) {
        return SelectorKeyRoute::Inactive;
    }
    if matches!(key, KeyInput::Enter) {
        return SelectorKeyRoute::Action {
            session_id: model.session_id,
            action: SelectorAction::AcceptSelected,
        };
    }
    let Some(command) = selector_control_command_for_key(key) else {
        return SelectorKeyRoute::Unmapped;
    };
    SelectorKeyRoute::Control {
        session_id: model.session_id,
        command,
    }
}

pub fn selector_control_command_for_key(
    key: &KeyInput,
) -> Option<RuntimeSelectorControllerCommand> {
    match key {
        KeyInput::Char('j') | KeyInput::Down => Some(RuntimeSelectorControllerCommand::CursorNext),
        KeyInput::Char('k') | KeyInput::Up => {
            Some(RuntimeSelectorControllerCommand::CursorPrevious)
        }
        KeyInput::PageDown | KeyInput::Ctrl('d') | KeyInput::Ctrl('D') => {
            Some(RuntimeSelectorControllerCommand::PageDown)
        }
        KeyInput::PageUp | KeyInput::Ctrl('u') | KeyInput::Ctrl('U') => {
            Some(RuntimeSelectorControllerCommand::PageUp)
        }
        KeyInput::Char('g') => Some(RuntimeSelectorControllerCommand::CursorFirst),
        KeyInput::Char('G') => Some(RuntimeSelectorControllerCommand::CursorLast),
        KeyInput::Escape => Some(RuntimeSelectorControllerCommand::Cancel),
        _ => None,
    }
}

fn selector_model_accepts_key(model: &SelectorTuiViewModel) -> bool {
    !model.hidden && !model.cancelled
}

#[cfg(test)]
mod tests {
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
            selector_control_command_for_key(&KeyInput::Char('k')),
            Some(RuntimeSelectorControllerCommand::CursorPrevious)
        );
        assert_eq!(
            selector_control_command_for_key(&KeyInput::Up),
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
            selector_control_command_for_key(&KeyInput::PageUp),
            Some(RuntimeSelectorControllerCommand::PageUp)
        );
        assert_eq!(
            selector_control_command_for_key(&KeyInput::Ctrl('u')),
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
}
