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
