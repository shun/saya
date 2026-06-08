use std::process::Command;
use vim_core_rs::VimCoreSession;

#[allow(dead_code)]
fn run_case_in_subprocess(relative_case_path: &str) {
    let output = Command::new(std::env::current_exe().expect("current test binary should exist"))
        .arg("--exact")
        .arg("__vim_core_run_upstream_case")
        .arg("--nocapture")
        .env("VIM_CORE_UPSTREAM_TEST_CASE", relative_case_path)
        .output()
        .expect("subprocess should launch");

    assert!(
        output.status.success(),
        "upstream case {} failed\nstdout:\n{}\nstderr:\n{}",
        relative_case_path,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn __vim_core_run_upstream_case() {
    let Ok(relative_case_path) = std::env::var("VIM_CORE_UPSTREAM_TEST_CASE") else {
        return;
    };

    // 公開 API 経由で Vim スクリプトを実行する。
    // 各ケースは独立したプロセス（この subprocess 境界）で実行されるため、
    // 前のテストの状態を引き継ぐことはない。
    let mut session =
        VimCoreSession::new("").expect("runner should initialize a single VimCoreSession");

    // スクリプトファイルを source する。
    // 相対パスはリポジトリルートからの相対であることを前提とする。
    let command = format!("source {}", relative_case_path);
    session
        .execute_ex_command(&command)
        .expect("upstream case should execute without error");
}

include!(concat!(env!("OUT_DIR"), "/upstream_vim_tests.rs"));
