//! TypeScript 設定の評価を担当するモジュール。
//!
//! 設定ファイルの読み込み、限定 API での評価、設定コマンドの生成を行う。
//! Vim script を前提としない公開面を提供し、失敗時は default 設定へ fallback する。

use std::path::PathBuf;

pub use crate::option_registry::{SayaOptionName, SayaOptionValue};
use crate::theme::{MarkdownSemanticStyleKey, ThemeTextStyleDeclaration};

/// 設定評価から得られるコマンド。
///
/// TypeScript 設定から得る不変コマンドで、session 初期化前に確定する。
/// 適用後は immutable log として扱う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigCommand {
    /// オプション値の設定
    SetOption {
        name: ConfigOptionName,
        value: ConfigOptionValue,
    },
    /// キーマッピングの設定
    MapKey {
        mode: ConfigKeyMode,
        lhs: String,
        rhs: String,
    },
}

/// 設定可能なオプション名（限定 API）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigOptionName {
    AutoIndent,
    CursorLine,
    ExpandTab,
    FoldLevel,
    FoldMethod,
    IgnoreCase,
    LastStatus,
    List,
    ListChars,
    RelativeNumber,
    ScrollOff,
    ShiftWidth,
    SidescrollOff,
    SmartCase,
    SmartIndent,
    SoftTabStop,
    Syntax,
    TabSize,
    LineNumbers,
    NumberWidth,
    Wrap,
}

/// オプション値の型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigOptionValue {
    Number(i64),
    Boolean(bool),
    String(String),
}

/// キーマッピング対象のモード。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigKeyMode {
    Normal,
    Insert,
}

/// TypeScript capability runtime の評価結果。
///
/// startup registry と旧来の config apply 用 command を同時に保持し、
/// 既存の boot 経路へは command のみを流し込めるようにする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityLoadResult {
    /// 既定値を使用
    DefaultUsed,
    /// 評価成功
    Success {
        path: PathBuf,
        registry: StartupRegistry,
        commands: Vec<ConfigCommand>,
    },
    /// 読み込み失敗
    ReadFailed { path: PathBuf, message: String },
    /// 評価失敗
    EvalFailed { path: PathBuf, message: String },
    /// startup phase で許可しない capability を検出
    UnsupportedCapability {
        path: PathBuf,
        capability: String,
        message: String,
    },
}

/// startup phase の正規化済み registry。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartupRegistry {
    entries: Vec<StartupRegistryEntry>,
}

impl StartupRegistry {
    /// registry の登録順をそのまま返す。
    pub fn entries(&self) -> &[StartupRegistryEntry] {
        &self.entries
    }

    /// entry 一覧から registry を構築する。
    pub fn from_entries(entries: Vec<StartupRegistryEntry>) -> Self {
        log::debug!(
            "[config_runtime] create startup registry from entries: entry_count={}",
            entries.len()
        );
        Self { entries }
    }

    pub(crate) fn push(&mut self, entry: StartupRegistryEntry) {
        self.entries.push(entry);
    }
}

/// startup phase で記録する capability entry。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupRegistryEntry {
    Option {
        name: SayaOptionName,
        value: SayaOptionValue,
    },
    Keymap {
        mode: SayaKeyMode,
        lhs: String,
        action: SayaKeymapAction,
    },
    Command {
        name: String,
        callback_source: String,
    },
    Event {
        name: String,
        callback_source: String,
    },
    ThemePalette {
        name: String,
        value: String,
    },
    ThemeMarkdownStyle {
        key: MarkdownSemanticStyleKey,
        style: ThemeTextStyleDeclaration,
    },
}

/// startup keymap のモード。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SayaKeyMode {
    Normal,
    Insert,
    Visual,
}

/// startup keymap の action。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SayaKeymapAction {
    Literal(String),
    RegisteredCommand(String),
}

impl From<SayaOptionName> for ConfigOptionName {
    fn from(value: SayaOptionName) -> Self {
        match value {
            SayaOptionName::AutoIndent => Self::AutoIndent,
            SayaOptionName::CursorLine => Self::CursorLine,
            SayaOptionName::ExpandTab => Self::ExpandTab,
            SayaOptionName::FoldLevel => Self::FoldLevel,
            SayaOptionName::FoldMethod => Self::FoldMethod,
            SayaOptionName::IgnoreCase => Self::IgnoreCase,
            SayaOptionName::LastStatus => Self::LastStatus,
            SayaOptionName::List => Self::List,
            SayaOptionName::ListChars => Self::ListChars,
            SayaOptionName::RelativeNumber => Self::RelativeNumber,
            SayaOptionName::ScrollOff => Self::ScrollOff,
            SayaOptionName::ShiftWidth => Self::ShiftWidth,
            SayaOptionName::SidescrollOff => Self::SidescrollOff,
            SayaOptionName::SmartCase => Self::SmartCase,
            SayaOptionName::SmartIndent => Self::SmartIndent,
            SayaOptionName::SoftTabStop => Self::SoftTabStop,
            SayaOptionName::Syntax => Self::Syntax,
            SayaOptionName::TabSize => Self::TabSize,
            SayaOptionName::LineNumbers => Self::LineNumbers,
            SayaOptionName::NumberWidth => Self::NumberWidth,
            SayaOptionName::Wrap => Self::Wrap,
            SayaOptionName::Backup
            | SayaOptionName::Clipboard
            | SayaOptionName::FileEncoding
            | SayaOptionName::FileFormat
            | SayaOptionName::MarkdownRender
            | SayaOptionName::Undofile
            | SayaOptionName::WriteBackup => {
                unreachable!(
                    "non-startup-public options must not be converted into startup config commands"
                )
            }
        }
    }
}

