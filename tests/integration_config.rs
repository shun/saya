//! 統合テスト: 設定機能の検証
//!
//! 有効設定の反映と失敗時 fallback が要件を満たすことを確認する。
//! Requirements: 4.1, 4.2, 4.3, 4.4

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::runtime::config::{ConfigInput, load_and_apply_config};

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-integ-config-{name}-{nanos}"))
}

// ---- 9.5.1: 有効設定の反映 ----

#[test]
fn valid_config_applied_correctly() {
    let config_path = unique_path("valid");
    let json_content = r#"{
        "tabstop": 4,
        "lineNumbers": true
    }"#;
    std::fs::write(&config_path, json_content).unwrap();

    let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(config_path.clone()));

    assert!(
        warnings.is_empty(),
        "Warnings should be empty for valid config"
    );
    assert_eq!(state.tab_size, 4);
    assert!(state.line_numbers);
}

// ---- 9.5.2: 失敗時の fallback ----

#[test]
fn invalid_config_falls_back_to_defaults_with_warning() {
    let config_path = unique_path("invalid");
    let invalid_content = "set number"; // Vim script is rejected
    std::fs::write(&config_path, invalid_content).unwrap();

    let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(config_path.clone()));

    assert!(!warnings.is_empty(), "Should produce warnings");
    assert_eq!(state.tab_size, 8, "Should fallback to default tabstop");
    assert!(
        !state.line_numbers,
        "Should fallback to default lineNumbers"
    );
}

#[test]
fn missing_config_falls_back_to_defaults_with_warning() {
    let missing_path = unique_path("missing");

    let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(missing_path.clone()));

    assert!(
        !warnings.is_empty(),
        "Should produce warnings for missing file"
    );
    assert_eq!(state.tab_size, 8);
    assert!(!state.line_numbers);
}

#[test]
fn no_config_input_uses_defaults_without_warning() {
    let (state, warnings) = load_and_apply_config(&ConfigInput::None);

    assert!(
        warnings.is_empty(),
        "Should not produce warnings for None config"
    );
    assert_eq!(state.tab_size, 8);
    assert!(!state.line_numbers);
}
