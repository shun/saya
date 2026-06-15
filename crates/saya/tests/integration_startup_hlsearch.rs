//! Startup hlsearch option integration tests.
//!
//! These tests assert that the startup `saya.options.hlsearch` declaration is
//! collected into the snapshot and applied through the Vim core so search
//! highlighting persists from launch without a manual `:set hlsearch`.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::app::bootstrap::{launch_test_lock, prepare_launch};
use saya::app::cli::{ConfigSource, InputSource, LaunchRequest};
use saya::features::search::query::SearchVisibleQuery;

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-startup-hlsearch-{name}-{nanos}"))
}

fn full_viewport_query() -> SearchVisibleQuery {
    SearchVisibleQuery {
        start_row: 1,
        end_row: 3,
    }
}

/// startup config -> option -> core 検索ハイライト適用の core/startup 契約検証。
/// 検証主眼は `saya.options.hlsearch = true` が snapshot に乗り、Vim core 側の
/// hlsearch 状態が起動直後から有効化されること。
#[test]
fn startup_hlsearch_true_enables_core_search_highlight() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("content");
    let config_path = unique_path("hlsearch-on-init.ts");
    std::fs::write(&target_path, "alpha\nbeta alpha\ngamma alpha\n").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            saya.options.hlsearch = true;
        "#,
    )
    .expect("config file");

    let mut outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with hlsearch enabled");

    assert!(
        outcome.startup_registry.options.hlsearch,
        "startup snapshot should record hlsearch as enabled"
    );

    let state = outcome
        .core_bridge
        .query_visible_search_state(full_viewport_query())
        .expect("visible search state");
    assert!(
        state.hlsearch_enabled,
        "core should report hlsearch enabled after startup application"
    );

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}

/// startup config -> option -> core 検索ハイライト適用の core/startup 契約検証。
/// 検証主眼は hlsearch 未指定（既定）のとき core 側ハイライトが無効のままである
/// こと。
#[test]
fn startup_hlsearch_default_keeps_core_search_highlight_disabled() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("content-default");
    let config_path = unique_path("hlsearch-default-init.ts");
    std::fs::write(&target_path, "alpha\nbeta alpha\ngamma alpha\n").expect("target file");
    std::fs::write(&config_path, "\n").expect("config file");

    let mut outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("startup with default hlsearch");

    assert!(
        !outcome.startup_registry.options.hlsearch,
        "startup snapshot should default hlsearch to disabled"
    );

    let state = outcome
        .core_bridge
        .query_visible_search_state(full_viewport_query())
        .expect("visible search state");
    assert!(
        !state.hlsearch_enabled,
        "core should keep hlsearch disabled when startup does not opt in"
    );

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}
