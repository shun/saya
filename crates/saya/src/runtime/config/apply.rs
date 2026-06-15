//! 設定コマンドの editor 状態への適用（適用層）。

use super::*;

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
    pub hlsearch: bool,
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
    pub message_height: i64,
    pub list: bool,
    pub listchars: String,
    pub mermaid_preview_auto: bool,
    pub mermaid_preview_background: String,
    pub mermaid_preview_width_percent: i64,
    pub mermaid_preview_height_percent: i64,
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
            hlsearch: false,
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
            message_height: 5,
            list: false,
            listchars: "tab:>-,trail:-".to_string(),
            mermaid_preview_auto: true,
            mermaid_preview_background: "transparent".to_string(),
            mermaid_preview_width_percent: 55,
            mermaid_preview_height_percent: 55,
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
            (ConfigOptionName::HlSearch, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime] setting hlsearch startup core command flag: {} -> {}",
                    state.hlsearch,
                    b
                );
                state.hlsearch = *b;
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
                        "tabstop の値は 1〜32 の範囲で指定してください: {}",
                        n
                    ));
                }
                log::debug!(
                    "[config_runtime] setting tabstop: {} -> {}",
                    state.tab_size,
                    n
                );
                state.tab_size = *n;
                Ok(())
            }
            (ConfigOptionName::LineNumbers, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime] setting number: {} -> {}",
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
                        "numberwidth の値は 1〜32 の範囲で指定してください: {}",
                        n
                    ));
                }
                log::debug!(
                    "[config_runtime] setting numberwidth: {} -> {}",
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
            (ConfigOptionName::MessageHeight, ConfigOptionValue::Number(n)) => {
                validate_number_range("cmdheight", *n, 1, 999)?;
                log::debug!(
                    "[config_runtime] setting cmdheight: {} -> {}",
                    state.message_height,
                    n
                );
                state.message_height = *n;
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
            (ConfigOptionName::MermaidPreview, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime][mermaid_preview] setting mermaidpreview: {} -> {}",
                    state.mermaid_preview_auto,
                    b
                );
                state.mermaid_preview_auto = *b;
                Ok(())
            }
            (ConfigOptionName::MermaidPreviewBackground, ConfigOptionValue::String(s)) => {
                let background = normalize_mermaid_preview_background(s);
                log::debug!(
                    "[config_runtime][mermaid_preview] setting mermaidpreviewbackground: {:?} -> {:?}",
                    state.mermaid_preview_background,
                    background
                );
                state.mermaid_preview_background = background;
                Ok(())
            }
            (ConfigOptionName::MermaidPreviewWidth, ConfigOptionValue::Number(n)) => {
                validate_number_range("mermaidpreviewwidth", *n, 1, 100)?;
                log::debug!(
                    "[config_runtime][mermaid_preview] setting mermaidpreviewwidth: {} -> {}",
                    state.mermaid_preview_width_percent,
                    n
                );
                state.mermaid_preview_width_percent = *n;
                Ok(())
            }
            (ConfigOptionName::MermaidPreviewHeight, ConfigOptionValue::Number(n)) => {
                validate_number_range("mermaidpreviewheight", *n, 1, 100)?;
                log::debug!(
                    "[config_runtime][mermaid_preview] setting mermaidpreviewheight: {} -> {}",
                    state.mermaid_preview_height_percent,
                    n
                );
                state.mermaid_preview_height_percent = *n;
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

fn normalize_mermaid_preview_background(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "transparent".to_string()
    } else {
        trimmed.to_string()
    }
}
