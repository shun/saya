//! `saya.process` op レイヤー (`Deno.core.ops.op_process_*`) の契約テスト。
//!
//! Phase A.2 のスコープに沿って、TS bootstrap ラッパ
//! (`saya.process.spawn(...)`) ではなく、op を `Deno.core.ops` 経由で
//! 直接呼び出して挙動を検証する。bootstrap ラッパの統合テストは Phase
//! A.3 で扱う。
//!
//! 本ファイルは「JsBuffer の zero-copy 受け渡しが成立しているか」「ハ
//! ンドル ID が正しく払い出されるか」「shutdown_all が runtime 終了時
//! に走るか」など、Rust 側 op 実装と `ProcessPool` 統合の境界条件を直
//! 接観測する。
//!
//! テスト戦略:
//! - `SayaLiveRuntime::spawn_from_seed` で seed runtime を起動する
//! - 起動したランタイムへ `bufferOpen` イベントを `dispatch_event` で
//!   流し込み、その handler 内で TS から op を呼ぶ
//! - op 戻り値の検証は handler 内で `throw` するか、検証成功時にだけ
//!   `saya.commands.execute("ok")` を呼ぶ形で `RecordingHostBridge`
//!   側に痕跡を残し、Rust 側で確認する

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use saya::callback_registry_seed::CallbackRegistrySeed;
use saya::saya_live_runtime::{
    BoxFuture, BufferEventPayload, HostCapabilityBridge, ReadonlyBufferSnapshot,
    ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeEventPayload,
    RuntimeMode, SayaLiveRuntime,
};
use saya::startup_runtime::StartupRegistryEntry;
use tokio::sync::Mutex;

/// 検証用の `HostCapabilityBridge`。
///
/// TS handler が `saya.commands.execute("...")` を呼んだ際に、コマンド
/// 名を共有 Mutex に蓄積する。Phase A.2 op テストでは、TS 側で「成功
/// パス」を踏んだ場合だけ特定コマンドを execute することで、外側 (Rust
/// 側) から検証できるようにする。
struct RecordingHostBridge {
    executed_commands: Arc<Mutex<Vec<String>>>,
    command_results: HashMap<String, RuntimeCommandError>,
}

impl RecordingHostBridge {
    fn new() -> Self {
        Self {
            executed_commands: Arc::new(Mutex::new(Vec::new())),
            command_results: HashMap::new(),
        }
    }

    fn shared_executed_commands(&self) -> Arc<Mutex<Vec<String>>> {
        self.executed_commands.clone()
    }
}

