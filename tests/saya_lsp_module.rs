//! Phase B: `plugins/saya-lsp/` 配下の名前空間モジュール群の単体テスト。
//!
//! 各 namespace（`__lspUtf8` / `__lspJsonRpc` / `__lspTransport` /
//! `__lspLifecycle` / `__lspSession` 等）の振る舞いを、seed runtime を介さず
//! `evaluate_startup_module` の top-level コード経由で検証する。
//!
//! 検証パターン:
//! - tmp に init.ts を書き、`import {} from "plugins/saya-lsp-client.ts";`
//!   でモジュール本体を inline 展開する
//! - top-level コードで namespace の振る舞いを検査し、不一致なら
//!   `throw new Error(...)` する
//! - `evaluate_startup_module(...).await` が `Ok(())` を返したら GREEN、
//!   `Err(message)` を返したら RED（panic 経由でテスト失敗）
//!
//! `tokio::time::timeout` で評価をラップして、deno_core の評価が
//! hang しないように防御する。

use std::path::PathBuf;
use std::time::Duration;

use saya::startup_runtime::{
    StartupModulePrepareResult, evaluate_startup_module, prepare_init_module,
};

fn unique_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-lsp-module-{name}-{nanos}"))
}

fn plugin_specifier() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("plugins/saya-lsp-client.ts")
        .to_string_lossy()
        .into_owned()
}

/// 指定された top-level JavaScript を、`plugins/saya-lsp-client.ts` を
/// inline 展開した状態で評価するヘルパ。
///
/// `top_level_source` は `__lspUtf8` 等の namespace を直接参照する
/// JavaScript コード（seed runtime に TextEncoder/TextDecoder が無い
/// 前提で書くこと）。
async fn evaluate_with_plugin(top_level_source: &str, label: &str) -> Result<(), String> {
    let config_path = unique_path(label);
    let source = format!(
        "import {{}} from {specifier:?};\n{body}\n",
        specifier = plugin_specifier(),
        body = top_level_source,
    );
    std::fs::write(&config_path, source).expect("test config write");
    let prepared = prepare_init_module(
        &config_path,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).as_path(),
    );
    let StartupModulePrepareResult::Success(module) = prepared else {
        return Err(format!("prepare_init_module failed: {prepared:?}"));
    };
    tokio::time::timeout(
        Duration::from_secs(5),
        evaluate_startup_module(&module.executable_source_text),
    )
    .await
    .map_err(|_| format!("evaluate_startup_module timed out for {label}"))?
}

/// T-UTF8-1: `__lspUtf8.encodeBytes("hello")` が ASCII 5 バイトを
/// 返すこと。
///
/// LSP の JSON-RPC body は UTF-8 でエンコードされる必要がある。
/// 最も単純な ASCII バイト列の場合に encodeBytes が
/// `Uint8Array([104, 101, 108, 108, 111])` を返せることが、
/// 後続の全テストの基盤になる。
#[tokio::test(flavor = "current_thread")]
async fn utf8_encode_bytes_returns_uint8array_for_ascii() {
    let source = r#"
        const bytes = __lspUtf8.encodeBytes("hello");
        if (!(bytes instanceof Uint8Array)) {
            throw new Error(
                "encodeBytes must return Uint8Array, got " + (bytes && bytes.constructor && bytes.constructor.name)
            );
        }
        if (bytes.length !== 5) {
            throw new Error("expected 5 bytes for 'hello', got " + bytes.length);
        }
        const expected = [104, 101, 108, 108, 111];
        for (let i = 0; i < expected.length; i = i + 1) {
            if (bytes[i] !== expected[i]) {
                throw new Error(
                    "byte mismatch at " + i + ": got " + bytes[i] + " expected " + expected[i]
                );
            }
        }
    "#;
    evaluate_with_plugin(source, "utf8-ascii")
        .await
        .expect("__lspUtf8.encodeBytes should encode ASCII bytes");
}

/// T-UTF8-2: 2 バイト UTF-8（U+0080〜U+07FF）の encode。
///
/// LSP のレスポンスには Latin-1 補助領域や Cyrillic / Greek 等の
/// 2 バイト範囲が頻出する。`é` (U+00E9) → `[0xc3, 0xa9]`。
#[tokio::test(flavor = "current_thread")]
async fn utf8_encode_bytes_handles_two_byte_codepoint() {
    let source = r#"
        const bytes = __lspUtf8.encodeBytes("é");
        if (bytes.length !== 2) {
            throw new Error("expected 2 bytes for 'é', got " + bytes.length);
        }
        if (bytes[0] !== 0xc3 || bytes[1] !== 0xa9) {
            throw new Error(
                "expected [0xc3, 0xa9], got [" + bytes[0] + ", " + bytes[1] + "]"
            );
        }
    "#;
    evaluate_with_plugin(source, "utf8-two-byte")
        .await
        .expect("__lspUtf8.encodeBytes should encode 2-byte codepoints");
}

/// T-UTF8-3: 3 バイト UTF-8（U+0800〜U+FFFF）の encode。
///
/// 日本語など CJK 文字を含む LSP メッセージで頻出。`あ` (U+3042) →
/// `[0xe3, 0x81, 0x82]`。
#[tokio::test(flavor = "current_thread")]
async fn utf8_encode_bytes_handles_three_byte_codepoint() {
    let source = r#"
        const bytes = __lspUtf8.encodeBytes("あ");
        if (bytes.length !== 3) {
            throw new Error("expected 3 bytes for 'あ', got " + bytes.length);
        }
        const expected = [0xe3, 0x81, 0x82];
        for (let i = 0; i < expected.length; i = i + 1) {
            if (bytes[i] !== expected[i]) {
                throw new Error(
                    "byte mismatch at " + i + ": got " + bytes[i] + " expected " + expected[i]
                );
            }
        }
    "#;
    evaluate_with_plugin(source, "utf8-three-byte")
        .await
        .expect("__lspUtf8.encodeBytes should encode 3-byte codepoints");
}

