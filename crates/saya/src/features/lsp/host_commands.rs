//! LSP 関連のホストコマンド実行と、ポップアップサイズ解決ロジック。
//!
//! TypeScript ランタイムから dispatch される LSP ホストコマンド
//! （hover / diagnostic / location list / symbol outline / goto /
//! workspace edit preview / code actions / publish diagnostics /
//! cycle diagnostic / status）の実体と、フローティングポップアップの
//! サイズ仕様（行/列・固定/パーセント）の解決を担う。フロート生成自体は
//! `features::lsp::float` に委譲する。

use crate::features::lsp::float::{
    LspDiagnosticFloatRequest, LspDiagnosticStore, LspHoverFloatRequest, LspLocationListRequest,
    LspSymbolOutlineRequest, PopupSizeBasis, PopupSizeSpec, PopupSizeValue, ResolvedPopupSizeLimit,
    open_lsp_diagnostic_float, open_lsp_hover_float, open_lsp_location_list_float,
    open_lsp_symbol_outline_float,
};
use crate::presentation::floating_window::FloatingWindowManager;
use crate::presentation::screen_model::PaneRect;
use crate::runtime::integration::RuntimeCommandEffect;
use crate::runtime::live::RuntimeCommandError;
use crate::terminal::lifecycle::current_terminal_size;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspHoverPopupKind {
    Hover,
    SignatureHelp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspPopupKind {
    Hover,
    Diagnostics,
    Locations,
    Symbols,
    SignatureHelp,
}

pub struct PopupSizingContext {
    pub terminal_width: u16,
    pub terminal_height: u16,
    pub parent_window_rect: PaneRect,
}

pub fn parse_lsp_hover_popup_kind(
    value: &serde_json::Value,
) -> Result<LspHoverPopupKind, RuntimeCommandError> {
    match value.get("kind").and_then(serde_json::Value::as_str) {
        None | Some("hover") => Ok(LspHoverPopupKind::Hover),
        Some("signatureHelp") => Ok(LspHoverPopupKind::SignatureHelp),
        Some(kind) => Err(RuntimeCommandError::CommandFailed {
            name: "lsp.floatHover".to_string(),
            message: format!("invalid LSP hover float payload: unsupported kind {kind:?}"),
        }),
    }
}

pub fn default_lsp_popup_basis(kind: LspPopupKind) -> PopupSizeBasis {
    match kind {
        LspPopupKind::Locations | LspPopupKind::Symbols => PopupSizeBasis::Editor,
        LspPopupKind::Hover | LspPopupKind::Diagnostics | LspPopupKind::SignatureHelp => {
            PopupSizeBasis::Window
        }
    }
}

pub(crate) fn resolve_lsp_popup_size_limit_from_payload(
    command_name: &str,
    value: &serde_json::Value,
    kind: LspPopupKind,
    snapshot: &vim_core_rs::CoreLightSnapshot,
    window_id: i32,
) -> Result<ResolvedPopupSizeLimit, RuntimeCommandError> {
    let spec = parse_lsp_popup_size_spec(
        value.get("ui"),
        default_lsp_popup_basis(kind),
        &format!("{command_name}.ui"),
    )
    .map_err(|message| RuntimeCommandError::CommandFailed {
        name: command_name.to_string(),
        message,
    })?;
    let context = lsp_popup_sizing_context(snapshot, window_id);
    let limit = resolve_lsp_popup_size_limit(spec, &context);
    log::debug!(
        "[main][lsp_float] resolved popup size: command={}, kind={:?}, spec={:?}, terminal=({},{}), parent_rect=({},{} {}x{}), limit=({},{})",
        command_name,
        kind,
        spec,
        context.terminal_width,
        context.terminal_height,
        context.parent_window_rect.x,
        context.parent_window_rect.y,
        context.parent_window_rect.width,
        context.parent_window_rect.height,
        limit.max_width,
        limit.max_height
    );
    Ok(limit)
}

pub fn parse_lsp_popup_size_spec(
    value: Option<&serde_json::Value>,
    default_basis: PopupSizeBasis,
    field: &str,
) -> Result<PopupSizeSpec, String> {
    let Some(value) = value else {
        return Ok(PopupSizeSpec::lsp_default(default_basis));
    };
    let object = value
        .as_object()
        .ok_or_else(|| format!("invalid LSP popup size config: {field} must be an object"))?;
    for key in object.keys() {
        if !matches!(key.as_str(), "width" | "height" | "basis") {
            return Err(format!(
                "invalid LSP popup size config: unknown {field}.{key}"
            ));
        }
    }
    let basis = match object.get("basis").and_then(serde_json::Value::as_str) {
        None => default_basis,
        Some("window") => PopupSizeBasis::Window,
        Some("editor") => PopupSizeBasis::Editor,
        Some("available") => PopupSizeBasis::Available,
        Some(basis) => {
            return Err(format!(
                "invalid LSP popup size config: {field}.basis must be \"window\", \"editor\", or \"available\", got {basis:?}"
            ));
        }
    };
    Ok(PopupSizeSpec {
        width: parse_lsp_popup_size_value(object.get("width"), &format!("{field}.width"))?
            .unwrap_or(PopupSizeValue::Cells(72)),
        height: parse_lsp_popup_size_value(object.get("height"), &format!("{field}.height"))?
            .unwrap_or(PopupSizeValue::Cells(12)),
        basis,
    })
}

pub(crate) fn parse_lsp_popup_size_value(
    value: Option<&serde_json::Value>,
    field: &str,
) -> Result<Option<PopupSizeValue>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if let Some(cells) = value.as_u64() {
        let cells = u16::try_from(cells).map_err(|_| {
            format!("invalid LSP popup size config: {field} must fit in terminal cells")
        })?;
        if cells == 0 {
            return Err(format!(
                "invalid LSP popup size config: {field} must be at least 1"
            ));
        }
        return Ok(Some(PopupSizeValue::Cells(cells)));
    }
    if let Some(percent) = value.as_str().and_then(parse_lsp_popup_percent) {
        return Ok(Some(PopupSizeValue::Percent(percent)));
    }
    Err(format!(
        "invalid LSP popup size config: {field} must be a positive integer or a percentage string from 1% through 100%"
    ))
}

