//! 統合テスト: ADR 0006 入力単一パイプラインの「実バイナリ end-to-end」回帰。
//!
//! 背景:
//! ADR 0006 の本丸 `gg` 不具合は、`g` 始まりの normal keymap が登録されている
//! ときだけ顕在化した（prefix 握り潰しで 1 打ズレる歴史的欠陥）。修正は
//! `resolve_pipeline_command_buffered`（`crates/saya/src/app/runtime_dispatch/command.rs`）
//! に集約され、本番対話ループ（`main.rs` の入力解決）はこの関数を呼ぶだけの
//! 薄い層になっている。
//!
//! 既存のミラーテスト（`integration_input_pipeline.rs`）は本番ループと同一の
//! 呼び出し形・classify・越境規約を「再現したハーネス」で検証するが、実 `sy`
//! バイナリの入力ループ配線そのものは未カバーだった。このテストは実 `sy` を
//! PTY 上で対話起動し、実入力ループ（`resolve_pipeline_command_buffered` 経路）
//! を通してキーストロークを流し、描画されたカーソル行から `gg` の先頭行ジャンプ
//! を黒箱で検証する。
//!
//! 観測トークン:
//! - vt100 エミュレータで PTY 出力を再構成し、`cursor_position()` の row を読む。
//!   行番号ガター（`number` オプション）を有効にしておくと本文が `   N body`
//!   形式で描画され、カーソル行の特定が安定する。
//!
//! 待機戦略:
//! - 固定 sleep に依存せず、「期待状態が現れるまで PTY 出力を poll（タイムアウト
//!   付き）」で待つ。各 poll に締め切りを設け、ハングを防ぐ。
//!
//! hermetic:
//! - 実 `~/.config/saya/init.ts` を読ませない。`-u <temp init.ts>`（ConfigSource::File）
//!   で temp 設定を注入し、`HOME` / `XDG_CONFIG_HOME` / `XDG_CACHE_HOME` を temp に
//!   向ける（親プロセス env は触らず、spawn する子プロセスの env のみ設定する）。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

const PTY_ROWS: u16 = 24;
const PTY_COLS: u16 = 80;

/// PTY 上で対話起動した `sy` を駆動し、画面を vt100 で再構成して読み取るハーネス。
struct SyPtySession {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    parser: vt100::Parser,
    _master: Box<dyn MasterPty + Send>,
}

impl SyPtySession {
    /// temp init.ts と target ファイルで `sy` を PTY 起動する。
    fn spawn(config_path: &Path, target_path: &Path, home_dir: &Path, xdg_config: &Path, xdg_cache: &Path) -> Self {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: PTY_ROWS,
                cols: PTY_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty should succeed");

        let mut command = CommandBuilder::new(sy_binary_path());
        // raw-mode TUI のため端末種別を明示する。
        command.env("TERM", "xterm-256color");
        // 実ホームを汚さず読まないよう、子プロセスの env のみ temp へ向ける。
        command.env("HOME", path_str(home_dir));
        command.env("XDG_CONFIG_HOME", path_str(xdg_config));
        command.env("XDG_CACHE_HOME", path_str(xdg_cache));
        command.env("SAYA_LOG", "0");
        command.arg("-u");
        command.arg(path_str(config_path));
        command.arg(path_str(target_path));

        let child = pair
            .slave
            .spawn_command(command)
            .expect("sy should spawn on the PTY slave");
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .expect("PTY reader clone should succeed");
        let writer = pair
            .master
            .take_writer()
            .expect("PTY writer should be available");

        let (sender, output) = channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if sender.send(buffer[..read].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            child,
            writer,
            output,
            parser: vt100::Parser::new(PTY_ROWS, PTY_COLS, 0),
            _master: pair.master,
        }
    }

    /// 受信済み PTY バイト列を vt100 パーサへ取り込む（ノンブロッキング寄り）。
    fn pump(&mut self, budget: Duration) {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            match self.output.recv_timeout(Duration::from_millis(40)) {
                Ok(chunk) => self.parser.process(&chunk),
                Err(_) => break,
            }
        }
    }

    /// 述語が真になるまで PTY 出力を poll する。タイムアウトで `false`。
    fn wait_until<F>(&mut self, timeout: Duration, mut predicate: F) -> bool
    where
        F: FnMut(&vt100::Parser) -> bool,
    {
        let deadline = Instant::now() + timeout;
        loop {
            if predicate(&self.parser) {
                return true;
            }
            if Instant::now() >= deadline {
                return predicate(&self.parser);
            }
            self.pump(Duration::from_millis(80));
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("PTY write should succeed");
        self.writer.flush().expect("PTY flush should succeed");
    }

    fn cursor_row(&self) -> u16 {
        self.parser.screen().cursor_position().0
    }

    /// メッセージ領域に指定文字列が含まれるか（pager 表示の検出などに使う）。
    fn screen_contains(&self, needle: &str) -> bool {
        screen_text(&self.parser).contains(needle)
    }
}

/// 先頭行が行番号ガター付きで描画されているか（編集画面到達の判定）。
fn editor_screen_ready(parser: &vt100::Parser) -> bool {
    screen_text(parser).contains("   1 line01")
}

