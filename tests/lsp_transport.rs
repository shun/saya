use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use saya::lsp_transport::{LspServerConfig, LspTransportClient, LspTransportError};
use serde_json::{Value, json};

fn unique_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-lsp-transport-{name}-{nanos}"))
}

fn fake_server_script(dir: &Path) -> PathBuf {
    let script = dir.join("fake_lsp_server.pl");
    fs::create_dir_all(dir).expect("fake server temp dir should be created");
    fs::write(
        &script,
        r#"
use strict;
use warnings;
use JSON::PP qw(decode_json encode_json);
use Cwd qw(getcwd);

binmode(STDIN);
binmode(STDOUT);
$| = 1;

my $mode = $ENV{"SAYA_FAKE_LSP_MODE"} // "normal";

if ($mode eq "malformed-header") {
    print "Content-Length: nope\r\n\r\n{}";
    exit 0;
}

if ($mode eq "immediate-exit") {
    exit 0;
}

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
    my $frame = "Content-Length: " . length($body) . "\r\n\r\n" . $body;
    if ($mode eq "partial") {
        for (my $i = 0; $i < length($frame); $i += 3) {
            print substr($frame, $i, 3);
            select(undef, undef, undef, 0.002);
        }
    } else {
        print $frame;
    }
}

while (defined(my $message = read_message())) {
    my $id = $message->{id};
    my $method = $message->{method} // "";

    if ($mode eq "server-error") {
        write_message({
            jsonrpc => "2.0",
            id => $id,
            error => { code => -32603, message => "fake server error" },
        });
        next;
    }

    if ($method eq "initialize") {
        write_message({
            jsonrpc => "2.0",
            method => "window/logMessage",
            params => { type => 3, message => "fake server initialized" },
        });
        write_message({
            jsonrpc => "2.0",
            id => $id,
            result => {
                capabilities => {
                    hoverProvider => JSON::PP::true,
                    textDocumentSync => 1,
                },
                serverInfo => {
                    cwd => getcwd(),
                    mode => $mode,
                },
            },
        });
        next;
    }

    if ($method eq "textDocument/hover") {
        if ($mode eq "no-response") {
            select(undef, undef, undef, 1.0);
            next;
        }
        if ($mode eq "out-of-order") {
            my $first_id = $id;
            my $next = read_message();
            my $next_id = $next->{id};
            my $next_method = $next->{method} // "";
            write_message({
                jsonrpc => "2.0",
                id => $next_id,
                result => { method => $next_method, order => "second-first" },
            });
            write_message({
                jsonrpc => "2.0",
                id => $first_id,
                result => { contents => { kind => "plaintext", value => "hover from fake server" } },
            });
            next;
        }
        write_message({
            jsonrpc => "2.0",
            method => "textDocument/publishDiagnostics",
            params => { uri => "file:///fake.go", diagnostics => [] },
        });
        write_message({
            jsonrpc => "2.0",
            id => $id,
            result => { contents => { kind => "plaintext", value => "hover from fake server" } },
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

fn fake_server_config_with_dir(mode: &str) -> (LspServerConfig, PathBuf) {
    let dir = unique_path(mode);
    let script = fake_server_script(&dir);
    let mut env = BTreeMap::new();
    env.insert("SAYA_FAKE_LSP_MODE".to_string(), mode.to_string());

    let config = LspServerConfig {
        command: "perl".into(),
        args: vec![script.to_string_lossy().into_owned()],
        env,
        cwd: Some(dir.clone()),
        request_timeout: Duration::from_secs(2),
        startup_timeout: Duration::from_secs(2),
        shutdown_timeout: Duration::from_secs(2),
    };
    (config, dir)
}

fn fake_server_config(mode: &str) -> LspServerConfig {
    fake_server_config_with_dir(mode).0
}

fn diagnostic_json_events(client: &LspTransportClient) -> Vec<Value> {
    client
        .diagnostic_events()
        .into_iter()
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .collect()
}

#[test]
fn lsp_transport_process_routes_responses_notifications_and_shutdown_with_logs() {
    let (config, server_dir) = fake_server_config_with_dir("partial");
    let mut client = LspTransportClient::start(config).expect("fake LSP process should start");

    let initialize = client
        .initialize(json!({ "capabilities": {} }))
        .expect("initialize response should route by request ID");
    assert_eq!(
        initialize
            .get("result")
            .and_then(|result| result.get("capabilities"))
            .and_then(|capabilities| capabilities.get("hoverProvider")),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        initialize
            .pointer("/result/serverInfo/cwd")
            .and_then(Value::as_str),
        Some(
            server_dir
                .canonicalize()
                .expect("fake server dir should canonicalize")
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(
        initialize
            .pointer("/result/serverInfo/mode")
            .and_then(Value::as_str),
        Some("partial")
    );

    let hover = client
        .request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": "file:///fake.go" },
                "position": { "line": 0, "character": 0 },
            }),
        )
        .expect("hover response should route by request ID");
    assert_eq!(
        hover
            .pointer("/result/contents/value")
            .and_then(Value::as_str),
        Some("hover from fake server")
    );

    let notifications = client
        .drain_notifications()
        .expect("notifications should drain");
    let notification_methods = notifications
        .iter()
        .filter_map(|notification| notification.get("method").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(notification_methods.contains(&"window/logMessage"));
    assert!(notification_methods.contains(&"textDocument/publishDiagnostics"));

    client
        .shutdown()
        .expect("shutdown should send shutdown, send exit, and wait for process exit");

    let logs = client.diagnostic_events();
    assert!(logs.iter().any(|line| line.contains("process start")));
    assert!(logs.iter().any(|line| line.contains("request send")));
    assert!(logs.iter().any(|line| line.contains("response receive")));
    assert!(
        logs.iter()
            .any(|line| line.contains("notification receive"))
    );
    assert!(logs.iter().any(|line| line.contains("shutdown complete")));
}

#[test]
fn lsp_transport_emits_structured_json_rpc_diagnostics_and_redacts_document_text() {
    let mut client = LspTransportClient::start(fake_server_config("partial"))
        .expect("fake LSP process should start");

    client
        .initialize(json!({ "capabilities": {}, "trace": "verbose" }))
        .expect("initialize should succeed");
    client
        .send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": "file:///fake.go",
                    "languageId": "go",
                    "version": 1,
                    "text": "secret source text"
                }
            }),
        )
        .expect("didOpen notification should be sent");
    client
        .request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": "file:///fake.go" },
                "position": { "line": 0, "character": 0 },
            }),
        )
        .expect("hover should succeed");

    let logs = client.diagnostic_events();
    assert!(
        logs.iter().all(|line| !line.contains("secret source text")),
        "diagnostic logs must redact document text by default: {logs:?}"
    );

    let events = diagnostic_json_events(&client);
    assert!(
        events.iter().any(|event| {
            event.pointer("/target").and_then(Value::as_str) == Some("lsp_transport")
                && event.pointer("/event").and_then(Value::as_str) == Some("request_send")
                && event.pointer("/requestId").and_then(Value::as_u64) == Some(1)
                && event.pointer("/method").and_then(Value::as_str) == Some("initialize")
        }),
        "initialize request_send event should include requestId and method: {events:?}"
    );
    assert!(
        events.iter().any(|event| {
            event.pointer("/event").and_then(Value::as_str) == Some("notification_send")
                && event.pointer("/method").and_then(Value::as_str) == Some("textDocument/didOpen")
                && event
                    .pointer("/params/textDocument/text")
                    .and_then(Value::as_str)
                    == Some("<redacted>")
        }),
        "didOpen notification should include redacted params: {events:?}"
    );
    assert!(
        events.iter().any(|event| {
            event.pointer("/event").and_then(Value::as_str) == Some("response_receive")
                && event
                    .pointer("/requestId")
                    .and_then(Value::as_u64)
                    .is_some()
        }),
        "response receive event should include requestId: {events:?}"
    );
}

