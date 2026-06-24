use crate::core::notification_prompt::{
    MessageLineCandidate, MessageLineSource, WorkspaceMessageLineState,
    resolve_workspace_message_line,
};
use crate::presentation::floating_window::FloatingWindowId;
use crate::presentation::screen_model::{CommandLineModel, WorkspaceScreenModel};
use crate::terminal::capability::TerminalCapabilityProfile;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OverlayContentKey {
    Builtin {
        kind: &'static str,
        variant: &'static str,
    },
    MermaidImage {
        buffer_id: i32,
        row: usize,
        digest: String,
    },
    RuntimeRegistered {
        id: String,
    },
}

impl OverlayContentKey {
    pub fn describe(&self) -> String {
        match self {
            Self::Builtin { kind, variant } => format!("builtin:{kind}:{variant}"),
            Self::MermaidImage {
                buffer_id,
                row,
                digest,
            } => format!("mermaid-image:{buffer_id}:{row}:{digest}"),
            Self::RuntimeRegistered { id } => format!("runtime:{id}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayTarget {
    ActivePaneCorner,
    PaneCell {
        window_id: i32,
        row: u16,
        col: u16,
        cell_width: u16,
        cell_height: u16,
    },
    FloatCell {
        float_id: FloatingWindowId,
        row: u16,
        col: u16,
        cell_width: u16,
        cell_height: u16,
    },
    StatusArea,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlaySourceRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationOverlayIntent {
    pub content_key: OverlayContentKey,
    pub target: OverlayTarget,
    pub source_rect: Option<OverlaySourceRect>,
    pub fallback_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePresentationIntent {
    pub content_key: OverlayContentKey,
    pub target: OverlayTarget,
    pub fallback_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresentationState {
    pub message_line: WorkspaceMessageLineState,
    pub command_line: Option<CommandLineModel>,
    pub overlays: Vec<PresentationOverlayIntent>,
}

impl PresentationState {
    pub fn visible_message_text(&self) -> Option<&str> {
        self.message_line.visible_text()
    }
}

pub trait PresentationEffectProjectorService {
    fn project(
        &self,
        workspace: &WorkspaceScreenModel,
        runtime_effects: &[RuntimePresentationIntent],
        capabilities: &TerminalCapabilityProfile,
    ) -> PresentationState;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PresentationEffectProjector;

impl PresentationEffectProjector {
    fn fallback_candidates(
        runtime_effects: &[RuntimePresentationIntent],
        capabilities: &TerminalCapabilityProfile,
    ) -> Vec<MessageLineCandidate> {
        if capabilities.inline_graphics.is_some() {
            return Vec::new();
        }

        let fallback = runtime_effects
            .iter()
            .map(|intent| intent.fallback_text.trim())
            .find(|text| !text.is_empty())
            .map(|text| {
                MessageLineCandidate::legacy(MessageLineSource::RuntimeOverlayFallback, text)
            });
        if let Some(candidate) = fallback.as_ref() {
            log::debug!(
                "[presentation_effect] promoting runtime overlay fallback into message candidates: {}",
                candidate.text
            );
        } else {
            log::debug!(
                "[presentation_effect] runtime overlay fallback suppressed because no non-empty fallback text was available"
            );
        }
        fallback.into_iter().collect()
    }
}

pub fn merge_presentation_message_line(
    workspace: &WorkspaceMessageLineState,
    fallback_candidates: impl IntoIterator<Item = MessageLineCandidate>,
) -> WorkspaceMessageLineState {
    let mut candidates = Vec::new();
    if let Some(visible) = workspace.visible.clone() {
        candidates.push(visible);
    }
    candidates.extend(workspace.suppressed.clone());
    candidates.extend(fallback_candidates);
    resolve_workspace_message_line(candidates)
}

impl PresentationEffectProjectorService for PresentationEffectProjector {
    fn project(
        &self,
        workspace: &WorkspaceScreenModel,
        runtime_effects: &[RuntimePresentationIntent],
        capabilities: &TerminalCapabilityProfile,
    ) -> PresentationState {
        let message_line = merge_presentation_message_line(
            &workspace.message_line,
            Self::fallback_candidates(runtime_effects, capabilities),
        );
        let overlays = if capabilities.inline_graphics.is_some() {
            runtime_effects
                .iter()
                .map(|intent| PresentationOverlayIntent {
                    content_key: intent.content_key.clone(),
                    target: intent.target,
                    source_rect: None,
                    fallback_text: intent.fallback_text.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        log::debug!(
            "[presentation_effect] projected presentation state: message_present={}, command_line_present={}, overlay_count={}",
            message_line.visible_text().is_some(),
            workspace.command_line.is_some(),
            overlays.len()
        );
        PresentationState {
            message_line,
            command_line: workspace.command_line.clone(),
            overlays,
        }
    }
}

#[cfg(test)]
#[path = "effect_test.rs"]
mod tests;
