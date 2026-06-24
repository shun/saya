use crate::core::notification_prompt::{MessageLineCandidate, MessageLineSource};
use crate::presentation::floating_window::{
    FloatingImage, FloatingImageSource, FloatingScreenModel,
};
use crate::presentation::markdown::render::{MermaidDiagramRenderer, MmdcMermaidDiagramRenderer};
use crate::presentation::overlay::asset_store::{
    OverlayAssetMedia, OverlayAssetRef, OverlayAssetSource, OverlayAssetStore,
    OverlayAssetStoreService,
};
use crate::presentation::overlay::effect::{
    OverlayContentKey, OverlaySourceRect, OverlayTarget, PresentationEffectProjector,
    PresentationEffectProjectorService, PresentationOverlayIntent, PresentationState,
    RuntimePresentationIntent, merge_presentation_message_line,
};
use crate::presentation::overlay::optional_graphics::{
    OptionalGraphicsAdapter, OptionalGraphicsAdapterService, OverlayRenderResult,
    OverlayTerminalWriter,
};
use crate::presentation::render::renderer::TuiRenderer;
pub use crate::presentation::render::renderer::{RenderFrameOptions, RenderTextMode};
use crate::presentation::screen_model::{
    CommandLineModel, ScreenCursorStyle, WorkspaceScreenModel,
};
use crate::presentation::structural_refresh::{
    ProjectionFailureDiagnostic, RedrawPlan, StructuralRefreshOutcome,
};
use crate::terminal::capability::{TerminalCapabilityProfile, TextStyleCapability};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{
    Arc,
    mpsc::{self, Receiver, TryRecvError},
};
use std::time::Instant;

pub struct RenderFrameRequest<'a> {
    pub workspace: &'a WorkspaceScreenModel,
    pub capabilities: &'a TerminalCapabilityProfile,
    pub presentation: &'a PresentationState,
    pub overlay_writer: Option<&'a mut dyn OverlayTerminalWriter>,
    pub redraw_plan: Option<&'a RedrawPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiRenderOutcome {
    pub rendered_workspace: WorkspaceScreenModel,
    pub text_mode: RenderTextMode,
    pub redraw_plan: RedrawPlan,
    pub frame_options: RenderFrameOptions,
    pub projection_failure: Option<ProjectionFailureDiagnostic>,
    pub overlay_results: Vec<OverlayRenderResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderFrameError {
    Projection { message: String },
    TerminalIo { message: String },
}

impl fmt::Display for RenderFrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Projection { message } => write!(f, "workspace projection failed: {message}"),
            Self::TerminalIo { message } => write!(f, "terminal render failed: {message}"),
        }
    }
}

pub trait TuiRenderCoordinatorService {
    fn render_workspace(
        &mut self,
        request: RenderFrameRequest<'_>,
    ) -> Result<TuiRenderOutcome, RenderFrameError>;
}

