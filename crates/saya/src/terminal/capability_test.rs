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
