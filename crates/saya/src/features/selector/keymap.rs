use crate::features::selector::runtime::RuntimeSelectorControllerCommand;
use crate::features::selector::tui_state::{SelectorMode, SelectorTuiViewModel};
use crate::input::router::KeyInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorKeyRoute {
    Control {
        session_id: u64,
        command: RuntimeSelectorControllerCommand,
    },
    QueryEdit {
        session_id: u64,
        edit: SelectorQueryEdit,
    },
    ModeSwitch {
        session_id: u64,
        switch: SelectorModeSwitch,
    },
    Action {
        session_id: u64,
        action: SelectorAction,
    },
    Noop {
        session_id: u64,
    },
    Inactive,
    Unmapped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorAction {
    AcceptSelected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorModeSwitch {
    EnterInsert,
    EnterNormal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorQueryEdit {
    Insert(char),
    Backspace,
}

impl SelectorQueryEdit {
    pub fn apply_to(self, query: &str) -> String {
        let mut next = query.to_string();
        match self {
            SelectorQueryEdit::Insert(ch) => next.push(ch),
            SelectorQueryEdit::Backspace => {
                next.pop();
            }
        }
        next
    }
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
    if model.mode == SelectorMode::Insert && matches!(key, KeyInput::Escape) {
        return SelectorKeyRoute::ModeSwitch {
            session_id: model.session_id,
            switch: SelectorModeSwitch::EnterNormal,
        };
    }
    if matches!(key, KeyInput::Enter) {
        return SelectorKeyRoute::Action {
            session_id: model.session_id,
            action: SelectorAction::AcceptSelected,
        };
    }
    if model.mode == SelectorMode::Normal
        && matches!(key, KeyInput::Char('i') | KeyInput::Char('/'))
    {
        return SelectorKeyRoute::ModeSwitch {
            session_id: model.session_id,
            switch: SelectorModeSwitch::EnterInsert,
        };
    }
    if model.mode == SelectorMode::Insert {
        if let Some(command) = selector_insert_mode_control_command_for_key(key) {
            return SelectorKeyRoute::Control {
                session_id: model.session_id,
                command,
            };
        }
        if let Some(edit) = selector_query_edit_for_key(key) {
            return SelectorKeyRoute::QueryEdit {
                session_id: model.session_id,
                edit,
            };
        }
    }
    if let Some(command) = selector_control_command_for_key(key) {
        return SelectorKeyRoute::Control {
            session_id: model.session_id,
            command,
        };
    }
    if model.mode == SelectorMode::Normal && matches!(key, KeyInput::Char(_) | KeyInput::Backspace)
    {
        return SelectorKeyRoute::Noop {
            session_id: model.session_id,
        };
    }
    SelectorKeyRoute::Unmapped
}

pub fn selector_control_command_for_key(
    key: &KeyInput,
) -> Option<RuntimeSelectorControllerCommand> {
    match key {
        KeyInput::Char('j') | KeyInput::Down => Some(RuntimeSelectorControllerCommand::CursorNext),
        KeyInput::Ctrl('n') | KeyInput::Ctrl('N') => {
            Some(RuntimeSelectorControllerCommand::CursorNext)
        }
        KeyInput::Char('k') | KeyInput::Up => {
            Some(RuntimeSelectorControllerCommand::CursorPrevious)
        }
        KeyInput::Ctrl('p') | KeyInput::Ctrl('P') => {
            Some(RuntimeSelectorControllerCommand::CursorPrevious)
        }
        KeyInput::PageDown
        | KeyInput::Ctrl('d')
        | KeyInput::Ctrl('D')
        | KeyInput::Ctrl('f')
        | KeyInput::Ctrl('F') => Some(RuntimeSelectorControllerCommand::PageDown),
        KeyInput::PageUp
        | KeyInput::Ctrl('u')
        | KeyInput::Ctrl('U')
        | KeyInput::Ctrl('b')
        | KeyInput::Ctrl('B') => Some(RuntimeSelectorControllerCommand::PageUp),
        KeyInput::Char('g') => Some(RuntimeSelectorControllerCommand::CursorFirst),
        KeyInput::Char('G') => Some(RuntimeSelectorControllerCommand::CursorLast),
        KeyInput::Escape => Some(RuntimeSelectorControllerCommand::Cancel),
        _ => None,
    }
}

pub fn selector_insert_mode_control_command_for_key(
    key: &KeyInput,
) -> Option<RuntimeSelectorControllerCommand> {
    match key {
        KeyInput::Ctrl('n') | KeyInput::Ctrl('N') => {
            Some(RuntimeSelectorControllerCommand::CursorNext)
        }
        KeyInput::Ctrl('p') | KeyInput::Ctrl('P') => {
            Some(RuntimeSelectorControllerCommand::CursorPrevious)
        }
        KeyInput::Down => Some(RuntimeSelectorControllerCommand::CursorNext),
        KeyInput::Up => Some(RuntimeSelectorControllerCommand::CursorPrevious),
        KeyInput::PageDown
        | KeyInput::Ctrl('d')
        | KeyInput::Ctrl('D')
        | KeyInput::Ctrl('f')
        | KeyInput::Ctrl('F') => Some(RuntimeSelectorControllerCommand::PageDown),
        KeyInput::PageUp
        | KeyInput::Ctrl('u')
        | KeyInput::Ctrl('U')
        | KeyInput::Ctrl('b')
        | KeyInput::Ctrl('B') => Some(RuntimeSelectorControllerCommand::PageUp),
        _ => None,
    }
}

pub fn selector_query_edit_for_key(key: &KeyInput) -> Option<SelectorQueryEdit> {
    match key {
        KeyInput::Char(ch) => Some(SelectorQueryEdit::Insert(*ch)),
        KeyInput::Backspace => Some(SelectorQueryEdit::Backspace),
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
}
