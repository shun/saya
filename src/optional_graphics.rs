use crate::overlay_asset_store::OverlayAssetSnapshot;
use crate::presentation_effect::{OverlayTarget, PresentationOverlayIntent};
use crate::screen_model::WorkspaceScreenModel;
use crate::terminal_capability::{InlineGraphicsProtocol, TerminalCapabilityProfile};
use crate::terminal_io_broker::TerminalIoBroker;
use crate::terminal_lifecycle::TerminalBackend;

pub trait OverlayTerminalWriter {
    fn write_overlay_bytes(&mut self, bytes: &[u8]) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct RecordingOverlayWriter {
    pub writes: Vec<Vec<u8>>,
}

impl OverlayTerminalWriter for RecordingOverlayWriter {
    fn write_overlay_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.writes.push(bytes.to_vec());
        Ok(())
    }
}

impl<B: TerminalBackend> OverlayTerminalWriter for TerminalIoBroker<'_, B> {
    fn write_overlay_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        TerminalIoBroker::write_overlay_bytes(self, bytes).map_err(|error| error.to_string())
    }
}

#[derive(Debug)]
pub struct GraphicsOverlayRequest<'a> {
    pub asset: OverlayAssetSnapshot<'a>,
    pub cell_x: u16,
    pub cell_y: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayRenderResult {
    Rendered,
    FallbackToText,
}

pub trait OptionalGraphicsAdapterService {
    fn negotiate(
        &self,
        capabilities: &TerminalCapabilityProfile,
    ) -> Option<InlineGraphicsProtocol>;
    fn project_request<'a>(
        &self,
        intent: &'a PresentationOverlayIntent,
        asset: OverlayAssetSnapshot<'a>,
        workspace: &WorkspaceScreenModel,
    ) -> Option<GraphicsOverlayRequest<'a>>;
    fn render_overlay(
        &mut self,
        request: &GraphicsOverlayRequest<'_>,
        protocol: InlineGraphicsProtocol,
        writer: &mut dyn OverlayTerminalWriter,
    ) -> OverlayRenderResult;
}

#[derive(Debug, Default)]
pub struct OptionalGraphicsAdapter {
    fail_all_renders_for_tests: bool,
}

impl OptionalGraphicsAdapter {
    pub fn new_failing_for_tests() -> Self {
        Self {
            fail_all_renders_for_tests: true,
        }
    }
}

impl OptionalGraphicsAdapterService for OptionalGraphicsAdapter {
    fn negotiate(
        &self,
        capabilities: &TerminalCapabilityProfile,
    ) -> Option<InlineGraphicsProtocol> {
        capabilities.inline_graphics
    }

    fn project_request<'a>(
        &self,
        intent: &'a PresentationOverlayIntent,
        asset: OverlayAssetSnapshot<'a>,
        workspace: &WorkspaceScreenModel,
    ) -> Option<GraphicsOverlayRequest<'a>> {
        let active_pane = workspace
            .panes
            .iter()
            .find(|pane| pane.window_id == workspace.active_window_id)?;
        let (cell_x, cell_y, cell_width, cell_height) = match intent.target {
            OverlayTarget::ActivePaneCorner => {
                let width = active_pane.rect.width.min(12).max(1);
                (
                    active_pane
                        .rect
                        .x
                        .saturating_add(active_pane.rect.width.saturating_sub(width)),
                    active_pane.rect.y,
                    width,
                    1,
                )
            }
            OverlayTarget::StatusArea => {
                let width = active_pane.rect.width.min(12).max(1);
                (
                    active_pane.rect.x,
                    active_pane
                        .rect
                        .y
                        .saturating_add(active_pane.rect.height.saturating_sub(1)),
                    width,
                    1,
                )
            }
        };
        Some(GraphicsOverlayRequest {
            asset,
            cell_x,
            cell_y,
            cell_width,
            cell_height,
        })
    }

    fn render_overlay(
        &mut self,
        request: &GraphicsOverlayRequest<'_>,
        protocol: InlineGraphicsProtocol,
        writer: &mut dyn OverlayTerminalWriter,
    ) -> OverlayRenderResult {
        if self.fail_all_renders_for_tests {
            log::debug!(
                "[optional_graphics] forced overlay failure for test adapter: protocol={protocol:?}, asset_ref={}",
                request.asset.asset_ref.id
            );
            return OverlayRenderResult::FallbackToText;
        }

        let encoded = match protocol {
            InlineGraphicsProtocol::Kitty => encode_kitty_payload(request),
            InlineGraphicsProtocol::Sixel => encode_sixel_payload(request),
        };
        match writer.write_overlay_bytes(encoded.as_bytes()) {
            Ok(()) => OverlayRenderResult::Rendered,
            Err(error) => {
                log::debug!(
                    "[optional_graphics] overlay write failed, falling back to text: protocol={protocol:?}, error={error}"
                );
                OverlayRenderResult::FallbackToText
            }
        }
    }
}

