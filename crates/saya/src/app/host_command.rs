//! ホストコマンド文字列のパース。
//!
//! Ex コマンド／ランタイム dispatch 文字列を `MainHostCommand` へ
//! 解釈する純粋関数群。保存・終了・dired・Markdown プレビュー・各種
//! フロート（buffer/terminal/LSP）・編集コマンドなどを識別する。
//! 実行はバイナリ側のディスパッチハブに委ねる。

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MainHostCommand {
    Save,
    SaveThenQuit,
    CancelDirectoryPreview,
    Edit(std::path::PathBuf),
    BufferWindowFloat(String),
    TerminalFloat(String),
    TerminalCloseFloat(String),
    MarkdownPreviewMermaid,
    LspHoverFloat(String),
    LspDiagnosticFloat(String),
    LspLocationListFloat(String),
    LspSymbolOutlineFloat(String),
    LspGotoDefinition(String),
    LspWorkspaceEditPreview(String),
    LspCodeActionsFloat(String),
    LspPublishDiagnostics(String),
    LspNextDiagnostic(Option<String>),
    LspPreviousDiagnostic(Option<String>),
    LspStatus(String),
}

pub fn parse_main_host_command(command: &str) -> Option<MainHostCommand> {
    if let Some(command) = parse_buffer_float_host_command(command) {
        return Some(command);
    }
    if let Some(command) = parse_terminal_float_host_command(command) {
        return Some(command);
    }
    if let Some(command) = parse_lsp_float_host_command(command) {
        return Some(command);
    }
    let normalized = normalize_main_host_command(command)?;
    match normalized.as_str() {
        "w" | "write" => Some(MainHostCommand::Save),
        "wq" | "x" | "xit" | "exit" => Some(MainHostCommand::SaveThenQuit),
        "dired-cancel" | "diredcancel" => Some(MainHostCommand::CancelDirectoryPreview),
        "markdown.previewMermaid" | "markdown.previewmermaid" => {
            Some(MainHostCommand::MarkdownPreviewMermaid)
        }
        _ => parse_runtime_edit_command(&normalized).map(MainHostCommand::Edit),
    }
}

pub(crate) fn parse_buffer_float_host_command(command: &str) -> Option<MainHostCommand> {
    let trimmed = command.trim();
    let trimmed = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();
    let payload = trimmed
        .strip_prefix("buffer.floatWindow ")
        .or_else(|| trimmed.strip_prefix("buffer.windowFloat "))
        .map(str::trim);
    payload
        .filter(|payload| !payload.is_empty())
        .map(|payload| MainHostCommand::BufferWindowFloat(payload.to_string()))
}

pub(crate) fn parse_terminal_float_host_command(command: &str) -> Option<MainHostCommand> {
    let trimmed = command.trim();
    let trimmed = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();
    let payload = trimmed
        .strip_prefix("terminal.float ")
        .or_else(|| trimmed.strip_prefix("terminal.openFloat "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::TerminalFloat(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("terminal.closeFloat ")
        .or_else(|| trimmed.strip_prefix("terminal.detachFloat "))
        .map(str::trim);
    payload
        .filter(|payload| !payload.is_empty())
        .map(|payload| MainHostCommand::TerminalCloseFloat(payload.to_string()))
}

pub(crate) fn parse_lsp_float_host_command(command: &str) -> Option<MainHostCommand> {
    let trimmed = command.trim();
    let trimmed = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();
    let payload = trimmed
        .strip_prefix("lsp.floatHover ")
        .or_else(|| trimmed.strip_prefix("lsp.hoverFloat "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspHoverFloat(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("lsp.floatDiagnostics ")
        .or_else(|| trimmed.strip_prefix("lsp.diagnosticFloat "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspDiagnosticFloat(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("lsp.floatLocations ")
        .or_else(|| trimmed.strip_prefix("lsp.locationsFloat "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspLocationListFloat(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("lsp.floatSymbols ")
        .or_else(|| trimmed.strip_prefix("lsp.symbolsFloat "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspSymbolOutlineFloat(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("lsp.gotoDefinition ")
        .or_else(|| trimmed.strip_prefix("lsp.definitionGoto "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspGotoDefinition(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("lsp.previewWorkspaceEdit ")
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspWorkspaceEditPreview(
            payload.to_string(),
        ));
    }
    let payload = trimmed.strip_prefix("lsp.floatCodeActions ").map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspCodeActionsFloat(payload.to_string()));
    }
    let payload = trimmed
        .strip_prefix("lsp.publishDiagnostics ")
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspPublishDiagnostics(payload.to_string()));
    }
    let payload = trimmed.strip_prefix("lsp.status ").map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspStatus(payload.to_string()));
    }
    if trimmed == "lsp.nextDiagnostic" {
        return Some(MainHostCommand::LspNextDiagnostic(None));
    }
    if trimmed == "lsp.previousDiagnostic" || trimmed == "lsp.prevDiagnostic" {
        return Some(MainHostCommand::LspPreviousDiagnostic(None));
    }
    let payload = trimmed.strip_prefix("lsp.nextDiagnostic ").map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspNextDiagnostic(Some(
            payload.to_string(),
        )));
    }
    let payload = trimmed
        .strip_prefix("lsp.previousDiagnostic ")
        .or_else(|| trimmed.strip_prefix("lsp.prevDiagnostic "))
        .map(str::trim);
    if let Some(payload) = payload.filter(|payload| !payload.is_empty()) {
        return Some(MainHostCommand::LspPreviousDiagnostic(Some(
            payload.to_string(),
        )));
    }
    None
}

pub(crate) fn parse_runtime_edit_command(normalized: &str) -> Option<std::path::PathBuf> {
    let path = normalized
        .strip_prefix("edit ")
        .or_else(|| normalized.strip_prefix("e "))?
        .trim();
    if path.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(path))
}

pub fn runtime_save_then_quit_ex_command(command: &str) -> Option<&'static str> {
    let normalized = normalize_main_host_command(command)?;
    match normalized.as_str() {
        "wq" => Some(":wq"),
        "x" | "xit" | "exit" => Some(":x"),
        _ => None,
    }
}

pub(crate) fn normalize_main_host_command(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let trimmed = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn startup_registered_command_name_for_ex_command(
    command: &str,
    registry: &crate::runtime::callback_registry_seed::CallbackRegistrySeed,
) -> Option<String> {
    let normalized = normalize_main_host_command(command)?;
    registry
        .commands()
        .iter()
        .find(|registered| registered.name() == normalized)
        .map(|registered| registered.name().to_string())
}