impl From<SayaOptionValue> for ConfigOptionValue {
    fn from(value: SayaOptionValue) -> Self {
        match value {
            SayaOptionValue::Number(number) => Self::Number(number),
            SayaOptionValue::Boolean(boolean) => Self::Boolean(boolean),
            SayaOptionValue::String(value) => Self::String(value),
        }
    }
}

impl From<SayaKeyMode> for ConfigKeyMode {
    fn from(value: SayaKeyMode) -> Self {
        match value {
            SayaKeyMode::Normal => Self::Normal,
            SayaKeyMode::Insert => Self::Insert,
            SayaKeyMode::Visual => Self::Normal,
        }
    }
}

/// 設定読み込みの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigLoadResult {
    /// 設定コマンドの適用成功
    Success { commands: Vec<ConfigCommand> },
    /// 設定なし（既定値使用）
    DefaultUsed,
    /// 読み込み失敗（warning 付きで継続）
    ReadFailed { path: PathBuf, message: String },
    /// 評価失敗（warning 付きで継続）
    EvalFailed { path: PathBuf, message: String },
}

/// 設定入力のソースを表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigInput {
    /// 設定なし（既定値を使用）
    None,
    /// ファイルパスから読み込む
    FilePath(PathBuf),
}

/// 設定ファイルを読み取り、文字列入力として取得する。
///
/// 起動時に設定ファイルを見つけ、文字列入力として取得する。
/// 設定未指定時は既定値扱いにする。
pub fn read_config_source(input: &ConfigInput) -> ConfigSourceResult {
    log::debug!("[config_runtime] reading config source: {:?}", input);
    match input {
        ConfigInput::None => {
            log::debug!("[config_runtime] no config input, using defaults");
            ConfigSourceResult::Default
        }
        ConfigInput::FilePath(path) => {
            log::debug!("[config_runtime] reading config file: {}", path.display());
            match std::fs::read_to_string(path) {
                Ok(source) => {
                    log::debug!(
                        "[config_runtime] config file read success: path={}, len={}",
                        path.display(),
                        source.len()
                    );
                    ConfigSourceResult::Loaded {
                        path: path.clone(),
                        source,
                    }
                }
                Err(error) => {
                    log::debug!(
                        "[config_runtime] config file read failed: path={}, error={}",
                        path.display(),
                        error
                    );
                    ConfigSourceResult::ReadFailed {
                        path: path.clone(),
                        message: error.to_string(),
                    }
                }
            }
        }
    }
}

