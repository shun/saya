use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use saya::lsp_runtime_bridge::{
    LspRuntimeBridgeRequest, LspRuntimeBridgeSource, LspRuntimePosition,
    LspRuntimeServerDefinition, LspRuntimeTextDocument,
};
use saya::lsp_session::{LspSessionManager, LspSessionRequestOptions};
use saya::saya_live_runtime::{ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, RuntimeMode};
use serde_json::{Value, json};

fn unique_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-lsp-session-{name}-{nanos}"))
}

fn fake_server_script(dir: &Path) -> PathBuf {
    let script = dir.join("fake_lsp_session_server.pl");
    fs::create_dir_all(dir).expect("fake server temp dir should be created");
    fs::write(
        &script,
        r#"
use strict;
use warnings;
use JSON::PP qw(decode_json encode_json);

binmode(STDIN);
binmode(STDOUT);
$| = 1;

my $mode = $ENV{"SAYA_FAKE_LSP_SESSION_MODE"} // "normal";
my $hover_count = 0;

sub read_message {
    my $content_length;
    while (defined(my $line = <STDIN>)) {
        $line =~ s/\r?\n$//;
        last if $line eq "";
        if ($line =~ /^Content-Length:\s*(\d+)/i) {
            $content_length = int($1);
        }
    }
    return undef unless defined $content_length;

    my $body = "";
    my $read = read(STDIN, $body, $content_length);
    die "short body read" unless defined($read) && $read == $content_length;
    return decode_json($body);
}

sub write_message {
    my ($message) = @_;
    my $body = encode_json($message);
    print "Content-Length: " . length($body) . "\r\n\r\n" . $body;
}

while (defined(my $message = read_message())) {
    my $id = $message->{id};
    my $method = $message->{method} // "";

    if ($mode eq "exit-after-initialize" && $method ne "initialize") {
        exit 0;
    }

    if ($method eq "initialize") {
        write_message({
            jsonrpc => "2.0",
            id => $id,
            result => {
                capabilities => {
                    hoverProvider => JSON::PP::true,
                    textDocumentSync => 1,
                },
                serverInfo => { pid => $$ },
            },
        });
        next;
    }

    if ($method eq "textDocument/didOpen" || $method eq "textDocument/didSave" || $method eq "initialized") {
        next;
    }

    if ($method eq "textDocument/hover") {
        $hover_count += 1;
        if ($mode eq "slow-hover") {
            select(undef, undef, undef, 0.08);
        }
        write_message({
            jsonrpc => "2.0",
            id => $id,
            result => {
                contents => {
                    kind => "plaintext",
                    value => "hover " . $hover_count . " from pid " . $$,
                },
            },
        });
        next;
    }

    if ($method eq "textDocument/references") {
        write_message({
            jsonrpc => "2.0",
            id => $id,
            result => [{
                uri => $message->{params}->{textDocument}->{uri},
                range => {
                    start => { line => 0, character => 0 },
                    end => { line => 0, character => 4 },
                },
            }],
        });
        next;
    }

    if ($method eq "shutdown") {
        write_message({ jsonrpc => "2.0", id => $id, result => undef });
        next;
    }

    if ($method eq "exit") {
        exit 0;
    }
}

"#,
    )
    .expect("fake server script should be written");
    script
}

fn server_definition(mode: &str, dir: &Path) -> LspRuntimeServerDefinition {
    let script = fake_server_script(dir);
    let mut env = BTreeMap::new();
    env.insert("SAYA_FAKE_LSP_SESSION_MODE".to_string(), mode.to_string());
    LspRuntimeServerDefinition {
        name: "fake-lsp".to_string(),
        command: "perl".to_string(),
        args: vec![script.to_string_lossy().into_owned()],
        env,
        cwd: Some(dir.to_string_lossy().into_owned()),
        root_markers: vec!["go.mod".to_string()],
        initialization_options: None,
    }
}

fn lsp_request(
    method: &str,
    uri: &str,
    server: LspRuntimeServerDefinition,
    params: Value,
) -> LspRuntimeBridgeRequest {
    LspRuntimeBridgeRequest {
        source: LspRuntimeBridgeSource::Lsp,
        protocol_version: "3.17".to_string(),
        method: method.to_string(),
        client_name: "saya-test".to_string(),
        root_uri: Some("file:///tmp/saya-lsp-session".to_string()),
        language_id: "go".to_string(),
        trace: "off".to_string(),
        position_encoding: "utf-16".to_string(),
        dump_path: String::new(),
        text_document: Some(LspRuntimeTextDocument {
            uri: uri.to_string(),
        }),
        server: Some(server),
        position: LspRuntimePosition {
            line: 0,
            character: 0,
        },
        params: Some(params),
        buffer: ReadonlyBufferSnapshot {
            id: 1,
            path: None,
            line_count: 1,
            cursor_row: 0,
            cursor_col: 0,
            current_line: "package main".to_string(),
            text: "package main\n".to_string(),
        },
        editor: ReadonlyEditorSnapshot {
            mode: RuntimeMode::Normal,
        },
        event: None,
    }
}

