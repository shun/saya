//! 汎用プロセス I/O プール（issue #32 の Phase A）。
//!
//! `saya.process.*` の Rust 側基盤を提供する。LSP / DAP / linter / formatter
//! など、子プロセスとバイトストリームでやり取りするすべてのプラグインから
//! 共通利用される。
//!
//! 設計指針:
//! - 子プロセスは `tokio::process::Child` で管理し、`kill_on_drop(true)` を
//!   付与して leak を防ぐ。
//! - 各ストリーム (stdin / stdout / stderr) は独立した `tokio::sync::Mutex`
//!   で保持し、stdin への書き込み中でも stdout の読み出しを並行できるように
//!   する。
//! - stdout / stderr の二重読み出しは `try_lock` で検出し、busy であれば
//!   `AlreadyReading` を返す（パイプのバイト順序を破壊するバグを防ぐ）。
//! - エラーは意味的に分離した `enum` で返し、文字列化を排除する。
//!
//! 本モジュールは特定プロトコルの知識を一切持たない汎用基盤である。

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{Mutex, TryLockError};

/// 子プロセスのストリームをどう扱うかの指定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdioMode {
    /// 親プロセスから継承する。
    Inherit,
    /// `/dev/null` 相当に向ける。
    Null,
    /// パイプを開き、`ProcessPool` 経由で読み書きする。
    Piped,
}

/// 子プロセスの起動仕様。
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub stdin: StdioMode,
    pub stdout: StdioMode,
    pub stderr: StdioMode,
}

impl Default for ProcessSpec {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            stdin: StdioMode::Null,
            stdout: StdioMode::Null,
            stderr: StdioMode::Null,
        }
    }
}

/// プロセスプールが返すエラーの分類。
#[derive(Debug)]
pub enum ProcessPoolError {
    /// `ProcessSpec` の検証に失敗した。
    InvalidSpec { detail: String },
    /// プロセス起動自体が失敗した。
    Spawn { detail: String },
    /// 指定された `handle` が存在しない。
    UnknownHandle { handle: u32 },
    /// 指定ストリームが `StdioMode::Piped` で開かれていない。
    StreamNotPiped {
        handle: u32,
        stream: &'static str,
    },
    /// 同じストリームを別タスクが既に読み出している。
    AlreadyReading {
        handle: u32,
        stream: &'static str,
    },
    /// I/O 操作でエラーが発生した。
    Io {
        context: &'static str,
        detail: String,
    },
    /// プロセスは既に kill 済みである。
    AlreadyKilled { handle: u32 },
}

impl std::fmt::Display for ProcessPoolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessPoolError::InvalidSpec { detail } => {
                write!(formatter, "invalid process spec: {detail}")
            }
            ProcessPoolError::Spawn { detail } => {
                write!(formatter, "failed to spawn process: {detail}")
            }
            ProcessPoolError::UnknownHandle { handle } => {
                write!(formatter, "unknown process handle: {handle}")
            }
            ProcessPoolError::StreamNotPiped { handle, stream } => write!(
                formatter,
                "stream not piped: handle={handle}, stream={stream}"
            ),
            ProcessPoolError::AlreadyReading { handle, stream } => write!(
                formatter,
                "stream already being read: handle={handle}, stream={stream}"
            ),
            ProcessPoolError::Io { context, detail } => write!(formatter, "{context}: {detail}"),
            ProcessPoolError::AlreadyKilled { handle } => {
                write!(formatter, "process already killed: handle={handle}")
            }
        }
    }
}

impl std::error::Error for ProcessPoolError {}

/// 起動済みの子プロセス 1 件を表す内部レコード。
///
/// 各ストリームを独立した `Mutex` で保護することで、`stdin` への書き込み中
/// でも `stdout` / `stderr` の読み出しを並行できるようにする。
struct ChildSession {
    /// プロセスハンドル本体。`kill` / `wait` のために排他確保する。
    child: Mutex<Child>,
    /// `StdioMode::Piped` のときのみ `Some`。
    stdin: Option<Mutex<ChildStdin>>,
    /// `StdioMode::Piped` のときのみ `Some`。二重 read 検出には `try_lock` を
    /// 用いる。
    stdout: Option<Mutex<ChildStdout>>,
    /// `StdioMode::Piped` のときのみ `Some`。二重 read 検出には `try_lock` を
    /// 用いる。
    stderr: Option<Mutex<ChildStderr>>,
}

