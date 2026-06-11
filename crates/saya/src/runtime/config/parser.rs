//! TypeScript 設定ソースの評価・構文解析（パース層）。

use super::*;

/// 設定ソースを評価し、ConfigCommand 列に変換する。
///
/// option 変更と key mapping に必要な最小コマンドへ変換する。
/// Vim script 前提の入力を受け付けない形にする。
pub fn evaluate_config(source_result: &ConfigSourceResult) -> ConfigLoadResult {
    log::debug!(
        "[config_runtime] evaluating config: {:?}",
        match source_result {
            ConfigSourceResult::Default => "default".to_string(),
            ConfigSourceResult::Loaded { path, .. } => format!("loaded:{}", path.display()),
            ConfigSourceResult::ReadFailed { path, .. } =>
                format!("read_failed:{}", path.display()),
        }
    );

    match evaluate_capability_source(source_result) {
        CapabilityLoadResult::DefaultUsed => ConfigLoadResult::DefaultUsed,
        CapabilityLoadResult::ReadFailed { path, message } => {
            ConfigLoadResult::ReadFailed { path, message }
        }
        CapabilityLoadResult::EvalFailed { path, message } => {
            ConfigLoadResult::EvalFailed { path, message }
        }
        CapabilityLoadResult::UnsupportedCapability {
            path, capability, ..
        } => ConfigLoadResult::EvalFailed {
            path,
            message: format!("未対応の capability です: {}", capability),
        },
        CapabilityLoadResult::Success { commands, .. } => ConfigLoadResult::Success { commands },
    }
}

/// TypeScript capability source を評価し、startup registry を返す。
pub fn evaluate_capability_source(source_result: &ConfigSourceResult) -> CapabilityLoadResult {
    log::debug!(
        "[config_runtime] evaluating capability source: {:?}",
        match source_result {
            ConfigSourceResult::Default => "default".to_string(),
            ConfigSourceResult::Loaded { path, .. } => format!("loaded:{}", path.display()),
            ConfigSourceResult::ReadFailed { path, .. } =>
                format!("read_failed:{}", path.display()),
        }
    );

    match source_result {
        ConfigSourceResult::Default => {
            log::debug!("[config_runtime] using default capability source");
            CapabilityLoadResult::DefaultUsed
        }
        ConfigSourceResult::ReadFailed { path, message } => {
            log::debug!(
                "[config_runtime] capability source read failed: path={}, message={}",
                path.display(),
                message
            );
            CapabilityLoadResult::ReadFailed {
                path: path.clone(),
                message: message.clone(),
            }
        }
        ConfigSourceResult::Loaded { path, source } => {
            log::debug!(
                "[config_runtime] evaluating capability source text: path={}, len={}",
                path.display(),
                source.len()
            );
            evaluate_capability_source_text(path, source)
        }
    }
}

/// 設定ソース文字列を評価し、startup registry を返す。
fn evaluate_capability_source_text(path: &std::path::Path, source: &str) -> CapabilityLoadResult {
    log::debug!(
        "[config_runtime] parsing capability source: path={}, source_preview={:?}",
        path.display(),
        &source[..source.len().min(100)]
    );

    if is_vim_script_syntax(source) {
        log::debug!(
            "[config_runtime] rejected: Vim script syntax detected in config: {}",
            path.display()
        );
        return CapabilityLoadResult::EvalFailed {
            path: path.to_path_buf(),
            message: "Vim script 形式の設定は受け付けません。TypeScript 形式で記述してください。"
                .to_string(),
        };
    }

    match parse_capability_program(source) {
        Ok(program) => {
            log::debug!(
                "[config_runtime] capability evaluation success: registry_entries={}, commands_count={}",
                program.registry.entries().len(),
                program.commands.len()
            );
            CapabilityLoadResult::Success {
                path: path.to_path_buf(),
                registry: program.registry,
                commands: program.commands,
            }
        }
        Err(CapabilityParseError::UnsupportedCapability {
            capability,
            message,
        }) => {
            log::debug!(
                "[config_runtime] unsupported capability detected: path={}, capability={}, message={}",
                path.display(),
                capability,
                message
            );
            CapabilityLoadResult::UnsupportedCapability {
                path: path.to_path_buf(),
                capability,
                message,
            }
        }
        Err(CapabilityParseError::EvalFailed(message)) => {
            log::debug!(
                "[config_runtime] capability evaluation failed: path={}, error={}",
                path.display(),
                message
            );
            CapabilityLoadResult::EvalFailed {
                path: path.to_path_buf(),
                message,
            }
        }
    }
}

