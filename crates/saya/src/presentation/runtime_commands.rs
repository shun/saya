//! ランタイム発のフローティングウィンドウ／パネル操作コマンドの実行。
//!
//! TypeScript ランタイムから dispatch されるウィンドウフロート・パネル・
//! ターミナルフロートの open/close 要求を、`FloatingWindowManager` /
//! `PanelManager` / `TerminalFloatManager` への操作へ変換する。要求 JSON
//! の各フィールド（anchor/border/lifecycle/zindex/placement/size など）の
//! 解釈もここで行う。

use crate::presentation::floating_window::{
    FloatingAnchor, FloatingBorder, FloatingChrome, FloatingFit, FloatingLifecycle,
    FloatingPlacement, FloatingRelativeTo, FloatingSize, FloatingWindowId, FloatingWindowManager,
    FloatingZIndex,
};
use crate::presentation::panel::{
    PanelCloseBehavior, PanelContent, PanelManager, PanelNode, PanelOpenRequest, PanelPosition,
    PanelSize,
};
use crate::runtime::integration::RuntimeCommandEffect;
use crate::runtime::live::{
    RuntimeCommandError, RuntimeFloatContentRequest, RuntimeFloatOpenRequest,
    RuntimeFloatRelativeToRequest, RuntimeFloatSnapshot, RuntimeFloatZIndexRequest,
    RuntimePanelContentRequest, RuntimePanelNodeRequest, RuntimePanelOpenRequest,
    RuntimePanelSnapshot,
};
use crate::terminal::float::{
    TerminalFloatCloseBehavior, TerminalFloatManager, TerminalFloatSpawnRequest,
};
use vim_core_rs::CoreLightSnapshot;

pub fn execute_runtime_window_open_float(
    request: RuntimeFloatOpenRequest,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<RuntimeFloatSnapshot, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "window.openFloat".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let size = FloatingSize {
        width: request.width.unwrap_or(60).max(1),
        height: request.height.unwrap_or(12).max(1),
    };
    let chrome = FloatingChrome {
        border: runtime_float_border(request.border.as_deref()),
    };
    let placement = runtime_float_placement(&request, outcome)?;
    let zindex = runtime_float_zindex(request.z_index.as_ref());
    let lifecycle = runtime_float_lifecycle(request.lifecycle.as_deref());
    let focusable = request.focusable.unwrap_or(false);
    let replacement_group = request.group.clone();

    let id = match request.content.clone() {
        RuntimeFloatContentRequest::Lines { lines } => manager
            .open_static_lines_with_lifecycle_and_replacement_group(
                lines,
                lifecycle,
                replacement_group,
                placement,
                size,
                chrome,
                zindex,
                focusable,
            ),
        RuntimeFloatContentRequest::Buffer {
            buffer_id,
            window_id,
        } => {
            let snapshot = outcome.core_bridge.light_snapshot();
            let (window_id, _buffer_id) = resolve_buffer_float_backing_window(
                "window.openFloat",
                &snapshot,
                window_id.map(|id| id as i32),
                buffer_id.map(|id| id as i32),
            )?;
            manager.open_core_window_with_lifecycle(
                window_id,
                lifecycle,
                replacement_group,
                placement,
                size,
                chrome,
                zindex,
                focusable,
            )
        }
        RuntimeFloatContentRequest::Terminal {
            command,
            close_behavior,
        } => {
            let terminal_manager =
                terminal_float_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
                    name: "window.openFloat".to_string(),
                    message: "terminal float manager is not available".to_string(),
                })?;
            let (command, args) = runtime_terminal_command_parts(command).ok_or_else(|| {
                RuntimeCommandError::CommandFailed {
                    name: "window.openFloat".to_string(),
                    message: "terminal float command must not be empty".to_string(),
                }
            })?;
            let terminal_size = runtime_terminal_content_size(size, chrome);
            let terminal_id = terminal_manager
                .spawn(TerminalFloatSpawnRequest {
                    command,
                    args,
                    width: terminal_size.width,
                    height: terminal_size.height,
                    close_behavior: runtime_terminal_close_behavior(close_behavior.as_deref()),
                })
                .map_err(|error| RuntimeCommandError::CommandFailed {
                    name: "window.openFloat".to_string(),
                    message: format!("failed to spawn terminal float: {error:?}"),
                })?;
            manager.open_terminal_with_lifecycle(
                terminal_id,
                lifecycle,
                replacement_group,
                placement,
                size,
                chrome,
                zindex,
                focusable,
            )
        }
    };

    if focusable {
        manager.focus_float(id);
    }
    log::debug!(
        "[main][runtime_window] openFloat applied: float_id={}, content={:?}, focusable={}, size=({},{})",
        id.0,
        request.content,
        focusable,
        size.width,
        size.height
    );
    let focused_float_id = manager.focused_float_id();
    manager
        .debug_window(id)
        .map(|window| runtime_float_snapshot(window, focused_float_id))
        .ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "window.openFloat".to_string(),
            message: format!("opened float is missing: id={}", id.0),
        })
}