/// T-UTF8-4: 4 バイト UTF-8（U+10000〜U+10FFFF）の encode。
///
/// 絵文字や追加 CJK 統合漢字。`😀` (U+1F600) →
/// `[0xf0, 0x9f, 0x98, 0x80]`。サロゲートペア (UTF-16) を 1 コード
/// ポイントとして扱えていることも検証する。
#[tokio::test(flavor = "current_thread")]
async fn utf8_encode_bytes_handles_four_byte_codepoint() {
    let source = r#"
        const bytes = __lspUtf8.encodeBytes("😀");
        if (bytes.length !== 4) {
            throw new Error("expected 4 bytes for '😀', got " + bytes.length);
        }
        const expected = [0xf0, 0x9f, 0x98, 0x80];
        for (let i = 0; i < expected.length; i = i + 1) {
            if (bytes[i] !== expected[i]) {
                throw new Error(
                    "byte mismatch at " + i + ": got " + bytes[i] + " expected " + expected[i]
                );
            }
        }
    "#;
    evaluate_with_plugin(source, "utf8-four-byte")
        .await
        .expect("__lspUtf8.encodeBytes should encode 4-byte codepoints");
}

/// T-UTF8-5: encode → decode の round-trip。
///
/// 混在文字列（ASCII / Latin-1 補助 / BMP / Astral）の双方向変換が
/// 文字列同一性を保つこと。JSON-RPC body の受信経路で必要。
#[tokio::test(flavor = "current_thread")]
async fn utf8_encode_decode_round_trip_preserves_mixed_codepoints() {
    let source = r#"
        const original = "hi é あ 😀 ok";
        const bytes = __lspUtf8.encodeBytes(original);
        const decoded = __lspUtf8.decodeBytes(bytes);
        if (decoded !== original) {
            throw new Error("round-trip mismatch: got " + JSON.stringify(decoded) + " expected " + JSON.stringify(original));
        }
    "#;
    evaluate_with_plugin(source, "utf8-round-trip")
        .await
        .expect("__lspUtf8 encode/decode should round-trip mixed codepoints");
}

/// T-UTF8-6: `byteLength` が encode 結果と一致する。
///
/// pre-alloc / Content-Length 計算で正確性が崩れると LSP メッセージ
/// が破損する。混在文字列で常に encodeBytes(...).length と一致する
/// ことを保証する。
#[tokio::test(flavor = "current_thread")]
async fn utf8_byte_length_matches_encode_output() {
    let source = r#"
        const samples = ["", "a", "é", "あ", "😀", "mixed: aé漢字😀"];
        for (let i = 0; i < samples.length; i = i + 1) {
            const text = samples[i];
            const expected = __lspUtf8.encodeBytes(text).length;
            const observed = __lspUtf8.byteLength(text);
            if (expected !== observed) {
                throw new Error(
                    "byteLength mismatch for " + JSON.stringify(text)
                        + ": got " + observed + " expected " + expected
                );
            }
        }
    "#;
    evaluate_with_plugin(source, "utf8-byte-length")
        .await
        .expect("__lspUtf8.byteLength should match encode output length");
}

/// T-UTF8-7: `decodeBytes` が `Uint8Array` 以外を `TypeError` で
/// 拒絶する。
///
/// 呼び出し側の誤用を静かに通すと、UTF-8 codec の不変条件が崩れて
/// 後続 framing で原因不明の bug につながる。境界での fail-fast を
/// 強制する。
#[tokio::test(flavor = "current_thread")]
async fn utf8_decode_rejects_non_uint8array_with_type_error() {
    let source = r#"
        let caught = null;
        try {
            __lspUtf8.decodeBytes("not a buffer");
        } catch (err) {
            caught = err;
        }
        if (!(caught instanceof TypeError)) {
            const name = (caught && caught.constructor && caught.constructor.name) || typeof caught;
            throw new Error("expected TypeError, got " + name);
        }
    "#;
    evaluate_with_plugin(source, "utf8-decode-typeerror")
        .await
        .expect("__lspUtf8.decodeBytes should reject non-Uint8Array with TypeError");
}

/// T-PLATFORM-1: startup runtime 上で利用可能な web プラットフォーム
/// API を網羅する。LSP クライアントの cancel / timeout 経路の設計判断に
/// 直結する。
///
/// 観測:
/// - `AbortController` / `AbortSignal` は **利用不可** (`deno_web` 不在)
///   → 自前 signal-like オブジェクトを `__lspJsonRpc` 名前空間で実装する
/// - `setTimeout` / `clearTimeout` / `Promise` 等の有無を観測値として
///   `globalThis.__sayaLspPlatformObservation` に残し、後続の設計が
///   依存できるようにする
///
/// このテストはランタイム互換性の現状を表明することが目的で、負の
/// 表明として書く。
#[tokio::test(flavor = "current_thread")]
async fn platform_records_available_web_apis_for_lsp_design() {
    // 注: `strip_type_annotations` が `key: callExpr(...)` を型注釈と
    // 誤判定して値を削ってしまうため、object literal で初期化せず
    // `Map` 風に逐次代入する。
    let source = r#"
        function probeGlobal(name) {
            try {
                const result = new Function("return typeof " + name)();
                return result;
            } catch (err) {
                return "throws:" + ((err && err.message) ? err.message : String(err));
            }
        }
        const observed = {};
        observed.AbortController = probeGlobal("AbortController");
        observed.AbortSignal = probeGlobal("AbortSignal");
        observed.setTimeout = probeGlobal("setTimeout");
        observed.clearTimeout = probeGlobal("clearTimeout");
        observed.Promise = probeGlobal("Promise");
        observed.queueMicrotask = probeGlobal("queueMicrotask");
        if (observed.AbortController !== "undefined") {
            throw new Error("AbortController is unexpectedly available (re-evaluate cancel design): " + observed.AbortController);
        }
        if (observed.Promise !== "function") {
            throw new Error("Promise must be available, got " + observed.Promise);
        }
        globalThis.__sayaLspPlatformObservation = observed;
    "#;
    evaluate_with_plugin(source, "platform-records")
        .await
        .expect("startup runtime should expose Promise; AbortController must be absent");
}