fn diagnostic_json_events(manager: &LspSessionManager) -> Vec<Value> {
    manager
        .diagnostic_events()
        .into_iter()
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .collect()
}

#[test]
fn lsp_session_manager_stress_handles_concurrent_hover_references_changes_and_restart() {
    let manager = LspSessionManager::default();
    let dir = unique_path("stress");
    let server = server_definition("normal", &dir);
    let uri = "file:///tmp/saya-lsp-session/stress.go";

    let initialize = manager
        .execute_blocking(lsp_request(
            "initialize",
            uri,
            server.clone(),
            json!({ "capabilities": {} }),
        ))
        .expect("initialize should start fake server");
    let first_pid = initialize
        .result
        .pointer("/result/serverInfo/pid")
        .and_then(Value::as_i64)
        .expect("fake server should report pid");
    manager
        .execute_blocking(lsp_request(
            "textDocument/didOpen",
            uri,
            server.clone(),
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "go",
                    "version": 1,
                    "text": "package main\n"
                }
            }),
        ))
        .expect("didOpen should mark document as open");

    let did_save = manager.submit(
        lsp_request(
            "textDocument/didSave",
            uri,
            server.clone(),
            json!({
                "textDocument": { "uri": uri },
                "text": "package main\nfunc main() {}\n",
            }),
        ),
        LspSessionRequestOptions::default(),
    );
    let hover = manager.submit(
        lsp_request(
            "textDocument/hover",
            uri,
            server.clone(),
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 0 },
            }),
        ),
        LspSessionRequestOptions::default(),
    );
    let references = manager.submit(
        lsp_request(
            "textDocument/references",
            uri,
            server.clone(),
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 0 },
            }),
        ),
        LspSessionRequestOptions::default(),
    );

    did_save
        .recv_timeout(Duration::from_secs(2))
        .expect("didSave should complete")
        .expect("didSave should succeed");
    hover
        .recv_timeout(Duration::from_secs(2))
        .expect("hover should complete")
        .expect("hover should succeed");
    let references_response = references
        .recv_timeout(Duration::from_secs(2))
        .expect("references should complete")
        .expect("references should succeed");
    assert_eq!(
        references_response
            .result
            .pointer("/result/0/uri")
            .and_then(Value::as_str),
        Some(uri)
    );

    manager
        .execute_blocking(lsp_request("shutdown", uri, server.clone(), Value::Null))
        .expect("shutdown should stop the session worker");
    let restarted = manager
        .execute_blocking(lsp_request(
            "initialize",
            uri,
            server.clone(),
            json!({ "capabilities": {} }),
        ))
        .expect("initialize after shutdown should restart the session worker");
    let second_pid = restarted
        .result
        .pointer("/result/serverInfo/pid")
        .and_then(Value::as_i64)
        .expect("restarted fake server should report pid");
    assert_ne!(first_pid, second_pid, "restart should spawn a new process");

    let logs = manager.diagnostic_events();
    assert!(logs.iter().any(|line| line.contains("session restart")));
    assert!(logs.iter().any(|line| line.contains("request dispatch")));
}

#[test]
fn lsp_session_manager_includes_server_root_request_and_trace_context_in_diagnostics() {
    let manager = LspSessionManager::default();
    let dir = unique_path("diagnostics");
    let server = server_definition("normal", &dir);
    let uri = "file:///tmp/saya-lsp-session/diagnostics.go";
    let root_uri = "file:///tmp/saya-lsp-session";

    let mut initialize = lsp_request(
        "initialize",
        uri,
        server.clone(),
        json!({ "capabilities": {} }),
    );
    initialize.trace = "messages".to_string();
    manager
        .execute_blocking(initialize)
        .expect("initialize should start fake server");
    manager
        .execute_blocking(lsp_request(
            "textDocument/didOpen",
            uri,
            server.clone(),
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "go",
                    "version": 1,
                    "text": "diagnostic secret text"
                }
            }),
        ))
        .expect("didOpen should mark document as open");
    manager
        .execute_blocking(lsp_request(
            "textDocument/hover",
            uri,
            server.clone(),
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 0 },
            }),
        ))
        .expect("hover should succeed");

    let logs = manager.diagnostic_events();
    assert!(
        logs.iter()
            .all(|line| !line.contains("diagnostic secret text")),
        "session diagnostics must not leak document text: {logs:?}"
    );

    let events = diagnostic_json_events(&manager);
    assert!(
        events.iter().any(|event| {
            event.pointer("/target").and_then(Value::as_str) == Some("lsp_session")
                && event.pointer("/event").and_then(Value::as_str) == Some("request_dispatch")
                && event.pointer("/serverName").and_then(Value::as_str) == Some("fake-lsp")
                && event.pointer("/workspaceRoot").and_then(Value::as_str) == Some(root_uri)
                && event.pointer("/method").and_then(Value::as_str) == Some("initialize")
                && event.pointer("/trace").and_then(Value::as_str) == Some("messages")
        }),
        "session request dispatch should include server, root, method, and trace: {events:?}"
    );
    assert!(
        events.iter().any(|event| {
            event.pointer("/target").and_then(Value::as_str) == Some("lsp_transport")
                && event.pointer("/event").and_then(Value::as_str) == Some("request_send")
                && event.pointer("/method").and_then(Value::as_str) == Some("initialize")
                && event.pointer("/params/trace").and_then(Value::as_str) == Some("messages")
        }),
        "initialize params should map trace mode to LSP trace field: {events:?}"
    );
}