pub(crate) fn parse_lsp_popup_percent(value: &str) -> Option<u8> {
    let digits = value.strip_suffix('%')?;
    if digits.is_empty() || !digits.chars().all(|char| char.is_ascii_digit()) {
        return None;
    }
    let percent = digits.parse::<u8>().ok()?;
    (1..=100).contains(&percent).then_some(percent)
}

pub(crate) fn lsp_popup_sizing_context(
    snapshot: &vim_core_rs::CoreLightSnapshot,
    window_id: i32,
) -> PopupSizingContext {
    let (terminal_width, terminal_height) = current_terminal_size();
    let parent_window_rect = snapshot
        .window(window_id)
        .map(PaneRect::from_core_window)
        .unwrap_or(PaneRect {
            x: 0,
            y: 0,
            width: terminal_width,
            height: terminal_height,
        });
    PopupSizingContext {
        terminal_width,
        terminal_height,
        parent_window_rect,
    }
}

pub fn resolve_lsp_popup_size_limit(
    spec: PopupSizeSpec,
    context: &PopupSizingContext,
) -> ResolvedPopupSizeLimit {
    if matches!(spec.basis, PopupSizeBasis::Available) {
        log::debug!(
            "[main][lsp_float] popup size basis \"available\" is accepted but origin-based resolution is not available yet; falling back to \"window\""
        );
    }
    let width = resolve_lsp_popup_size_value(
        spec.width,
        spec.basis,
        context.terminal_width,
        context.parent_window_rect.width,
    );
    let height = resolve_lsp_popup_size_value(
        spec.height,
        spec.basis,
        context.terminal_height,
        context.parent_window_rect.height,
    );
    let limit = ResolvedPopupSizeLimit::bordered(width, height);
    ResolvedPopupSizeLimit {
        max_width: limit.max_width.min(context.terminal_width.max(1)),
        max_height: limit.max_height.min(context.terminal_height.max(1)),
    }
}

