use super::*;

#[test]
fn render_help_text_lists_vim_compatible_options() {
    let help = render_help_text();

    assert!(help.contains("Usage: sy [arguments] [file]"));
    assert!(help.contains("  --               Only file names after this"));
    assert!(help.contains("  -                Read text from stdin"));
    assert!(help.contains("Default: $XDG_CONFIG_HOME/saya/init.ts"));
    assert!(help.contains("Fallback: $HOME/.config/saya/init.ts"));
    assert!(help.contains("  +<lnum>          Start at line <lnum>"));
    assert!(help.contains("  -R               Read-only mode"));
    assert!(help.contains("  --version        Print version information and exit"));
}

#[test]
fn render_version_text_includes_package_version() {
    let version = render_version_text();

    assert_eq!(version, format!("sy {}", env!("CARGO_PKG_VERSION")));
}
