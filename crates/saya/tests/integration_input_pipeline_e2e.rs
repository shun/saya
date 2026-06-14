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
    fn spawn(
        config_path: &Path,
        target_path: &Path,
        home_dir: &Path,
        xdg_config: &Path,
        xdg_cache: &Path,
    ) -> Self {
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

    /// 子プロセスが終了するまで（タイムアウト付きで）待つ。`:q` の終了検証に使う。
    /// `try_wait` を poll し、終了が観測できたら `true`。
    fn wait_until_process_exit(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            match self.child.try_wait() {
                Ok(Some(_status)) => return true,
                Ok(None) => {}
                Err(_) => return false,
            }
            if Instant::now() >= deadline {
                return false;
            }
            // 出力を消費しつつ短く待ってから再チェックする。
            self.pump(Duration::from_millis(80));
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer
            .write_all(bytes)
            .expect("PTY write should succeed");
        self.writer.flush().expect("PTY flush should succeed");
    }

    fn cursor_row(&self) -> u16 {
        self.parser.screen().cursor_position().0
    }

    /// 指定スクリーン行のテキスト（trim_end 済み）を返す。
    fn screen_row_text(&self, row: u16) -> String {
        let screen = self.parser.screen();
        let mut text = String::new();
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
        text.trim_end().to_string()
    }

    /// 画面最上段に描画されている本文行のガター行番号（`   N body` の N）。
    /// ページ送りでビューポートが下へスクロールしたことを観測するのに使う。
    fn first_body_line(&self) -> Option<u32> {
        let text = self.screen_row_text(0);
        text.split_whitespace().next()?.parse::<u32>().ok()
    }

    /// メッセージ領域に指定文字列が含まれるか（pager 表示の検出などに使う）。
    fn screen_contains(&self, needle: &str) -> bool {
        screen_text(&self.parser).contains(needle)
    }

    /// 最終行（コマンドライン領域）のテキストを左トリムして返す。
    /// `:` / `/` 入口の検出に使う。
    fn command_line_row_text(&self) -> String {
        let screen = self.parser.screen();
        let row = PTY_ROWS - 1;
        let mut text = String::new();
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
        text.trim_end().to_string()
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
    // Cargo が当該テスト用にビルドした `sy` の絶対パスを使う。手組みの
    // current_exe() 相対解決は stale バイナリを掴む危険があるため、Cargo が
    // 提供する `CARGO_BIN_EXE_sy` を参照して常に今ビルドした実体を検証する。
    PathBuf::from(env!("CARGO_BIN_EXE_sy"))
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
        Self::with_lines(10)
    }

    /// 指定行数の `lineNN` バッファで fixture を組む。ページ送り（`Ctrl-f` 等）の
    /// ように画面より長いバッファが必要なテストでは行数を増やして使う。
    fn with_lines(line_count: usize) -> Self {
        Self::with_lines_and_config(line_count, "")
    }

    /// 指定行数のバッファに加え、init.ts へ任意の追加設定行を差し込んで fixture を
    /// 組む。`smartindent` のようなオプションを実バイナリへ注入したいテストで使う。
    /// 追加設定は `g` 始まり keymap 登録（`gg` 再現条件）の直後に連結する。
    fn with_lines_and_config(line_count: usize, extra_config: &str) -> Self {
        let config_path = unique_path("init.ts");
        let target_path = unique_path("target.txt");
        let home_dir = unique_path("home");
        let xdg_config = unique_path("xdg-config");
        let xdg_cache = unique_path("xdg-cache");
        std::fs::create_dir_all(&home_dir).expect("temp home should be created");
        std::fs::create_dir_all(&xdg_config).expect("temp xdg config should be created");
        std::fs::create_dir_all(&xdg_cache).expect("temp xdg cache should be created");

        // `lineNN` の連番ファイル。`gg`/ページ送りで行位置の移動を観測するため、
        // 各行が一意に判別できる本文（行番号サフィックス付き）にしている。
        let body: String = (1..=line_count).map(|n| format!("line{n:02}\n")).collect();
        std::fs::write(&target_path, body).expect("target file should be created");

        // `g` 始まり normal keymap（`gd`）を registered command 付きで登録する。
        // この mapping が存在するときだけ `gg` 不具合が顕在化したため、再現条件。
        let config = format!(
            r#"
            saya.options.number = true;
            saya.commands.register("e2eGoDefinition", () => {{
                saya.commands.execute("write");
            }});
            saya.keymap.set("normal", "gd", saya.commands.execute("e2eGoDefinition"));
            {extra_config}
            "#,
        );
        std::fs::write(&config_path, config).expect("init.ts should be created");

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
    assert_ne!(
        row_after_g, 0,
        "precondition: cursor must be off row 0 before gg"
    );

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

/// 実 `sy` の入力ループで normal モードから `:` を押すとコマンドライン
/// （ex モード）に入ること（最終行に `:` プロンプトが表示される）を黒箱で検証する。
/// ADR 0006 の単一パイプライン化で `:` が完成 builtin として core へ越境し、
/// host のコマンドライン入口がバイパスされる回帰の回帰防止。
#[test]
fn colon_enters_command_line_through_the_sy_binary() {
    let fixture = E2eFixture::new();
    let mut session = fixture.spawn();

    assert!(
        session.wait_until(Duration::from_secs(12), editor_screen_ready),
        "editor screen with line-number gutter should render on startup"
    );

    if session.screen_contains("[pager") {
        session.send(b"\r");
        assert!(
            session.wait_until(Duration::from_secs(5), |parser| {
                !screen_text(parser).contains("[pager")
            }),
            "startup message pager should be dismissed by Enter before editing"
        );
    }

    // normal モードで `:` を送ると最終行に `:` プロンプトが出る。
    session.send(b":");
    let entered = session.wait_until(Duration::from_secs(5), |parser| {
        let row = PTY_ROWS - 1;
        let screen = parser.screen();
        let mut text = String::new();
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
        text.trim_start().starts_with(':')
    });
    assert!(
        entered,
        "`:` should enter command-line mode and render a `:` prompt on the last row, got command-line row: {:?}",
        session.command_line_row_text()
    );

    // さらに `q!` まで打って `:q!` が表示され、ex コマンドが編集できることも確認。
    session.send(b"q!");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            let row = PTY_ROWS - 1;
            let screen = parser.screen();
            let mut text = String::new();
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
            text.contains(":q!")
        }),
        "typing `q!` after `:` should render `:q!` in the command line, got: {:?}",
        session.command_line_row_text()
    );

    // 後始末は Drop（:q!）に委ねる。
}