/// 子プロセス群を所有するプール。
///
/// `Default` で空のプールを得る。内部状態は `spawn` で追加され、`kill` / 終了時
/// sweeper で解放される。
pub struct ProcessPool {
    sessions: Mutex<HashMap<u32, Arc<ChildSession>>>,
    next_id: AtomicU32,
}

impl Default for ProcessPool {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            // ハンドル ID は 1 始まり。0 は「未割当」を表す番兵として確保しておく。
            next_id: AtomicU32::new(1),
        }
    }
}

impl ProcessPool {
    pub fn new() -> Self {
        Self::default()
    }

    /// 子プロセスを起動し、ハンドル ID を返す。
    pub async fn spawn(&self, spec: ProcessSpec) -> Result<u32, ProcessPoolError> {
        log::debug!(
            "[process_pool] spawn requested: command={:?}, args_len={}, stdin={:?}, stdout={:?}, stderr={:?}",
            spec.command,
            spec.args.len(),
            spec.stdin,
            spec.stdout,
            spec.stderr
        );
        if spec.command.trim().is_empty() {
            log::debug!("[process_pool] spawn rejected: empty command");
            return Err(ProcessPoolError::InvalidSpec {
                detail: "command must be a non-empty string".to_string(),
            });
        }

        let mut command = tokio::process::Command::new(&spec.command);
        command.args(&spec.args);
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        if let Some(cwd) = spec.cwd.as_ref() {
            command.current_dir(cwd);
        }
        command
            .stdin(stdio_for(spec.stdin))
            .stdout(stdio_for(spec.stdout))
            .stderr(stdio_for(spec.stderr))
            // プールが drop された際にも子プロセスが残らないよう OS 側で kill する。
            .kill_on_drop(true);

        let mut child = command.spawn().map_err(|error| {
            log::debug!(
                "[process_pool] spawn failed: command={:?}, error={}",
                spec.command,
                error
            );
            ProcessPoolError::Spawn {
                detail: error.to_string(),
            }
        })?;

        let stdin = child.stdin.take().map(Mutex::new);
        let stdout = child.stdout.take().map(Mutex::new);
        let stderr = child.stderr.take().map(Mutex::new);
        let session = Arc::new(ChildSession {
            child: Mutex::new(child),
            stdin,
            stdout,
            stderr,
        });

        let handle = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut sessions = self.sessions.lock().await;
        sessions.insert(handle, session);
        log::debug!(
            "[process_pool] spawn ok: handle={handle}, command={:?}",
            spec.command
        );
        Ok(handle)
    }

    /// `handle` の stdin に `buf` を書き込む。
    ///
    /// 書き込めたバイト数を返す。`AsyncWriteExt::write_all` を使うので、
    /// 戻り値は常に `buf.len()` と一致する（途中失敗は `Err` で返る）。
    pub async fn write_stdin(
        &self,
        handle: u32,
        buf: &[u8],
    ) -> Result<usize, ProcessPoolError> {
        log::trace!(
            "[process_pool] write_stdin: handle={handle}, bytes={}",
            buf.len()
        );
        let session = self.session(handle).await?;
        let stdin_mutex =
            session
                .stdin
                .as_ref()
                .ok_or(ProcessPoolError::StreamNotPiped {
                    handle,
                    stream: "stdin",
                })?;
        let mut stdin = stdin_mutex.lock().await;
        stdin.write_all(buf).await.map_err(|error| {
            log::debug!(
                "[process_pool] stdin write_all failed: handle={handle}, error={error}"
            );
            ProcessPoolError::Io {
                context: "stdin write_all",
                detail: error.to_string(),
            }
        })?;
        stdin.flush().await.map_err(|error| {
            log::debug!("[process_pool] stdin flush failed: handle={handle}, error={error}");
            ProcessPoolError::Io {
                context: "stdin flush",
                detail: error.to_string(),
            }
        })?;
        log::trace!(
            "[process_pool] write_stdin ok: handle={handle}, bytes={}",
            buf.len()
        );
        Ok(buf.len())
    }