/// 設定ファイル読み取りの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSourceResult {
    /// 既定値を使用
    Default,
    /// ファイルから読み込み成功
    Loaded { path: PathBuf, source: String },
    /// ファイル読み込み失敗
    ReadFailed { path: PathBuf, message: String },
}

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

    // "tabSize": <number> を検出
    if let Some(value) = extract_json_number(trimmed, "tabSize") {
        log::debug!("[config_runtime] found tabSize option: {}", value);
        commands.push(ConfigCommand::SetOption {
            name: ConfigOptionName::TabSize,
            value: ConfigOptionValue::Number(value),
        });
    }

    // "lineNumbers": <bool> を検出
    if let Some(value) = extract_json_bool(trimmed, "lineNumbers") {
        log::debug!("[config_runtime] found lineNumbers option: {}", value);
        commands.push(ConfigCommand::SetOption {
            name: ConfigOptionName::LineNumbers,
            value: ConfigOptionValue::Boolean(value),
        });
    }

    // "numberWidth": <number> を検出
    if let Some(value) = extract_json_number(trimmed, "numberWidth") {
        log::debug!("[config_runtime] found numberWidth option: {}", value);
        commands.push(ConfigCommand::SetOption {
            name: ConfigOptionName::NumberWidth,
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

    let definition = crate::option_registry::SayaOptionRegistry::resolve(lhs.trim())
        .expect("normalized startup option should resolve");
    let value = match definition.value_type {
        crate::option_registry::SayaOptionType::Boolean => match rhs {
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
        crate::option_registry::SayaOptionType::Number => rhs
            .parse::<i64>()
            .map(SayaOptionValue::Number)
            .map_err(|_| {
                CapabilityParseError::EvalFailed(format!(
                    "option {} の値が不正です: {}",
                    lhs.trim(),
                    rhs
                ))
            })?,
        crate::option_registry::SayaOptionType::String => {
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
    crate::option_registry::SayaOptionRegistry::resolve(value)
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

/// 設定コマンドを起動時の editor 状態へ適用する。
///
/// 初期 session 準備後、描画開始前に設定を反映する。
/// 設定適用順が毎回ぶれないように固定する。
pub fn apply_config_commands(
    commands: &[ConfigCommand],
    state: &mut ConfigApplyState,
) -> ConfigApplyResult {
    log::debug!(
        "[config_runtime] applying {} config commands to editor state",
        commands.len()
    );

    let mut applied_count = 0;
    let mut errors = Vec::new();

    for (index, command) in commands.iter().enumerate() {
        log::debug!(
            "[config_runtime] applying command {}/{}: {:?}",
            index + 1,
            commands.len(),
            command
        );
        match apply_single_command(command, state) {
            Ok(()) => {
                applied_count += 1;
                log::debug!(
                    "[config_runtime] command {}/{} applied successfully",
                    index + 1,
                    commands.len()
                );
            }
            Err(message) => {
                log::debug!(
                    "[config_runtime] command {}/{} failed: {}",
                    index + 1,
                    commands.len(),
                    message
                );
                errors.push(ConfigApplyError {
                    command_index: index,
                    message,
                });
            }
        }
    }

    let result = ConfigApplyResult {
        applied_count,
        errors,
    };
    log::debug!(
        "[config_runtime] config apply complete: applied={}, errors={}",
        result.applied_count,
        result.errors.len()
    );
    result
}

/// 設定適用先の状態。editor session の設定可能な部分を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigApplyState {
    pub tab_size: i64,
    pub expandtab: bool,
    pub shiftwidth: i64,
    pub softtabstop: i64,
    pub autoindent: bool,
    pub smartindent: bool,
    pub ignorecase: bool,
    pub smartcase: bool,
    pub syntax: bool,
    pub scrolloff: i64,
    pub sidescrolloff: i64,
    pub wrap: bool,
    pub line_numbers: bool,
    pub relative_number: bool,
    pub cursorline: bool,
    pub number_width: i64,
    pub laststatus: i64,
    pub list: bool,
    pub listchars: String,
    pub foldmethod: String,
    pub foldlevel: i64,
    pub key_mappings: Vec<AppliedKeyMapping>,
}

/// 適用済みキーマッピング。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedKeyMapping {
    pub mode: ConfigKeyMode,
    pub lhs: String,
    pub rhs: String,
}

impl ConfigApplyState {
    /// 既定値で初期化する。
    pub fn default_state() -> Self {
        log::debug!("[config_runtime] creating default config apply state");
        Self {
            tab_size: 8,
            expandtab: false,
            shiftwidth: 8,
            softtabstop: 0,
            autoindent: false,
            smartindent: false,
            ignorecase: false,
            smartcase: false,
            syntax: false,
            scrolloff: 0,
            sidescrolloff: 0,
            wrap: true,
            line_numbers: false,
            relative_number: false,
            cursorline: false,
            number_width: 4,
            laststatus: 2,
            list: false,
            listchars: "tab:>-,trail:-".to_string(),
            foldmethod: "manual".to_string(),
            foldlevel: 0,
            key_mappings: Vec::new(),
        }
    }
}

/// 設定適用の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigApplyResult {
    pub applied_count: usize,
    pub errors: Vec<ConfigApplyError>,
}

impl ConfigApplyResult {
    pub fn is_fully_applied(&self) -> bool {
        self.errors.is_empty()
    }
}

/// 個別の設定適用エラー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigApplyError {
    pub command_index: usize,
    pub message: String,
}

/// 個別のコマンドを適用する。
fn apply_single_command(
    command: &ConfigCommand,
    state: &mut ConfigApplyState,
) -> Result<(), String> {
    match command {
        ConfigCommand::SetOption { name, value } => match (name, value) {
            (ConfigOptionName::ExpandTab, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime] setting expandtab: {} -> {}",
                    state.expandtab,
                    b
                );
                state.expandtab = *b;
                Ok(())
            }
            (ConfigOptionName::ShiftWidth, ConfigOptionValue::Number(n)) => {
                validate_number_range("shiftwidth", *n, 0, 32)?;
                log::debug!(
                    "[config_runtime] setting shiftwidth: {} -> {}",
                    state.shiftwidth,
                    n
                );
                state.shiftwidth = *n;
                Ok(())
            }
            (ConfigOptionName::SoftTabStop, ConfigOptionValue::Number(n)) => {
                validate_number_range("softtabstop", *n, -1, 32)?;
                log::debug!(
                    "[config_runtime] setting softtabstop: {} -> {}",
                    state.softtabstop,
                    n
                );
                state.softtabstop = *n;
                Ok(())
            }
            (ConfigOptionName::AutoIndent, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime] setting autoindent: {} -> {}",
                    state.autoindent,
                    b
                );
                state.autoindent = *b;
                Ok(())
            }
            (ConfigOptionName::SmartIndent, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime] setting smartindent: {} -> {}",
                    state.smartindent,
                    b
                );
                state.smartindent = *b;
                Ok(())
            }
            (ConfigOptionName::IgnoreCase, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime] setting ignorecase: {} -> {}",
                    state.ignorecase,
                    b
                );
                state.ignorecase = *b;
                Ok(())
            }
            (ConfigOptionName::SmartCase, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime] setting smartcase: {} -> {}",
                    state.smartcase,
                    b
                );
                state.smartcase = *b;
                Ok(())
            }
            (ConfigOptionName::Syntax, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime] setting syntax startup core command flag: {} -> {}",
                    state.syntax,
                    b
                );
                state.syntax = *b;
                Ok(())
            }
            (ConfigOptionName::ScrollOff, ConfigOptionValue::Number(n)) => {
                validate_number_range("scrolloff", *n, 0, 999)?;
                log::debug!(
                    "[config_runtime] setting scrolloff: {} -> {}",
                    state.scrolloff,
                    n
                );
                state.scrolloff = *n;
                Ok(())
            }
            (ConfigOptionName::SidescrollOff, ConfigOptionValue::Number(n)) => {
                validate_number_range("sidescrolloff", *n, 0, 999)?;
                log::debug!(
                    "[config_runtime] setting sidescrolloff: {} -> {}",
                    state.sidescrolloff,
                    n
                );
                state.sidescrolloff = *n;
                Ok(())
            }
            (ConfigOptionName::Wrap, ConfigOptionValue::Boolean(b)) => {
                log::debug!("[config_runtime] setting wrap: {} -> {}", state.wrap, b);
                state.wrap = *b;
                Ok(())
            }
            (ConfigOptionName::TabSize, ConfigOptionValue::Number(n)) => {
                if *n < 1 || *n > 32 {
                    return Err(format!(
                        "tabSize の値は 1〜32 の範囲で指定してください: {}",
                        n
                    ));
                }
                log::debug!(
                    "[config_runtime] setting tabSize: {} -> {}",
                    state.tab_size,
                    n
                );
                state.tab_size = *n;
                Ok(())
            }
            (ConfigOptionName::LineNumbers, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime] setting lineNumbers: {} -> {}",
                    state.line_numbers,
                    b
                );
                state.line_numbers = *b;
                Ok(())
            }
            (ConfigOptionName::RelativeNumber, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime] setting relativenumber: {} -> {}",
                    state.relative_number,
                    b
                );
                state.relative_number = *b;
                Ok(())
            }
            (ConfigOptionName::CursorLine, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime] setting cursorline: {} -> {}",
                    state.cursorline,
                    b
                );
                state.cursorline = *b;
                Ok(())
            }
            (ConfigOptionName::NumberWidth, ConfigOptionValue::Number(n)) => {
                if *n < 1 || *n > 32 {
                    return Err(format!(
                        "numberWidth の値は 1〜32 の範囲で指定してください: {}",
                        n
                    ));
                }
                log::debug!(
                    "[config_runtime] setting numberWidth: {} -> {}",
                    state.number_width,
                    n
                );
                state.number_width = *n;
                Ok(())
            }
            (ConfigOptionName::LastStatus, ConfigOptionValue::Number(n)) => {
                validate_number_range("laststatus", *n, 0, 3)?;
                log::debug!(
                    "[config_runtime] setting laststatus: {} -> {}",
                    state.laststatus,
                    n
                );
                state.laststatus = *n;
                Ok(())
            }
            (ConfigOptionName::List, ConfigOptionValue::Boolean(b)) => {
                log::debug!("[config_runtime] setting list: {} -> {}", state.list, b);
                state.list = *b;
                Ok(())
            }
            (ConfigOptionName::ListChars, ConfigOptionValue::String(s)) => {
                log::debug!(
                    "[config_runtime] setting listchars: {:?} -> {:?}",
                    state.listchars,
                    s
                );
                state.listchars = s.clone();
                Ok(())
            }
            (ConfigOptionName::FoldMethod, ConfigOptionValue::String(s)) => {
                log::debug!(
                    "[config_runtime] setting foldmethod: {:?} -> {:?}",
                    state.foldmethod,
                    s
                );
                state.foldmethod = s.clone();
                Ok(())
            }
            (ConfigOptionName::FoldLevel, ConfigOptionValue::Number(n)) => {
                validate_number_range("foldlevel", *n, 0, 99)?;
                log::debug!(
                    "[config_runtime] setting foldlevel: {} -> {}",
                    state.foldlevel,
                    n
                );
                state.foldlevel = *n;
                Ok(())
            }
            (name, value) => Err(format!(
                "オプション {:?} に対して不正な値型 {:?} が指定されました",
                name, value
            )),
        },
        ConfigCommand::MapKey { mode, lhs, rhs } => {
            if lhs.is_empty() {
                return Err("キーマッピングの lhs が空です".to_string());
            }
            if rhs.is_empty() {
                return Err("キーマッピングの rhs が空です".to_string());
            }
            log::debug!(
                "[config_runtime] adding key mapping: mode={:?}, lhs={:?}, rhs={:?}",
                mode,
                lhs,
                rhs
            );
            state.key_mappings.push(AppliedKeyMapping {
                mode: mode.clone(),
                lhs: lhs.clone(),
                rhs: rhs.clone(),
            });
            Ok(())
        }
    }
}

