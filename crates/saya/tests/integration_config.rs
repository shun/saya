//! 統合テスト: 設定機能の検証
//!
//! 有効設定の反映と失敗時 fallback が要件を満たすことを確認する。
//! Requirements: 4.1, 4.2, 4.3, 4.4
//!
//! 本番起動経路 `prepare_launch`（`config_resolve` -> `evaluate_bootstrap_capability`
//! -> 実 deno_core ランタイム）を通して「設定ファイル -> 初期 options/状態」を検証する。
//! 旧テストは手書きパーサ専用の dead 関数 `load_and_apply_config` を呼んでおり、deno 経路の
//! 回帰を検出できなかったため、実起動経路ベースへ移植した。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::app::bootstrap::{BootstrapWarning, launch_test_lock, prepare_launch};
use saya::app::cli::{ConfigSource, InputSource, LaunchRequest};

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
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("valid-init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.options.tabstop = 4;
            saya.options.number = true;
        "#,
    )
    .expect("write config");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with valid config");

    assert!(
        outcome.warnings.is_empty(),
        "Warnings should be empty for valid config, got: {:?}",
        outcome.warnings
    );
    assert_eq!(outcome.initial_tab_size, 4);
    assert!(outcome.initial_line_numbers);

    std::fs::remove_file(&config_path).expect("remove config");
}

// ---- 9.5.2: 失敗時の fallback ----

#[test]
fn invalid_config_falls_back_to_defaults_with_warning() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let config_path = unique_path("invalid-init.ts");
    // Vim script 形式は実 deno 経路でも拒否される。
    std::fs::write(&config_path, "set number\n").expect("write config");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup should continue with default fallback");

    assert!(
        !outcome.warnings.is_empty(),
        "Should produce warnings for rejected config"
    );
    assert!(
        outcome.warnings.iter().any(|warning| matches!(
            warning,
            BootstrapWarning::ConfigEvalFailed { path, .. } if path == &config_path
        )),
        "Should report an eval failure warning, got: {:?}",
        outcome.warnings
    );
    assert_eq!(
        outcome.initial_tab_size, 8,
        "Should fallback to default tabstop"
    );
    assert!(
        !outcome.initial_line_numbers,
        "Should fallback to default number"
    );

    std::fs::remove_file(&config_path).expect("remove config");
}

#[test]
fn missing_config_falls_back_to_defaults_with_warning() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let missing_path = unique_path("missing-init.ts");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::File(missing_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup should continue when config file is missing");

    assert!(
        outcome.warnings.iter().any(|warning| matches!(
            warning,
            BootstrapWarning::ConfigLoadFailed { path, .. } if path == &missing_path
        )),
        "Should produce a load-failed warning for missing file, got: {:?}",
        outcome.warnings
    );
    assert_eq!(outcome.initial_tab_size, 8);
    assert!(!outcome.initial_line_numbers);
}

#[test]
fn no_config_input_uses_defaults_without_warning() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // `ConfigSource::Default` を実ホーム非依存で検証するため、`saya/init.ts` を置かない
    // 一時ディレクトリを `default_config_dir` として注入する（hermetic 原則）。
    let config_dir = unique_path("no-config-dir");
    std::fs::create_dir_all(&config_dir).expect("create hermetic config dir");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::Empty,
        config_source: ConfigSource::Default,
        default_config_dir: Some(config_dir.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup should continue without any config");

    assert!(
        outcome.warnings.is_empty(),
        "Should not produce warnings for absent config, got: {:?}",
        outcome.warnings
    );
    assert_eq!(outcome.initial_tab_size, 8);
    assert!(!outcome.initial_line_numbers);

    let _ = std::fs::remove_dir_all(&config_dir);
}
