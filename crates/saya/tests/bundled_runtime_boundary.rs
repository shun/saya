mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use saya::features::completion::session::CompletionShowRequest;
use saya::runtime::callback_registry_seed::CallbackRegistrySeed;
use saya::runtime::live::{
    BoxFuture, HostCapabilityBridge, ReadonlyBufferSnapshot, ReadonlyEditorSnapshot,
    ReadonlyWindowSnapshot, RuntimeCommandError, RuntimeMode, SayaLiveRuntime,
};
use saya::runtime::startup::{
    StartupModulePrepareResult, collect_startup_registry, prepare_init_module,
};

fn unique_path(name: &str) -> PathBuf {
    support::temp::unique_temp_path("startup-runtime", name)
}

struct CompletionRuntimeHostBridge {
    shown: Arc<Mutex<Vec<CompletionShowRequest>>>,
    closed: Arc<Mutex<usize>>,
}

impl HostCapabilityBridge for CompletionRuntimeHostBridge {
    fn execute_host_command(&self, name: &str) -> BoxFuture<Result<(), RuntimeCommandError>> {
        let name = name.to_string();
        Box::pin(async move { Err(RuntimeCommandError::UnknownCommand { name }) })
    }

    fn show_completion(
        &self,
        request: CompletionShowRequest,
    ) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        let shown = self.shown.clone();
        Box::pin(async move {
            shown
                .lock()
                .expect("completion requests lock")
                .push(request);
            Ok(true)
        })
    }

    fn close_completion(&self) -> BoxFuture<Result<bool, RuntimeCommandError>> {
        let closed = self.closed.clone();
        Box::pin(async move {
            *closed.lock().expect("completion close count lock") += 1;
            Ok(true)
        })
    }

    fn current_buffer(&self) -> BoxFuture<ReadonlyBufferSnapshot> {
        Box::pin(async move {
            ReadonlyBufferSnapshot {
                id: 42,
                path: Some(PathBuf::from("/workspace/main.go")),
                line_count: 3,
                cursor_row: 0,
                cursor_col: 3,
                current_line: "pri".to_string(),
                text: "pri\nprintln\nprivate\n".to_string(),
            }
        })
    }

    fn current_window(&self) -> BoxFuture<ReadonlyWindowSnapshot> {
        Box::pin(async move { ReadonlyWindowSnapshot { id: 1 } })
    }

    fn current_editor(&self) -> BoxFuture<ReadonlyEditorSnapshot> {
        Box::pin(async move {
            ReadonlyEditorSnapshot {
                mode: RuntimeMode::Insert,
            }
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn bundled_completion_command_runs_after_startup_to_live_runtime_boundary() {
    let current_dir = unique_path("cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    let completion_path =
        saya::support::paths::dev_ts_plugins_dir().join("bundled/completion/index.ts");
    let completion_specifier = completion_path.to_string_lossy();
    std::fs::write(
        &config_path,
        format!(
            r#"
                import {{ createBufferWordSource, setupSayaCompletion }} from "{completion_specifier}";
                setupSayaCompletion({{
                    key: "<C-x>",
                    minPrefixLength: 2,
                    sourceTimeoutMs: 1000,
                    sources: [createBufferWordSource()],
                }});
            "#
        ),
    )
    .expect("config file");

    let prepared = prepare_init_module(&config_path, &current_dir);
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("startup module should prepare: {prepared:?}");
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("startup registry should collect");
    let seed = CallbackRegistrySeed::from_startup_registry(&registry);
    let shown = Arc::new(Mutex::new(Vec::new()));
    let closed = Arc::new(Mutex::new(0));
    let runtime = SayaLiveRuntime::spawn_from_seed(
        Arc::new(CompletionRuntimeHostBridge {
            shown: shown.clone(),
            closed,
        }),
        seed,
    )
    .expect("live runtime should spawn from startup callback seed");

    let receipt = runtime
        .execute_command("completion.trigger")
        .expect("completion trigger should queue");
    receipt
        .await_result()
        .await
        .expect("completion trigger should run in live runtime");

    let shown = shown.lock().expect("completion requests lock");
    assert_eq!(shown.len(), 1);
    let labels = shown[0]
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(labels, vec!["println", "private"]);
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_completion_close_reaches_live_runtime_boundary() {
    let current_dir = unique_path("close-cwd");
    std::fs::create_dir_all(&current_dir).expect("current dir");
    let config_path = current_dir.join("init.ts");
    std::fs::write(
        &config_path,
        r#"
            saya.commands.register("completion.closeForTest", async () => {
                return await saya.completion.close();
            });
        "#,
    )
    .expect("config file");

    let prepared = prepare_init_module(&config_path, &current_dir);
    let StartupModulePrepareResult::Success(module) = prepared else {
        panic!("startup module should prepare: {prepared:?}");
    };
    let registry = collect_startup_registry(&module.executable_source_text)
        .await
        .expect("startup registry should collect");
    let seed = CallbackRegistrySeed::from_startup_registry(&registry);
    let closed = Arc::new(Mutex::new(0));
    let runtime = SayaLiveRuntime::spawn_from_seed(
        Arc::new(CompletionRuntimeHostBridge {
            shown: Arc::new(Mutex::new(Vec::new())),
            closed: closed.clone(),
        }),
        seed,
    )
    .expect("live runtime should spawn from startup callback seed");

    let receipt = runtime
        .execute_command("completion.closeForTest")
        .expect("completion close command should queue");
    receipt
        .await_result()
        .await
        .expect("completion close command should run in live runtime");

    assert_eq!(*closed.lock().expect("completion close count lock"), 1);
}
