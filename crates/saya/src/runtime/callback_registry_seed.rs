use crate::app::bootstrap::{StartupKeymapAction, StartupKeymapMode, StartupKeymapSnapshot};
use crate::runtime::config::{AppliedKeyMapping, ConfigKeyMode, SayaKeyMode, SayaKeymapAction};
use crate::runtime::config::{StartupRegistry, StartupRegistryEntry};

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
#[path = "callback_registry_seed_test.rs"]
mod tests;