/// Vim script 構文を検出する。
///
/// `set`, `let`, `map`, `noremap` などの Vim script 特有のコマンドを検出する。
fn is_vim_script_syntax(source: &str) -> bool {
    let trimmed = source.trim();
    let vim_patterns = [
        "set ",
        "let ",
        "let g:",
        "noremap ",
        "nnoremap ",
        "inoremap ",
        "vnoremap ",
        "map ",
        "nmap ",
        "imap ",
        "vmap ",
        "autocmd ",
        "augroup ",
        "function!",
        "endfunction",
        "if has(",
        "source ",
        "colorscheme ",
        "syntax ",
        "filetype ",
    ];

    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('"') {
            continue;
        }
        for pattern in &vim_patterns {
            if line.starts_with(pattern) {
                log::debug!(
                    "[config_runtime] vim script pattern detected: {:?} in line: {:?}",
                    pattern,
                    line
                );
                return true;
            }
        }
    }
    false
}

/// MVP 用の JSON 設定パーサー。
///
/// `{ "options": {...}, "keyMappings": [...] }` 形式を受け付ける。
/// deno_core による TypeScript 評価への移行を前提とした最小実装。
fn parse_config_json(source: &str) -> Result<Vec<ConfigCommand>, String> {
    let trimmed = source.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        log::debug!("[config_runtime] empty or trivial config, returning empty commands");
        return Ok(Vec::new());
    }

    // 簡易 JSON パーサー（外部依存なし、MVP 最小限）
    let mut commands = Vec::new();

    // "tabstop": <number> を検出
    if let Some(value) = extract_json_number(trimmed, "tabstop") {
        log::debug!("[config_runtime] found tabstop option: {}", value);
        commands.push(ConfigCommand::SetOption {
            name: ConfigOptionName::TabSize,
            value: ConfigOptionValue::Number(value),
        });
    }

    // "number": <bool> を検出
    if let Some(value) = extract_json_bool(trimmed, "number") {
        log::debug!("[config_runtime] found number option: {}", value);
        commands.push(ConfigCommand::SetOption {
            name: ConfigOptionName::LineNumbers,
            value: ConfigOptionValue::Boolean(value),
        });
    }

    // "numberwidth": <number> を検出
    if let Some(value) = extract_json_number(trimmed, "numberwidth") {
        log::debug!("[config_runtime] found numberwidth option: {}", value);
        commands.push(ConfigCommand::SetOption {
            name: ConfigOptionName::NumberWidth,
            value: ConfigOptionValue::Number(value),
        });
    }

    // "cmdheight": <number> を検出
    if let Some(value) = extract_json_number(trimmed, "cmdheight") {
        log::debug!("[config_runtime] found cmdheight option: {}", value);
        commands.push(ConfigCommand::SetOption {
            name: ConfigOptionName::MessageHeight,
            value: ConfigOptionValue::Number(value),
        });
    }

    // "keyMappings" 配列は MVP では簡易的に扱う
    // 完全な JSON パースは deno_core 移行時に置き換え予定

    log::debug!(
        "[config_runtime] parsed {} commands from JSON config",
        commands.len()
    );
    Ok(commands)
}

/// TypeScript capability program の評価結果。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedCapabilityProgram {
    registry: StartupRegistry,
    commands: Vec<ConfigCommand>,
}

/// capability ソースの評価失敗。
#[derive(Debug, Clone, PartialEq, Eq)]
enum CapabilityParseError {
    EvalFailed(String),
    UnsupportedCapability { capability: String, message: String },
}

