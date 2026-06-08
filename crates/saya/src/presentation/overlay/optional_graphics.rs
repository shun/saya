use crate::presentation::floating_window::FloatingBorder;
use crate::presentation::overlay::asset_store::OverlayAssetSnapshot;
use crate::presentation::overlay::effect::{
    OverlaySourceRect, OverlayTarget, PresentationOverlayIntent,
};
use crate::presentation::screen_model::{ScreenCursorStyle, WorkspaceScreenModel};
use crate::terminal::capability::{InlineGraphicsProtocol, TerminalCapabilityProfile};
use crate::terminal::io_broker::TerminalIoBroker;
use crate::terminal::lifecycle::TerminalBackend;

pub trait OverlayTerminalWriter {
    fn write_overlay_bytes(&mut self, bytes: &[u8]) -> Result<(), String>;
    fn set_cursor_style(&mut self, style: ScreenCursorStyle) -> Result<(), String>;

    fn write_bell(&mut self, count: usize) -> Result<(), String> {
        if count == 0 {
            return Ok(());
        }
        self.write_overlay_bytes(&vec![b'\x07'; count])
    }
}

#[derive(Debug, Default)]
pub struct RecordingOverlayWriter {
    pub writes: Vec<Vec<u8>>,
    pub cursor_styles: Vec<ScreenCursorStyle>,
}

impl OverlayTerminalWriter for RecordingOverlayWriter {
    fn write_overlay_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.writes.push(bytes.to_vec());
        Ok(())
    }

    fn set_cursor_style(&mut self, style: ScreenCursorStyle) -> Result<(), String> {
        self.cursor_styles.push(style);
        Ok(())
    }
}

impl<B: TerminalBackend> OverlayTerminalWriter for TerminalIoBroker<'_, B> {
    fn write_overlay_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        TerminalIoBroker::write_overlay_bytes(self, bytes).map_err(|error| error.to_string())
    }

    fn write_bell(&mut self, count: usize) -> Result<(), String> {
        TerminalIoBroker::write_bell(self, count).map_err(|error| error.to_string())
    }

    fn set_cursor_style(&mut self, style: ScreenCursorStyle) -> Result<(), String> {
        TerminalIoBroker::set_cursor_style(self, style).map_err(|error| error.to_string())
    }
}

#[derive(Debug)]
pub struct GraphicsOverlayRequest<'a> {
    pub asset: OverlayAssetSnapshot<'a>,
    pub cell_x: u16,
    pub cell_y: u16,
    pub cell_width: u16,
    pub cell_height: u16,
    pub source_rect: Option<OverlaySourceRect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayRenderResult {
    Rendered,
    FallbackToText,
}

pub trait OptionalGraphicsAdapterService {
    fn negotiate(&self, capabilities: &TerminalCapabilityProfile)
    -> Option<InlineGraphicsProtocol>;
    fn clear_overlays(
        &mut self,
        protocol: InlineGraphicsProtocol,
        writer: &mut dyn OverlayTerminalWriter,
    ) -> OverlayRenderResult;
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
            OverlayTarget::PaneCell {
                window_id,
                row,
                col,
                cell_width,
                cell_height,
            } => {
                let pane = workspace
                    .panes
                    .iter()
                    .find(|pane| pane.window_id == window_id)?;
                (
                    pane.rect.x.saturating_add(col),
                    pane.rect.y.saturating_add(row),
                    cell_width.max(1),
                    cell_height.max(1),
                )
            }
            OverlayTarget::FloatCell {
                float_id,
                row,
                col,
                cell_width,
                cell_height,
            } => {
                let float = workspace.floats.iter().find(|float| float.id == float_id)?;
                let border_offset = match float.chrome.border {
                    FloatingBorder::None => 0,
                    FloatingBorder::Single => 1,
                };
                (
                    float
                        .rect
                        .x
                        .saturating_add(border_offset)
                        .saturating_add(col),
                    float
                        .rect
                        .y
                        .saturating_add(border_offset)
                        .saturating_add(row),
                    cell_width.max(1),
                    cell_height.max(1),
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
            source_rect: intent.source_rect,
        })
    }

    fn clear_overlays(
        &mut self,
        protocol: InlineGraphicsProtocol,
        writer: &mut dyn OverlayTerminalWriter,
    ) -> OverlayRenderResult {
        let encoded = match protocol {
            InlineGraphicsProtocol::Kitty => encode_kitty_clear_visible_payload(),
            InlineGraphicsProtocol::Sixel => String::new(),
        };
        if encoded.is_empty() {
            return OverlayRenderResult::Rendered;
        }
        match writer.write_overlay_bytes(encoded.as_bytes()) {
            Ok(()) => {
                log::debug!(
                    "[optional_graphics] overlay clear succeeded: protocol={protocol:?}, bytes={}",
                    encoded.len()
                );
                OverlayRenderResult::Rendered
            }
            Err(error) => {
                log::debug!(
                    "[optional_graphics] overlay clear failed: protocol={protocol:?}, error={error}"
                );
                OverlayRenderResult::FallbackToText
            }
        }
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
            Ok(()) => {
                log::debug!(
                    "[optional_graphics] overlay write succeeded: protocol={protocol:?}, asset_ref={}, bytes={}, cell=({},{} {}x{}), media={}x{}",
                    request.asset.asset_ref.id,
                    encoded.len(),
                    request.cell_x,
                    request.cell_y,
                    request.cell_width,
                    request.cell_height,
                    request.asset.metadata.pixel_width,
                    request.asset.metadata.pixel_height
                );
                OverlayRenderResult::Rendered
            }
            Err(error) => {
                log::debug!(
                    "[optional_graphics] overlay write failed, falling back to text: protocol={protocol:?}, error={error}"
                );
                OverlayRenderResult::FallbackToText
            }
        }
    }
}