/// T-ASM-1: 名前空間オブジェクトを「メソッドの `toString()` を連結した
/// オブジェクトリテラル文字列」として再構築し、`new Function(...)()` で
/// 評価した結果が元の名前空間と同等に振る舞うことを検証する。
///
/// 背景:
/// - 名前空間定義は startup runtime にしか存在しない（`Function.toString()`
///   は callback の source として LIVE runtime に渡される唯一の経路）
/// - そのため `__lspXxxSource = namespaceToSource(__lspXxx)` の形で
///   各 namespace を JS ソース文字列に変換し、callback の body に
///   inline する必要がある
///
/// このテストは、`Function.prototype.toString()` がメソッド本体を
/// 再評価可能な形で返すこと、および再構築後のオブジェクトが
/// 同じ入出力を返すことを保証する。失敗した場合、設計を `eval` 経路
/// から別の機構（例: 二重定義 + 文字列定数）に切り替える必要がある。
#[tokio::test(flavor = "current_thread")]
async fn assembly_namespace_methods_round_trip_via_function_to_string() {
    let source = r#"
        // namespaceToSource は index.ts で定義済みのものを再利用する
        // (plugin が import 展開された時点で同じスコープに存在する)。
        const originalSource = namespaceToSource("__lspUtf8", __lspUtf8);
        // 再構築: source を eval して新しい __lspUtf8_re を得る
        const rebuilt = new Function(originalSource + "\nreturn __lspUtf8;")();
        if (typeof rebuilt.encodeBytes !== "function") {
            throw new Error("rebuilt namespace must expose encodeBytes function");
        }
        const probe = "hi é 漢 😀";
        const originalBytes = __lspUtf8.encodeBytes(probe);
        const rebuiltBytes = rebuilt.encodeBytes(probe);
        if (originalBytes.length !== rebuiltBytes.length) {
            throw new Error(
                "byte length mismatch: original=" + originalBytes.length
                    + " rebuilt=" + rebuiltBytes.length
            );
        }
        for (let i = 0; i < originalBytes.length; i = i + 1) {
            if (originalBytes[i] !== rebuiltBytes[i]) {
                throw new Error(
                    "byte mismatch at " + i + ": original=" + originalBytes[i]
                        + " rebuilt=" + rebuiltBytes[i]
                );
            }
        }
        // round trip back via rebuilt.decodeBytes
        const decoded = rebuilt.decodeBytes(rebuiltBytes);
        if (decoded !== probe) {
            throw new Error(
                "rebuilt decode round-trip mismatch: got " + JSON.stringify(decoded)
                    + " expected " + JSON.stringify(probe)
            );
        }
        globalThis.__sayaLspAssemblyValidated = true;
    "#;
    evaluate_with_plugin(source, "assembly-namespace")
        .await
        .expect("namespace assembly via Function.toString must round-trip");
}

// =========================================================================
// json-rpc.ts: `__lspJsonRpc` 名前空間のテスト
// =========================================================================

/// T-JSONRPC-1: `encodeMessageBytes` が `Content-Length` ヘッダ + 空行
/// + body の正しい framing を返す。
///
/// LSP の wire format は `Content-Length: <bytes>\r\n\r\n<JSON body>` で
/// 固定。framing が崩れるとサーバが parse error を返すので、最低限
/// この構造の検査を最初の RED に置く。
#[tokio::test(flavor = "current_thread")]
async fn jsonrpc_encode_message_bytes_produces_lsp_framing() {
    let source = r#"
        const message = { jsonrpc: "2.0", method: "initialized" };
        const bytes = __lspJsonRpc.encodeMessageBytes(message);
        if (!(bytes instanceof Uint8Array)) {
            throw new Error("encodeMessageBytes must return Uint8Array");
        }
        const decoded = __lspUtf8.decodeBytes(bytes);
        const expectedBody = JSON.stringify(message);
        const expectedHeader = "Content-Length: " + __lspUtf8.byteLength(expectedBody) + "\r\n\r\n";
        const expected = expectedHeader + expectedBody;
        if (decoded !== expected) {
            throw new Error(
                "framing mismatch: got " + JSON.stringify(decoded)
                    + " expected " + JSON.stringify(expected)
            );
        }
    "#;
    evaluate_with_plugin(source, "jsonrpc-encode")
        .await
        .expect("__lspJsonRpc.encodeMessageBytes should produce LSP framing");
}

/// T-JSONRPC-2: `createBytesParser` が単一メッセージを decode する。
///
/// encode → parse の round-trip は JSON-RPC 通信の根幹なので、最も単純な
/// notification で先に検証する。parser は `onMessage(parsedJson)` を
/// callback で呼ぶ。
#[tokio::test(flavor = "current_thread")]
async fn jsonrpc_create_bytes_parser_round_trips_single_message() {
    let source = r#"
        const original = { jsonrpc: "2.0", method: "initialized", params: {} };
        const encoded = __lspJsonRpc.encodeMessageBytes(original);
        const received = [];
        const parser = __lspJsonRpc.createBytesParser(function(message) {
            received.push(message);
        });
        parser.accept(encoded);
        if (received.length !== 1) {
            throw new Error("expected 1 message, got " + received.length);
        }
        const got = received[0];
        if (got.jsonrpc !== "2.0" || got.method !== "initialized") {
            throw new Error("parsed message mismatch: " + JSON.stringify(got));
        }
    "#;
    evaluate_with_plugin(source, "jsonrpc-parser-single")
        .await
        .expect("__lspJsonRpc.createBytesParser should round-trip a single message");
}

/// T-JSONRPC-3: parser がチャンク境界をまたぐメッセージを正しく
/// 復元できる。
///
/// 実プロセス stdout からの読み出しは任意のバイト境界で chunk を返す。
/// header の途中で切れた場合 / body の途中で切れた場合の双方で
/// `accept` が正しく continuation できることを検証する。
#[tokio::test(flavor = "current_thread")]
async fn jsonrpc_parser_handles_chunk_boundaries_mid_header_and_mid_body() {
    let source = r#"
        const original = { jsonrpc: "2.0", method: "textDocument/publishDiagnostics", params: { uri: "file:///x" } };
        const encoded = __lspJsonRpc.encodeMessageBytes(original);
        // 3 分割: header 途中 / header と body の境界またぎ / body 末尾
        const splits = [3, 18, encoded.length];
        const received = [];
        const parser = __lspJsonRpc.createBytesParser(function(message) {
            received.push(message);
        });
        let cursor = 0;
        for (let i = 0; i < splits.length; i = i + 1) {
            const end = splits[i];
            parser.accept(encoded.slice(cursor, end));
            cursor = end;
        }
        if (received.length !== 1) {
            throw new Error("expected 1 message after streaming, got " + received.length);
        }
        if (received[0].method !== "textDocument/publishDiagnostics") {
            throw new Error("parsed method mismatch: " + JSON.stringify(received[0]));
        }
    "#;
    evaluate_with_plugin(source, "jsonrpc-parser-chunks")
        .await
        .expect("parser must handle chunk boundaries");
}

