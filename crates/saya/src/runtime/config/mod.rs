//! TypeScript 設定の評価を担当するモジュール。
//!
//! 設定ファイルの読み込み、限定 API での評価、設定コマンドの生成を行う。
//! Vim script を前提としない公開面を提供し、失敗時は default 設定へ fallback する。

use std::path::PathBuf;

use serde_json::Value as JsonValue;

use crate::presentation::theme::{
    FilerSemanticStyleKey, MarkdownSemanticStyleKey, SyntaxSemanticStyleKey,
    ThemeTextStyleDeclaration, UiStyleKey,
};
pub use crate::runtime::options::{SayaOptionName, SayaOptionValue};

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
    MermaidPreview,
    MermaidPreviewBackground,
    MermaidPreviewHeight,
    MermaidPreviewWidth,
    MessageHeight,
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

impl IntoIterator for StartupRegistry {
    type Item = StartupRegistryEntry;
    type IntoIter = std::vec::IntoIter<StartupRegistryEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
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
    FtPlugin {
        action: FtPluginStartupAction,
    },
    StatusLine {
        config: StatusLineConfig,
    },
    ThemePalette {
        name: String,
        value: String,
    },
    ThemeMarkdownStyle {
        key: MarkdownSemanticStyleKey,
        style: ThemeTextStyleDeclaration,
    },
    ThemeUiStyle {
        key: UiStyleKey,
        style: ThemeTextStyleDeclaration,
    },
    ThemeSyntaxStyle {
        key: SyntaxSemanticStyleKey,
        style: ThemeTextStyleDeclaration,
    },
    ThemeLanguageSyntaxStyle {
        language: String,
        key: SyntaxSemanticStyleKey,
        style: ThemeTextStyleDeclaration,
    },
    ThemeFilerStyle {
        key: FilerSemanticStyleKey,
        style: ThemeTextStyleDeclaration,
    },
    LogFile {
        path: String,
    },
    LogLevel {
        level: log::LevelFilter,
    },
    PluginUse {
        declaration: StartupPluginDeclaration,
    },
    PluginLazy {
        declaration: StartupPluginDeclaration,
    },
    Warning {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FtPluginStartupAction {
    SetEnabled(bool),
    SetDefinition(FtPluginDefinition),
    DisableFileType { filetype: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtPluginConfig {
    pub enabled: bool,
    pub definitions: Vec<FtPluginDefinition>,
}

impl Default for FtPluginConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            definitions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtPluginDefinition {
    pub filetype: String,
    pub extensions: Vec<String>,
    pub options: Vec<FtPluginOption>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtPluginOption {
    pub name: SayaOptionName,
    pub value: SayaOptionValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLineConfig {
    pub left: Vec<StatusLineSegment>,
    pub right: Vec<StatusLineSegment>,
}

impl Default for StatusLineConfig {
    fn default() -> Self {
        Self {
            left: vec![
                StatusLineSegment::FileName,
                StatusLineSegment::Mode,
                StatusLineSegment::FileType,
                StatusLineSegment::Modified,
            ],
            right: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLineSegment {
    FileName,
    Mode,
    FileType,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupPluginDeclaration {
    pub name: String,
    pub source: StartupPluginSource,
    pub module: String,
    pub setup: String,
    pub commands: Vec<String>,
    pub events: Vec<String>,
    pub options: Option<JsonValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupPluginSource {
    Local { path: String },
    Github { repo: String, rev: Option<String> },
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
            SayaOptionName::MermaidPreview => Self::MermaidPreview,
            SayaOptionName::MermaidPreviewBackground => Self::MermaidPreviewBackground,
            SayaOptionName::MermaidPreviewHeight => Self::MermaidPreviewHeight,
            SayaOptionName::MermaidPreviewWidth => Self::MermaidPreviewWidth,
            SayaOptionName::MessageHeight => Self::MessageHeight,
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

mod apply;
mod parser;
mod source;

pub use apply::{
    AppliedKeyMapping, ConfigApplyError, ConfigApplyResult, ConfigApplyState, apply_config_commands,
};
pub use parser::{evaluate_capability_source, evaluate_config};
pub use source::{ConfigSourceResult, read_config_source};

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
                "Failed to read config; using defaults ({}): {}",
                path.display(),
                message
            );
            log::debug!("[config_runtime] read failure warning: {}", warning);
            warnings.push(warning);
        }
        ConfigLoadResult::EvalFailed { path, message } => {
            let warning = format!(
                "Failed to evaluate config; using defaults ({}): {}",
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
        std::fs::write(&config_path, "{ \"tabstop\": 4 }").expect("write config");

        let result = read_config_source(&ConfigInput::FilePath(config_path.clone()));

        match result {
            ConfigSourceResult::Loaded { path, source } => {
                assert_eq!(path, config_path);
                assert_eq!(source, "{ \"tabstop\": 4 }");
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
    fn evaluate_config_parses_tabstop_option() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("test.json"),
            source: "{ \"tabstop\": 4 }".to_string(),
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
                    "tabstop オプションが正しくパースされること"
                );
            }
            other => panic!("Success を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_config_parses_number_option() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("test.json"),
            source: "{ \"number\": true }".to_string(),
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
    fn evaluate_config_parses_numberwidth_option() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("test.json"),
            source: "{ \"numberwidth\": 6 }".to_string(),
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
            source: "{ \"tabstop\": 2, \"number\": false }".to_string(),
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
                saya.options.tabstop = 4;
                saya.options.number = true;
                saya.options.numberwidth = 6;
                saya.options.cmdheight = 3;
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
                    4,
                    "startup option は 4 件の command に正規化されること"
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
                        StartupRegistryEntry::Option {
                            name: SayaOptionName::MessageHeight,
                            value: SayaOptionValue::Number(3),
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
            source: "saya.options.tabstop = 6;".to_string(),
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

        assert_eq!(state.tab_size, 4, "tabstop が 4 に変更されること");
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

        assert!(state.line_numbers, "number が true に変更されること");
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

        assert_eq!(state.number_width, 6, "numberwidth が 6 に変更されること");
        assert!(result.is_fully_applied());
    }

    #[test]
    fn apply_message_height_command_updates_state() {
        let commands = vec![ConfigCommand::SetOption {
            name: ConfigOptionName::MessageHeight,
            value: ConfigOptionValue::Number(3),
        }];
        let mut state = ConfigApplyState::default_state();
        assert_eq!(state.message_height, 5, "既定値は 5 であること");

        let result = apply_config_commands(&commands, &mut state);

        assert_eq!(state.message_height, 3, "cmdheight が 3 に変更されること");
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
            warnings[0].contains("Failed to read config"),
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
            warnings[0].contains("Failed to evaluate config"),
            "評価失敗の warning メッセージ: {}",
            warnings[0]
        );

        std::fs::remove_file(vim_config).expect("cleanup");
    }

    #[test]
    fn load_and_apply_with_valid_config_applies_successfully() {
        let config_path = unique_path("config-valid");
        std::fs::write(&config_path, "{ \"tabstop\": 4, \"number\": true }").expect("write config");

        let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(config_path.clone()));

        assert_eq!(state.tab_size, 4, "tabstop が設定値に変更されること");
        assert!(state.line_numbers, "number が設定値に変更されること");
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