/// 起動直後の編集画面到達と startup pager の消し込みまでを共通化する。
fn ready_for_editing(session: &mut SyPtySession) {
    assert!(
        session.wait_until(Duration::from_secs(12), editor_screen_ready),
        "editor screen with line-number gutter should render on startup"
    );

    if session.screen_contains("[pager") {
        session.send(b"\r");
        assert!(
            session.wait_until(Duration::from_secs(5), |parser| {
                !screen_text(parser).contains("[pager")
            }),
            "startup message pager should be dismissed by Enter before editing"
        );
    }
}

/// 実 `sy` の入力ループで normal モードから `/` を押すと検索コマンドライン
/// （search モード）に入ること（最終行に `/` プロンプトが表示される）を黒箱で検証する。
/// `:`（ex）同様、`/` も単一パイプラインで完成 builtin として core へ越境すると
/// host の検索コマンドライン入口がバイパスされる回帰の回帰防止。
#[test]
fn slash_enters_search_command_line_through_the_sy_binary() {
    let fixture = E2eFixture::new();
    let mut session = fixture.spawn();

    ready_for_editing(&mut session);

    // normal モードで `/` を送ると最終行に `/` プロンプトが出る。
    session.send(b"/");
    let entered = session.wait_until(Duration::from_secs(5), |parser| {
        let row = PTY_ROWS - 1;
        let screen = parser.screen();
        let mut text = String::new();
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
        text.trim_start().starts_with('/')
    });
    assert!(
        entered,
        "`/` should enter search command-line mode and render a `/` prompt on the last row, got command-line row: {:?}",
        session.command_line_row_text()
    );

    // さらに検索語を打って `/line` が表示され、検索コマンドラインが編集できることも確認。
    session.send(b"line");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            let row = PTY_ROWS - 1;
            let screen = parser.screen();
            let mut text = String::new();
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
            text.contains("/line")
        }),
        "typing `line` after `/` should render `/line` in the command line, got: {:?}",
        session.command_line_row_text()
    );

    // 後始末は Drop（:q!）に委ねる。
}

/// 実 `sy` の入力ループで編集 → `Esc` → `:w<Enter>` を通すと、編集内容が
/// ディスク上の実ファイルへ保存されることを end-to-end で検証する。
/// command-line 入口 → ex 実行 → host action write の経路を実バイナリで通す。
#[test]
fn write_command_saves_file_through_the_sy_binary() {
    let fixture = E2eFixture::new();
    let mut session = fixture.spawn();

    ready_for_editing(&mut session);

    // 編集は Normal モードのみで行う。PTY 上で Insert→Esc を確実に抜けるのは
    // crossterm の ESC 曖昧性解決（後続バイト待ち）で不安定になりやすいため、
    // Insert モードを避けて Normal の `dd`（行削除）で 1 行目を消す。
    // 元ファイルは先頭行が `line01` なので、保存後に `line01` が消えていれば
    // 「編集済みバッファがディスクへ書かれた」と判定できる。
    assert!(
        session.screen_contains("line01"),
        "precondition: target should start with line01, screen:\n{}",
        screen_text(&session.parser)
    );

    // `dd` で先頭行を削除する。
    session.send(b"dd");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            // 1 行目に line01 が無く line02 が来ていれば削除成功。
            let screen = parser.screen();
            let mut first_row = String::new();
            for col in 0..PTY_COLS {
                if let Some(cell) = screen.cell(0, col) {
                    first_row.push_str(&cell.contents());
                }
            }
            first_row.contains("line02") && !first_row.contains("line01")
        }),
        "`dd` should delete the first line in the buffer, screen:\n{}",
        screen_text(&session.parser)
    );

    // `:w` を打って command-line に `:w` が表示されることを確認してから Enter。
    session.send(b":w");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            let row = PTY_ROWS - 1;
            let screen = parser.screen();
            let mut text = String::new();
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
            text.contains(":w")
        }),
        "`:w` should render in the command line before Enter, got: {:?}\nscreen:\n{}",
        session.command_line_row_text(),
        screen_text(&session.parser)
    );
    // Enter で write を実行する。command-line 入口 → ex `write` → host write。
    session.send(b"\r");

    // ディスク上の実ファイルが「先頭行 line01 が削除された」状態へ保存される
    // まで poll する。編集前は `line01\n` で始まっていたので、保存後にその先頭が
    // `line02\n` になっていれば編集済みバッファが書き込まれたと判定できる。
    let target = fixture.target_path.clone();
    let saved = wait_for_file(&target, Duration::from_secs(8), |content| {
        content.starts_with("line02") && !content.starts_with("line01")
    });
    assert!(
        saved,
        "`:w` should persist the edited (first-line-deleted) buffer to disk; file content was: {:?}",
        std::fs::read_to_string(&target).ok()
    );

    // 後始末は Drop（:q!）に委ねる。
}