pub(crate) fn resolve_lsp_popup_size_value(
    value: PopupSizeValue,
    basis: PopupSizeBasis,
    editor_dimension: u16,
    window_dimension: u16,
) -> u16 {
    let resolved = match value {
        PopupSizeValue::Cells(cells) => cells,
        PopupSizeValue::Percent(percent) => {
            let basis_dimension = match basis {
                PopupSizeBasis::Editor => editor_dimension,
                PopupSizeBasis::Window | PopupSizeBasis::Available => window_dimension,
            };
            let cells = u32::from(basis_dimension) * u32::from(percent) / 100;
            u16::try_from(cells).unwrap_or(u16::MAX).max(1)
        }
    };
    resolved.max(1).min(editor_dimension.max(1))
}

pub fn execute_lsp_hover_float_host_command(
    payload: &str,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    lsp_diagnostic_store: Option<&LspDiagnosticStore>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "lsp.floatHover".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.floatHover".to_string(),
            message: format!("invalid LSP hover float payload: {error}"),
        })?;
    let snapshot = outcome.core_bridge.light_snapshot();
    let window_id = snapshot.active_window_id().unwrap_or(1);
    let hover_kind = parse_lsp_hover_popup_kind(&value)?;
    let size_limit = resolve_lsp_popup_size_limit_from_payload(
        "lsp.floatHover",
        &value,
        match hover_kind {
            LspHoverPopupKind::Hover => LspPopupKind::Hover,
            LspHoverPopupKind::SignatureHelp => LspPopupKind::SignatureHelp,
        },
        &snapshot,
        window_id,
    )?;
    let response = value
        .get("response")
        .or_else(|| value.get("result"))
        .cloned()
        .unwrap_or(value);
    if hover_response_is_plain_any(&response)
        && let Some(diagnostic) = lsp_diagnostic_store.and_then(|store| {
            store.diagnostic_at_position(snapshot.cursor_row, snapshot.cursor_col)
        })
    {
        let response = serde_json::json!({
            "result": {
                "contents": format!("{}: {}", lsp_diagnostic_severity_label(diagnostic.severity), diagnostic.message)
            }
        });
        let outcome = open_lsp_hover_float(
            manager,
            LspHoverFloatRequest {
                window_id,
                cursor_row: snapshot.cursor_row,
                cursor_col: snapshot.cursor_col,
                response,
                size_limit,
            },
        );
        log::debug!(
            "[main][lsp_float] hover any replaced with cursor-relative diagnostic hover: outcome={:?}, window_id={}, cursor=({}, {}), diagnostic=({}, {})",
            outcome,
            window_id,
            snapshot.cursor_row,
            snapshot.cursor_col,
            diagnostic.line,
            diagnostic.column
        );
        return Ok(RuntimeCommandEffect {
            transient_message: outcome.is_none().then(|| "No LSP diagnostics".to_string()),
            ..RuntimeCommandEffect::default()
        });
    }
    let outcome = open_lsp_hover_float(
        manager,
        LspHoverFloatRequest {
            window_id,
            cursor_row: snapshot.cursor_row,
            cursor_col: snapshot.cursor_col,
            response,
            size_limit,
        },
    );
    log::debug!(
        "[main][lsp_float] hover float host command applied: outcome={:?}, window_id={}, cursor=({}, {})",
        outcome,
        window_id,
        snapshot.cursor_row,
        snapshot.cursor_col
    );
    Ok(RuntimeCommandEffect {
        transient_message: outcome
            .is_none()
            .then(|| "No LSP hover content".to_string()),
        ..RuntimeCommandEffect::default()
    })
}

pub(crate) fn lsp_diagnostic_severity_label(severity: Option<u64>) -> &'static str {
    match severity {
        Some(1) => "Error",
        Some(2) => "Warning",
        Some(3) => "Info",
        Some(4) => "Hint",
        _ => "Diagnostic",
    }
}

