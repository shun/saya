use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use saya::runtime::config::{StartupRegistry, StartupRegistryEntry};
use saya::runtime::plugin::{
    BundledPluginManifest, LazyIndex, LazyTarget, LockedPlugin, PluginCacheRoot, PluginCommand,
    PluginHost, PluginLockfile, PluginManagerReport, StartupPlan, StartupPlanEntry,
    StartupPlanValidation,
};

fn unique_cache_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-plugin-manager-{name}-{nanos}"))
}

#[test]
fn plugin_host_merges_valid_cached_startup_plan_and_logs_cache_hit() {
    let root = PluginCacheRoot::new(unique_cache_root("startup-plan-hit"));
    let host = PluginHost::new(root.clone());
    host.write_startup_plan(&StartupPlan {
        version: StartupPlan::CURRENT_VERSION,
        source_hash: "hash-a".to_string(),
        entries: vec![StartupPlanEntry::Command {
            name: "plugin.ready".to_string(),
            callback_source: "() => console.info(\"ready\")".to_string(),
        }],
    })
    .expect("startup plan should be written");

    let mut registry = StartupRegistry::default();
    let report = host
        .merge_cached_startup_plan(
            &mut registry,
            StartupPlanValidation::SourceHash("hash-a".to_string()),
        )
        .expect("startup plan should merge");

    assert_eq!(report.loaded_entries, 1);
    assert!(
        report
            .logs
            .iter()
            .any(|line| line.contains("[saya-plugin-host][cache] startup plan cache hit"))
    );
    assert!(matches!(
        registry.entries(),
        [StartupRegistryEntry::Command { name, .. }] if name == "plugin.ready"
    ));
}

#[test]
fn plugin_host_rejects_stale_startup_plan_and_logs_invalidation() {
    let root = PluginCacheRoot::new(unique_cache_root("startup-plan-stale"));
    let host = PluginHost::new(root);
    host.write_startup_plan(&StartupPlan {
        version: StartupPlan::CURRENT_VERSION,
        source_hash: "old-hash".to_string(),
        entries: vec![StartupPlanEntry::Warning {
            message: "must not merge".to_string(),
        }],
    })
    .expect("startup plan should be written");

    let mut registry = StartupRegistry::default();
    let report = host
        .merge_cached_startup_plan(
            &mut registry,
            StartupPlanValidation::SourceHash("new-hash".to_string()),
        )
        .expect("stale startup plan should be reported, not fatal");

    assert_eq!(report.loaded_entries, 0);
    assert!(registry.entries().is_empty());
    assert!(
        report
            .logs
            .iter()
            .any(|line| line.contains("startup plan invalidated: reason=source_hash_mismatch"))
    );
}

#[test]
fn plugin_host_generates_lazy_placeholders_that_log_trigger_bridge() {
    let root = PluginCacheRoot::new(unique_cache_root("lazy-index"));
    let host = PluginHost::new(root);
    let mut commands = BTreeMap::new();
    commands.insert(
        "GitStatus".to_string(),
        LazyTarget {
            plugin: "git-tools".to_string(),
            module: "plugins/git-tools.ts".to_string(),
            export_name: "setup".to_string(),
        },
    );
    let mut events = BTreeMap::new();
    events.insert(
        "bufferOpen".to_string(),
        vec![LazyTarget {
            plugin: "buffer-tools".to_string(),
            module: "plugins/buffer-tools.ts".to_string(),
            export_name: "setup".to_string(),
        }],
    );
    host.write_lazy_index(&LazyIndex {
        version: LazyIndex::CURRENT_VERSION,
        commands,
        events,
    })
    .expect("lazy index should be written");

    let (registry, report) = host
        .startup_registry_from_lazy_index()
        .expect("lazy placeholders should be generated");

    assert_eq!(report.loaded_entries, 2);
    assert!(
        report
            .logs
            .iter()
            .any(|line| line.contains("[saya-plugin-host][lazy] registered lazy command"))
    );
    assert!(registry.entries().iter().any(|entry| matches!(
        entry,
        StartupRegistryEntry::Command {
            name,
            callback_source
        } if name == "GitStatus"
            && callback_source.contains("[saya-plugin-host][lazy] command trigger")
            && callback_source.contains("saya.plugins.loadLazy")
    )));
    assert!(registry.entries().iter().any(|entry| matches!(
        entry,
        StartupRegistryEntry::Event {
            name,
            callback_source
        } if name == "bufferOpen"
            && callback_source.contains("[saya-plugin-host][lazy] event trigger")
            && callback_source.contains("saya.plugins.loadLazy")
    )));
}

