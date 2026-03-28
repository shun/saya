/// 統合テスト: 起動フローの検証
///
/// このファイルは `saya` の main startup and session orchestration suite
/// です。
///
/// 責務は host/application 層の起動準備、セッションガード、bootstrap
/// cleanup、初期 projection、startup warning routing に限定する。
/// 詳細な編集セマンティクスは ADR 0001 に従って `vim-core-rs` に委ねる。
///
/// 既存ファイル起動、新規バッファ起動、読込失敗を個別に確認する。
/// 起動失敗時にセッションが中途半端に残らないことを確認する。
/// Requirements: 1.1, 1.2, 1.3, 3.4
use std::io;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use saya::app_startup::prepare_launch_and_start_terminal;
use saya::bootstrap::{
    BootstrapError, BootstrapWarning, LoadedConfig, bootstrap_warning_message, launch_test_lock,
    prepare_launch, prepare_launch_with_reader,
};
use saya::cli::{
    ConfigSource, InitialCursorPosition, InputSource, LaunchRequest, parse_launch_request,
};
use saya::editor_session::{EditorSessionState, SaveRequestError};
use saya::screen_model::{ProjectionInput, project};
use saya::terminal_lifecycle::TerminalBackend;
use vim_core_rs::CoreMode;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-integ-startup-{name}-{nanos}"))
}

fn default_request() -> LaunchRequest {
    LaunchRequest::default()
}

fn cwd_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn startup_suite_scope_statement() -> &'static str {
    "main startup and session orchestration suite for host/application launch preparation, session guard cleanup, bootstrap cleanup, startup warning routing, and initial projection"
}

#[test]
fn startup_suite_scope_statement_stays_pinned_to_host_layer_orchestration() {
    let statement = startup_suite_scope_statement();

    assert!(
        statement.contains("main startup and session orchestration suite"),
        "suite ownership statement should stay explicit"
    );
    assert!(
        statement.contains("launch preparation"),
        "suite ownership statement should stay host-layer focused"
    );
    assert!(
        statement.contains("session guard cleanup"),
        "suite ownership statement should keep lifecycle responsibility visible"
    );
    assert!(
        statement.contains("startup warning routing"),
        "suite ownership statement should mention projected startup warnings"
    );
    assert!(
        !statement.contains("editing semantics"),
        "suite ownership statement must not drift into core-editing ownership"
    );
}