pub(crate) fn hover_response_is_plain_any(response: &serde_json::Value) -> bool {
    let Some(contents) = response
        .pointer("/result/contents")
        .or_else(|| response.get("contents"))
    else {
        return false;
    };
    hover_contents_plain_text(contents)
        .map(|text| {
            text.replace("```typescript", "")
                .replace("```ts", "")
                .replace("```", "")
                .trim()
                == "any"
        })
        .unwrap_or(false)
}

pub(crate) fn hover_contents_plain_text(contents: &serde_json::Value) -> Option<String> {
    if let Some(text) = contents.as_str() {
        return Some(text.to_string());
    }
    if let Some(value) = contents.get("value").and_then(serde_json::Value::as_str) {
        return Some(value.to_string());
    }
    let items = contents.as_array()?;
    let mut parts = Vec::new();
    for item in items {
        if let Some(text) = hover_contents_plain_text(item) {
            parts.push(text);
        }
    }
    Some(parts.join("\n"))
}

pub fn execute_lsp_diagnostic_float_host_command(
    payload: &str,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "lsp.floatDiagnostics".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.floatDiagnostics".to_string(),
            message: format!("invalid LSP diagnostic float payload: {error}"),
        })?;
    let snapshot = outcome.core_bridge.light_snapshot();
    let window_id = snapshot.active_window_id().unwrap_or(1);
    let size_limit = resolve_lsp_popup_size_limit_from_payload(
        "lsp.floatDiagnostics",
        &value,
        LspPopupKind::Diagnostics,
        &snapshot,
        window_id,
    )?;
    let line = value
        .get("line")
        .and_then(serde_json::Value::as_u64)
        .map(|line| line as usize)
        .unwrap_or(snapshot.cursor_row);
    let column = value
        .get("column")
        .and_then(serde_json::Value::as_u64)
        .map(|column| column as usize)
        .unwrap_or(snapshot.cursor_col);
    let diagnostics = value
        .get("diagnostics")
        .or_else(|| value.pointer("/params/diagnostics"))
        .cloned()
        .unwrap_or(value);
    let id = open_lsp_diagnostic_float(
        manager,
        LspDiagnosticFloatRequest {
            window_id,
            line,
            column,
            diagnostics,
            size_limit,
        },
    );
    log::debug!(
        "[main][lsp_float] diagnostic float host command applied: opened={:?}, window_id={}, position=({}, {})",
        id.map(|id| id.0),
        window_id,
        line,
        column
    );
    Ok(RuntimeCommandEffect {
        transient_message: id.is_none().then(|| "No LSP diagnostics".to_string()),
        ..RuntimeCommandEffect::default()
    })
}

pub fn execute_lsp_location_list_float_host_command(
    payload: &str,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "lsp.floatLocations".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.floatLocations".to_string(),
            message: format!("invalid LSP locations float payload: {error}"),
        })?;
    let snapshot = outcome.core_bridge.light_snapshot();
    let window_id = snapshot.active_window_id().unwrap_or(1);
    let size_limit = resolve_lsp_popup_size_limit_from_payload(
        "lsp.floatLocations",
        &value,
        LspPopupKind::Locations,
        &snapshot,
        window_id,
    )?;
    let title = value
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Locations")
        .to_string();
    let response = value
        .get("response")
        .or_else(|| value.get("result"))
        .cloned()
        .unwrap_or(value);
    let id = open_lsp_location_list_float(
        manager,
        LspLocationListRequest {
            window_id,
            title,
            response,
            size_limit,
        },
    );
    log::debug!(
        "[main][lsp_float] location list float host command applied: opened={:?}, window_id={}",
        id.map(|id| id.0),
        window_id
    );
    Ok(RuntimeCommandEffect {
        transient_message: id.is_none().then(|| "No LSP locations".to_string()),
        ..RuntimeCommandEffect::default()
    })
}