impl HostCapabilityBridge for RecordingHostBridge {
    fn execute_host_command(&self, name: &str) -> BoxFuture<Result<(), RuntimeCommandError>> {
        let executed_commands = self.executed_commands.clone();
        let name_owned = name.to_string();
        let result = self.command_results.get(&name_owned).cloned();
        Box::pin(async move {
            if let Some(error) = result {
                return Err(error);
            }
            executed_commands.lock().await.push(name_owned);
            Ok(())
        })
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

fn buffer_open_payload() -> RuntimeEventPayload {
    RuntimeEventPayload::BufferOpen(BufferEventPayload {
        buffer: ReadonlyBufferSnapshot {
            id: 1,
            path: Some(PathBuf::from("saya-process-op.test.md")),
            line_count: 1,
            cursor_row: 0,
            cursor_col: 0,
            current_line: String::new(),
            text: String::new(),
        },
    })
}

/// TS で実行する script 本体を `bufferOpen` ハンドラとして seed に
/// 登録するためのヘルパ。
fn seed_with_handler(handler_source: &str) -> CallbackRegistrySeed {
    CallbackRegistrySeed::from_startup_entries(vec![StartupRegistryEntry::Event {
        name: "bufferOpen".to_string(),
        callback_source: handler_source.to_string(),
    }])
}

/// T-OP-1: `op_process_spawn` を JSON で呼ぶと数値ハンドルが返り、
/// `saya.commands.execute("op-spawn-ok")` が呼ばれる。
#[tokio::test(flavor = "current_thread")]
async fn op_process_spawn_returns_numeric_handle_for_cat_session() {
    let bridge = Arc::new(RecordingHostBridge::new());
    let executed = bridge.shared_executed_commands();

    let handler = r#"
        async (_payload) => {
            const spec = JSON.stringify({
                command: "cat",
                args: ["-u"],
                env: {},
                cwd: null,
                stdin: "piped",
                stdout: "piped",
                stderr: "piped",
            });
            const handle = await Deno.core.ops.op_process_spawn(spec);
            if (typeof handle !== "number") {
                throw new Error("expected numeric handle, got " + typeof handle);
            }
            if (handle < 1) {
                throw new Error("expected handle >= 1, got " + handle);
            }
            await Deno.core.ops.op_process_kill(handle);
            await Deno.core.ops.op_process_wait(handle);
            await saya.commands.execute("op-spawn-ok");
        }
    "#;

    let runtime = SayaLiveRuntime::spawn_from_seed(bridge.clone(), seed_with_handler(handler))
        .expect("seed runtime should initialize");

    let receipt = runtime
        .dispatch_event(buffer_open_payload())
        .expect("dispatch should succeed");
    let report = receipt.await_result().await.expect("dispatch result");
    assert_eq!(report.handler_count, 1);

    let names = executed.lock().await.clone();
    assert_eq!(names, vec!["op-spawn-ok".to_string()]);
}

/// T-OP-2: `op_process_write_stdin` + `op_process_read_stdout` の
/// round-trip が成立する（zero-copy で Uint8Array に書き戻される）。
#[tokio::test(flavor = "current_thread")]
async fn op_process_write_stdin_then_read_stdout_round_trip() {
    let bridge = Arc::new(RecordingHostBridge::new());
    let executed = bridge.shared_executed_commands();

    // deno_core の seed runtime には TextEncoder/TextDecoder が無いため、
    // ASCII の payload を `charCodeAt` 経由で `Uint8Array` に詰める形で
    // 検証する（Phase A.2 の op レイヤー検証では Web API に依存させない）。
    let handler = r#"
        async (_payload) => {
            const spec = JSON.stringify({
                command: "cat",
                args: ["-u"],
                env: {},
                cwd: null,
                stdin: "piped",
                stdout: "piped",
                stderr: "piped",
            });
            const handle = await Deno.core.ops.op_process_spawn(spec);

            const payloadText = "saya-process op round trip\n";
            const payload = new Uint8Array(payloadText.length);
            for (let i = 0; i < payloadText.length; i += 1) {
                payload[i] = payloadText.charCodeAt(i) & 0xff;
            }
            const written = await Deno.core.ops.op_process_write_stdin(handle, payload);
            if (written !== payload.byteLength) {
                throw new Error("write returned " + written + " expected " + payload.byteLength);
            }

            const buf = new Uint8Array(64);
            const n = await Deno.core.ops.op_process_read_stdout(handle, buf);
            if (typeof n !== "number") {
                throw new Error("read should return number, got " + typeof n);
            }
            if (n !== payload.byteLength) {
                throw new Error("read returned " + n + " expected " + payload.byteLength);
            }
            for (let i = 0; i < payload.byteLength; i += 1) {
                if (buf[i] !== payload[i]) {
                    throw new Error("echo byte mismatch at " + i + ": got " + buf[i] + " expected " + payload[i]);
                }
            }

            await Deno.core.ops.op_process_kill(handle);
            await Deno.core.ops.op_process_wait(handle);
            await saya.commands.execute("op-roundtrip-ok");
        }
    "#;

    let runtime = SayaLiveRuntime::spawn_from_seed(bridge.clone(), seed_with_handler(handler))
        .expect("seed runtime should initialize");

    let receipt = runtime
        .dispatch_event(buffer_open_payload())
        .expect("dispatch should succeed");
    let report = tokio::time::timeout(Duration::from_secs(5), receipt.await_result())
        .await
        .expect("round-trip should complete within 5s")
        .expect("dispatch result");
    assert_eq!(report.handler_count, 1);

    let names = executed.lock().await.clone();
    assert_eq!(names, vec!["op-roundtrip-ok".to_string()]);
}

/// T-OP-3: `true` プロセスが exit したあと `op_process_read_stdout` は
/// EOF (0) を返す。
#[tokio::test(flavor = "current_thread")]
async fn op_process_read_stdout_returns_zero_after_child_exits() {
    let bridge = Arc::new(RecordingHostBridge::new());
    let executed = bridge.shared_executed_commands();

    let handler = r#"
        async (_payload) => {
            const spec = JSON.stringify({
                command: "true",
                args: [],
                env: {},
                cwd: null,
                stdin: "null",
                stdout: "piped",
                stderr: "piped",
            });
            const handle = await Deno.core.ops.op_process_spawn(spec);
            const buf = new Uint8Array(64);
            const n = await Deno.core.ops.op_process_read_stdout(handle, buf);
            if (n !== 0) {
                throw new Error("expected EOF (0), got " + n);
            }
            const code = await Deno.core.ops.op_process_wait(handle);
            if (code !== 0) {
                throw new Error("expected exit code 0, got " + code);
            }
            await saya.commands.execute("op-eof-ok");
        }
    "#;

    let runtime = SayaLiveRuntime::spawn_from_seed(bridge.clone(), seed_with_handler(handler))
        .expect("seed runtime should initialize");

    let receipt = runtime
        .dispatch_event(buffer_open_payload())
        .expect("dispatch should succeed");
    let report = tokio::time::timeout(Duration::from_secs(5), receipt.await_result())
        .await
        .expect("EOF check should complete within 5s")
        .expect("dispatch result");
    assert_eq!(report.handler_count, 1);

    let names = executed.lock().await.clone();
    assert_eq!(names, vec!["op-eof-ok".to_string()]);
}

/// T-OP-4: `op_process_kill` の後 `op_process_wait` で非ゼロ exit code
/// が返り、`AlreadyKilled` 等ではエラーにならない。
#[tokio::test(flavor = "current_thread")]
async fn op_process_kill_then_wait_returns_non_zero_exit_code() {
    let bridge = Arc::new(RecordingHostBridge::new());
    let executed = bridge.shared_executed_commands();

    let handler = r#"
        async (_payload) => {
            const spec = JSON.stringify({
                command: "cat",
                args: ["-u"],
                env: {},
                cwd: null,
                stdin: "piped",
                stdout: "piped",
                stderr: "piped",
            });
            const handle = await Deno.core.ops.op_process_spawn(spec);
            await Deno.core.ops.op_process_kill(handle);
            const code = await Deno.core.ops.op_process_wait(handle);
            if (code === 0) {
                throw new Error("expected non-zero exit code after kill, got 0");
            }
            await saya.commands.execute("op-kill-ok");
        }
    "#;

    let runtime = SayaLiveRuntime::spawn_from_seed(bridge.clone(), seed_with_handler(handler))
        .expect("seed runtime should initialize");

    let receipt = runtime
        .dispatch_event(buffer_open_payload())
        .expect("dispatch should succeed");
    let report = tokio::time::timeout(Duration::from_secs(5), receipt.await_result())
        .await
        .expect("kill+wait should complete within 5s")
        .expect("dispatch result");
    assert_eq!(report.handler_count, 1);

    let names = executed.lock().await.clone();
    assert_eq!(names, vec!["op-kill-ok".to_string()]);
}

/// T-OP-5: `op_process_read_stderr` が stdout と独立して動作する。
#[tokio::test(flavor = "current_thread")]
async fn op_process_read_stderr_independent_from_stdout() {
    let bridge = Arc::new(RecordingHostBridge::new());
    let executed = bridge.shared_executed_commands();

    let handler = r#"
        async (_payload) => {
            const spec = JSON.stringify({
                command: "sh",
                args: ["-c", "printf out; printf err 1>&2"],
                env: {},
                cwd: null,
                stdin: "null",
                stdout: "piped",
                stderr: "piped",
            });
            const handle = await Deno.core.ops.op_process_spawn(spec);

            const outBuf = new Uint8Array(16);
            const nOut = await Deno.core.ops.op_process_read_stdout(handle, outBuf);
            const errBuf = new Uint8Array(16);
            const nErr = await Deno.core.ops.op_process_read_stderr(handle, errBuf);

            // ASCII の "out" / "err" を charCode で逐次比較する（seed
            // runtime に TextDecoder が無いため）。
            const expectedOut = [111, 117, 116]; // "out"
            const expectedErr = [101, 114, 114]; // "err"
            if (nOut !== expectedOut.length) {
                throw new Error("stdout length mismatch: got " + nOut);
            }
            if (nErr !== expectedErr.length) {
                throw new Error("stderr length mismatch: got " + nErr);
            }
            for (let i = 0; i < expectedOut.length; i += 1) {
                if (outBuf[i] !== expectedOut[i]) {
                    throw new Error("stdout byte mismatch at " + i + ": got " + outBuf[i]);
                }
            }
            for (let i = 0; i < expectedErr.length; i += 1) {
                if (errBuf[i] !== expectedErr[i]) {
                    throw new Error("stderr byte mismatch at " + i + ": got " + errBuf[i]);
                }
            }

            await Deno.core.ops.op_process_wait(handle);
            await saya.commands.execute("op-stderr-ok");
        }
    "#;

    let runtime = SayaLiveRuntime::spawn_from_seed(bridge.clone(), seed_with_handler(handler))
        .expect("seed runtime should initialize");

    let receipt = runtime
        .dispatch_event(buffer_open_payload())
        .expect("dispatch should succeed");
    let report = tokio::time::timeout(Duration::from_secs(5), receipt.await_result())
        .await
        .expect("stderr round-trip should complete within 5s")
        .expect("dispatch result");
    assert_eq!(report.handler_count, 1);

    let names = executed.lock().await.clone();
    assert_eq!(names, vec!["op-stderr-ok".to_string()]);
}