/// startup phase の TypeScript 形式を解析する。
fn parse_capability_program(source: &str) -> Result<ParsedCapabilityProgram, CapabilityParseError> {
    let trimmed = source.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        log::debug!("[config_runtime] empty capability source, returning empty registry");
        return Ok(ParsedCapabilityProgram {
            registry: StartupRegistry::default(),
            commands: Vec::new(),
        });
    }

    if looks_like_json(trimmed) {
        let commands = parse_config_json(trimmed).map_err(CapabilityParseError::EvalFailed)?;
        let registry = registry_from_commands(&commands);
        return Ok(ParsedCapabilityProgram { registry, commands });
    }

    let statements = split_top_level_statements(trimmed);
    log::debug!(
        "[config_runtime] parsed {} top-level statements from capability source",
        statements.len()
    );

    let mut registry = StartupRegistry::default();
    let mut commands = Vec::new();

    for statement in statements {
        let statement = statement.trim();
        if statement.is_empty() {
            continue;
        }

        log::debug!("[config_runtime] inspecting statement: {:?}", statement);

        if is_scaffolding_statement(statement) {
            log::debug!(
                "[config_runtime] ignoring type-only scaffolding statement: {:?}",
                statement
            );
            continue;
        }

        if is_legacy_vim_compat_statement(statement) {
            return Err(CapabilityParseError::EvalFailed(
                "Vim script 形式の設定は受け付けません。TypeScript 形式で記述してください。"
                    .to_string(),
            ));
        }

        if let Some((entry, command)) = parse_capability_statement(statement)? {
            registry.push(entry);
            if let Some(command) = command {
                commands.push(command);
            }
            continue;
        }

        return Err(CapabilityParseError::EvalFailed(format!(
            "未対応のトップレベル構文です: {}",
            statement
        )));
    }

    Ok(ParsedCapabilityProgram { registry, commands })
}

fn registry_from_commands(commands: &[ConfigCommand]) -> StartupRegistry {
    let mut registry = StartupRegistry::default();
    for command in commands {
        match command {
            ConfigCommand::SetOption { name, value } => {
                let option_name = saya_option_name_from_config_name(*name);
                let option_value = match value {
                    ConfigOptionValue::Number(number) => SayaOptionValue::Number(*number),
                    ConfigOptionValue::Boolean(boolean) => SayaOptionValue::Boolean(*boolean),
                    ConfigOptionValue::String(value) => SayaOptionValue::String(value.clone()),
                };
                registry.push(StartupRegistryEntry::Option {
                    name: option_name,
                    value: option_value,
                });
            }
            ConfigCommand::MapKey { mode, lhs, rhs } => {
                registry.push(StartupRegistryEntry::Keymap {
                    mode: match mode {
                        ConfigKeyMode::Normal => SayaKeyMode::Normal,
                        ConfigKeyMode::Insert => SayaKeyMode::Insert,
                    },
                    lhs: lhs.clone(),
                    action: SayaKeymapAction::Literal(rhs.clone()),
                });
            }
        }
    }
    registry
}

fn parse_capability_statement(
    statement: &str,
) -> Result<Option<(StartupRegistryEntry, Option<ConfigCommand>)>, CapabilityParseError> {
    if let Some((name, value)) = parse_option_statement(statement)? {
        let config_name: ConfigOptionName = name.into();
        let config_value: ConfigOptionValue = value.clone().into();
        return Ok(Some((
            StartupRegistryEntry::Option { name, value },
            Some(ConfigCommand::SetOption {
                name: config_name,
                value: config_value,
            }),
        )));
    }

    if let Some((entry, command)) = parse_keymap_statement(statement)? {
        return Ok(Some((entry, command)));
    }

    if let Some(entry) = parse_command_register_statement(statement)? {
        return Ok(Some((entry, None)));
    }

    if let Some(entry) = parse_event_register_statement(statement)? {
        return Ok(Some((entry, None)));
    }

    if let Some(capability) = detect_unsupported_capability(statement) {
        return Err(CapabilityParseError::UnsupportedCapability {
            capability,
            message: "startup phase で公開しない capability が含まれています".to_string(),
        });
    }

    Ok(None)
}

fn parse_option_statement(
    statement: &str,
) -> Result<Option<(SayaOptionName, SayaOptionValue)>, CapabilityParseError> {
    let prefix = "saya.options.";
    let Some(rest) = statement.strip_prefix(prefix) else {
        return Ok(None);
    };

    let Some((lhs, rhs)) = rest.split_once('=') else {
        return Err(CapabilityParseError::EvalFailed(format!(
            "option 設定の構文が不正です: {}",
            statement
        )));
    };

    let option_name = normalize_option_name(lhs.trim()).ok_or_else(|| {
        CapabilityParseError::EvalFailed(format!("未対応の option 名です: {}", lhs.trim()))
    })?;
    let rhs = rhs.trim().trim_end_matches(';').trim();

    let definition = crate::runtime::options::SayaOptionRegistry::resolve(lhs.trim())
        .expect("normalized startup option should resolve");
    let value = match definition.value_type {
        crate::runtime::options::SayaOptionType::Boolean => match rhs {
            "true" => SayaOptionValue::Boolean(true),
            "false" => SayaOptionValue::Boolean(false),
            _ => {
                return Err(CapabilityParseError::EvalFailed(format!(
                    "option {} の値が不正です: {}",
                    lhs.trim(),
                    rhs
                )));
            }
        },
        crate::runtime::options::SayaOptionType::Number => rhs
            .parse::<i64>()
            .map(SayaOptionValue::Number)
            .map_err(|_| {
                CapabilityParseError::EvalFailed(format!(
                    "option {} の値が不正です: {}",
                    lhs.trim(),
                    rhs
                ))
            })?,
        crate::runtime::options::SayaOptionType::String => {
            SayaOptionValue::String(parse_string_literal(rhs).unwrap_or_else(|| rhs.to_string()))
        }
    };

    log::debug!(
        "[config_runtime] parsed option statement: name={:?}, value={:?}",
        option_name,
        value
    );
    Ok(Some((option_name, value)))
}

