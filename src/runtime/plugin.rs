use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::runtime::config::{
    StartupPluginDeclaration, StartupPluginSource, StartupRegistry, StartupRegistryEntry,
};

const CACHE_SUBDIR: &str = "plugins";
const LOCKFILE_NAME: &str = "plugin-lock.json";
const STARTUP_PLAN_NAME: &str = "startup-plan.json";
const LAZY_INDEX_NAME: &str = "lazy-index.json";
const OPERATIONS_LOG_NAME: &str = "operations.log";
const BUNDLED_PLUGIN_MANIFEST_DIR: &str = "plugins/bundled";
const BUNDLED_PLUGIN_MANIFEST_NAME: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCacheRoot {
    path: PathBuf,
}

impl PluginCacheRoot {
    pub fn new(path: PathBuf) -> Self {
        log::debug!(
            "[saya-plugin-host][cache] create plugin cache root: root={}",
            path.display()
        );
        Self { path }
    }

    pub fn default_from_env() -> Self {
        if let Some(path) = env::var_os("SAYA_CACHE_DIR") {
            return Self::new(PathBuf::from(path));
        }

        let root = env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".cache")
            .join("saya");
        Self::new(root)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn plugins_dir(&self) -> PathBuf {
        self.path.join(CACHE_SUBDIR)
    }

    pub fn lockfile_path(&self) -> PathBuf {
        self.plugins_dir().join(LOCKFILE_NAME)
    }

    pub fn startup_plan_path(&self) -> PathBuf {
        self.plugins_dir().join(STARTUP_PLAN_NAME)
    }

    pub fn lazy_index_path(&self) -> PathBuf {
        self.plugins_dir().join(LAZY_INDEX_NAME)
    }

    pub fn operations_log_path(&self) -> PathBuf {
        self.plugins_dir().join(OPERATIONS_LOG_NAME)
    }