pub struct TuiRenderCoordinator {
    renderer: Option<TuiRenderer>,
    projector: PresentationEffectProjector,
    asset_store: OverlayAssetStore,
    graphics_adapter: OptionalGraphicsAdapter,
    mermaid_renderer: Option<Box<dyn MermaidDiagramRenderer>>,
    mermaid_png_cache: BTreeMap<String, MermaidPngCacheEntry>,
    last_frame_had_graphics_overlay: bool,
    last_successful_workspace: Option<WorkspaceScreenModel>,
    last_applied_cursor_style: Option<ScreenCursorStyle>,
    mermaid_redraw_callback: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl TuiRenderCoordinator {
    pub fn new(
        renderer: TuiRenderer,
        asset_store: OverlayAssetStore,
        graphics_adapter: OptionalGraphicsAdapter,
    ) -> Self {
        Self {
            renderer: Some(renderer),
            projector: PresentationEffectProjector,
            asset_store,
            graphics_adapter,
            mermaid_renderer: Some(Box::new(MmdcMermaidDiagramRenderer::default())),
            mermaid_png_cache: BTreeMap::new(),
            last_frame_had_graphics_overlay: false,
            last_successful_workspace: None,
            last_applied_cursor_style: None,
            mermaid_redraw_callback: None,
        }
    }

    pub fn new_headless(
        asset_store: OverlayAssetStore,
        graphics_adapter: OptionalGraphicsAdapter,
    ) -> Self {
        Self {
            renderer: None,
            projector: PresentationEffectProjector,
            asset_store,
            graphics_adapter,
            mermaid_renderer: None,
            mermaid_png_cache: BTreeMap::new(),
            last_frame_had_graphics_overlay: false,
            last_successful_workspace: None,
            last_applied_cursor_style: None,
            mermaid_redraw_callback: None,
        }
    }

    pub fn set_mermaid_redraw_callback(&mut self, callback: impl Fn() + Send + Sync + 'static) {
        self.mermaid_redraw_callback = Some(Arc::new(callback));
    }

    pub fn with_mermaid_renderer(mut self, renderer: Box<dyn MermaidDiagramRenderer>) -> Self {
        self.mermaid_renderer = Some(renderer);
        self
    }

    pub fn render_workspace_result<E: fmt::Display>(
        &mut self,
        render_result: Result<WorkspaceScreenModel, E>,
        capabilities: &TerminalCapabilityProfile,
        runtime_effects: &[RuntimePresentationIntent],
        overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
    ) -> Result<TuiRenderOutcome, RenderFrameError> {
        self.render_workspace_result_with_redraw_plan(
            render_result,
            capabilities,
            runtime_effects,
            overlay_writer,
            RedrawPlan::default(),
        )
    }

    pub fn render_workspace_result_with_redraw_plan<E: fmt::Display>(
        &mut self,
        render_result: Result<WorkspaceScreenModel, E>,
        capabilities: &TerminalCapabilityProfile,
        runtime_effects: &[RuntimePresentationIntent],
        overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
        redraw_plan: RedrawPlan,
    ) -> Result<TuiRenderOutcome, RenderFrameError> {
        self.render_workspace_result_with_context(
            render_result,
            capabilities,
            runtime_effects,
            overlay_writer,
            redraw_plan,
            None,
        )
    }

    pub fn render_workspace_result_with_structural_refresh<E: fmt::Display>(
        &mut self,
        render_result: Result<WorkspaceScreenModel, E>,
        capabilities: &TerminalCapabilityProfile,
        runtime_effects: &[RuntimePresentationIntent],
        overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
        structural_refresh: Option<&StructuralRefreshOutcome>,
    ) -> Result<TuiRenderOutcome, RenderFrameError> {
        let redraw_plan = structural_refresh
            .map(|refresh| refresh.redraw_plan.clone())
            .unwrap_or_default();
        self.render_workspace_result_with_structural_refresh_and_redraw_plan(
            render_result,
            capabilities,
            runtime_effects,
            overlay_writer,
            structural_refresh,
            redraw_plan,
        )
    }

    pub fn render_workspace_result_with_structural_refresh_and_redraw_plan<E: fmt::Display>(
        &mut self,
        render_result: Result<WorkspaceScreenModel, E>,
        capabilities: &TerminalCapabilityProfile,
        runtime_effects: &[RuntimePresentationIntent],
        overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
        structural_refresh: Option<&StructuralRefreshOutcome>,
        redraw_plan: RedrawPlan,
    ) -> Result<TuiRenderOutcome, RenderFrameError> {
        self.render_workspace_result_with_context(
            render_result,
            capabilities,
            runtime_effects,
            overlay_writer,
            redraw_plan,
            structural_refresh,
        )
    }

    fn render_workspace_result_with_context<E: fmt::Display>(
        &mut self,
        render_result: Result<WorkspaceScreenModel, E>,
        capabilities: &TerminalCapabilityProfile,
        runtime_effects: &[RuntimePresentationIntent],
        overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
        redraw_plan: RedrawPlan,
        structural_refresh: Option<&StructuralRefreshOutcome>,
    ) -> Result<TuiRenderOutcome, RenderFrameError> {
        match render_result {
            Ok(workspace) => {
                let presentation =
                    self.projector
                        .project(&workspace, runtime_effects, capabilities);
                self.render_workspace_with_presentation(
                    &workspace,
                    capabilities,
                    &presentation,
                    overlay_writer,
                    redraw_plan,
                    None,
                    true,
                )
            }
            Err(error) => {
                let message = error.to_string();
                let projection_failure = structural_refresh.map(|refresh| {
                    refresh.projection_failure(message.clone(), refresh.viewport_status)
                });
                let Some(last_successful) = self.last_successful_workspace.clone() else {
                    return Err(RenderFrameError::Projection { message });
                };
                let rollback = last_successful;
                let rollback_presentation = PresentationState {
                    message_line: merge_presentation_message_line(
                        &rollback.message_line,
                        [MessageLineCandidate::legacy(
                            MessageLineSource::RenderProjectionError,
                            message,
                        )],
                    ),
                    command_line: rollback.command_line.clone(),
                    overlays: Vec::new(),
                };
                self.render_workspace_with_presentation(
                    &rollback,
                    capabilities,
                    &rollback_presentation,
                    overlay_writer,
                    redraw_plan,
                    projection_failure,
                    false,
                )
            }
        }
    }

    fn resolve_text_mode(
        capabilities: &TerminalCapabilityProfile,
        workspace: &WorkspaceScreenModel,
    ) -> RenderTextMode {
        if workspace_has_syntax_chunks(workspace)
            && matches!(capabilities.text_style, TextStyleCapability::Monochrome)
        {
            log::debug!(
                "[tui_render_coordinator] promoting monochrome terminal profile to truecolor because syntax chunks are present"
            );
            return RenderTextMode::StyledTrueColor;
        }
        match capabilities.text_style {
            TextStyleCapability::Plain => RenderTextMode::Plain,
            TextStyleCapability::Monochrome => RenderTextMode::StyledMonochrome,
            TextStyleCapability::Ansi => RenderTextMode::StyledAnsi,
            TextStyleCapability::TrueColor => RenderTextMode::StyledTrueColor,
        }
    }

    fn apply_presentation(
        workspace: &WorkspaceScreenModel,
        presentation: &PresentationState,
    ) -> WorkspaceScreenModel {
        let mut rendered = workspace.clone();
        rendered.message_line = presentation.message_line.clone();
        rendered.command_line = presentation.command_line.clone();
        rendered
    }

    fn ensure_fallback_message(workspace: &mut WorkspaceScreenModel, fallback_text: &str) {
        if fallback_text.trim().is_empty() {
            return;
        }
        workspace.message_line = merge_presentation_message_line(
            &workspace.message_line,
            [MessageLineCandidate::legacy(
                MessageLineSource::RuntimeOverlayFallback,
                fallback_text,
            )],
        );
    }

    fn collect_floating_image_overlays(
        &mut self,
        workspace: &mut WorkspaceScreenModel,
        capabilities: &TerminalCapabilityProfile,
        overlay_writer_present: bool,
        overlay_results: &mut Vec<OverlayRenderResult>,
    ) -> Vec<PresentationOverlayIntent> {
        let Some(protocol) = self.graphics_adapter.negotiate(capabilities) else {
            apply_floating_mermaid_source_fallbacks(workspace, "graphics-unavailable");
            log::debug!(
                "[tui_render_coordinator] floating image overlays skipped because inline graphics are unavailable"
            );
            return Vec::new();
        };
        if !overlay_writer_present {
            apply_floating_mermaid_source_fallbacks(workspace, "overlay-writer-unavailable");
            log::debug!(
                "[tui_render_coordinator] floating image overlays skipped because no overlay writer is available: protocol={protocol:?}"
            );
            return Vec::new();
        }

        let mut overlays = Vec::new();
        let mut fallback_messages = Vec::new();
        for float in &mut workspace.floats {
            let images = float.images.clone();
            for image in &images {
                match &image.source {
                    FloatingImageSource::Mermaid {
                        buffer_id,
                        row,
                        alt_text,
                        background,
                        source,
                    } => {
                        let digest = digest_mermaid_source(source, background);
                        let key = OverlayContentKey::MermaidImage {
                            buffer_id: *buffer_id,
                            row: *row,
                            digest: digest.clone(),
                        };
                        let media_result = match self.poll_mermaid_png_cache(&digest) {
                            Some(cached) => {
                                log::debug!(
                                    "[tui_render_coordinator] reused cached floating mermaid image render: digest={}, key={}, float_id={}",
                                    digest,
                                    key.describe(),
                                    float.id.0
                                );
                                cached
                            }
                            None if self.mermaid_png_cache.contains_key(&digest) => {
                                apply_floating_mermaid_pending_fallback(float, image);
                                log::debug!(
                                    "[tui_render_coordinator] floating mermaid image pending fallback applied: digest={}, float_id={}, row={}",
                                    digest,
                                    float.id.0,
                                    row
                                );
                                overlay_results.push(OverlayRenderResult::FallbackToText);
                                continue;
                            }
                            None => {
                                let Some(renderer) = self.mermaid_renderer.as_deref() else {
                                    apply_floating_mermaid_source_fallback(float, image, source);
                                    log::debug!(
                                        "[tui_render_coordinator] floating mermaid image fallback because no renderer is configured: float_id={}, row={}",
                                        float.id.0,
                                        row
                                    );
                                    overlay_results.push(OverlayRenderResult::FallbackToText);
                                    continue;
                                };
                                log::debug!(
                                    "[tui_render_coordinator] rendering floating mermaid image because cache missed: key={}, float_id={}, background={:?}, source_bytes={}",
                                    key.describe(),
                                    float.id.0,
                                    background,
                                    source.len()
                                );
                                let Some(receiver) =
                                    renderer.render_png_async(source.clone(), background.clone())
                                else {
                                    apply_floating_mermaid_source_fallback(float, image, source);
                                    log::debug!(
                                        "[tui_render_coordinator] floating mermaid image fallback because renderer has no async path: float_id={}, row={}",
                                        float.id.0,
                                        row
                                    );
                                    overlay_results.push(OverlayRenderResult::FallbackToText);
                                    continue;
                                };
                                log::debug!(
                                    "[tui_render_coordinator] started async floating mermaid image render: digest={}, key={}, float_id={}",
                                    digest,
                                    key.describe(),
                                    float.id.0
                                );
                                let receiver = self.wrap_mermaid_async_receiver(receiver);
                                self.mermaid_png_cache.insert(
                                    digest.clone(),
                                    MermaidPngCacheEntry::Pending { receiver },
                                );
                                let Some(result) = self.poll_mermaid_png_cache(&digest) else {
                                    apply_floating_mermaid_pending_fallback(float, image);
                                    log::debug!(
                                        "[tui_render_coordinator] floating mermaid image pending fallback applied after async start: digest={}, float_id={}, row={}",
                                        digest,
                                        float.id.0,
                                        row
                                    );
                                    overlay_results.push(OverlayRenderResult::FallbackToText);
                                    continue;
                                };
                                result
                            }
                        };
                        match media_result {
                            Ok(media) => {
                                let placement = resolve_floating_image_placement(&media, image);
                                clear_floating_image_area(float, image);
                                self.asset_store
                                    .register_asset(key.clone(), OverlayAssetSource::Static(media));
                                overlays.push(PresentationOverlayIntent {
                                    content_key: key,
                                    target: OverlayTarget::FloatCell {
                                        float_id: float.id,
                                        row: image.line,
                                        col: image.column,
                                        cell_width: placement.cell_width,
                                        cell_height: placement.cell_height,
                                    },
                                    source_rect: placement.source_rect,
                                    fallback_text: source.clone(),
                                });
                                log::debug!(
                                    "[tui_render_coordinator] floating mermaid image overlay registered: float_id={}, buffer_id={}, row={}, protocol={protocol:?}, source_bytes={}, cells={}x{}, source_rect={:?}",
                                    float.id.0,
                                    buffer_id,
                                    row,
                                    source.len(),
                                    placement.cell_width,
                                    placement.cell_height,
                                    placement.source_rect
                                );
                            }
                            Err(error) => {
                                let summary = summarize_mermaid_render_error(&error);
                                let diagnostics =
                                    mermaid_render_error_popup_lines(&error, source, *row);
                                apply_floating_mermaid_error_fallback(
                                    float,
                                    image,
                                    diagnostics,
                                    source,
                                );
                                fallback_messages.push(summary);
                                log::debug!(
                                    "[tui_render_coordinator] floating mermaid renderer failed, keeping text-only float fallback: float_id={}, row={}, error={error}",
                                    float.id.0,
                                    row
                                );
                                let _ = alt_text;
                                overlay_results.push(OverlayRenderResult::FallbackToText);
                            }
                        }
                    }
                }
            }
        }
        for message in fallback_messages {
            Self::ensure_fallback_message(workspace, &message);
        }
        overlays
    }

    fn poll_mermaid_png_cache(
        &mut self,
        digest: &str,
    ) -> Option<Result<OverlayAssetMedia, String>> {
        let entry = self.mermaid_png_cache.get_mut(digest)?;
        match entry {
            MermaidPngCacheEntry::Ready(result) => Some(result.clone()),
            MermaidPngCacheEntry::Pending { receiver } => match receiver.try_recv() {
                Ok(result) => {
                    log::debug!(
                        "[tui_render_coordinator] async floating mermaid image render completed: digest={digest}, success={}",
                        result.is_ok()
                    );
                    *entry = MermaidPngCacheEntry::Ready(result.clone());
                    Some(result)
                }
                Err(TryRecvError::Empty) => {
                    log::debug!(
                        "[tui_render_coordinator] async floating mermaid image render still pending: digest={digest}"
                    );
                    None
                }
                Err(TryRecvError::Disconnected) => {
                    let result = Err("async mermaid renderer disconnected".to_string());
                    log::debug!(
                        "[tui_render_coordinator] async floating mermaid image render disconnected: digest={digest}"
                    );
                    *entry = MermaidPngCacheEntry::Ready(result.clone());
                    Some(result)
                }
            },
        }
    }

    fn wrap_mermaid_async_receiver(
        &self,
        receiver: Receiver<Result<OverlayAssetMedia, String>>,
    ) -> Receiver<Result<OverlayAssetMedia, String>> {
        let Some(callback) = self.mermaid_redraw_callback.clone() else {
            return receiver;
        };
        let (sender, wrapped_receiver) = mpsc::channel();
        std::thread::spawn(move || match receiver.recv() {
            Ok(result) => {
                let send_result = sender.send(result);
                callback();
                if send_result.is_err() {
                    log::debug!(
                        "[tui_render_coordinator] async mermaid redraw callback fired after receiver was dropped"
                    );
                }
            }
            Err(error) => {
                log::debug!(
                    "[tui_render_coordinator] async mermaid receiver closed before completion: error={error}"
                );
                callback();
            }
        });
        wrapped_receiver
    }

    pub fn render_command_line_overlay(
        &mut self,
        command_line: &CommandLineModel,
        mut overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
    ) -> Result<(), RenderFrameError> {
        if let Some(writer) = overlay_writer.as_deref_mut() {
            self.apply_cursor_style_if_changed(writer, ScreenCursorStyle::SteadyBar)?;
        }
        if let Some(renderer) = self.renderer.as_mut() {
            renderer
                .draw_command_line_overlay(command_line)
                .map_err(|error| RenderFrameError::TerminalIo {
                    message: error.to_string(),
                })?;
        }
        log::debug!(
            "[tui_render_coordinator] rendered command-line-only overlay: text_len={}, cursor_col={}",
            command_line.text.len(),
            command_line.cursor_col
        );
        Ok(())
    }

    fn apply_cursor_style_if_changed(
        &mut self,
        writer: &mut dyn OverlayTerminalWriter,
        cursor_style: ScreenCursorStyle,
    ) -> Result<(), RenderFrameError> {
        if self.last_applied_cursor_style == Some(cursor_style) {
            log::debug!(
                "[tui_render_coordinator] skipped unchanged cursor style: style={cursor_style:?}"
            );
            return Ok(());
        }
        writer
            .set_cursor_style(cursor_style)
            .map_err(|message| RenderFrameError::TerminalIo { message })?;
        self.last_applied_cursor_style = Some(cursor_style);
        log::debug!(
            "[tui_render_coordinator] applied cursor style before rendering: style={cursor_style:?}"
        );
        Ok(())
    }

    fn render_workspace_with_presentation(
        &mut self,
        workspace: &WorkspaceScreenModel,
        capabilities: &TerminalCapabilityProfile,
        presentation: &PresentationState,
        overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
        redraw_plan: RedrawPlan,
        projection_failure: Option<ProjectionFailureDiagnostic>,
        update_last_successful_workspace: bool,
    ) -> Result<TuiRenderOutcome, RenderFrameError> {
        self.render_workspace_inner(
            workspace,
            capabilities,
            presentation,
            overlay_writer,
            redraw_plan,
            projection_failure,
            update_last_successful_workspace,
        )
    }

    fn render_workspace_inner(
        &mut self,
        workspace: &WorkspaceScreenModel,
        capabilities: &TerminalCapabilityProfile,
        presentation: &PresentationState,
        mut overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
        redraw_plan: RedrawPlan,
        projection_failure: Option<ProjectionFailureDiagnostic>,
        update_last_successful_workspace: bool,
    ) -> Result<TuiRenderOutcome, RenderFrameError> {
        let total_started_at = Instant::now();
        let text_mode = Self::resolve_text_mode(capabilities, workspace);
        log::debug!(
            "[tui_render_coordinator] resolved render text mode: text_mode={text_mode:?}, terminal_text_style={:?}, syntax_chunks_present={}",
            capabilities.text_style,
            workspace_has_syntax_chunks(workspace)
        );
        let frame_options = frame_options_from_redraw_plan(&redraw_plan);
        let presentation_started_at = Instant::now();
        let mut rendered_workspace = Self::apply_presentation(workspace, presentation);
        let presentation_ms = presentation_started_at.elapsed().as_millis();
        let mut overlay_results = Vec::new();
        let mut active_assets = Vec::<OverlayAssetRef>::new();
        let mut frame_overlays = presentation.overlays.clone();
        let floating_image_overlays = self.collect_floating_image_overlays(
            &mut rendered_workspace,
            capabilities,
            overlay_writer.is_some(),
            &mut overlay_results,
        );
        frame_overlays.extend(floating_image_overlays);
        let mut prepared_overlays = Vec::<PreparedFrameOverlay>::new();

        if let Some(writer) = overlay_writer.as_deref_mut() {
            let cursor_style = rendered_workspace.active_cursor_style();
            self.apply_cursor_style_if_changed(writer, cursor_style)?;
        }

        if let Some(bell) = rendered_workspace.bell
            && let Some(writer) = overlay_writer.as_deref_mut()
        {
            writer
                .write_bell(bell.count)
                .map_err(|message| RenderFrameError::TerminalIo { message })?;
            log::debug!(
                "[tui_render_coordinator] emitted terminal bell signal: count={}",
                bell.count
            );
        }

        if (self.last_frame_had_graphics_overlay || !frame_overlays.is_empty())
            && let Some(protocol) = self.graphics_adapter.negotiate(capabilities)
            && let Some(writer) = overlay_writer.as_deref_mut()
        {
            let clear_result = self.graphics_adapter.clear_overlays(protocol, writer);
            log::debug!(
                "[tui_render_coordinator] requested optional graphics clear before text draw: protocol={protocol:?}, result={clear_result:?}, previous_had_graphics={}, current_overlay_count={}",
                self.last_frame_had_graphics_overlay,
                frame_overlays.len()
            );
        }

        for overlay in &frame_overlays {
            let asset_ref = match self.asset_store.materialize(&overlay.content_key) {
                Ok(asset_ref) => asset_ref,
                Err(error) => {
                    log::debug!(
                        "[tui_render_coordinator] overlay asset materialization failed, using text fallback: key={}, error={error}",
                        overlay.content_key.describe()
                    );
                    overlay_results.push(OverlayRenderResult::FallbackToText);
                    Self::ensure_fallback_message(&mut rendered_workspace, &overlay.fallback_text);
                    continue;
                }
            };
            let Some(protocol) = self.graphics_adapter.negotiate(capabilities) else {
                overlay_results.push(OverlayRenderResult::FallbackToText);
                Self::ensure_fallback_message(&mut rendered_workspace, &overlay.fallback_text);
                continue;
            };
            if overlay_writer.is_none() {
                overlay_results.push(OverlayRenderResult::FallbackToText);
                Self::ensure_fallback_message(&mut rendered_workspace, &overlay.fallback_text);
                continue;
            }
            let snapshot = match self.asset_store.resolve(&asset_ref) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    log::debug!(
                        "[tui_render_coordinator] overlay asset resolve failed, using text fallback: asset_ref={}, error={error}",
                        asset_ref.id
                    );
                    overlay_results.push(OverlayRenderResult::FallbackToText);
                    Self::ensure_fallback_message(&mut rendered_workspace, &overlay.fallback_text);
                    continue;
                }
            };
            let Some(graphics_request) =
                self.graphics_adapter
                    .project_request(overlay, snapshot, &rendered_workspace)
            else {
                overlay_results.push(OverlayRenderResult::FallbackToText);
                Self::ensure_fallback_message(&mut rendered_workspace, &overlay.fallback_text);
                continue;
            };
            log::debug!(
                "[tui_render_coordinator] prepared overlay for post-text draw: key={}, asset_ref={}, protocol={protocol:?}, cell=({},{} {}x{})",
                overlay.content_key.describe(),
                asset_ref.id,
                graphics_request.cell_x,
                graphics_request.cell_y,
                graphics_request.cell_width,
                graphics_request.cell_height
            );
            prepared_overlays.push(PreparedFrameOverlay {
                intent: overlay.clone(),
                asset_ref,
                protocol,
            });
        }