fn parse_keymap_statement(
    statement: &str,
) -> Result<Option<(StartupRegistryEntry, Option<ConfigCommand>)>, CapabilityParseError> {
    let prefix = "saya.keymap.set";
    let Some(args) = extract_call_arguments(statement, prefix) else {
        return Ok(None);
    };

    let parts = split_top_level_arguments(&args);
    if parts.len() != 3 {
        return Err(CapabilityParseError::EvalFailed(format!(
            "keymap.set は 3 引数が必要です: {}",
            statement
        )));
    }

    let mode = parse_key_mode(parts[0].trim())?;
    let lhs = parse_string_literal(parts[1].trim()).ok_or_else(|| {
        CapabilityParseError::EvalFailed(format!(
            "keymap.set の lhs は文字列である必要があります: {}",
            parts[1].trim()
        ))
    })?;
    let action_expr = parts[2].trim();
    let action = if let Some(action) = parse_string_literal(action_expr) {
        SayaKeymapAction::Literal(action)
    } else if let Some(command_name) = parse_registered_command_reference(action_expr) {
        SayaKeymapAction::RegisteredCommand(command_name)
    } else {
        return Err(CapabilityParseError::EvalFailed(format!(
            "keymap.set の action が不正です: {}",
            action_expr
        )));
    };

    log::debug!(
        "[config_runtime] parsed keymap statement: mode={:?}, lhs={:?}, action={:?}",
        mode,
        lhs,
        action
    );

    Ok(Some((
        StartupRegistryEntry::Keymap { mode, lhs, action },
        None,
    )))
}

fn parse_command_register_statement(
    statement: &str,
) -> Result<Option<StartupRegistryEntry>, CapabilityParseError> {
    let prefix = "saya.commands.register";
    let Some(args) = extract_call_arguments(statement, prefix) else {
        return Ok(None);
    };

    let parts = split_top_level_arguments(&args);
    if parts.len() != 2 {
        return Err(CapabilityParseError::EvalFailed(format!(
            "commands.register は 2 引数が必要です: {}",
            statement
        )));
    }

    let name = parse_string_literal(parts[0].trim()).ok_or_else(|| {
        CapabilityParseError::EvalFailed(format!(
            "commands.register の name は文字列である必要があります: {}",
            parts[0].trim()
        ))
    })?;
    let callback_source = extract_arrow_callback_body(parts[1].trim()).ok_or_else(|| {
        CapabilityParseError::EvalFailed(format!(
            "commands.register の callback が不正です: {}",
            parts[1].trim()
        ))
    })?;

    log::debug!(
        "[config_runtime] parsed command registration: name={:?}",
        name
    );

    Ok(Some(StartupRegistryEntry::Command {
        name,
        callback_source,
    }))
}

fn parse_event_register_statement(
    statement: &str,
) -> Result<Option<StartupRegistryEntry>, CapabilityParseError> {
    let prefix = "saya.events.on";
    let Some(args) = extract_call_arguments(statement, prefix) else {
        return Ok(None);
    };

    let parts = split_top_level_arguments(&args);
    if parts.len() != 2 {
        return Err(CapabilityParseError::EvalFailed(format!(
            "events.on は 2 引数が必要です: {}",
            statement
        )));
    }

    let name = parse_string_literal(parts[0].trim()).ok_or_else(|| {
        CapabilityParseError::EvalFailed(format!(
            "events.on の event 名は文字列である必要があります: {}",
            parts[0].trim()
        ))
    })?;
    let callback_source = extract_arrow_callback_body(parts[1].trim()).ok_or_else(|| {
        CapabilityParseError::EvalFailed(format!(
            "events.on の callback が不正です: {}",
            parts[1].trim()
        ))
    })?;

    log::debug!(
        "[config_runtime] parsed event subscription: name={:?}",
        name
    );

    Ok(Some(StartupRegistryEntry::Event {
        name,
        callback_source,
    }))
}