#[test]
fn startup_related_test_files_use_startup_prefix_instead_of_wave6_prefix() {
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let file_names: Vec<String> = std::fs::read_dir(&tests_dir)
        .expect("tests directory should be readable")
        .map(|entry| {
            entry
                .expect("test directory entry should be readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    assert!(
        file_names.contains(&"integration_startup_boot_flow.rs".to_string()),
        "startup boot flow suite should use the startup-oriented naming convention"
    );
    assert!(
        file_names.contains(&"integration_startup_tab_size.rs".to_string()),
        "startup tab size suite should use the startup-oriented naming convention"
    );
    assert!(
        !file_names.contains(&"integration_wave6_boot_flow.rs".to_string()),
        "wave6 boot flow naming should be retired from the startup suite"
    );
    assert!(
        !file_names.contains(&"integration_wave6_startup_tab_size.rs".to_string()),
        "wave6 tab size naming should be retired from the startup suite"
    );
}

#[test]
fn major_integration_files_keep_host_layer_file_comments() {
    let major_files = [
        "integration_startup.rs",
        "integration_startup_boot_flow.rs",
        "integration_startup_tab_size.rs",
        "integration_presentation_line_numbers.rs",
        "integration_save_quit.rs",
        "integration_terminal.rs",
        "integration_typescript_runtime_config_api.rs",
        "integration_typescript_runtime_command.rs",
        "integration_typescript_runtime_typed_payload.rs",
    ];
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");

    for file_name in major_files {
        let path = tests_dir.join(file_name);
        let header = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {file_name}: {error}"));
        let header = header.lines().take(8).collect::<Vec<_>>().join("\n");

        assert!(
            header.contains("host/application") || header.contains("host layer"),
            "major integration file should state its host-layer responsibility: {file_name}"
        );
    }
}

#[derive(Default)]
struct RecordingTerminalBackend {
    calls: Vec<&'static str>,
}

impl TerminalBackend for RecordingTerminalBackend {
    fn enable_raw_mode(&mut self) -> io::Result<()> {
        self.calls.push("enable_raw_mode");
        Ok(())
    }

    fn enter_alternate_screen(&mut self) -> io::Result<()> {
        self.calls.push("enter_alternate_screen");
        Ok(())
    }

    fn leave_alternate_screen(&mut self) -> io::Result<()> {
        self.calls.push("leave_alternate_screen");
        Ok(())
    }

    fn disable_raw_mode(&mut self) -> io::Result<()> {
        self.calls.push("disable_raw_mode");
        Ok(())
    }
}

// ---- 9.1.1: 既存ファイル起動の統合フロー ----

/// CLI 引数パースから起動完了まで、既存ファイル起動の一連の流れが成立する。
#[test]
fn existing_file_startup_flow_from_cli_args_to_initial_screen_model() {
    let target_path = unique_path("existing-file");
    let target_content = "Hello Saya\nSecond line\n";
    std::fs::write(&target_path, target_content).expect("テストファイルの作成");

    // 1. CLI 引数パース
    let request = parse_launch_request([target_path.to_str().unwrap()])
        .expect("CLI 引数のパースが成功すること");
    assert_eq!(request.target_path(), Some(&target_path));

    // 2. 起動準備
    let outcome = prepare_launch(request).expect("既存ファイルでの起動が成功すること");

    // 3. 起動結果の検証
    assert_eq!(outcome.target_path, Some(target_path.clone()));
    assert_eq!(outcome.initial_snapshot.text, target_content);
    assert_eq!(outcome.initial_snapshot.mode, CoreMode::Normal);
    assert!(!outcome.initial_snapshot.dirty);
    assert!(outcome.warnings.is_empty());

    // 4. 起動直後の ScreenModel 生成
    let session_state = EditorSessionState::new(outcome.target_path.clone());
    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));

    assert_eq!(
        model.file_name,
        target_path.display().to_string(),
        "ファイル名が表示されること"
    );
    assert_eq!(model.mode_label, "NORMAL");
    assert!(!model.dirty);
    assert!(!model.lines.is_empty(), "行データが存在すること");
    assert_eq!(model.message_line, None);

    // クリーンアップ
    std::fs::remove_file(&target_path).expect("テストファイルの削除");
}

// ---- 9.1.2: 新規バッファ起動の統合フロー ----

/// CLI 引数なしで起動した場合、空の新規バッファとして開始し、
/// ScreenModel に "[新規]" 表示が反映される。
#[test]
fn new_buffer_startup_flow_without_target_path() {
    // 1. CLI 引数パース（対象パスなし）
    let request =
        parse_launch_request::<&[&str], &&str>(&[]).expect("空引数のパースが成功すること");
    assert_eq!(request.target_path(), None);

    // 2. 起動準備
    let outcome = prepare_launch(request).expect("新規バッファでの起動が成功すること");

    // 3. 起動結果の検証
    assert_eq!(outcome.target_path, None);
    assert_eq!(outcome.initial_snapshot.text, "\n");
    assert_eq!(outcome.initial_snapshot.mode, CoreMode::Normal);
    assert!(!outcome.initial_snapshot.dirty);
    assert!(outcome.warnings.is_empty());

    // 4. ScreenModel 生成
    let session_state = EditorSessionState::new(None);
    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));

    assert_eq!(
        model.file_name, "[新規]",
        "新規バッファでは [新規] が表示されること"
    );
    assert_eq!(model.mode_label, "NORMAL");
    assert!(!model.dirty);
}

// ---- 9.1.3: 読込失敗の統合フロー ----

/// 存在しないファイルを指定した場合、起動失敗として BootstrapError を返す。
#[test]
fn read_failure_startup_flow_returns_fatal_error() {
    let missing_path = unique_path("nonexistent");

    // 1. CLI 引数パース
    let request = parse_launch_request([missing_path.to_str().unwrap()])
        .expect("CLI 引数のパースが成功すること");

    // 2. 起動準備は失敗する
    let result = prepare_launch(request);

    match result {
        Err(BootstrapError::TargetReadFailed { path, message }) => {
            assert_eq!(path, missing_path);
            assert!(!message.is_empty(), "エラーメッセージは空でないこと");
        }
        other => panic!(
            "存在しないファイルは TargetReadFailed を返すこと, got: {:?}",
            other
        ),
    }
}

