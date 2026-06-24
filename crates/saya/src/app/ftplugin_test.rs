use super::*;

#[test]
fn default_go_ftplugin_matches_vim_recommended_go_style() {
    let config = default_ftplugin_config();
    let plugin =
        resolve_ftplugin_for_path(Some(Path::new("main.go")), &config).expect("go ftplugin");

    assert_eq!(plugin.filetype, "go".to_string());
    assert_eq!(
        plugin.options,
        vec![
            FtPluginOption {
                name: SayaOptionName::ExpandTab,
                value: SayaOptionValue::Boolean(false),
            },
            FtPluginOption {
                name: SayaOptionName::SoftTabStop,
                value: SayaOptionValue::Number(0),
            },
            FtPluginOption {
                name: SayaOptionName::ShiftWidth,
                value: SayaOptionValue::Number(0),
            },
        ]
    );
}

#[test]
fn unknown_extension_has_no_ftplugin() {
    assert_eq!(
        resolve_ftplugin_for_path(Some(Path::new("notes.txt")), &default_ftplugin_config()),
        None
    );
}

#[test]
fn startup_action_can_disable_all_ftplugins() {
    let mut config = default_ftplugin_config();

    apply_ftplugin_startup_action(&mut config, &FtPluginStartupAction::SetEnabled(false));

    assert_eq!(
        resolve_ftplugin_for_path(Some(Path::new("main.go")), &config),
        None
    );
}

#[test]
fn startup_definition_overrides_builtin_filetype_by_name() {
    let mut config = default_ftplugin_config();

    apply_ftplugin_startup_action(
        &mut config,
        &FtPluginStartupAction::SetDefinition(FtPluginDefinition {
            filetype: "go".to_string(),
            extensions: vec!["go".to_string()],
            options: vec![FtPluginOption {
                name: SayaOptionName::ExpandTab,
                value: SayaOptionValue::Boolean(true),
            }],
            enabled: true,
        }),
    );

    let plugin =
        resolve_ftplugin_for_path(Some(Path::new("main.go")), &config).expect("go ftplugin");
    assert_eq!(
        plugin.options,
        vec![FtPluginOption {
            name: SayaOptionName::ExpandTab,
            value: SayaOptionValue::Boolean(true),
        }]
    );
}