fn encode_kitty_clear_visible_payload() -> String {
    "\u{1b}_Ga=d\u{1b}\\".to_string()
}

fn encode_kitty_payload(request: &GraphicsOverlayRequest<'_>) -> String {
    let encoded = base64_payload(request.asset.bytes);
    let mut output = format!(
        "\u{1b}[{};{}H",
        request.cell_y.saturating_add(1),
        request.cell_x.saturating_add(1)
    );
    let mut remaining = encoded.as_str();
    while !remaining.is_empty() {
        let chunk_len = remaining.len().min(KITTY_CHUNK_BYTES);
        let (chunk, rest) = remaining.split_at(chunk_len);
        remaining = rest;
        let more_chunks = !remaining.is_empty();
        let source_rect = request
            .source_rect
            .map(|rect| {
                format!(
                    ",x={},y={},w={},h={}",
                    rect.x, rect.y, rect.width, rect.height
                )
            })
            .unwrap_or_default();
        let mut control = format!(
            "a=T,f=100,s={},v={},c={},r={}",
            request.asset.metadata.pixel_width,
            request.asset.metadata.pixel_height,
            request.cell_width,
            request.cell_height,
        );
        control.push_str(&source_rect);
        control.push_str(&format!(",m={}", if more_chunks { 1 } else { 0 }));
        output.push_str(&format!("\u{1b}_G{control};{chunk}\u{1b}\\"));
    }
    output
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

const KITTY_CHUNK_BYTES: usize = 4096;
const BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_payload(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        encoded.push(BASE64_TABLE[(b0 >> 2) as usize] as char);
        encoded.push(BASE64_TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(BASE64_TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(BASE64_TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::notification_prompt::{MessageLineCandidate, resolve_workspace_message_line};
    use crate::presentation::overlay::asset_store::{OverlayAssetMedia, OverlayAssetRef};
    use crate::presentation::overlay::effect::{
        OverlayContentKey, OverlayTarget, PresentationOverlayIntent,
    };
    use crate::presentation::screen_model::{PaneRect, ScreenCursorStyle, ScreenModel};
    use crate::terminal::capability::{
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
                color_text: true,
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
                status_line: "sample | NORMAL".to_string(),
                cursor_style: ScreenCursorStyle::Block,
                dirty: false,
                lines: vec!["alpha".to_string()],
                line_projections: vec![],
                cursor_row: 0,
                cursor_col: 0,
                visual_selection: None,
                search_overlays: vec![],
                syntax_chunks: vec![],
                markdown_style_ranges: vec![],
                filer_style_ranges: vec![],
                resolved_theme: crate::presentation::theme::ResolvedTheme::default(),
                message_line: None,
                command_cursor_col: None,
                is_active: true,
            }],
            floats: vec![],
            active_window_id: 3,
            message_line: resolve_workspace_message_line(Vec::<MessageLineCandidate>::new()),
            message_area_height: 5,
            message_scroll_offset: 0,
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: None,
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
            source_rect: None,
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

    #[test]
    fn kitty_encoder_uses_base64_payload_and_positions_cursor_before_image() {
        let asset_ref = OverlayAssetRef {
            id: "overlay-1".to_string(),
        };
        let media = OverlayAssetMedia::png("preview", 2, 1, b"png".to_vec());
        let request = GraphicsOverlayRequest {
            asset: OverlayAssetSnapshot {
                asset_ref: &asset_ref,
                metadata: &media.metadata,
                bytes: &media.bytes,
            },
            cell_x: 4,
            cell_y: 2,
            cell_width: 8,
            cell_height: 3,
            source_rect: None,
        };

        let encoded = encode_kitty_payload(&request);

        assert!(
            encoded.starts_with("\u{1b}[3;5H\u{1b}_G"),
            "kitty image should be emitted after cursor positioning, got {encoded:?}"
        );
        assert!(
            encoded.contains("a=T,f=100,s=2,v=1,c=8,r=3,m=0;"),
            "kitty control data should include PNG metadata and final chunk marker: {encoded:?}"
        );
        assert!(
            encoded.contains(";cG5n\u{1b}\\"),
            "kitty payload must be base64 PNG bytes, not hex: {encoded:?}"
        );
        assert!(
            !encoded.contains(";706e67"),
            "kitty payload must not use hex encoding"
        );
    }

    #[test]
    fn kitty_clear_payload_deletes_visible_images() {
        assert_eq!(encode_kitty_clear_visible_payload(), "\u{1b}_Ga=d\u{1b}\\");
    }
}
