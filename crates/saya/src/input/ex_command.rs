use crate::app::session::EditorSessionState;
use crate::runtime::options::{
    ParsedSayaSet, SayaOptionOwner, SayaOptionRegistry, SayaOptionValue,
};

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
    NoOp,
    SearchOption(SearchOptionCommand),
    PresentationLocal,
    CoreOwned,
    UnsupportedPlanned,
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

    if let Ok(parsed) = SayaOptionRegistry::parse_set_command(&normalized) {
        log::debug!(
            "[ex_command] applying registry-driven local option command: command={:?}, parsed={:?}",
            command,
            parsed
        );
        return apply_parsed_presentation_option(session_state, parsed);
    }

    if let Some(message) = apply_number_width_command(session_state, &normalized, command) {
        return Some(message);
    }
    let message = match normalized.as_str() {
        "messages" | "message" | "mes" => {
            return Some(
                session_state
                    .reopen_message_pager()
                    .unwrap_or_else(|| "No messages".to_string()),
            );
        }
        "dired-cancel" | "diredcancel" => {
            return Some(
                match session_state.cancel_directory_buffer_operation_preview() {
                    Some(preview) => format!(
                        "Directory operation preview cancelled: {} operation(s), preview_id={}",
                        preview.operation_count, preview.id
                    ),
                    None => "No directory operation preview to cancel".to_string(),
                },
            );
        }
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

pub fn parse_search_option_command(command: &str) -> Option<SearchOptionCommand> {
    let normalized = normalize_command(command)?;
    parse_search_option_command_normalized(&normalized)
}

pub fn route_ex_command(command: &str) -> ExCommandRoute {
    let Some(normalized) = normalize_command(command) else {
        log::debug!("[ex_command] routing empty ex command as host no-op");
        return ExCommandRoute::NoOp;
    };

    if let Some(search_option) = parse_search_option_command_normalized(&normalized) {
        log::debug!(
            "[ex_command] routing search command to core-owned option update: command={:?}, search_option={:?}",
            command,
            search_option
        );
        return ExCommandRoute::SearchOption(search_option);
    }

    if let Ok(parsed) = SayaOptionRegistry::parse_set_command(&normalized) {
        return match parsed.definition.owner {
            SayaOptionOwner::PresentationOwned => {
                log::debug!(
                    "[ex_command] routing set command to presentation option pipeline: command={:?}, option={}",
                    command,
                    parsed.definition.name
                );
                ExCommandRoute::PresentationLocal
            }
            SayaOptionOwner::CoreOwned => {
                log::debug!(
                    "[ex_command] routing set command to core-owned option pipeline: command={:?}, option={}",
                    command,
                    parsed.definition.name
                );
                ExCommandRoute::CoreOwned
            }
            SayaOptionOwner::HostOwned | SayaOptionOwner::UnsupportedPlanned => {
                log::debug!(
                    "[ex_command] routing set command to unsupported planned option pipeline: command={:?}, option={}, owner={:?}",
                    command,
                    parsed.definition.name,
                    parsed.definition.owner
                );
                ExCommandRoute::UnsupportedPlanned
            }
        };
    }

    if is_presentation_local_command_normalized(&normalized) {
        log::debug!(
            "[ex_command] routing command to host presentation state: command={:?}",
            command
        );
        return ExCommandRoute::PresentationLocal;
    }

    if is_save_family_command_normalized(&normalized) {
        log::debug!(
            "[ex_command] routing save-family command to core-owned ex handler: command={:?}",
            command
        );
        return ExCommandRoute::CoreOwned;
    }

    log::debug!(
        "[ex_command] routing command to core-owned ex handler: command={:?}",
        command
    );
    ExCommandRoute::CoreOwned
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
        "messages"
            | "message"
            | "mes"
            | "dired-cancel"
            | "diredcancel"
            | "set number"
            | "set nu"
            | "set nonumber"
            | "set nonu"
    ) || normalized.starts_with("set numberwidth=")
        || normalized.starts_with("set nuw=")
}

