use std::env;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalSessionKind {
    Local,
    Ssh,
    Tmux,
    Container,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextStyleCapability {
    Plain,
    Ansi,
    TrueColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineGraphicsProtocol {
    Kitty,
    Sixel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineGraphicsProbeResult {
    Supported(InlineGraphicsProtocol),
    Unsupported,
    Timeout,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityDegradationReason {
    MissingStyledText,
    GraphicsUnsupported,
    GraphicsProbeTimedOut,
    GraphicsDisabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCapabilityObservation {
    pub session_kind: TerminalSessionKind,
    pub basic_terminal_control: bool,
    pub styled_text: bool,
    pub truecolor: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCapabilityProfile {
    pub session_kind: TerminalSessionKind,
    pub maintains_core_workflow: bool,
    pub requires_remote_gui_transport: bool,
    pub text_style: TextStyleCapability,
    pub inline_graphics: Option<InlineGraphicsProtocol>,
    pub degraded_reasons: Vec<CapabilityDegradationReason>,
}

pub trait TerminalCapabilityProbeService {
    fn detect(&mut self) -> TerminalCapabilityProfile;
}

#[derive(Debug, Clone)]
pub struct TerminalCapabilityProbe {
    observation: TerminalCapabilityObservation,
    inline_graphics: InlineGraphicsProbeResult,
}

impl TerminalCapabilityProbe {
    pub fn new(
        observation: TerminalCapabilityObservation,
        inline_graphics: InlineGraphicsProbeResult,
    ) -> Self {
        Self {
            observation,
            inline_graphics,
        }
    }

    pub fn from_env() -> Self {
        let session_kind = if env::var_os("TMUX").is_some() {
            TerminalSessionKind::Tmux
        } else if env::var_os("SSH_CONNECTION").is_some() {
            TerminalSessionKind::Ssh
        } else if env::var_os("container").is_some() || std::path::Path::new("/.dockerenv").exists()
        {
            TerminalSessionKind::Container
        } else {
            TerminalSessionKind::Local
        };

        let colorterm = env::var("COLORTERM")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let truecolor = colorterm.contains("truecolor") || colorterm.contains("24bit");
        let styled_text = env::var_os("NO_COLOR").is_none();

        Self::new(
            TerminalCapabilityObservation {
                session_kind,
                basic_terminal_control: true,
                styled_text,
                truecolor,
            },
            InlineGraphicsProbeResult::Unsupported,
        )
    }
}

impl TerminalCapabilityProbeService for TerminalCapabilityProbe {
    fn detect(&mut self) -> TerminalCapabilityProfile {
        let mut degraded_reasons = Vec::new();
        let text_style = if !self.observation.styled_text {
            degraded_reasons.push(CapabilityDegradationReason::MissingStyledText);
            TextStyleCapability::Plain
        } else if self.observation.truecolor {
            TextStyleCapability::TrueColor
        } else {
            TextStyleCapability::Ansi
        };

        let inline_graphics = match self.inline_graphics {
            InlineGraphicsProbeResult::Supported(protocol) => Some(protocol),
            InlineGraphicsProbeResult::Unsupported => {
                degraded_reasons.push(CapabilityDegradationReason::GraphicsUnsupported);
                None
            }
            InlineGraphicsProbeResult::Timeout => {
                degraded_reasons.push(CapabilityDegradationReason::GraphicsProbeTimedOut);
                None
            }
            InlineGraphicsProbeResult::Disabled => {
                degraded_reasons.push(CapabilityDegradationReason::GraphicsDisabled);
                None
            }
        };

        TerminalCapabilityProfile {
            session_kind: self.observation.session_kind,
            maintains_core_workflow: self.observation.basic_terminal_control,
            requires_remote_gui_transport: false,
            text_style,
            inline_graphics,
            degraded_reasons,
        }
    }
}
