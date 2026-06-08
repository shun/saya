//! Managed LSP session end-to-end integration: TS LSP プラグインが
//! `saya.lsp.connect(...)` 経由で Rust host 管理のサーバ session を開き、
//! JSON-RPC over stdio で hover が動くことを検証する。
//!
//! 仕組み:
//! - tmp dir に perl 製の fake LSP server スクリプトを書く
//! - `setupSayaLspClient` で `command: "perl"` と `args: [<script path>]` を設定する
//! - `prepare_init_module` → `collect_startup_registry` → `spawn_from_seed`
//! - `lsp.hover` コマンドを execute し、ホスト bridge が `lsp.floatHover ...`
//!   コマンドを観測することで応答ルーティングが完結したことを確認する
//!
//! 旧 `saya.lsp.request` と汎用 `saya.process.spawn` 依存を一切経由しないことが
//! 重要。host bridge の LSIF bridge 実装は呼ばれた時点で panic させ、
//! managed session 経路だけが使われていることを保証する。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use saya::features::lsp::runtime_bridge::{LspRuntimeBridgeRequest, LspRuntimeBridgeResponse};
use saya::runtime::callback_registry_seed::CallbackRegistrySeed;
use saya::runtime::live::{
    BoxFuture, HostCapabilityBridge, ReadonlyBufferSnapshot, ReadonlyEditorSnapshot,
    ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeMode, SayaLiveRuntime,
};
use saya::runtime::startup::{
    StartupModulePrepareResult, collect_startup_registry, prepare_init_module,
};

fn unique_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-lsp-e2e-{name}-{nanos}"))
}

fn file_uri(path: &Path) -> String {
    format!(
        "file://{}",
        path.to_string_lossy()
            .split('/')
            .map(|part| part.replace(' ', "%20"))
            .collect::<Vec<_>>()
            .join("/")
    )
}

/// perl 製の最小 LSP server。`initialize` には capability を返し、
/// `hover` には固定文字列を返す。`shutdown` / `exit` で終了する。
const FAKE_LSP_PERL_SCRIPT: &str = r#"
use strict;
use warnings;
use JSON::PP qw(decode_json encode_json);