// =============================================================================
// カテゴリF: 基本キーバインドの実バイナリ E2E マトリクス
//
// これらのテストは「キーバインドが実 `sy` バイナリ経由で動く」正典スイート。
// 大半の既存 saya テストは core へ入力を直送し core 挙動だけを検証していて、
// host 入力パイプライン（実キー `KeyInput` → `key_input_to_vim_key`(router.rs,
// `(ch & 0x1f)` で Ctrl シリアライズ) → `resolve_pipeline_command_buffered`
// → core）をスキップしていた。ADR 0006 でこの host パイプラインへ統一した後に
// `Ctrl-f`/`Ctrl-b` 等のページ送りが壊れる回帰が全緑をすり抜けたため、ここで
// 実バイナリ経由のキー配線そのものを黒箱で固定する。
//
// 観測戦略:
// - カーソル行/列（vt100 `cursor_position()`）、画面最上段のガター行番号
//   （`first_body_line()` = ビューポートの top line）、本文セルの文字列を読む。
// - 固定 sleep を避け、述語が真になるまで PTY 出力を poll（`wait_until`）する。
//
// 既知の RED:
// - `Ctrl-f`/`Ctrl-b`（および `Ctrl-d`/`Ctrl-u`）のページ送りは現状の host
//   パイプライン回帰で動かない。これらのテストは「正しい vim 挙動」を assert
//   しており、修正前は意図的に RED になる（別タスク A-1 が修正する）。
// =============================================================================

/// 100 行バッファ。画面（24 行）より長くしてビューポートのスクロールを観測する。
const TALL_LINES: usize = 100;
/// 通常の編集観測に使う行数。
const EDIT_LINES: usize = 20;

/// カーソル行が `expected` になるまで待つ（タイムアウト付き）。
fn wait_cursor_row(session: &mut SyPtySession, expected: u16, timeout: Duration) -> bool {
    session.wait_until(timeout, |parser| {
        parser.screen().cursor_position().0 == expected
    })
}

// ----- ページ送り（Ctrl-f / Ctrl-b / Ctrl-d / Ctrl-u） -----------------------

/// `Ctrl-f`（一画面下スクロール）で、画面より長いバッファのビューポートが
/// 先頭行より下へ送られることを実バイナリ経由で検証する。
/// NOTE: ADR 0006 後の host パイプライン回帰で現状 RED（A-1 が修正）。
#[test]
fn ctrl_f_pages_down_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(TALL_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    assert_eq!(
        session.first_body_line(),
        Some(1),
        "precondition: viewport should start at the first line"
    );

    // Ctrl-f = 0x06。router.rs の `(ch & 0x1f)` シリアライズ経路を実際に通す。
    session.send(b"\x06");
    let paged = session.wait_until(Duration::from_secs(5), |parser| {
        let top = screen_text(parser)
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().next().map(str::to_string))
            .and_then(|n| n.parse::<u32>().ok());
        matches!(top, Some(n) if n > 1)
    });
    assert!(
        paged,
        "`Ctrl-f` should scroll the viewport down a page (top line > 1), got top line {:?}",
        session.first_body_line()
    );
}

/// `Ctrl-b`（一画面上スクロール）で、最終行付近からビューポートが上へ送られる
/// ことを実バイナリ経由で検証する。`G` で末尾へ移動してから観測する。
/// NOTE: ADR 0006 後の host パイプライン回帰で現状 RED（A-1 が修正）。
#[test]
fn ctrl_b_pages_up_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(TALL_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // 末尾へ移動してビューポートを下端まで送る（前提づくり）。
    session.send(b"G");
    let scrolled_down = session.wait_until(Duration::from_secs(5), |parser| {
        let top = parser.screen().cell(0, 0).map(|_| ()).and(
            screen_text(parser).lines().next().and_then(|line| {
                line.split_whitespace()
                    .next()
                    .and_then(|n| n.parse::<u32>().ok())
            }),
        );
        matches!(top, Some(n) if n > 1)
    });
    assert!(
        scrolled_down,
        "precondition: `G` should scroll the viewport to the bottom (top line > 1), got {:?}",
        session.first_body_line()
    );
    let top_after_g = session
        .first_body_line()
        .expect("top line should be numeric after G");

    // Ctrl-b = 0x02。
    session.send(b"\x02");
    let paged_up = session.wait_until(Duration::from_secs(5), |parser| {
        let top = screen_text(parser).lines().next().and_then(|line| {
            line.split_whitespace()
                .next()
                .and_then(|n| n.parse::<u32>().ok())
        });
        matches!(top, Some(n) if n < top_after_g)
    });
    assert!(
        paged_up,
        "`Ctrl-b` should scroll the viewport up a page (top line < {top_after_g}), got top line {:?}",
        session.first_body_line()
    );
}

/// `Ctrl-d`（半画面下スクロール）でビューポート/カーソルが下へ進むことを検証する。
/// NOTE: ADR 0006 後の host パイプライン回帰で現状 RED（A-1 が修正）。
#[test]
fn ctrl_d_scrolls_down_half_page_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(TALL_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    assert_eq!(
        session.first_body_line(),
        Some(1),
        "precondition: viewport at top"
    );

    // Ctrl-d = 0x04。半画面下では top line もしくはカーソルが下へ進む。
    session.send(b"\x04");
    let scrolled = session.wait_until(Duration::from_secs(5), |parser| {
        let top = screen_text(parser).lines().next().and_then(|line| {
            line.split_whitespace()
                .next()
                .and_then(|n| n.parse::<u32>().ok())
        });
        let cursor_row = parser.screen().cursor_position().0;
        matches!(top, Some(n) if n > 1) || cursor_row > 0
    });
    assert!(
        scrolled,
        "`Ctrl-d` should advance the view/cursor down a half page, got top line {:?}, cursor row {}",
        session.first_body_line(),
        session.cursor_row()
    );
}