/// T-JSONRPC-4: parser が同一 chunk に複数 message を含む場合に
/// 全部 emit する。
///
/// バックプレッシャ下でサーバが複数 message を連続送出すると、
/// 1 回の `read` で複数 message が届くケースがある。すべてが
/// `onMessage` callback で順序通り通知されることを検証する。
#[tokio::test(flavor = "current_thread")]
async fn jsonrpc_parser_emits_all_messages_in_a_single_chunk() {
    let source = r#"
        const m1 = { jsonrpc: "2.0", id: 1, result: "first" };
        const m2 = { jsonrpc: "2.0", method: "notify", params: {} };
        const m3 = { jsonrpc: "2.0", id: 2, result: "third" };
        const e1 = __lspJsonRpc.encodeMessageBytes(m1);
        const e2 = __lspJsonRpc.encodeMessageBytes(m2);
        const e3 = __lspJsonRpc.encodeMessageBytes(m3);
        const concat = new Uint8Array(e1.length + e2.length + e3.length);
        concat.set(e1, 0);
        concat.set(e2, e1.length);
        concat.set(e3, e1.length + e2.length);
        const received = [];
        const parser = __lspJsonRpc.createBytesParser(function(message) {
            received.push(message);
        });
        parser.accept(concat);
        if (received.length !== 3) {
            throw new Error("expected 3 messages, got " + received.length);
        }
        if (received[0].id !== 1 || received[1].method !== "notify" || received[2].id !== 2) {
            throw new Error("parsed order mismatch: " + JSON.stringify(received));
        }
    "#;
    evaluate_with_plugin(source, "jsonrpc-parser-multi")
        .await
        .expect("parser must emit all messages in a single chunk");
}

/// T-JSONRPC-5: クライアントが request を送り、対応する response で
/// resolve すること（fake transport 経由）。
///
/// LSP の最も基本的な動作。outbound `id=1` の request が transport へ
/// 書き込まれ、fake server がそれに対応する `id=1` の response を
/// 配信したら request promise が `result` で resolve することを確認。
#[tokio::test(flavor = "current_thread")]
async fn jsonrpc_client_request_resolves_with_matching_response() {
    let source = r#"
        function createFakeTransport() {
            const sent = [];
            let bytesHandler = null;
            let closeHandler = null;
            const t = {};
            t.writeBytes = function (uint8) {
                sent.push(uint8);
                return Promise.resolve();
            };
            t.onBytes = function (handler) { bytesHandler = handler; };
            t.onClose = function (handler) { closeHandler = handler; };
            t.__sent = function () { return sent; };
            t.__deliverBytes = function (uint8) { if (bytesHandler) bytesHandler(uint8); };
            t.__simulateClose = function (reason) { if (closeHandler) closeHandler(reason); };
            return t;
        }
        const transport = createFakeTransport();
        const client = __lspJsonRpc.createClient(transport);
        const pending = client.request("textDocument/hover", { uri: "file:///a" });
        // fake server: id=1 の response を返す
        const response = { jsonrpc: "2.0", id: 1, result: { contents: "hovered" } };
        transport.__deliverBytes(__lspJsonRpc.encodeMessageBytes(response));
        const got = await pending;
        if (!got || got.contents !== "hovered") {
            throw new Error("expected resolved result.contents='hovered', got " + JSON.stringify(got));
        }
        // 送信側 wire format も検証する
        const sent = transport.__sent();
        if (sent.length !== 1) {
            throw new Error("expected 1 outbound message, got " + sent.length);
        }
        const sentText = __lspUtf8.decodeBytes(sent[0]);
        if (sentText.indexOf("\"method\":\"textDocument/hover\"") < 0 || sentText.indexOf("\"id\":1") < 0) {
            throw new Error("outbound request shape mismatch: " + sentText);
        }
    "#;
    evaluate_with_plugin(source, "jsonrpc-client-request")
        .await
        .expect("client request should resolve with matching response");
}

/// T-JSONRPC-6: クライアントの `notify` は id を持たず response を
/// 待たない。
///
/// LSP notification は fire-and-forget。`await notify(...)` が即座に
/// 解決し、transport には `id` フィールドが無い message が書き込まれる
/// ことを検証する。
#[tokio::test(flavor = "current_thread")]
async fn jsonrpc_client_notify_writes_message_without_id() {
    let source = r#"
        function createFakeTransport() {
            const sent = [];
            const t = {};
            t.writeBytes = function (uint8) {
                sent.push(uint8);
                return Promise.resolve();
            };
            t.onBytes = function () {};
            t.onClose = function () {};
            t.__sent = function () { return sent; };
            return t;
        }
        const transport = createFakeTransport();
        const client = __lspJsonRpc.createClient(transport);
        await client.notify("textDocument/didOpen", { textDocument: { uri: "file:///x" } });
        const sent = transport.__sent();
        if (sent.length !== 1) {
            throw new Error("expected 1 outbound notification, got " + sent.length);
        }
        const text = __lspUtf8.decodeBytes(sent[0]);
        if (text.indexOf("\"method\":\"textDocument/didOpen\"") < 0) {
            throw new Error("outbound notification method missing: " + text);
        }
        if (text.indexOf("\"id\":") >= 0) {
            throw new Error("notification must not include an id field: " + text);
        }
    "#;
    evaluate_with_plugin(source, "jsonrpc-client-notify")
        .await
        .expect("client notify should write a message without an id");
}