pub fn execute_runtime_window_close_float(
    id: u64,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<bool, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "window.close".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let float_id = FloatingWindowId(id);
    let terminal_id = manager
        .window_content(float_id)
        .and_then(|content| match content {
            crate::presentation::floating_window::FloatingContentRef::Terminal { terminal_id } => {
                Some(*terminal_id)
            }
            _ => None,
        });
    let closed = manager.close(float_id);
    if closed
        && let (Some(terminal_id), Some(terminal_manager)) = (terminal_id, terminal_float_manager)
    {
        let _ = terminal_manager.close_view(terminal_id);
    }
    log::debug!(
        "[main][runtime_window] close applied: float_id={}, closed={}, terminal_id={:?}",
        id,
        closed,
        terminal_id
    );
    Ok(closed)
}

pub fn runtime_float_snapshots(manager: &FloatingWindowManager) -> Vec<RuntimeFloatSnapshot> {
    let focused_float_id = manager.focused_float_id();
    manager
        .windows()
        .iter()
        .map(|window| runtime_float_snapshot(window, focused_float_id))
        .collect()
}

pub(crate) fn runtime_float_snapshot(
    window: &crate::presentation::floating_window::FloatingWindow,
    focused_float_id: Option<FloatingWindowId>,
) -> RuntimeFloatSnapshot {
    RuntimeFloatSnapshot {
        id: window.id.0,
        kind: runtime_float_content_kind(&window.content).to_string(),
        focused: focused_float_id == Some(window.id),
        focusable: window.focusable,
        width: window.size.width,
        height: window.size.height,
        row: window.placement.row,
        col: window.placement.col,
        border: runtime_float_border_label(window.chrome.border).to_string(),
        z_index: window.zindex,
        lifecycle: runtime_float_lifecycle_label(window.lifecycle).to_string(),
        replacement_group: window.replacement_group.clone(),
    }
}

pub(crate) fn runtime_float_content_kind(
    content: &crate::presentation::floating_window::FloatingContentRef,
) -> &'static str {
    match content {
        crate::presentation::floating_window::FloatingContentRef::CoreWindow { .. } => "buffer",
        crate::presentation::floating_window::FloatingContentRef::ScratchBuffer { .. } => "buffer",
        crate::presentation::floating_window::FloatingContentRef::Terminal { .. } => "terminal",
        crate::presentation::floating_window::FloatingContentRef::StaticLines { .. } => "lines",
        crate::presentation::floating_window::FloatingContentRef::CompletionMenu { .. } => {
            "completionMenu"
        }
    }
}

pub(crate) fn runtime_float_border_label(border: FloatingBorder) -> &'static str {
    match border {
        FloatingBorder::None => "none",
        FloatingBorder::Single => "single",
    }
}

pub(crate) fn runtime_float_lifecycle_label(lifecycle: FloatingLifecycle) -> &'static str {
    match lifecycle {
        FloatingLifecycle::Manual => "manual",
        FloatingLifecycle::CloseOnCursorMove => "closeOnCursorMove",
        FloatingLifecycle::CloseOnInsert => "closeOnInsert",
        FloatingLifecycle::CloseOnBufferChange => "closeOnBufferChange",
        FloatingLifecycle::CloseOnEvents(_) => "closeOnEvents",
        FloatingLifecycle::ReplaceByGroup(_) => "replaceByGroup",
    }
}

pub(crate) fn runtime_float_border(border: Option<&str>) -> FloatingBorder {
    match border.unwrap_or("single") {
        "none" | "borderless" => FloatingBorder::None,
        _ => FloatingBorder::Single,
    }
}

