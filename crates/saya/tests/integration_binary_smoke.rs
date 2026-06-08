//! 統合テスト: `sy` バイナリの headless smoke 検証。
//!
//! 実行ファイルを直接起動し、ファイルを開いて 1 回編集し、
//! clean に保存して終了できることを確認する。

mod support;

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

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
    support::temp::unique_temp_path("binary-smoke", name)
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

fn smoke_state(stderr: &[u8], label: &str) -> serde_json::Value {
    let stderr = String::from_utf8_lossy(stderr);
    let prefix = format!("[main][smoke][state] {label} ");
    let line = stderr
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("smoke state {label:?} should be present: stderr={stderr}"));
    serde_json::from_str(line).unwrap_or_else(|error| {
        panic!("smoke state {label:?} should be valid JSON: {error}: line={line}")
    })
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
fn opening_missing_file_then_writing_creates_the_file_through_the_sy_binary() {
    let target_path = unique_path("open-missing-write.txt");
    let swap_path = swapfile_path_for_target(&target_path);
    assert!(
        !target_path.exists(),
        "test starts with a nonexistent target file"
    );

    let target_path_arg = target_path
        .to_str()
        .expect("target path should be valid UTF-8");
    let output = run_sy_headless_smoke(&[target_path_arg]);

    assert!(
        output.status.success(),
        "sy binary should open, edit, write, and quit a missing target cleanly: status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&target_path).expect("missing target should be created on write"),
        "X\n"
    );
    assert!(
        !swap_path.exists(),
        "normal quit should remove the swapfile for the newly-created file: {:?}",
        swap_path
    );

    std::fs::remove_file(&target_path).expect("cleanup target");
}

#[test]
fn opening_missing_file_then_quitting_without_write_does_not_create_the_file() {
    let target_path = unique_path("open-missing-quit.txt");
    assert!(
        !target_path.exists(),
        "test starts with a nonexistent target file"
    );

    let output = run_sy_headless_smoke_with_env(
        &[target_path
            .to_str()
            .expect("target path should be valid UTF-8")],
        &[("SAYA_BINARY_SMOKE_QUIT_WITHOUT_EDIT", "1")],
    );

    assert!(
        output.status.success(),
        "sy binary should open and quit a missing target cleanly without writing: status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !target_path.exists(),
        "quitting without a write must not create the target file"
    );
    let startup_state = smoke_state(&output.stderr, "startup");
    assert_eq!(
        startup_state["fileName"],
        target_path.to_string_lossy().as_ref()
    );
    assert_eq!(startup_state["dirty"], false);
}

