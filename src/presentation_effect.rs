use crate::core_notification_prompt::{
    MessageLineCandidate, MessageLineSource, WorkspaceMessageLineState,
    resolve_workspace_message_line,
};
use crate::screen_model::{CommandLineModel, WorkspaceScreenModel};
use crate::terminal_capability::TerminalCapabilityProfile;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OverlayContentKey {
    Builtin {
        kind: &'static str,
        variant: &'static str,
    },
    RuntimeRegistered {
        id: String,
    },
}

impl OverlayContentKey {
    pub fn describe(&self) -> String {
        match self {
            Self::Builtin { kind, variant } => format!("builtin:{kind}:{variant}"),
            Self::RuntimeRegistered { id } => format!("runtime:{id}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayTarget {
    ActivePaneCorner,
    StatusArea,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationOverlayIntent {
    pub content_key: OverlayContentKey,
    pub target: OverlayTarget,
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
mod tests {
    use super::*;
    use crate::core_notification_prompt::{
        MessageLineCandidate, MessageLineSource, resolve_workspace_message_line,
    };
    use crate::screen_model::{PaneRect, ScreenModel};
    use crate::terminal_capability::{
        InlineGraphicsProbeResult, TerminalCapabilityObservation, TerminalCapabilityProbe,
        TerminalCapabilityProbeService, TerminalSessionKind,
    };

    fn workspace_model() -> WorkspaceScreenModel {
        WorkspaceScreenModel {
            panes: vec![ScreenModel {
                window_id: 1,
                buffer_id: 1,
                rect: PaneRect {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 4,
                },
                file_name: "sample.txt".to_string(),
                mode_label: "NORMAL".to_string(),
                dirty: false,
                lines: vec!["alpha".to_string()],
                cursor_row: 0,
                cursor_col: 0,
                visual_selection: None,
                search_overlays: vec![],
                message_line: None,
                command_cursor_col: None,
                is_active: true,
            }],
            active_window_id: 1,
            message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
            command_line: None,
        }
    }

    #[test]
    fn projector_promotes_runtime_fallback_into_message_line_when_graphics_are_disabled() {
        let workspace = workspace_model();
        let capabilities = TerminalCapabilityProbe::new(
            TerminalCapabilityObservation {
                session_kind: TerminalSessionKind::Local,
                basic_terminal_control: true,
                styled_text: false,
                truecolor: false,
            },
            InlineGraphicsProbeResult::Disabled,
        )
        .detect();
        let projector = PresentationEffectProjector;
        let presentation = projector.project(
            &workspace,
            &[RuntimePresentationIntent {
                content_key: OverlayContentKey::RuntimeRegistered {
                    id: "runtime.preview".to_string(),
                },
                target: OverlayTarget::StatusArea,
                fallback_text: "preview unavailable".to_string(),
            }],
            &capabilities,
        );

        assert_eq!(
            presentation.visible_message_text(),
            Some("preview unavailable")
        );
        assert!(presentation.overlays.is_empty());
    }

    #[test]
    fn projector_keeps_core_message_visible_and_retains_runtime_fallback_as_suppressed() {
        let mut workspace = workspace_model();
        workspace.message_line =
            resolve_workspace_message_line(vec![MessageLineCandidate::legacy(
                MessageLineSource::CoreNotification,
                "core note",
            )]);
        let capabilities = TerminalCapabilityProbe::new(
            TerminalCapabilityObservation {
                session_kind: TerminalSessionKind::Local,
                basic_terminal_control: true,
                styled_text: false,
                truecolor: false,
            },
            InlineGraphicsProbeResult::Disabled,
        )
        .detect();
        let projector = PresentationEffectProjector;

        let presentation = projector.project(
            &workspace,
            &[RuntimePresentationIntent {
                content_key: OverlayContentKey::RuntimeRegistered {
                    id: "runtime.preview".to_string(),
                },
                target: OverlayTarget::StatusArea,
                fallback_text: "preview unavailable".to_string(),
            }],
            &capabilities,
        );

        assert_eq!(presentation.visible_message_text(), Some("core note"));
        assert_eq!(
            presentation.message_line.visible_source(),
            Some(MessageLineSource::CoreNotification)
        );
        assert_eq!(
            presentation.message_line.suppressed_sources(),
            vec![MessageLineSource::RuntimeOverlayFallback]
        );
    }
}