/// T-JSONRPC-7: 着信した notification は `onNotification` callback に
/// 配送される。
///
/// publishDiagnostics / showMessage / logMessage 等のサーバ → クライアント
/// 通知が登録した handler に届くことを検証。`id` を持たない着信
/// message はすべて handler に渡る。
#[tokio::test(flavor = "current_thread")]
async fn jsonrpc_client_routes_inbound_notifications_to_handler() {
    let source = r#"
        function createFakeTransport() {
            let bytesHandler = null;
            const t = {};
            t.writeBytes = function () { return Promise.resolve(); };
            t.onBytes = function (handler) { bytesHandler = handler; };
            t.onClose = function () {};
            t.__deliverBytes = function (uint8) { if (bytesHandler) bytesHandler(uint8); };
            return t;
        }
        const observedNotifications = [];
        const transport = createFakeTransport();
        // transpiler が `key: function` を型注釈と誤判定するため
        // option は property assignment で組み立てる。
        const clientOptions = {};
        clientOptions.onNotification = function (message) {
            observedNotifications.push(message);
        };
        const client = __lspJsonRpc.createClient(transport, clientOptions);
        const notification = {
            jsonrpc: "2.0",
            method: "textDocument/publishDiagnostics",
            params: { uri: "file:///x", diagnostics: [] },
        };
        transport.__deliverBytes(__lspJsonRpc.encodeMessageBytes(notification));
        if (observedNotifications.length !== 1) {
            throw new Error("expected 1 notification, got " + observedNotifications.length);
        }
        if (observedNotifications[0].method !== "textDocument/publishDiagnostics") {
            throw new Error("unexpected notification: " + JSON.stringify(observedNotifications[0]));
        }
    "#;
    evaluate_with_plugin(source, "jsonrpc-client-inbound-notify")
        .await
        .expect("client should route inbound notifications to handler");
}

/// T-JSONRPC-8: error response が来たら request は `error` オブジェクトで
/// reject する。
///
/// LSP サーバが `id=N` に対して `error: { code, message, data? }` を
/// 返した場合、対応する pending request は reject される。
#[tokio::test(flavor = "current_thread")]
async fn jsonrpc_client_request_rejects_on_error_response() {
    let source = r#"
        function createFakeTransport() {
            let bytesHandler = null;
            const t = {};
            t.writeBytes = function () { return Promise.resolve(); };
            t.onBytes = function (handler) { bytesHandler = handler; };
            t.onClose = function () {};
            t.__deliverBytes = function (uint8) { if (bytesHandler) bytesHandler(uint8); };
            return t;
        }
        const transport = createFakeTransport();
        const client = __lspJsonRpc.createClient(transport);
        const pending = client.request("textDocument/hover", null);
        transport.__deliverBytes(__lspJsonRpc.encodeMessageBytes({
            jsonrpc: "2.0",
            id: 1,
            error: { code: -32601, message: "Method not found" },
        }));
        let caught = null;
        try {
            await pending;
        } catch (err) {
            caught = err;
        }
        if (!caught || caught.code !== -32601 || caught.message !== "Method not found") {
            throw new Error("expected error rejection, got " + JSON.stringify(caught));
        }
    "#;
    evaluate_with_plugin(source, "jsonrpc-client-error")
        .await
        .expect("client should reject request with server error object");
}

/// T-JSONRPC-9: 自前 cancel token 経由で request を cancel すると
/// `$/cancelRequest` 通知が送出され、request promise は reject する。
///
/// AbortController は startup runtime にも live runtime にも無いため、
/// `__lspJsonRpc.createCancelToken()` で自前の signal-like を提供する。
/// LSP の慣例通り、cancel は `$/cancelRequest` 通知をサーバへ送る。
#[tokio::test(flavor = "current_thread")]
async fn jsonrpc_client_cancel_token_emits_cancel_request_notification() {
    let source = r#"
        function createFakeTransport() {
            const sent = [];
            let bytesHandler = null;
            const t = {};
            t.writeBytes = function (uint8) { sent.push(uint8); return Promise.resolve(); };
            t.onBytes = function (handler) { bytesHandler = handler; };
            t.onClose = function () {};
            t.__sent = function () { return sent; };
            t.__deliverBytes = function (uint8) { if (bytesHandler) bytesHandler(uint8); };
            return t;
        }
        const transport = createFakeTransport();
        const client = __lspJsonRpc.createClient(transport);
        const token = __lspJsonRpc.createCancelToken();
        // `{ signal: token.signal }` リテラルは `strip_type_annotations`
        // に消されるため property assignment で組み立てる。
        const requestOptions = {};
        requestOptions.signal = token.signal;
        const pending = client.request("textDocument/hover", null, requestOptions);
        token.cancel(new Error("user cancelled"));
        let caught = null;
        try {
            await pending;
        } catch (err) {
            caught = err;
        }
        if (!caught) {
            throw new Error("request must reject after cancel");
        }
        const sent = transport.__sent();
        if (sent.length < 2) {
            throw new Error("expected request + $/cancelRequest, got " + sent.length);
        }
        const cancelText = __lspUtf8.decodeBytes(sent[1]);
        if (cancelText.indexOf("\"method\":\"$/cancelRequest\"") < 0) {
            throw new Error("cancel notification not emitted: " + cancelText);
        }
        if (cancelText.indexOf("\"id\":1") < 0) {
            throw new Error("cancel notification must reference cancelled id: " + cancelText);
        }
    "#;
    evaluate_with_plugin(source, "jsonrpc-client-cancel")
        .await
        .expect("cancel token should reject request and emit $/cancelRequest");
}

/// T-JSONRPC-10: transport が close すると全 pending が reject する。
///
/// LSP サーバプロセス crash や明示 shutdown 後、in-flight な request は
/// reject されるべき。エディタ全体が hang しないための重要な保証。
#[tokio::test(flavor = "current_thread")]
async fn jsonrpc_client_rejects_all_pending_on_transport_close() {
    let source = r#"
        function createFakeTransport() {
            let closeHandler = null;
            const t = {};
            t.writeBytes = function () { return Promise.resolve(); };
            t.onBytes = function () {};
            t.onClose = function (handler) { closeHandler = handler; };
            t.__simulateClose = function (reason) { if (closeHandler) closeHandler(reason); };
            return t;
        }
        const transport = createFakeTransport();
        const client = __lspJsonRpc.createClient(transport);
        const p1 = client.request("textDocument/hover", null);
        const p2 = client.request("textDocument/definition", null);
        transport.__simulateClose("server crashed");
        let caught1 = null;
        let caught2 = null;
        try { await p1; } catch (err) { caught1 = err; }
        try { await p2; } catch (err) { caught2 = err; }
        if (!caught1 || !caught2) {
            throw new Error("both requests must reject after transport close");
        }
        // 続けて request すると即座に reject する
        let caught3 = null;
        try {
            await client.request("textDocument/hover", null);
        } catch (err) {
            caught3 = err;
        }
        if (!caught3) {
            throw new Error("request after close must reject immediately");
        }
    "#;
    evaluate_with_plugin(source, "jsonrpc-client-close")
        .await
        .expect("client should reject all pending on transport close");
}

