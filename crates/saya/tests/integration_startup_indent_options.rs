//! Startup indentation option integration tests.
//!
//! These tests assert the final buffer contents so the startup option path is
//! verified past config collection and through the Vim core editing behavior.

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
    std::env::temp_dir().join(format!("saya-startup-indent-{name}-{nanos}"))
}

/// startup config -> option -> core インデント適用の core/startup 契約検証。
/// 検証主眼は core の option 適用（smartindent x shiftwidth）であり、`\r` は生バイト
/// 直送する。Enter キー押下からの到達性は実バイナリ E2E
/// `enter_key_triggers_autoindent_with_smartindent_through_the_sy_binary`
/// （integration_input_pipeline_e2e.rs）が担保する。
#[test]
fn startup_smartindent_true_indents_after_open_brace() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("smartindent-on-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.options.expandtab = true;
            saya.options.shiftwidth = 4;
            saya.options.smartindent = true;
        "#,
    )
    .expect("config file");

    let mut outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with smartindent config");

    assert!(outcome.startup_registry.options.smartindent);

    outcome.core_bridge.dispatch_key("i").expect("insert mode");
    outcome
        .core_bridge
        .dispatch_key("if (enabled) {")
        .expect("insert opening block");
    outcome
        .core_bridge
        .dispatch_key("\r")
        .expect("insert newline");
    outcome
        .core_bridge
        .dispatch_key("return 1;")
        .expect("insert body");

    assert_eq!(
        outcome.core_bridge.buffer_text(),
        "if (enabled) {\n    return 1;\n",
        "smartindent should apply shiftwidth indentation after an opening brace"
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

/// startup config -> option -> core インデント適用の core/startup 契約検証。
/// 検証主眼は core の option 適用（smartindent=false で素の改行）であり、`\r` は生バイト
/// 直送する。Enter キー押下からの到達性は実バイナリ E2E
/// `enter_key_triggers_autoindent_with_smartindent_through_the_sy_binary`
/// （integration_input_pipeline_e2e.rs）が担保する。
#[test]
fn startup_smartindent_false_keeps_plain_newline_after_open_brace() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("smartindent-off-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.options.expandtab = true;
            saya.options.shiftwidth = 4;
            saya.options.smartindent = false;
        "#,
    )
    .expect("config file");

    let mut outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with smartindent disabled");

    assert!(!outcome.startup_registry.options.smartindent);

    outcome.core_bridge.dispatch_key("i").expect("insert mode");
    outcome
        .core_bridge
        .dispatch_key("if (disabled) {")
        .expect("insert opening block");
    outcome
        .core_bridge
        .dispatch_key("\r")
        .expect("insert newline");
    outcome
        .core_bridge
        .dispatch_key("return 1;")
        .expect("insert body");

    assert_eq!(
        outcome.core_bridge.buffer_text(),
        "if (disabled) {\nreturn 1;\n",
        "disabled smartindent should leave Enter as a plain newline"
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

/// startup config -> option -> core インデント適用の core/startup 契約検証。
/// 検証主眼は core の option 適用（go ftplugin による noexpandtab override）であり、
/// `\r` は生バイト直送する。Enter キー押下からの到達性は実バイナリ E2E
/// `enter_key_triggers_autoindent_with_smartindent_through_the_sy_binary`
/// （integration_input_pipeline_e2e.rs）が担保する。
#[test]
fn startup_go_ftplugin_overrides_global_expandtab_for_tab_indentation() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("main").with_extension("go");
    let config_path = unique_path("go-ftplugin-init.ts");
    std::fs::write(&target_path, "").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            saya.options.tabstop = 4;
            saya.options.expandtab = true;
            saya.options.shiftwidth = 4;
            saya.options.softtabstop = 4;
            saya.options.smartindent = true;
        "#,
    )
    .expect("config file");

    let mut outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with Go ftplugin settings");
    let session_state = outcome.editor_session_state();
    assert_eq!(session_state.filetype(), Some("go"));
    let status_model = project(&ProjectionInput::new(
        &outcome.core_bridge.snapshot(),
        &session_state,
        None,
    ));
    assert!(
        status_model.status_line.contains("go"),
        "Go ftplugin should expose filetype to the status line: {:?}",
        status_model.status_line
    );

    outcome.core_bridge.dispatch_key("i").expect("insert mode");
    outcome
        .core_bridge
        .dispatch_key("func main() {")
        .expect("insert opening block");
    outcome
        .core_bridge
        .dispatch_key("\r")
        .expect("insert newline");
    outcome
        .core_bridge
        .dispatch_key("return")
        .expect("insert body");

    assert_eq!(
        outcome.core_bridge.buffer_text(),
        "func main() {\n\treturn\n",
        "Go ftplugin should use noexpandtab with tab indentation even when global expandtab is true"
    );

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

/// startup config -> option -> core インデント適用の core/startup 契約検証。
/// 検証主眼は core の option 適用（ftplugin 無効化で global expandtab を維持）であり、
/// `\r` は生バイト直送する。Enter キー押下からの到達性は実バイナリ E2E
/// `enter_key_triggers_autoindent_with_smartindent_through_the_sy_binary`
/// （integration_input_pipeline_e2e.rs）が担保する。
#[test]
fn startup_ftplugin_can_be_disabled_from_config() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("main").with_extension("go");
    let config_path = unique_path("go-ftplugin-disabled-init.ts");
    std::fs::write(&target_path, "").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            saya.options.expandtab = true;
            saya.options.shiftwidth = 4;
            saya.options.softtabstop = 4;
            saya.options.smartindent = true;
            saya.ftplugin.enabled = false;
        "#,
    )
    .expect("config file");

    let mut outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with ftplugin disabled");
    let session_state = outcome.editor_session_state();
    assert_eq!(session_state.filetype(), None);

    outcome.core_bridge.dispatch_key("i").expect("insert mode");
    outcome
        .core_bridge
        .dispatch_key("func main() {")
        .expect("insert opening block");
    outcome
        .core_bridge
        .dispatch_key("\r")
        .expect("insert newline");
    outcome
        .core_bridge
        .dispatch_key("return")
        .expect("insert body");

    assert_eq!(
        outcome.core_bridge.buffer_text(),
        "func main() {\n    return\n",
        "disabled ftplugin should leave global expandtab indentation in control"
    );

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