fn validate_number_range(name: &str, value: i64, min: i64, max: i64) -> Result<(), String> {
    if value < min || value > max {
        return Err(format!(
            "{name} の値は {min}〜{max} の範囲で指定してください: {value}"
        ));
    }
    Ok(())
}

/// 設定の全フローを実行する統合関数。
///
/// 読み込み -> 評価 -> 適用 を一貫して実行し、
/// 失敗時は warning 付きで既定値を返す。
pub fn load_and_apply_config(input: &ConfigInput) -> (ConfigApplyState, Vec<String>) {
    log::debug!("[config_runtime] starting full config load-and-apply flow");
    let mut warnings = Vec::new();
    let mut state = ConfigApplyState::default_state();

    let source_result = read_config_source(input);
    let load_result = evaluate_config(&source_result);

    match load_result {
        ConfigLoadResult::Success { commands } => {
            let apply_result = apply_config_commands(&commands, &mut state);
            if !apply_result.is_fully_applied() {
                for error in &apply_result.errors {
                    let warning = format!(
                        "設定コマンド {} の適用に失敗しました: {}",
                        error.command_index + 1,
                        error.message
                    );
                    log::debug!("[config_runtime] apply warning: {}", warning);
                    warnings.push(warning);
                }
            }
            log::debug!(
                "[config_runtime] config applied: applied={}, warnings={}",
                apply_result.applied_count,
                warnings.len()
            );
        }
        ConfigLoadResult::DefaultUsed => {
            log::debug!("[config_runtime] using default config, no warnings");
        }
        ConfigLoadResult::ReadFailed { path, message } => {
            let warning = format!(
                "設定ファイルの読み込みに失敗したため既定値で起動します ({}): {}",
                path.display(),
                message
            );
            log::debug!("[config_runtime] read failure warning: {}", warning);
            warnings.push(warning);
        }
        ConfigLoadResult::EvalFailed { path, message } => {
            let warning = format!(
                "設定ファイルの評価に失敗したため既定値で起動します ({}): {}",
                path.display(),
                message
            );
            log::debug!("[config_runtime] eval failure warning: {}", warning);
            warnings.push(warning);
        }
    }

    log::debug!(
        "[config_runtime] load-and-apply complete: state={:?}, warnings={}",
        state,
        warnings.len()
    );
    (state, warnings)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn unique_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("saya-config-{name}-{nanos}"))
    }

    // ==== タスク 8.1: 限定された設定入力を読み取れるようにする ====

    #[test]
    fn read_config_returns_default_when_no_input() {
        let result = read_config_source(&ConfigInput::None);

        assert_eq!(
            result,
            ConfigSourceResult::Default,
            "設定未指定時は既定値扱いになること"
        );
    }

    #[test]
    fn read_config_returns_loaded_when_file_exists() {
        let config_path = unique_path("config-exists");
        std::fs::write(&config_path, "{ \"tabSize\": 4 }").expect("write config");

        let result = read_config_source(&ConfigInput::FilePath(config_path.clone()));

        match result {
            ConfigSourceResult::Loaded { path, source } => {
                assert_eq!(path, config_path);
                assert_eq!(source, "{ \"tabSize\": 4 }");
            }
            other => panic!("既存ファイルは Loaded を返すこと, got: {:?}", other),
        }

        std::fs::remove_file(config_path).expect("cleanup");
    }

    #[test]
    fn read_config_returns_read_failed_when_file_missing() {
        let missing_path = unique_path("config-missing");

        let result = read_config_source(&ConfigInput::FilePath(missing_path.clone()));

        match result {
            ConfigSourceResult::ReadFailed { path, message } => {
                assert_eq!(path, missing_path);
                assert!(!message.is_empty(), "読み込み失敗メッセージは空でないこと");
            }
            other => panic!(
                "存在しないファイルは ReadFailed を返すこと, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn read_config_treats_unspecified_as_default() {
        // 設定未指定は既定値として扱い、エラーにならないこと
        let result = read_config_source(&ConfigInput::None);
        assert_eq!(result, ConfigSourceResult::Default);

        let load_result = evaluate_config(&result);
        assert_eq!(
            load_result,
            ConfigLoadResult::DefaultUsed,
            "設定未指定は DefaultUsed になること"
        );
    }

    // ==== タスク 8.2: 限定 API だけを使って設定を評価できるようにする ====

    #[test]
    fn evaluate_config_parses_tab_size_option() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("test.json"),
            source: "{ \"tabSize\": 4 }".to_string(),
        };

        let result = evaluate_config(&source);

        match result {
            ConfigLoadResult::Success { commands } => {
                assert_eq!(commands.len(), 1);
                assert_eq!(
                    commands[0],
                    ConfigCommand::SetOption {
                        name: ConfigOptionName::TabSize,
                        value: ConfigOptionValue::Number(4),
                    },
                    "tabSize オプションが正しくパースされること"
                );
            }
            other => panic!("Success を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_config_parses_line_numbers_option() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("test.json"),
            source: "{ \"lineNumbers\": true }".to_string(),
        };

        let result = evaluate_config(&source);

        match result {
            ConfigLoadResult::Success { commands } => {
                assert_eq!(commands.len(), 1);
                assert_eq!(
                    commands[0],
                    ConfigCommand::SetOption {
                        name: ConfigOptionName::LineNumbers,
                        value: ConfigOptionValue::Boolean(true),
                    }
                );
            }
            other => panic!("Success を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_config_parses_number_width_option() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("test.json"),
            source: "{ \"numberWidth\": 6 }".to_string(),
        };

        let result = evaluate_config(&source);

        match result {
            ConfigLoadResult::Success { commands } => {
                assert_eq!(commands.len(), 1);
                assert_eq!(
                    commands[0],
                    ConfigCommand::SetOption {
                        name: ConfigOptionName::NumberWidth,
                        value: ConfigOptionValue::Number(6),
                    }
                );
            }
            other => panic!("Success を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_config_parses_multiple_options() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("test.json"),
            source: "{ \"tabSize\": 2, \"lineNumbers\": false }".to_string(),
        };

        let result = evaluate_config(&source);

        match result {
            ConfigLoadResult::Success { commands } => {
                assert_eq!(commands.len(), 2, "複数オプションが全てパースされること");
            }
            other => panic!("Success を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_config_rejects_vim_script_syntax() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("init.vim"),
            source: "set tabstop=4\nset number\n".to_string(),
        };

        let result = evaluate_config(&source);

        match result {
            ConfigLoadResult::EvalFailed { path, message } => {
                assert_eq!(path, PathBuf::from("init.vim"));
                assert!(
                    message.contains("Vim script"),
                    "Vim script 拒否メッセージを含むこと: {}",
                    message
                );
            }
            other => panic!("Vim script は EvalFailed を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_config_rejects_noremap_vim_script() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("config.vim"),
            source: "nnoremap <leader>f :Files<CR>".to_string(),
        };

        let result = evaluate_config(&source);

        assert!(
            matches!(result, ConfigLoadResult::EvalFailed { .. }),
            "noremap 構文は拒否されること"
        );
    }

    #[test]
    fn evaluate_config_returns_empty_commands_for_empty_config() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("empty.json"),
            source: "{}".to_string(),
        };

        let result = evaluate_config(&source);

        match result {
            ConfigLoadResult::Success { commands } => {
                assert!(
                    commands.is_empty(),
                    "空の設定は空のコマンドリストを返すこと"
                );
            }
            other => panic!("Success を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_config_returns_default_used_for_no_config() {
        let source = ConfigSourceResult::Default;

        let result = evaluate_config(&source);

        assert_eq!(
            result,
            ConfigLoadResult::DefaultUsed,
            "設定なしは DefaultUsed を返すこと"
        );
    }

    #[test]
    fn evaluate_config_propagates_read_failure() {
        let source = ConfigSourceResult::ReadFailed {
            path: PathBuf::from("missing.json"),
            message: "file not found".to_string(),
        };

        let result = evaluate_config(&source);

        assert_eq!(
            result,
            ConfigLoadResult::ReadFailed {
                path: PathBuf::from("missing.json"),
                message: "file not found".to_string(),
            },
            "読み込み失敗がそのまま伝播すること"
        );
    }

    // ==== タスク 1.x / 2.x / 5.x: TypeScript-first capability API ====

    #[test]
    fn evaluate_capability_source_parses_startup_registry_in_source_order() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("init.ts"),
            source: r#"
                saya.options.tabSize = 4;
                saya.options.lineNumbers = true;
                saya.options.numberWidth = 6;
                saya.keymap.set("normal", "x", "dd");
                saya.commands.register("writeCurrent", () => {
                    saya.commands.execute("write");
                });
                saya.events.on("bufferOpen", (payload) => {
                    console.log(payload);
                });
            "#
            .to_string(),
        };

        let result = evaluate_capability_source(&source);

        match result {
            CapabilityLoadResult::Success {
                registry, commands, ..
            } => {
                assert_eq!(
                    commands.len(),
                    3,
                    "startup option は 3 件の command に正規化されること"
                );
                assert_eq!(
                    registry.entries(),
                    &[
                        StartupRegistryEntry::Option {
                            name: SayaOptionName::TabSize,
                            value: SayaOptionValue::Number(4),
                        },
                        StartupRegistryEntry::Option {
                            name: SayaOptionName::LineNumbers,
                            value: SayaOptionValue::Boolean(true),
                        },
                        StartupRegistryEntry::Option {
                            name: SayaOptionName::NumberWidth,
                            value: SayaOptionValue::Number(6),
                        },
                        StartupRegistryEntry::Keymap {
                            mode: SayaKeyMode::Normal,
                            lhs: "x".to_string(),
                            action: SayaKeymapAction::Literal("dd".to_string()),
                        },
                        StartupRegistryEntry::Command {
                            name: "writeCurrent".to_string(),
                            callback_source: "saya.commands.execute(\"write\");".to_string(),
                        },
                        StartupRegistryEntry::Event {
                            name: "bufferOpen".to_string(),
                            callback_source: "console.log(payload);".to_string(),
                        },
                    ]
                );
            }
            other => panic!("Success を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_capability_source_rejects_runtime_only_surface_at_startup() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("init.ts"),
            source: r#"
                saya.commands.execute("write");
            "#
            .to_string(),
        };

        let result = evaluate_capability_source(&source);

        assert!(matches!(
            result,
            CapabilityLoadResult::UnsupportedCapability { .. }
        ));
    }

    #[test]
    fn evaluate_capability_source_normalizes_vim_aliases_to_formal_names() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("init.ts"),
            source: r#"
                saya.options.tabstop = 2;
                saya.options.number = true;
                saya.options.nuw = 5;
            "#
            .to_string(),
        };

        let result = evaluate_capability_source(&source);

        match result {
            CapabilityLoadResult::Success {
                registry, commands, ..
            } => {
                assert_eq!(
                    commands.len(),
                    3,
                    "alias option も既存 boot 経路向け command に正規化されること"
                );
                assert_eq!(
                    registry.entries(),
                    &[
                        StartupRegistryEntry::Option {
                            name: SayaOptionName::TabSize,
                            value: SayaOptionValue::Number(2),
                        },
                        StartupRegistryEntry::Option {
                            name: SayaOptionName::LineNumbers,
                            value: SayaOptionValue::Boolean(true),
                        },
                        StartupRegistryEntry::Option {
                            name: SayaOptionName::NumberWidth,
                            value: SayaOptionValue::Number(5),
                        },
                    ]
                );
            }
            other => panic!("Success を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_capability_source_rejects_filesystem_and_network_capabilities() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("init.ts"),
            source: r#"
                saya.filesystem.readText("/tmp/notes.txt");
            "#
            .to_string(),
        };

        let result = evaluate_capability_source(&source);

        assert!(matches!(
            result,
            CapabilityLoadResult::UnsupportedCapability { .. }
        ));
    }

    #[test]
    fn evaluate_config_uses_typescript_capability_source_for_existing_boot_path() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("init.ts"),
            source: "saya.options.tabSize = 6;".to_string(),
        };

        let result = evaluate_config(&source);

        assert_eq!(
            result,
            ConfigLoadResult::Success {
                commands: vec![ConfigCommand::SetOption {
                    name: ConfigOptionName::TabSize,
                    value: ConfigOptionValue::Number(6),
                }],
            }
        );
    }

    // ==== タスク 8.3: 設定コマンドを起動時の editor 状態へ適用する ====

    #[test]
    fn apply_tab_size_command_updates_state() {
        let commands = vec![ConfigCommand::SetOption {
            name: ConfigOptionName::TabSize,
            value: ConfigOptionValue::Number(4),
        }];
        let mut state = ConfigApplyState::default_state();
        assert_eq!(state.tab_size, 8, "既定値は 8 であること");

        let result = apply_config_commands(&commands, &mut state);

        assert_eq!(state.tab_size, 4, "tabSize が 4 に変更されること");
        assert!(result.is_fully_applied());
        assert_eq!(result.applied_count, 1);
    }

    #[test]
    fn apply_line_numbers_command_updates_state() {
        let commands = vec![ConfigCommand::SetOption {
            name: ConfigOptionName::LineNumbers,
            value: ConfigOptionValue::Boolean(true),
        }];
        let mut state = ConfigApplyState::default_state();
        assert!(!state.line_numbers, "既定値は false であること");

        let result = apply_config_commands(&commands, &mut state);

        assert!(state.line_numbers, "lineNumbers が true に変更されること");
        assert!(result.is_fully_applied());
    }

    #[test]
    fn apply_number_width_command_updates_state() {
        let commands = vec![ConfigCommand::SetOption {
            name: ConfigOptionName::NumberWidth,
            value: ConfigOptionValue::Number(6),
        }];
        let mut state = ConfigApplyState::default_state();
        assert_eq!(state.number_width, 4, "既定値は 4 であること");

        let result = apply_config_commands(&commands, &mut state);

        assert_eq!(state.number_width, 6, "numberWidth が 6 に変更されること");
        assert!(result.is_fully_applied());
    }

    #[test]
    fn apply_key_mapping_command_adds_to_state() {
        let commands = vec![ConfigCommand::MapKey {
            mode: ConfigKeyMode::Normal,
            lhs: "<leader>f".to_string(),
            rhs: ":find ".to_string(),
        }];
        let mut state = ConfigApplyState::default_state();

        let result = apply_config_commands(&commands, &mut state);

        assert_eq!(state.key_mappings.len(), 1);
        assert_eq!(state.key_mappings[0].lhs, "<leader>f");
        assert_eq!(state.key_mappings[0].rhs, ":find ");
        assert_eq!(state.key_mappings[0].mode, ConfigKeyMode::Normal);
        assert!(result.is_fully_applied());
    }

    #[test]
    fn apply_commands_in_deterministic_order() {
        let commands = vec![
            ConfigCommand::SetOption {
                name: ConfigOptionName::TabSize,
                value: ConfigOptionValue::Number(2),
            },
            ConfigCommand::SetOption {
                name: ConfigOptionName::LineNumbers,
                value: ConfigOptionValue::Boolean(true),
            },
            ConfigCommand::MapKey {
                mode: ConfigKeyMode::Insert,
                lhs: "jk".to_string(),
                rhs: "\x1b".to_string(),
            },
        ];
        let mut state = ConfigApplyState::default_state();

        let result = apply_config_commands(&commands, &mut state);

        // 適用順が固定されていること
        assert_eq!(state.tab_size, 2);
        assert!(state.line_numbers);
        assert_eq!(state.key_mappings.len(), 1);
        assert_eq!(result.applied_count, 3);
        assert!(result.is_fully_applied());
    }

    #[test]
    fn apply_rejects_invalid_tab_size() {
        let commands = vec![ConfigCommand::SetOption {
            name: ConfigOptionName::TabSize,
            value: ConfigOptionValue::Number(0),
        }];
        let mut state = ConfigApplyState::default_state();

        let result = apply_config_commands(&commands, &mut state);

        assert!(!result.is_fully_applied());
        assert_eq!(result.errors.len(), 1);
        assert_eq!(state.tab_size, 8, "不正な値の場合は既定値が維持されること");
    }

    #[test]
    fn apply_rejects_type_mismatch() {
        let commands = vec![ConfigCommand::SetOption {
            name: ConfigOptionName::TabSize,
            value: ConfigOptionValue::Boolean(true),
        }];
        let mut state = ConfigApplyState::default_state();

        let result = apply_config_commands(&commands, &mut state);

        assert!(!result.is_fully_applied());
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn apply_empty_commands_is_noop() {
        let commands: Vec<ConfigCommand> = Vec::new();
        let mut state = ConfigApplyState::default_state();

        let result = apply_config_commands(&commands, &mut state);

        assert!(result.is_fully_applied());
        assert_eq!(result.applied_count, 0);
        assert_eq!(state.tab_size, 8, "空コマンドでは状態が変わらないこと");
    }

    // ==== タスク 8.4: 設定失敗時に warning 付きで起動継続できるようにする ====

    #[test]
    fn load_and_apply_with_no_config_returns_defaults_and_no_warnings() {
        let (state, warnings) = load_and_apply_config(&ConfigInput::None);

        assert_eq!(state.tab_size, 8);
        assert!(!state.line_numbers);
        assert!(state.key_mappings.is_empty());
        assert!(
            warnings.is_empty(),
            "設定なしの場合は warning なしで起動すること"
        );
    }

    #[test]
    fn load_and_apply_with_missing_file_returns_defaults_with_warning() {
        let missing_path = unique_path("config-load-missing");

        let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(missing_path));

        assert_eq!(state.tab_size, 8, "読み込み失敗時は既定値で起動すること");
        assert_eq!(
            warnings.len(),
            1,
            "読み込み失敗時は warning が 1 つ出ること"
        );
        assert!(
            warnings[0].contains("読み込みに失敗"),
            "読み込み失敗の warning メッセージ: {}",
            warnings[0]
        );
    }

    #[test]
    fn load_and_apply_with_vim_script_returns_defaults_with_warning() {
        let vim_config = unique_path("config-vim");
        std::fs::write(&vim_config, "set tabstop=4\nset number\n").expect("write vim config");

        let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(vim_config.clone()));

        assert_eq!(
            state.tab_size, 8,
            "Vim script 設定は無視され既定値になること"
        );
        assert_eq!(warnings.len(), 1, "評価失敗時は warning が 1 つ出ること");
        assert!(
            warnings[0].contains("評価に失敗"),
            "評価失敗の warning メッセージ: {}",
            warnings[0]
        );

        std::fs::remove_file(vim_config).expect("cleanup");
    }

    #[test]
    fn load_and_apply_with_valid_config_applies_successfully() {
        let config_path = unique_path("config-valid");
        std::fs::write(&config_path, "{ \"tabSize\": 4, \"lineNumbers\": true }")
            .expect("write config");

        let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(config_path.clone()));

        assert_eq!(state.tab_size, 4, "tabSize が設定値に変更されること");
        assert!(state.line_numbers, "lineNumbers が設定値に変更されること");
        assert!(
            warnings.is_empty(),
            "有効な設定では warning なしで適用されること"
        );

        std::fs::remove_file(config_path).expect("cleanup");
    }

    #[test]
    fn load_and_apply_with_empty_config_uses_defaults_without_warning() {
        let config_path = unique_path("config-empty");
        std::fs::write(&config_path, "{}").expect("write empty config");

        let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(config_path.clone()));

        assert_eq!(state.tab_size, 8, "空設定は既定値のまま");
        assert!(warnings.is_empty(), "空設定は warning なし");

        std::fs::remove_file(config_path).expect("cleanup");
    }

    #[test]
    fn config_failure_does_not_block_editor_basic_workflow() {
        // 設定失敗後も既定値で完全に動作する状態が返ること
        let missing_path = unique_path("config-fail-workflow");

        let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(missing_path));

        // 既定値で editor が動作可能な状態であること
        assert_eq!(state.tab_size, 8);
        assert!(!state.line_numbers);
        assert!(state.key_mappings.is_empty());

        // warning は出ているが、state は完全に有効
        assert!(!warnings.is_empty());

        // 状態は clone 可能（session に渡せる）
        let _cloned = state.clone();
    }

    #[test]
    fn read_failure_and_eval_failure_share_common_fallback() {
        // 読み込み失敗と評価失敗の両方が同じ既定値 fallback を使うこと
        let missing_path = unique_path("config-read-fail");
        let (state_read_fail, _) = load_and_apply_config(&ConfigInput::FilePath(missing_path));

        let vim_config = unique_path("config-eval-fail");
        std::fs::write(&vim_config, "set number").expect("write vim config");
        let (state_eval_fail, _) =
            load_and_apply_config(&ConfigInput::FilePath(vim_config.clone()));

        // 両方とも同じ既定値状態であること
        assert_eq!(
            state_read_fail, state_eval_fail,
            "読み込み失敗と評価失敗で同じ既定値 fallback が使われること"
        );

        std::fs::remove_file(vim_config).expect("cleanup");
    }
}