/// 読み込み不能ファイル（権限不足）で起動失敗となる。
#[test]
fn read_failure_startup_flow_for_permission_denied() {
    let restricted_path = unique_path("permission-denied");
    std::fs::write(&restricted_path, "restricted").expect("テストファイルの作成");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&restricted_path, std::fs::Permissions::from_mode(0o000))
            .expect("権限の変更");
    }

    let request = parse_launch_request([restricted_path.to_str().unwrap()])
        .expect("CLI 引数のパースが成功すること");

    #[cfg(unix)]
    {
        let result = prepare_launch(request);
        assert!(
            matches!(result, Err(BootstrapError::TargetReadFailed { .. })),
            "権限不足のファイルは TargetReadFailed を返すこと"
        );
    }

    // クリーンアップ
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&restricted_path, std::fs::Permissions::from_mode(0o644));
    }
    let _ = std::fs::remove_file(&restricted_path);
}

// ---- 9.1.4: 起動失敗時の terminal lifecycle 未初期化確認 ----

/// 起動準備が失敗した場合、terminal lifecycle backend が一切触られない。
#[test]
fn startup_failure_leaves_terminal_lifecycle_uninitialized() {
    let missing_path = unique_path("terminal-uninitialized");
    let request = LaunchRequest {
        input_source: InputSource::File(missing_path),
        config_source: ConfigSource::Default,
        ..default_request()
    };
    let mut backend = RecordingTerminalBackend::default();

    let result = prepare_launch_and_start_terminal(request, &mut backend);

    assert!(result.is_err(), "起動失敗になること");
    drop(result);
    assert!(
        backend.calls.is_empty(),
        "bootstrap failure 時に terminal backend が一切呼ばれないこと"
    );
}

// ---- 9.1.4: 起動失敗時のセッション残留なし ----

/// 起動失敗後にセッションガードが解放され、再度起動可能であることを確認する。
#[test]
fn session_guard_released_after_startup_failure() {
    let missing_path = unique_path("session-cleanup");

    // 1. 最初の起動試行（失敗する）
    let result = prepare_launch(LaunchRequest {
        input_source: InputSource::File(missing_path),
        config_source: ConfigSource::Default,
        ..default_request()
    });
    assert!(result.is_err(), "起動は失敗すること");

    // 2. セッションガードが解放されているので、再度起動可能
    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::Default,
        ..default_request()
    });
    assert!(
        outcome.is_ok(),
        "起動失敗後にセッションが解放され、再起動できること"
    );
}

/// 起動成功後に outcome を drop すると、セッションガードが解放される。
#[test]
fn session_guard_released_after_successful_startup_outcome_dropped() {
    // 1. 成功起動
    {
        let _outcome = prepare_launch(LaunchRequest {
            input_source: InputSource::Empty,
            config_source: ConfigSource::Default,
            ..default_request()
        })
        .expect("起動成功");
        // outcome がスコープを抜けて drop される
    }

    // 2. 再度起動可能であること
    let outcome2 = prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::Default,
        ..default_request()
    });
    assert!(
        outcome2.is_ok(),
        "前回の outcome drop 後にセッションガードが解放され、再起動できること"
    );
}

