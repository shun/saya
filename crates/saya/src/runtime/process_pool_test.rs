use super::*;
use std::time::Duration;

/// テスト用に `cat -u` を起動する仕様を組み立てる。
///
/// `cat -u` は Linux / macOS の標準ユーティリティで、unbuffered モード
/// で stdin から読んだバイトをそのまま stdout に書き戻す echo サーバ
/// として振る舞う。LSP プロセスの代用として round-trip テストに用いる。
fn cat_spec() -> ProcessSpec {
    ProcessSpec {
        command: "cat".to_string(),
        args: vec!["-u".to_string()],
        stdin: StdioMode::Piped,
        stdout: StdioMode::Piped,
        stderr: StdioMode::Piped,
        ..ProcessSpec::default()
    }
}

/// T1: 空文字の `command` で `spawn` を呼ぶと `InvalidSpec` で即返ること。
///
/// プロセス起動の前段で入力検証を行い、誤った `ProcessSpec` をそのまま
/// `tokio::process::Command` に渡してしまわないことを保証する。
#[tokio::test]
async fn spawn_with_empty_command_returns_invalid_spec_error() {
    let pool = ProcessPool::new();
    let spec = ProcessSpec {
        command: String::new(),
        ..ProcessSpec::default()
    };

    let result = pool.spawn(spec).await;

    assert!(
        matches!(result, Err(ProcessPoolError::InvalidSpec { .. })),
        "expected InvalidSpec error, got {result:?}"
    );
}

/// T2: 同じプールで連続して `spawn` を呼ぶと、ハンドル ID が単調増加で
/// 払い出される。
///
/// ハンドル ID は `u32` の連番。`AtomicU32` をベースとした採番が、
/// 同じセッションを別の ID と取り違えない最低条件である。
#[tokio::test]
async fn spawn_returns_monotonic_handle_ids() {
    let pool = ProcessPool::new();

    let h1 = pool.spawn(cat_spec()).await.expect("first spawn succeeds");
    let h2 = pool.spawn(cat_spec()).await.expect("second spawn succeeds");

    assert!(
        h2 > h1,
        "expected monotonically increasing handle ids, got h1={h1}, h2={h2}"
    );
}

/// 即時に終了する子プロセスを起動するための仕様。
///
/// `true(1)` は POSIX 標準で常に exit 0 を返すユーティリティ。
/// stdin/stdout を piped にしておくと、起動直後に stdout が EOF を
/// 返すかを観測できる。
fn true_spec() -> ProcessSpec {
    ProcessSpec {
        command: "true".to_string(),
        args: Vec::new(),
        stdin: StdioMode::Null,
        stdout: StdioMode::Piped,
        stderr: StdioMode::Piped,
        ..ProcessSpec::default()
    }
}

/// T3: `cat -u` を起動し、stdin に書いたバイトを stdout から読み返せる
/// ことを確認する。
///
/// 実プロセスとの双方向 I/O が成立することの最低限の保証。これが
/// 通れば `write_stdin` / `read_stdout` の責務分担と排他制御が機能
/// していると判断できる。
#[tokio::test]
async fn spawn_cat_then_write_stdin_then_read_stdout_round_trip() {
    let pool = ProcessPool::new();
    let handle = pool.spawn(cat_spec()).await.expect("cat spawns");

    let payload: &[u8] = b"saya-process round trip\n";
    let written = pool
        .write_stdin(handle, payload)
        .await
        .expect("write_stdin succeeds");
    assert_eq!(
        written,
        payload.len(),
        "write_stdin should report the full payload length"
    );

    // 読み戻し。cat -u は unbuffered なので同じバイト列が返ってくる。
    // テストがハングしないようタイムアウトでガード。
    let mut buf = [0u8; 128];
    let read_result =
        tokio::time::timeout(Duration::from_secs(2), pool.read_stdout(handle, &mut buf))
            .await
            .expect("read_stdout completes within 2s");
    let n = read_result
        .expect("read_stdout returns Ok")
        .expect("read_stdout returns Some bytes (not EOF)");
    assert_eq!(
        &buf[..n],
        payload,
        "stdout should echo the bytes written to stdin"
    );
}

/// T12: stdout を `Piped` 以外で起動したプロセスへの `read_stdout` は
/// `StreamNotPiped` で返る。
///
/// プラグイン側のミス（spawn 時の StdioMode を間違えた等）が壊れた
/// I/O ループに化けないよう、明示的に弾く契約。
#[tokio::test]
async fn stream_not_piped_errors_when_reading_null_mode() {
    let pool = ProcessPool::new();
    let spec = ProcessSpec {
        command: "true".to_string(),
        stdin: StdioMode::Null,
        stdout: StdioMode::Null,
        stderr: StdioMode::Null,
        ..ProcessSpec::default()
    };
    let handle = pool
        .spawn(spec)
        .await
        .expect("true spawns even with stdout=Null");

    let mut buf = [0u8; 16];
    let result = pool.read_stdout(handle, &mut buf).await;
    assert!(
        matches!(
            result,
            Err(ProcessPoolError::StreamNotPiped {
                stream: "stdout",
                ..
            })
        ),
        "expected StreamNotPiped for stdout, got {result:?}"
    );
}

