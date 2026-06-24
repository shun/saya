use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::runtime::startup::PreparedStartupModule;

use super::SayaStartupPhaseRunner;
use super::live_test_support::{SleepingStartupEvaluator, unique_path};
use super::startup_runtime_prepare_test_support::spawn_startup_runtime_prepare_runner;

#[tokio::test(flavor = "current_thread")]
async fn startup_phase_runs_on_worker_boundary_without_blocking_caller() {
    let runner = SayaStartupPhaseRunner::new(Arc::new(SleepingStartupEvaluator));

    let started_at = Instant::now();
    let receipt = runner.begin().expect("startup evaluation should be queued");

    assert!(
        started_at.elapsed() < Duration::from_millis(20),
        "begin should return quickly without waiting for startup evaluation"
    );

    let result = receipt
        .await_result()
        .await
        .expect("startup evaluation result");
    assert_eq!(result, "startup-ready");
}

#[tokio::test(flavor = "current_thread")]
async fn startup_runtime_prepare_runs_on_worker_boundary_without_blocking_caller() {
    let current_dir = unique_path("startup-runtime-cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        r#"
                const tabstop: number = 4;
                saya.options.tabstop = tabstop;
            "#,
    )
    .expect("config file");

    let runner = spawn_startup_runtime_prepare_runner(config_path.clone(), current_dir.clone());

    let started_at = Instant::now();
    let receipt = runner
        .begin()
        .expect("startup runtime evaluation should be queued");

    assert!(
        started_at.elapsed() < Duration::from_millis(20),
        "begin should return quickly without waiting for startup runtime preparation"
    );

    let result: PreparedStartupModule = receipt
        .await_result()
        .await
        .expect("startup runtime evaluation result");
    assert_eq!(result.path, config_path);
    assert_eq!(
        result.specifier.as_str(),
        format!("file://{}/init.ts", current_dir.to_string_lossy())
    );
    assert!(result.executable_source_text.contains("const tabstop = 4;"));
}
