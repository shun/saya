//! 統合テスト: `sy` バイナリの headless smoke 検証。
//!
//! 実行ファイルを直接起動し、ファイルを開いて 1 回編集し、
//! clean に保存して終了できることを確認する。

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn swapfile_path_for_target(target_path: &std::path::Path) -> PathBuf {
    let file_name = target_path
        .file_name()
        .expect("target file name should exist")
        .to_string_lossy();
    let swap_name = if file_name.starts_with('.') {
        format!("{file_name}.swp")
    } else {
        format!(".{file_name}.swp")
    };
    target_path.with_file_name(swap_name)
}

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-binary-smoke-{name}-{nanos}"))
}

fn sy_binary_path() -> PathBuf {
    let mut path = std::env::current_exe().expect("test binary path should exist");
    path.pop();
    path.pop();
    path.push("sy");
    path
}

fn run_sy_headless_smoke(args: &[&str]) -> Output {
    run_sy_headless_smoke_with_env(args, &[])
}

fn run_sy_headless_smoke_with_env(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let binary = sy_binary_path();
    assert!(binary.exists(), "sy binary should exist at {:?}", binary);

    let mut command = Command::new(binary);
    command.env("SAYA_BINARY_SMOKE", "1");
    for (key, value) in envs {
        command.env(key, value);
    }
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("headless smoke should spawn sy")
}

fn run_sy_completion_smoke_with_env(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let binary = sy_binary_path();
    assert!(binary.exists(), "sy binary should exist at {:?}", binary);

    let mut command = Command::new(binary);
    command
        .env("SAYA_BINARY_SMOKE", "1")
        .env("SAYA_COMPLETION_SMOKE", "1");
    for (key, value) in envs {
        command.env(key, value);
    }
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("completion smoke should spawn sy")
}

fn run_sy_headless_smoke_with_stdin(args: &[&str], stdin_text: &[u8]) -> Output {
    let binary = sy_binary_path();
    assert!(binary.exists(), "sy binary should exist at {:?}", binary);

    let mut child = Command::new(binary)
        .env("SAYA_BINARY_SMOKE", "1")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("headless smoke should spawn sy");

    child
        .stdin
        .as_mut()
        .expect("stdin should be available")
        .write_all(stdin_text)
        .expect("stdin smoke input should write");
    drop(child.stdin.take());

    child
        .wait_with_output()
        .expect("headless smoke should finish")
}