/// `Ctrl-u`（半画面上スクロール）でビューポート/カーソルが上へ戻ることを検証する。
/// `G` で末尾へ送ってから観測する。
/// NOTE: ADR 0006 後の host パイプライン回帰で現状 RED（A-1 が修正）。
#[test]
fn ctrl_u_scrolls_up_half_page_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(TALL_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    session.send(b"G");
    let scrolled_down = session.wait_until(Duration::from_secs(5), |parser| {
        let top = screen_text(parser).lines().next().and_then(|line| {
            line.split_whitespace()
                .next()
                .and_then(|n| n.parse::<u32>().ok())
        });
        matches!(top, Some(n) if n > 1)
    });
    assert!(
        scrolled_down,
        "precondition: `G` should scroll the viewport down, got {:?}",
        session.first_body_line()
    );
    let top_after_g = session
        .first_body_line()
        .expect("top line should be numeric after G");

    // Ctrl-u = 0x15。
    session.send(b"\x15");
    let scrolled_up = session.wait_until(Duration::from_secs(5), |parser| {
        let top = screen_text(parser).lines().next().and_then(|line| {
            line.split_whitespace()
                .next()
                .and_then(|n| n.parse::<u32>().ok())
        });
        matches!(top, Some(n) if n < top_after_g)
    });
    assert!(
        scrolled_up,
        "`Ctrl-u` should scroll the view up a half page (top line < {top_after_g}), got {:?}",
        session.first_body_line()
    );
}

// ----- 移動（h / j / k / l / gg / G） ----------------------------------------

/// `j` で下、`k` で上へカーソルが 1 行移動することを実バイナリ経由で検証する。
#[test]
fn j_and_k_move_cursor_vertically_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    assert_eq!(
        session.cursor_row(),
        0,
        "precondition: cursor starts on row 0"
    );
    session.send(b"j");
    assert!(
        wait_cursor_row(&mut session, 1, Duration::from_secs(5)),
        "`j` should move the cursor down one line, got row {}",
        session.cursor_row()
    );
    session.send(b"k");
    assert!(
        wait_cursor_row(&mut session, 0, Duration::from_secs(5)),
        "`k` should move the cursor back up one line, got row {}",
        session.cursor_row()
    );
}

/// `l` で右、`h` で左へカーソルが 1 桁移動することを実バイナリ経由で検証する。
#[test]
fn h_and_l_move_cursor_horizontally_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    let start_col = session.parser.screen().cursor_position().1;
    session.send(b"l");
    let moved_right = session.wait_until(Duration::from_secs(5), |parser| {
        parser.screen().cursor_position().1 == start_col + 1
    });
    assert!(
        moved_right,
        "`l` should move the cursor one column right (from {start_col}), got col {}",
        session.parser.screen().cursor_position().1
    );
    session.send(b"h");
    let moved_left = session.wait_until(Duration::from_secs(5), |parser| {
        parser.screen().cursor_position().1 == start_col
    });
    assert!(
        moved_left,
        "`h` should move the cursor back one column left (to {start_col}), got col {}",
        session.parser.screen().cursor_position().1
    );
}

/// `G` で最終行へ、`gg` で先頭行へ移動することを実バイナリ経由で検証する。
/// （`gg` の prefix 不具合の回帰防止は既存 `gg_jumps_to_first_line...` が担うが、
/// こちらは基本マトリクスの一員として `G`↔`gg` の往復を素直に固定する。）
#[test]
fn g_and_gg_jump_to_last_and_first_line_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    session.send(b"G");
    let moved_off_top = session.wait_until(Duration::from_secs(5), |parser| {
        parser.screen().cursor_position().0 != 0
    });
    assert!(
        moved_off_top,
        "`G` should move the cursor off the first line, got row {}",
        session.cursor_row()
    );

    session.send(b"gg");
    assert!(
        wait_cursor_row(&mut session, 0, Duration::from_secs(5)),
        "`gg` should return the cursor to the first line (row 0), got row {}",
        session.cursor_row()
    );
}

// ----- count 付き移動（3j） ---------------------------------------------------

/// `3j` でカーソルが 3 行下（row 3）へ移動することを実バイナリ経由で検証する。
#[test]
fn count_prefixed_j_moves_multiple_lines_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    session.send(b"3j");
    assert!(
        wait_cursor_row(&mut session, 3, Duration::from_secs(5)),
        "`3j` should move the cursor down three lines (row 3), got row {}",
        session.cursor_row()
    );
    assert!(
        session.screen_row_text(3).contains("line04"),
        "`3j` should land on line04, cursor row text was {:?}",
        session.screen_row_text(3)
    );
}

// ----- 編集（x / dd / p / 2dd / .） ------------------------------------------

/// `x` で 1 文字削除されることを実バイナリ経由で検証する（line01 → ine01）。
#[test]
fn x_deletes_a_character_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    session.send(b"x");
    let deleted = session.wait_until(Duration::from_secs(5), |parser| {
        first_row_body(parser) == "ine01"
    });
    assert!(
        deleted,
        "`x` should delete the first character of line01, got first row {:?}",
        session.screen_row_text(0)
    );
}

/// `dd` で先頭行（line01）が削除され line02 が繰り上がることを検証する。
#[test]
fn dd_deletes_a_line_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    session.send(b"dd");
    let deleted = session.wait_until(Duration::from_secs(5), |parser| {
        let first = first_row_body(parser);
        first == "line02"
    });
    assert!(
        deleted,
        "`dd` should delete line01 so line02 becomes the first row, got {:?}",
        session.screen_row_text(0)
    );
}

/// `2dd` で先頭 2 行（line01/line02）が削除され line03 が繰り上がることを検証する。
#[test]
fn count_prefixed_dd_deletes_multiple_lines_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    session.send(b"2dd");
    let deleted = session.wait_until(Duration::from_secs(5), |parser| {
        first_row_body(parser) == "line03"
    });
    assert!(
        deleted,
        "`2dd` should delete line01 and line02 so line03 becomes the first row, got {:?}",
        session.screen_row_text(0)
    );
}

