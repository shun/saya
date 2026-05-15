use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use saya::lsif_index::LsifIndexCache;
use saya::lsp_runtime_bridge::{
    LspRuntimeBridgeRequest, LspRuntimeBridgeSource, LspRuntimePosition, LspRuntimeTextDocument,
};
use saya::saya_live_runtime::{
    ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, RuntimeCommandError, RuntimeMode,
};
use serde_json::json;

fn unique_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-lsif-index-{name}-{nanos}.lsif"))
}

fn file_uri(path: &std::path::Path) -> String {
    format!(
        "file://{}",
        path.to_string_lossy()
            .split('/')
            .map(|part| part.replace(' ', "%20"))
            .collect::<Vec<_>>()
            .join("/")
    )
}

fn lsif_request(dump_path: &std::path::Path, method: &str, uri: &str) -> LspRuntimeBridgeRequest {
    LspRuntimeBridgeRequest {
        source: LspRuntimeBridgeSource::Lsif,
        protocol_version: "0.6.0".to_string(),
        method: method.to_string(),
        client_name: "saya-lsif-test".to_string(),
        root_uri: None,
        language_id: "rust".to_string(),
        trace: "off".to_string(),
        position_encoding: "utf-16".to_string(),
        dump_path: dump_path.to_string_lossy().to_string(),
        text_document: Some(LspRuntimeTextDocument {
            uri: uri.to_string(),
        }),
        server: None,
        position: LspRuntimePosition {
            line: 2,
            character: 6,
        },
        params: Some(json!({
            "textDocument": { "uri": uri },
            "position": { "line": 2, "character": 6 }
        })),
        buffer: ReadonlyBufferSnapshot {
            id: 7,
            path: Some(PathBuf::from("/workspace/src/main.rs")),
            line_count: 8,
            cursor_row: 2,
            cursor_col: 6,
            current_line: "let value = symbol();".to_string(),
            text: "fn main() {\n  let value = symbol();\n}\n".to_string(),
        },
        editor: ReadonlyEditorSnapshot {
            mode: RuntimeMode::Normal,
        },
        event: None,
    }
}

fn new_diagnostic_events() -> Arc<Mutex<Vec<String>>> {
    Arc::new(Mutex::new(Vec::new()))
}

fn collect_events(events: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    events.lock().map(|guard| guard.clone()).unwrap_or_default()
}

#[test]
fn lsif_bridge_resolves_hover_and_definition_from_index_edges() {
    let dump_path = unique_path("hover-definition");
    let document_path = PathBuf::from("/workspace/src/main.rs");
    let definition_path = PathBuf::from("/workspace/src/lib.rs");
    let document_uri = file_uri(&document_path);
    let definition_uri = file_uri(&definition_path);
    std::fs::write(
        &dump_path,
        format!(
            r#"{{"id":1,"type":"vertex","label":"document","uri":{document_uri:?}}}
{{"id":2,"type":"vertex","label":"range","start":{{"line":2,"character":4}},"end":{{"line":2,"character":10}}}}
{{"id":3,"type":"vertex","label":"resultSet"}}
{{"id":4,"type":"vertex","label":"hoverResult","result":{{"contents":{{"kind":"markdown","value":"indexed hover"}}}}}}
{{"id":5,"type":"vertex","label":"definitionResult"}}
{{"id":6,"type":"vertex","label":"document","uri":{definition_uri:?}}}
{{"id":7,"type":"vertex","label":"range","start":{{"line":4,"character":2}},"end":{{"line":4,"character":8}}}}
{{"id":8,"type":"edge","label":"contains","outV":1,"inVs":[2]}}
{{"id":9,"type":"edge","label":"next","outV":2,"inV":3}}
{{"id":10,"type":"edge","label":"textDocument/hover","outV":3,"inV":4}}
{{"id":11,"type":"edge","label":"textDocument/definition","outV":3,"inV":5}}
{{"id":12,"type":"edge","label":"contains","outV":6,"inVs":[7]}}
{{"id":13,"type":"edge","label":"item","outV":5,"inVs":[7],"document":6,"property":"definitions"}}
"#
        ),
    )
    .expect("LSIF fixture should be written");
    let mut cache = LsifIndexCache::default();
    let events = new_diagnostic_events();

    let hover = cache
        .execute_request(
            lsif_request(&dump_path, "textDocument/hover", &document_uri),
            &events,
        )
        .expect("LSIF hover should resolve");
    assert_eq!(hover.source, LspRuntimeBridgeSource::Lsif);
    assert_eq!(hover.result["contents"]["value"], "indexed hover");

    let definition = cache
        .execute_request(
            lsif_request(&dump_path, "textDocument/definition", &document_uri),
            &events,
        )
        .expect("LSIF definition should resolve");
    assert_eq!(definition.source, LspRuntimeBridgeSource::Lsif);
    assert_eq!(definition.result[0]["uri"], definition_uri);
    assert_eq!(definition.result[0]["range"]["start"]["line"], 4);

    let events = collect_events(&events);
    assert!(
        events
            .iter()
            .any(|event| event.contains("lsif index loaded")),
        "expected LSIF load diagnostics, got {events:?}"
    );
}