binmode(STDIN);
binmode(STDOUT);
$| = 1;

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

    if ($method eq "initialize") {
        write_message({
            jsonrpc => "2.0",
            id => $id,
            result => {
                capabilities => {
                    hoverProvider => JSON::PP::true,
                    textDocumentSync => 1,
                },
            },
        });
        next;
    }

    if ($method eq "initialized" || $method eq "textDocument/didOpen" || $method eq "textDocument/didChange" || $method eq "textDocument/didSave" || $method eq "textDocument/didClose") {
        next;
    }

    if ($method eq "textDocument/hover") {
        write_message({
            jsonrpc => "2.0",
            id => $id,
            result => {
                contents => {
                    kind => "plaintext",
                    value => "hover-from-perl-fake",
                },
            },
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
"#;

/// LSIF bridge 経路が呼ばれたら即 panic するホスト bridge。
/// host コマンド呼び出しは記録する。managed LSP session 経路だけが
/// 使われていることを保証する。
struct ManagerOnlyHostBridge {
    host_commands: Arc<StdMutex<Vec<String>>>,
    buffer: Arc<StdMutex<ReadonlyBufferSnapshot>>,
}

impl ManagerOnlyHostBridge {
    fn new(buffer: ReadonlyBufferSnapshot) -> Self {
        Self {
            host_commands: Arc::new(StdMutex::new(Vec::new())),
            buffer: Arc::new(StdMutex::new(buffer)),
        }
    }

    fn observed_host_commands(&self) -> Vec<String> {
        self.host_commands
            .lock()
            .expect("host commands lock")
            .clone()
    }
}

impl HostCapabilityBridge for ManagerOnlyHostBridge {
    fn execute_host_command(&self, name: &str) -> BoxFuture<Result<(), RuntimeCommandError>> {
        self.host_commands
            .lock()
            .expect("host commands lock")
            .push(name.to_string());
        Box::pin(async move { Ok(()) })
    }

    fn execute_lsif_request(
        &self,
        request: LspRuntimeBridgeRequest,
    ) -> BoxFuture<Result<LspRuntimeBridgeResponse, RuntimeCommandError>> {
        // Phase C の受入条件: LSP リクエストは新 manager 経路を通り、
        // 旧 typed-bridge (saya.lsp.request → execute_lsp_request) は廃止済み。
        // LSIF 経路だけがこのメソッドを使うため、テスト中に呼ばれた場合は
        // 想定外の dispatch とみなして panic させ、回帰を検知する。
        panic!(
            "manager-only host bridge must not receive LSIF requests: method={}",
            request.method
        );
    }

    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot> {
        let buffer = self.buffer.lock().expect("buffer lock").clone();
        Box::pin(async move { buffer })
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

/// Managed-session E2E: hover が `saya.lsp.connect` 経由で動くこと。
///
/// 旧 bridge 経路が呼ばれたら ManagerOnlyHostBridge が panic するため、
/// managed session 経路だけが使われていることが GREEN の証拠になる。
#[tokio::test(flavor = "current_thread")]
async fn manager_e2e_hover_via_managed_lsp_session_with_perl_fake_server() {
    // 1. workspace + fake server script + ターゲットファイルを作る
    let workspace = unique_path("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    std::fs::write(workspace.join("go.mod"), "module example.com/saya_e2e\n").expect("go.mod");
    let script_path = workspace.join("fake-lsp.pl");
    std::fs::write(&script_path, FAKE_LSP_PERL_SCRIPT).expect("perl script");
    let target_path = workspace.join("main.go");
    let document_text = "package main\n\nfunc main() {}\n";
    std::fs::write(&target_path, document_text).expect("target file");

    // 2. init.ts を組み立てる
    let plugin_path = saya::support::paths::dev_ts_plugins_dir().join("saya-lsp-client.ts");
    let config_path = workspace.join("init.ts");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ setupSayaLspClient }} from "{plugin}";
                setupSayaLspClient({{
                    clientName: "saya-e2e",
                    rootUri: "{root}",
                    languageIdByExtension: {{ go: "go" }},
                    enableBufferEvents: false,
                    servers: [
                        {{
                            name: "fake-perl-lsp",
                            command: "perl",
                            args: ["{script}"],
                            languages: ["go"],
                            filePatterns: ["**/*.go"],
                            rootMarkers: ["go.mod"],
                        }},
                    ],
                }});
            "#,
            plugin = plugin_path.display(),
            root = file_uri(&workspace),
            script = script_path.display(),
        ),
    )
    .expect("init.ts");

    // 3. prepare → registry collect → live runtime spawn
    let prepared = prepare_init_module(&config_path, &workspace);
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("E2E init module should prepare, got: {:?}", prepared);
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("startup registry should collect");
    let buffer = ReadonlyBufferSnapshot {
        id: 7,
        path: Some(target_path.clone()),
        line_count: 3,
        cursor_row: 2,
        cursor_col: 5,
        current_line: "func main() {}".to_string(),
        text: document_text.to_string(),
    };
    let host_bridge = Arc::new(ManagerOnlyHostBridge::new(buffer.clone()));
    let runtime = SayaLiveRuntime::spawn_from_seed(
        host_bridge.clone(),
        CallbackRegistrySeed::from_startup_registry(&registry),
    )
    .expect("E2E runtime should spawn");

    // 4. initialize → hover を 5 秒制限付きで実行する（hang 防止）
    let do_run = async {
        runtime
            .execute_command("lsp.initialize")
            .expect("queue initialize")
            .await_result()
            .await
            .expect("initialize should complete via manager");
        runtime
            .execute_command("lsp.hover")
            .expect("queue hover")
            .await_result()
            .await
            .expect("hover should complete via manager");
        runtime
            .execute_command("lsp.shutdown")
            .expect("queue shutdown")
            .await_result()
            .await
            .expect("shutdown should complete via manager");
    };
    tokio::time::timeout(Duration::from_secs(10), do_run)
        .await
        .expect("E2E flow should complete within 10s");

    // 5. host bridge が lsp.floatHover にルーティングされたコマンドを観測しているか
    let observed = host_bridge.observed_host_commands();
    let saw_hover_route = observed
        .iter()
        .any(|command| command.starts_with("lsp.floatHover "));
    assert!(
        saw_hover_route,
        "hover response must route to lsp.floatHover UI command via manager; observed={observed:?}"
    );
    let saw_status_error = observed
        .iter()
        .any(|command| command.starts_with("lsp.status "));
    assert!(
        !saw_status_error,
        "manager path should not report language server readiness failures; observed={observed:?}"
    );
}