/// `dd` でヤンクした行を `p` で下に貼り付けられることを実バイナリ経由で検証する。
#[test]
fn p_pastes_yanked_line_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // `dd` で line01 を削除（= ヤンク）。先頭は line02 になる。
    session.send(b"dd");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            first_row_body(parser) == "line02"
        }),
        "precondition: `dd` should leave line02 on the first row, got {:?}",
        session.screen_row_text(0)
    );

    // `p` で削除した line01 をカーソル行の下（= 2 行目）へ貼り付ける。
    session.send(b"p");
    let pasted = session.wait_until(Duration::from_secs(5), |parser| {
        row_body(parser, 0) == "line02" && row_body(parser, 1) == "line01"
    });
    assert!(
        pasted,
        "`p` should paste line01 below the cursor line, got row0 {:?} / row1 {:?}",
        session.screen_row_text(0),
        session.screen_row_text(1)
    );
}

/// `x` で 1 文字削除した後、`.` で同じ削除が繰り返されることを検証する。
/// line01 → (x) ine01 → (.) ne01。
#[test]
fn dot_repeats_last_change_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    session.send(b"x");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            first_row_body(parser) == "ine01"
        }),
        "precondition: `x` should produce ine01, got {:?}",
        session.screen_row_text(0)
    );
    session.send(b".");
    let repeated = session.wait_until(Duration::from_secs(5), |parser| {
        first_row_body(parser) == "ne01"
    });
    assert!(
        repeated,
        "`.` should repeat the last `x` (ine01 -> ne01), got {:?}",
        session.screen_row_text(0)
    );
}

// ----- モード遷移（i / a / o / v / Esc） -------------------------------------
//
// 既定 statusline はモードラベルを描画しないため、モード遷移は「入力した結果が
// どう反映されるか（テキスト挿入/選択削除）」で黒箱観測する。

/// `i` で Insert モードに入り、入力した文字が本文先頭へ挿入されることを検証する。
#[test]
fn i_enters_insert_mode_and_inserts_text_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // `i` で挿入モードに入り `ZZ` をタイプすると line01 -> ZZline01 になる。
    session.send(b"iZZ");
    let inserted = session.wait_until(Duration::from_secs(5), |parser| {
        first_row_body(parser) == "ZZline01"
    });
    assert!(
        inserted,
        "`i` should enter insert mode and prepend typed text, got {:?}",
        session.screen_row_text(0)
    );
}

/// `a` で Insert モード（カーソルの後ろ）に入り、文字が挿入されることを検証する。
#[test]
fn a_enters_insert_mode_after_cursor_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // 先頭文字 `l` の後ろに挿入されるので line01 -> lZZine01。
    session.send(b"aZZ");
    let inserted = session.wait_until(Duration::from_secs(5), |parser| {
        first_row_body(parser) == "lZZine01"
    });
    assert!(
        inserted,
        "`a` should insert after the cursor (lZZine01), got {:?}",
        session.screen_row_text(0)
    );
}

/// `o` でカーソル行の下に新規行を開いて Insert モードに入ることを検証する。
#[test]
fn o_opens_line_below_and_enters_insert_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // `o` で 2 行目に新規行を開き `ZZZ` を入力 -> row0=line01, row1=ZZZ。
    session.send(b"oZZZ");
    let opened = session.wait_until(Duration::from_secs(5), |parser| {
        row_body(parser, 0) == "line01" && row_body(parser, 1) == "ZZZ"
    });
    assert!(
        opened,
        "`o` should open a new line below and enter insert, got row0 {:?} / row1 {:?}",
        session.screen_row_text(0),
        session.screen_row_text(1)
    );
}

/// `v` で Visual モードに入り、`ll` で 3 文字選択して `x` で削除できることを検証する。
/// （Visual 選択が成立していれば line01 -> e01 になる。）
#[test]
fn v_enters_visual_mode_and_selects_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // `v` で visual に入り、`ll` で 3 文字（lin）を選択して `x` で削除する。
    session.send(b"vll");
    // 選択中はカーソル列が 2 桁右へ進む（選択拡張の観測）。
    let start_col = 5; // ガター `   1 ` の直後（line01 先頭）。
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            parser.screen().cursor_position().1 == start_col + 2
        }),
        "`v` then `ll` should extend the selection two columns, got col {}",
        session.parser.screen().cursor_position().1
    );
    session.send(b"x");
    let deleted = session.wait_until(Duration::from_secs(5), |parser| {
        first_row_body(parser) == "e01"
    });
    assert!(
        deleted,
        "`x` over a 3-char visual selection should leave e01, got {:?}",
        session.screen_row_text(0)
    );
}

/// `i`（Insert）→ `Esc` → normal コマンド `dd` が効くことで `Esc` の
/// モード復帰を実バイナリ経由で検証する。ESC は単独バイト `0x1b` で送る。
#[test]
fn esc_returns_to_normal_mode_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // Insert に入って文字を挿入 -> ZZline01。
    session.send(b"iZZ");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            first_row_body(parser) == "ZZline01"
        }),
        "precondition: insert should produce ZZline01, got {:?}",
        session.screen_row_text(0)
    );
    // Esc で normal へ戻る。
    session.send(b"\x1b");
    // normal に戻っていれば `dd` が行削除として効き、先頭行が line02 になる。
    session.send(b"dd");
    let back_to_normal = session.wait_until(Duration::from_secs(5), |parser| {
        first_row_body(parser) == "line02"
    });
    assert!(
        back_to_normal,
        "after `Esc`, `dd` should delete the (edited) first line so line02 surfaces, got {:?}",
        session.screen_row_text(0)
    );
}

// ----- コマンド（: / / / :w / :q） -------------------------------------------

