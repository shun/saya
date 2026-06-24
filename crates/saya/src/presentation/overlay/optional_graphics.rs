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
pub struct OptionalGraphicsAdapter;

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
#[path = "optional_graphics_test.rs"]
mod tests;
