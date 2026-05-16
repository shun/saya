//! 統合テスト: presentation line numbers の headless 検証。
//!
//! startup config による line number projection が host/application 層の
//! presentation で反映されることを確認する。fallback 起動も同じファイル内
//! で検証する。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::app::bootstrap::{launch_test_lock, prepare_launch};
use saya::app::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::presentation::screen_model::{ProjectionInput, project};

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-presentation-line-numbers-{name}-{nanos}"))
}

#[test]
fn presentation_line_numbers_reflect_in_headless_projection_without_conflicting_with_tab_size() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("target.txt");
    let config_path = unique_path("init.ts");
    std::fs::write(&target_path, "ab\tcd\nsecond line\n").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            saya.options.tabstop = 4;
            saya.options.number = true;
            saya.options.numberwidth = 4;
        "#,
    )
    .expect("config file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with typescript config");

    assert_eq!(outcome.initial_tab_size, 4);
    assert!(outcome.initial_line_numbers);
    assert_eq!(outcome.initial_number_width, 4);

    let session_state = outcome.editor_session_state();
    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));

    assert_eq!(session_state.tab_size(), 4);
    assert!(session_state.line_numbers());
    assert_eq!(session_state.number_width(), 4);
    assert_eq!(model.lines[0], "   1 ab  cd");
    assert_eq!(model.lines[1], "   2 second line");

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn presentation_line_numbers_falls_back_to_default_projection_when_config_is_missing() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("fallback-target.txt");
    let config_path = unique_path("missing-init.ts");
    std::fs::write(&target_path, "ab\tcd\n").expect("target file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup should continue with default fallback");

    assert_eq!(outcome.initial_tab_size, 8);
    assert!(!outcome.initial_line_numbers);
    assert_eq!(outcome.initial_number_width, 4);

    let session_state = outcome.editor_session_state();
    let model = project(&ProjectionInput::new(
        &outcome.initial_snapshot,
        &session_state,
        None,
    ));

    assert_eq!(session_state.tab_size(), 8);
    assert!(!session_state.line_numbers());
    assert_eq!(session_state.number_width(), 4);
    assert_eq!(model.lines[0], "ab      cd");

    std::fs::remove_file(&target_path).expect("remove target");
}
