use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::bootstrap::{StartupKeymapAction, launch_test_lock, prepare_launch};
use saya::cli::{ConfigSource, InputSource, LaunchRequest};

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-wave6-boot-flow-{name}-{nanos}"))
}

#[test]
fn formal_boot_flow_uses_deno_core_runtime_for_expression_based_startup_config() {
    let _lock = launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let target_path = unique_path("target.txt");
    let config_path = unique_path("init.ts");

    std::fs::write(&target_path, "alpha\n").expect("target file");
    std::fs::write(
        &config_path,
        r#"
            const computedTabSize = 2 + 2;
            const writeCurrent = "writeCurrent";

            saya.options.tabSize = computedTabSize;
            saya.options.lineNumbers = true;
            saya.keymap.set("normal", "<leader>w", saya.commands.execute(writeCurrent));
            saya.commands.register(writeCurrent, () => {
                return saya.commands.execute("write");
            });
            saya.events.on("bufferOpen", (payload) => {
                if (payload.buffer.id > 0) {
                    console.log(payload.buffer.id);
                }
            });
        "#,
    )
    .expect("config file");

    let outcome = prepare_launch(LaunchRequest {
        input_source: InputSource::File(target_path.clone()),
        config_source: ConfigSource::File(config_path.clone()),
        ..LaunchRequest::default()
    })
    .expect("expression-based startup config should boot");

    assert_eq!(outcome.initial_tab_size, 4);
    assert!(outcome.initial_line_numbers);
    assert_eq!(outcome.startup_registry.keymaps.len(), 1);
    assert_eq!(
        outcome.startup_registry.keymaps[0].action,
        StartupKeymapAction::RegisteredCommand("writeCurrent".to_string())
    );
    assert_eq!(outcome.callback_registry.commands().len(), 1);
    assert_eq!(outcome.callback_registry.events().len(), 1);

    std::fs::remove_file(&target_path).expect("remove target");
    std::fs::remove_file(&config_path).expect("remove config");
}