        let draw_started_at = Instant::now();
        if let Some(renderer) = self.renderer.as_mut() {
            renderer
                .draw_with_mode_and_options(&rendered_workspace, text_mode, frame_options)
                .map_err(|error| RenderFrameError::TerminalIo {
                    message: error.to_string(),
                })?;
        }
        let draw_ms = draw_started_at.elapsed().as_millis();

        let mut redraw_after_overlay_fallback = false;
        for prepared in &prepared_overlays {
            let snapshot = match self.asset_store.resolve(&prepared.asset_ref) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    log::debug!(
                        "[tui_render_coordinator] overlay asset resolve failed after text draw, using text fallback: asset_ref={}, error={error}",
                        prepared.asset_ref.id
                    );
                    overlay_results.push(OverlayRenderResult::FallbackToText);
                    Self::ensure_fallback_message(
                        &mut rendered_workspace,
                        &prepared.intent.fallback_text,
                    );
                    redraw_after_overlay_fallback = true;
                    continue;
                }
            };
            let Some(graphics_request) = self.graphics_adapter.project_request(
                &prepared.intent,
                snapshot,
                &rendered_workspace,
            ) else {
                overlay_results.push(OverlayRenderResult::FallbackToText);
                Self::ensure_fallback_message(
                    &mut rendered_workspace,
                    &prepared.intent.fallback_text,
                );
                redraw_after_overlay_fallback = true;
                continue;
            };
            let Some(writer) = overlay_writer.as_deref_mut() else {
                overlay_results.push(OverlayRenderResult::FallbackToText);
                Self::ensure_fallback_message(
                    &mut rendered_workspace,
                    &prepared.intent.fallback_text,
                );
                redraw_after_overlay_fallback = true;
                continue;
            };
            let result =
                self.graphics_adapter
                    .render_overlay(&graphics_request, prepared.protocol, writer);
            if result == OverlayRenderResult::Rendered {
                log::debug!(
                    "[tui_render_coordinator] overlay render succeeded: key={}, asset_ref={}, protocol={protocol:?}, target={:?}",
                    prepared.intent.content_key.describe(),
                    prepared.asset_ref.id,
                    prepared.intent.target,
                    protocol = prepared.protocol
                );
                active_assets.push(prepared.asset_ref.clone());
            } else {
                log::debug!(
                    "[tui_render_coordinator] overlay render fell back to text: key={}, asset_ref={}, protocol={protocol:?}, target={:?}",
                    prepared.intent.content_key.describe(),
                    prepared.asset_ref.id,
                    prepared.intent.target,
                    protocol = prepared.protocol
                );
                Self::ensure_fallback_message(
                    &mut rendered_workspace,
                    &prepared.intent.fallback_text,
                );
                redraw_after_overlay_fallback = true;
            }
            overlay_results.push(result);
        }
        if redraw_after_overlay_fallback {
            if let Some(renderer) = self.renderer.as_mut() {
                renderer
                    .draw_with_mode_and_options(&rendered_workspace, text_mode, frame_options)
                    .map_err(|error| RenderFrameError::TerminalIo {
                        message: error.to_string(),
                    })?;
            }
            log::debug!(
                "[tui_render_coordinator] redrew text frame after overlay fallback changed visible fallback state"
            );
        }
        if !active_assets.is_empty()
            && let Some(writer) = overlay_writer.as_deref_mut()
        {
            Self::restore_cursor_after_optional_overlays(&rendered_workspace, writer)?;
        }
        self.asset_store.release_unused(&active_assets);
        self.last_frame_had_graphics_overlay = !active_assets.is_empty();
        log::debug!(
            "[PERF][tui_render_coordinator] render_workspace panes={} visible_lines={} presentation_ms={} draw_ms={} total_ms={}",
            rendered_workspace.panes.len(),
            rendered_workspace
                .panes
                .iter()
                .map(|pane| pane.lines.len())
                .sum::<usize>(),
            presentation_ms,
            draw_ms,
            total_started_at.elapsed().as_millis()
        );
        if update_last_successful_workspace {
            self.last_successful_workspace = Some(rendered_workspace.clone());
        } else {
            log::debug!(
                "[tui_render_coordinator] retained last successful workspace after rollback render"
            );
        }
        Ok(TuiRenderOutcome {
            rendered_workspace,
            text_mode,
            redraw_plan,
            frame_options,
            projection_failure,
            overlay_results,
        })
    }

    fn restore_cursor_after_optional_overlays(
        workspace: &WorkspaceScreenModel,
        writer: &mut dyn OverlayTerminalWriter,
    ) -> Result<(), RenderFrameError> {
        if workspace.command_line.is_some() {
            log::debug!(
                "[tui_render_coordinator] skipped cursor restore after optional overlays because command-line cursor placement is owned by the text renderer"
            );
            return Ok(());
        }
        let Some(active_pane) = workspace
            .panes
            .iter()
            .find(|pane| pane.window_id == workspace.active_window_id)
        else {
            log::debug!(
                "[tui_render_coordinator] skipped cursor restore after optional overlays because active pane was not found: active_window_id={}",
                workspace.active_window_id
            );
            return Ok(());
        };
        let body_height = active_pane.rect.height.saturating_sub(1);
        if active_pane.cursor_row >= body_height {
            log::debug!(
                "[tui_render_coordinator] skipped cursor restore after optional overlays because cursor is outside pane body: window_id={}, cursor_row={}, body_height={}",
                active_pane.window_id,
                active_pane.cursor_row,
                body_height
            );
            return Ok(());
        }
        let cell_x = active_pane.rect.x.saturating_add(active_pane.cursor_col);
        let cell_y = active_pane.rect.y.saturating_add(active_pane.cursor_row);
        let payload = format!(
            "\u{1b}[{};{}H",
            cell_y.saturating_add(1),
            cell_x.saturating_add(1)
        );
        writer
            .write_overlay_bytes(payload.as_bytes())
            .map_err(|message| RenderFrameError::TerminalIo { message })?;
        log::debug!(
            "[tui_render_coordinator] restored cursor after optional overlays: window_id={}, cell=({},{}), bytes={}",
            active_pane.window_id,
            cell_x,
            cell_y,
            payload.len()
        );
        Ok(())
    }
}