/// T13: stderr の読み出しが stdout と独立して動作する。
///
/// `sh -c "printf out; printf err 1>&2"` を起動し、stdout と stderr の
/// 内容がそれぞれの read で取得できることを確認する。LSP server の
/// 診断ログ収集経路として stderr が必須なので、独立性を保証する。
#[tokio::test]
async fn stderr_read_independent_from_stdout_read() {
    let pool = ProcessPool::new();
    let spec = ProcessSpec {
        command: "sh".to_string(),
        args: vec!["-c".to_string(), "printf out; printf err 1>&2".to_string()],
        stdin: StdioMode::Null,
        stdout: StdioMode::Piped,
        stderr: StdioMode::Piped,
        ..ProcessSpec::default()
    };
    let handle = pool.spawn(spec).await.expect("sh spawns");

    // 短命プロセスなので、書き込みが終わるのを少し待つ。
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut out_buf = [0u8; 16];
    let mut err_buf = [0u8; 16];

    let n_out = tokio::time::timeout(
        Duration::from_secs(2),
        pool.read_stdout(handle, &mut out_buf),
    )
    .await
    .expect("stdout read within 2s")
    .expect("stdout read ok")
    .expect("stdout returns Some");
    let n_err = tokio::time::timeout(
        Duration::from_secs(2),
        pool.read_stderr(handle, &mut err_buf),
    )
    .await
    .expect("stderr read within 2s")
    .expect("stderr read ok")
    .expect("stderr returns Some");

    assert_eq!(&out_buf[..n_out], b"out");
    assert_eq!(&err_buf[..n_err], b"err");
}

/// T14: `read_stdout` は `buf.len()` を超えるバイトを書き込まない。
///
/// `AsyncRead::read` の標準仕様だが、誤って `read_to_end` 等に変更
/// された場合に検出できるよう契約として固定する。
#[tokio::test]
async fn read_stdout_into_buffer_writes_at_most_buf_len_bytes() {
    let pool = ProcessPool::new();
    let handle = pool.spawn(cat_spec()).await.expect("cat spawns");

    // バッファサイズより十分大きいペイロード。
    let payload: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRST"; // 30 bytes
    pool.write_stdin(handle, payload)
        .await
        .expect("write_stdin succeeds");

    let mut small_buf = [0u8; 8];
    let n = tokio::time::timeout(
        Duration::from_secs(2),
        pool.read_stdout(handle, &mut small_buf),
    )
    .await
    .expect("read within 2s")
    .expect("read ok")
    .expect("read returns Some");

    assert!(
        n <= small_buf.len(),
        "read_stdout returned {n} bytes which exceeds buffer length {}",
        small_buf.len()
    );
}

/// T11: `shutdown_all` でプール内の全プロセスが kill される。
///
/// saya 終了時の sweeper の契約。kill 済みハンドルでもエラーを発生
/// させず、`wait` で exit コードを取得できることを確認する。
#[tokio::test]
async fn shutdown_all_kills_every_alive_session() {
    let pool = ProcessPool::new();
    let h1 = pool.spawn(cat_spec()).await.expect("first spawn");
    let h2 = pool.spawn(cat_spec()).await.expect("second spawn");

    pool.shutdown_all().await;

    let code1 = tokio::time::timeout(Duration::from_secs(2), pool.wait(h1))
        .await
        .expect("wait 1 within 2s")
        .expect("wait 1 ok");
    let code2 = tokio::time::timeout(Duration::from_secs(2), pool.wait(h2))
        .await
        .expect("wait 2 within 2s")
        .expect("wait 2 ok");
    assert_ne!(code1, 0, "session 1 expected non-zero exit code");
    assert_ne!(code2, 0, "session 2 expected non-zero exit code");
}

/// T9: `kill` でプロセスが終了し、`wait` が exit コードを返す。
///
/// `cat -u` を起動して即座に kill すると、SIGKILL を受けた状態で終了
/// するため exit コードは 0 にならない契約。
#[tokio::test]
async fn kill_terminates_child_and_wait_returns_exit_code() {
    let pool = ProcessPool::new();
    let handle = pool.spawn(cat_spec()).await.expect("cat spawns");
    pool.kill(handle).await.expect("kill ok");

    let code = tokio::time::timeout(Duration::from_secs(2), pool.wait(handle))
        .await
        .expect("wait completes within 2s")
        .expect("wait returns Ok");
    assert_ne!(
        code, 0,
        "expected non-zero exit code after kill, got {code}"
    );
}

/// T10: 未登録ハンドルへの `wait` は `UnknownHandle` を返す。
#[tokio::test]
async fn wait_on_unknown_handle_errors() {
    let pool = ProcessPool::new();
    let result = pool.wait(9999).await;
    assert!(
        matches!(
            result,
            Err(ProcessPoolError::UnknownHandle { handle: 9999 })
        ),
        "expected UnknownHandle for wait, got {result:?}"
    );
}

