/// Wave 6.1: startup option `tabSize` の headless 統合テスト。
///
/// 正式な boot flow で startup option が session state と screen projection に
/// 反映されることを確認する。fallback 起動も同じファイル内で検証する。
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::bootstrap::{BootstrapWarning, launch_test_lock, prepare_launch};
use saya::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::screen_model::{ProjectionInput, project};
use vim_core_rs::CoreMode;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-wave6-startup-tab-size-{name}-{nanos}"))
}

#[test]
fn startup_tab_size_reflects_in_headless_boot_projection() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("target.txt");
    let config_path = unique_path("init.ts");

    std::fs::write(&target_path, "a\tb\n").expect("target file");
    std::fs::write(&config_path, "saya.options.tabSize = 4;").expect("config file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with tabSize config");

    assert_eq!(outcome.initial_tab_size, 4);
    assert_eq!(outcome.initial_snapshot.mode, CoreMode::Normal);
    assert!(outcome.warnings.is_empty());

    let session_state = outcome.editor_session_state();
    assert_eq!(session_state.tab_size(), 4);

    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));

    assert_eq!(model.file_name, target_path.display().to_string());
    assert_eq!(model.mode_label, "NORMAL");
    assert_eq!(model.lines, vec!["a   b".to_string()]);

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn startup_tab_size_falls_back_when_config_is_missing() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("fallback-target.txt");
    let missing_config = unique_path("missing-init.ts");

    std::fs::write(&target_path, "a\tb\n").expect("target file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(missing_config.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup should continue with default fallback");

    assert_eq!(outcome.initial_tab_size, 8);
    assert!(outcome.warnings.iter().any(|warning| matches!(
        warning,
        BootstrapWarning::ConfigLoadFailed { path, .. } if path == &missing_config
    )));

    let session_state = outcome.editor_session_state();
    assert_eq!(session_state.tab_size(), 8);

    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));

    assert_eq!(model.file_name, target_path.display().to_string());
    assert_eq!(model.mode_label, "NORMAL");
    assert_eq!(model.lines, vec!["a       b".to_string()]);

    std::fs::remove_file(&target_path).expect("remove target");
}