// =========================================================================
// transport.ts: `__lspTransport` 名前空間のテスト
// =========================================================================

/// T-TRANSPORT-1: in-memory transport pair が双方向にバイトを配送する。
///
/// `createInMemoryPair()` は test 用に 2 つの transport を返し、片方の
/// `writeBytes` が他方の `onBytes` ハンドラに直接配送される。これは
/// LSP クライアント単体テストの基盤になる（実プロセス不要）。
#[tokio::test(flavor = "current_thread")]
async fn transport_in_memory_pair_delivers_bytes_bidirectionally() {
    let source = r#"
        const pair = __lspTransport.createInMemoryPair();
        if (!pair || !pair.client || !pair.server) {
            throw new Error("createInMemoryPair must return { client, server }");
        }
        const clientReceived = [];
        const serverReceived = [];
        pair.client.onBytes(function (chunk) { clientReceived.push(chunk); });
        pair.server.onBytes(function (chunk) { serverReceived.push(chunk); });
        const payload1 = new Uint8Array([1, 2, 3]);
        const payload2 = new Uint8Array([9, 8, 7, 6]);
        await pair.client.writeBytes(payload1);
        await pair.server.writeBytes(payload2);
        if (serverReceived.length !== 1 || serverReceived[0].length !== 3) {
            throw new Error(
                "server expected to receive 1 chunk of length 3, got "
                    + serverReceived.length + "/"
                    + (serverReceived[0] ? serverReceived[0].length : "n/a"),
            );
        }
        if (clientReceived.length !== 1 || clientReceived[0].length !== 4) {
            throw new Error(
                "client expected to receive 1 chunk of length 4, got "
                    + clientReceived.length + "/"
                    + (clientReceived[0] ? clientReceived[0].length : "n/a"),
            );
        }
        for (let i = 0; i < 3; i = i + 1) {
            if (serverReceived[0][i] !== payload1[i]) {
                throw new Error("server byte mismatch at " + i);
            }
        }
        for (let i = 0; i < 4; i = i + 1) {
            if (clientReceived[0][i] !== payload2[i]) {
                throw new Error("client byte mismatch at " + i);
            }
        }
    "#;
    evaluate_with_plugin(source, "transport-in-memory-pair")
        .await
        .expect("createInMemoryPair must deliver bytes bidirectionally");
}

/// T-TRANSPORT-2: in-memory transport を `close()` すると相手側の
/// `onClose` ハンドラが発火する。
///
/// transport の cleanup 契約。session の reader loop / pending pool が
/// 確実に reject されるための前提となる。
#[tokio::test(flavor = "current_thread")]
async fn transport_in_memory_pair_close_propagates_to_peer() {
    let source = r#"
        const pair = __lspTransport.createInMemoryPair();
        let observedCloseReason = null;
        pair.server.onClose(function (reason) { observedCloseReason = reason; });
        pair.client.close("client-side shutdown");
        if (observedCloseReason !== "client-side shutdown") {
            throw new Error(
                "expected close reason 'client-side shutdown', got " + JSON.stringify(observedCloseReason),
            );
        }
        // 二重 close は idempotent
        pair.client.close("duplicate");
        // close 後の writeBytes はエラーで reject する
        let caught = null;
        try {
            await pair.client.writeBytes(new Uint8Array([1]));
        } catch (err) {
            caught = err;
        }
        if (!caught) {
            throw new Error("writeBytes after close must reject");
        }
    "#;
    evaluate_with_plugin(source, "transport-in-memory-close")
        .await
        .expect("transport close must propagate and reject subsequent writes");
}

/// T-TRANSPORT-3: createClient + in-memory pair で end-to-end の
/// request/response が成立する。
///
/// LSP クライアント単体での JSON-RPC ワイヤを完全模擬する。client 側で
/// `client.request(...)` を呼び、server 側で受信した framing をパースして
/// 対応 response を返却することで往復が完了する。
#[tokio::test(flavor = "current_thread")]
async fn transport_client_server_round_trip_via_in_memory_pair() {
    let source = r#"
        const pair = __lspTransport.createInMemoryPair();
        // server 側 parser を立てて、id を保持した上で固定 result を返す
        const parser = __lspJsonRpc.createBytesParser(function (message) {
            if (typeof message.method === "string" && message.id !== undefined) {
                // `{ ok: true, echoedMethod: message.method }` リテラルは
                // transpiler が壊すため property assignment で組み立てる
                const resultPayload = { ok: true };
                resultPayload.echoedMethod = message.method;
                const response = __lspJsonRpc.buildSuccessResponseMessage(
                    message.id,
                    resultPayload,
                );
                pair.server.writeBytes(__lspJsonRpc.encodeMessageBytes(response));
            }
        });
        pair.server.onBytes(function (chunk) { parser.accept(chunk); });

        const client = __lspJsonRpc.createClient(pair.client);
        const result = await client.request("textDocument/hover", { uri: "file:///a" });
        if (!result || result.ok !== true || result.echoedMethod !== "textDocument/hover") {
            throw new Error("round-trip mismatch: " + JSON.stringify(result));
        }
    "#;
    evaluate_with_plugin(source, "transport-client-server-roundtrip")
        .await
        .expect("client + in-memory transport pair must round-trip a request");
}

// =========================================================================
// lifecycle.ts: `__lspLifecycle` 名前空間のテスト
// =========================================================================