fn frame_options_from_redraw_plan(redraw_plan: &RedrawPlan) -> RenderFrameOptions {
    let options = RenderFrameOptions {
        full_redraw: redraw_plan.full,
        clear_before_draw: redraw_plan.clear_before_draw,
    };
    log::debug!(
        "[tui_render_coordinator] resolved frame options from RedrawPlan: requested={}, full={}, clear_before_draw={}, source={:?}, coalesced_count={}",
        redraw_plan.requested,
        redraw_plan.full,
        redraw_plan.clear_before_draw,
        redraw_plan.source,
        redraw_plan.coalesced_count
    );
    options
}

fn workspace_has_syntax_chunks(workspace: &WorkspaceScreenModel) -> bool {
    workspace
        .panes
        .iter()
        .any(|pane| !pane.syntax_chunks.is_empty())
}

#[derive(Debug, Clone)]
struct PreparedFrameOverlay {
    intent: PresentationOverlayIntent,
    asset_ref: OverlayAssetRef,
    protocol: crate::terminal::capability::InlineGraphicsProtocol,
}

enum MermaidPngCacheEntry {
    Ready(Result<OverlayAssetMedia, String>),
    Pending {
        receiver: Receiver<Result<OverlayAssetMedia, String>>,
    },
}

fn digest_mermaid_source(source: &str, background: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(background.as_bytes());
    hasher.update(b"\0");
    hasher.update(source.as_bytes());
    let digest = hasher.finalize();
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn apply_floating_mermaid_source_fallbacks(workspace: &mut WorkspaceScreenModel, reason: &str) {
    for float in &mut workspace.floats {
        let images = float.images.clone();
        for image in &images {
            let FloatingImageSource::Mermaid { source, row, .. } = &image.source;
            apply_floating_mermaid_source_fallback(float, image, source);
            log::debug!(
                "[tui_render_coordinator] floating mermaid source fallback applied: reason={reason}, float_id={}, row={}, source_bytes={}",
                float.id.0,
                row,
                source.len()
            );
        }
    }
}

fn apply_floating_mermaid_pending_fallback(float: &mut FloatingScreenModel, image: &FloatingImage) {
    write_floating_image_area_lines(
        float,
        image,
        std::iter::once("Rendering Mermaid preview...".to_string()),
    );
}

fn apply_floating_mermaid_source_fallback(
    float: &mut FloatingScreenModel,
    image: &FloatingImage,
    source: &str,
) {
    write_floating_image_area_lines(float, image, source.lines().map(str::to_string));
}

fn apply_floating_mermaid_error_fallback(
    float: &mut FloatingScreenModel,
    image: &FloatingImage,
    diagnostics: Vec<String>,
    source: &str,
) {
    write_floating_image_area_lines(
        float,
        image,
        diagnostics
            .into_iter()
            .chain(std::iter::once(String::new()))
            .chain(source.lines().map(str::to_string)),
    );
}

fn mermaid_render_error_popup_lines(error: &str, source: &str, fence_row: usize) -> Vec<String> {
    let detail = mermaid_error_detail(error);
    let mut lines = vec!["Mermaid render failed".to_string()];
    let parse_line = parse_mermaid_error_line_number(detail);
    if let Some(parse_line) = parse_line {
        lines.push(format!("Mermaid line {parse_line}"));
        if let Some(source_line) = source.lines().nth(parse_line.saturating_sub(1)) {
            if !source_line.trim().is_empty() {
                lines.push(format!("line {parse_line}: {}", source_line.trim()));
            }
        }
    } else if !detail.is_empty() {
        lines.push(detail.to_string());
    }
    if detail.contains("got 'STYLE_SEPARATOR'")
        && let Some(fix) = mermaid_style_separator_fix(source, parse_line)
    {
        lines.push(format!(
            "Fix editor line {}",
            mermaid_source_line_to_editor_line(fence_row, fix.line)
        ));
        lines.push(fix.source_line);
        lines.push("remove space before :::".to_string());
    }
    if let Some(hint) = mermaid_error_hint(detail, source, parse_line) {
        lines.push(hint.to_string());
    }
    if let Some(expected) = detail
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("Expecting "))
    {
        lines.push(expected.to_string());
    }
    lines
}