pub fn execute_lsp_symbol_outline_float_host_command(
    payload: &str,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "lsp.floatSymbols".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.floatSymbols".to_string(),
            message: format!("invalid LSP symbols float payload: {error}"),
        })?;
    let snapshot = outcome.core_bridge.light_snapshot();
    let window_id = snapshot.active_window_id().unwrap_or(1);
    let size_limit = resolve_lsp_popup_size_limit_from_payload(
        "lsp.floatSymbols",
        &value,
        LspPopupKind::Symbols,
        &snapshot,
        window_id,
    )?;
    let response = value
        .get("response")
        .or_else(|| value.get("result"))
        .cloned()
        .unwrap_or(value);
    let id = open_lsp_symbol_outline_float(
        manager,
        LspSymbolOutlineRequest {
            window_id,
            response,
            size_limit,
        },
    );
    log::debug!(
        "[main][lsp_float] symbol outline float host command applied: opened={:?}, window_id={}",
        id.map(|id| id.0),
        window_id
    );
    Ok(RuntimeCommandEffect {
        transient_message: id.is_none().then(|| "No LSP symbols".to_string()),
        ..RuntimeCommandEffect::default()
    })
}

pub fn execute_lsp_workspace_edit_preview_host_command(
    payload: &str,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.previewWorkspaceEdit".to_string(),
            message: format!("invalid LSP workspace edit preview payload: {error}"),
        })?;
    let title = value
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Workspace edit preview");
    let edit = value
        .pointer("/response/result")
        .or_else(|| value.get("response"))
        .or_else(|| value.get("result"))
        .unwrap_or(&value);
    let lines = workspace_edit_preview_lines(title, edit);
    let response = serde_json::json!({
        "result": {
            "contents": lines.join("\n")
        }
    });
    let payload = serde_json::json!({ "response": response }).to_string();
    let mut effect =
        execute_lsp_hover_float_host_command(&payload, outcome, floating_window_manager, None)?;
    effect.transient_message = Some(format!(
        "{title}: {} change(s)",
        lines.len().saturating_sub(1)
    ));
    log::debug!(
        "[main][lsp] workspace edit preview displayed: title={}, lines={}",
        title,
        lines.len()
    );
    Ok(effect)
}

pub fn execute_lsp_code_actions_float_host_command(
    payload: &str,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.floatCodeActions".to_string(),
            message: format!("invalid LSP code action payload: {error}"),
        })?;
    let actions = value
        .pointer("/response/result")
        .or_else(|| value.get("response"))
        .or_else(|| value.get("result"))
        .unwrap_or(&value);
    let lines = code_action_preview_lines(actions);
    let response = serde_json::json!({
        "result": {
            "contents": lines.join("\n")
        }
    });
    let payload = serde_json::json!({ "response": response }).to_string();
    let mut effect =
        execute_lsp_hover_float_host_command(&payload, outcome, floating_window_manager, None)?;
    effect.transient_message = Some(format!(
        "LSP code actions: {} action(s)",
        lines.len().saturating_sub(1)
    ));
    log::debug!(
        "[main][lsp] code action float displayed: lines={}",
        lines.len()
    );
    Ok(effect)
}

pub fn execute_lsp_publish_diagnostics_host_command(
    payload: &str,
    _outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    _floating_window_manager: Option<&mut FloatingWindowManager>,
    lsp_diagnostic_store: Option<&mut LspDiagnosticStore>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.publishDiagnostics".to_string(),
            message: format!("invalid LSP diagnostics payload: {error}"),
        })?;
    if let Some(store) = lsp_diagnostic_store {
        store.replace_from_lsp_value(&value);
    }
    Ok(RuntimeCommandEffect {
        transient_message: None,
        ..RuntimeCommandEffect::default()
    })
}