/// T-LIFECYCLE-1: state machine が許可された遷移のみ受け付け、
/// 不正遷移を拒絶する。
///
/// `idle → initializing → ready → shuttingDown → exited` の一方向の
/// 流れを保証する。`ready → initializing` 等の戻り遷移は禁止。
#[tokio::test(flavor = "current_thread")]
async fn lifecycle_transitions_only_along_allowed_path() {
    let source = r#"
        const lc = __lspLifecycle.create();
        if (lc.state() !== "idle") {
            throw new Error("initial state must be 'idle', got " + lc.state());
        }
        lc.transition("initializing");
        if (lc.state() !== "initializing") {
            throw new Error("state after transition('initializing') should be 'initializing'");
        }
        lc.transition("ready");
        lc.transition("shuttingDown");
        lc.transition("exited");
        if (lc.state() !== "exited") {
            throw new Error("final state must be 'exited'");
        }
        // 不正遷移は throw
        let caught = null;
        try {
            const lc2 = __lspLifecycle.create();
            lc2.transition("ready");
        } catch (err) {
            caught = err;
        }
        if (!caught) {
            throw new Error("transition(idle → ready) must throw");
        }
        let caught2 = null;
        try {
            const lc3 = __lspLifecycle.create();
            lc3.transition("initializing");
            lc3.transition("idle");
        } catch (err) {
            caught2 = err;
        }
        if (!caught2) {
            throw new Error("transition(initializing → idle) must throw");
        }
    "#;
    evaluate_with_plugin(source, "lifecycle-transitions")
        .await
        .expect("lifecycle must enforce one-way state transitions");
}

/// T-LIFECYCLE-2: `ready` 到達まで request を queue に貯め、到達時に
/// 順序を保って flush する。
///
/// `initialize` 完了前に到着した `textDocument/didOpen` 等を捨てて
/// しまわないために必要。順序が崩れると LSP セッションが破綻する。
#[tokio::test(flavor = "current_thread")]
async fn lifecycle_queue_runs_pending_work_in_order_on_ready() {
    let source = r#"
        const lc = __lspLifecycle.create();
        const trail = [];
        lc.runWhenReady(function () { trail.push("first"); });
        lc.runWhenReady(function () { trail.push("second"); });
        lc.runWhenReady(function () { trail.push("third"); });
        if (trail.length !== 0) {
            throw new Error("queued work must not run before ready, got " + trail.length);
        }
        lc.transition("initializing");
        if (trail.length !== 0) {
            throw new Error("queued work must wait for ready, not initializing");
        }
        lc.transition("ready");
        // microtask boundary
        await Promise.resolve();
        if (trail.length !== 3) {
            throw new Error("expected 3 flushed tasks, got " + trail.length);
        }
        if (trail[0] !== "first" || trail[1] !== "second" || trail[2] !== "third") {
            throw new Error("flush order broken: " + JSON.stringify(trail));
        }
        // ready 状態で追加された work は即座に実行される
        lc.runWhenReady(function () { trail.push("post-ready"); });
        await Promise.resolve();
        if (trail.length !== 4 || trail[3] !== "post-ready") {
            throw new Error("post-ready work must run synchronously (microtask), got " + JSON.stringify(trail));
        }
    "#;
    evaluate_with_plugin(source, "lifecycle-queue-order")
        .await
        .expect("lifecycle queue must flush work in order on transition to ready");
}

/// T-LIFECYCLE-3: `exited` (or `shuttingDown` 経由) になると queue の
/// 未実行 work は reject される。
///
/// LSP サーバが crash した場合、未配送の request が宙に浮かないように
/// する。`runWhenReady` の戻り値 promise はすべて reject される。
#[tokio::test(flavor = "current_thread")]
async fn lifecycle_queue_rejects_on_exit_before_ready() {
    let source = r#"
        const lc = __lspLifecycle.create();
        const pending1 = lc.runWhenReady(function () { return "result-1"; });
        const pending2 = lc.runWhenReady(function () { return "result-2"; });
        lc.transition("initializing");
        lc.transition("exited");
        let caught1 = null;
        let caught2 = null;
        try { await pending1; } catch (err) { caught1 = err; }
        try { await pending2; } catch (err) { caught2 = err; }
        if (!caught1 || !caught2) {
            throw new Error("queued work must reject when exited before ready");
        }
    "#;
    evaluate_with_plugin(source, "lifecycle-queue-exit-reject")
        .await
        .expect("lifecycle queue must reject pending work on exit");
}

// =========================================================================
// session.ts: `__lspSession` 名前空間のテスト
// =========================================================================

/// T-SESSION-1: `start(initializeParams)` が initialize 要求を送り、
/// capability 応答を受け、`initialized` 通知を送って `ready` に遷移する。
///
/// LSP lifecycle の中核となるハンドシェイク。シーケンスは
/// (1) clinet → server: `initialize`
/// (2) server → client: capability response
/// (3) client → server: `initialized` 通知
/// (4) 状態が `ready` に遷移
///
/// fake server を in-memory pair で立てて、観測した順序を検証する。
#[tokio::test(flavor = "current_thread")]
async fn session_start_completes_initialize_handshake_and_reaches_ready() {
    let source = r#"
        const pair = __lspTransport.createInMemoryPair();
        // fake server - initialize に対して capability を返す
        const serverObserved = [];
        const parser = __lspJsonRpc.createBytesParser(function (message) {
            // object literal の key:identifier パターンを避けて property assignment で組み立てる
            const observation = {};
            observation.method = message.method;
            observation.hasId = message.id !== undefined;
            serverObserved.push(observation);
            if (message.method === "initialize") {
                const capabilities = { hoverProvider: true };
                const responseResult = { capabilities };
                const response = __lspJsonRpc.buildSuccessResponseMessage(
                    message.id,
                    responseResult,
                );
                pair.server.writeBytes(__lspJsonRpc.encodeMessageBytes(response));
            }
        });
        pair.server.onBytes(function (chunk) { parser.accept(chunk); });

        const sessionOptions = {};
        sessionOptions.clientName = "saya-test";
        const session = __lspSession.create(pair.client, sessionOptions);
        const initializeParams = { processId: null, rootUri: "file:///workspace", capabilities: {} };
        const result = await session.start(initializeParams);
        if (session.state() !== "ready") {
            throw new Error("session must be 'ready' after start, got " + session.state());
        }
        if (!result || !result.capabilities || result.capabilities.hoverProvider !== true) {
            throw new Error("expected capabilities to be returned, got " + JSON.stringify(result));
        }
        // microtask 一巡して initialized 通知が flush するのを待つ
        await Promise.resolve();
        await Promise.resolve();
        if (serverObserved.length < 2) {
            throw new Error("expected initialize + initialized observations, got " + serverObserved.length);
        }
        if (serverObserved[0].method !== "initialize" || !serverObserved[0].hasId) {
            throw new Error("first observation must be initialize request: " + JSON.stringify(serverObserved[0]));
        }
        if (serverObserved[1].method !== "initialized" || serverObserved[1].hasId) {
            throw new Error("second observation must be initialized notification: " + JSON.stringify(serverObserved[1]));
        }
    "#;
    evaluate_with_plugin(source, "session-initialize-handshake")
        .await
        .expect("session.start should complete initialize handshake and reach ready");
}