#[test]
fn lsp_transport_emits_structured_timeout_and_shutdown_diagnostics() {
    let mut timeout_client = LspTransportClient::start(LspServerConfig {
        request_timeout: Duration::from_millis(50),
        startup_timeout: Duration::from_secs(2),
        shutdown_timeout: Duration::from_millis(50),
        ..fake_server_config("no-response")
    })
    .expect("fake LSP process should start");

    timeout_client
        .initialize(json!({ "capabilities": {} }))
        .expect("initialize should succeed before timeout request");
    let error = timeout_client
        .request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": "file:///fake.go" },
                "position": { "line": 0, "character": 0 },
            }),
        )
        .expect_err("hover should time out");
    assert!(matches!(error, LspTransportError::Timeout { .. }));
    let timeout_events = diagnostic_json_events(&timeout_client);
    assert!(
        timeout_events.iter().any(|event| {
            event.pointer("/event").and_then(Value::as_str) == Some("timeout")
                && event.pointer("/method").and_then(Value::as_str) == Some("textDocument/hover")
                && event
                    .pointer("/requestId")
                    .and_then(Value::as_u64)
                    .is_some()
        }),
        "timeout event should include phase, method, and requestId: {timeout_events:?}"
    );

    let mut shutdown_client = LspTransportClient::start(fake_server_config("partial"))
        .expect("fake LSP process should start");
    shutdown_client
        .initialize(json!({ "capabilities": {} }))
        .expect("initialize should succeed");
    shutdown_client
        .shutdown()
        .expect("shutdown should complete");
    let shutdown_events = diagnostic_json_events(&shutdown_client);
    assert!(
        shutdown_events.iter().any(|event| {
            event.pointer("/event").and_then(Value::as_str) == Some("process_shutdown_complete")
        }),
        "shutdown completion should be structured: {shutdown_events:?}"
    );
}