#[test]
fn opening_editing_once_and_quitting_cleanly_works_through_the_sy_binary() {
    let target_path = unique_path("open-edit-quit.txt");
    std::fs::write(&target_path, "alpha\n").expect("test file should be created");
    let swap_path = swapfile_path_for_target(&target_path);

    let target_path_arg = target_path
        .to_str()
        .expect("target path should be valid UTF-8");
    let output = run_sy_headless_smoke(&[target_path_arg]);

    assert!(
        output.status.success(),
        "sy binary should exit cleanly: status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let contents = std::fs::read_to_string(&target_path).expect("file should still be readable");
    assert_eq!(contents, "Xalpha\n");
    assert!(
        !swap_path.exists(),
        "normal quit should remove the swapfile: {:?}",
        swap_path
    );

    std::fs::remove_file(&target_path).expect("cleanup");
}

#[test]
fn opening_with_u_init_ts_projects_startup_configuration_into_the_ui() {
    let target_path = unique_path("startup-config-target.txt");
    let config_path = unique_path("init.ts");
    std::fs::write(&target_path, "alpha\nbeta\n").expect("target file should be created");
    std::fs::write(
        &config_path,
        r#"
            saya.options.number = true;
            saya.options.numberwidth = 4;
        "#,
    )
    .expect("startup config should be created");

    let target_path_arg = target_path
        .to_str()
        .expect("target path should be valid UTF-8");
    let config_path_arg = config_path
        .to_str()
        .expect("config path should be valid UTF-8");
    let output = run_sy_headless_smoke(&["-u", config_path_arg, target_path_arg]);

    assert!(
        output.status.success(),
        "sy binary should exit cleanly: status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("projected startup ui"),
        "smoke output should include the projected startup UI: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Some(\"   1 alpha\")"),
        "startup config should affect projected line numbers: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::remove_file(&target_path).expect("cleanup target");
    std::fs::remove_file(&config_path).expect("cleanup config");
}

#[test]
fn init_ts_log_file_writes_binary_smoke_logs() {
    let target_path = unique_path("startup-log-target.txt");
    let config_path = unique_path("log-init.ts");
    let log_path = unique_path("startup.log");
    std::fs::write(&target_path, "alpha\n").expect("target file should be created");
    std::fs::write(
        &config_path,
        format!(
            r#"
                saya.log.file = {};
            "#,
            serde_json::to_string(log_path.to_str().expect("log path should be UTF-8"))
                .expect("log path should serialize")
        ),
    )
    .expect("startup config should be created");

    let output = run_sy_headless_smoke_with_env(
        &[
            "-u",
            config_path.to_str().expect("config path should be UTF-8"),
            target_path.to_str().expect("target path should be UTF-8"),
        ],
        &[("SAYA_LOG", "0")],
    );

    assert!(
        output.status.success(),
        "sy binary should exit cleanly: status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let log = std::fs::read_to_string(&log_path).expect("init.ts log file should be created");
    assert!(
        log.contains("[diagnostic_log] startup log file configured"),
        "log should confirm init.ts configured logging: {log}"
    );
    assert!(
        log.contains("[bootstrap] startup preflight requested"),
        "buffered startup logs should be flushed into the init.ts log file: {log}"
    );

    std::fs::remove_file(&target_path).expect("cleanup target");
    std::fs::remove_file(&config_path).expect("cleanup config");
    std::fs::remove_file(&log_path).expect("cleanup log");
}

#[test]
fn init_ts_log_level_filters_binary_smoke_logs() {
    let target_path = unique_path("startup-log-level-target.txt");
    let config_path = unique_path("log-level-init.ts");
    let log_path = unique_path("startup-level.log");
    std::fs::write(&target_path, "alpha\n").expect("target file should be created");
    std::fs::write(
        &config_path,
        format!(
            r#"
                saya.log.file = {};
                saya.log.level = "warn";
            "#,
            serde_json::to_string(log_path.to_str().expect("log path should be UTF-8"))
                .expect("log path should serialize")
        ),
    )
    .expect("startup config should be created");

    let output = run_sy_headless_smoke_with_env(
        &[
            "-u",
            config_path.to_str().expect("config path should be UTF-8"),
            target_path.to_str().expect("target path should be UTF-8"),
        ],
        &[("SAYA_LOG", "0")],
    );

    assert!(
        output.status.success(),
        "sy binary should exit cleanly: status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let log = std::fs::read_to_string(&log_path).expect("init.ts log file should be created");
    assert!(
        !log.contains("[bootstrap] startup preflight requested"),
        "warn startup level should filter debug logs: {log}"
    );

    std::fs::remove_file(&target_path).expect("cleanup target");
    std::fs::remove_file(&config_path).expect("cleanup config");
    std::fs::remove_file(&log_path).expect("cleanup log");
}

#[test]
fn starting_from_stdin_surfaces_save_path_restriction_in_the_smoke_output() {
    let output = run_sy_headless_smoke_with_stdin(&["-"], b"alpha\nbeta\n");

    assert!(
        !output.status.success(),
        "stdin smoke should fail because no save path is available: status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("No file name to save"),
        "stdin smoke should surface the save-path restriction: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("first_line=Some(\"")
            && String::from_utf8_lossy(&output.stderr).contains("alpha"),
        "stdin smoke should project stdin contents into the startup UI: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn bundled_completion_keymap_accepts_candidate_through_the_sy_binary() {
    let target_path = unique_path("completion-target.txt");
    let config_path = unique_path("completion-init.ts");
    let completion_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/bundled/completion/index.ts");
    std::fs::write(&target_path, "ty\ntype\n").expect("target file should be created");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ setupSayaCompletion }} from "{}";
                setupSayaCompletion({{ key: "<C-x>", sourceTimeoutMs: 0 }});
            "#,
            completion_path.to_string_lossy()
        ),
    )
    .expect("startup config should be created");

    let output = run_sy_completion_smoke_with_env(
        &[
            "-u",
            config_path.to_str().expect("config path should be UTF-8"),
            target_path.to_str().expect("target path should be UTF-8"),
        ],
        &[("SAYA_LOG", "0")],
    );

    assert!(
        output.status.success(),
        "completion smoke should exit cleanly: status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&target_path).expect("target should be readable"),
        "type\ntype\n"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[main][smoke][completion] completed"),
        "completion smoke should report completion: stderr={}",
        stderr
    );
    assert!(
        stderr.contains("[main][smoke][completion] after confirm: cursor=(0,4), mode=Insert"),
        "completion smoke should confirm cursor state after insertion: stderr={stderr}"
    );

    std::fs::remove_file(&target_path).expect("cleanup target");
    std::fs::remove_file(&config_path).expect("cleanup config");
}