#[test]
fn opening_markdown_mermaid_file_keeps_body_raw_through_the_sy_binary() {
    let target_path = unique_path("markdown-mermaid.md");
    std::fs::write(
        &target_path,
        "Before\n```mermaid\ngraph TD\n  A-->B\n```\nAfter\n",
    )
    .expect("markdown mermaid fixture should be created");

    let output = run_sy_headless_smoke_with_env(
        &[target_path
            .to_str()
            .expect("target path should be valid UTF-8")],
        &[("SAYA_BINARY_SMOKE_QUIT_WITHOUT_EDIT", "1")],
    );

    assert!(
        output.status.success(),
        "sy binary should open markdown mermaid file cleanly: status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let startup_state = smoke_state(&output.stderr, "startup");
    let lines = startup_state["lines"]
        .as_array()
        .expect("startup state should include visible lines")
        .iter()
        .filter_map(|line| line.as_str())
        .filter(|line| !line.is_empty())
        .map(|line| {
            let trimmed = line.trim_start();
            let without_number = trimmed
                .split_once(' ')
                .filter(|(prefix, _)| prefix.chars().all(|ch| ch.is_ascii_digit()))
                .map(|(_, rest)| rest)
                .unwrap_or(trimmed);
            without_number.to_string()
        })
        .collect::<Vec<_>>();
    assert!(
        lines.iter().any(|line| line == "```mermaid"),
        "Mermaid fence must remain in the body view: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line == "graph TD")
            && lines
                .iter()
                .any(|line| line == "A-->B" || line == "  A-->B"),
        "Mermaid body must remain visible as source text: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line == "[mermaid diagram]"),
        "inline image placeholder must not replace body source lines: {lines:?}"
    );

    std::fs::remove_file(&target_path).expect("cleanup markdown target");
}

#[test]
fn starting_with_directory_opens_dired_listing_through_the_sy_binary() {
    let root_path = unique_path("dired-startup-root");
    let nested_path = root_path.join("src");
    let file_path = root_path.join("README.md");
    let config_path = unique_path("dired-startup-init.ts");
    std::fs::create_dir_all(&nested_path).expect("nested directory should be created");
    std::fs::write(&file_path, "hello\n").expect("directory entry file should be created");
    std::fs::write(&config_path, "").expect("empty startup config should be created");

    let root_path_arg = root_path.to_str().expect("root path should be valid UTF-8");
    let config_path_arg = config_path
        .to_str()
        .expect("config path should be valid UTF-8");
    let output = run_sy_headless_smoke(&["-u", config_path_arg, root_path_arg]);

    assert!(
        output.status.success(),
        "directory smoke should exit cleanly: status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let startup_state = smoke_state(&output.stderr, "startup");
    assert_eq!(startup_state["firstLine"], "src/");
    assert_eq!(startup_state["fileName"], root_path_arg);
    assert_eq!(startup_state["mode"], "NORMAL");
    assert_eq!(startup_state["dirty"], false);

    std::fs::remove_dir_all(&root_path).expect("cleanup directory root");
    std::fs::remove_file(&config_path).expect("cleanup config");
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
    let startup_state = smoke_state(&output.stderr, "startup");
    assert_eq!(startup_state["firstLine"], "   1 alpha");
    assert_eq!(startup_state["fileName"], target_path_arg);
    assert_eq!(startup_state["mode"], "NORMAL");
    assert_eq!(startup_state["dirty"], false);
    assert_eq!(startup_state["lineNumbers"], true);
    assert_eq!(startup_state["numberWidth"], 4);

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
    let startup_state = smoke_state(&output.stderr, "startup");
    assert!(
        startup_state["firstLine"]
            .as_str()
            .is_some_and(|line| line.contains("alpha")),
        "stdin startup should read back projected stdin contents: {startup_state}"
    );
}

#[test]
fn bundled_completion_keymap_accepts_candidate_through_the_sy_binary() {
    let target_path = unique_path("completion-target.txt");
    let config_path = unique_path("completion-init.ts");
    let completion_path =
        saya::support::paths::dev_ts_plugins_dir().join("bundled/completion/index.ts");
    std::fs::write(&target_path, "ty\ntype\n").expect("target file should be created");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ createBufferWordSource, setupSayaCompletion }} from "{}";
                setupSayaCompletion({{
                    key: "<C-x>",
                    keys: {{ confirm: ["<Enter>"] }},
                    minPrefixLength: 2,
                    sourceTimeoutMs: 0,
                    sources: [createBufferWordSource()],
                }});
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
    let completed = smoke_state(&output.stderr, "completion-completed");
    assert_eq!(completed["transient"], "Saved successfully");
    let after_confirm = smoke_state(&output.stderr, "completion-after-confirm");
    assert_eq!(after_confirm["cursorRow"], 0);
    assert_eq!(after_confirm["cursorCol"], 4);
    assert_eq!(after_confirm["mode"], "Insert");

    std::fs::remove_file(&target_path).expect("cleanup target");
    std::fs::remove_file(&config_path).expect("cleanup config");
}