#[test]
fn lsp_transport_routes_out_of_order_concurrent_responses_by_request_id() {
    let mut client = LspTransportClient::start(fake_server_config("out-of-order"))
        .expect("fake LSP process should start");
    client
        .initialize(json!({ "capabilities": {} }))
        .expect("initialize should complete before concurrent requests");

    let hover_id = client
        .send_request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": "file:///fake.go" },
                "position": { "line": 0, "character": 0 },
            }),
        )
        .expect("hover request should be sent without waiting");
    let references_id = client
        .send_request(
            "textDocument/references",
            json!({
                "textDocument": { "uri": "file:///fake.go" },
                "position": { "line": 0, "character": 0 },
            }),
        )
        .expect("references request should be sent without waiting");

    let hover = client
        .wait_for_response_by_id(hover_id, "textDocument/hover")
        .expect("hover response should be routed even when it arrives second");
    assert_eq!(
        hover
            .pointer("/result/contents/value")
            .and_then(Value::as_str),
        Some("hover from fake server")
    );
    let references = client
        .wait_for_response_by_id(references_id, "textDocument/references")
        .expect("references response should be retained after arriving first");
    assert_eq!(
        references.pointer("/result/order").and_then(Value::as_str),
        Some("second-first")
    );

    let logs = client.diagnostic_events();
    assert!(
        logs.iter()
            .any(|line| line.contains(&format!("response route: id={hover_id}")))
    );
    assert!(
        logs.iter()
            .any(|line| line.contains(&format!("response route: id={references_id}")))
    );
}

#[test]
fn lsp_transport_reports_malformed_headers_from_fake_server_process() {
    let mut client = LspTransportClient::start(fake_server_config("malformed-header"))
        .expect("fake LSP process should start");

    let error = client
        .request("initialize", json!({}))
        .expect_err("malformed Content-Length should fail the request");
    assert!(matches!(error, LspTransportError::MalformedHeader { .. }));
}

#[test]
fn lsp_transport_reports_server_error_responses() {
    let mut client = LspTransportClient::start(fake_server_config("server-error"))
        .expect("fake LSP process should start");

    let error = client
        .request("initialize", json!({}))
        .expect_err("JSON-RPC error responses should fail the request");
    assert!(matches!(error, LspTransportError::ServerError { .. }));
}

#[test]
fn lsp_transport_reports_process_exit_before_response() {
    let mut client = LspTransportClient::start(fake_server_config("immediate-exit"))
        .expect("fake LSP process should start");

    let error = client
        .request("initialize", json!({}))
        .expect_err("process exit before response should fail the request");
    assert!(matches!(
        error,
        LspTransportError::ProcessExited | LspTransportError::Io { .. }
    ));
}
