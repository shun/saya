use saya::saya_live_runtime::runtime_public_surface_paths;
use saya::startup_runtime::startup_public_surface_paths;

const FORBIDDEN_COMPAT_STRING_APIS: &[&str] = &["vim.cmd", ":set", ":map"];

#[test]
fn startup_surface_excludes_compatibility_string_apis() {
    let surface = startup_public_surface_paths();

    for forbidden in FORBIDDEN_COMPAT_STRING_APIS {
        assert!(
            !surface.contains(forbidden),
            "startup surface should not expose compatibility string api: {forbidden}"
        );
    }

    assert!(
        !surface.iter().any(|entry| entry.starts_with("vim.")),
        "startup surface should not expose vim namespace"
    );
}

#[test]
fn runtime_surface_excludes_compatibility_string_apis() {
    let surface = runtime_public_surface_paths();

    for forbidden in FORBIDDEN_COMPAT_STRING_APIS {
        assert!(
            !surface.contains(forbidden),
            "runtime surface should not expose compatibility string api: {forbidden}"
        );
    }

    assert!(
        !surface.iter().any(|entry| entry.starts_with("vim.")),
        "runtime surface should not expose vim namespace"
    );
}

