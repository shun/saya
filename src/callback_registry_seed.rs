use crate::bootstrap::{StartupKeymapAction, StartupKeymapMode, StartupKeymapSnapshot};
use crate::config_runtime::{AppliedKeyMapping, ConfigKeyMode, SayaKeyMode, SayaKeymapAction};
use crate::config_runtime::{StartupRegistry, StartupRegistryEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackRegistrySeed {
    commands: Vec<RegisteredCommandSeed>,
    events: Vec<RegisteredEventSeed>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredCommandSeed {
    slot: usize,
    name: String,
    callback_source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredEventSeed {
    slot: usize,
    name: String,
    callback_source: String,
}

impl CallbackRegistrySeed {
    pub fn empty() -> Self {
        log::debug!("[callback_registry_seed] create empty callback registry seed");
        Self {
            commands: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn from_startup_registry(registry: &StartupRegistry) -> Self {
        log::debug!(
            "[callback_registry_seed] build seed from startup registry: entry_count={}",
            registry.entries().len()
        );
        Self::from_startup_entries(registry.entries().iter().cloned())
    }

    pub fn from_startup_entries<I>(entries: I) -> Self
    where
        I: IntoIterator<Item = StartupRegistryEntry>,
    {
        let mut commands = Vec::new();
        let mut events = Vec::new();

        for entry in entries {
            match entry {
                StartupRegistryEntry::Command {
                    name,
                    callback_source,
                } => {
                    let slot = commands.len();
                    log::debug!(
                        "[callback_registry_seed] register command seed: slot={}, name={}",
                        slot,
                        name
                    );
                    commands.push(RegisteredCommandSeed {
                        slot,
                        name,
                        callback_source,
                    });
                }
                StartupRegistryEntry::Event {
                    name,
                    callback_source,
                } => {
                    let slot = events.len();
                    log::debug!(
                        "[callback_registry_seed] register event seed: slot={}, name={}",
                        slot,
                        name
                    );
                    events.push(RegisteredEventSeed {
                        slot,
                        name,
                        callback_source,
                    });
                }
                _ => {}
            }
        }

        log::debug!(
            "[callback_registry_seed] build complete: commands={}, events={}",
            commands.len(),
            events.len()
        );

        Self { commands, events }
    }

    pub fn commands(&self) -> &[RegisteredCommandSeed] {
        &self.commands
    }

    pub fn events(&self) -> &[RegisteredEventSeed] {
        &self.events
    }
}

impl RegisteredCommandSeed {
    pub fn slot(&self) -> usize {
        self.slot
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn callback_source(&self) -> &str {
        &self.callback_source
    }
}

impl RegisteredEventSeed {
    pub fn slot(&self) -> usize {
        self.slot
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn callback_source(&self) -> &str {
        &self.callback_source
    }
}

impl From<AppliedKeyMapping> for StartupKeymapSnapshot {
    fn from(value: AppliedKeyMapping) -> Self {
        Self {
            mode: value.mode.into(),
            lhs: value.lhs,
            action: StartupKeymapAction::Literal(value.rhs),
        }
    }
}

impl From<SayaKeyMode> for StartupKeymapMode {
    fn from(value: SayaKeyMode) -> Self {
        match value {
            SayaKeyMode::Normal => Self::Normal,
            SayaKeyMode::Insert => Self::Insert,
            SayaKeyMode::Visual => Self::Visual,
        }
    }
}

impl From<ConfigKeyMode> for StartupKeymapMode {
    fn from(value: ConfigKeyMode) -> Self {
        match value {
            ConfigKeyMode::Normal => Self::Normal,
            ConfigKeyMode::Insert => Self::Insert,
        }
    }
}

impl From<SayaKeymapAction> for StartupKeymapAction {
    fn from(value: SayaKeymapAction) -> Self {
        match value {
            SayaKeymapAction::Literal(text) => Self::Literal(text),
            SayaKeymapAction::RegisteredCommand(name) => Self::RegisteredCommand(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_from_startup_registry_preserves_metadata_and_order() {
        let registry = StartupRegistry::from_entries(vec![
            StartupRegistryEntry::Command {
                name: "alpha".to_string(),
                callback_source: "() => 1".to_string(),
            },
            StartupRegistryEntry::Event {
                name: "bufferOpen".to_string(),
                callback_source: "(payload) => payload".to_string(),
            },
            StartupRegistryEntry::Command {
                name: "beta".to_string(),
                callback_source: "() => 2".to_string(),
            },
        ]);

        let seed = CallbackRegistrySeed::from_startup_registry(&registry);

        assert_eq!(seed.commands.len(), 2);
        assert_eq!(seed.events.len(), 1);
        assert_eq!(seed.commands[0].slot, 0);
        assert_eq!(seed.commands[0].name, "alpha");
        assert_eq!(seed.commands[0].callback_source, "() => 1");
        assert_eq!(seed.commands[1].slot, 1);
        assert_eq!(seed.commands[1].name, "beta");
        assert_eq!(seed.commands[1].callback_source, "() => 2");
        assert_eq!(seed.events[0].slot, 0);
        assert_eq!(seed.events[0].name, "bufferOpen");
        assert_eq!(seed.events[0].callback_source, "(payload) => payload");
    }

    #[test]
    fn seed_from_startup_entries_keeps_duplicates_in_source_order() {
        let seed = CallbackRegistrySeed::from_startup_entries(vec![
            StartupRegistryEntry::Command {
                name: "duplicate".to_string(),
                callback_source: "() => 1".to_string(),
            },
            StartupRegistryEntry::Command {
                name: "duplicate".to_string(),
                callback_source: "() => 2".to_string(),
            },
            StartupRegistryEntry::Event {
                name: "duplicateEvent".to_string(),
                callback_source: "(payload) => 1".to_string(),
            },
            StartupRegistryEntry::Event {
                name: "duplicateEvent".to_string(),
                callback_source: "(payload) => 2".to_string(),
            },
        ]);

        assert_eq!(
            seed.commands
                .iter()
                .map(|entry| entry.callback_source())
                .collect::<Vec<_>>(),
            vec!["() => 1", "() => 2"]
        );
        assert_eq!(
            seed.events
                .iter()
                .map(|entry| entry.callback_source())
                .collect::<Vec<_>>(),
            vec!["(payload) => 1", "(payload) => 2"]
        );
    }
}
