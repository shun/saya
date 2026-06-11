//! プラグイン startup cache のマージ処理を担当する。

use super::*;

pub(super) fn merge_plugin_startup_cache(
    loaded_config: &LoadedConfig,
    registry: &mut StartupRegistry,
) {
    let host = PluginHost::default_from_env();
    let validation = match loaded_config {
        LoadedConfig::Default => StartupPlanValidation::Any,
        LoadedConfig::File { source, .. } => {
            StartupPlanValidation::SourceHash(source_hash_for_startup_cache(source))
        }
    };

    let mut loaded_plugin_entries = 0usize;
    match host.merge_cached_startup_plan(registry, validation) {
        Ok(report) => {
            loaded_plugin_entries += report.loaded_entries;
            log::debug!(
                "[bootstrap][plugin-host] startup plan merge completed: loaded_entries={}, cache_root={}",
                report.loaded_entries,
                host.root().path().display()
            );
        }
        Err(error) => {
            log::debug!(
                "[bootstrap][plugin-host] startup plan merge failed and was reported as warning: error={}",
                error
            );
            registry.push(StartupRegistryEntry::Warning {
                message: format!("plugin startup cache failed: {error}"),
            });
        }
    }

    match host.startup_registry_from_lazy_index() {
        Ok((lazy_registry, report)) => {
            loaded_plugin_entries += report.loaded_entries;
            for entry in lazy_registry {
                push_plugin_registry_entry_if_unclaimed(registry, entry);
            }
            log::debug!(
                "[bootstrap][plugin-host] lazy index merge completed: loaded_entries={}, cache_root={}",
                report.loaded_entries,
                host.root().path().display()
            );
        }
        Err(error) => {
            log::debug!(
                "[bootstrap][plugin-host] lazy index merge failed and was reported as warning: error={}",
                error
            );
            registry.push(StartupRegistryEntry::Warning {
                message: format!("plugin lazy cache failed: {error}"),
            });
        }
    }

    if loaded_plugin_entries == 0 {
        let disabled = host.external_disabled_report("cache_missing");
        for line in disabled.logs {
            registry.push(StartupRegistryEntry::Warning { message: line });
        }
        match host.startup_registry_from_bundled_manifests() {
            Ok((bundled_registry, report)) => {
                for entry in bundled_registry {
                    push_plugin_registry_entry_if_unclaimed(registry, entry);
                }
                log::debug!(
                    "[bootstrap][plugin-host] bundled manifest fallback completed: loaded_entries={}, cache_root={}",
                    report.loaded_entries,
                    host.root().path().display()
                );
            }
            Err(error) => {
                log::debug!(
                    "[bootstrap][plugin-host] bundled manifest fallback failed and was reported as warning: error={}",
                    error
                );
                registry.push(StartupRegistryEntry::Warning {
                    message: format!("plugin bundled manifest fallback failed: {error}"),
                });
            }
        }
    }
}

pub(super) fn push_plugin_registry_entry_if_unclaimed(
    registry: &mut StartupRegistry,
    entry: StartupRegistryEntry,
) {
    let claimed = match &entry {
        StartupRegistryEntry::Command { name, .. } => registry.entries().iter().any(|existing| {
            matches!(existing, StartupRegistryEntry::Command { name: existing_name, .. } if existing_name == name)
        }),
        StartupRegistryEntry::Event { name, .. } => registry.entries().iter().any(|existing| {
            matches!(existing, StartupRegistryEntry::Event { name: existing_name, .. } if existing_name == name)
        }),
        _ => false,
    };
    if claimed {
        log::debug!(
            "[bootstrap][plugin-host] skipped plugin registry entry because startup config already registered same command/event: {:?}",
            entry
        );
        return;
    }
    registry.push(entry);
}

pub(super) fn source_hash_for_startup_cache(source: &str) -> String {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
