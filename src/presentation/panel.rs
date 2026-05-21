use std::collections::BTreeMap;

use crate::presentation::floating_window::{
    FloatingBorder, FloatingChrome, FloatingContentRef, FloatingCursor, FloatingInlineStyle,
    FloatingScreenModel, FloatingWindowId,
};
use crate::presentation::screen_model::PaneRect;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PanelPosition {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PanelCloseBehavior {
    Kill,
    Detach,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PanelSize {
    Cells(u16),
    Percent(u16),
}

impl PanelSize {
    fn resolve(self, total: u16) -> u16 {
        match self {
            Self::Cells(cells) => cells.min(total).max(1),
            Self::Percent(percent) => {
                let clamped = percent.clamp(1, 100);
                let resolved = (u32::from(total) * u32::from(clamped)).div_ceil(100);
                u16::try_from(resolved).unwrap_or(total).min(total).max(1)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelNode {
    Text { text: String },
    Heading { text: String },
    Divider,
    Image { src: String, alt: Option<String> },
    Badge { label: String },
    Progress { label: Option<String>, value: u16 },
    Button { label: String },
}

impl PanelNode {
    fn rendered_line(&self) -> String {
        match self {
            Self::Text { text } | Self::Heading { text } => text.clone(),
            Self::Divider => "--------".to_string(),
            Self::Image { src, alt } => match alt.as_deref().filter(|alt| !alt.is_empty()) {
                Some(alt) => format!("[image: {alt}] {src}"),
                None => format!("[image] {src}"),
            },
            Self::Badge { label } => format!("[{label}]"),
            Self::Progress { label, value } => {
                let clamped = (*value).min(100);
                let filled = usize::from(clamped / 10);
                let empty = 10usize.saturating_sub(filled);
                let bar = format!("{}{}", "#".repeat(filled), "-".repeat(empty));
                match label.as_deref().filter(|label| !label.is_empty()) {
                    Some(label) => format!("{label} [{bar}] {clamped}%"),
                    None => format!("[{bar}] {clamped}%"),
                }
            }
            Self::Button { label } => format!("[ {label} ]"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelContent {
    Lines {
        lines: Vec<String>,
    },
    View {
        nodes: Vec<PanelNode>,
    },
    Terminal {
        terminal_id: u64,
        close_behavior: PanelCloseBehavior,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelContentRef {
    Lines {
        lines: Vec<String>,
    },
    View {
        nodes: Vec<PanelNode>,
    },
    Terminal {
        terminal_id: u64,
        close_behavior: PanelCloseBehavior,
        lines: Vec<String>,
    },
}

impl PanelContentRef {
    pub fn terminal_id(&self) -> Option<u64> {
        match self {
            Self::Terminal { terminal_id, .. } => Some(*terminal_id),
            Self::Lines { .. } | Self::View { .. } => None,
        }
    }

    fn rendered_lines(&self) -> Vec<String> {
        match self {
            Self::Lines { lines } | Self::Terminal { lines, .. } => lines.clone(),
            Self::View { nodes } => nodes.iter().map(PanelNode::rendered_line).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelOpenRequest {
    pub id: String,
    pub position: PanelPosition,
    pub size: PanelSize,
    pub content: PanelContent,
    pub focus: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelOpenResult {
    pub id: String,
    pub numeric_id: u64,
    pub position: PanelPosition,
    pub size: PanelSize,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelSnapshot {
    pub id: String,
    pub numeric_id: u64,
    pub position: PanelPosition,
    pub size: PanelSize,
    pub kind: &'static str,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelScreenModel {
    pub id: u64,
    pub plugin_id: String,
    pub rect: PaneRect,
    pub lines: Vec<String>,
    pub focused: bool,
    pub content: PanelContentRef,
    pub creation_order: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelTerminalViewRequest {
    pub id: String,
    pub terminal_id: u64,
    pub content_height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Panel {
    pub id: String,
    pub numeric_id: u64,
    pub position: PanelPosition,
    pub size: PanelSize,
    pub content: PanelContentRef,
    creation_order: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PanelManager {
    panels: BTreeMap<String, Panel>,
    order: Vec<String>,
    next_id: u64,
    next_creation_order: u64,
    focus: Option<String>,
}

impl PanelManager {
    pub fn open(&mut self, request: PanelOpenRequest) -> PanelOpenResult {
        let id = request.id.trim().to_string();
        let existing = self.panels.get(&id).cloned();
        let numeric_id = existing
            .as_ref()
            .map(|panel| panel.numeric_id)
            .unwrap_or_else(|| self.allocate_id());
        let creation_order = existing
            .as_ref()
            .map(|panel| panel.creation_order)
            .unwrap_or_else(|| self.allocate_creation_order());
        let content = match request.content {
            PanelContent::Lines { lines } => PanelContentRef::Lines { lines },
            PanelContent::View { nodes } => PanelContentRef::View { nodes },
            PanelContent::Terminal {
                terminal_id,
                close_behavior,
            } => PanelContentRef::Terminal {
                terminal_id,
                close_behavior,
                lines: Vec::new(),
            },
        };
        if existing.is_none() {
            self.order.push(id.clone());
        }
        let panel = Panel {
            id: id.clone(),
            numeric_id,
            position: request.position,
            size: request.size,
            content,
            creation_order,
        };
        self.panels.insert(id.clone(), panel);
        if request.focus {
            self.focus = Some(id.clone());
        }
        log::debug!(
            "[panel] open applied: id={}, numeric_id={}, position={:?}, size={:?}, focus={}, replaced={}",
            id,
            numeric_id,
            request.position,
            request.size,
            request.focus,
            existing.is_some()
        );
        PanelOpenResult {
            id,
            numeric_id,
            position: request.position,
            size: request.size,
            focused: request.focus,
        }
    }

    pub fn panel(&self, id: &str) -> Option<&Panel> {
        self.panels.get(id)
    }

    pub fn focused_panel_id(&self) -> Option<&str> {
        self.focus.as_deref()
    }

    pub fn focused_terminal_id(&self) -> Option<u64> {
        let focused = self.focus.as_deref()?;
        self.panels
            .get(focused)
            .and_then(|panel| panel.content.terminal_id())
    }

    pub fn focus(&mut self, id: &str) -> bool {
        let focused = self.panels.contains_key(id);
        if focused {
            self.focus = Some(id.to_string());
        }
        log::debug!("[panel] focus requested: id={}, focused={}", id, focused);
        focused
    }

    pub fn unfocus(&mut self) -> bool {
        let had_focus = self.focus.take().is_some();
        log::debug!("[panel] unfocus requested: had_focus={}", had_focus);
        had_focus
    }

    pub fn close(&mut self, id: &str) -> Option<PanelContentRef> {
        let removed = self.panels.remove(id)?;
        self.order.retain(|existing| existing != id);
        if self.focus.as_deref() == Some(id) {
            self.focus = None;
        }
        log::debug!(
            "[panel] close applied: id={}, numeric_id={}, content={:?}",
            id,
            removed.numeric_id,
            removed.content
        );
        Some(removed.content)
    }

    pub fn snapshots(&self) -> Vec<PanelSnapshot> {
        self.order
            .iter()
            .filter_map(|id| self.panels.get(id))
            .map(|panel| PanelSnapshot {
                id: panel.id.clone(),
                numeric_id: panel.numeric_id,
                position: panel.position,
                size: panel.size,
                kind: match panel.content {
                    PanelContentRef::Lines { .. } => "lines",
                    PanelContentRef::View { .. } => "view",
                    PanelContentRef::Terminal { .. } => "terminal",
                },
                focused: self.focus.as_deref() == Some(panel.id.as_str()),
            })
            .collect()
    }

    pub fn send(&mut self, id: &str, text: &str) -> Result<Option<u64>, String> {
        let Some(panel) = self.panels.get(id) else {
            return Err(format!("unknown panel: {id}"));
        };
        let terminal_id = panel.content.terminal_id();
        log::debug!(
            "[panel] send requested: id={}, terminal_id={:?}, bytes={}",
            id,
            terminal_id,
            text.len()
        );
        Ok(terminal_id)
    }

    pub fn terminal_view_requests(&self) -> Vec<PanelTerminalViewRequest> {
        self.order
            .iter()
            .filter_map(|id| self.panels.get(id))
            .filter_map(|panel| {
                let terminal_id = panel.content.terminal_id()?;
                Some(PanelTerminalViewRequest {
                    id: panel.id.clone(),
                    terminal_id,
                    content_height: 1,
                })
            })
            .collect()
    }

    pub fn replace_terminal_lines(&mut self, id: &str, lines: Vec<String>) -> bool {
        let Some(panel) = self.panels.get_mut(id) else {
            return false;
        };
        let PanelContentRef::Terminal {
            terminal_id,
            close_behavior,
            ..
        } = panel.content
        else {
            return false;
        };
        panel.content = PanelContentRef::Terminal {
            terminal_id,
            close_behavior,
            lines,
        };
        log::trace!(
            "[panel] terminal lines refreshed: id={}, terminal_id={}",
            id,
            terminal_id
        );
        true
    }

    pub fn resolve_screen_models(
        &self,
        terminal_width: u16,
        terminal_height: u16,
    ) -> Vec<PanelScreenModel> {
        self.order
            .iter()
            .filter_map(|id| self.panels.get(id))
            .map(|panel| {
                let rect = panel_rect(panel.position, panel.size, terminal_width, terminal_height);
                PanelScreenModel {
                    id: panel.numeric_id,
                    plugin_id: panel.id.clone(),
                    rect,
                    lines: panel.content.rendered_lines(),
                    focused: self.focus.as_deref() == Some(panel.id.as_str()),
                    content: panel.content.clone(),
                    creation_order: panel.creation_order,
                }
            })
            .collect()
    }

    pub fn resolve_floating_screen_models(
        &self,
        terminal_width: u16,
        terminal_height: u16,
    ) -> Vec<FloatingScreenModel> {
        self.resolve_screen_models(terminal_width, terminal_height)
            .into_iter()
            .map(|panel| FloatingScreenModel {
                id: FloatingWindowId(panel.id),
                content: match panel.content {
                    PanelContentRef::Terminal { terminal_id, .. } => {
                        FloatingContentRef::Terminal { terminal_id }
                    }
                    PanelContentRef::Lines { .. } => FloatingContentRef::StaticLines {
                        content_id: panel.id,
                    },
                    PanelContentRef::View { .. } => FloatingContentRef::StaticLines {
                        content_id: panel.id,
                    },
                },
                rect: panel.rect,
                lines: panel.lines,
                inline_styles: Vec::<FloatingInlineStyle>::new(),
                cursor: panel
                    .focused
                    .then_some(FloatingCursor { line: 0, column: 0 }),
                focusable: true,
                mouse: true,
                chrome: FloatingChrome {
                    border: FloatingBorder::Single,
                },
                zindex: 30,
                creation_order: panel.creation_order,
            })
            .collect()
    }

    fn allocate_id(&mut self) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        self.next_id
    }

    fn allocate_creation_order(&mut self) -> u64 {
        self.next_creation_order = self.next_creation_order.saturating_add(1);
        self.next_creation_order
    }
}

fn panel_rect(
    position: PanelPosition,
    size: PanelSize,
    terminal_width: u16,
    terminal_height: u16,
) -> PaneRect {
    match position {
        PanelPosition::Left => {
            let width = size.resolve(terminal_width);
            PaneRect {
                x: 0,
                y: 0,
                width,
                height: terminal_height,
            }
        }
        PanelPosition::Right => {
            let width = size.resolve(terminal_width);
            PaneRect {
                x: terminal_width.saturating_sub(width),
                y: 0,
                width,
                height: terminal_height,
            }
        }
        PanelPosition::Top => {
            let height = size.resolve(terminal_height);
            PaneRect {
                x: 0,
                y: 0,
                width: terminal_width,
                height,
            }
        }
        PanelPosition::Bottom => {
            let height = size.resolve(terminal_height);
            PaneRect {
                x: 0,
                y: terminal_height.saturating_sub(height),
                width: terminal_width,
                height,
            }
        }
    }
}