    /// `handle` の stdout から `buf` に最大 `buf.len()` バイト読み出す。
    ///
    /// `Ok(Some(n))` で実際に読めたバイト数、`Ok(None)` で EOF を表す。
    /// 同じ stdout を複数のタスクから同時に読もうとした場合は
    /// `AlreadyReading` を返す（パイプの順序保証を破る読み出しを防ぐ）。
    pub async fn read_stdout(
        &self,
        handle: u32,
        buf: &mut [u8],
    ) -> Result<Option<usize>, ProcessPoolError> {
        self.read_stream(handle, buf, ReadTarget::Stdout).await
    }

    /// `handle` の stderr から `buf` に最大 `buf.len()` バイト読み出す。
    ///
    /// 動作仕様は `read_stdout` と同等（`AlreadyReading` 排他、EOF=`None`）。
    pub async fn read_stderr(
        &self,
        handle: u32,
        buf: &mut [u8],
    ) -> Result<Option<usize>, ProcessPoolError> {
        self.read_stream(handle, buf, ReadTarget::Stderr).await
    }

    /// `handle` の stdout / stderr 共通の読み出し処理。
    async fn read_stream(
        &self,
        handle: u32,
        buf: &mut [u8],
        target: ReadTarget,
    ) -> Result<Option<usize>, ProcessPoolError> {
        log::trace!(
            "[process_pool] read_{}: handle={handle}, capacity={}",
            target.label(),
            buf.len()
        );
        let session = self.session(handle).await?;
        match target {
            ReadTarget::Stdout => read_via_mutex(
                handle,
                session.stdout.as_ref(),
                "stdout",
                buf,
            )
            .await,
            ReadTarget::Stderr => read_via_mutex(
                handle,
                session.stderr.as_ref(),
                "stderr",
                buf,
            )
            .await,
        }
    }

    /// `handle` のプロセスに kill シグナルを送る。
    ///
    /// 戻り値は「kill リクエストが OS に渡せたか」のみを表し、プロセスが
    /// 実際に終了したことは保証しない。`wait` を続けて呼ぶことで終了
    /// コードを取得できる。
    pub async fn kill(&self, handle: u32) -> Result<(), ProcessPoolError> {
        let session = self.session(handle).await?;
        let mut child = session.child.lock().await;
        child.start_kill().map_err(|error| {
            log::debug!("[process_pool] start_kill failed: handle={handle}, error={error}");
            ProcessPoolError::Io {
                context: "child start_kill",
                detail: error.to_string(),
            }
        })?;
        log::debug!("[process_pool] kill signal sent: handle={handle}");
        Ok(())
    }

    /// プールに残っているすべてのプロセスを kill する。
    ///
    /// saya 終了時に呼ばれる sweeper。エラーは debug ログに残しつつ可能
    /// な限り全件 kill を試みる。明示 kill 済みのプロセスを再 kill しても
    /// OS がエラーを返すだけで害はないため、ここでは握りつぶす。
    pub async fn shutdown_all(&self) {
        let handles: Vec<u32> = {
            let sessions = self.sessions.lock().await;
            sessions.keys().copied().collect()
        };
        log::info!(
            "[process_pool] shutdown_all sweeping {} session(s)",
            handles.len()
        );
        for handle in handles {
            if let Err(error) = self.kill(handle).await {
                log::debug!(
                    "[process_pool] shutdown_all best-effort kill error: handle={handle}, error={error}"
                );
            }
        }
    }

    /// `handle` のプロセスの終了を待ち、exit コードを返す。
    ///
    /// シグナルで終了した場合は Unix 慣例に倣って `128 + signal` を返す。
    pub async fn wait(&self, handle: u32) -> Result<i32, ProcessPoolError> {
        let session = self.session(handle).await?;
        let mut child = session.child.lock().await;
        let status = child.wait().await.map_err(|error| {
            log::debug!("[process_pool] wait failed: handle={handle}, error={error}");
            ProcessPoolError::Io {
                context: "child wait",
                detail: error.to_string(),
            }
        })?;
        let code = exit_code_from(&status);
        log::debug!("[process_pool] wait done: handle={handle}, code={code}");
        Ok(code)
    }

