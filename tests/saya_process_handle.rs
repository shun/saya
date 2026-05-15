//! `saya.process.spawn(...)` bootstrap ラッパの E2E 契約テスト。
//!
//! Phase A.3 のスコープに沿って、`Deno.core.ops.op_process_*` を直接
//! 叩くのではなく、`LIVE_RUNTIME_BOOTSTRAP` で公開された
//! `saya.process.spawn` 経由でハンドルを取得し、`child.stdin.write` /
//! `child.stdout.read` / `child.stderr.read` / `child.kill` / `child.wait`
//! を呼んで観測する。
//!
//! 本ファイルは「bootstrap ラッパ越しでしか観測できない契約」を狙う:
//! - `read` 戻り値の `0` を `null` に正規化する EOF 規約
//! - `Uint8Array` でない buffer を渡した場合の `TypeError`
//! - `Object.freeze` によるハンドル不変性
//! - 複数ハンドルの並列起動と非混線
//! - spec フィールド欠落時のフォールバック（未指定 stdio が `"null"`）
//!
//! op レイヤーの直叩き検証は `tests/saya_process_op.rs` で完了済み。
//! 本ファイルでは Rust 側 op 実装には触れず、TS 側の正規化責務のみを
//! 検証する。
//!
//! テスト戦略:
//! - `SayaLiveRuntime::spawn_from_seed` で seed runtime を起動する
//! - `bufferOpen` イベント handler 内で `saya.process.spawn(...)` を呼ぶ
//! - 検証成功時のみ `saya.commands.execute("...")` を呼んで
//!   `RecordingHostBridge` 側に痕跡を残し、Rust 側で確認する
//! - 検証失敗時は handler 内 `throw new Error(...)` で
//!   「期待値と観測値の両方を含むメッセージ」を投げ、`await_result`
//!   経由で panic させる
//! - 全テストに `tokio::time::timeout(Duration::from_secs(5), ...)` を
//!   巻き、子プロセスが応答しない場合のハングを防ぐ

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use saya::runtime::callback_registry_seed::CallbackRegistrySeed;
use saya::runtime::live::{
    BoxFuture, BufferEventPayload, HostCapabilityBridge, ReadonlyBufferSnapshot,
    ReadonlyEditorSnapshot, ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeEventPayload,
    RuntimeMode, SayaLiveRuntime,
};
use saya::runtime::startup::StartupRegistryEntry;
use tokio::sync::Mutex;

/// 検証用の `HostCapabilityBridge`。
///
/// TS handler が `saya.commands.execute("...")` を呼んだ際に、コマンド
/// 名を共有 Mutex に蓄積する。Phase A.3 のラッパ E2E テストでは、TS
/// 側で「成功パス」を踏んだ場合だけ特定コマンドを execute することで、
/// 外側 (Rust 側) から検証できるようにする。
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
            path: Some(PathBuf::from("saya-process-handle.test.md")),
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

