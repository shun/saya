/// 統合テスト: 起動フローの検証
///
/// 既存ファイル起動、新規バッファ起動、読込失敗を個別に確認する。
/// 起動失敗時にセッションが中途半端に残らないことを確認する。
/// Requirements: 1.1, 1.2, 1.3, 3.4
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::bootstrap::{BootstrapError, BootstrapWarning, LoadedConfig, prepare_launch};
use saya::cli::{ConfigSource, LaunchRequest, parse_launch_request};
use saya::editor_session::EditorSessionState;
use saya::screen_model::{ProjectionInput, project};
use vim_core_rs::CoreMode;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-integ-startup-{name}-{nanos}"))
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
    assert_eq!(request.target_path, Some(target_path.clone()));

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
    assert_eq!(model.status_message, None);

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
    assert_eq!(request.target_path, None);

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

// ---- 9.1.4: 起動失敗時のセッション残留なし ----

/// 起動失敗後にセッションガードが解放され、再度起動可能であることを確認する。
#[test]
fn session_guard_released_after_startup_failure() {
    let missing_path = unique_path("session-cleanup");

    // 1. 最初の起動試行（失敗する）
    let result = prepare_launch(LaunchRequest {
        target_path: Some(missing_path),
        config_source: ConfigSource::Default,
    });
    assert!(result.is_err(), "起動は失敗すること");

    // 2. セッションガードが解放されているので、再度起動可能
    let outcome = prepare_launch(LaunchRequest {
        target_path: None,
        config_source: ConfigSource::Default,
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
            target_path: None,
            config_source: ConfigSource::Default,
        })
        .expect("起動成功");
        // outcome がスコープを抜けて drop される
    }

    // 2. 再度起動可能であること
    let outcome2 = prepare_launch(LaunchRequest {
        target_path: None,
        config_source: ConfigSource::Default,
    });
    assert!(
        outcome2.is_ok(),
        "前回の outcome drop 後にセッションガードが解放され、再起動できること"
    );
}

// ---- 9.1.5: 設定付き起動の統合フロー ----

/// 設定ファイル付きの起動で config が warning なく読み込まれる。
#[test]
fn startup_with_config_file_loads_without_warning() {
    let config_path = unique_path("config-ok.json");
    std::fs::write(&config_path, "{ \"tabSize\": 4 }").expect("設定ファイルの作成");

    let request = parse_launch_request(["--config", config_path.to_str().unwrap()])
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