#[test]
fn plugin_host_generates_lazy_placeholders_from_bundled_manifests() {
    let root = PluginCacheRoot::new(unique_cache_root("bundled-fallback"));
    let host = PluginHost::new(root);

    let (registry, report) = host
        .startup_registry_from_bundled_manifests()
        .expect("bundled manifests should generate fallback placeholders");

    assert!(report.loaded_entries >= 2);
    assert!(
        report
            .logs
            .iter()
            .any(|line| line.contains("[PERF][plugin-host] bundled_manifest_fallback"))
    );
    assert!(registry.entries().iter().any(|entry| matches!(
        entry,
        StartupRegistryEntry::Command {
            name,
            callback_source
        } if name == "dired.open"
            && callback_source.contains("[saya-plugin-host][lazy] command trigger")
            && callback_source.contains("plugins/bundled/dired/index.ts")
    )));
    assert!(registry.entries().iter().any(|entry| matches!(
        entry,
        StartupRegistryEntry::Command { name, .. } if name == "lsp.start"
    )));
}

#[test]
fn plugin_host_reports_external_disabled_and_sync_required_when_cache_is_missing() {
    let root = PluginCacheRoot::new(unique_cache_root("external-disabled"));
    let host = PluginHost::new(root);

    let report = host.external_disabled_report("cache_missing");

    assert_eq!(report.loaded_entries, 0);
    assert!(
        report
            .logs
            .iter()
            .any(|line| line.contains("external plugins disabled: reason=cache_missing"))
    );
    assert!(
        report
            .logs
            .iter()
            .any(|line| line.contains("sync required: reason=cache_missing"))
    );
}

#[test]
fn bundled_manifest_schema_matches_distribution_files() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugins/bundled");
    let host = PluginHost::new(PluginCacheRoot::new(unique_cache_root("schema")));
    let manifests = host
        .read_bundled_manifests_from(root)
        .expect("bundled manifests should parse");

    let names = manifests
        .iter()
        .map(|manifest: &BundledPluginManifest| manifest.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"dired"));
    assert!(names.contains(&"lsp-client"));
    assert!(
        manifests
            .iter()
            .all(|manifest| manifest.module.starts_with("plugins/bundled/")),
        "bundled manifests should point at the new plugin layout"
    );
}

#[test]
fn plugin_operations_report_cache_artifacts_and_append_operation_logs() {
    let root = PluginCacheRoot::new(unique_cache_root("ops"));
    let host = PluginHost::new(root.clone());
    host.write_lockfile(&PluginLockfile {
        version: PluginLockfile::CURRENT_VERSION,
        plugins: vec![LockedPlugin {
            name: "local-tools".to_string(),
            source: "local:plugins/local-tools.ts".to_string(),
            revision: "workspace".to_string(),
            depends: vec![],
            before: vec![],
            after: vec![],
        }],
    })
    .expect("lockfile should be written");

    let report = host
        .run_operation(PluginCommand::List)
        .expect("list should inspect cache artifacts");

    assert!(matches!(
        report,
        PluginManagerReport::List {
            plugin_count: 1,
            ..
        }
    ));
    let operation_log = std::fs::read_to_string(root.operations_log_path())
        .expect("operation log should be written");
    assert!(operation_log.contains("[saya-plugin-manager][operation] list"));
    assert!(operation_log.contains("plugin_count=1"));
}

#[test]
fn plugin_sync_regenerates_bundled_startup_and_lazy_artifacts_without_locking_bundled_plugins() {
    let root = PluginCacheRoot::new(unique_cache_root("sync-bundled"));
    let host = PluginHost::new(root.clone());

    let report = host
        .run_operation(PluginCommand::Sync)
        .expect("sync should regenerate host-readable bundled artifacts");

    assert!(matches!(
        report,
        PluginManagerReport::Sync {
            plugin_count: 0,
            ..
        }
    ));
    let lazy_index = host
        .read_lazy_index()
        .expect("lazy index should be readable")
        .expect("lazy index should be written");
    assert!(lazy_index.commands.contains_key("dired.open"));
    assert!(lazy_index.commands.contains_key("lsp.start"));

    let lockfile = host
        .read_lockfile()
        .expect("lockfile should be readable")
        .expect("lockfile should be written");
    assert!(
        lockfile.plugins.is_empty(),
        "bundled plugins must not become install targets"
    );

    let operation_log = std::fs::read_to_string(root.operations_log_path())
        .expect("operation log should be written");
    assert!(operation_log.contains("[saya-plugin-manager][operation] sync"));
    assert!(operation_log.contains("regenerated bundled artifacts"));
}