fn is_save_family_command_normalized(normalized: &str) -> bool {
    matches!(normalized, "w" | "write" | "wq" | "x" | "xit" | "exit")
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

fn apply_parsed_presentation_option(
    session_state: &mut EditorSessionState,
    parsed: ParsedSayaSet,
) -> Option<String> {
    if parsed.definition.owner != SayaOptionOwner::PresentationOwned {
        return None;
    }
    let current = current_presentation_value(session_state, parsed.definition.name)?;
    let is_query = matches!(
        parsed.operation,
        crate::runtime::options::SayaSetOperation::Query
    );
    let value = match &parsed.operation {
        crate::runtime::options::SayaSetOperation::Assign(value) => value.clone(),
        crate::runtime::options::SayaSetOperation::Toggle => match current {
            SayaOptionValue::Boolean(value) => SayaOptionValue::Boolean(!value),
            _ => return None,
        },
        crate::runtime::options::SayaSetOperation::Invert => match current {
            SayaOptionValue::Boolean(value) => SayaOptionValue::Boolean(!value),
            _ => return None,
        },
        crate::runtime::options::SayaSetOperation::Query => current,
    };
    let name = parsed.definition.name;
    if !is_query {
        if let Err(message) = session_state.apply_presentation_option(name, value.clone()) {
            log::debug!(
                "[ex_command] presentation option application failed: option={}, value={:?}, error={}",
                name,
                value,
                message
            );
            return Some(message);
        }
    }
    let rendered_value = current_presentation_value(session_state, name).unwrap_or(value);
    let rendered = render_option_message(name.canonical(), &rendered_value);
    Some(rendered)
}

fn current_presentation_value(
    session_state: &EditorSessionState,
    name: crate::runtime::options::SayaOptionName,
) -> Option<SayaOptionValue> {
    Some(match name {
        crate::runtime::options::SayaOptionName::LineNumbers => {
            SayaOptionValue::Boolean(session_state.line_numbers())
        }
        crate::runtime::options::SayaOptionName::NumberWidth => {
            SayaOptionValue::Number(i64::from(session_state.number_width()))
        }
        crate::runtime::options::SayaOptionName::RelativeNumber => {
            SayaOptionValue::Boolean(session_state.relative_number())
        }
        crate::runtime::options::SayaOptionName::CursorLine => {
            SayaOptionValue::Boolean(session_state.cursorline())
        }
        crate::runtime::options::SayaOptionName::ScrollOff => {
            SayaOptionValue::Number(i64::from(session_state.scrolloff()))
        }
        crate::runtime::options::SayaOptionName::SidescrollOff => {
            SayaOptionValue::Number(i64::from(session_state.sidescrolloff()))
        }
        crate::runtime::options::SayaOptionName::Wrap => {
            SayaOptionValue::Boolean(session_state.wrap())
        }
        crate::runtime::options::SayaOptionName::LastStatus => {
            SayaOptionValue::Number(i64::from(session_state.laststatus()))
        }
        crate::runtime::options::SayaOptionName::MessageHeight => {
            SayaOptionValue::Number(i64::from(session_state.message_area_height()))
        }
        crate::runtime::options::SayaOptionName::List => {
            SayaOptionValue::Boolean(session_state.list())
        }
        crate::runtime::options::SayaOptionName::ListChars => {
            SayaOptionValue::String(session_state.listchars().to_string())
        }
        crate::runtime::options::SayaOptionName::MarkdownRender => {
            SayaOptionValue::Boolean(session_state.markdown_render())
        }
        crate::runtime::options::SayaOptionName::MermaidPreview => {
            SayaOptionValue::Boolean(session_state.mermaid_preview_auto())
        }
        crate::runtime::options::SayaOptionName::MermaidPreviewBackground => {
            SayaOptionValue::String(session_state.mermaid_preview_background().to_string())
        }
        crate::runtime::options::SayaOptionName::MermaidPreviewWidth => {
            SayaOptionValue::Number(i64::from(session_state.mermaid_preview_width_percent()))
        }
        crate::runtime::options::SayaOptionName::MermaidPreviewHeight => {
            SayaOptionValue::Number(i64::from(session_state.mermaid_preview_height_percent()))
        }
        crate::runtime::options::SayaOptionName::FoldMethod => {
            SayaOptionValue::String(session_state.foldmethod().to_string())
        }
        crate::runtime::options::SayaOptionName::FoldLevel => {
            SayaOptionValue::Number(i64::from(session_state.foldlevel()))
        }
        _ => return None,
    })
}

fn render_option_message(name: &str, value: &SayaOptionValue) -> String {
    if name == "number" {
        return match value {
            SayaOptionValue::Boolean(true) => "line numbers: on".to_string(),
            SayaOptionValue::Boolean(false) => "line numbers: off".to_string(),
            _ => format!("{name}={value:?}"),
        };
    }
    match value {
        SayaOptionValue::Boolean(true) => format!("{name}: on"),
        SayaOptionValue::Boolean(false) => format!("{name}: off"),
        SayaOptionValue::Number(value) => format!("{name}={value}"),
        SayaOptionValue::String(value) => format!("{name}={value}"),
    }
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
    use crate::presentation::screen_model::{ProjectionInput, project};
    use crate::support::session_guard::test_lock as session_test_lock;

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
}
