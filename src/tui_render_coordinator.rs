use crate::core_notification_prompt::{MessageLineCandidate, MessageLineSource};
use crate::optional_graphics::{
    OptionalGraphicsAdapter, OptionalGraphicsAdapterService, OverlayRenderResult,
    OverlayTerminalWriter,
};
use crate::overlay_asset_store::{OverlayAssetRef, OverlayAssetStore, OverlayAssetStoreService};
use crate::presentation_effect::{
    PresentationEffectProjector, PresentationEffectProjectorService, PresentationState,
    RuntimePresentationIntent, merge_presentation_message_line,
};
use crate::screen_model::{CommandLineModel, ScreenCursorStyle, WorkspaceScreenModel};
use crate::structural_refresh::{
    ProjectionFailureDiagnostic, RedrawPlan, StructuralRefreshOutcome,
};
use crate::terminal_capability::{TerminalCapabilityProfile, TextStyleCapability};
use crate::tui_renderer::TuiRenderer;
pub use crate::tui_renderer::{RenderFrameOptions, RenderTextMode};
use std::fmt;

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
    last_successful_workspace: Option<WorkspaceScreenModel>,
    last_applied_cursor_style: Option<ScreenCursorStyle>,
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
            last_applied_cursor_style: None,
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
            last_applied_cursor_style: None,
        }
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

    fn resolve_text_mode(capabilities: &TerminalCapabilityProfile) -> RenderTextMode {
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
        let text_mode = Self::resolve_text_mode(capabilities);
        let frame_options = frame_options_from_redraw_plan(&redraw_plan);
        let mut rendered_workspace = Self::apply_presentation(workspace, presentation);
        let mut overlay_results = Vec::new();
        let mut active_assets = Vec::<OverlayAssetRef>::new();

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
                .draw_with_mode_and_options(&rendered_workspace, text_mode, frame_options)
                .map_err(|error| RenderFrameError::TerminalIo {
                    message: error.to_string(),
                })?;
        }
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
mod tests {
    use super::*;
    use crate::core_notification_prompt::{
        BellIndication, MessageLineCandidate, MessageLineSource, resolve_workspace_message_line,
    };
    use crate::screen_model::{PaneRect, ScreenCursorStyle, ScreenModel, WorkspaceProjectionError};
    use crate::terminal_capability::{
        InlineGraphicsProbeResult, TerminalCapabilityObservation, TerminalCapabilityProbe,
        TerminalCapabilityProbeService, TerminalSessionKind,
    };

    fn capabilities_without_graphics() -> TerminalCapabilityProfile {
        TerminalCapabilityProbe::new(
            TerminalCapabilityObservation {
                session_kind: TerminalSessionKind::Local,
                basic_terminal_control: true,
                styled_text: false,
                color_text: false,
                truecolor: false,
            },
            InlineGraphicsProbeResult::Disabled,
        )
        .detect()
    }

    fn workspace() -> WorkspaceScreenModel {
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
                message_line: None,
                command_cursor_col: None,
                is_active: true,
            }],
            active_window_id: 1,
            message_line: resolve_workspace_message_line(vec![MessageLineCandidate::legacy(
                MessageLineSource::CoreNotification,
                "core note",
            )]),
            prompt_line: None,
            pager_prompt: None,
            suppressed_prompt_hints: vec![],
            bell: Some(BellIndication { count: 1 }),
            command_line: None,
        }
    }

    #[test]
    fn render_workspace_result_keeps_core_message_visible_when_projection_rolls_back() {
        let mut coordinator = TuiRenderCoordinator::new_for_tests(
            OverlayAssetStore::default(),
            OptionalGraphicsAdapter::default(),
        );
        let capabilities = capabilities_without_graphics();

        let first = coordinator
            .render_workspace_result::<WorkspaceProjectionError>(
                Ok(workspace()),
                &capabilities,
                &[],
                None,
            )
            .expect("initial render should succeed");
        assert_eq!(
            first.rendered_workspace.visible_message_source(),
            Some(MessageLineSource::CoreNotification)
        );

        let second = coordinator
            .render_workspace_result::<WorkspaceProjectionError>(
                Err(WorkspaceProjectionError::ActiveWindowMissing),
                &capabilities,
                &[],
                None,
            )
            .expect("rollback render should succeed");

        assert_eq!(
            second.rendered_workspace.visible_message_text(),
            Some("core note")
        );
        assert_eq!(
            second.rendered_workspace.visible_message_source(),
            Some(MessageLineSource::CoreNotification)
        );
        assert_eq!(
            second.rendered_workspace.suppressed_message_sources(),
            vec![MessageLineSource::RenderProjectionError]
        );
    }

    #[test]
    fn render_workspace_result_retains_runtime_fallback_as_suppressed_when_core_message_exists() {
        let mut coordinator = TuiRenderCoordinator::new_for_tests(
            OverlayAssetStore::default(),
            OptionalGraphicsAdapter::default(),
        );
        let capabilities = capabilities_without_graphics();
        let outcome = coordinator
            .render_workspace_result::<WorkspaceProjectionError>(
                Ok(workspace()),
                &capabilities,
                &[RuntimePresentationIntent {
                    content_key: crate::presentation_effect::OverlayContentKey::RuntimeRegistered {
                        id: "runtime.preview".to_string(),
                    },
                    target: crate::presentation_effect::OverlayTarget::StatusArea,
                    fallback_text: "preview unavailable".to_string(),
                }],
                None,
            )
            .expect("render should succeed");

        assert_eq!(
            outcome.rendered_workspace.visible_message_source(),
            Some(MessageLineSource::CoreNotification)
        );
        assert_eq!(
            outcome.rendered_workspace.suppressed_message_sources(),
            vec![MessageLineSource::RuntimeOverlayFallback]
        );
    }
}