/// T-SESSION-2: `ready` 到達前の `request` は queue に積まれ、ready 到達時に
/// 送信される。
///
/// `initialize` 中に hover 等が来ても捨てず、handshake 後に送る。
/// `start()` と並行に request を呼んでも捨てられないことを検証。
#[tokio::test(flavor = "current_thread")]
async fn session_queues_requests_until_ready_then_flushes() {
    let source = r#"
        const pair = __lspTransport.createInMemoryPair();
        const serverMethods = [];
        let initializeId = null;
        const parser = __lspJsonRpc.createBytesParser(function (message) {
            if (message.method) {
                serverMethods.push(message.method);
            }
            if (message.method === "initialize") {
                initializeId = message.id;
                // 即座には返さない（hover が queue に積まれることを確認するため）
                Promise.resolve().then(function () {
                    const capabilities = { hoverProvider: true };
                    const result = { capabilities };
                    pair.server.writeBytes(__lspJsonRpc.encodeMessageBytes(
                        __lspJsonRpc.buildSuccessResponseMessage(initializeId, result),
                    ));
                });
                return;
            }
            if (message.method === "textDocument/hover" && message.id !== undefined) {
                const hoverResult = { contents: "hovered" };
                pair.server.writeBytes(__lspJsonRpc.encodeMessageBytes(
                    __lspJsonRpc.buildSuccessResponseMessage(message.id, hoverResult),
                ));
            }
        });
        pair.server.onBytes(function (chunk) { parser.accept(chunk); });

        const session = __lspSession.create(pair.client, {});
        // initialize を発火する前に hover request を投げる
        const hoverPromise = session.request("textDocument/hover", { uri: "file:///a" });
        // start を呼ぶ
        const initializeParams = { processId: null, rootUri: null, capabilities: {} };
        const startPromise = session.start(initializeParams);
        const startResult = await startPromise;
        if (!startResult || !startResult.capabilities) {
            throw new Error("start should resolve with capabilities");
        }
        const hover = await hoverPromise;
        if (!hover || hover.contents !== "hovered") {
            throw new Error("queued hover must complete after ready, got " + JSON.stringify(hover));
        }
        // serverMethods には initialize, initialized, textDocument/hover が
        // この順で観測されているはず（hover は ready 後）
        let idxInit = -1;
        let idxInitialized = -1;
        let idxHover = -1;
        for (let i = 0; i < serverMethods.length; i = i + 1) {
            if (serverMethods[i] === "initialize" && idxInit < 0) idxInit = i;
            if (serverMethods[i] === "initialized" && idxInitialized < 0) idxInitialized = i;
            if (serverMethods[i] === "textDocument/hover" && idxHover < 0) idxHover = i;
        }
        if (idxInit < 0 || idxInitialized < 0 || idxHover < 0) {
            throw new Error("missing observation: " + JSON.stringify(serverMethods));
        }
        if (!(idxInit < idxInitialized && idxInitialized < idxHover)) {
            throw new Error("order violated: " + JSON.stringify(serverMethods));
        }
    "#;
    evaluate_with_plugin(source, "session-queue-flush")
        .await
        .expect("session must queue requests until ready");
}

/// T-SESSION-3: `shutdown()` が `shutdown` request → `exit` 通知 →
/// `exited` 遷移の順で動作する。
///
/// LSP の正規 shutdown シーケンス。エディタ終了時に未送信の request が
/// なくなり、サーバプロセスがクリーンに片付くことを保証する。
#[tokio::test(flavor = "current_thread")]
async fn session_shutdown_sequence_sends_shutdown_then_exit_and_transitions_exited() {
    let source = r#"
        const pair = __lspTransport.createInMemoryPair();
        const serverObserved = [];
        const parser = __lspJsonRpc.createBytesParser(function (message) {
            const observation = {};
            observation.method = message.method;
            observation.id = message.id;
            observation.hasId = message.id !== undefined;
            serverObserved.push(observation);
            if (message.method === "initialize") {
                const capabilities = {};
                const result = { capabilities };
                pair.server.writeBytes(__lspJsonRpc.encodeMessageBytes(
                    __lspJsonRpc.buildSuccessResponseMessage(message.id, result),
                ));
                return;
            }
            if (message.method === "shutdown") {
                pair.server.writeBytes(__lspJsonRpc.encodeMessageBytes(
                    __lspJsonRpc.buildSuccessResponseMessage(message.id, null),
                ));
            }
        });
        pair.server.onBytes(function (chunk) { parser.accept(chunk); });

        const session = __lspSession.create(pair.client, {});
        await session.start({ processId: null, rootUri: null, capabilities: {} });
        await session.shutdown();
        if (session.state() !== "exited") {
            throw new Error("after shutdown the session must be 'exited', got " + session.state());
        }
        // shutdown と exit が両方観測されているか
        let sawShutdown = false;
        let sawExit = false;
        for (let i = 0; i < serverObserved.length; i = i + 1) {
            if (serverObserved[i].method === "shutdown" && serverObserved[i].hasId) sawShutdown = true;
            if (serverObserved[i].method === "exit" && !serverObserved[i].hasId) sawExit = true;
        }
        if (!sawShutdown || !sawExit) {
            throw new Error("shutdown sequence missing: " + JSON.stringify(serverObserved));
        }
    "#;
    evaluate_with_plugin(source, "session-shutdown")
        .await
        .expect("session.shutdown must perform shutdown + exit and transition to exited");
}