pub(crate) fn runtime_float_lifecycle(lifecycle: Option<&str>) -> FloatingLifecycle {
    match lifecycle.unwrap_or("manual") {
        "closeOnCursorMove" | "close-on-cursor-move" => FloatingLifecycle::CloseOnCursorMove,
        "closeOnInsert" | "close-on-insert" => FloatingLifecycle::CloseOnInsert,
        "closeOnBufferChange" | "close-on-buffer-change" => FloatingLifecycle::CloseOnBufferChange,
        _ => FloatingLifecycle::Manual,
    }
}

pub(crate) fn runtime_float_zindex(zindex: Option<&RuntimeFloatZIndexRequest>) -> FloatingZIndex {
    match zindex {
        Some(RuntimeFloatZIndexRequest::Custom(value)) => FloatingZIndex::Custom(*value),
        Some(RuntimeFloatZIndexRequest::Named(name)) => match name.as_str() {
            "hover" => FloatingZIndex::Hover,
            "completion" => FloatingZIndex::Completion,
            "completionDocumentation" | "completion-documentation" => {
                FloatingZIndex::CompletionDocumentation
            }
            "blockingPrompt" | "blocking-prompt" => FloatingZIndex::BlockingPrompt,
            _ => FloatingZIndex::User,
        },
        None => FloatingZIndex::User,
    }
}

pub(crate) fn runtime_float_placement(
    request: &RuntimeFloatOpenRequest,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
) -> Result<FloatingPlacement, RuntimeCommandError> {
    let row = request.row.unwrap_or(1);
    let col = request.col.unwrap_or(2);
    let relative_to = match request.relative_to.as_ref() {
        Some(RuntimeFloatRelativeToRequest::Cursor { window_id }) => FloatingRelativeTo::Cursor {
            window_id: runtime_float_window_id(*window_id, outcome)?,
        },
        Some(RuntimeFloatRelativeToRequest::Window { window_id }) => FloatingRelativeTo::Window {
            window_id: runtime_float_window_id(*window_id, outcome)?,
        },
        Some(RuntimeFloatRelativeToRequest::BufferPosition {
            window_id,
            line,
            column,
        }) => FloatingRelativeTo::BufferPosition {
            window_id: runtime_float_window_id(*window_id, outcome)?,
            line: *line,
            column: *column,
        },
        _ => FloatingRelativeTo::Editor,
    };
    Ok(FloatingPlacement {
        relative_to,
        anchor: runtime_float_anchor(request.anchor.as_deref()),
        row,
        col,
        fit: FloatingFit::TruncateToGrid,
    })
}

pub(crate) fn runtime_float_anchor(anchor: Option<&str>) -> FloatingAnchor {
    match anchor.unwrap_or("nw") {
        "ne" => FloatingAnchor::NorthEast,
        "sw" => FloatingAnchor::SouthWest,
        "se" => FloatingAnchor::SouthEast,
        _ => FloatingAnchor::NorthWest,
    }
}

pub(crate) fn runtime_float_window_id(
    requested: Option<u64>,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
) -> Result<i32, RuntimeCommandError> {
    requested
        .map(|id| id as i32)
        .or_else(|| outcome.core_bridge.light_snapshot().active_window_id())
        .ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "window.openFloat".to_string(),
            message: "active window is not available".to_string(),
        })
}

pub(crate) fn runtime_terminal_command_parts(
    command: Vec<String>,
) -> Option<(String, Vec<String>)> {
    let mut parts = command.into_iter();
    let command = parts.next()?.trim().to_string();
    if command.is_empty() {
        return None;
    }
    Some((command, parts.collect()))
}

pub(crate) fn runtime_terminal_content_size(
    size: FloatingSize,
    chrome: FloatingChrome,
) -> FloatingSize {
    match chrome.border {
        FloatingBorder::Single => FloatingSize {
            width: size.width.saturating_sub(2).max(1),
            height: size.height.saturating_sub(2).max(1),
        },
        FloatingBorder::None => size,
    }
}

pub(crate) fn runtime_terminal_close_behavior(
    close_behavior: Option<&str>,
) -> TerminalFloatCloseBehavior {
    match close_behavior.unwrap_or("kill") {
        "detach" | "detachOnClose" | "detach-on-close" => TerminalFloatCloseBehavior::DetachOnClose,
        _ => TerminalFloatCloseBehavior::KillOnClose,
    }
}