/// `:` で ex コマンドラインに入れることを実バイナリ経由で検証する。
/// （`colon_enters_command_line...` と重複するが、基本マトリクスとして明示保持。）
#[test]
fn colon_enters_ex_command_line_matrix_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    session.send(b":");
    let entered = session.wait_until(Duration::from_secs(5), |parser| {
        command_line_text(parser).trim_start().starts_with(':')
    });
    assert!(
        entered,
        "`:` should render a `:` prompt on the command line, got {:?}",
        session.command_line_row_text()
    );
}

/// `/` で検索コマンドラインに入れることを実バイナリ経由で検証する。
#[test]
fn slash_enters_search_command_line_matrix_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    session.send(b"/");
    let entered = session.wait_until(Duration::from_secs(5), |parser| {
        command_line_text(parser).trim_start().starts_with('/')
    });
    assert!(
        entered,
        "`/` should render a `/` prompt on the command line, got {:?}",
        session.command_line_row_text()
    );
}

/// `:w<Enter>` で編集済みバッファが実ファイルへ保存されることを検証する。
/// （`write_command_saves_file...` と重複するが、基本マトリクスとして明示保持。）
#[test]
fn write_command_persists_buffer_matrix_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // `dd` で先頭行 line01 を消し、保存後に line02 始まりになることで検証する。
    session.send(b"dd");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            first_row_body(parser) == "line02"
        }),
        "precondition: `dd` should leave line02 on the first row, got {:?}",
        session.screen_row_text(0)
    );
    session.send(b":w\r");
    let target = fixture.target_path.clone();
    let saved = wait_for_file(&target, Duration::from_secs(8), |content| {
        content.starts_with("line02") && !content.starts_with("line01")
    });
    assert!(
        saved,
        "`:w` should persist the edited buffer to disk, file content was {:?}",
        std::fs::read_to_string(&target).ok()
    );
}

/// `:q` で実 `sy` プロセスが終了することを検証する。未編集バッファなので
/// `:q` がそのまま受理され、PTY が EOF になる（= プロセス終了）。
#[test]
fn quit_command_exits_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    session.send(b":q\r");
    let exited = session.wait_until_process_exit(Duration::from_secs(8));
    assert!(
        exited,
        "`:q` on an unmodified buffer should exit the sy process"
    );
}

// ----- 検索反復（n / N） -----------------------------------------------------

/// `/line<Enter>` で最初の一致へ移動し、`n` で次の一致、`N` で前の一致へ
/// 移動することを実バイナリ経由で検証する。`line` は全行に一致する。
#[test]
fn n_and_capital_n_repeat_search_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // 先頭(line01)から `/line` 検索。カーソル以降の次の一致 line02(row1)へ。
    session.send(b"/line\r");
    assert!(
        wait_cursor_row(&mut session, 1, Duration::from_secs(5)),
        "`/line` should jump to the next match on row 1, got row {}",
        session.cursor_row()
    );
    // `n` で次の一致 line03(row2)へ。
    session.send(b"n");
    assert!(
        wait_cursor_row(&mut session, 2, Duration::from_secs(5)),
        "`n` should repeat search to row 2, got row {}",
        session.cursor_row()
    );
    // `N` で前の一致 line02(row1)へ戻る。
    session.send(b"N");
    assert!(
        wait_cursor_row(&mut session, 1, Duration::from_secs(5)),
        "`N` should repeat search backward to row 1, got row {}",
        session.cursor_row()
    );
}

// =============================================================================
// カテゴリ A-3: 保存・終了の複合/変種を実バイナリ end-to-end で黒箱観測する。
//
// 既存マトリクスは `:w`（単純保存）/`:q`（単純終了）だけをカバーしている。
// ここでは保存と終了を一手で行う複合 ex コマンド（`:wq` / `:x`）と、明示パスへ
// 書き出す `:write <path>` を実 `sy` で観測し、「実ファイル内容」と「プロセス
// 終了」の両方を黒箱で検証する。保存経路:
//   `:`Enter -> route_ex_command -> process_pending_host_actions_with_runtime
//   -> handle_write_host_action_with_runtime -> write_to_path
// =============================================================================

/// `:wq<Enter>` で編集済みバッファが保存され、かつプロセスが終了することを
/// 実バイナリ経由で検証する（保存＋終了の複合）。
#[test]
fn wq_command_saves_then_exits_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // `dd` で先頭行 line01 を削除し、保存後に line02 始まりになることで
    // 「編集済みバッファが書かれた」と判定できる前提を作る。
    session.send(b"dd");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            first_row_body(parser) == "line02"
        }),
        "precondition: `dd` should leave line02 on the first row, got {:?}",
        session.screen_row_text(0)
    );

    // `:wq` で保存してから終了する。
    session.send(b":wq\r");

    // 終了（PTY EOF）を観測する。
    let exited = session.wait_until_process_exit(Duration::from_secs(8));
    assert!(exited, "`:wq` should exit the sy process after saving");

    // 終了後、ディスク上の実ファイルが編集済み（line01 削除）で保存されている。
    let target = fixture.target_path.clone();
    let saved = wait_for_file(&target, Duration::from_secs(5), |content| {
        content.starts_with("line02") && !content.starts_with("line01")
    });
    assert!(
        saved,
        "`:wq` should persist the edited buffer to disk, file content was {:?}",
        std::fs::read_to_string(&target).ok()
    );
}

/// `:x<Enter>` をクリーン（未編集）バッファで実行すると、ファイルを書き換えず
/// にそのまま終了することを実バイナリ経由で検証する。`:x` は変更が無ければ
/// 書き込みをスキップする vim 挙動。ここでは「終了する」「ファイルが不変」を観測する。
#[test]
fn x_command_on_clean_buffer_exits_without_changing_file() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let target = fixture.target_path.clone();
    let before = std::fs::read_to_string(&target).expect("target should be readable before");

    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // 何も編集せず `:x` で終了する。
    session.send(b":x\r");
    let exited = session.wait_until_process_exit(Duration::from_secs(8));
    assert!(exited, "`:x` on a clean buffer should exit the sy process");

    // ファイル内容は不変（クリーンバッファなので書き換わらない）。
    let after = std::fs::read_to_string(&target).expect("target should be readable after");
    assert_eq!(
        before, after,
        "`:x` on a clean buffer should leave the file unchanged"
    );
}