#[test]
fn lsp_session_manager_reuses_session_and_rejects_unopened_documents() {
    let manager = LspSessionManager::default();
    let dir = unique_path("reuse");
    let server = server_definition("normal", &dir);
    let uri = "file:///tmp/saya-lsp-session/main.go";

    let initialize = manager
        .execute_blocking(lsp_request(
            "initialize",
            uri,
            server.clone(),
            json!({ "capabilities": {} }),
        ))
        .expect("initialize should start fake server");
    let first_pid = initialize
        .result
        .pointer("/result/serverInfo/pid")
        .and_then(Value::as_i64)
        .expect("fake server should report pid");

    let unopened_hover = manager
        .execute_blocking(lsp_request(
            "textDocument/hover",
            uri,
            server.clone(),
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 0 },
            }),
        ))
        .expect_err("document request should be rejected before didOpen");
    assert!(
        format!("{unopened_hover:?}").contains("not open"),
        "unexpected unopened document error: {unopened_hover:?}"
    );

    manager
        .execute_blocking(lsp_request(
            "textDocument/didOpen",
            uri,
            server.clone(),
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "go",
                    "version": 1,
                    "text": "package main\n"
                }
            }),
        ))
        .expect("didOpen should mark document as open");

    let hover = manager
        .execute_blocking(lsp_request(
            "textDocument/hover",
            uri,
            server.clone(),
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 0 },
            }),
        ))
        .expect("hover should reach opened document");
    let hover_text = hover
        .result
        .pointer("/result/contents/value")
        .and_then(Value::as_str)
        .expect("hover response text");
    assert!(
        hover_text.contains(&format!("pid {first_pid}")),
        "session should be reused; hover response was {hover_text:?}"
    );

    let logs = manager.diagnostic_events();
    assert!(logs.iter().any(|line| line.contains("session start")));
    assert!(logs.iter().any(|line| line.contains("session reuse")));
}

#[test]
fn lsp_session_manager_cancels_stale_queued_requests_and_reports_exit() {
    let manager = LspSessionManager::default();
    let dir = unique_path("cancel");
    let server = server_definition("slow-hover", &dir);
    let uri = "file:///tmp/saya-lsp-session/slow.go";

    manager
        .execute_blocking(lsp_request(
            "initialize",
            uri,
            server.clone(),
            json!({ "capabilities": {} }),
        ))
        .expect("initialize should start fake server");
    manager
        .execute_blocking(lsp_request(
            "textDocument/didOpen",
            uri,
            server.clone(),
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "go",
                    "version": 1,
                    "text": "package main\n"
                }
            }),
        ))
        .expect("didOpen should mark document as open");

    let first = manager.submit(
        lsp_request(
            "textDocument/hover",
            uri,
            server.clone(),
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 0 },
            }),
        ),
        LspSessionRequestOptions::default(),
    );
    let stale = manager.submit(
        lsp_request(
            "textDocument/hover",
            uri,
            server.clone(),
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 0 },
            }),
        ),
        LspSessionRequestOptions::default(),
    );
    stale.cancel();

    first
        .recv_timeout(Duration::from_secs(2))
        .expect("first hover should reply")
        .expect("first hover should succeed");
    let stale_result = stale
        .recv_timeout(Duration::from_secs(2))
        .expect("stale hover should complete with cancellation");
    assert!(
        format!("{stale_result:?}").contains("cancelled"),
        "unexpected stale result: {stale_result:?}"
    );

    let exit_dir = unique_path("exit");
    let exit_server = server_definition("exit-after-initialize", &exit_dir);
    manager
        .execute_blocking(lsp_request(
            "initialize",
            uri,
            exit_server.clone(),
            json!({ "capabilities": {} }),
        ))
        .expect("exit test initialize should start fake server");
    manager
        .execute_blocking(lsp_request(
            "textDocument/didOpen",
            uri,
            exit_server.clone(),
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "go",
                    "version": 1,
                    "text": "package main\n"
                }
            }),
        ))
        .expect("didOpen notification may be accepted before the reader observes process exit");
    manager
        .execute_blocking(lsp_request(
            "textDocument/hover",
            uri,
            exit_server.clone(),
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 0 },
            }),
        ))
        .expect_err("unexpected exit should be reported on the next request");
}