    fn ensure_plugins_dir(&self) -> Result<(), PluginHostError> {
        fs::create_dir_all(self.plugins_dir()).map_err(|error| PluginHostError::Io {
            path: self.plugins_dir(),
            message: error.to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupPlan {
    pub version: u32,
    pub source_hash: String,
    pub entries: Vec<StartupPlanEntry>,
}

impl StartupPlan {
    pub const CURRENT_VERSION: u32 = 1;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StartupPlanEntry {
    Command {
        name: String,
        callback_source: String,
    },
    Event {
        name: String,
        callback_source: String,
    },
    Warning {
        message: String,
    },
}

impl From<StartupPlanEntry> for StartupRegistryEntry {
    fn from(entry: StartupPlanEntry) -> Self {
        match entry {
            StartupPlanEntry::Command {
                name,
                callback_source,
            } => Self::Command {
                name,
                callback_source,
            },
            StartupPlanEntry::Event {
                name,
                callback_source,
            } => Self::Event {
                name,
                callback_source,
            },
            StartupPlanEntry::Warning { message } => Self::Warning { message },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupPlanValidation {
    Any,
    SourceHash(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LazyIndex {
    pub version: u32,
    pub commands: BTreeMap<String, LazyTarget>,
    pub events: BTreeMap<String, Vec<LazyTarget>>,
}

impl LazyIndex {
    pub const CURRENT_VERSION: u32 = 1;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LazyTarget {
    pub plugin: String,
    pub module: String,
    pub export_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundledPluginManifest {
    pub version: u32,
    pub name: String,
    pub module: String,
    #[serde(default = "default_setup_export")]
    pub setup: String,
    #[serde(default)]
    pub lazy: BundledPluginLazyManifest,
}

impl BundledPluginManifest {
    pub const CURRENT_VERSION: u32 = 1;
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundledPluginLazyManifest {
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub events: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginLockfile {
    pub version: u32,
    pub plugins: Vec<LockedPlugin>,
}

impl PluginLockfile {
    pub const CURRENT_VERSION: u32 = 1;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedPlugin {
    pub name: String,
    pub source: String,
    pub revision: String,
    pub depends: Vec<String>,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCommand {
    Sync,
    Update,
    List,
    Clean,
    Doctor,
}

impl PluginCommand {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "sync" => Some(Self::Sync),
            "update" => Some(Self::Update),
            "list" => Some(Self::List),
            "clean" => Some(Self::Clean),
            "doctor" => Some(Self::Doctor),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Update => "update",
            Self::List => "list",
            Self::Clean => "clean",
            Self::Doctor => "doctor",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHostReport {
    pub loaded_entries: usize,
    pub logs: Vec<String>,
}

impl PluginHostReport {
    fn empty(logs: Vec<String>) -> Self {
        Self {
            loaded_entries: 0,
            logs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginManagerReport {
    Sync {
        plugin_count: usize,
        logs: Vec<String>,
    },
    Update {
        plugin_count: usize,
        logs: Vec<String>,
    },
    List {
        plugin_count: usize,
        plugins: Vec<String>,
        logs: Vec<String>,
    },
    Clean {
        removed_files: usize,
        logs: Vec<String>,
    },
    Doctor {
        ok: bool,
        logs: Vec<String>,
    },
}

impl PluginManagerReport {
    pub fn logs(&self) -> &[String] {
        match self {
            Self::Sync { logs, .. }
            | Self::Update { logs, .. }
            | Self::List { logs, .. }
            | Self::Clean { logs, .. }
            | Self::Doctor { logs, .. } => logs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginHostError {
    Io { path: PathBuf, message: String },
    Json { path: PathBuf, message: String },
    Operation { message: String },
}

impl fmt::Display for PluginHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => write!(formatter, "{}: {}", path.display(), message),
            Self::Json { path, message } => write!(formatter, "{}: {}", path.display(), message),
            Self::Operation { message } => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for PluginHostError {}

#[derive(Debug, Clone)]
pub struct PluginHost {
    root: PluginCacheRoot,
}

impl PluginHost {
    pub fn new(root: PluginCacheRoot) -> Self {
        Self { root }
    }

    pub fn default_from_env() -> Self {
        Self::new(PluginCacheRoot::default_from_env())
    }

    pub fn root(&self) -> &PluginCacheRoot {
        &self.root
    }

    pub fn write_startup_plan(&self, plan: &StartupPlan) -> Result<(), PluginHostError> {
        self.write_json(self.root.startup_plan_path(), plan)
    }

    pub fn write_lazy_index(&self, index: &LazyIndex) -> Result<(), PluginHostError> {
        self.write_json(self.root.lazy_index_path(), index)
    }

    pub fn write_lockfile(&self, lockfile: &PluginLockfile) -> Result<(), PluginHostError> {
        self.write_json(self.root.lockfile_path(), lockfile)
    }

    pub fn read_startup_plan(&self) -> Result<Option<StartupPlan>, PluginHostError> {
        self.read_json(self.root.startup_plan_path())
    }

    pub fn read_lazy_index(&self) -> Result<Option<LazyIndex>, PluginHostError> {
        self.read_json(self.root.lazy_index_path())
    }

    pub fn read_lockfile(&self) -> Result<Option<PluginLockfile>, PluginHostError> {
        self.read_json(self.root.lockfile_path())
    }

    pub fn read_bundled_manifests(&self) -> Result<Vec<BundledPluginManifest>, PluginHostError> {
        self.read_bundled_manifests_from(bundled_manifest_dir())
    }

    pub fn read_bundled_manifests_from(
        &self,
        manifest_dir: PathBuf,
    ) -> Result<Vec<BundledPluginManifest>, PluginHostError> {
        let entries = match fs::read_dir(&manifest_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(PluginHostError::Io {
                    path: manifest_dir,
                    message: error.to_string(),
                });
            }
        };

        let mut manifests = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| PluginHostError::Io {
                path: manifest_dir.clone(),
                message: error.to_string(),
            })?;
            let manifest_path = entry.path().join(BUNDLED_PLUGIN_MANIFEST_NAME);
            if !manifest_path.is_file() {
                continue;
            }
            let Some(manifest) = self.read_json::<BundledPluginManifest>(manifest_path.clone())?
            else {
                continue;
            };
            if manifest.version != BundledPluginManifest::CURRENT_VERSION {
                let logs = vec![format!(
                    "[saya-plugin-host] bundled manifest skipped: reason=version_mismatch path={} expected={} actual={}",
                    manifest_path.display(),
                    BundledPluginManifest::CURRENT_VERSION,
                    manifest.version
                )];
                emit_logs(&logs);
                continue;
            }
            manifests.push(manifest);
        }
        manifests.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(manifests)
    }

    pub fn merge_cached_startup_plan(
        &self,
        registry: &mut StartupRegistry,
        validation: StartupPlanValidation,
    ) -> Result<PluginHostReport, PluginHostError> {
        let started = Instant::now();
        let Some(plan) = self.read_startup_plan()? else {
            let logs = vec![format!(
                "[saya-plugin-host][cache] startup plan cache miss: path={}",
                self.root.startup_plan_path().display()
            )];
            emit_logs(&logs);
            return Ok(PluginHostReport::empty(logs));
        };

        if plan.version != StartupPlan::CURRENT_VERSION {
            let logs = vec![format!(
                "[saya-plugin-host][cache] startup plan invalidated: reason=version_mismatch expected={} actual={}",
                StartupPlan::CURRENT_VERSION,
                plan.version
            )];
            emit_logs(&logs);
            return Ok(PluginHostReport::empty(logs));
        }

        if let StartupPlanValidation::SourceHash(expected) = validation
            && plan.source_hash != expected
        {
            let logs = vec![format!(
                "[saya-plugin-host][cache] startup plan invalidated: reason=source_hash_mismatch expected={} actual={}",
                expected, plan.source_hash
            )];
            emit_logs(&logs);
            return Ok(PluginHostReport::empty(logs));
        }

        let entry_count = plan.entries.len();
        for entry in plan.entries {
            registry.push(entry.into());
        }
        let logs = vec![format!(
            "[PERF][plugin-host] cache_hit elapsed_ms={} [saya-plugin-host][cache] startup plan cache hit: path={}, entry_count={}",
            started.elapsed().as_millis(),
            self.root.startup_plan_path().display(),
            entry_count
        )];
        emit_logs(&logs);
        Ok(PluginHostReport {
            loaded_entries: entry_count,
            logs,
        })
    }

    pub fn startup_registry_from_lazy_index(
        &self,
    ) -> Result<(StartupRegistry, PluginHostReport), PluginHostError> {
        let Some(index) = self.read_lazy_index()? else {
            let logs = vec![format!(
                "[saya-plugin-host][lazy] lazy index cache miss: path={}",
                self.root.lazy_index_path().display()
            )];
            emit_logs(&logs);
            return Ok((StartupRegistry::default(), PluginHostReport::empty(logs)));
        };

        if index.version != LazyIndex::CURRENT_VERSION {
            let logs = vec![format!(
                "[saya-plugin-host][lazy] lazy index invalidated: reason=version_mismatch expected={} actual={}",
                LazyIndex::CURRENT_VERSION,
                index.version
            )];
            emit_logs(&logs);
            return Ok((StartupRegistry::default(), PluginHostReport::empty(logs)));
        }

        let mut entries = Vec::new();
        let mut logs = Vec::new();
        for (name, target) in index.commands {
            logs.push(format!(
                "[saya-plugin-host][lazy] registered lazy command: command={}, plugin={}, module={}",
                name, target.plugin, target.module
            ));
            entries.push(StartupRegistryEntry::Command {
                name: name.clone(),
                callback_source: lazy_callback_source("command", &name, &target),
            });
        }
        for (name, targets) in index.events {
            for target in targets {
                logs.push(format!(
                    "[saya-plugin-host][lazy] registered lazy event: event={}, plugin={}, module={}",
                    name, target.plugin, target.module
                ));
                entries.push(StartupRegistryEntry::Event {
                    name: name.clone(),
                    callback_source: lazy_callback_source("event", &name, &target),
                });
            }
        }
        emit_logs(&logs);
        let loaded_entries = entries.len();
        Ok((
            StartupRegistry::from_entries(entries),
            PluginHostReport {
                loaded_entries,
                logs,
            },
        ))
    }

    pub fn startup_registry_from_bundled_manifests(
        &self,
    ) -> Result<(StartupRegistry, PluginHostReport), PluginHostError> {
        let started = Instant::now();
        let manifests = self.read_bundled_manifests()?;
        let (registry, loaded_entries) = registry_from_bundled_manifests(&manifests);
        let bundled = manifests
            .iter()
            .map(|manifest| manifest.name.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let logs = vec![format!(
            "[PERF][plugin-host] bundled_manifest_fallback elapsed_ms={} bundled={} placeholders={}",
            started.elapsed().as_millis(),
            bundled,
            loaded_entries
        )];
        emit_logs(&logs);
        Ok((
            registry,
            PluginHostReport {
                loaded_entries,
                logs,
            },
        ))
    }

    pub fn external_disabled_report(&self, reason: &str) -> PluginHostReport {
        let logs = vec![
            format!("[saya-plugin-host] external plugins disabled: reason={reason}"),
            format!("[saya-plugin-manager] sync required: reason={reason}"),
        ];
        emit_logs(&logs);
        PluginHostReport::empty(logs)
    }

    pub fn sync_startup_plugin_declarations(
        &self,
        registry: &StartupRegistry,
        source_hash: String,
    ) -> Result<PluginManagerReport, PluginHostError> {
        self.root.ensure_plugins_dir()?;
        let use_declarations = registry
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                StartupRegistryEntry::PluginUse { declaration } => Some(declaration.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let lazy_declarations = registry
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                StartupRegistryEntry::PluginLazy { declaration } => Some(declaration.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let artifacts = self.artifacts_from_startup_plugin_declarations(
            &use_declarations,
            &lazy_declarations,
            source_hash,
        )?;
        let plugin_count = artifacts.lockfile.plugins.len();
        self.write_lockfile(&artifacts.lockfile)?;
        self.write_startup_plan(&artifacts.startup_plan)?;
        self.write_lazy_index(&artifacts.lazy_index)?;

        let mut logs = artifacts.logs;
        logs.push(format!(
            "[saya-plugin-manager][operation] sync startup declarations: cache_root={}, plugin_count={plugin_count}",
            self.root.path().display()
        ));
        let report = PluginManagerReport::Sync { plugin_count, logs };
        self.append_operation_logs(report.logs())?;
        emit_logs(report.logs());
        Ok(report)
    }

    pub fn run_operation(
        &self,
        command: PluginCommand,
    ) -> Result<PluginManagerReport, PluginHostError> {
        self.root.ensure_plugins_dir()?;
        let lockfile = self.read_lockfile()?.unwrap_or(PluginLockfile {
            version: PluginLockfile::CURRENT_VERSION,
            plugins: Vec::new(),
        });
        let plugin_count = lockfile.plugins.len();
        let base_log = format!(
            "[saya-plugin-manager][operation] {}: cache_root={}, plugin_count={}",
            command.as_str(),
            self.root.path().display(),
            plugin_count
        );
        let mut logs = vec![base_log];

        let report = match command {
            PluginCommand::Sync => {
                let bundled_count = self.sync_bundled_artifacts()?;
                logs.push(format!(
                    "[saya-plugin-manager][operation] sync regenerated bundled artifacts: bundled_count={bundled_count}"
                ));
                PluginManagerReport::Sync { plugin_count, logs }
            }
            PluginCommand::Update => {
                logs.push(
                    "[saya-plugin-manager][operation] update delegated to TypeScript manager"
                        .to_string(),
                );
                PluginManagerReport::Update { plugin_count, logs }
            }
            PluginCommand::List => {
                let plugins = lockfile
                    .plugins
                    .into_iter()
                    .map(|plugin| plugin.name)
                    .collect();
                PluginManagerReport::List {
                    plugin_count,
                    plugins,
                    logs,
                }
            }
            PluginCommand::Clean => {
                let removed_files = remove_optional_file(self.root.startup_plan_path())?
                    + remove_optional_file(self.root.lazy_index_path())?;
                logs.push(format!(
                    "[saya-plugin-manager][operation] clean removed_files={removed_files}"
                ));
                PluginManagerReport::Clean {
                    removed_files,
                    logs,
                }
            }
            PluginCommand::Doctor => {
                let ok = self
                    .root
                    .lockfile_path()
                    .starts_with(self.root.plugins_dir());
                logs.push(format!(
                    "[saya-plugin-manager][operation] doctor cache_boundary_ok={ok}"
                ));
                PluginManagerReport::Doctor { ok, logs }
            }
        };

        self.append_operation_logs(report.logs())?;
        emit_logs(report.logs());
        Ok(report)
    }

    fn artifacts_from_startup_plugin_declarations(
        &self,
        use_declarations: &[StartupPluginDeclaration],
        lazy_declarations: &[StartupPluginDeclaration],
        source_hash: String,
    ) -> Result<PluginDeclarationArtifacts, PluginHostError> {
        let manifests = self.read_bundled_manifests()?;
        let mut lazy_index = lazy_index_from_bundled_manifests(&manifests);
        let mut lockfile = PluginLockfile {
            version: PluginLockfile::CURRENT_VERSION,
            plugins: Vec::new(),
        };
        let mut startup_plan = StartupPlan {
            version: StartupPlan::CURRENT_VERSION,
            source_hash,
            entries: Vec::new(),
        };
        let mut logs = vec![format!(
            "[saya-plugin-manager][sync] build startup declaration artifacts: bundled_count={} use_count={} lazy_count={}",
            manifests.len(),
            use_declarations.len(),
            lazy_declarations.len()
        )];

        for declaration in use_declarations {
            lockfile
                .plugins
                .push(locked_plugin_from_declaration(declaration));
            startup_plan.entries.push(StartupPlanEntry::Command {
                name: format!("{}.setup", declaration.name),
                callback_source: startup_declaration_callback_source(declaration),
            });
        }

        for declaration in lazy_declarations {
            lockfile
                .plugins
                .push(locked_plugin_from_declaration(declaration));
            let target = lazy_target_from_declaration(declaration);
            for command in &declaration.commands {
                lazy_index.commands.insert(command.clone(), target.clone());
            }
            for event in &declaration.events {
                lazy_index
                    .events
                    .entry(event.clone())
                    .or_default()
                    .push(target.clone());
            }
        }

        dedupe_locked_plugins(&mut lockfile.plugins);
        logs.push(format!(
            "[saya-plugin-manager][lazy] startup declaration index generated: commands={} events={}",
            lazy_index.commands.len(),
            lazy_index.events.len()
        ));
        logs.push(format!(
            "[saya-plugin-manager][lockfile] startup declaration plugins locked: plugin_count={}",
            lockfile.plugins.len()
        ));
        Ok(PluginDeclarationArtifacts {
            lockfile,
            startup_plan,
            lazy_index,
            logs,
        })
    }

    fn write_json<T>(&self, path: PathBuf, value: &T) -> Result<(), PluginHostError>
    where
        T: Serialize,
    {
        self.root.ensure_plugins_dir()?;
        let encoded =
            serde_json::to_string_pretty(value).map_err(|error| PluginHostError::Json {
                path: path.clone(),
                message: error.to_string(),
            })?;
        fs::write(&path, encoded).map_err(|error| PluginHostError::Io {
            path,
            message: error.to_string(),
        })
    }

    fn read_json<T>(&self, path: PathBuf) -> Result<Option<T>, PluginHostError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(PluginHostError::Io {
                    path,
                    message: error.to_string(),
                });
            }
        };
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|error| PluginHostError::Json {
                path,
                message: error.to_string(),
            })
    }

    fn append_operation_logs(&self, logs: &[String]) -> Result<(), PluginHostError> {
        self.root.ensure_plugins_dir()?;
        let mut text = String::new();
        for line in logs {
            text.push_str(line);
            text.push('\n');
        }
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.operations_log_path())
            .and_then(|mut file| {
                use std::io::Write;
                file.write_all(text.as_bytes())
            })
            .map_err(|error| PluginHostError::Io {
                path: self.root.operations_log_path(),
                message: error.to_string(),
            })
    }

    fn sync_bundled_artifacts(&self) -> Result<usize, PluginHostError> {
        let manifests = self.read_bundled_manifests()?;
        let lazy_index = lazy_index_from_bundled_manifests(&manifests);
        self.write_lazy_index(&lazy_index)?;
        self.write_startup_plan(&StartupPlan {
            version: StartupPlan::CURRENT_VERSION,
            source_hash: "bundled-manifest".to_string(),
            entries: Vec::new(),
        })?;
        if self.read_lockfile()?.is_none() {
            self.write_lockfile(&PluginLockfile {
                version: PluginLockfile::CURRENT_VERSION,
                plugins: Vec::new(),
            })?;
        }
        Ok(manifests.len())
    }
}

fn bundled_manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(BUNDLED_PLUGIN_MANIFEST_DIR)
}

fn registry_from_bundled_manifests(
    manifests: &[BundledPluginManifest],
) -> (StartupRegistry, usize) {
    let mut entries = Vec::new();
    for manifest in manifests {
        let target = LazyTarget {
            plugin: manifest.name.clone(),
            module: manifest.module.clone(),
            export_name: manifest.setup.clone(),
        };
        for command in &manifest.lazy.commands {
            entries.push(StartupRegistryEntry::Command {
                name: command.clone(),
                callback_source: lazy_callback_source("command", command, &target),
            });
        }
        for event in &manifest.lazy.events {
            entries.push(StartupRegistryEntry::Event {
                name: event.clone(),
                callback_source: lazy_callback_source("event", event, &target),
            });
        }
    }
    let loaded_entries = entries.len();
    (StartupRegistry::from_entries(entries), loaded_entries)
}

fn lazy_index_from_bundled_manifests(manifests: &[BundledPluginManifest]) -> LazyIndex {
    let mut commands = BTreeMap::new();
    let mut events: BTreeMap<String, Vec<LazyTarget>> = BTreeMap::new();
    for manifest in manifests {
        let target = LazyTarget {
            plugin: manifest.name.clone(),
            module: manifest.module.clone(),
            export_name: manifest.setup.clone(),
        };
        for command in &manifest.lazy.commands {
            commands.insert(command.clone(), target.clone());
        }
        for event in &manifest.lazy.events {
            events
                .entry(event.clone())
                .or_default()
                .push(target.clone());
        }
    }
    LazyIndex {
        version: LazyIndex::CURRENT_VERSION,
        commands,
        events,
    }
}

fn default_setup_export() -> String {
    "setup".to_string()
}

fn remove_optional_file(path: PathBuf) -> Result<usize, PluginHostError> {
    match fs::remove_file(&path) {
        Ok(()) => Ok(1),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(PluginHostError::Io {
            path,
            message: error.to_string(),
        }),
    }
}

struct PluginDeclarationArtifacts {
    lockfile: PluginLockfile,
    startup_plan: StartupPlan,
    lazy_index: LazyIndex,
    logs: Vec<String>,
}

fn locked_plugin_from_declaration(declaration: &StartupPluginDeclaration) -> LockedPlugin {
    LockedPlugin {
        name: declaration.name.clone(),
        source: source_string_from_declaration(declaration),
        revision: revision_from_declaration(declaration),
        depends: Vec::new(),
        before: Vec::new(),
        after: Vec::new(),
    }
}

fn source_string_from_declaration(declaration: &StartupPluginDeclaration) -> String {
    match &declaration.source {
        StartupPluginSource::Local { path } => format!("local:{path}"),
        StartupPluginSource::Github { repo, .. } => format!("github:{repo}"),
    }
}

fn revision_from_declaration(declaration: &StartupPluginDeclaration) -> String {
    match &declaration.source {
        StartupPluginSource::Local { .. } => "workspace".to_string(),
        StartupPluginSource::Github { rev, .. } => {
            rev.clone().unwrap_or_else(|| "HEAD".to_string())
        }
    }
}

fn startup_declaration_callback_source(declaration: &StartupPluginDeclaration) -> String {
    format!(
        r#"async () => {{
                console.info("[saya-plugin-manager][startup] eager setup plugin={} module={}");
            }}"#,
        declaration.name, declaration.module
    )
}

fn lazy_target_from_declaration(declaration: &StartupPluginDeclaration) -> LazyTarget {
    LazyTarget {
        plugin: declaration.name.clone(),
        module: declaration.module.clone(),
        export_name: declaration.setup.clone(),
    }
}

fn dedupe_locked_plugins(plugins: &mut Vec<LockedPlugin>) {
    let mut deduped = BTreeMap::new();
    for plugin in plugins.drain(..) {
        deduped.insert(plugin.name.clone(), plugin);
    }
    plugins.extend(deduped.into_values());
}

fn emit_logs(logs: &[String]) {
    for line in logs {
        log::info!("{line}");
    }
}

fn lazy_callback_source(kind: &str, name: &str, target: &LazyTarget) -> String {
    let name_json = serde_json::to_string(name).expect("lazy trigger name should serialize");
    let plugin_json = serde_json::to_string(&target.plugin).expect("plugin name should serialize");
    let module_json = serde_json::to_string(&target.module).expect("module path should serialize");
    let export_json =
        serde_json::to_string(&target.export_name).expect("export name should serialize");
    format!(
        r#"async () => {{
                console.info("[saya-plugin-host][lazy] {kind} trigger: name={name} plugin={plugin} module={module}");
                return await saya.plugins.loadLazy({{
                    kind: "{kind}",
                    name: {name_json},
                    plugin: {plugin_json},
                    module: {module_json},
                    exportName: {export_json},
                }});
            }}"#,
        kind = kind,
        name = name,
        plugin = target.plugin,
        module = target.module,
        name_json = name_json,
        plugin_json = plugin_json,
        module_json = module_json,
        export_json = export_json
    )
}

pub fn render_plugin_report(report: &PluginManagerReport) -> String {
    match report {
        PluginManagerReport::Sync { plugin_count, .. } => {
            format!("plugin sync completed: plugin_count={plugin_count}")
        }
        PluginManagerReport::Update { plugin_count, .. } => {
            format!("plugin update completed: plugin_count={plugin_count}")
        }
        PluginManagerReport::List {
            plugin_count,
            plugins,
            ..
        } => {
            if plugins.is_empty() {
                format!("plugin list: plugin_count={plugin_count}")
            } else {
                format!(
                    "plugin list: plugin_count={plugin_count}\n{}",
                    plugins.join("\n")
                )
            }
        }
        PluginManagerReport::Clean { removed_files, .. } => {
            format!("plugin clean completed: removed_files={removed_files}")
        }
        PluginManagerReport::Doctor { ok, .. } => {
            format!("plugin doctor completed: ok={ok}")
        }
    }
}