fn parse_key_mode(value: &str) -> Result<SayaKeyMode, CapabilityParseError> {
    let normalized = parse_string_literal(value).unwrap_or_else(|| value.trim().to_string());
    match normalized.as_str() {
        "normal" => Ok(SayaKeyMode::Normal),
        "insert" => Ok(SayaKeyMode::Insert),
        "visual" => Ok(SayaKeyMode::Visual),
        other => Err(CapabilityParseError::EvalFailed(format!(
            "未対応の keymap mode です: {}",
            other
        ))),
    }
}

fn normalize_option_name(value: &str) -> Option<SayaOptionName> {
    crate::runtime::options::SayaOptionRegistry::resolve(value)
        .filter(|definition| definition.startup_public)
        .map(|definition| definition.name)
}

fn saya_option_name_from_config_name(name: ConfigOptionName) -> SayaOptionName {
    match name {
        ConfigOptionName::AutoIndent => SayaOptionName::AutoIndent,
        ConfigOptionName::CursorLine => SayaOptionName::CursorLine,
        ConfigOptionName::ExpandTab => SayaOptionName::ExpandTab,
        ConfigOptionName::FoldLevel => SayaOptionName::FoldLevel,
        ConfigOptionName::FoldMethod => SayaOptionName::FoldMethod,
        ConfigOptionName::IgnoreCase => SayaOptionName::IgnoreCase,
        ConfigOptionName::LastStatus => SayaOptionName::LastStatus,
        ConfigOptionName::List => SayaOptionName::List,
        ConfigOptionName::ListChars => SayaOptionName::ListChars,
        ConfigOptionName::MermaidPreview => SayaOptionName::MermaidPreview,
        ConfigOptionName::MermaidPreviewBackground => SayaOptionName::MermaidPreviewBackground,
        ConfigOptionName::MermaidPreviewHeight => SayaOptionName::MermaidPreviewHeight,
        ConfigOptionName::MermaidPreviewWidth => SayaOptionName::MermaidPreviewWidth,
        ConfigOptionName::MessageHeight => SayaOptionName::MessageHeight,
        ConfigOptionName::RelativeNumber => SayaOptionName::RelativeNumber,
        ConfigOptionName::ScrollOff => SayaOptionName::ScrollOff,
        ConfigOptionName::ShiftWidth => SayaOptionName::ShiftWidth,
        ConfigOptionName::SidescrollOff => SayaOptionName::SidescrollOff,
        ConfigOptionName::SmartCase => SayaOptionName::SmartCase,
        ConfigOptionName::SmartIndent => SayaOptionName::SmartIndent,
        ConfigOptionName::SoftTabStop => SayaOptionName::SoftTabStop,
        ConfigOptionName::Syntax => SayaOptionName::Syntax,
        ConfigOptionName::TabSize => SayaOptionName::TabSize,
        ConfigOptionName::LineNumbers => SayaOptionName::LineNumbers,
        ConfigOptionName::NumberWidth => SayaOptionName::NumberWidth,
        ConfigOptionName::Wrap => SayaOptionName::Wrap,
    }
}

fn parse_string_literal(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() < 2 {
        return None;
    }

    let bytes = value.as_bytes();
    let quote = bytes.first().copied()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    if bytes.last().copied()? != quote {
        return None;
    }

    Some(value[1..value.len() - 1].to_string())
}

fn parse_registered_command_reference(value: &str) -> Option<String> {
    let value = value.trim();
    let prefix = "saya.commands.execute";
    let args = extract_call_arguments(value, prefix)?;
    parse_string_literal(args.trim())
}

fn extract_call_arguments(statement: &str, prefix: &str) -> Option<String> {
    let start = statement.find(prefix)?;
    let after_prefix = &statement[start + prefix.len()..];
    let mut chars = after_prefix.char_indices();
    let mut open_index = None;
    while let Some((idx, ch)) = chars.next() {
        if ch == '(' {
            open_index = Some(idx);
            break;
        }
        if !ch.is_whitespace() {
            return None;
        }
    }
    let open_index = open_index?;
    let after_open = &after_prefix[open_index + 1..];
    let mut depth = 1usize;
    let mut in_string: Option<char> = None;
    let mut escape = false;
    for (idx, ch) in after_open.char_indices() {
        if let Some(quote) = in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' => in_string = Some(ch),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(after_open[..idx].to_string());
                }
            }
            _ => {}
        }
    }

    None
}