#[test]
fn repeated_start_fail_start_cycles_keep_launch_state_and_cleanup_consistent() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let success_path = unique_path("repeat-success");
    let failure_path = unique_path("repeat-failure");

    std::fs::write(&success_path, "line1\nline2\n").expect("成功用のテストファイルの作成");

    let success_request = parse_launch_request(["-R", "+2", success_path.to_str().unwrap()])
        .expect("成功サイクル用 CLI 引数のパースが成功すること");
    let failure_request = parse_launch_request(["-R", "+2", failure_path.to_str().unwrap()])
        .expect("失敗サイクル用 CLI 引数のパースが成功すること");

    assert!(success_request.read_only);
    assert_eq!(
        success_request.initial_cursor,
        InitialCursorPosition::Line(2)
    );
    assert!(failure_request.read_only);
    assert_eq!(
        failure_request.initial_cursor,
        InitialCursorPosition::Line(2)
    );

    log::debug!("[test] cycle 1: successful launch before failure");
    let first_outcome =
        prepare_launch(success_request.clone()).expect("最初の起動サイクルが成功すること");
    assert_eq!(first_outcome.target_path, Some(success_path.clone()));
    assert!(
        first_outcome.read_only,
        "CLI の read-only 状態が保持されること"
    );
    assert_eq!(
        first_outcome.initial_snapshot.cursor_row, 1,
        "CLI の行指定が初回起動に反映されること"
    );
    drop(first_outcome);

    log::debug!("[test] cycle 2: expected bootstrap failure");
    let failure = prepare_launch(failure_request.clone());
    match failure {
        Err(BootstrapError::TargetReadFailed { path, .. }) => {
            assert_eq!(path, failure_path);
        }
        other => panic!(
            "存在しないファイルは TargetReadFailed を返すこと, got: {:?}",
            other
        ),
    }

    log::debug!("[test] cycle 3: successful relaunch after failure");
    let second_outcome =
        prepare_launch(success_request).expect("失敗後に同じ launch state で再起動できること");
    assert_eq!(second_outcome.target_path, Some(success_path.clone()));
    assert!(
        second_outcome.read_only,
        "再起動後も read-only 状態が保持されること"
    );
    assert_eq!(
        second_outcome.initial_snapshot.cursor_row, 1,
        "再起動後も CLI の行指定が反映されること"
    );

    std::fs::remove_file(&success_path).expect("成功用のテストファイルの削除");
}

// ---- 9.1.5: 設定付き起動の統合フロー ----

/// 設定ファイル付きの起動で config が warning なく読み込まれる。
#[test]
fn startup_with_vim_style_u_option_loads_config_without_warning() {
    let config_path = unique_path("config-ok.json");
    std::fs::write(&config_path, "{ \"tabSize\": 4 }").expect("設定ファイルの作成");

    let request = parse_launch_request(["-u", config_path.to_str().unwrap()])
        .expect("CLI 引数のパースが成功すること");

    let outcome = prepare_launch(request).expect("設定付き起動が成功すること");

    assert_eq!(
        outcome.loaded_config,
        LoadedConfig::File {
            path: config_path.clone(),
            source: "{ \"tabSize\": 4 }".to_string(),
        }
    );
    assert!(
        outcome.warnings.is_empty(),
        "有効な設定ファイルでは warning なし"
    );

    std::fs::remove_file(&config_path).expect("設定ファイルの削除");
}

/// 存在しない設定ファイルを指定した場合、warning 付きで既定値起動する。
#[test]
fn startup_with_missing_config_falls_back_with_warning() {
    let missing_config = unique_path("config-missing.json");

    let request = parse_launch_request(["--config", missing_config.to_str().unwrap()])
        .expect("CLI 引数のパースが成功すること");

    let outcome = prepare_launch(request).expect("設定失敗でも起動は成功すること");

    assert_eq!(outcome.loaded_config, LoadedConfig::Default);
    assert_eq!(outcome.warnings.len(), 1);
    assert!(matches!(
        &outcome.warnings[0],
        BootstrapWarning::ConfigLoadFailed { path, .. } if *path == missing_config
    ));
}

#[test]
fn startup_warning_projects_into_initial_message_line() {
    let missing_config = unique_path("config-warning-projection.json");

    let request = parse_launch_request(["--config", missing_config.to_str().unwrap()])
        .expect("CLI 引数のパースが成功すること");
    let outcome = prepare_launch(request).expect("warning 付き起動が成功すること");
    let session_state = outcome.editor_session_state();
    let warning_message = bootstrap_warning_message(&outcome.warnings)
        .expect("起動 warning が host message として可視化されること");

    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        Some(warning_message.as_str()),
    ));

    assert_eq!(model.message_line, Some(warning_message));
}

#[test]
fn startup_from_stdin_populates_initial_snapshot() {
    let request = parse_launch_request(["-"]).expect("CLI 引数のパースが成功すること");
    let mut stdin = Cursor::new("stdin line 1\nstdin line 2\n");

    let outcome =
        prepare_launch_with_reader(request, &mut stdin).expect("stdin 起動が成功すること");

    assert_eq!(outcome.target_path, None);
    assert_eq!(
        outcome.initial_snapshot.text,
        "stdin line 1\nstdin line 2\n"
    );
    assert_eq!(outcome.initial_snapshot.mode, CoreMode::Normal);
}

