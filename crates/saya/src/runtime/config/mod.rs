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
    HlSearch,
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
            SayaOptionName::HlSearch => Self::HlSearch,
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

mod apply;
mod parser;
mod source;

pub use apply::{
    AppliedKeyMapping, ConfigApplyError, ConfigApplyResult, ConfigApplyState, apply_config_commands,
};
pub use parser::evaluate_capability_source;
pub use source::ConfigSourceResult;

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
