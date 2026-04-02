use crate::editor_session::EditorSessionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalHostCommand {
    Save,
    SaveThenQuit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOptionCommand {
    EnableHlSearch,
    DisableHlSearch,
    ToggleHlSearch,
    EnableIncSearch,
    DisableIncSearch,
    ToggleIncSearch,
    ClearHlSearch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExCommandRoute {
    LocalHost(LocalHostCommand),
    SearchOption(SearchOptionCommand),
    PresentationLocal,
    CoreOwned,
}

pub fn apply_local_ex_command(
    session_state: &mut EditorSessionState,
    command: &str,
) -> Option<String> {
    let normalized = normalize_command(command)?;
    if let Some(search_option) = parse_search_option_command_normalized(&normalized) {
        log::debug!(
            "[ex_command] skipping search option command in host-local handler: command={:?}, search_option={:?}",
            command,
            search_option
        );
        return None;
    }

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
    parse_local_host_command_normalized(&normalized)
}

pub fn parse_search_option_command(command: &str) -> Option<SearchOptionCommand> {
    let normalized = normalize_command(command)?;
    parse_search_option_command_normalized(&normalized)
}

pub fn route_ex_command(command: &str) -> ExCommandRoute {
    let Some(normalized) = normalize_command(command) else {
        log::debug!("[ex_command] routing empty ex command as core-owned no-op");
        return ExCommandRoute::CoreOwned;
    };

    if let Some(host_command) = parse_local_host_command_normalized(&normalized) {
        log::debug!(
            "[ex_command] routing command to host save policy: command={:?}, host_command={:?}",
            command,
            host_command
        );
        return ExCommandRoute::LocalHost(host_command);
    }

    if let Some(search_option) = parse_search_option_command_normalized(&normalized) {
        log::debug!(
            "[ex_command] routing search command to core-owned option update: command={:?}, search_option={:?}",
            command,
            search_option
        );
        return ExCommandRoute::SearchOption(search_option);
    }

    if is_presentation_local_command_normalized(&normalized) {
        log::debug!(
            "[ex_command] routing command to host presentation state: command={:?}",
            command
        );
        return ExCommandRoute::PresentationLocal;
    }

    log::debug!(
        "[ex_command] routing command to core-owned ex handler: command={:?}",
        command
    );
    ExCommandRoute::CoreOwned
}

fn parse_local_host_command_normalized(normalized: &str) -> Option<LocalHostCommand> {
    match normalized {
        "w" | "write" => Some(LocalHostCommand::Save),
        "wq" | "x" => Some(LocalHostCommand::SaveThenQuit),
        _ => None,
    }
}

fn parse_search_option_command_normalized(normalized: &str) -> Option<SearchOptionCommand> {
    match normalized {
        "nohlsearch" => Some(SearchOptionCommand::ClearHlSearch),
        "set hlsearch" | "set hls" => Some(SearchOptionCommand::EnableHlSearch),
        "set nohlsearch" | "set nohls" => Some(SearchOptionCommand::DisableHlSearch),
        "set hls!" | "set invhlsearch" => Some(SearchOptionCommand::ToggleHlSearch),
        "set incsearch" => Some(SearchOptionCommand::EnableIncSearch),
        "set noincsearch" => Some(SearchOptionCommand::DisableIncSearch),
        "set incsearch!" | "set invincsearch" => Some(SearchOptionCommand::ToggleIncSearch),
        _ => None,
    }
}

fn is_presentation_local_command_normalized(normalized: &str) -> bool {
    matches!(
        normalized,
        "set number" | "set nu" | "set nonumber" | "set nonu"
    ) || normalized.starts_with("set numberwidth=")
        || normalized.starts_with("set nuw=")
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
    use crate::session_guard::test_lock as session_test_lock;

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
    fn route_ex_command_keeps_host_commands_separate_from_search_options() {
        assert_eq!(
            route_ex_command(":w"),
            ExCommandRoute::LocalHost(LocalHostCommand::Save)
        );
        assert_eq!(
            route_ex_command(":wq"),
            ExCommandRoute::LocalHost(LocalHostCommand::SaveThenQuit)
        );
        assert_eq!(
            parse_local_host_command(":set hlsearch"),
            None,
            "search option commands must not be parsed as host-local commands"
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
}