pub fn execute_runtime_panel_open(
    request: RuntimePanelOpenRequest,
    panel_manager: Option<&mut PanelManager>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<RuntimePanelSnapshot, RuntimeCommandError> {
    let manager = panel_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "panel.open".to_string(),
        message: "panel manager is not available".to_string(),
    })?;
    let position = runtime_panel_position(&request.position)?;
    let size = runtime_panel_size(&request.size)?;
    let content = runtime_panel_content(request.content.clone(), terminal_float_manager)?;
    let result = manager.open(PanelOpenRequest {
        id: request.id.clone(),
        position,
        size,
        content,
        focus: request.focus,
    });
    log::debug!(
        "[main][panel] open applied: id={}, numeric_id={}, position={:?}, size={:?}, focus={}",
        result.id,
        result.numeric_id,
        result.position,
        result.size,
        result.focused
    );
    manager
        .snapshots()
        .into_iter()
        .find(|snapshot| snapshot.id == request.id)
        .map(runtime_panel_snapshot)
        .ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "panel.open".to_string(),
            message: format!("opened panel is missing: id={}", request.id),
        })
}

pub fn execute_runtime_panel_close(
    id: String,
    panel_manager: Option<&mut PanelManager>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<bool, RuntimeCommandError> {
    let manager = panel_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "panel.close".to_string(),
        message: "panel manager is not available".to_string(),
    })?;
    let content = manager.close(&id);
    if let Some(content) = content.as_ref()
        && let Some(terminal_id) = content.terminal_id()
        && let Some(terminal_manager) = terminal_float_manager
    {
        let _ = terminal_manager.close_view(terminal_id);
    }
    log::debug!(
        "[main][panel] close applied: id={}, closed={}, content={:?}",
        id,
        content.is_some(),
        content
    );
    Ok(content.is_some())
}

pub fn runtime_panel_snapshot(
    snapshot: crate::presentation::panel::PanelSnapshot,
) -> RuntimePanelSnapshot {
    RuntimePanelSnapshot {
        id: snapshot.id,
        numeric_id: snapshot.numeric_id,
        position: runtime_panel_position_label(snapshot.position).to_string(),
        size: runtime_panel_size_label(snapshot.size),
        kind: snapshot.kind.to_string(),
        focused: snapshot.focused,
    }
}

pub(crate) fn runtime_panel_position(value: &str) -> Result<PanelPosition, RuntimeCommandError> {
    match value {
        "left" => Ok(PanelPosition::Left),
        "right" => Ok(PanelPosition::Right),
        "top" => Ok(PanelPosition::Top),
        "bottom" => Ok(PanelPosition::Bottom),
        _ => Err(RuntimeCommandError::CommandFailed {
            name: "panel.open".to_string(),
            message: format!("unsupported panel position: {value}"),
        }),
    }
}

pub(crate) fn runtime_panel_position_label(position: PanelPosition) -> &'static str {
    match position {
        PanelPosition::Left => "left",
        PanelPosition::Right => "right",
        PanelPosition::Top => "top",
        PanelPosition::Bottom => "bottom",
    }
}

pub(crate) fn runtime_panel_size(value: &str) -> Result<PanelSize, RuntimeCommandError> {
    let trimmed = value.trim();
    if let Some(percent) = trimmed.strip_suffix('%') {
        return percent
            .parse::<u16>()
            .map(PanelSize::Percent)
            .map_err(|error| RuntimeCommandError::CommandFailed {
                name: "panel.open".to_string(),
                message: format!("invalid panel percent size: {error}"),
            });
    }
    trimmed
        .parse::<u16>()
        .map(PanelSize::Cells)
        .map_err(|error| RuntimeCommandError::CommandFailed {
            name: "panel.open".to_string(),
            message: format!("invalid panel size: {error}"),
        })
}

pub(crate) fn runtime_panel_size_label(size: PanelSize) -> String {
    match size {
        PanelSize::Cells(cells) => cells.to_string(),
        PanelSize::Percent(percent) => format!("{percent}%"),
    }
}