#[test]
fn bundled_completion_binary_smoke_can_select_second_candidate() {
    let target_path = unique_path("completion-select-target.txt");
    let config_path = unique_path("completion-select-init.ts");
    let completion_path =
        saya::support::paths::dev_ts_plugins_dir().join("bundled/completion/index.ts");
    std::fs::write(&target_path, "ty\ntype\ntyped\n").expect("target file should be created");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ createBufferWordSource, setupSayaCompletion }} from "{}";
                setupSayaCompletion({{
                    key: "<C-x>",
                    keys: {{ confirm: ["<Enter>"], next: ["<Down>"] }},
                    minPrefixLength: 2,
                    sourceTimeoutMs: 0,
                    sources: [createBufferWordSource()],
                }});
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
    let opened = smoke_state(&output.stderr, "completion-menu-opened");
    let opened_lines = opened["lines"]
        .as_array()
        .expect("opened completion lines should be an array");
    assert!(opened_lines.iter().any(|line| {
        line.as_str()
            .is_some_and(|line| line.contains("[Text] type"))
    }));
    assert!(opened_lines.iter().any(|line| {
        line.as_str()
            .is_some_and(|line| line.contains("[Text] typed"))
    }));
    let after_down = smoke_state(&output.stderr, "completion-menu-after-down");
    let after_down_lines = after_down["lines"]
        .as_array()
        .expect("selected completion lines should be an array");
    assert!(
        after_down_lines.iter().any(|line| {
            line.as_str()
                .is_some_and(|line| line.starts_with("> ") && line.contains("typed"))
        }),
        "second completion candidate should be selected after Down: {after_down_lines:?}"
    );
    let after_confirm = smoke_state(&output.stderr, "completion-after-confirm");
    assert_eq!(after_confirm["cursorRow"], 0);
    assert_eq!(after_confirm["cursorCol"], 5);
    assert_eq!(after_confirm["mode"], "Insert");

    std::fs::remove_file(&target_path).expect("cleanup target");
    std::fs::remove_file(&config_path).expect("cleanup config");
}

#[test]
fn bundled_path_completion_does_not_replace_buffer_with_directory_listing() {
    let root_path = unique_path("completion-path-root");
    let target_path = root_path.join("main.go");
    let dir_path = root_path.join("a_dir");
    let child_path = dir_path.join("child.go");
    let file_path = root_path.join("z.txt");
    let config_path = unique_path("completion-path-init.ts");
    let completion_path =
        saya::support::paths::dev_ts_plugins_dir().join("bundled/completion/index.ts");
    std::fs::create_dir_all(&dir_path).expect("directory candidate should be created");
    std::fs::write(&child_path, "package child\n").expect("child file candidate should be created");
    std::fs::write(&file_path, "z\n").expect("file candidate should be created");
    std::fs::write(&target_path, "./\n").expect("target file should be created");
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ createPathCompletionSource, setupSayaCompletion }} from "{}";
                setupSayaCompletion({{
                    key: "<C-x>",
                    keys: {{ confirm: ["<Tab>"] }},
                    autoTrigger: true,
                    autoTriggerDelayMs: 0,
                    sourceTimeoutMs: 0,
                    sources: [createPathCompletionSource({{
                        minPrefixLength: 1,
                        triggerCharacters: ["/", "."],
                    }})],
                }});
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
            ("SAYA_COMPLETION_SMOKE_CONFIRM_TAB", "1"),
            ("SAYA_COMPLETION_SMOKE_EXPECT_REOPEN_AFTER_CONFIRM", "1"),
        ],
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
    let opened = smoke_state(&output.stderr, "completion-menu-opened");
    let opened_lines = opened["lines"]
        .as_array()
        .expect("path completion lines should be an array");
    assert!(
        opened_lines
            .iter()
            .any(|line| line.as_str().is_some_and(|line| line.contains("./a_dir/")))
    );
    let after_confirm = smoke_state(&output.stderr, "completion-after-confirm");
    assert_eq!(after_confirm["cursorRow"], 0);
    assert_eq!(after_confirm["cursorCol"], 8);
    assert_eq!(after_confirm["mode"], "Insert");
    let reopened = smoke_state(&output.stderr, "completion-menu-reopened-after-confirm");
    let reopened_lines = reopened["lines"]
        .as_array()
        .expect("reopened path completion lines should be an array");
    assert!(
        reopened_lines.iter().any(|line| line
            .as_str()
            .is_some_and(|line| line.contains("./a_dir/child.go"))),
        "Tab-confirmed directory should immediately show child path candidates: {reopened_lines:?}"
    );

    std::fs::remove_file(&target_path).expect("cleanup target");
    std::fs::remove_file(&child_path).expect("cleanup child file");
    std::fs::remove_file(&file_path).expect("cleanup file");
    std::fs::remove_dir(&dir_path).expect("cleanup dir");
    std::fs::remove_dir(&root_path).expect("cleanup root");
    std::fs::remove_file(&config_path).expect("cleanup config");
}
