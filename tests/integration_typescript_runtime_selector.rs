//! TypeScript runtime selector integration.
//!
//! This suite pins the headless selector session API exposed to TypeScript. It
//! intentionally avoids TUI, floating UI, source integrations, and actions.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::features::selector::host_adapter::{
    HeadlessSelectorUiProjectionSink, SelectorHostViewAdapter,
};
use saya::features::selector::keymap::{
    SelectorAction, SelectorKeyRoute, SelectorModeSwitch, SelectorQueryEdit,
    selector_key_route_for_model,
};
use saya::features::selector::runtime::{
    HeadlessSelectorViewBackend, RuntimeRenderedSelectorItem, SelectorViewBackend,
    parse_rg_selector_location_detail, parse_rg_vimgrep_output,
};
use saya::features::selector::tui_state::{
    SelectorMode, SelectorTuiProjectionSink, selector_tui_model_to_workspace_float,
};
use saya::input::router::KeyInput;
use saya::runtime::callback_registry_seed::CallbackRegistrySeed;
use saya::runtime::config::StartupRegistryEntry;
use saya::runtime::integration::{RuntimeCommandEffect, RuntimeHostSession, RuntimeSessionOwner};
use saya::runtime::live::{
    BoxFuture, BufferEventPayload, HostCapabilityBridge, ReadonlyBufferSnapshot,
    ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeEventPayload,
    RuntimeInputPromptRequest, RuntimeInputPromptResponse, RuntimeMode, SayaLiveRuntime,
};
use tokio::sync::Mutex;

#[tokio::test(flavor = "current_thread")]
async fn runtime_selector_static_source_can_open_update_read_cancel_and_dispose_headlessly() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async () => {
                const opened = await saya.selector.open({
                    source: {
                        kind: "static",
                        items: [
                            { id: "a", value: "alpha parser", kind: "test", detail: { rank: 1 } },
                            { id: "b", value: "beta query", kind: "test", detail: { rank: 2 } },
                            { id: "c", value: "alpha query parser", kind: "test", detail: { rank: 3 } },
                        ],
                    },
                    matcher: "substringAnd",
                    query: "alpha",
                    limits: { maxRenderedItems: 10 },
                });
                if (opened.renderedItems.map((item) => item.id).join(",") !== "a,c") {
                    throw new Error(`unexpected opened items: ${JSON.stringify(opened.renderedItems)}`);
                }
                if (opened.status.match.totalMatched !== 2 || opened.status.match.totalRendered !== 2) {
                    throw new Error(`unexpected opened status: ${JSON.stringify(opened.status)}`);
                }

                const updated = await saya.selector.update(opened.id, { query: "query" });
                if (updated.renderedItems.map((item) => item.id).join(",") !== "b,c") {
                    throw new Error(`unexpected updated items: ${JSON.stringify(updated.renderedItems)}`);
                }

                const refined = await saya.selector.update(opened.id, { query: "alpha query" });
                if (refined.renderedItems.map((item) => item.id).join(",") !== "c") {
                    throw new Error(`space-separated AND query should keep only items containing every term: ${JSON.stringify(refined.renderedItems)}`);
                }
                const refinedHighlights = refined.renderedItems[0].highlights
                    .map((highlight) => `${highlight.kind}:${highlight.column}:${highlight.width}`)
                    .join(",");
                if (refinedHighlights !== "match:0:5,match:6:5") {
                    throw new Error(`substring AND query should expose match highlights: ${JSON.stringify(refined.renderedItems[0].highlights)}`);
                }

                const current = await saya.selector.current(opened.id);
                if (current.query !== "alpha query" || current.status.match.totalMatched !== 1) {
                    throw new Error(`unexpected current selector snapshot: ${JSON.stringify(current)}`);
                }

                const cancelled = await saya.selector.cancel(opened.id);
                if (cancelled.status.match.state !== "cancelled") {
                    throw new Error(`selector cancel should report cancelled match status: ${JSON.stringify(cancelled)}`);
                }

                await saya.selector.dispose(opened.id);
                const suffix = await saya.selector.open({
                    source: {
                        kind: "static",
                        items: [
                            { id: "suffix-a", value: "alpha parser", kind: "test", detail: null },
                            { id: "suffix-b", value: "alpha parsex", kind: "test", detail: null },
                        ],
                    },
                    matcher: "suffixAnd",
                    query: "ser",
                });
                if (suffix.renderedItems.map((item) => item.id).join(",") !== "suffix-a") {
                    throw new Error(`unexpected suffix matcher items: ${JSON.stringify(suffix.renderedItems)}`);
                }
                await saya.selector.dispose(suffix.id);

                await saya.commands.execute(`selector:${opened.id}:${updated.renderedItems.length}:${refined.renderedItems.length}:${current.status.collect.totalStored}:${suffix.renderedItems.length}`);
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["selector:1:2:1:3:1".to_string()]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_selector_max_rendered_items_keeps_total_matched_count_headlessly() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async () => {
                const items = Array.from({ length: 25 }, (_, index) => ({
                    id: `item-${index}`,
                    value: `needle row ${index}`,
                    kind: "test",
                    detail: { index },
                }));
                const snapshot = await saya.selector.open({
                    source: { kind: "static", items },
                    matcher: "prefixAnd",
                    query: "needle",
                    limits: { maxRenderedItems: 4 },
                });

                if (snapshot.renderedItems.length !== 4) {
                    throw new Error(`maxRenderedItems should limit rendered rows only: ${snapshot.renderedItems.length}`);
                }
                if (snapshot.status.match.totalMatched !== 25 || snapshot.status.match.totalRendered !== 4) {
                    throw new Error(`matched coverage should remain complete: ${JSON.stringify(snapshot.status.match)}`);
                }

                await saya.commands.execute(`selector-limit:${snapshot.renderedItems.length}:${snapshot.status.match.totalMatched}`);
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["selector-limit:4:25".to_string()]
    );
}