pub(crate) fn runtime_panel_content(
    request: RuntimePanelContentRequest,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<PanelContent, RuntimeCommandError> {
    match request.kind.as_str() {
        "lines" => Ok(PanelContent::Lines {
            lines: request.lines,
        }),
        "view" => {
            let nodes = request
                .nodes
                .into_iter()
                .map(runtime_panel_node)
                .collect::<Result<Vec<_>, _>>()?;
            log::debug!(
                "[main][panel] converted runtime view content: nodes={}",
                nodes.len()
            );
            Ok(PanelContent::View { nodes })
        }
        "terminal" => {
            let terminal_manager =
                terminal_float_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
                    name: "panel.open".to_string(),
                    message: "terminal manager is not available".to_string(),
                })?;
            let (command, args) =
                runtime_terminal_command_parts(request.command).ok_or_else(|| {
                    RuntimeCommandError::CommandFailed {
                        name: "panel.open".to_string(),
                        message: "panel terminal command must not be empty".to_string(),
                    }
                })?;
            let close_behavior = runtime_panel_close_behavior(request.close_behavior.as_deref());
            let terminal_id = terminal_manager
                .spawn(TerminalFloatSpawnRequest {
                    command,
                    args,
                    width: 80,
                    height: 24,
                    close_behavior: match close_behavior {
                        PanelCloseBehavior::Kill => TerminalFloatCloseBehavior::KillOnClose,
                        PanelCloseBehavior::Detach => TerminalFloatCloseBehavior::DetachOnClose,
                    },
                })
                .map_err(|error| RuntimeCommandError::CommandFailed {
                    name: "panel.open".to_string(),
                    message: format!("failed to spawn panel terminal: {error:?}"),
                })?;
            Ok(PanelContent::Terminal {
                terminal_id,
                close_behavior,
            })
        }
        other => Err(RuntimeCommandError::CommandFailed {
            name: "panel.open".to_string(),
            message: format!("unsupported panel content kind: {other}"),
        }),
    }
}

pub(crate) fn runtime_panel_node(
    request: RuntimePanelNodeRequest,
) -> Result<PanelNode, RuntimeCommandError> {
    let required_text = |field: Option<String>, node_type: &str, name: &str| {
        field
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| RuntimeCommandError::CommandFailed {
                name: "panel.open".to_string(),
                message: format!("panel view node {node_type} requires {name}"),
            })
    };
    match request.node_type.as_str() {
        "text" => Ok(PanelNode::Text {
            text: required_text(request.text, "text", "text")?,
        }),
        "heading" => Ok(PanelNode::Heading {
            text: required_text(request.text, "heading", "text")?,
        }),
        "divider" => Ok(PanelNode::Divider),
        "image" => Ok(PanelNode::Image {
            src: required_text(request.src, "image", "src")?,
            alt: request.alt,
        }),
        "badge" => Ok(PanelNode::Badge {
            label: required_text(request.label, "badge", "label")?,
        }),
        "progress" => Ok(PanelNode::Progress {
            label: request.label,
            value: request.value.unwrap_or(0).min(100),
        }),
        "button" => Ok(PanelNode::Button {
            label: required_text(request.label, "button", "label")?,
        }),
        other => Err(RuntimeCommandError::CommandFailed {
            name: "panel.open".to_string(),
            message: format!("unsupported panel view node type: {other}"),
        }),
    }
}

pub(crate) fn runtime_panel_close_behavior(close_behavior: Option<&str>) -> PanelCloseBehavior {
    match close_behavior.unwrap_or("kill") {
        "detach" | "detachOnClose" | "detach-on-close" => PanelCloseBehavior::Detach,
        _ => PanelCloseBehavior::Kill,
    }
}