fn mermaid_source_line_to_editor_line(fence_row: usize, source_line: usize) -> usize {
    fence_row + source_line + 1
}

fn summarize_mermaid_render_error(error: &str) -> String {
    let detail = mermaid_error_detail(error)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let detail = detail
        .strip_prefix("stderr=")
        .unwrap_or(detail)
        .trim()
        .trim_end_matches('.');
    if detail.is_empty() {
        "Mermaid render failed".to_string()
    } else {
        format!("Mermaid render failed: {detail}")
    }
}

fn mermaid_error_detail(error: &str) -> &str {
    if let Some((_, stderr)) = error.split_once("stderr=") {
        stderr.trim()
    } else {
        error.trim()
    }
}

fn parse_mermaid_error_line_number(detail: &str) -> Option<usize> {
    let rest = if let Some((_, rest)) = detail.split_once("Parse error on line ") {
        rest
    } else {
        let (_, rest) = detail.split_once("Lexical error on line ")?;
        rest
    };
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn mermaid_error_hint(
    detail: &str,
    source: &str,
    parse_line: Option<usize>,
) -> Option<&'static str> {
    if detail.contains("got 'STYLE_SEPARATOR'") {
        Some("Hint: use Node:::class (no space before :::)")
    } else if detail.contains("Expecting 'SPACE', got 'UNICODE_TEXT'") {
        Some("Hint: use classDef Name ... with a space")
    } else if parse_line
        .and_then(|line| source.lines().nth(line.saturating_sub(1)))
        .map(|line| line.trim_start().starts_with("subgraph ") && !line.contains('['))
        .unwrap_or(false)
    {
        Some("Hint: use subgraph id[Label]")
    } else {
        None
    }
}

