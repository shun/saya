use saya::callback_registry_seed::CallbackRegistrySeed;
use saya::config_runtime::StartupRegistryEntry;

#[test]
fn callback_registry_seed_preserves_command_and_event_registration_order() {
    let registry = vec![
        StartupRegistryEntry::Command {
            name: "firstCommand".to_string(),
            callback_source: "() => console.log(\"first\")".to_string(),
        },
        StartupRegistryEntry::Event {
            name: "firstEvent".to_string(),
            callback_source: "(payload) => console.log(payload)".to_string(),
        },
        StartupRegistryEntry::Command {
            name: "secondCommand".to_string(),
            callback_source: "() => console.log(\"second\")".to_string(),
        },
        StartupRegistryEntry::Event {
            name: "secondEvent".to_string(),
            callback_source: "(payload) => console.log(payload.buffer.id)".to_string(),
        },
    ];

    let seed = CallbackRegistrySeed::from_startup_entries(registry);

    assert_eq!(seed.commands().len(), 2);
    assert_eq!(seed.events().len(), 2);
    assert_eq!(seed.commands()[0].slot(), 0);
    assert_eq!(seed.commands()[0].name(), "firstCommand");
    assert_eq!(seed.commands()[0].callback_source(), "() => console.log(\"first\")");
    assert_eq!(seed.commands()[1].slot(), 1);
    assert_eq!(seed.commands()[1].name(), "secondCommand");
    assert_eq!(seed.commands()[1].callback_source(), "() => console.log(\"second\")");
    assert_eq!(seed.events()[0].slot(), 0);
    assert_eq!(seed.events()[0].name(), "firstEvent");
    assert_eq!(seed.events()[0].callback_source(), "(payload) => console.log(payload)");
    assert_eq!(seed.events()[1].slot(), 1);
    assert_eq!(seed.events()[1].name(), "secondEvent");
    assert_eq!(
        seed.events()[1].callback_source(),
        "(payload) => console.log(payload.buffer.id)"
    );
}

#[test]
fn callback_registry_seed_keeps_duplicate_callbacks_in_source_order() {
    let registry = vec![
        StartupRegistryEntry::Command {
            name: "duplicate".to_string(),
            callback_source: "() => console.log(\"first\")".to_string(),
        },
        StartupRegistryEntry::Event {
            name: "duplicateEvent".to_string(),
            callback_source: "(payload) => console.log(\"first\")".to_string(),
        },
        StartupRegistryEntry::Command {
            name: "duplicate".to_string(),
            callback_source: "() => console.log(\"second\")".to_string(),
        },
        StartupRegistryEntry::Event {
            name: "duplicateEvent".to_string(),
            callback_source: "(payload) => console.log(\"second\")".to_string(),
        },
    ];

    let seed = CallbackRegistrySeed::from_startup_entries(registry);

    assert_eq!(
        seed.commands()
            .iter()
            .map(|entry| entry.callback_source())
            .collect::<Vec<_>>(),
        vec!["() => console.log(\"first\")", "() => console.log(\"second\")"]
    );
    assert_eq!(
        seed.events()
            .iter()
            .map(|entry| entry.callback_source())
            .collect::<Vec<_>>(),
        vec![
            "(payload) => console.log(\"first\")",
            "(payload) => console.log(\"second\")"
        ]
    );
}