#[test]
fn bundled_completion_binary_smoke_can_select_second_candidate() {
    let target_path = unique_path("completion-select-target.txt");
    let config_path = unique_path("completion-select-init.ts");
    let completion_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/bundled/completion/index.ts");
    std::fs::write(&target_path, "ty\ntype\ntyped\n").expect("target file should be created");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ setupSayaCompletion }} from "{}";
                setupSayaCompletion({{ key: "<C-x>", sourceTimeoutMs: 0 }});
            "#,
            completion_path.to_string_lossy()
        ),
    )
    .expect("startup config should be created");

    let output = run_sy_completion_smoke_with_env(
        &[
            "-u",
            config_path.to_str().expect("config path should be UTF-8"),
            target_path.to_str().expect("target path should be UTF-8"),
        ],
        &[
            ("SAYA_LOG", "0"),
            ("SAYA_COMPLETION_SMOKE_EXPECT_MULTIPLE", "1"),
            ("SAYA_COMPLETION_SMOKE_SELECT_NEXT", "1"),
        ],
    );

    assert!(
        output.status.success(),
        "completion select smoke should exit cleanly: status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&target_path).expect("target should be readable"),
        "typed\ntype\ntyped\n"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("menu opened: lines=")
            && stderr.contains("type")
            && stderr.contains("typed"),
        "completion smoke should log multiple menu candidates: stderr={stderr}"
    );
    assert!(
        stderr.contains("menu after Down: lines=") && stderr.contains("> [Text] typed"),
        "completion smoke should log selected second candidate after Down: stderr={stderr}"
    );
    assert!(
        stderr.contains("[main][smoke][completion] after confirm: cursor=(0,5), mode=Insert"),
        "completion smoke should confirm cursor state after selected insertion: stderr={stderr}"
    );

    std::fs::remove_file(&target_path).expect("cleanup target");
    std::fs::remove_file(&config_path).expect("cleanup config");
}

#[test]
fn bundled_path_completion_does_not_replace_buffer_with_directory_listing() {
    let root_path = unique_path("completion-path-root");
    let target_path = root_path.join("main.go");
    let dir_path = root_path.join("a_dir");
    let file_path = root_path.join("z.txt");
    let config_path = unique_path("completion-path-init.ts");
    let completion_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/bundled/completion/index.ts");
    std::fs::create_dir_all(&dir_path).expect("directory candidate should be created");
    std::fs::write(&file_path, "z\n").expect("file candidate should be created");
    std::fs::write(&target_path, "./\n").expect("target file should be created");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ setupSayaCompletion }} from "{}";
                setupSayaCompletion({{ key: "<C-x>", sourceTimeoutMs: 0 }});
            "#,
            completion_path.to_string_lossy()
        ),
    )
    .expect("startup config should be created");

    let output = run_sy_completion_smoke_with_env(
        &[
            "-u",
            config_path.to_str().expect("config path should be UTF-8"),
            target_path.to_str().expect("target path should be UTF-8"),
        ],
        &[("SAYA_LOG", "0")],
    );

    assert!(
        output.status.success(),
        "path completion smoke should exit cleanly: status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&target_path).expect("target should be readable"),
        "./a_dir/\n",
        "path completion must update only the typed path prefix, not project a directory listing into the edited buffer"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("menu opened: lines=") && stderr.contains("./a_dir/"),
        "path completion smoke should log path candidates: stderr={stderr}"
    );
    assert!(
        stderr.contains("[main][smoke][completion] after confirm: cursor=(0,8), mode=Insert"),
        "path completion smoke should confirm cursor state after path insertion: stderr={stderr}"
    );

    std::fs::remove_file(&target_path).expect("cleanup target");
    std::fs::remove_file(&file_path).expect("cleanup file");
    std::fs::remove_dir(&dir_path).expect("cleanup dir");
    std::fs::remove_dir(&root_path).expect("cleanup root");
    std::fs::remove_file(&config_path).expect("cleanup config");
}
