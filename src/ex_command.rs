use crate::editor_session::EditorSessionState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalHostCommand {
    Save,
    SaveThenQuit,
}

pub fn apply_local_ex_command(
    session_state: &mut EditorSessionState,
    command: &str,
) -> Option<String> {
    let normalized = normalize_command(command)?;
    if let Some(message) = apply_number_width_command(session_state, &normalized, command) {
        return Some(message);
    }
    let message = match normalized.as_str() {
        "set number" | "set nu" => {
            log::debug!(
                "[ex_command] enabling line numbers from command: {:?}",
                command
            );
            session_state.set_line_numbers(true);
            "line numbers: on"
        }
        "set nonumber" | "set nonu" => {
            log::debug!(
                "[ex_command] disabling line numbers from command: {:?}",
                command
            );
            session_state.set_line_numbers(false);
            "line numbers: off"
        }
        _ => return None,
    };

    Some(message.to_string())
}

pub fn parse_local_host_command(command: &str) -> Option<LocalHostCommand> {
    let normalized = normalize_command(command)?;
    match normalized.as_str() {
        "w" | "write" => {
            log::debug!(
                "[ex_command] routing command to host save policy: {:?}",
                command
            );
            Some(LocalHostCommand::Save)
        }
        "wq" | "x" => {
            log::debug!(
                "[ex_command] routing command to host save-then-quit policy: {:?}",
                command
            );
            Some(LocalHostCommand::SaveThenQuit)
        }
        _ => None,
    }
}

fn apply_number_width_command(
    session_state: &mut EditorSessionState,
    normalized: &str,
    original_command: &str,
) -> Option<String> {
    let value = normalized
        .strip_prefix("set numberwidth=")
        .or_else(|| normalized.strip_prefix("set nuw="))
        .and_then(|value| value.parse::<u16>().ok())?;
    session_state.set_number_width(value);
    log::debug!(
        "[ex_command] updating number width from command: {:?}, width={}",
        original_command,
        session_state.number_width()
    );
    Some(format!("numberwidth={}", session_state.number_width()))
}

fn normalize_command(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let trimmed = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.split_whitespace().collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use crate::screen_model::{ProjectionInput, project};

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
        let mut session_state =
            EditorSessionState::new_with_tab_size_and_line_numbers(None, 8, true);
        let message = apply_local_ex_command(&mut session_state, ":set nonumber");

        assert_eq!(message, Some("line numbers: off".to_string()));
        assert!(!session_state.line_numbers());
    }

    #[test]
    fn apply_local_ex_command_disables_line_numbers_for_set_nonu() {
        let mut session_state =
            EditorSessionState::new_with_tab_size_and_line_numbers(None, 8, true);
        let message = apply_local_ex_command(&mut session_state, "set nonu");

        assert_eq!(message, Some("line numbers: off".to_string()));
        assert!(!session_state.line_numbers());
    }

    #[test]
    fn apply_local_ex_command_updates_screen_projection() {
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
        let mut session_state =
            EditorSessionState::new_with_tab_size_and_line_numbers(None, 8, true);

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
    fn apply_local_ex_command_returns_none_for_unknown_command() {
        let mut session_state = EditorSessionState::new(None);

        let result = apply_local_ex_command(&mut session_state, ":w");

        assert_eq!(result, None);
        assert!(!session_state.line_numbers());
    }

    #[test]
    fn parse_local_host_command_routes_write_variants_to_host_policy() {
        assert_eq!(parse_local_host_command(":w"), Some(LocalHostCommand::Save));
        assert_eq!(
            parse_local_host_command("write"),
            Some(LocalHostCommand::Save)
        );
        assert_eq!(
            parse_local_host_command(":wq"),
            Some(LocalHostCommand::SaveThenQuit)
        );
        assert_eq!(
            parse_local_host_command("x"),
            Some(LocalHostCommand::SaveThenQuit)
        );
    }
}