#[test]
fn startup_with_initial_line_number_moves_cursor_to_requested_line() {
    let target_path = unique_path("cursor-line");
    std::fs::write(&target_path, "line1\nline2\nline3\n").expect("テストファイルの作成");

    let request = parse_launch_request(["+2", target_path.to_str().unwrap()])
        .expect("CLI 引数のパースが成功すること");
    let outcome = prepare_launch(request).expect("行指定付き起動が成功すること");

    assert_eq!(outcome.initial_snapshot.cursor_row, 1);
    assert_eq!(outcome.initial_snapshot.cursor_col, 0);

    std::fs::remove_file(&target_path).expect("テストファイルの削除");
}

#[test]
fn startup_with_end_of_file_moves_cursor_to_last_line() {
    let target_path = unique_path("cursor-end");
    std::fs::write(&target_path, "line1\nline2\nline3\n").expect("テストファイルの作成");

    let request = parse_launch_request(["+", target_path.to_str().unwrap()])
        .expect("CLI 引数のパースが成功すること");
    let outcome = prepare_launch(request).expect("EOF 指定付き起動が成功すること");

    assert_eq!(outcome.initial_snapshot.cursor_row, 2);
    assert_eq!(outcome.initial_snapshot.cursor_col, 0);

    std::fs::remove_file(&target_path).expect("テストファイルの削除");
}

#[test]
fn startup_with_read_only_rejects_save_request() {
    let target_path = unique_path("read-only");
    std::fs::write(&target_path, "line1\n").expect("テストファイルの作成");

    let request = parse_launch_request(["-R", target_path.to_str().unwrap()])
        .expect("CLI 引数のパースが成功すること");
    let outcome = prepare_launch(request).expect("read-only 起動が成功すること");
    let session_state = outcome.editor_session_state();
    let save_result = session_state.build_save_request(&outcome.initial_snapshot.text);

    assert_eq!(save_result, Err(SaveRequestError::ReadOnly));

    std::fs::remove_file(&target_path).expect("テストファイルの削除");
}

#[test]
fn startup_with_dash_dash_accepts_leading_dash_file_name() {
    let target_path = unique_path("-leading-name.txt");
    std::fs::write(&target_path, "dash file\n").expect("テストファイルの作成");

    let request = parse_launch_request(["--", target_path.to_str().unwrap()])
        .expect("CLI 引数のパースが成功すること");
    let outcome = prepare_launch(request).expect("ダッシュ始まりのファイル起動が成功すること");

    assert_eq!(outcome.target_path, Some(target_path.clone()));
    assert_eq!(outcome.initial_snapshot.text, "dash file\n");

    std::fs::remove_file(&target_path).expect("テストファイルの削除");
}

#[test]
fn startup_with_relative_config_path_resolves_line_numbers_from_current_directory() {
    let _cwd_lock = cwd_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let base_dir = unique_path("relative-config-base");
    let work_dir = base_dir.join("worktree").join("nested");
    let config_path = base_dir.join("init.ts");
    let target_path = base_dir.join("target.txt");

    std::fs::create_dir_all(&work_dir).expect("作業ディレクトリの作成");
    std::fs::write(&config_path, "saya.options.lineNumbers = true;\n").expect("設定ファイルの作成");
    std::fs::write(&target_path, "alpha\nbeta\n").expect("対象ファイルの作成");

    let previous_dir = std::env::current_dir().expect("現在ディレクトリの取得");
    std::env::set_current_dir(&work_dir).expect("作業ディレクトリへの移動");

    let request = parse_launch_request(["-u", "../../init.ts", "../../target.txt"])
        .expect("CLI 引数のパースが成功すること");
    let outcome = prepare_launch(request).expect("相対設定パスでの起動が成功すること");

    std::env::set_current_dir(previous_dir).expect("カレントディレクトリの復元");

    assert!(outcome.initial_line_numbers);
    let session_state = outcome.editor_session_state();
    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));
    assert_eq!(model.lines[0], "   1 alpha");
    assert_eq!(model.lines[1], "   2 beta");

    std::fs::remove_file(&config_path).expect("設定ファイルの削除");
    std::fs::remove_file(&target_path).expect("対象ファイルの削除");
    std::fs::remove_dir_all(&base_dir).expect("テストディレクトリの削除");
}