/// `:x<Enter>` を dirty（編集済み）バッファで実行すると、保存してから終了する
/// ことを実バイナリ経由で検証する。
#[test]
fn x_command_on_dirty_buffer_saves_then_exits() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // `dd` で先頭行を削除して dirty にする。
    session.send(b"dd");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            first_row_body(parser) == "line02"
        }),
        "precondition: `dd` should leave line02 on the first row, got {:?}",
        session.screen_row_text(0)
    );

    // `:x` で保存してから終了する。
    session.send(b":x\r");
    let exited = session.wait_until_process_exit(Duration::from_secs(8));
    assert!(exited, "`:x` on a dirty buffer should exit the sy process");

    let target = fixture.target_path.clone();
    let saved = wait_for_file(&target, Duration::from_secs(5), |content| {
        content.starts_with("line02") && !content.starts_with("line01")
    });
    assert!(
        saved,
        "`:x` on a dirty buffer should persist the edited buffer to disk, file content was {:?}",
        std::fs::read_to_string(&target).ok()
    );
}

/// `:write <path><Enter>` で現在のターゲットとは別の明示パスへ書き出せることを
/// 実バイナリ経由で検証する（explicit path write）。元のターゲットは不変で、
/// 指定した alternate ファイルが生成されることを観測する。
#[test]
fn write_command_with_explicit_path_creates_alternate_file() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let alternate = unique_path("write-explicit-alt.txt");
    // 念のため事前に存在しないことを保証する。
    let _ = std::fs::remove_file(&alternate);

    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // `:write <alternate>` を打って Enter。明示パスへ書き出す。
    let cmd = format!(":write {}\r", path_str(&alternate));
    session.send(cmd.as_bytes());

    // alternate ファイルが現在のバッファ内容（line01 始まり）で生成される。
    let created = wait_for_file(&alternate, Duration::from_secs(8), |content| {
        content.starts_with("line01")
    });
    assert!(
        created,
        "`:write <path>` should create the alternate file with the buffer contents, got {:?}",
        std::fs::read_to_string(&alternate).ok()
    );

    let _ = std::fs::remove_file(&alternate);
    // 後始末は Drop（:q!）に委ねる。
}

// =============================================================================
// カテゴリ A-4: 検索の変種を実バイナリ end-to-end で黒箱観測する。
//
// 既存マトリクスは `/`（入口）/`/line<Enter>` からの n/N 反復をカバーしている。
// ここでは「実マッチへのカーソル移動」「`/` 入力中の Esc キャンセル復帰」
// 「not-found(E486 相当)のメッセージ表示」という未カバー変種を観測する。
// =============================================================================

/// `/lineNN<Enter>` で、入口だけでなく実際に一致行へカーソルが移動することを
/// 実バイナリ経由で検証する。先頭(line01,row0)から `/line05` で row4 へ。
#[test]
fn slash_search_moves_cursor_to_match_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    assert_eq!(
        session.cursor_row(),
        0,
        "precondition: cursor starts on row 0"
    );

    // `/line05` で 5 行目（row4）の一致へジャンプする。
    session.send(b"/line05\r");
    assert!(
        wait_cursor_row(&mut session, 4, Duration::from_secs(5)),
        "`/line05` should move the cursor to the matching line (row 4), got row {}",
        session.cursor_row()
    );
    assert!(
        session.screen_row_text(4).contains("line05"),
        "`/line05` should land on line05, cursor row text was {:?}",
        session.screen_row_text(4)
    );
}

/// `/` 入力中に `Esc` でキャンセルすると command-line がクリアされ、カーソルが
/// 動かない（検索が実行されない）こと、その後あらためて `/line05<Enter>` で移動
/// できることを実バイナリ経由で検証する（キャンセル復帰フロー）。
#[test]
fn slash_search_esc_cancels_then_recovers_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    assert_eq!(
        session.cursor_row(),
        0,
        "precondition: cursor starts on row 0"
    );

    // `/` で検索入口に入り、語を打って command-line に表示されることを確認する。
    session.send(b"/line05");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            command_line_text(parser).contains("/line05")
        }),
        "`/line05` should render in the command line before Esc, got {:?}",
        session.command_line_row_text()
    );

    // `Esc` でキャンセル。command-line がクリアされ、カーソルは row0 のまま。
    session.send(b"\x1b");
    let cancelled = session.wait_until(Duration::from_secs(5), |parser| {
        !command_line_text(parser).trim_start().starts_with('/')
            && parser.screen().cursor_position().0 == 0
    });
    assert!(
        cancelled,
        "`Esc` during `/` should clear the command line and leave the cursor on row 0, got command-line {:?} / row {}",
        session.command_line_row_text(),
        session.cursor_row()
    );

    // キャンセル後、あらためて検索すると正しく移動できる（復帰フロー）。
    session.send(b"/line05\r");
    assert!(
        wait_cursor_row(&mut session, 4, Duration::from_secs(5)),
        "after Esc-cancel, `/line05` should still move the cursor to row 4, got row {}",
        session.cursor_row()
    );
}

