//! Public-surface boundary suite for `saya` startup and runtime guards.
//!
//! This file stays focused on host/application API exposure and capability
//! boundaries. It must not drift into duplicated editing semantics.

use std::path::PathBuf;
use std::sync::Arc;

use saya::runtime::callback_registry_seed::CallbackRegistrySeed;
use saya::runtime::live::{
    BoxFuture, HostCapabilityBridge, ReadonlyEditorSnapshot, ReadonlyWindowSnapshot,
    RuntimeCommandError, RuntimeMode,
};
use saya::runtime::live::{
    BufferEventPayload, ReadonlyBufferSnapshot, RuntimeEventPayload, SayaLiveRuntime,
    runtime_forbidden_surface_names, runtime_public_surface_names,
};
use saya::runtime::startup::StartupRegistryEntry;
use saya::runtime::startup::{
    evaluate_startup_module, startup_forbidden_surface_names, startup_public_surface_names,
};

fn saya_surface_suite_scope_statement() -> &'static str {
    "public-surface boundary suite for startup and runtime capability exposure in host/application API gating"
}

#[test]
fn saya_surface_suite_scope_statement_stays_pinned_to_public_boundary_ownership() {
    let statement = saya_surface_suite_scope_statement();

    assert!(
        statement.contains("public-surface boundary suite"),
        "suite ownership statement should stay explicit"
    );
    assert!(
        statement.contains("startup"),
        "suite ownership statement should keep startup responsibility visible"
    );
    assert!(
        statement.contains("runtime"),
        "suite ownership statement should keep runtime responsibility visible"
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
fn detailed_editing_semantics_validation_remains_owned_by_vim_core_rs() {
    let docs = std::fs::read_to_string("docs/testing.md")
        .expect("testing docs should be readable from the repository root");

    assert!(
        docs.contains("Do not add detailed editing-semantics validation here."),
        "testing boundary should explicitly keep detailed editing-semantics validation out of saya"
    );
    assert!(
        docs.contains("vim-core-rs"),
        "testing boundary should keep detailed editing-semantics ownership with vim-core-rs"
    );
}

#[test]
fn saya_tests_are_added_only_for_host_application_value() {
    let docs = std::fs::read_to_string("docs/testing.md")
        .expect("testing docs should be readable from the repository root");

    assert!(
        docs.contains("Add saya tests only when they prove host-application value."),
        "testing boundary should require host-application value before adding more saya tests"
    );
    assert!(
        docs.contains("host application layer around `vim-core-rs`"),
        "testing boundary should keep the value check anchored to the host layer"
    );
}

#[test]
fn headless_end_to_end_coverage_is_preferred_over_core_detail_duplication() {
    let docs = std::fs::read_to_string("docs/testing.md")
        .expect("testing docs should be readable from the repository root");

    assert!(
        docs.contains("Prefer headless end-to-end coverage over duplicated core-detail checks."),
        "testing boundary should prefer headless end-to-end coverage over duplicated core-detail checks"
    );
    assert!(
        docs.contains("headless verification"),
        "testing boundary should keep the headless verification preference visible"
    );
}

#[test]
fn live_runtime_review_is_recorded_against_saya_live_runtime_in_next_review() {
    let docs = std::fs::read_to_string("docs/testing-todo.md")
        .expect("testing todo should be readable from the repository root");
    let normalized = docs.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(
        normalized.contains(
            "- [x] Revisit this list whenever the live runtime path gains new integration points."
        ),
        "live runtime review should be marked complete once the todo has been revisited"
    );
    assert!(
        docs.contains("Review this list after the main TUI loop gains fuller"),
        "next review guidance should stay tied to the main TUI loop integration point"
    );
    assert!(
        docs.contains("SayaLiveRuntime"),
        "next review guidance should keep SayaLiveRuntime visible as the runtime integration boundary"
    );
}

#[test]
fn large_test_migration_review_records_current_inventory_without_premature_completion() {
    let docs = std::fs::read_to_string("docs/testing-todo.md")
        .expect("testing todo should be readable from the repository root");
    let normalized = docs.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(
        docs.contains("Review log: March 28, 2026"),
        "testing todo should record the latest large-test-migration review with a concrete date"
    );
    assert!(
        docs.contains("integration_editing.rs"),
        "testing todo should record the saya-side review inventory"
    );
    assert!(
        docs.contains("vim-core-rs/tests/mode_transition_contract.rs"),
        "testing todo should identify the core-side destination for migration candidates"
    );
    assert!(
        normalized.contains(
            "- [x] Review this list after any large test migration between `saya` and `vim-core-rs`."
        ),
        "large test migration review should be marked complete after the migration is executed"
    );
    assert!(
        docs.contains("visual_selection_contract.rs"),
        "testing todo should record the vim-core-rs visual-selection destination after migration"
    );
    assert!(
        docs.contains("The detailed editing-semantics assertions that used"),
        "testing todo should explain that detailed semantics moved out of saya after migration"
    );
}

#[test]
fn integration_editing_stays_trimmed_to_host_smoke_after_core_migration() {
    let source = std::fs::read_to_string("tests/integration_editing.rs")
        .expect("integration_editing source should be readable from the repository root");

    assert!(
        !source.contains("assert_eq!((selection.start_row, selection.start_col), (0, 6));"),
        "saya should not keep exact visual-selection coordinates once vim-core-rs owns them"
    );
    assert!(
        !source.contains("assert_eq!((selection.end_row, selection.end_col_exclusive), (0, 10));"),
        "saya should not keep exact visual-selection ranges once vim-core-rs owns them"
    );
    assert!(
        !source.contains("assert_eq!(snapshot.cursor_row, 1);"),
        "saya should not keep exact cursor-row editing semantics in the representative editing smoke"
    );
    assert!(
        !source.contains("assert_eq!(outcome.core_bridge.snapshot().mode, CoreMode::Insert);"),
        "saya should not keep exact insert-mode transition semantics once vim-core-rs owns them"
    );
}

#[test]
fn integration_editing_smoke_does_not_reintroduce_core_owned_selection_and_edit_details() {
    let source = std::fs::read_to_string("tests/integration_editing.rs")
        .expect("integration editing suite should be readable from the repository root");

    assert!(
        !source.contains("(selection.start_row, selection.start_col)"),
        "saya smoke should not pin exact visual-selection coordinates after vim-core-rs owns that contract"
    );
    assert!(
        !source.contains("line.contains(\"XY\")"),
        "saya smoke should not pin exact inserted-text semantics once vim-core-rs owns that round trip"
    );
    assert!(
        !source.contains("snapshot.cursor_row, 1"),
        "saya smoke should not pin exact cursor-row edit semantics once vim-core-rs owns delete/motion contracts"
    );
    assert!(
        !source.contains("snapshot.mode, CoreMode::Insert"),
        "saya smoke should not pin exact intermediate mode semantics once vim-core-rs owns them"
    );
    assert!(
        source.contains("visual_selection.is_some()"),
        "saya should keep host-side projection smoke for visual-selection handoff"
    );
    assert!(
        source.contains("model.lines != initial_model.lines"),
        "saya should keep high-level projection-change smoke for integrated editing flows"
    );
}

#[test]
fn core_bridge_debug_does_not_materialize_full_snapshots() {
    let source = std::fs::read_to_string("src/core/bridge.rs")
        .expect("core bridge source should be readable from the repository root");

    assert!(
        !source.contains(".field(\"snapshot\", &self.snapshot())"),
        "CoreBridge Debug must not call snapshot(), because formatting a bridge would materialize full buffer text"
    );
}

#[test]
fn register_behavior_remains_out_of_scope_for_saya() {
    let docs = std::fs::read_to_string("docs/testing.md")
        .expect("testing docs should be readable from the repository root");

    assert!(
        docs.contains("Do not add exhaustive register-behavior coverage here."),
        "testing boundary should explicitly keep exhaustive register coverage out of saya"
    );
    assert!(
        docs.contains("vim-core-rs"),
        "testing boundary should keep register-detail ownership with vim-core-rs"
    );
}

#[test]
fn mark_and_jumplist_behavior_remains_out_of_scope_for_saya() {
    let docs = std::fs::read_to_string("docs/testing.md")
        .expect("testing docs should be readable from the repository root");

    assert!(
        docs.contains("Do not add exhaustive mark and jumplist coverage here."),
        "testing boundary should explicitly keep exhaustive mark and jumplist coverage out of saya"
    );
    assert!(
        docs.contains("vim-core-rs"),
        "testing boundary should keep mark and jumplist ownership with vim-core-rs"
    );
}

#[test]
fn undo_tree_behavior_remains_out_of_scope_for_saya() {
    let docs = std::fs::read_to_string("docs/testing.md")
        .expect("testing docs should be readable from the repository root");

    assert!(
        docs.contains("Do not add exhaustive undo-tree coverage here."),
        "testing boundary should explicitly keep exhaustive undo-tree coverage out of saya"
    );
    assert!(
        docs.contains("vim-core-rs"),
        "testing boundary should keep undo-tree detail ownership with vim-core-rs"
    );
}

#[test]
fn search_syntax_popup_behavior_remains_out_of_scope_for_saya() {
    let docs = std::fs::read_to_string("docs/testing.md")
        .expect("testing docs should be readable from the repository root");

    assert!(
        docs.contains("Do not add exhaustive search, syntax, highlight, conceal, or pop-up menu"),
        "testing boundary should explicitly keep exhaustive search, syntax, and pop-up menu extraction coverage out of saya"
    );
    assert!(
        docs.contains("Keep detailed search, syntax, highlight, conceal,")
            && docs.contains("and pop-up menu extraction semantics in `vim-core-rs`."),
        "testing boundary should keep search, syntax, and pop-up menu extraction ownership with vim-core-rs"
    );
}

#[test]
fn vfs_and_job_protocol_behavior_remains_out_of_scope_for_saya() {
    let docs = std::fs::read_to_string("docs/testing.md")
        .expect("testing docs should be readable from the repository root");

    assert!(
        docs.contains("Do not add detailed VFS protocol or job protocol contract suites here"),
        "testing boundary should explicitly keep detailed VFS/job protocol coverage out of saya"
    );
    assert!(
        docs.contains("vim-core-rs"),
        "testing boundary should keep VFS/job protocol detail ownership with vim-core-rs"
    );
}

#[test]
fn startup_surface_excludes_filesystem_and_network_capabilities() {
    let surface = startup_public_surface_names();

    assert_eq!(
        surface,
        &[
            "options", "keymap", "commands", "events", "theme", "log", "plugins"
        ]
    );
    assert_eq!(
        startup_forbidden_surface_names(),
        &["filesystem", "network"]
    );
    assert!(!surface.contains(&"filesystem"));
    assert!(!surface.contains(&"network"));
}

#[tokio::test(flavor = "current_thread")]
async fn startup_runtime_does_not_expose_filesystem_or_network() {
    evaluate_startup_module(
        r#"
            if (typeof saya.filesystem !== "undefined") {
                throw new Error("filesystem capability leaked into startup surface");
            }
            if (typeof saya.network !== "undefined") {
                throw new Error("network capability leaked into startup surface");
            }
        "#,
    )
    .await
    .expect("startup surface should hide filesystem and network");
}

#[test]
fn runtime_surface_excludes_filesystem_and_network_capabilities() {
    let surface = runtime_public_surface_names();

    assert_eq!(
        surface,
        &[
            "commands",
            "buffer",
            "window",
            "panel",
            "editor",
            "filer",
            "lsp",
            "lsif",
            "input",
            "selector",
            "completion",
            "process",
            "plugins"
        ]
    );
    assert_eq!(
        runtime_forbidden_surface_names(),
        &["filesystem", "network"]
    );
    assert!(!surface.contains(&"filesystem"));
    assert!(!surface.contains(&"network"));
}

#[test]
fn dired_preview_surface_is_documented_from_setup_to_host_layer_decision() {
    let readme = std::fs::read_to_string("README.md")
        .expect("README should be readable from repository root");
    let adr = std::fs::read_to_string("docs/adr/0003-keep-dired-in-host-layer.md")
        .expect("dired host-layer ADR should be readable from repository root");

    assert!(
        readme.contains("setupSayaDired")
            && readme.contains("hiddenFilePolicy")
            && readme.contains("sortPolicy")
            && readme.contains("confirmStrategy"),
        "README should show how to configure the preview dired plugin"
    );
    assert!(
        adr.contains("host application layer")
            && adr.contains("TypeScript")
            && adr.contains("vim-core-rs")
            && adr.contains("preview feature")
            && adr.contains("Destructive operations"),
        "dired ADR should record ownership, preview status, and destructive-operation policy"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_does_not_expose_filesystem_or_network() {
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            (payload) => {
                if (typeof saya.filesystem !== "undefined") {
                    throw new Error("filesystem capability leaked into runtime surface");
                }
                if (typeof saya.network !== "undefined") {
                    throw new Error("network capability leaked into runtime surface");
                }
                return saya.commands.execute(`buffer:${payload.buffer.id}`);
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(Arc::new(NoopHostBridge), seed)
        .expect("runtime should initialize");
    let report = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: ReadonlyBufferSnapshot {
                id: 7,
                path: Some(PathBuf::from("surface-guard.md")),
                line_count: 1,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
            },
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(report.handler_count, 1);
}

struct NoopHostBridge;

impl HostCapabilityBridge for NoopHostBridge {
    fn execute_host_command(&self, _name: &str) -> BoxFuture<Result<(), RuntimeCommandError>> {
        Box::pin(async move { Ok(()) })
    }

    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot> {
        Box::pin(async move {
            ReadonlyBufferSnapshot {
                id: 1,
                path: None,
                line_count: 0,
                cursor_row: 0,
                cursor_col: 0,
                current_line: String::new(),
                text: String::new(),
            }
        })
    }

    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot> {
        Box::pin(async move { ReadonlyWindowSnapshot { id: 1 } })
    }

    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot> {
        Box::pin(async move {
            ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            }
        })
    }
}
