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
#[path = "ex_command_test.rs"]
mod tests;