fn extract_arrow_callback_body(callback_source: &str) -> Option<String> {
    let normalized = callback_source.trim().trim_end_matches(';').trim();
    let arrow_index = normalized.find("=>")?;
    let after_arrow = normalized[arrow_index + 2..].trim();

    if after_arrow.starts_with('{') {
        let inner = after_arrow
            .trim_start_matches('{')
            .trim_end_matches('}')
            .trim();
        return Some(collapse_whitespace(inner));
    }

    Some(collapse_whitespace(after_arrow))
}

fn split_top_level_arguments(args: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string: Option<char> = None;
    let mut escape = false;

    for ch in args.chars() {
        if let Some(quote) = in_string {
            current.push(ch);
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' => {
                in_string = Some(ch);
                current.push(ch);
            }
            '(' => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                current.push(ch);
            }
            '{' => {
                brace_depth += 1;
                current.push(ch);
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                current.push(ch);
            }
            '[' => {
                bracket_depth += 1;
                current.push(ch);
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                result.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.trim().is_empty() {
        result.push(current.trim().to_string());
    }

    result
}

fn split_top_level_statements(source: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string: Option<char> = None;
    let mut escape = false;

    for ch in source.chars() {
        if let Some(quote) = in_string {
            current.push(ch);
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' => {
                in_string = Some(ch);
                current.push(ch);
            }
            '(' => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                current.push(ch);
            }
            '{' => {
                brace_depth += 1;
                current.push(ch);
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                current.push(ch);
            }
            '[' => {
                bracket_depth += 1;
                current.push(ch);
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                current.push(ch);
            }
            ';' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                let statement = current.trim();
                if !statement.is_empty() {
                    statements.push(statement.to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.trim().is_empty() {
        statements.push(current.trim().to_string());
    }

    statements
}

fn collapse_whitespace(source: &str) -> String {
    source
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn looks_like_json(source: &str) -> bool {
    source.starts_with('{') || source.starts_with('[')
}

fn is_scaffolding_statement(statement: &str) -> bool {
    let statement = statement.trim_start();
    matches!(
        statement,
        s if s.starts_with("import ")
            || s.starts_with("export ")
            || s.starts_with("const ")
            || s.starts_with("let ")
            || s.starts_with("var ")
            || s.starts_with("function ")
            || s.starts_with("async function ")
            || s.starts_with("type ")
            || s.starts_with("interface ")
            || s.starts_with("class ")
            || s.starts_with("return ")
            || s == "{"
            || s == "}"
    )
}

fn is_legacy_vim_compat_statement(statement: &str) -> bool {
    let statement = statement.trim_start();
    statement.starts_with(':')
        || statement.starts_with("set ")
        || statement.starts_with("map ")
        || statement.starts_with("nnoremap ")
        || statement.starts_with("noremap ")
        || statement.starts_with("vim.cmd(")
}

fn detect_unsupported_capability(statement: &str) -> Option<String> {
    let statement = statement.trim();
    let unsupported_prefixes = [
        "saya.commands.execute(",
        "saya.buffer.",
        "saya.window.",
        "saya.editor.",
        "saya.filesystem.",
        "saya.network.",
        "editor.",
        "buffer.",
        "window.",
        "fetch(",
        "Deno.readTextFile(",
        "Deno.writeTextFile(",
    ];

    for prefix in unsupported_prefixes {
        if statement.starts_with(prefix) {
            return Some(
                prefix
                    .trim_end_matches('(')
                    .trim_end_matches('.')
                    .to_string(),
            );
        }
    }

    None
}

/// JSON 文字列から指定キーの数値を簡易抽出する。
fn extract_json_number(json: &str, key: &str) -> Option<i64> {
    let pattern = format!("\"{}\"", key);
    let pos = json.find(&pattern)?;
    let after_key = &json[pos + pattern.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();

    // 数値の開始位置から連続する数字を取得
    let num_str: String = after_colon
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    num_str.parse().ok()
}

/// JSON 文字列から指定キーの真偽値を簡易抽出する。
fn extract_json_bool(json: &str, key: &str) -> Option<bool> {
    let pattern = format!("\"{}\"", key);
    let pos = json.find(&pattern)?;
    let after_key = &json[pos + pattern.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();

    if after_colon.starts_with("true") {
        Some(true)
    } else if after_colon.starts_with("false") {
        Some(false)
    } else {
        None
    }
}