/// T-H-1: `saya.process.spawn(...)` 越しの round-trip。
///
/// `cat -u` を `saya.process.spawn(...)` で起動し、`child.stdin.write`
/// が書き込みバイト数を返し、`child.stdout.read` で同一バイト列が読み
/// 戻り、`child.kill()` 後の `child.wait()` が非ゼロ exit code を返す
/// ことを確認する。bootstrap ラッパが `id` フィールドを 1 始まりの
/// 数値で公開していることも合わせて検証する。
#[tokio::test(flavor = "current_thread")]
async fn handle_round_trip_via_spawn_wrapper() {
    let bridge = Arc::new(RecordingHostBridge::new());
    let executed = bridge.shared_executed_commands();

    // deno_core の seed runtime には TextEncoder/TextDecoder が無いため、
    // ASCII の payload を charCodeAt 経由で `Uint8Array` に詰める。
    let handler = r#"
        async (_payload) => {
            const child = await saya.process.spawn({
                command: "cat",
                args: ["-u"],
                env: {},
                cwd: null,
                stdin: "piped",
                stdout: "piped",
                stderr: "piped",
            });
            if (typeof child.id !== "number" || child.id < 1) {
                throw new Error(
                    "expected child.id to be number >= 1, got " + child.id
                );
            }

            const payloadText = "saya-process-handle round trip\n";
            const payload = new Uint8Array(payloadText.length);
            for (let i = 0; i < payloadText.length; i += 1) {
                payload[i] = payloadText.charCodeAt(i) & 0xff;
            }

            const written = await child.stdin.write(payload);
            if (written !== payload.byteLength) {
                throw new Error(
                    "stdin.write returned " + written
                        + " expected " + payload.byteLength
                );
            }

            const buf = new Uint8Array(64);
            const n = await child.stdout.read(buf);
            if (n !== payload.byteLength) {
                throw new Error(
                    "stdout.read returned " + n
                        + " expected " + payload.byteLength
                );
            }
            for (let i = 0; i < payload.byteLength; i += 1) {
                if (buf[i] !== payload[i]) {
                    throw new Error(
                        "echo byte mismatch at " + i
                            + ": got " + buf[i]
                            + " expected " + payload[i]
                    );
                }
            }

            await child.kill();
            const code = await child.wait();
            if (code === 0) {
                throw new Error(
                    "expected non-zero exit code after kill, got 0"
                );
            }

            await saya.commands.execute("handle-roundtrip-ok");
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
    assert_eq!(names, vec!["handle-roundtrip-ok".to_string()]);
}

/// T-H-2: EOF が `null` に正規化される。
///
/// `true` プロセスは即座に exit し、stdout は何も書かずに閉じる。
/// op レイヤーは EOF を `0` で返すが、bootstrap ラッパは `0` を `null`
/// に upgrade する仕様。`null` であることと `0` ではないことの両方を
/// 確認することで、正規化責務が抜けると即座に検知できるようにする。
#[tokio::test(flavor = "current_thread")]
async fn handle_read_returns_null_at_eof() {
    let bridge = Arc::new(RecordingHostBridge::new());
    let executed = bridge.shared_executed_commands();

    let handler = r#"
        async (_payload) => {
            const child = await saya.process.spawn({
                command: "true",
                args: [],
                env: {},
                cwd: null,
                stdin: "null",
                stdout: "piped",
                stderr: "piped",
            });

            const buf = new Uint8Array(64);
            const n = await child.stdout.read(buf);
            if (n !== null) {
                throw new Error(
                    "expected EOF to be normalized to null, got "
                        + (typeof n) + " value " + n
                );
            }

            const code = await child.wait();
            if (code !== 0) {
                throw new Error("expected exit code 0, got " + code);
            }

            await saya.commands.execute("handle-eof-ok");
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
    assert_eq!(names, vec!["handle-eof-ok".to_string()]);
}

/// T-H-3: stderr が stdout と独立して動く（ラッパ越し）。
///
/// `sh -c "printf out; printf err 1>&2"` を起動し、`child.stdout.read`
/// で `"out"`、`child.stderr.read` で `"err"` を取得する。bootstrap
/// ラッパが stdout / stderr の `read` を別々のオブジェクトとして公開
/// しており、内部で正しく対応する op (`op_process_read_stdout` /
/// `op_process_read_stderr`) を呼んでいることを確認する。
#[tokio::test(flavor = "current_thread")]
async fn handle_stderr_independent_from_stdout() {
    let bridge = Arc::new(RecordingHostBridge::new());
    let executed = bridge.shared_executed_commands();

    let handler = r#"
        async (_payload) => {
            const child = await saya.process.spawn({
                command: "sh",
                args: ["-c", "printf out; printf err 1>&2"],
                env: {},
                cwd: null,
                stdin: "null",
                stdout: "piped",
                stderr: "piped",
            });

            const outBuf = new Uint8Array(16);
            const nOut = await child.stdout.read(outBuf);
            const errBuf = new Uint8Array(16);
            const nErr = await child.stderr.read(errBuf);

            // ASCII の "out" / "err" を charCode で逐次比較する（seed
            // runtime に TextDecoder が無いため）。
            const expectedOut = [111, 117, 116]; // "out"
            const expectedErr = [101, 114, 114]; // "err"
            if (nOut !== expectedOut.length) {
                throw new Error(
                    "stdout length mismatch: got " + nOut
                        + " expected " + expectedOut.length
                );
            }
            if (nErr !== expectedErr.length) {
                throw new Error(
                    "stderr length mismatch: got " + nErr
                        + " expected " + expectedErr.length
                );
            }
            for (let i = 0; i < expectedOut.length; i += 1) {
                if (outBuf[i] !== expectedOut[i]) {
                    throw new Error(
                        "stdout byte mismatch at " + i
                            + ": got " + outBuf[i]
                            + " expected " + expectedOut[i]
                    );
                }
            }
            for (let i = 0; i < expectedErr.length; i += 1) {
                if (errBuf[i] !== expectedErr[i]) {
                    throw new Error(
                        "stderr byte mismatch at " + i
                            + ": got " + errBuf[i]
                            + " expected " + expectedErr[i]
                    );
                }
            }

            const code = await child.wait();
            if (code !== 0) {
                throw new Error("expected exit code 0, got " + code);
            }

            await saya.commands.execute("handle-stderr-ok");
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
    assert_eq!(names, vec!["handle-stderr-ok".to_string()]);
}

/// T-H-4: `Uint8Array` 以外を `write` / `read` に渡すと `TypeError`。
///
/// bootstrap ラッパが `Uint8Array` 以外の引数を弾くバリデーションを
/// 行うことを、3 系統 (`stdin.write` / `stdout.read` / `stderr.read`)
/// で同時に検証する。エラーメッセージ中に "Uint8Array" を含むことも
/// チェックすることで、別系統の例外（op エラー等）に退化した場合に
/// 即座に検知できるようにする。
#[tokio::test(flavor = "current_thread")]
async fn handle_rejects_non_uint8array_with_type_error() {
    let bridge = Arc::new(RecordingHostBridge::new());
    let executed = bridge.shared_executed_commands();

    let handler = r#"
        async (_payload) => {
            const child = await saya.process.spawn({
                command: "cat",
                args: ["-u"],
                env: {},
                cwd: null,
                stdin: "piped",
                stdout: "piped",
                stderr: "piped",
            });

            async function expectTypeError(label, action) {
                let caught = null;
                try {
                    await action();
                } catch (err) {
                    caught = err;
                }
                if (caught === null) {
                    throw new Error(
                        label + ": expected TypeError, got no throw"
                    );
                }
                if (!(caught instanceof TypeError)) {
                    const ctorName =
                        (caught && caught.constructor && caught.constructor.name)
                            || (typeof caught);
                    throw new Error(
                        label + ": expected TypeError, got " + ctorName
                            + " (message=" + String(caught && caught.message) + ")"
                    );
                }
                if (!String(caught.message).includes("Uint8Array")) {
                    throw new Error(
                        label + ": expected message to mention Uint8Array, got "
                            + String(caught.message)
                    );
                }
            }

            await expectTypeError(
                "stdin.write(string)",
                () => child.stdin.write("not a buffer")
            );
            await expectTypeError(
                "stdout.read(string)",
                () => child.stdout.read("not a buffer")
            );
            await expectTypeError(
                "stderr.read(string)",
                () => child.stderr.read("not a buffer")
            );

            await child.kill();
            await child.wait();

            await saya.commands.execute("handle-typeerror-ok");
        }
    "#;

    let runtime = SayaLiveRuntime::spawn_from_seed(bridge.clone(), seed_with_handler(handler))
        .expect("seed runtime should initialize");

    let receipt = runtime
        .dispatch_event(buffer_open_payload())
        .expect("dispatch should succeed");
    let report = tokio::time::timeout(Duration::from_secs(5), receipt.await_result())
        .await
        .expect("type error checks should complete within 5s")
        .expect("dispatch result");
    assert_eq!(report.handler_count, 1);

    let names = executed.lock().await.clone();
    assert_eq!(names, vec!["handle-typeerror-ok".to_string()]);
}

/// T-H-5: `Object.freeze` でハンドルが不変化されている。
///
/// `Object.isFrozen(child)` および `Object.isFrozen(child.stdin)` 等で
/// 凍結を確認しつつ、実際にプロパティ書き換えを試みた後に reference
/// 等価で値が変わっていないことも検証する。strict / 非 strict の
/// 両モードで挙動が一貫するよう、書き換え試行は `try/catch` で吸収
/// する。
#[tokio::test(flavor = "current_thread")]
async fn handle_is_frozen_against_property_overrides() {
    let bridge = Arc::new(RecordingHostBridge::new());
    let executed = bridge.shared_executed_commands();

    let handler = r#"
        async (_payload) => {
            const child = await saya.process.spawn({
                command: "cat",
                args: ["-u"],
                env: {},
                cwd: null,
                stdin: "piped",
                stdout: "piped",
                stderr: "piped",
            });

            if (!Object.isFrozen(child)) {
                throw new Error("child handle should be frozen");
            }
            if (!Object.isFrozen(child.stdin)) {
                throw new Error("child.stdin should be frozen");
            }
            if (!Object.isFrozen(child.stdout)) {
                throw new Error("child.stdout should be frozen");
            }
            if (!Object.isFrozen(child.stderr)) {
                throw new Error("child.stderr should be frozen");
            }

            const originalStdin = child.stdin;
            const originalStdinWrite = child.stdin.write;
            const originalStdout = child.stdout;
            const originalKill = child.kill;

            // strict モードでは throw、非 strict では silent no-op に
            // なるため、try/catch で吸収して reference 等価で観測する。
            try { child.stdin = null; } catch (_e) {}
            if (child.stdin !== originalStdin) {
                throw new Error(
                    "child.stdin should not be reassignable, got new reference"
                );
            }

            try { child.stdout = null; } catch (_e) {}
            if (child.stdout !== originalStdout) {
                throw new Error(
                    "child.stdout should not be reassignable, got new reference"
                );
            }

            try { child.kill = () => {}; } catch (_e) {}
            if (child.kill !== originalKill) {
                throw new Error(
                    "child.kill should not be reassignable, got new reference"
                );
            }

            try { child.stdin.write = () => 0; } catch (_e) {}
            if (child.stdin.write !== originalStdinWrite) {
                throw new Error(
                    "child.stdin.write should not be reassignable, got new reference"
                );
            }

            await child.kill();
            await child.wait();

            await saya.commands.execute("handle-frozen-ok");
        }
    "#;

    let runtime = SayaLiveRuntime::spawn_from_seed(bridge.clone(), seed_with_handler(handler))
        .expect("seed runtime should initialize");

    let receipt = runtime
        .dispatch_event(buffer_open_payload())
        .expect("dispatch should succeed");
    let report = tokio::time::timeout(Duration::from_secs(5), receipt.await_result())
        .await
        .expect("freeze checks should complete within 5s")
        .expect("dispatch result");
    assert_eq!(report.handler_count, 1);

    let names = executed.lock().await.clone();
    assert_eq!(names, vec!["handle-frozen-ok".to_string()]);
}

/// T-H-6: 複数 handle を並列に spawn しても独立に動き、混線しない。
///
/// `cat -u` を 2 つ並べて、それぞれに違う payload を `Promise.all` で
/// 並列に書き込み、各々の stdout から読み戻したバイト列が自分の
/// payload と一致することで「ハンドル間で stdout が混線しない」
/// 不変条件を検証する。ハンドル ID が別物であることも合わせて確認。
#[tokio::test(flavor = "current_thread")]
async fn handles_run_in_parallel_without_cross_talk() {
    let bridge = Arc::new(RecordingHostBridge::new());
    let executed = bridge.shared_executed_commands();

    let handler = r#"
        async (_payload) => {
            const [a, b] = await Promise.all([
                saya.process.spawn({
                    command: "cat",
                    args: ["-u"],
                    env: {},
                    cwd: null,
                    stdin: "piped",
                    stdout: "piped",
                    stderr: "piped",
                }),
                saya.process.spawn({
                    command: "cat",
                    args: ["-u"],
                    env: {},
                    cwd: null,
                    stdin: "piped",
                    stdout: "piped",
                    stderr: "piped",
                }),
            ]);

            if (a.id === b.id) {
                throw new Error(
                    "expected distinct handle ids, got " + a.id + " and " + b.id
                );
            }

            function encodeAscii(text) {
                const buf = new Uint8Array(text.length);
                for (let i = 0; i < text.length; i += 1) {
                    buf[i] = text.charCodeAt(i) & 0xff;
                }
                return buf;
            }

            const aPayload = encodeAscii("alpha-payload-line\n");
            const bPayload = encodeAscii("beta-payload-line-XX\n");

            const [wA, wB] = await Promise.all([
                a.stdin.write(aPayload),
                b.stdin.write(bPayload),
            ]);
            if (wA !== aPayload.byteLength) {
                throw new Error(
                    "a.stdin.write returned " + wA
                        + " expected " + aPayload.byteLength
                );
            }
            if (wB !== bPayload.byteLength) {
                throw new Error(
                    "b.stdin.write returned " + wB
                        + " expected " + bPayload.byteLength
                );
            }

            const aBuf = new Uint8Array(64);
            const bBuf = new Uint8Array(64);
            const nA = await a.stdout.read(aBuf);
            const nB = await b.stdout.read(bBuf);

            if (nA !== aPayload.byteLength) {
                throw new Error(
                    "a.stdout.read returned " + nA
                        + " expected " + aPayload.byteLength
                );
            }
            if (nB !== bPayload.byteLength) {
                throw new Error(
                    "b.stdout.read returned " + nB
                        + " expected " + bPayload.byteLength
                );
            }
            for (let i = 0; i < aPayload.byteLength; i += 1) {
                if (aBuf[i] !== aPayload[i]) {
                    throw new Error(
                        "a stdout cross-talk detected at " + i
                            + ": got " + aBuf[i] + " expected " + aPayload[i]
                    );
                }
            }
            for (let i = 0; i < bPayload.byteLength; i += 1) {
                if (bBuf[i] !== bPayload[i]) {
                    throw new Error(
                        "b stdout cross-talk detected at " + i
                            + ": got " + bBuf[i] + " expected " + bPayload[i]
                    );
                }
            }

            await Promise.all([a.kill(), b.kill()]);
            await Promise.all([a.wait(), b.wait()]);

            await saya.commands.execute("handle-parallel-ok");
        }
    "#;

    let runtime = SayaLiveRuntime::spawn_from_seed(bridge.clone(), seed_with_handler(handler))
        .expect("seed runtime should initialize");

    let receipt = runtime
        .dispatch_event(buffer_open_payload())
        .expect("dispatch should succeed");
    let report = tokio::time::timeout(Duration::from_secs(5), receipt.await_result())
        .await
        .expect("parallel handle check should complete within 5s")
        .expect("dispatch result");
    assert_eq!(report.handler_count, 1);

    let names = executed.lock().await.clone();
    assert_eq!(names, vec!["handle-parallel-ok".to_string()]);
}

/// T-H-7: spec フィールド欠落時のフォールバック。
///
/// `await saya.process.spawn({ command: "true" })` だけで動くこと、
/// および戻ってきたハンドルが期待する形状（`id` / `stdin` / `stdout`
/// / `stderr` / `kill` / `wait`）を備えていることを検証する。
/// `args` / `env` / `cwd` / `stdin` / `stdout` / `stderr` の欠落で
/// 例外にならず、`wait()` が `0` を返すことで「stdio 欠落時に `null`
/// (= `/dev/null`) にフォールバックする」契約が守られていることを
/// 確認する。
#[tokio::test(flavor = "current_thread")]
async fn spawn_falls_back_when_optional_fields_omitted() {
    let bridge = Arc::new(RecordingHostBridge::new());
    let executed = bridge.shared_executed_commands();

    let handler = r#"
        async (_payload) => {
            const child = await saya.process.spawn({ command: "true" });

            if (typeof child.id !== "number" || child.id < 1) {
                throw new Error(
                    "expected child.id to be number >= 1, got " + child.id
                );
            }
            if (!child.stdin || typeof child.stdin.write !== "function") {
                throw new Error(
                    "expected child.stdin.write to be function, got "
                        + typeof (child.stdin && child.stdin.write)
                );
            }
            if (!child.stdout || typeof child.stdout.read !== "function") {
                throw new Error(
                    "expected child.stdout.read to be function, got "
                        + typeof (child.stdout && child.stdout.read)
                );
            }
            if (!child.stderr || typeof child.stderr.read !== "function") {
                throw new Error(
                    "expected child.stderr.read to be function, got "
                        + typeof (child.stderr && child.stderr.read)
                );
            }
            if (typeof child.kill !== "function") {
                throw new Error(
                    "expected child.kill to be function, got " + typeof child.kill
                );
            }
            if (typeof child.wait !== "function") {
                throw new Error(
                    "expected child.wait to be function, got " + typeof child.wait
                );
            }

            const code = await child.wait();
            if (code !== 0) {
                throw new Error("expected exit code 0, got " + code);
            }

            await saya.commands.execute("handle-fallback-ok");
        }
    "#;

    let runtime = SayaLiveRuntime::spawn_from_seed(bridge.clone(), seed_with_handler(handler))
        .expect("seed runtime should initialize");

    let receipt = runtime
        .dispatch_event(buffer_open_payload())
        .expect("dispatch should succeed");
    let report = tokio::time::timeout(Duration::from_secs(5), receipt.await_result())
        .await
        .expect("fallback spawn should complete within 5s")
        .expect("dispatch result");
    assert_eq!(report.handler_count, 1);

    let names = executed.lock().await.clone();
    assert_eq!(names, vec!["handle-fallback-ok".to_string()]);
}
