use saya::saya_live_runtime::RUNTIME_SAYA_TYPE_DECLARATION;
use saya::startup_runtime::STARTUP_SAYA_TYPE_DECLARATION;

#[test]
fn startup_public_api_type_declaration_covers_formal_configuration_surface() {
    let declaration = STARTUP_SAYA_TYPE_DECLARATION;

    assert!(declaration.contains("declare global"));
    assert!(declaration.contains("tabSize"));
    assert!(declaration.contains("lineNumbers"));
    assert!(declaration.contains("numberWidth"));
    assert!(declaration.contains("keymap"));
    assert!(declaration.contains("commands"));
    assert!(declaration.contains("events"));
    assert!(declaration.contains("SayaStartupSurface"));
}

#[test]
fn startup_public_api_type_declaration_does_not_expose_runtime_or_gui_transport_details() {
    let declaration = STARTUP_SAYA_TYPE_DECLARATION.to_ascii_lowercase();

    for forbidden in [
        "overlay",
        "graphics",
        "protocol",
        "bytes",
        "kitty",
        "sixel",
        "window.open",
        "gpu",
    ] {
        assert!(
            !declaration.contains(forbidden),
            "startup declaration should stay TUI-only and avoid transport leaks: {forbidden}"
        );
    }
}

#[test]
fn runtime_public_api_type_declaration_covers_formal_execution_surface() {
    let declaration = RUNTIME_SAYA_TYPE_DECLARATION;

    assert!(declaration.contains("declare global"));
    assert!(declaration.contains("commands"));
    assert!(declaration.contains("buffer"));
    assert!(declaration.contains("window"));
    assert!(declaration.contains("editor"));
    assert!(declaration.contains("SayaRuntimeSurface"));
}

#[test]
fn runtime_public_api_type_declaration_does_not_expose_transport_specific_presentation_details() {
    let declaration = RUNTIME_SAYA_TYPE_DECLARATION.to_ascii_lowercase();

    for forbidden in [
        "overlay", "graphics", "protocol", "bytes", "asset", "kitty", "sixel", "wgpu", "neovim",
    ] {
        assert!(
            !declaration.contains(forbidden),
            "runtime declaration should keep presentation transport private: {forbidden}"
        );
    }
}