struct MermaidStyleSeparatorFix {
    line: usize,
    source_line: String,
}

fn mermaid_style_separator_fix(
    source: &str,
    parse_line: Option<usize>,
) -> Option<MermaidStyleSeparatorFix> {
    let candidates: Vec<(usize, &str)> = source.lines().enumerate().collect();
    let mut matches = candidates
        .iter()
        .filter(|(_, line)| line.contains(" :::"))
        .map(|(index, line)| (*index + 1, *line))
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return None;
    }
    if let Some(parse_line) = parse_line {
        matches.sort_by_key(|(line, _)| line.abs_diff(parse_line));
    }
    let (line, source_line) = matches[0];
    Some(MermaidStyleSeparatorFix {
        line,
        source_line: source_line.trim().to_string(),
    })
}

fn clear_floating_image_area(float: &mut FloatingScreenModel, image: &FloatingImage) {
    let width = image.max_width.max(1) as usize;
    let rows = image.max_height.max(1) as usize;
    write_floating_image_area_lines(float, image, std::iter::repeat_n(" ".repeat(width), rows));
}

fn write_floating_image_area_lines(
    float: &mut FloatingScreenModel,
    image: &FloatingImage,
    lines: impl IntoIterator<Item = String>,
) {
    let start = image.line as usize;
    let rows = image.max_height.max(1) as usize;
    let width = image.max_width.max(1) as usize;
    if float.lines.len() < start + rows {
        float.lines.resize(start + rows, String::new());
    }

    let mut incoming = lines.into_iter();
    for index in 0..rows {
        let next = incoming.next().unwrap_or_else(|| " ".repeat(width));
        float.lines[start + index] = clip_display_line(&next, width);
    }
}