/// T8: 複数のハンドルが互いに独立に動く。
///
/// 1 つのプールで 2 セッション (`cat`) を起動し、それぞれ異なるバイト列
/// を書き、対応する stdout から正しく読み返せることを確認する。
/// 内部の `HashMap` キーが取り違えられていないことの保証。
#[tokio::test]
async fn multiple_handles_run_independently() {
    let pool = ProcessPool::new();
    let h_a = pool.spawn(cat_spec()).await.expect("first cat spawns");
    let h_b = pool.spawn(cat_spec()).await.expect("second cat spawns");

    let payload_a: &[u8] = b"alpha\n";
    let payload_b: &[u8] = b"bravo-bravo\n";

    pool.write_stdin(h_a, payload_a)
        .await
        .expect("write to a succeeds");
    pool.write_stdin(h_b, payload_b)
        .await
        .expect("write to b succeeds");

    let mut buf_a = [0u8; 64];
    let n_a = tokio::time::timeout(Duration::from_secs(2), pool.read_stdout(h_a, &mut buf_a))
        .await
        .expect("read a within 2s")
        .expect("read a ok")
        .expect("read a returns Some");
    assert_eq!(&buf_a[..n_a], payload_a);

    let mut buf_b = [0u8; 64];
    let n_b = tokio::time::timeout(Duration::from_secs(2), pool.read_stdout(h_b, &mut buf_b))
        .await
        .expect("read b within 2s")
        .expect("read b ok")
        .expect("read b returns Some");
    assert_eq!(&buf_b[..n_b], payload_b);
}

/// T7: 同じ stdout を複数タスクから同時に `read_stdout` すると、
/// 後発のタスクは `AlreadyReading` を受け取って即時に拒否される。
///
/// これがないと、両タスクで read のバイト列が分割して受け取られ、
/// 上位 (TS の framing parser 等) で順序保証が壊れる。
/// `Mutex::try_lock` による排他で防御する契約。
#[tokio::test]
async fn parallel_read_stdout_on_same_handle_errors_with_already_reading() {
    let pool = std::sync::Arc::new(ProcessPool::new());
    let handle = pool.spawn(cat_spec()).await.expect("cat spawns");

    // 1 本目の reader をバックグラウンドで開始する。
    // cat -u は stdin に何も来ていない間は block するので、
    // この read 呼び出しは Mutex を取った状態で待機する。
    let blocker_pool = pool.clone();
    let blocker = tokio::spawn(async move {
        let mut buf = [0u8; 32];
        // テストハングを避けるため呼び出し全体にタイムアウトを掛ける。
        let _ = tokio::time::timeout(
            Duration::from_millis(800),
            blocker_pool.read_stdout(handle, &mut buf),
        )
        .await;
    });

    // 1 本目が Mutex を確保するまで待つ。
    tokio::time::sleep(Duration::from_millis(80)).await;

    // 2 本目は AlreadyReading で拒否されるはず。
    let mut buf2 = [0u8; 32];
    let result = pool.read_stdout(handle, &mut buf2).await;
    assert!(
        matches!(
            result,
            Err(ProcessPoolError::AlreadyReading {
                handle: _,
                stream: "stdout"
            })
        ),
        "expected AlreadyReading on stdout, got {result:?}"
    );

    // 1 本目はタイムアウトで畳み終わるはずなので join しておく。
    blocker.await.ok();
}

/// T5: 未登録ハンドルへの `read_stdout` は `UnknownHandle` で返る。
#[tokio::test]
async fn read_stdout_with_unknown_handle_errors() {
    let pool = ProcessPool::new();
    let mut buf = [0u8; 16];
    let result = pool.read_stdout(9999, &mut buf).await;
    assert!(
        matches!(
            result,
            Err(ProcessPoolError::UnknownHandle { handle: 9999 })
        ),
        "expected UnknownHandle for read_stdout, got {result:?}"
    );
}

/// T6: 未登録ハンドルへの `write_stdin` は `UnknownHandle` で返る。
#[tokio::test]
async fn write_stdin_with_unknown_handle_errors() {
    let pool = ProcessPool::new();
    let result = pool.write_stdin(9999, b"x").await;
    assert!(
        matches!(
            result,
            Err(ProcessPoolError::UnknownHandle { handle: 9999 })
        ),
        "expected UnknownHandle for write_stdin, got {result:?}"
    );
}

/// T4: 子プロセスが exit した後の `read_stdout` は EOF (`Ok(None)`) を
/// 返す。
///
/// `true` は起動直後に exit 0 で終了する。stdout が close されるので、
/// `read_stdout` は 0 バイト読みで `None` を返す契約となる。これにより
/// TS 側のループが「EOF=null」を検出してループを抜けられる。
#[tokio::test]
async fn read_stdout_returns_none_after_child_exits() {
    let pool = ProcessPool::new();
    let handle = pool.spawn(true_spec()).await.expect("true command spawns");

    let mut buf = [0u8; 64];
    let outcome = tokio::time::timeout(Duration::from_secs(2), pool.read_stdout(handle, &mut buf))
        .await
        .expect("read_stdout completes within 2s")
        .expect("read_stdout returns Ok");
    assert_eq!(outcome, None, "expected EOF (None) after the child exited");
}