fn encode_kitty_payload(request: &GraphicsOverlayRequest<'_>) -> String {
    format!(
        "\u{1b}_Ga=T,f=100,s={},v={},x={},y={},c={},r={};{}\u{1b}\\",
        request.asset.metadata.pixel_width,
        request.asset.metadata.pixel_height,
        request.cell_x,
        request.cell_y,
        request.cell_width,
        request.cell_height,
        hex_payload(request.asset.bytes)
    )
}

fn encode_sixel_payload(request: &GraphicsOverlayRequest<'_>) -> String {
    format!(
        "\u{1b}Pq\"1;1;{};{}#0;2;{}\u{1b}\\",
        request.asset.metadata.pixel_width,
        request.asset.metadata.pixel_height,
        hex_payload(request.asset.bytes)
    )
}

fn hex_payload(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay_asset_store::{OverlayAssetMedia, OverlayAssetRef};
    use crate::presentation_effect::{OverlayContentKey, OverlayTarget, PresentationOverlayIntent};
    use crate::screen_model::{PaneRect, ScreenModel};
    use crate::terminal_capability::{
        InlineGraphicsProbeResult, TerminalCapabilityObservation, TerminalCapabilityProbe,
        TerminalCapabilityProbeService, TerminalSessionKind,
    };

    #[test]
    fn adapter_negotiates_graphics_only_when_profile_supports_it() {
        let capabilities = TerminalCapabilityProbe::new(
            TerminalCapabilityObservation {
                session_kind: TerminalSessionKind::Local,
                basic_terminal_control: true,
                styled_text: true,
                truecolor: true,
            },
            InlineGraphicsProbeResult::Supported(InlineGraphicsProtocol::Kitty),
        )
        .detect();

        assert_eq!(
            OptionalGraphicsAdapter::default().negotiate(&capabilities),
            Some(InlineGraphicsProtocol::Kitty)
        );
    }

    #[test]
    fn adapter_projects_overlay_request_from_semantic_target() {
        let workspace = WorkspaceScreenModel {
            panes: vec![ScreenModel {
                window_id: 3,
                buffer_id: 1,
                rect: PaneRect {
                    x: 2,
                    y: 1,
                    width: 20,
                    height: 4,
                },
                file_name: "sample".to_string(),
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
            active_window_id: 3,
            global_message_line: None,
            command_line: None,
        };
        let asset_ref = OverlayAssetRef {
            id: "overlay-1".to_string(),
        };
        let media = OverlayAssetMedia::png("preview", 12, 8, b"png".to_vec());
        let intent = PresentationOverlayIntent {
            content_key: OverlayContentKey::RuntimeRegistered {
                id: "runtime.preview".to_string(),
            },
            target: OverlayTarget::ActivePaneCorner,
            fallback_text: "preview unavailable".to_string(),
        };
        let request = OptionalGraphicsAdapter::default()
            .project_request(
                &intent,
                OverlayAssetSnapshot {
                    asset_ref: &asset_ref,
                    metadata: &media.metadata,
                    bytes: &media.bytes,
                },
                &workspace,
            )
            .expect("active pane corner should project");

        assert_eq!(request.cell_y, 1);
        assert_eq!(request.cell_height, 1);
        assert!(request.cell_x >= 2);
    }
}