impl Drop for SyPtySession {
    fn drop(&mut self) {
        // 行儀よく終了させ、失敗してもテストプロセスに残骸を残さない。
        let _ = self.writer.write_all(b"\x1b:q!\r");
        let _ = self.writer.flush();
        std::thread::sleep(Duration::from_millis(50));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("temp path should be valid UTF-8")
}

fn screen_text(parser: &vt100::Parser) -> String {
    let screen = parser.screen();
    let mut text = String::new();
    for row in 0..PTY_ROWS {
        for col in 0..PTY_COLS {
            if let Some(cell) = screen.cell(row, col) {
                let contents = cell.contents();
                if contents.is_empty() {
                    text.push(' ');
                } else {
                    text.push_str(&contents);
                }
            }
        }
        text.push('\n');
    }
    text
}

fn sy_binary_path() -> PathBuf {
    let mut path = std::env::current_exe().expect("test binary path should exist");
    path.pop();
    path.pop();
    path.push("sy");
    path
}

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be monotonic")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-input-e2e-{name}-{nanos}"))
}

/// temp 設定一式（init.ts / target / hermetic ホーム群）を組み立てる。
struct E2eFixture {
    config_path: PathBuf,
    target_path: PathBuf,
    home_dir: PathBuf,
    xdg_config: PathBuf,
    xdg_cache: PathBuf,
}

impl E2eFixture {
    fn new() -> Self {
        let config_path = unique_path("init.ts");
        let target_path = unique_path("target.txt");
        let home_dir = unique_path("home");
        let xdg_config = unique_path("xdg-config");
        let xdg_cache = unique_path("xdg-cache");
        std::fs::create_dir_all(&home_dir).expect("temp home should be created");
        std::fs::create_dir_all(&xdg_config).expect("temp xdg config should be created");
        std::fs::create_dir_all(&xdg_cache).expect("temp xdg cache should be created");

        // 10 行のファイル。`gg` が確実に先頭行へ「戻る」観測のため複数行にする。
        let body: String = (1..=10).map(|n| format!("line{n:02}\n")).collect();
        std::fs::write(&target_path, body).expect("target file should be created");

        // `g` 始まり normal keymap（`gd`）を registered command 付きで登録する。
        // この mapping が存在するときだけ `gg` 不具合が顕在化したため、再現条件。
        std::fs::write(
            &config_path,
            r#"
            saya.options.number = true;
            saya.commands.register("e2eGoDefinition", () => {
                saya.commands.execute("write");
            });
            saya.keymap.set("normal", "gd", saya.commands.execute("e2eGoDefinition"));
            "#,
        )
        .expect("init.ts should be created");

        Self {
            config_path,
            target_path,
            home_dir,
            xdg_config,
            xdg_cache,
        }
    }

    fn spawn(&self) -> SyPtySession {
        SyPtySession::spawn(
            &self.config_path,
            &self.target_path,
            &self.home_dir,
            &self.xdg_config,
            &self.xdg_cache,
        )
    }
}

impl Drop for E2eFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.config_path);
        let _ = std::fs::remove_file(&self.target_path);
        let _ = std::fs::remove_dir_all(&self.home_dir);
        let _ = std::fs::remove_dir_all(&self.xdg_config);
        let _ = std::fs::remove_dir_all(&self.xdg_cache);
    }
}

/// `g` 始まり keymap 登録下で、実 `sy` の入力ループを通した `g`,`g` が
/// 先頭行（cursor row 0）へ戻ることを黒箱で検証する。
#[test]
fn gg_jumps_to_first_line_through_the_sy_binary_with_g_prefixed_keymap() {
    let fixture = E2eFixture::new();
    let mut session = fixture.spawn();

    // 編集画面が描画されるまで待つ（起動はやや重いので余裕を持つ）。
    assert!(
        session.wait_until(Duration::from_secs(12), editor_screen_ready),
        "editor screen with line-number gutter should render on startup"
    );

    // hermetic ホームには plugin cache が無く、起動時に長文の警告メッセージが
    // 出て message pager（HitReturn）に入る。Enter で確実に閉じてから編集に入る。
    if session.screen_contains("[pager") {
        session.send(b"\r");
        assert!(
            session.wait_until(Duration::from_secs(5), |parser| {
                !screen_text(parser).contains("[pager")
            }),
            "startup message pager should be dismissed by Enter before editing"
        );
    }

    // 先頭行以外へ移動（前提: row != 0）。`G` で最終行（10 行目 = row 9）へ。
    session.send(b"G");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            parser.screen().cursor_position().0 != 0
        }),
        "G should move the cursor off the first line, got row {}",
        session.cursor_row()
    );
    let row_after_g = session.cursor_row();
    assert_ne!(row_after_g, 0, "precondition: cursor must be off row 0 before gg");

    // 本丸: `g`,`g` で先頭行（row 0）へ戻る。
    session.send(b"gg");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            parser.screen().cursor_position().0 == 0
        }),
        "gg should return the cursor to the first line (row 0), got row {} (row after G was {})",
        session.cursor_row(),
        row_after_g
    );

    // 後始末は Drop（:q!）に委ねる。
}