pub fn resolve_buffer_float_backing_window(
    command_name: &str,
    snapshot: &CoreLightSnapshot,
    requested_window_id: Option<i32>,
    requested_buffer_id: Option<i32>,
) -> Result<(i32, i32), RuntimeCommandError> {
    let chosen_window_id = match (requested_window_id, requested_buffer_id) {
        (Some(window_id), _) => window_id,
        (None, Some(buffer_id)) => {
            let Some(window) = snapshot.windows.iter().find(|window| window.buf_id == buffer_id)
            else {
                log::debug!(
                    "[main][buffer_float] rejected unbacked buffer float because hidden core-window creation is unavailable: command={}, buffer_id={}",
                    command_name,
                    buffer_id
                );
                return Err(RuntimeCommandError::CommandFailed {
                    name: command_name.to_string(),
                    message: format!(
                        "hidden core-window creation is not available for buffer-backed floats: buffer_id={buffer_id}"
                    ),
                });
            };
            window.id
        }
        (None, None) => snapshot.active_window_id().ok_or_else(|| {
            log::debug!(
                "[main][buffer_float] rejected buffer float because active window is unavailable: command={}",
                command_name
            );
            RuntimeCommandError::CommandFailed {
                name: command_name.to_string(),
                message: "active window is not available".to_string(),
            }
        })?,
    };
    let Some(window) = snapshot.window(chosen_window_id) else {
        log::debug!(
            "[main][buffer_float] rejected buffer float because backing window is missing: command={}, window_id={}, requested_buffer_id={:?}",
            command_name,
            chosen_window_id,
            requested_buffer_id
        );
        return Err(RuntimeCommandError::CommandFailed {
            name: command_name.to_string(),
            message: format!("window not found: window_id={chosen_window_id}"),
        });
    };
    if let Some(buffer_id) = requested_buffer_id
        && window.buf_id != buffer_id
    {
        log::debug!(
            "[main][buffer_float] rejected buffer float because requested window does not display requested buffer: command={}, window_id={}, window_buffer_id={}, requested_buffer_id={}",
            command_name,
            chosen_window_id,
            window.buf_id,
            buffer_id
        );
        return Err(RuntimeCommandError::CommandFailed {
            name: command_name.to_string(),
            message: format!(
                "backing window {chosen_window_id} displays buffer {}, not requested buffer {buffer_id}; hidden core-window creation is not available",
                window.buf_id
            ),
        });
    }
    log::debug!(
        "[main][buffer_float] resolved existing backing window for buffer float: command={}, window_id={}, buffer_id={}, requested_window_id={:?}, requested_buffer_id={:?}",
        command_name,
        chosen_window_id,
        window.buf_id,
        requested_window_id,
        requested_buffer_id
    );
    Ok((chosen_window_id, window.buf_id))
}

pub fn execute_buffer_window_float_host_command(
    payload: &str,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    floating_window_manager: Option<&mut FloatingWindowManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let manager = floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
        name: "buffer.floatWindow".to_string(),
        message: "floating window manager is not available".to_string(),
    })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "buffer.floatWindow".to_string(),
            message: format!("invalid buffer float payload: {error}"),
        })?;
    let snapshot = outcome.core_bridge.light_snapshot();
    let requested_window_id = value
        .get("windowId")
        .or_else(|| value.get("window_id"))
        .and_then(serde_json::Value::as_i64)
        .map(|id| id as i32);
    let requested_buffer_id = value
        .get("bufferId")
        .or_else(|| value.get("buffer_id"))
        .and_then(serde_json::Value::as_i64)
        .map(|id| id as i32);
    let (window_id, buffer_id) = resolve_buffer_float_backing_window(
        "buffer.floatWindow",
        &snapshot,
        requested_window_id,
        requested_buffer_id,
    )?;
    let width = value
        .get("width")
        .and_then(serde_json::Value::as_u64)
        .map(|width| width as u16)
        .unwrap_or(60)
        .max(1);
    let height = value
        .get("height")
        .and_then(serde_json::Value::as_u64)
        .map(|height| height as u16)
        .unwrap_or(12)
        .max(1);
    let row = value
        .get("row")
        .and_then(serde_json::Value::as_i64)
        .map(|row| row as i16)
        .unwrap_or(1);
    let col = value
        .get("col")
        .or_else(|| value.get("column"))
        .and_then(serde_json::Value::as_i64)
        .map(|col| col as i16)
        .unwrap_or(2);
    let border = match value
        .get("border")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("single")
    {
        "none" | "borderless" => FloatingBorder::None,
        _ => FloatingBorder::Single,
    };
    let id = manager.open_core_window(
        window_id,
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Editor,
            anchor: FloatingAnchor::NorthWest,
            row,
            col,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize { width, height },
        FloatingChrome { border },
        FloatingZIndex::User,
        true,
    );
    manager.focus_float(id);
    log::debug!(
        "[main][buffer_float] buffer float host command applied: float_id={}, window_id={}, buffer_id={}, size=({},{})",
        id.0,
        window_id,
        buffer_id,
        width,
        height
    );
    Ok(RuntimeCommandEffect::default())
}