/// startup config -> option -> core インデント適用の core/startup 契約検証。
/// 検証主眼は core の option 適用（拡張子から引いた custom ftplugin override）であり、
/// `\r` は生バイト直送する。Enter キー押下からの到達性は実バイナリ E2E
/// `enter_key_triggers_autoindent_with_smartindent_through_the_sy_binary`
/// （integration_input_pipeline_e2e.rs）が担保する。
#[test]
fn startup_custom_ftplugin_definition_applies_by_extension() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("sample").with_extension("toy");
    let config_path = unique_path("custom-ftplugin-init.ts");
    std::fs::write(&target_path, "").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            saya.options.expandtab = true;
            saya.options.shiftwidth = 4;
            saya.options.softtabstop = 4;
            saya.options.smartindent = true;
            saya.ftplugin.set("toy", {
                extensions: ["toy"],
                options: {
                    expandtab: false,
                    softtabstop: 0,
                    shiftwidth: 0,
                },
            });
        "#,
    )
    .expect("config file");

    let mut outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with custom ftplugin");
    let session_state = outcome.editor_session_state();
    assert_eq!(session_state.filetype(), Some("toy"));

    outcome.core_bridge.dispatch_key("i").expect("insert mode");
    outcome
        .core_bridge
        .dispatch_key("block {")
        .expect("insert opening block");
    outcome
        .core_bridge
        .dispatch_key("\r")
        .expect("insert newline");
    outcome
        .core_bridge
        .dispatch_key("body")
        .expect("insert body");

    assert_eq!(
        outcome.core_bridge.buffer_text(),
        "block {\n\tbody\n",
        "custom ftplugin should apply configured option overrides by extension"
    );

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}