#[test]
fn runtime_selector_rg_vimgrep_output_generates_rg_items_with_location_detail() {
    let output = "src/main.rs:12:5:needle in main\nREADME.md:3:1:needle in docs\n";
    let items = parse_rg_vimgrep_output(output).expect("rg vimgrep output should parse");

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "rg:src/main.rs:12:5:0");
    assert_eq!(items[0].kind, "rg");
    assert_eq!(items[0].value, "src/main.rs:12:5:needle in main");
    assert_eq!(items[0].detail["path"], "src/main.rs");
    assert_eq!(items[0].detail["line"], 12);
    assert_eq!(items[0].detail["column"], 5);
    assert_eq!(items[0].detail["text"], "needle in main");
    assert_eq!(items[1].id, "rg:README.md:3:1:1");
}

#[test]
fn runtime_selector_rg_location_detail_parse_accepts_only_valid_rg_items() {
    let item = RuntimeRenderedSelectorItem {
        id: "rg:src/main.rs:12:5:0".to_string(),
        label: "src/main.rs:12:5:needle".to_string(),
        kind: "rg".to_string(),
        detail: serde_json::json!({
            "path": "src/main.rs",
            "line": 12,
            "column": 5,
            "text": "needle",
        }),
        highlights: Vec::new(),
    };

    let location = parse_rg_selector_location_detail(&item).expect("valid rg detail should parse");
    assert_eq!(location.path, PathBuf::from("src/main.rs"));
    assert_eq!(location.line, 12);
    assert_eq!(location.column, 5);

    let unsupported = RuntimeRenderedSelectorItem {
        kind: "test".to_string(),
        ..item.clone()
    };
    assert!(
        parse_rg_selector_location_detail(&unsupported).is_err(),
        "non-rg selector items should not jump through rg action"
    );

    let invalid = RuntimeRenderedSelectorItem {
        detail: serde_json::json!({ "path": "src/main.rs", "line": 0, "column": 5 }),
        ..item
    };
    assert!(
        parse_rg_selector_location_detail(&invalid).is_err(),
        "rg line/column are required to be positive 1-based values"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_selector_rg_source_opens_visible_rows_from_host_rg_results() {
    let root = create_selector_rg_fixture("match");
    std::fs::write(
        root.join("src.txt"),
        "alpha needle one\nbeta row\nneedle second\n",
    )
    .expect("fixture file");
    std::fs::write(root.join("other.txt"), "plain row\n").expect("fixture file");

    let tui_state = Arc::new(SelectorTuiProjectionSink::new());
    let selector_backend = Arc::new(SelectorHostViewAdapter::new(tui_state.clone(), 10));
    let host_bridge = Arc::new(RecordingHostBridge::with_selector_view_backend(
        selector_backend.clone(),
    ));
    let root_literal = ts_string_literal(&root);
    let callback_source = format!(
        r#"
            async () => {{
                const opened = await saya.selector.open({{
                    source: {{
                        kind: "rg",
                        root: {root_literal},
                        pattern: "needle",
                    }},
                    matcher: "substringAnd",
                    query: "needle",
                }});

                if (opened.renderedItems.length !== 2) {{
                    throw new Error(`rg selector should render two rows: ${{JSON.stringify(opened)}}`);
                }}
                if (!opened.renderedItems[0].label.includes("src.txt:1:7:alpha needle one")) {{
                    throw new Error(`first rg row should include vimgrep location: ${{JSON.stringify(opened.renderedItems)}}`);
                }}
                if (opened.status.collect.totalStored !== 2 || opened.status.match.totalMatched !== 2) {{
                    throw new Error(`rg selector status should count collected and matched rows: ${{JSON.stringify(opened.status)}}`);
                }}

                await saya.commands.execute(`selector-rg:${{opened.id}}:${{opened.renderedItems.length}}:${{opened.status.collect.totalStored}}`);
            }}
        "#
    );
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source,
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    let model = tui_state
        .current_model()
        .expect("rg selector should publish visible TUI state");
    assert!(tui_state.is_visible());
    assert_eq!(
        model
            .visible_rows
            .iter()
            .map(|row| row.item.id.starts_with("rg:"))
            .collect::<Vec<_>>(),
        vec![true, true]
    );
    assert!(
        model.visible_rows[0]
            .item
            .label
            .contains("src.txt:1:7:alpha needle one"),
        "visible rows should include rg location labels: {:?}",
        model.visible_rows
    );
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["selector-rg:1:2:2".to_string()]
    );

    remove_selector_rg_fixture(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_prompt_can_supply_rg_selector_pattern_headlessly() {
    let root = create_selector_rg_fixture("prompt");
    std::fs::write(root.join("alpha.txt"), "prompt needle\nmain ignored\n")
        .expect("write alpha fixture");
    std::fs::write(root.join("beta.txt"), "prompt needle again\n").expect("write beta fixture");

    let host_bridge = Arc::new(RecordingHostBridge::with_prompt_responses(vec![
        RuntimeInputPromptResponse::Submitted {
            value: "prompt needle".to_string(),
        },
    ]));
    let root_literal = ts_string_literal(&root);
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: format!(
            r#"
            async () => {{
                const pattern = await saya.input.prompt({{
                    title: "rg pattern",
                    placeholder: "pattern",
                }});
                if (pattern === null) {{
                    throw new Error("prompt should resolve to a submitted value");
                }}
                const opened = await saya.selector.open({{
                    source: {{ kind: "rg", root: {root_literal}, pattern }},
                    matcher: "substringAnd",
                    query: pattern,
                }});
                await saya.commands.execute(`prompt-rg:${{pattern}}:${{opened.query}}:${{opened.status.collect.totalStored}}`);
            }}
        "#
        ),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(
        host_bridge.prompt_requests.lock().await.clone(),
        vec![RuntimeInputPromptRequest {
            title: "rg pattern".to_string(),
            placeholder: Some("pattern".to_string()),
        }]
    );
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["prompt-rg:prompt needle:prompt needle:2".to_string()]
    );

    remove_selector_rg_fixture(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_prompt_empty_input_does_not_open_rg_selector() {
    let host_bridge = Arc::new(RecordingHostBridge::with_prompt_responses(vec![
        RuntimeInputPromptResponse::Submitted {
            value: String::new(),
        },
    ]));
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async () => {
                const pattern = await saya.input.prompt({ title: "rg pattern" });
                if (pattern && pattern.trim().length > 0) {
                    await saya.selector.open({
                        source: { kind: "rg", root: ".", pattern },
                        matcher: "substringAnd",
                        query: pattern,
                    });
                    await saya.commands.execute("prompt-empty:opened");
                } else {
                    await saya.commands.execute("prompt-empty:no-op");
                }
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["prompt-empty:no-op".to_string()]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_prompt_cancel_does_not_open_rg_selector() {
    let host_bridge = Arc::new(RecordingHostBridge::with_prompt_responses(vec![
        RuntimeInputPromptResponse::Cancelled,
    ]));
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async () => {
                const pattern = await saya.input.prompt({ title: "rg pattern" });
                if (pattern !== null) {
                    await saya.selector.open({
                        source: { kind: "rg", root: ".", pattern },
                        matcher: "substringAnd",
                        query: pattern,
                    });
                    await saya.commands.execute("prompt-cancel:opened");
                } else {
                    await saya.commands.execute("prompt-cancel:cancelled");
                }
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["prompt-cancel:cancelled".to_string()]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_selector_rg_source_no_matches_opens_empty_selector() {
    let root = create_selector_rg_fixture("no-match");
    std::fs::write(root.join("src.txt"), "alpha row\nbeta row\n").expect("fixture file");

    let host_bridge = Arc::new(RecordingHostBridge::new());
    let root_literal = ts_string_literal(&root);
    let callback_source = format!(
        r#"
            async () => {{
                const opened = await saya.selector.open({{
                    source: {{
                        kind: "rg",
                        root: {root_literal},
                        pattern: "needle",
                    }},
                    matcher: "substringAnd",
                    query: "needle",
                }});

                if (opened.renderedItems.length !== 0) {{
                    throw new Error(`no-match rg selector should render no rows: ${{JSON.stringify(opened)}}`);
                }}
                if (opened.status.collect.totalStored !== 0 || opened.status.collect.state !== "completed") {{
                    throw new Error(`no-match rg selector should complete with empty collection: ${{JSON.stringify(opened.status)}}`);
                }}

                await saya.commands.execute(`selector-rg-empty:${{opened.id}}:${{opened.status.collect.totalStored}}`);
            }}
        "#
    );
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source,
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["selector-rg-empty:1:0".to_string()]
    );

    remove_selector_rg_fixture(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_selector_rg_source_error_returns_runtime_callback_failure() {
    let missing_root = std::env::temp_dir().join(format!(
        "saya-selector-rg-missing-{}",
        selector_rg_unique_suffix()
    ));
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let root_literal = ts_string_literal(&missing_root);
    let callback_source = format!(
        r#"
            async () => {{
                await saya.selector.open({{
                    source: {{
                        kind: "rg",
                        root: {root_literal},
                        pattern: "needle",
                    }},
                    matcher: "substringAnd",
                    query: "needle",
                }});
            }}
        "#
    );
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source,
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    let error = runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect_err("rg execution failure should reject the runtime callback");

    match error {
        saya::runtime::live::RuntimeDispatchError::CallbackFailed {
            error: saya::runtime::live::RuntimeCallbackError::ScriptFailed { message },
            ..
        } => {
            assert!(
                message.contains("rg selector source failed"),
                "callback error should surface rg source failure: {message}"
            );
        }
        other => panic!("unexpected dispatch error: {other:?}"),
    }
}

#[test]
fn runtime_selector_rg_source_does_not_expand_public_selector_surface() {
    let surface = saya::runtime::live::runtime_public_surface_paths();

    assert_eq!(
        surface
            .iter()
            .copied()
            .filter(|entry| entry.starts_with("saya.selector."))
            .collect::<Vec<_>>(),
        vec![
            "saya.selector.open",
            "saya.selector.update",
            "saya.selector.current",
            "saya.selector.control",
            "saya.selector.cancel",
            "saya.selector.dispose",
        ]
    );
    assert!(!surface.iter().any(|entry| entry.starts_with("vim.")));
    assert!(!surface.iter().any(|entry| entry.starts_with("nvim.")));
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_selector_cancelled_stale_session_update_does_not_affect_active_session() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async () => {
                const first = await saya.selector.open({
                    source: {
                        kind: "static",
                        items: [
                            { id: "first-a", value: "first alpha", kind: "test", detail: null },
                            { id: "first-b", value: "first beta", kind: "test", detail: null },
                        ],
                    },
                    matcher: "substringAnd",
                    query: "alpha",
                });
                await saya.selector.cancel(first.id);

                const active = await saya.selector.open({
                    source: {
                        kind: "static",
                        items: [
                            { id: "active-a", value: "active alpha", kind: "test", detail: null },
                            { id: "active-b", value: "active beta", kind: "test", detail: null },
                        ],
                    },
                    matcher: "substringAnd",
                    query: "alpha",
                });

                let staleRejected = false;
                try {
                    await saya.selector.update(first.id, { query: "beta" });
                } catch (_error) {
                    staleRejected = true;
                }
                const currentActive = await saya.selector.current(active.id);
                if (!staleRejected) {
                    throw new Error("cancelled stale selector update should be rejected");
                }
                if (currentActive.renderedItems.map((item) => item.id).join(",") !== "active-a") {
                    throw new Error(`stale update affected active session: ${JSON.stringify(currentActive)}`);
                }

                await saya.commands.execute(`selector-stale:${active.id}:${currentActive.renderedItems[0].id}`);
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["selector-stale:2:active-a".to_string()]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_selector_headless_controller_updates_view_state_and_selected_item() {
    let host_bridge = Arc::new(RecordingHostBridge::new());
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async () => {
                const opened = await saya.selector.open({
                    source: {
                        kind: "static",
                        items: Array.from({ length: 12 }, (_, index) => ({
                            id: `row-${index}`,
                            value: `row ${index}`,
                            kind: "test",
                            detail: { index },
                        })),
                    },
                    matcher: "substringAnd",
                    query: "row",
                    limits: { maxRenderedItems: 12 },
                });

                if (opened.view.cursor !== 0 || opened.view.offset !== 0 || opened.selectedItem?.id !== "row-0") {
                    throw new Error(`unexpected initial view state: ${JSON.stringify(opened)}`);
                }

                const next = await saya.selector.control(opened.id, { command: "cursorNext" });
                if (next.view.cursor !== 1 || next.selectedItem?.id !== "row-1") {
                    throw new Error(`cursorNext did not select row-1: ${JSON.stringify(next)}`);
                }

                const last = await saya.selector.control(opened.id, { command: "cursorLast" });
                if (last.view.cursor !== 11 || last.selectedItem?.id !== "row-11") {
                    throw new Error(`cursorLast did not clamp to last rendered item: ${JSON.stringify(last)}`);
                }

                const afterEnd = await saya.selector.control(opened.id, { command: "cursorNext" });
                if (afterEnd.view.cursor !== 11 || afterEnd.selectedItem?.id !== "row-11") {
                    throw new Error(`cursorNext should clamp at end: ${JSON.stringify(afterEnd)}`);
                }

                const pageUp = await saya.selector.control(opened.id, { command: "pageUp" });
                if (pageUp.view.cursor !== 1 || pageUp.view.offset !== 1 || pageUp.selectedItem?.id !== "row-1") {
                    throw new Error(`pageUp should move by page size within rendered range: ${JSON.stringify(pageUp)}`);
                }

                const first = await saya.selector.control(opened.id, { command: "cursorFirst" });
                if (first.view.cursor !== 0 || first.view.offset !== 0 || first.selectedItem?.id !== "row-0") {
                    throw new Error(`cursorFirst did not return to first item: ${JSON.stringify(first)}`);
                }

                const hidden = await saya.selector.control(opened.id, { command: "hide" });
                if (!hidden.view.hidden || hidden.view.cancelled) {
                    throw new Error(`hide should hide without cancelling: ${JSON.stringify(hidden.view)}`);
                }

                const stillThere = await saya.selector.current(opened.id);
                if (!stillThere.view.hidden || stillThere.status.match.state !== "completed") {
                    throw new Error(`hide should keep session and completed work state: ${JSON.stringify(stillThere)}`);
                }

                const shown = await saya.selector.control(opened.id, { command: "show" });
                if (shown.view.hidden || shown.view.cancelled || shown.selectedItem?.id !== "row-0") {
                    throw new Error(`show should redisplay the same selector session: ${JSON.stringify(shown)}`);
                }

                const cancelled = await saya.selector.control(opened.id, { command: "cancel" });
                if (!cancelled.view.hidden || !cancelled.view.cancelled || cancelled.status.match.state !== "cancelled") {
                    throw new Error(`cancel should mark hidden/cancelled and cancel active work: ${JSON.stringify(cancelled)}`);
                }

                await saya.commands.execute(`selector-control:${next.selectedItem.id}:${first.view.cursor}:${stillThere.view.hidden}:${cancelled.status.match.state}`);
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["selector-control:row-1:0:true:cancelled".to_string()]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_selector_headless_view_backend_receives_controller_snapshots() {
    let selector_backend = Arc::new(HeadlessSelectorViewBackend::new());
    let host_bridge = Arc::new(RecordingHostBridge::with_selector_view_backend(
        selector_backend.clone(),
    ));
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async () => {
                const opened = await saya.selector.open({
                    source: {
                        kind: "static",
                        items: Array.from({ length: 12 }, (_, index) => ({
                            id: `row-${index}`,
                            value: `row ${index}`,
                            kind: "test",
                            detail: { index },
                        })),
                    },
                    matcher: "substringAnd",
                    query: "row",
                    limits: { maxRenderedItems: 12 },
                });

                await saya.selector.control(opened.id, { command: "cursorNext" });
                await saya.selector.control(opened.id, { command: "cursorLast" });
                await saya.selector.control(opened.id, { command: "cursorPrevious" });
                await saya.selector.control(opened.id, { command: "pageUp" });
                await saya.selector.control(opened.id, { command: "pageDown" });
                await saya.selector.control(opened.id, { command: "cursorFirst" });
                await saya.selector.control(opened.id, { command: "hide" });
                await saya.selector.control(opened.id, { command: "cancel" });

                await saya.commands.execute(`selector-backend:${opened.id}`);
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    let frames = selector_backend.frames();
    assert_eq!(
        frames.len(),
        9,
        "open plus eight controller commands render"
    );

    let opened = &frames[0];
    assert_eq!(opened.session_id, 1);
    assert_eq!(opened.query, "row");
    assert_eq!(opened.rendered_items.len(), 12);
    assert_eq!(
        opened.selected_item.as_ref().map(|item| item.id.as_str()),
        Some("row-0")
    );
    assert_eq!(opened.cursor, 0);
    assert_eq!(opened.offset, 0);
    assert!(!opened.hidden);
    assert!(!opened.cancelled);
    assert_eq!(opened.status.collect.total_stored, 12);
    assert_eq!(opened.status.match_status.total_matched, 12);
    assert_eq!(opened.status.store.total_stored, 12);

    assert_eq!(frames[1].cursor, 1);
    assert_eq!(
        frames[1]
            .selected_item
            .as_ref()
            .map(|item| item.id.as_str()),
        Some("row-1")
    );
    assert_eq!(frames[2].cursor, 11);
    assert_eq!(frames[2].offset, 2);
    assert_eq!(
        frames[2]
            .selected_item
            .as_ref()
            .map(|item| item.id.as_str()),
        Some("row-11")
    );
    assert_eq!(frames[3].cursor, 10);
    assert_eq!(frames[3].offset, 2);
    assert_eq!(frames[4].cursor, 0);
    assert_eq!(frames[4].offset, 0);
    assert_eq!(frames[5].cursor, 10);
    assert_eq!(frames[5].offset, 10);
    assert_eq!(frames[6].cursor, 0);
    assert_eq!(frames[6].offset, 0);

    let hidden = &frames[7];
    assert!(hidden.hidden);
    assert!(!hidden.cancelled);
    assert_eq!(
        format!("{:?}", hidden.status.match_status.state),
        "Completed"
    );

    let cancelled = &frames[8];
    assert!(cancelled.hidden);
    assert!(cancelled.cancelled);
    assert_eq!(
        format!("{:?}", cancelled.status.match_status.state),
        "Cancelled"
    );

    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["selector-backend:1".to_string()]
    );
    assert_eq!(
        *host_bridge.open_float_calls.lock().await,
        0,
        "headless selector backend must not open floating UI"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_selector_host_adapter_projects_without_opening_float_ui() {
    let projection_sink = Arc::new(HeadlessSelectorUiProjectionSink::new());
    let selector_backend = Arc::new(SelectorHostViewAdapter::new(projection_sink.clone(), 10));
    let host_bridge = Arc::new(RecordingHostBridge::with_selector_view_backend(
        selector_backend.clone(),
    ));
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async () => {
                const opened = await saya.selector.open({
                    source: {
                        kind: "static",
                        items: Array.from({ length: 3 }, (_, index) => ({
                            id: `row-${index}`,
                            value: `row ${index}`,
                            kind: "test",
                            detail: { index },
                        })),
                    },
                    matcher: "substringAnd",
                    query: "row",
                });

                await saya.selector.control(opened.id, { command: "cursorNext" });
                await saya.selector.control(opened.id, { command: "hide" });
                await saya.commands.execute(`selector-host-adapter:${opened.id}`);
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    let projections = projection_sink.projections();
    assert_eq!(projections.len(), 3);
    assert_eq!(projections[0].session_id, 1);
    assert_eq!(projections[0].query, "row");
    assert_eq!(
        projections[1]
            .selected_row
            .as_ref()
            .map(|row| row.item.id.as_str()),
        Some("row-1")
    );
    assert_eq!(
        projections[2].intent,
        saya::features::selector::host_adapter::SelectorUiIntent::Hide
    );
    assert!(!projections[2].should_dispose_session);
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["selector-host-adapter:1".to_string()]
    );
    assert_eq!(
        *host_bridge.open_float_calls.lock().await,
        0,
        "host selector adapter must project model only and must not open floating UI"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_selector_host_adapter_updates_tui_state_without_opening_float_ui() {
    let tui_state = Arc::new(SelectorTuiProjectionSink::new());
    let selector_backend = Arc::new(SelectorHostViewAdapter::new(tui_state.clone(), 10));
    let host_bridge = Arc::new(RecordingHostBridge::with_selector_view_backend(
        selector_backend.clone(),
    ));
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async () => {
                const opened = await saya.selector.open({
                    source: {
                        kind: "static",
                        items: Array.from({ length: 4 }, (_, index) => ({
                            id: `row-${index}`,
                            value: `row ${index}`,
                            kind: "test",
                            detail: { index },
                        })),
                    },
                    matcher: "substringAnd",
                    query: "row",
                });

                await saya.selector.control(opened.id, { command: "cursorNext" });
                await saya.selector.control(opened.id, { command: "cancel" });
                await saya.commands.execute(`selector-tui-state:${opened.id}`);
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    let model = tui_state
        .current_model()
        .expect("runtime host adapter should update TUI selector state");
    assert!(!tui_state.is_visible());
    assert_eq!(model.session_id, 1);
    assert_eq!(model.query, "row");
    assert_eq!(
        model
            .visible_rows
            .iter()
            .map(|row| row.index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(
        model.selected_row.as_ref().map(|row| row.item.id.as_str()),
        Some("row-1")
    );
    assert!(model.cancelled);
    assert!(!model.should_dispose_session);
    assert_eq!(tui_state.projection_count(), 3);
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["selector-tui-state:1".to_string()]
    );
    assert_eq!(
        *host_bridge.open_float_calls.lock().await,
        0,
        "TUI selector state wiring must not open floating UI"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_selector_tui_state_projects_visible_and_hidden_workspace_float_models() {
    let tui_state = Arc::new(SelectorTuiProjectionSink::new());
    let selector_backend = Arc::new(SelectorHostViewAdapter::new(tui_state.clone(), 10));
    let host_bridge = Arc::new(RecordingHostBridge::with_selector_view_backend(
        selector_backend.clone(),
    ));
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async () => {
                const opened = await saya.selector.open({
                    source: {
                        kind: "static",
                        items: [
                            { id: "alpha", value: "alpha row", kind: "test", detail: null },
                            { id: "beta", value: "beta row", kind: "test", detail: null },
                            { id: "gamma", value: "gamma row", kind: "test", detail: null },
                        ],
                    },
                    matcher: "substringAnd",
                    query: "row",
                });
                await saya.commands.execute(`selector-visible:${opened.id}`);

                await saya.selector.control(opened.id, { command: "hide" });
                const hiddenCurrent = await saya.selector.current(opened.id);
                if (!hiddenCurrent.view.hidden || hiddenCurrent.view.cancelled) {
                    throw new Error(`hide should keep session without cancellation: ${JSON.stringify(hiddenCurrent.view)}`);
                }
                await saya.commands.execute(`selector-hidden:${hiddenCurrent.id}:${hiddenCurrent.view.hidden}`);

                const shownCurrent = await saya.selector.control(opened.id, { command: "show" });
                if (shownCurrent.view.hidden || shownCurrent.view.cancelled) {
                    throw new Error(`show should redisplay a hidden selector session: ${JSON.stringify(shownCurrent.view)}`);
                }
                await saya.commands.execute(`selector-shown:${shownCurrent.id}:${shownCurrent.view.hidden}`);

                await saya.selector.control(opened.id, { command: "cancel" });
                const cancelledCurrent = await saya.selector.current(opened.id);
                if (!cancelledCurrent.view.hidden || !cancelledCurrent.view.cancelled) {
                    throw new Error(`cancel should keep hidden cancelled state: ${JSON.stringify(cancelledCurrent.view)}`);
                }
                await saya.commands.execute(`selector-cancelled:${cancelledCurrent.id}:${cancelledCurrent.view.cancelled}`);
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    let current = tui_state
        .current_model()
        .expect("runtime should leave latest selector TUI model");
    assert!(current.cancelled);
    assert!(!current.should_dispose_session);
    assert!(selector_tui_model_to_workspace_float(&current, 80, 24).is_none());
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec![
            "selector-visible:1".to_string(),
            "selector-hidden:1:true".to_string(),
            "selector-shown:1:false".to_string(),
            "selector-cancelled:1:true".to_string()
        ]
    );
    assert_eq!(
        *host_bridge.open_float_calls.lock().await,
        0,
        "selector projection to workspace float must not dispose or open runtime float UI"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_selector_open_ui_window_configures_tui_workspace_float() {
    let tui_state = Arc::new(SelectorTuiProjectionSink::new());
    let selector_backend = Arc::new(SelectorHostViewAdapter::new(tui_state.clone(), 10));
    let host_bridge = Arc::new(RecordingHostBridge::with_selector_view_backend(
        selector_backend.clone(),
    ));
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async () => {
                const opened = await saya.selector.open({
                    source: {
                        kind: "static",
                        items: [
                            { id: "alpha", value: "alpha row", kind: "test", detail: null },
                            { id: "beta", value: "beta row", kind: "test", detail: null },
                            { id: "gamma", value: "gamma row", kind: "test", detail: null },
                            { id: "delta", value: "delta row", kind: "test", detail: null },
                            { id: "epsilon", value: "epsilon row", kind: "test", detail: null },
                        ],
                    },
                    matcher: "substringAnd",
                    query: "row",
                    ui: {
                        window: {
                            width: "75%",
                            height: 9,
                        },
                    },
                });
                await saya.commands.execute(`selector-window:${opened.id}:${opened.renderedItems.length}`);
            }
        "#
        .to_string(),
    }]);

    let runtime = SayaLiveRuntime::spawn_from_seed(host_bridge.clone(), seed)
        .expect("seed runtime should initialize");

    runtime
        .dispatch_event(RuntimeEventPayload::BufferOpen(BufferEventPayload {
            buffer: snapshot_buffer(),
        }))
        .expect("dispatch queued")
        .await_result()
        .await
        .expect("dispatch result");

    let model = tui_state
        .current_model()
        .expect("selector should publish TUI state with window options");
    let visible_float = selector_tui_model_to_workspace_float(&model, 120, 30)
        .expect("configured selector should project to workspace float");
    assert_eq!(visible_float.rect.width, 90);
    assert_eq!(visible_float.rect.height, 9);
    assert_eq!(
        visible_float.lines.len(),
        7,
        "height 9 leaves 5 content rows after border/query/status"
    );
    assert_eq!(
        host_bridge.executed_commands.lock().await.clone(),
        vec!["selector-window:1:5".to_string()]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_session_owner_wires_selector_host_adapter_to_tui_workspace_projection() {
    let seed = CallbackRegistrySeed::from_startup_entries(vec![
        StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: r#"
                async () => {
                    const opened = await saya.selector.open({
                        source: {
                            kind: "static",
                            items: [
                                { id: "alpha", value: "alpha row", kind: "test", detail: null },
                                { id: "beta", value: "beta row", kind: "test", detail: null },
                            ],
                        },
                        matcher: "substringAnd",
                        query: "row",
                    });
                    globalThis.__selectorOwnerTestId = opened.id;
                }
            "#
            .to_string(),
        },
        StartupRegistryEntry::Command {
            name: "selector.hideLatest".to_string(),
            callback_source: r#"
                async () => {
                    await saya.selector.control(globalThis.__selectorOwnerTestId, { command: "hide" });
                }
            "#
            .to_string(),
        },
        StartupRegistryEntry::Command {
            name: "selector.cancelLatest".to_string(),
            callback_source: r#"
                async () => {
                    await saya.selector.control(globalThis.__selectorOwnerTestId, { command: "cancel" });
                }
            "#
            .to_string(),
        },
    ]);
    let mut owner = RuntimeSessionOwner::spawn(seed).expect("runtime owner should initialize");
    let sink = owner.selector_tui_projection_sink();
    let mut host_session = SelectorOwnerTestHostSession;

    let opened = owner
        .dispatch(
            RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: snapshot_buffer(),
            }),
            &mut host_session,
        )
        .await;
    assert!(opened.requires_redraw);
    let visible_model = sink
        .current_model()
        .expect("owner host bridge should update selector TUI state");
    let visible_float = selector_tui_model_to_workspace_float(&visible_model, 80, 24)
        .expect("visible owner selector model should project to workspace float");
    assert_eq!(visible_float.lines[0], "query: row");
    assert_eq!(&visible_float.lines[1..3], ["> alpha row", "  beta row"]);

    let hidden = owner
        .execute_command("selector.hideLatest", &mut host_session)
        .await;
    assert!(hidden.requires_redraw);
    let hidden_model = sink
        .current_model()
        .expect("hide should retain selector TUI state");
    assert!(hidden_model.hidden);
    assert!(!hidden_model.should_dispose_session);
    assert!(selector_tui_model_to_workspace_float(&hidden_model, 80, 24).is_none());

    let cancelled = owner
        .execute_command("selector.cancelLatest", &mut host_session)
        .await;
    assert!(cancelled.requires_redraw);
    let cancelled_model = sink
        .current_model()
        .expect("cancel should retain selector TUI state");
    assert!(cancelled_model.cancelled);
    assert!(!cancelled_model.should_dispose_session);
    assert!(selector_tui_model_to_workspace_float(&cancelled_model, 80, 24).is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_session_owner_controls_active_selector_from_tui_key_routes_headlessly() {
    let seed = CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: r#"
            async () => {
                await saya.selector.open({
                    source: {
                        kind: "static",
                        items: Array.from({ length: 12 }, (_, index) => ({
                            id: `row-${index}`,
                            value: `row ${index}`,
                            kind: "test",
                            detail: { index },
                        })),
                    },
                    matcher: "substringAnd",
                    query: "row",
                    limits: { maxRenderedItems: 12 },
                });
            }
        "#
        .to_string(),
    }]);
    let mut owner = RuntimeSessionOwner::spawn(seed).expect("runtime owner should initialize");
    let sink = owner.selector_tui_projection_sink();
    let mut host_session = SelectorOwnerTestHostSession;

    owner
        .dispatch(
            RuntimeEventPayload::BufferOpen(BufferEventPayload {
                buffer: snapshot_buffer(),
            }),
            &mut host_session,
        )
        .await;

    let initial_model = sink
        .current_model()
        .expect("opened selector should publish TUI state");
    assert_eq!(initial_model.mode, SelectorMode::Insert);
    assert_eq!(
        selector_key_route_for_model(Some(&initial_model), &KeyInput::Down),
        SelectorKeyRoute::Control {
            session_id: initial_model.session_id,
            command:
                saya::features::selector::runtime::RuntimeSelectorControllerCommand::CursorNext,
        }
    );
    assert_eq!(
        selector_key_route_for_model(Some(&initial_model), &KeyInput::Enter),
        SelectorKeyRoute::Action {
            session_id: initial_model.session_id,
            action: SelectorAction::AcceptSelected,
        },
        "Enter should route to selector action instead of controller or normal input"
    );
    assert_eq!(
        selector_key_route_for_model(Some(&initial_model), &KeyInput::Char('j')),
        SelectorKeyRoute::QueryEdit {
            session_id: initial_model.session_id,
            edit: SelectorQueryEdit::Insert('j'),
        },
        "filter input focus should let printable selector navigation letters edit the query"
    );
    assert_eq!(
        selector_key_route_for_model(Some(&initial_model), &KeyInput::Backspace),
        SelectorKeyRoute::QueryEdit {
            session_id: initial_model.session_id,
            edit: SelectorQueryEdit::Backspace,
        },
        "Backspace should edit the active selector query"
    );
    assert_eq!(
        selector_key_route_for_model(Some(&initial_model), &KeyInput::Escape),
        SelectorKeyRoute::ModeSwitch {
            session_id: initial_model.session_id,
            switch: SelectorModeSwitch::EnterNormal,
        },
        "Escape in selector insert mode should switch to selector normal mode"
    );
    sink.set_mode(initial_model.session_id, SelectorMode::Normal)
        .expect("active selector mode should be switchable");
    let normal_model = sink
        .current_model()
        .expect("selector mode switch should keep current model");
    assert_eq!(normal_model.mode, SelectorMode::Normal);
    assert_eq!(
        selector_key_route_for_model(Some(&normal_model), &KeyInput::Char('j')),
        SelectorKeyRoute::Control {
            session_id: normal_model.session_id,
            command:
                saya::features::selector::runtime::RuntimeSelectorControllerCommand::CursorNext,
        },
        "j should navigate in selector normal mode"
    );
    assert_eq!(
        selector_key_route_for_model(Some(&normal_model), &KeyInput::Char('h')),
        SelectorKeyRoute::Noop {
            session_id: normal_model.session_id,
        },
        "h should be consumed by selector normal mode instead of editing the query or buffer"
    );
    assert_eq!(
        selector_key_route_for_model(Some(&normal_model), &KeyInput::Char('l')),
        SelectorKeyRoute::Noop {
            session_id: normal_model.session_id,
        },
        "l should be consumed by selector normal mode instead of editing the query or buffer"
    );
    assert_eq!(
        selector_key_route_for_model(Some(&normal_model), &KeyInput::Backspace),
        SelectorKeyRoute::Noop {
            session_id: normal_model.session_id,
        },
        "Backspace should not edit the query while selector normal mode owns focus"
    );
    assert_eq!(
        selector_key_route_for_model(Some(&normal_model), &KeyInput::Char('i')),
        SelectorKeyRoute::ModeSwitch {
            session_id: normal_model.session_id,
            switch: SelectorModeSwitch::EnterInsert,
        },
        "i should return to selector insert mode"
    );
    assert_eq!(
        selector_key_route_for_model(Some(&normal_model), &KeyInput::Escape),
        SelectorKeyRoute::Control {
            session_id: normal_model.session_id,
            command: saya::features::selector::runtime::RuntimeSelectorControllerCommand::Cancel,
        },
        "Escape in selector normal mode should cancel"
    );
    sink.set_mode(normal_model.session_id, SelectorMode::Insert)
        .expect("selector should return to insert mode for query edit coverage");

    let edit_outcome = owner
        .update_selector_query(
            initial_model.session_id,
            "row 1".to_string(),
            &mut host_session,
        )
        .await;
    assert!(edit_outcome.requires_redraw);
    let edited_model = sink
        .current_model()
        .expect("selector query edit should publish updated TUI state");
    assert_eq!(edited_model.query, "row 1");
    assert_eq!(
        edited_model
            .visible_rows
            .iter()
            .map(|row| row.item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["row-1", "row-10", "row-11"],
        "query edit should re-run substring AND matching through the selector session"
    );
    let edited_float = selector_tui_model_to_workspace_float(&edited_model, 80, 24)
        .expect("edited selector model should remain visible");
    assert_eq!(edited_float.lines[0], "query: row 1");
    assert!(
        edited_float.inline_styles.iter().any(|style| matches!(
            style.kind,
            saya::presentation::floating_window::FloatingInlineStyleKind::Match
        )),
        "updated selector float should retain match highlights"
    );
    let reset_outcome = owner
        .update_selector_query(
            initial_model.session_id,
            "row".to_string(),
            &mut host_session,
        )
        .await;
    assert!(reset_outcome.requires_redraw);

    for (key, expected_cursor) in [
        (KeyInput::Ctrl('n'), 1),
        (KeyInput::Ctrl('N'), 2),
        (KeyInput::Ctrl('p'), 1),
        (KeyInput::Ctrl('P'), 0),
        (KeyInput::Down, 1),
        (KeyInput::Up, 0),
        (KeyInput::PageDown, 10),
        (KeyInput::Ctrl('d'), 11),
        (KeyInput::Ctrl('f'), 11),
        (KeyInput::PageUp, 1),
        (KeyInput::Ctrl('u'), 0),
        (KeyInput::Ctrl('b'), 0),
    ] {
        let before = sink
            .current_model()
            .expect("selector should remain active before key route");
        let SelectorKeyRoute::Control {
            session_id,
            command,
        } = selector_key_route_for_model(Some(&before), &key)
        else {
            panic!("selector key should route to controller command: {key:?}");
        };
        let outcome = owner
            .control_selector(session_id, command, &mut host_session)
            .await;
        assert!(outcome.requires_redraw);
        let after = sink
            .current_model()
            .expect("selector control should publish updated TUI state");
        assert_eq!(
            after.selected_row.as_ref().map(|row| row.index),
            Some(expected_cursor),
            "key {key:?} should update selector cursor"
        );
    }

    let before_cancel_mode = sink
        .current_model()
        .expect("selector should remain active before cancel mode switch");
    sink.set_mode(before_cancel_mode.session_id, SelectorMode::Normal)
        .expect("selector should enter normal mode before Escape cancel");
    let before_cancel = sink
        .current_model()
        .expect("selector should remain active before escape");
    let SelectorKeyRoute::Control {
        session_id,
        command,
    } = selector_key_route_for_model(Some(&before_cancel), &KeyInput::Escape)
    else {
        panic!("escape should route to selector cancel");
    };
    let cancelled = owner
        .control_selector(session_id, command, &mut host_session)
        .await;
    assert!(cancelled.requires_redraw);
    let cancelled_model = sink
        .current_model()
        .expect("cancel should publish hidden TUI state");
    assert!(cancelled_model.cancelled);
    assert_eq!(
        selector_key_route_for_model(Some(&cancelled_model), &KeyInput::Char('j')),
        SelectorKeyRoute::Inactive,
        "cancelled selector must stop intercepting normal input"
    );
    assert_eq!(
        selector_key_route_for_model(Some(&cancelled_model), &KeyInput::Enter),
        SelectorKeyRoute::Inactive,
        "cancelled selector must not intercept Enter"
    );
    assert_eq!(
        selector_key_route_for_model(None, &KeyInput::Char('j')),
        SelectorKeyRoute::Inactive,
        "missing selector state must not intercept normal input"
    );
}

struct RecordingHostBridge {
    executed_commands: Arc<Mutex<Vec<String>>>,
    open_float_calls: Arc<Mutex<usize>>,
    selector_view_backend: Option<Arc<dyn SelectorViewBackend>>,
    prompt_requests: Arc<Mutex<Vec<RuntimeInputPromptRequest>>>,
    prompt_responses: Arc<Mutex<Vec<RuntimeInputPromptResponse>>>,
}

struct SelectorOwnerTestHostSession;

impl RuntimeHostSession for SelectorOwnerTestHostSession {
    fn current_buffer_snapshot(&mut self) -> ReadonlyBufferSnapshot {
        snapshot_buffer()
    }

    fn current_window_snapshot(&mut self) -> ReadonlyWindowSnapshot {
        ReadonlyWindowSnapshot { id: 1 }
    }

    fn current_editor_snapshot(&mut self) -> ReadonlyEditorSnapshot {
        ReadonlyEditorSnapshot {
            mode: RuntimeMode::Normal,
        }
    }

    fn execute_host_command(
        &mut self,
        name: &str,
    ) -> Result<RuntimeCommandEffect, RuntimeCommandError> {
        Err(RuntimeCommandError::UnknownCommand {
            name: name.to_string(),
        })
    }
}

impl RecordingHostBridge {
    fn new() -> Self {
        Self {
            executed_commands: Arc::new(Mutex::new(Vec::new())),
            open_float_calls: Arc::new(Mutex::new(0)),
            selector_view_backend: None,
            prompt_requests: Arc::new(Mutex::new(Vec::new())),
            prompt_responses: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with_selector_view_backend(selector_view_backend: Arc<dyn SelectorViewBackend>) -> Self {
        Self {
            executed_commands: Arc::new(Mutex::new(Vec::new())),
            open_float_calls: Arc::new(Mutex::new(0)),
            selector_view_backend: Some(selector_view_backend),
            prompt_requests: Arc::new(Mutex::new(Vec::new())),
            prompt_responses: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with_prompt_responses(responses: Vec<RuntimeInputPromptResponse>) -> Self {
        Self {
            executed_commands: Arc::new(Mutex::new(Vec::new())),
            open_float_calls: Arc::new(Mutex::new(0)),
            selector_view_backend: None,
            prompt_requests: Arc::new(Mutex::new(Vec::new())),
            prompt_responses: Arc::new(Mutex::new(responses)),
        }
    }
}

impl HostCapabilityBridge for RecordingHostBridge {
    fn execute_host_command(&self, name: &str) -> BoxFuture<Result<(), RuntimeCommandError>> {
        let executed_commands = self.executed_commands.clone();
        let name = name.to_string();
        Box::pin(async move {
            executed_commands.lock().await.push(name);
            Ok(())
        })
    }

    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot> {
        Box::pin(async move { snapshot_buffer() })
    }

    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot> {
        Box::pin(async move { ReadonlyWindowSnapshot { id: 1 } })
    }

    fn open_float(
        &self,
        _request: saya::runtime::live::RuntimeFloatOpenRequest,
    ) -> BoxFuture<Result<saya::runtime::live::RuntimeFloatSnapshot, RuntimeCommandError>> {
        let open_float_calls = self.open_float_calls.clone();
        Box::pin(async move {
            *open_float_calls.lock().await += 1;
            Err(RuntimeCommandError::CommandFailed {
                name: "window.openFloat".to_string(),
                message: "test bridge does not provide floating UI".to_string(),
            })
        })
    }

    fn selector_view_backend(&self) -> Option<Arc<dyn SelectorViewBackend>> {
        self.selector_view_backend.clone()
    }

    fn request_input_prompt(
        &self,
        request: RuntimeInputPromptRequest,
    ) -> BoxFuture<Result<RuntimeInputPromptResponse, RuntimeCommandError>> {
        let prompt_requests = self.prompt_requests.clone();
        let prompt_responses = self.prompt_responses.clone();
        Box::pin(async move {
            prompt_requests.lock().await.push(request);
            let response = {
                let mut responses = prompt_responses.lock().await;
                if responses.is_empty() {
                    RuntimeInputPromptResponse::Cancelled
                } else {
                    responses.remove(0)
                }
            };
            Ok(response)
        })
    }

    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot> {
        Box::pin(async move {
            ReadonlyEditorSnapshot {
                mode: RuntimeMode::Normal,
            }
        })
    }
}

fn snapshot_buffer() -> ReadonlyBufferSnapshot {
    ReadonlyBufferSnapshot {
        id: 1,
        path: Some(PathBuf::from("selector-runtime.md")),
        line_count: 1,
        cursor_row: 0,
        cursor_col: 0,
        current_line: String::new(),
        text: String::new(),
    }
}

fn create_selector_rg_fixture(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "saya-selector-rg-{name}-{}",
        selector_rg_unique_suffix()
    ));
    std::fs::create_dir_all(&path).expect("create rg fixture directory");
    path
}

fn remove_selector_rg_fixture(path: &Path) {
    std::fs::remove_dir_all(path).expect("remove rg fixture directory");
}

fn selector_rg_unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

fn ts_string_literal(path: &Path) -> String {
    serde_json::to_string(&path.to_string_lossy()).expect("path should encode as JSON string")
}