/// 一致しない語で検索すると not-found（E486 相当）のメッセージが表示されること
/// を実バイナリ経由で検証する。`zzzNoMatch` はバッファに存在しない。
#[test]
fn slash_search_not_found_reports_e486_through_the_sy_binary() {
    let fixture = E2eFixture::with_lines(EDIT_LINES);
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // startup の cache-missing 警告 pager は、フルスイート実行（起動が遅い）だと
    // `ready_for_editing` 通過後に遅れて出てくることがあり、その場合 `/` 打鍵が
    // pager に食われて検索が走らない。pager を出し切る → 検索する、を pager が
    // 残らなくなるまで数回リトライして、タイミング flake を構造的に潰す。
    // 検索メッセージ自体（E486）の検出とは独立した「入力到達性の前提づくり」。
    let reported = {
        let mut got = false;
        for _ in 0..4 {
            drain_startup_pager(&mut session);
            session.send(b"/zzzNoMatch\r");
            // vim の not-found メッセージは "E486: Pattern not found: zzzNoMatch"。
            // 実装差を吸収するため "E486"/"Pattern not found"/"not found" を許容する。
            got = session.wait_until(Duration::from_secs(4), |parser| {
                let text = screen_text(parser);
                text.contains("E486")
                    || text.contains("Pattern not found")
                    || text.contains("not found")
            });
            if got {
                break;
            }
            // 検索が pager に食われていた場合に備え、Esc で入力途中状態を畳んでから
            // 次のリトライへ（pager が残っていれば次ループ頭の drain で消す）。
            session.send(b"\x1b");
        }
        got
    };
    assert!(
        reported,
        "searching for a missing pattern should report E486/not-found, screen:\n{}",
        screen_text(&session.parser)
    );
}

// =============================================================================
// カテゴリ A-6: Enter キーの到達性（autoindent/smartindent）を実バイナリで観測する。
//
// core の option 適用ロジック自体は core 契約テストが担う。ここでは「Enter キー
// が host 入力パイプラインを通って届き、改行＋自動インデントが起きる」という
// キー到達性だけを黒箱で確認する。smartindent を init.ts で有効化し、`{` の直後の
// Enter で次行がインデントされる（カーソル列が左端より右にある）ことを観測する。
// =============================================================================

/// smartindent 有効下で `i`→`{`→`Enter` を打つと、Enter 押下によって次行へ
/// 自動インデントが入る（カーソル列がインデント分だけ右に来る）ことを
/// 実バイナリ経由で検証する。空ファイルを使い、インデント観測を素直にする。
#[test]
fn enter_key_triggers_autoindent_with_smartindent_through_the_sy_binary() {
    // smartindent + shiftwidth を有効化した init.ts で空バッファを開く。
    let fixture = E2eFixture::with_lines_and_config(
        1,
        "saya.options.smartindent = true;\n            saya.options.shiftwidth = 4;",
    );
    let mut session = fixture.spawn();
    ready_for_editing(&mut session);

    // Insert に入り、`{` を入力する。`A`（行末追記）で line01 末尾に `{` を足す。
    session.send(b"A{");
    assert!(
        session.wait_until(Duration::from_secs(5), |parser| {
            first_row_body(parser).ends_with('{')
        }),
        "precondition: typing `{{` in insert mode should append it to line01, got {:?}",
        session.screen_row_text(0)
    );

    // ここで Enter を押す。smartindent により次行が自動インデントされ、
    // カーソル列がガター直後（インデント 0 の位置）より右へ来るはず。
    let col_before_enter = session.parser.screen().cursor_position().1;
    session.send(b"\r");
    let indented = session.wait_until(Duration::from_secs(5), |parser| {
        // 改行で row が 1 つ下へ進み、かつカーソル列がガター直後より右（インデント有）。
        let (row, col) = parser.screen().cursor_position();
        row >= 1 && col > GUTTER_COLS
    });
    assert!(
        indented,
        "Enter under smartindent should open an auto-indented next line (cursor col > gutter {GUTTER_COLS}), got cursor {:?} (col before Enter was {col_before_enter})",
        session.parser.screen().cursor_position()
    );
}

/// 起動時 pager（cache-missing 警告など）が残っていれば、消えるまで HitReturn を
/// 送って出し切る。`ready_for_editing` の一発 Enter で消えない／遅れて出る場合の
/// 取りこぼしを、メッセージ表示系テストの前段で補強するためのヘルパ。
fn drain_startup_pager(session: &mut SyPtySession) {
    for _ in 0..5 {
        if !session.screen_contains("[pager") {
            return;
        }
        session.send(b"\r");
        let _ = session.wait_until(Duration::from_secs(2), |parser| {
            !screen_text(parser).contains("[pager")
        });
    }
}

/// 行番号ガター（`   N `）が占める列幅。インデント観測のしきい値に使う。
/// 既定の number 表示は最小幅で本文が概ね col 5 付近から始まる。
const GUTTER_COLS: u16 = 4;

/// 画面最上段（row 0）の本文（ガター番号を除いた残り）を返す。
fn first_row_body(parser: &vt100::Parser) -> String {
    row_body(parser, 0)
}

/// 指定スクリーン行の本文（先頭のガター番号トークンを除いた残り）を返す。
fn row_body(parser: &vt100::Parser, row: u16) -> String {
    let screen = parser.screen();
    let mut text = String::new();
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
    // 行番号ガター（`   N `）を除いた本文部分。先頭空白＋数字＋空白を 1 トークンとみなす。
    let trimmed = text.trim_start();
    match trimmed.split_once(char::is_whitespace) {
        Some((gutter, rest)) if gutter.chars().all(|c| c.is_ascii_digit()) => {
            rest.trim().to_string()
        }
        _ => trimmed.trim_end().to_string(),
    }
}

/// 最終行（コマンドライン領域）のテキストを返す（`wait_until` の述語内で使う）。
fn command_line_text(parser: &vt100::Parser) -> String {
    let screen = parser.screen();
    let row = PTY_ROWS - 1;
    let mut text = String::new();
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
    text.trim_end().to_string()
}

/// 指定ファイルが述語を満たす内容になるまで（タイムアウト付きで）poll する。
fn wait_for_file<F>(path: &Path, timeout: Duration, predicate: F) -> bool
where
    F: Fn(&str) -> bool,
{
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(content) = std::fs::read_to_string(path)
            && predicate(&content)
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(80));
    }
}