fn clip_display_line(line: &str, width: usize) -> String {
    let clipped: String = line.chars().take(width).collect();
    if clipped.is_empty() {
        " ".repeat(width)
    } else {
        clipped
    }
}

fn estimate_markdown_image_cell_height(
    media: &OverlayAssetMedia,
    cell_width: u16,
    max_height: u16,
) -> u16 {
    let pixel_width = media.metadata.pixel_width.max(1);
    let pixel_height = media.metadata.pixel_height.max(1);
    let estimated = (u32::from(cell_width.max(1)) * pixel_height)
        .div_ceil(pixel_width.saturating_mul(2).max(1));
    u16::try_from(estimated)
        .unwrap_or(u16::MAX)
        .clamp(1, max_height.max(1))
}

fn estimate_markdown_image_cell_width(media: &OverlayAssetMedia, max_width: u16) -> u16 {
    let pixel_width = media.metadata.pixel_width.max(1);
    let estimated = pixel_width.div_ceil(16);
    u16::try_from(estimated)
        .unwrap_or(u16::MAX)
        .clamp(8, max_width.max(1))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FloatingImagePlacement {
    cell_width: u16,
    cell_height: u16,
    source_rect: Option<OverlaySourceRect>,
}

fn resolve_floating_image_placement(
    media: &OverlayAssetMedia,
    image: &FloatingImage,
) -> FloatingImagePlacement {
    let Some(zoom_percent) = image.view.zoom_percent else {
        let cell_width = estimate_markdown_image_cell_width(media, image.max_width);
        let cell_height = estimate_markdown_image_cell_height(media, cell_width, image.max_height);
        return FloatingImagePlacement {
            cell_width,
            cell_height,
            source_rect: None,
        };
    };

    let pixel_width = media.metadata.pixel_width.max(1);
    let pixel_height = media.metadata.pixel_height.max(1);
    let natural_width = pixel_width.div_ceil(16).max(1);
    let zoomed_width = natural_width
        .saturating_mul(u32::from(zoom_percent.max(1)))
        .div_ceil(100)
        .max(1);
    let cell_width = u16::try_from(zoomed_width)
        .unwrap_or(u16::MAX)
        .clamp(1, image.max_width.max(1));
    let cell_height = estimate_markdown_image_cell_height(media, cell_width, image.max_height);

    let visible_width = pixel_width
        .saturating_mul(u32::from(cell_width))
        .div_ceil(zoomed_width)
        .clamp(1, pixel_width);
    let zoomed_height = (zoomed_width.saturating_mul(pixel_height))
        .div_ceil(pixel_width.saturating_mul(2).max(1))
        .max(1);
    let visible_height = pixel_height
        .saturating_mul(u32::from(cell_height))
        .div_ceil(zoomed_height)
        .clamp(1, pixel_height);
    let max_x = pixel_width.saturating_sub(visible_width);
    let max_y = pixel_height.saturating_sub(visible_height);
    let x = image.view.pan_x_px.min(max_x);
    let y = image.view.pan_y_px.min(max_y);

    FloatingImagePlacement {
        cell_width,
        cell_height,
        source_rect: Some(OverlaySourceRect {
            x,
            y,
            width: visible_width,
            height: visible_height,
        }),
    }
}

impl TuiRenderCoordinatorService for TuiRenderCoordinator {
    fn render_workspace(
        &mut self,
        request: RenderFrameRequest<'_>,
    ) -> Result<TuiRenderOutcome, RenderFrameError> {
        self.render_workspace_inner(
            request.workspace,
            request.capabilities,
            request.presentation,
            request.overlay_writer,
            request.redraw_plan.cloned().unwrap_or_default(),
            None,
            true,
        )
    }
}

#[cfg(test)]
#[path = "coordinator_test.rs"]
mod tests;
