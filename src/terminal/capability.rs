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
    Monochrome,
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
    pub color_text: bool,
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
        let color_text = env::var_os("NO_COLOR").is_none();
        let truecolor =
            color_text && (colorterm.contains("truecolor") || colorterm.contains("24bit"));

        let inline_graphics = inline_graphics_probe_result_from_env();

        Self::new(
            TerminalCapabilityObservation {
                session_kind,
                basic_terminal_control: true,
                styled_text: true,
                color_text,
                truecolor,
            },
            inline_graphics,
        )
    }
}

fn inline_graphics_probe_result_from_env() -> InlineGraphicsProbeResult {
    if let Ok(value) = env::var("SAYA_INLINE_GRAPHICS") {
        return match value.trim().to_ascii_lowercase().as_str() {
            "kitty" => InlineGraphicsProbeResult::Supported(InlineGraphicsProtocol::Kitty),
            "sixel" => InlineGraphicsProbeResult::Supported(InlineGraphicsProtocol::Sixel),
            "0" | "false" | "off" | "no" | "disabled" => InlineGraphicsProbeResult::Disabled,
            _ => InlineGraphicsProbeResult::Unsupported,
        };
    }

    let term = env::var("TERM").unwrap_or_default().to_ascii_lowercase();
    let term_program = env::var("TERM_PROGRAM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if env::var_os("KITTY_WINDOW_ID").is_some()
        || env::var_os("GHOSTTY_RESOURCES_DIR").is_some()
        || term.contains("xterm-kitty")
        || term.contains("xterm-ghostty")
        || term_program.contains("kitty")
        || term_program.contains("ghostty")
    {
        return InlineGraphicsProbeResult::Supported(InlineGraphicsProtocol::Kitty);
    }

    InlineGraphicsProbeResult::Unsupported
}

impl TerminalCapabilityProbeService for TerminalCapabilityProbe {
    fn detect(&mut self) -> TerminalCapabilityProfile {
        let mut degraded_reasons = Vec::new();
        let text_style = if !self.observation.styled_text {
            degraded_reasons.push(CapabilityDegradationReason::MissingStyledText);
            TextStyleCapability::Plain
        } else if !self.observation.color_text {
            TextStyleCapability::Monochrome
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_color_terminal_keeps_text_modifiers_available_without_color() {
        let mut probe = TerminalCapabilityProbe::new(
            TerminalCapabilityObservation {
                session_kind: TerminalSessionKind::Local,
                basic_terminal_control: true,
                styled_text: true,
                color_text: false,
                truecolor: true,
            },
            InlineGraphicsProbeResult::Unsupported,
        );

        let profile = probe.detect();

        assert_eq!(profile.text_style, TextStyleCapability::Monochrome);
        assert!(
            !profile
                .degraded_reasons
                .contains(&CapabilityDegradationReason::MissingStyledText),
            "NO_COLOR should disable color without disabling bold or underline"
        );
    }

    #[test]
    fn terminal_without_styled_text_still_falls_back_to_plain() {
        let mut probe = TerminalCapabilityProbe::new(
            TerminalCapabilityObservation {
                session_kind: TerminalSessionKind::Local,
                basic_terminal_control: true,
                styled_text: false,
                color_text: false,
                truecolor: false,
            },
            InlineGraphicsProbeResult::Unsupported,
        );

        let profile = probe.detect();

        assert_eq!(profile.text_style, TextStyleCapability::Plain);
        assert!(
            profile
                .degraded_reasons
                .contains(&CapabilityDegradationReason::MissingStyledText)
        );
    }

    #[test]
    fn from_env_detects_kitty_inline_graphics_from_kitty_window_id() {
        let _guard = env_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _kitty = EnvGuard::set("KITTY_WINDOW_ID", Some("7"));
        let _term = EnvGuard::set("TERM", Some("xterm-256color"));
        let _override = EnvGuard::set("SAYA_INLINE_GRAPHICS", None);

        let mut probe = TerminalCapabilityProbe::from_env();
        let profile = probe.detect();

        assert_eq!(profile.inline_graphics, Some(InlineGraphicsProtocol::Kitty));
    }

    #[test]
    fn from_env_detects_ghostty_as_kitty_inline_graphics_compatible() {
        let _guard = env_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _kitty = EnvGuard::set("KITTY_WINDOW_ID", None);
        let _ghostty_resources = EnvGuard::set("GHOSTTY_RESOURCES_DIR", Some("/tmp/ghostty"));
        let _term = EnvGuard::set("TERM", Some("xterm-ghostty"));
        let _term_program = EnvGuard::set("TERM_PROGRAM", Some("ghostty"));
        let _override = EnvGuard::set("SAYA_INLINE_GRAPHICS", None);

        let mut probe = TerminalCapabilityProbe::from_env();
        let profile = probe.detect();

        assert_eq!(profile.inline_graphics, Some(InlineGraphicsProtocol::Kitty));
    }

    #[test]
    fn from_env_allows_inline_graphics_override_for_headless_diagnostics() {
        let _guard = env_test_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _kitty = EnvGuard::set("KITTY_WINDOW_ID", None);
        let _term = EnvGuard::set("TERM", Some("xterm-256color"));
        let _override = EnvGuard::set("SAYA_INLINE_GRAPHICS", Some("kitty"));

        let mut probe = TerminalCapabilityProbe::from_env();
        let profile = probe.detect();

        assert_eq!(profile.inline_graphics, Some(InlineGraphicsProtocol::Kitty));
    }

    fn env_test_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }
}