pub fn execute_lsp_cycle_diagnostic_host_command(
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    lsp_diagnostic_store: Option<&mut LspDiagnosticStore>,
    next: bool,
    payload: Option<&str>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let store = lsp_diagnostic_store.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "lsp.diagnosticNavigation".to_string(),
        message: "LSP diagnostic store is not available".to_string(),
    })?;
    let diagnostic = if next {
        store.next_diagnostic()
    } else {
        store.previous_diagnostic()
    }
    .cloned();
    let Some(diagnostic) = diagnostic else {
        return Ok(RuntimeCommandEffect {
            transient_message: Some("No LSP diagnostics".to_string()),
            ..RuntimeCommandEffect::default()
        });
    };
    let ui =
        match payload {
            Some(payload) => {
                let value: serde_json::Value = serde_json::from_str(payload).map_err(|error| {
                    RuntimeCommandError::CommandFailed {
                        name: "lsp.diagnosticNavigation".to_string(),
                        message: format!("invalid LSP diagnostic navigation payload: {error}"),
                    }
                })?;
                Some(value.get("ui").cloned().ok_or_else(|| {
                    RuntimeCommandError::CommandFailed {
                        name: "lsp.diagnosticNavigation".to_string(),
                        message: "diagnostic navigation payload must include ui".to_string(),
                    }
                })?)
            }
            None => None,
        };
    let mut payload = serde_json::json!({
        "line": diagnostic.line,
        "column": diagnostic.column,
        "diagnostics": [{
            "severity": diagnostic.severity,
            "message": diagnostic.message,
        }],
    });
    if let Some(ui) = ui {
        payload["ui"] = ui;
    }
    let payload = payload.to_string();
    let mut effect =
        execute_lsp_diagnostic_float_host_command(&payload, outcome, floating_window_manager)?;
    effect.transient_message = Some(format!(
        "LSP diagnostic: {}:{} {}",
        diagnostic.line + 1,
        diagnostic.column + 1,
        diagnostic.message
    ));
    Ok(effect)
}

pub(crate) fn workspace_edit_preview_lines(title: &str, edit: &serde_json::Value) -> Vec<String> {
    let mut lines = vec![title.to_string()];
    if let Some(changes) = edit.get("changes").and_then(serde_json::Value::as_object) {
        for (uri, edits) in changes {
            let count = edits
                .as_array()
                .map(|items| items.len())
                .unwrap_or_default();
            lines.push(format!("{uri}: {count} edit(s)"));
        }
    }
    if let Some(document_changes) = edit
        .get("documentChanges")
        .and_then(serde_json::Value::as_array)
    {
        for change in document_changes {
            let uri = change
                .pointer("/textDocument/uri")
                .or_else(|| change.get("uri"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<unknown>");
            if let Some(edits) = change.get("edits").and_then(serde_json::Value::as_array) {
                lines.push(format!("{uri}: {} edit(s)", edits.len()));
            } else if let Some(kind) = change.get("kind").and_then(serde_json::Value::as_str) {
                lines.push(format!("{kind}: {uri}"));
            }
        }
    }
    if lines.len() == 1 && edit.as_array().is_some() {
        lines.push(format!(
            "current document: {} edit(s)",
            edit.as_array().map(|items| items.len()).unwrap_or_default()
        ));
    }
    if lines.len() == 1 {
        lines.push("No workspace edits".to_string());
    }
    lines
}

pub(crate) fn code_action_preview_lines(actions: &serde_json::Value) -> Vec<String> {
    let mut lines = vec!["Code actions".to_string()];
    match actions {
        serde_json::Value::Array(items) => {
            for action in items {
                let title = action
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("<untitled>");
                let kind = action
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("action");
                lines.push(format!("{kind}: {title}"));
            }
        }
        serde_json::Value::Object(_) => {
            let title = actions
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<untitled>");
            let kind = actions
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("action");
            lines.push(format!("{kind}: {title}"));
        }
        _ => {}
    }
    if lines.len() == 1 {
        lines.push("No code actions".to_string());
    }
    lines
}

pub fn execute_lsp_status_host_command(
    payload: &str,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "lsp.status".to_string(),
            message: format!("invalid LSP status payload: {error}"),
        })?;
    let message = value
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Language server is not ready")
        .to_string();
    log::info!("[main][lsp] status: {message}");
    Ok(RuntimeCommandEffect {
        transient_message: Some(message),
        ..RuntimeCommandEffect::default()
    })
}