pub fn execute_terminal_float_host_command(
    payload: &str,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let floating_manager =
        floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "terminal.float".to_string(),
            message: "floating window manager is not available".to_string(),
        })?;
    let terminal_manager =
        terminal_float_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "terminal.float".to_string(),
            message: "terminal float manager is not available".to_string(),
        })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "terminal.float".to_string(),
            message: format!("invalid terminal float payload: {error}"),
        })?;
    let command = value
        .get("command")
        .or_else(|| value.get("cmd"))
        .and_then(serde_json::Value::as_str)
        .filter(|command| !command.trim().is_empty())
        .ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "terminal.float".to_string(),
            message: "terminal command is required".to_string(),
        })?
        .to_string();
    let args = value
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|args| {
            args.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let width = value
        .get("width")
        .and_then(serde_json::Value::as_u64)
        .map(|width| width as u16)
        .unwrap_or(80)
        .max(1);
    let height = value
        .get("height")
        .and_then(serde_json::Value::as_u64)
        .map(|height| height as u16)
        .unwrap_or(16)
        .max(1);
    let row = value
        .get("row")
        .and_then(serde_json::Value::as_i64)
        .map(|row| row as i16)
        .unwrap_or(1);
    let col = value
        .get("col")
        .or_else(|| value.get("column"))
        .and_then(serde_json::Value::as_i64)
        .map(|col| col as i16)
        .unwrap_or(2);
    let border = match value
        .get("border")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("single")
    {
        "none" | "borderless" => FloatingBorder::None,
        _ => FloatingBorder::Single,
    };
    let close_behavior = match value
        .get("closeBehavior")
        .or_else(|| value.get("close_behavior"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("kill")
    {
        "detach" | "detachOnClose" | "detach-on-close" => TerminalFloatCloseBehavior::DetachOnClose,
        _ => TerminalFloatCloseBehavior::KillOnClose,
    };
    let terminal_id = terminal_manager
        .spawn(TerminalFloatSpawnRequest {
            command: command.clone(),
            args: args.clone(),
            width,
            height,
            close_behavior,
        })
        .map_err(|error| RuntimeCommandError::CommandFailed {
            name: "terminal.float".to_string(),
            message: format!("failed to spawn terminal float: {error:?}"),
        })?;
    let float_id = floating_manager.open_terminal(
        terminal_id,
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Editor,
            anchor: FloatingAnchor::NorthWest,
            row,
            col,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: width.saturating_add(if matches!(border, FloatingBorder::Single) {
                2
            } else {
                0
            }),
            height: height.saturating_add(if matches!(border, FloatingBorder::Single) {
                2
            } else {
                0
            }),
        },
        FloatingChrome { border },
        FloatingZIndex::User,
        true,
    );
    floating_manager.focus_float(float_id);
    log::debug!(
        "[main][terminal_float] terminal float host command applied: float_id={}, terminal_id={}, command={}, args={:?}, size=({},{}), close_behavior={:?}",
        float_id.0,
        terminal_id,
        command,
        args,
        width,
        height,
        close_behavior
    );
    Ok(RuntimeCommandEffect::default())
}

pub fn execute_terminal_close_float_host_command(
    payload: &str,
    floating_window_manager: Option<&mut FloatingWindowManager>,
    terminal_float_manager: Option<&mut TerminalFloatManager>,
) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
    let floating_manager =
        floating_window_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "terminal.closeFloat".to_string(),
            message: "floating window manager is not available".to_string(),
        })?;
    let terminal_manager =
        terminal_float_manager.ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "terminal.closeFloat".to_string(),
            message: "terminal float manager is not available".to_string(),
        })?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|error| RuntimeCommandError::CommandFailed {
            name: "terminal.closeFloat".to_string(),
            message: format!("invalid terminal close payload: {error}"),
        })?;
    let terminal_id = value
        .get("terminalId")
        .or_else(|| value.get("terminal_id"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| floating_manager.focused_terminal_id())
        .ok_or_else(|| RuntimeCommandError::CommandFailed {
            name: "terminal.closeFloat".to_string(),
            message: "terminal id is required when no terminal float is focused".to_string(),
        })?;
    let float_id = floating_manager
        .focused_float_id()
        .filter(|_| floating_manager.focused_terminal_id() == Some(terminal_id));
    if let Some(float_id) = float_id {
        floating_manager.close(float_id);
    }
    terminal_manager.close_view(terminal_id).map_err(|error| {
        RuntimeCommandError::CommandFailed {
            name: "terminal.closeFloat".to_string(),
            message: format!("failed to close terminal float: {error:?}"),
        }
    })?;
    log::debug!(
        "[main][terminal_float] terminal float close host command applied: float_id={:?}, terminal_id={}",
        float_id.map(|id| id.0),
        terminal_id
    );
    Ok(RuntimeCommandEffect::default())
}
