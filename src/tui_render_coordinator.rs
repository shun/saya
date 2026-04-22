use crate::optional_graphics::{
    OptionalGraphicsAdapter, OptionalGraphicsAdapterService, OverlayRenderResult,
    OverlayTerminalWriter,
};
use crate::overlay_asset_store::{OverlayAssetRef, OverlayAssetStore, OverlayAssetStoreService};
use crate::presentation_effect::{
    PresentationEffectProjector, PresentationEffectProjectorService, PresentationState,
    RuntimePresentationIntent,
};
use crate::screen_model::WorkspaceScreenModel;
use crate::terminal_capability::{TerminalCapabilityProfile, TextStyleCapability};
pub use crate::tui_renderer::RenderTextMode;
use crate::tui_renderer::TuiRenderer;
use std::fmt;

pub struct RenderFrameRequest<'a> {
    pub workspace: &'a WorkspaceScreenModel,
    pub capabilities: &'a TerminalCapabilityProfile,
    pub presentation: &'a PresentationState,
    pub overlay_writer: Option<&'a mut dyn OverlayTerminalWriter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiRenderOutcome {
    pub rendered_workspace: WorkspaceScreenModel,
    pub text_mode: RenderTextMode,
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
    last_successful_workspace: Option<WorkspaceScreenModel>,
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
            last_successful_workspace: None,
        }
    }

    pub fn new_for_tests(
        asset_store: OverlayAssetStore,
        graphics_adapter: OptionalGraphicsAdapter,
    ) -> Self {
        Self {
            renderer: None,
            projector: PresentationEffectProjector,
            asset_store,
            graphics_adapter,
            last_successful_workspace: None,
        }
    }

    pub fn render_workspace_result<E: fmt::Display>(
        &mut self,
        render_result: Result<WorkspaceScreenModel, E>,
        capabilities: &TerminalCapabilityProfile,
        runtime_effects: &[RuntimePresentationIntent],
        overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
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
                )
            }
            Err(error) => {
                let message = error.to_string();
                let Some(last_successful) = self.last_successful_workspace.clone() else {
                    return Err(RenderFrameError::Projection { message });
                };
                let mut rollback = last_successful;
                rollback.global_message_line = Some(message);
                let rollback_presentation = self.projector.project(&rollback, &[], capabilities);
                self.render_workspace_with_presentation(
                    &rollback,
                    capabilities,
                    &rollback_presentation,
                    overlay_writer,
                )
            }
        }
    }

    fn resolve_text_mode(capabilities: &TerminalCapabilityProfile) -> RenderTextMode {
        match capabilities.text_style {
            TextStyleCapability::Plain => RenderTextMode::Plain,
            TextStyleCapability::Ansi => RenderTextMode::StyledAnsi,
            TextStyleCapability::TrueColor => RenderTextMode::StyledTrueColor,
        }
    }

    fn apply_presentation(
        workspace: &WorkspaceScreenModel,
        presentation: &PresentationState,
    ) -> WorkspaceScreenModel {
        let mut rendered = workspace.clone();
        rendered.global_message_line = presentation.global_message_line.clone();
        rendered.command_line = presentation.command_line.clone();
        rendered
    }

    fn ensure_fallback_message(workspace: &mut WorkspaceScreenModel, fallback_text: &str) {
        if workspace.global_message_line.is_none() && !fallback_text.trim().is_empty() {
            workspace.global_message_line = Some(fallback_text.to_string());
        }
    }

    fn render_workspace_with_presentation(
        &mut self,
        workspace: &WorkspaceScreenModel,
        capabilities: &TerminalCapabilityProfile,
        presentation: &PresentationState,
        overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
    ) -> Result<TuiRenderOutcome, RenderFrameError> {
        self.render_workspace_inner(workspace, capabilities, presentation, overlay_writer)
    }

    fn render_workspace_inner(
        &mut self,
        workspace: &WorkspaceScreenModel,
        capabilities: &TerminalCapabilityProfile,
        presentation: &PresentationState,
        mut overlay_writer: Option<&mut dyn OverlayTerminalWriter>,
    ) -> Result<TuiRenderOutcome, RenderFrameError> {
        let text_mode = Self::resolve_text_mode(capabilities);
        let mut rendered_workspace = Self::apply_presentation(workspace, presentation);
        let mut overlay_results = Vec::new();
        let mut active_assets = Vec::<OverlayAssetRef>::new();

        for overlay in &presentation.overlays {
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
            let Some(protocol) = self.graphics_adapter.negotiate(capabilities) else {
                overlay_results.push(OverlayRenderResult::FallbackToText);
                Self::ensure_fallback_message(&mut rendered_workspace, &overlay.fallback_text);
                continue;
            };
            let Some(graphics_request) =
                self.graphics_adapter
                    .project_request(overlay, snapshot, &rendered_workspace)
            else {
                overlay_results.push(OverlayRenderResult::FallbackToText);
                Self::ensure_fallback_message(&mut rendered_workspace, &overlay.fallback_text);
                continue;
            };
            let Some(writer) = overlay_writer.as_deref_mut() else {
                overlay_results.push(OverlayRenderResult::FallbackToText);
                Self::ensure_fallback_message(&mut rendered_workspace, &overlay.fallback_text);
                continue;
            };
            let result = self
                .graphics_adapter
                .render_overlay(&graphics_request, protocol, writer);
            if result == OverlayRenderResult::Rendered {
                active_assets.push(asset_ref);
            } else {
                Self::ensure_fallback_message(&mut rendered_workspace, &overlay.fallback_text);
            }
            overlay_results.push(result);
        }
        self.asset_store.release_unused(&active_assets);

        if let Some(renderer) = self.renderer.as_mut() {
            renderer
                .draw_with_mode(&rendered_workspace, text_mode)
                .map_err(|error| RenderFrameError::TerminalIo {
                    message: error.to_string(),
                })?;
        }
        self.last_successful_workspace = Some(rendered_workspace.clone());
        Ok(TuiRenderOutcome {
            rendered_workspace,
            text_mode,
            overlay_results,
        })
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
        )
    }
}