#[test]
fn lsif_bridge_invalidates_cached_index_when_dump_file_changes() {
    let dump_path = unique_path("cache-invalidation");
    let document_uri = file_uri(&PathBuf::from("/workspace/src/main.rs"));
    let write_fixture = |hover_text: &str| {
        std::fs::write(
            &dump_path,
            format!(
                r#"{{"id":1,"type":"vertex","label":"document","uri":{document_uri:?}}}
{{"id":2,"type":"vertex","label":"range","start":{{"line":2,"character":4}},"end":{{"line":2,"character":10}}}}
{{"id":3,"type":"vertex","label":"resultSet"}}
{{"id":4,"type":"vertex","label":"hoverResult","result":{{"contents":{{"kind":"plaintext","value":{hover_text:?}}}}}}}
{{"id":5,"type":"edge","label":"contains","outV":1,"inVs":[2]}}
{{"id":6,"type":"edge","label":"next","outV":2,"inV":3}}
{{"id":7,"type":"edge","label":"textDocument/hover","outV":3,"inV":4}}
"#
            ),
        )
        .expect("LSIF fixture should be written");
    };
    write_fixture("first hover");
    let mut cache = LsifIndexCache::default();
    let events = new_diagnostic_events();

    let first = cache
        .execute_request(
            lsif_request(&dump_path, "textDocument/hover", &document_uri),
            &events,
        )
        .expect("first LSIF hover should resolve");
    assert_eq!(first.result["contents"]["value"], "first hover");

    write_fixture("second hover with longer text");
    let second = cache
        .execute_request(
            lsif_request(&dump_path, "textDocument/hover", &document_uri),
            &events,
        )
        .expect("second LSIF hover should resolve after dump change");
    assert_eq!(
        second.result["contents"]["value"],
        "second hover with longer text"
    );

    let events = collect_events(&events);
    assert!(
        events
            .iter()
            .any(|event| event.contains("lsif index cache invalidated")),
        "expected LSIF cache invalidation diagnostics, got {events:?}"
    );
}

#[test]
fn lsif_bridge_reports_missing_lookup_without_lsp_fallback() {
    let dump_path = unique_path("missing");
    let document_uri = file_uri(&PathBuf::from("/workspace/src/main.rs"));
    std::fs::write(
        &dump_path,
        format!(
            r#"{{"id":1,"type":"vertex","label":"document","uri":{document_uri:?}}}
{{"id":2,"type":"vertex","label":"range","start":{{"line":2,"character":4}},"end":{{"line":2,"character":10}}}}
{{"id":3,"type":"edge","label":"contains","outV":1,"inVs":[2]}}
"#
        ),
    )
    .expect("LSIF fixture should be written");
    let mut cache = LsifIndexCache::default();
    let events = new_diagnostic_events();

    let error = cache
        .execute_request(
            lsif_request(&dump_path, "textDocument/hover", &document_uri),
            &events,
        )
        .expect_err("missing LSIF hover should return a bridge error");
    match error {
        RuntimeCommandError::CommandFailed { message, .. } => {
            assert!(
                message.contains("LSIF hover not found"),
                "unexpected LSIF error: {message}"
            );
        }
        other => panic!("unexpected runtime error: {other:?}"),
    }
}