    /// 内部ヘルパ: ハンドルから `Arc<ChildSession>` を取り出す。
    async fn session(&self, handle: u32) -> Result<Arc<ChildSession>, ProcessPoolError> {
        let sessions = self.sessions.lock().await;
        sessions
            .get(&handle)
            .cloned()
            .ok_or(ProcessPoolError::UnknownHandle { handle })
    }
}

/// stdout / stderr のどちらを読み出すかを示す内部マーカ。
#[derive(Debug, Clone, Copy)]
enum ReadTarget {
    Stdout,
    Stderr,
}

impl ReadTarget {
    fn label(self) -> &'static str {
        match self {
            ReadTarget::Stdout => "stdout",
            ReadTarget::Stderr => "stderr",
        }
    }
}

/// `Mutex<ChildStdout>` / `Mutex<ChildStderr>` 共通の二重 read 検出付き
/// 読み出し関数。
///
/// `try_lock` で busy 判定し、既に他タスクが読み中なら `AlreadyReading`
/// を返す。EOF（read 結果 0 バイト）は `Ok(None)` で表現する。
async fn read_via_mutex<R>(
    handle: u32,
    stream: Option<&Mutex<R>>,
    label: &'static str,
    buf: &mut [u8],
) -> Result<Option<usize>, ProcessPoolError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mutex = stream.ok_or(ProcessPoolError::StreamNotPiped {
        handle,
        stream: label,
    })?;
    let mut guard = mutex.try_lock().map_err(|error| match error {
        TryLockError { .. } => {
            log::debug!(
                "[process_pool] read_{label} rejected (already reading): handle={handle}"
            );
            ProcessPoolError::AlreadyReading {
                handle,
                stream: label,
            }
        }
    })?;
    let n = guard.read(buf).await.map_err(|error| {
        log::debug!(
            "[process_pool] {label} read failed: handle={handle}, error={error}"
        );
        ProcessPoolError::Io {
            context: "stream read",
            detail: error.to_string(),
        }
    })?;
    if n == 0 {
        log::debug!("[process_pool] read_{label} reached EOF: handle={handle}");
        return Ok(None);
    }
    log::trace!("[process_pool] read_{label} ok: handle={handle}, bytes={n}");
    Ok(Some(n))
}

/// `ExitStatus` から exit コードを取り出すヘルパ。
///
/// シグナル終了を `128 + signal` の慣例で表現する（Unix 標準 shell の
/// 表現と一致）。Windows では原則 `code()` がそのまま返るので signal
/// パスは到達しない。
fn exit_code_from(status: &std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    -1
}

/// `StdioMode` を `std::process::Stdio` に変換するヘルパ。
fn stdio_for(mode: StdioMode) -> Stdio {
    match mode {
        StdioMode::Inherit => Stdio::inherit(),
        StdioMode::Null => Stdio::null(),
        StdioMode::Piped => Stdio::piped(),
    }
}

#[cfg(test)]
mod tests {
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
        let h2 = pool
            .spawn(cat_spec())
            .await
            .expect("second spawn succeeds");

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
        let read_result = tokio::time::timeout(
            Duration::from_secs(2),
            pool.read_stdout(handle, &mut buf),
        )
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
            args: vec![
                "-c".to_string(),
                "printf out; printf err 1>&2".to_string(),
            ],
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
        let n_a = tokio::time::timeout(
            Duration::from_secs(2),
            pool.read_stdout(h_a, &mut buf_a),
        )
        .await
        .expect("read a within 2s")
        .expect("read a ok")
        .expect("read a returns Some");
        assert_eq!(&buf_a[..n_a], payload_a);

        let mut buf_b = [0u8; 64];
        let n_b = tokio::time::timeout(
            Duration::from_secs(2),
            pool.read_stdout(h_b, &mut buf_b),
        )
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
        let handle = pool
            .spawn(true_spec())
            .await
            .expect("true command spawns");

        let mut buf = [0u8; 64];
        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            pool.read_stdout(handle, &mut buf),
        )
        .await
        .expect("read_stdout completes within 2s")
        .expect("read_stdout returns Ok");
        assert_eq!(
            outcome, None,
            "expected EOF (None) after the child exited"
        );
    }
}
