//! Public-surface boundary suite for the `saya` namespace contract.
//!
//! This file stays focused on string-level public API exposure and namespace
//! exclusions. It must not drift into duplicated editing semantics.

use saya::saya_live_runtime::runtime_public_surface_paths;
use saya::startup_runtime::startup_public_surface_paths;

const FORBIDDEN_COMPAT_STRING_APIS: &[&str] = &["vim.cmd", ":set", ":map"];

fn public_surface_suite_scope_statement() -> &'static str {
    "public-surface boundary suite for startup/runtime surface names and namespace exclusions in host/application API gating"
}

#[test]
fn public_surface_suite_scope_statement_stays_pinned_to_boundary_ownership() {
    let statement = public_surface_suite_scope_statement();

    assert!(
        statement.contains("public-surface boundary suite"),
        "suite ownership statement should stay explicit"
    );
    assert!(
        statement.contains("startup/runtime"),
        "suite ownership statement should keep both surfaces visible"
    );
    assert!(
        statement.contains("namespace exclusions"),
        "suite ownership statement should keep namespace exclusions visible"
    );
    assert!(
        statement.contains("host/application"),
        "suite ownership statement should stay anchored to the host layer"
    );
    assert!(
        !statement.contains("editing semantics"),
        "suite ownership statement must not drift into core-editing ownership"
    );
}

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

#[test]
fn main_loop_consumes_normalized_outcomes_without_raw_core_outcome_enums() {
    let source = std::fs::read_to_string("src/main.rs")
        .expect("main source should be readable from the repository root");

    for raw_enum in ["CoreHostAction", "CoreEvent"] {
        assert!(
            !source.contains(raw_enum),
            "main loop must consume folded normalized outcomes instead of raw {raw_enum}"
        );
    }
}

#[test]
fn notification_projection_module_keeps_raw_core_enums_out_of_ui_surface() {
    let source = std::fs::read_to_string("src/core_notification_prompt.rs")
        .expect("notification projection module should be readable from the repository root");

    for raw_enum in ["CoreHostAction", "CoreEvent"] {
        assert!(
            !source.contains(raw_enum),
            "notification projection module must not depend on raw {raw_enum}"
        );
    }
}

#[test]
fn testing_docs_pin_suite_specific_gtimeout_acceptance_commands() {
    let docs = std::fs::read_to_string("docs/testing.md")
        .expect("testing docs should be readable from the repository root");

    for command in [
        "gtimeout 120 cargo test notification_prompt -- --test-threads=1",
        "gtimeout 120 cargo test public_surface_guard -- --test-threads=1",
    ] {
        assert!(
            docs.contains(command),
            "testing docs should pin the suite-specific gtimeout acceptance command: {command}"
        );
    }
}

#[test]
fn typed_message_line_migration_forbids_direct_string_overwrite_paths() {
    for path in [
        "src/screen_model.rs",
        "src/presentation_effect.rs",
        "src/tui_render_coordinator.rs",
        "src/tui_renderer.rs",
        "src/main.rs",
    ] {
        let source =
            std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));
        for forbidden in [
            "global_message_line =",
            "pub global_message_line: Option<String>",
            ".global_message_line =",
        ] {
            assert!(
                !source.contains(forbidden),
                "typed message line migration forbids direct string overwrite path in {path}: {forbidden}"
            );
        }
    }
}
