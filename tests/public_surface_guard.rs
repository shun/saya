//! Public-surface boundary suite for the `saya` namespace contract.
//!
//! This file stays focused on string-level public API exposure and namespace
//! exclusions. It must not drift into duplicated editing semantics.

use saya::saya_live_runtime::runtime_public_surface_paths;
use saya::startup_runtime::startup_public_surface_paths;
use std::path::{Path, PathBuf};

const FORBIDDEN_COMPAT_STRING_APIS: &[&str] = &["vim.cmd", ":set", ":map"];
const STRUCTURAL_ACCEPTANCE_COMMAND: &str = "gtimeout 30s cargo test --test structural_refresh_contract && gtimeout 30s cargo test --test core_outcome_contract && gtimeout 30s cargo test --test tui_render_coordinator && gtimeout 30s cargo test --test integration_terminal && gtimeout 30s cargo test --test public_surface_guard";

struct BoundaryGuard<'a> {
    boundary: &'a str,
    path: &'a str,
    forbidden_terms: &'a [&'a str],
}

impl BoundaryGuard<'_> {
    fn assert_clean(&self) {
        let source = std::fs::read_to_string(self.path).unwrap_or_else(|error| {
            panic!("{} boundary source is readable: {error}", self.boundary)
        });
        let violations = self
            .forbidden_terms
            .iter()
            .copied()
            .filter(|term| source.contains(term))
            .collect::<Vec<_>>();

        assert!(
            violations.is_empty(),
            "{} boundary must not directly consume forbidden structural-refresh seams in {}: {:?}",
            self.boundary,
            self.path,
            violations
        );
    }
}

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

    for expected in [
        "saya.window.openFloat",
        "saya.window.close",
        "saya.window.focus",
        "saya.window.floats",
    ] {
        assert!(
            surface.contains(&expected),
            "runtime surface should expose typed floating-window API: {expected}"
        );
    }

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
fn tree_sitter_syntax_feature_uses_stable_name_without_worker_compatibility_path() {
    let manifest = std::fs::read_to_string("Cargo.toml")
        .expect("Cargo manifest should be readable from the repository root");

    assert!(
        manifest.contains("[features]\ndefault = [\"tree-sitter-syntax\"]\ntree-sitter-syntax = ["),
        "default release builds should include the stable tree-sitter-syntax feature"
    );
    assert!(
        manifest.contains("\"vim-core-rs/tree-sitter-syntax\""),
        "saya should enable vim-core-rs/tree-sitter-syntax directly"
    );

    let forbidden_terms = [
        concat!("experimental", "-tree-sitter"),
        concat!("experimental", "-tree-sitter-syntax"),
        concat!("SAYA", "_EXPERIMENTAL_TREE_SITTER_SYNTAX"),
        concat!("SAYA", "_TREE_SITTER_SYNTAX_WORKER"),
        concat!("sy", "-tree-sitter-syntax-worker"),
        concat!("tree", "_sitter_worker"),
    ];
    let mut violations = Vec::new();
    for path in source_guard_paths() {
        let source =
            std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
        for term in forbidden_terms {
            if source.contains(term) {
                violations.push(format!("{} contains {term}", path.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Tree-sitter syntax integration must not retain experimental naming or worker compatibility paths: {violations:?}"
    );
}

fn source_guard_paths() -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from("Cargo.toml")];
    for root in ["src", "tests", "docs"] {
        collect_source_guard_paths(Path::new(root), &mut paths);
    }
    paths
}

fn collect_source_guard_paths(path: &Path, paths: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(path).unwrap_or_else(|error| {
        panic!("source guard directory should be readable {path:?}: {error}")
    });
    for entry in entries {
        let entry = entry.expect("source guard directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            collect_source_guard_paths(&path, paths);
            continue;
        }
        if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("rs" | "md")
        ) {
            paths.push(path);
        }
    }
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
fn main_consume_path_wires_folded_structural_refresh_before_workspace_render() {
    let source = std::fs::read_to_string("src/main.rs")
        .expect("main source should be readable from the repository root");

    for expected in [
        "StructuralRefresh::from_folded_effects(&effects.structural)",
        "last_structural_refresh",
        "sync_from_windows_for_render",
        "with_viewport_sync_summary",
        "projection_summary()",
        "with_projection_summary",
    ] {
        assert!(
            source.contains(expected),
            "main consume/render path should expose task 3 structural refresh wiring: {expected}"
        );
    }
}

#[test]
fn structural_refresh_consumes_only_folded_structural_effects() {
    BoundaryGuard {
        boundary: "structural_refresh",
        path: "src/structural_refresh.rs",
        forbidden_terms: &[
            "CoreHostAction",
            "CoreEvent",
            "NormalizedCoreOutcome",
            "take_pending_redraw",
            "take_pending_redraw_requests",
            "PendingRedrawRequest",
        ],
    }
    .assert_clean();

    let source = std::fs::read_to_string("src/structural_refresh.rs")
        .expect("structural refresh source should be readable from the repository root");

    assert!(
        source.contains("StructuralEffectSet"),
        "structural_refresh boundary should stay anchored to folded structural effects"
    );
}

#[test]
fn structural_refresh_does_not_own_prompt_notification_bell_or_job_behavior() {
    BoundaryGuard {
        boundary: "structural_refresh",
        path: "src/structural_refresh.rs",
        forbidden_terms: &[
            "core_notification_prompt",
            "Prompt",
            "Notification",
            "Bell",
            "Job",
            "prompt_line",
            "pager_prompt",
            "bell",
        ],
    }
    .assert_clean();
}

#[test]
fn structural_refresh_boundary_guard_names_each_checked_boundary() {
    for guard in [
        BoundaryGuard {
            boundary: "structural_refresh",
            path: "src/structural_refresh.rs",
            forbidden_terms: &[
                "CoreHostAction",
                "CoreEvent",
                "NormalizedCoreOutcome",
                "take_pending_redraw",
                "take_pending_redraw_requests",
                "PendingRedrawRequest",
                "core_notification_prompt",
                "Prompt",
                "Notification",
                "Bell",
                "Job",
            ],
        },
        BoundaryGuard {
            boundary: "main_ui_consume_path",
            path: "src/main.rs",
            forbidden_terms: &[
                "CoreHostAction",
                "CoreEvent",
                "take_pending_redraw",
                "take_pending_redraw_requests",
                "PendingRedrawRequest",
            ],
        },
    ] {
        guard.assert_clean();
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
fn structural_refresh_acceptance_command_is_headless_timeout_guarded_and_complete() {
    let docs = std::fs::read_to_string("docs/testing.md")
        .expect("testing docs should be readable from the repository root");

    assert!(
        docs.contains(STRUCTURAL_ACCEPTANCE_COMMAND),
        "testing docs should pin the structural refresh acceptance command: {STRUCTURAL_ACCEPTANCE_COMMAND}"
    );
    assert!(
        !STRUCTURAL_ACCEPTANCE_COMMAND.contains("--nocapture")
            && !STRUCTURAL_ACCEPTANCE_COMMAND.contains("script ")
            && !STRUCTURAL_ACCEPTANCE_COMMAND.contains("pty")
            && !STRUCTURAL_ACCEPTANCE_COMMAND.contains("tty"),
        "structural refresh acceptance command must remain headless and non-interactive"
    );

    for suite in [
        "core_outcome_contract",
        "structural_refresh_contract",
        "tui_render_coordinator",
        "integration_terminal",
        "public_surface_guard",
    ] {
        let suite_command = format!("gtimeout 30s cargo test --test {suite}");
        assert!(
            STRUCTURAL_ACCEPTANCE_COMMAND.contains(&suite_command),
            "structural refresh acceptance command must include timeout-guarded suite: {suite}"
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
