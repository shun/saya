use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSurfaceMode {
    TuiOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiFeatureRequest {
    CoreEditing,
    StyledText,
    InlineGraphics,
    DedicatedGuiWindow,
    DirectGpuRendering,
    GuiCompatibility,
    NeovimCompatibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyViolation {
    UnsupportedSurface { requested: UiFeatureRequest },
}

impl fmt::Display for PolicyViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyViolation::UnsupportedSurface { requested } => {
                write!(
                    f,
                    "TUI-only policy rejected unsupported surface request: {requested:?}"
                )
            }
        }
    }
}

pub trait UiSurfacePolicyService {
    fn resolve_mode(&self) -> UiSurfaceMode;
    fn validate_feature_request(&self, request: UiFeatureRequest) -> Result<(), PolicyViolation>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct UiSurfacePolicy;

impl UiSurfacePolicyService for UiSurfacePolicy {
    fn resolve_mode(&self) -> UiSurfaceMode {
        UiSurfaceMode::TuiOnly
    }

    fn validate_feature_request(&self, request: UiFeatureRequest) -> Result<(), PolicyViolation> {
        match request {
            UiFeatureRequest::CoreEditing
            | UiFeatureRequest::StyledText
            | UiFeatureRequest::InlineGraphics => Ok(()),
            UiFeatureRequest::DedicatedGuiWindow
            | UiFeatureRequest::DirectGpuRendering
            | UiFeatureRequest::GuiCompatibility
            | UiFeatureRequest::NeovimCompatibility => {
                Err(PolicyViolation::UnsupportedSurface { requested: request })
            }
        }
    }
}
