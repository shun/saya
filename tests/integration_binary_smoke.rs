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
            saya.options.lineNumbers = true;
            saya.options.numberWidth = 4;
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
