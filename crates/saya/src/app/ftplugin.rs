use std::path::Path;

use crate::runtime::config::{
    FtPluginConfig, FtPluginDefinition, FtPluginOption, FtPluginStartupAction,
};
use crate::runtime::options::{SayaOptionName, SayaOptionValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtPlugin {
    pub filetype: String,
    pub options: Vec<FtPluginOption>,
}

pub fn default_ftplugin_config() -> FtPluginConfig {
    FtPluginConfig {
        enabled: true,
        definitions: vec![FtPluginDefinition {
            filetype: "go".to_string(),
            extensions: vec!["go".to_string()],
            options: vec![
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
            ],
            enabled: true,
        }],
    }
}

pub fn apply_ftplugin_startup_action(config: &mut FtPluginConfig, action: &FtPluginStartupAction) {
    match action {
        FtPluginStartupAction::SetEnabled(enabled) => {
            config.enabled = *enabled;
        }
        FtPluginStartupAction::SetDefinition(definition) => {
            upsert_definition(config, definition.clone());
        }
        FtPluginStartupAction::DisableFileType { filetype } => {
            if let Some(definition) = config
                .definitions
                .iter_mut()
                .find(|definition| definition.filetype == *filetype)
            {
                definition.enabled = false;
            } else {
                config.definitions.push(FtPluginDefinition {
                    filetype: filetype.clone(),
                    extensions: Vec::new(),
                    options: Vec::new(),
                    enabled: false,
                });
            }
        }
    }
}

pub fn resolve_ftplugin_for_path(path: Option<&Path>, config: &FtPluginConfig) -> Option<FtPlugin> {
    if !config.enabled {
        return None;
    }
    let path = path?;
    let extension = path.extension()?.to_str()?.trim().to_ascii_lowercase();
    config
        .definitions
        .iter()
        .rev()
        .find(|definition| {
            definition.enabled
                && definition
                    .extensions
                    .iter()
                    .any(|candidate| normalize_extension(candidate) == extension)
        })
        .map(|definition| FtPlugin {
            filetype: definition.filetype.clone(),
            options: definition.options.clone(),
        })
}

fn upsert_definition(config: &mut FtPluginConfig, definition: FtPluginDefinition) {
    if let Some(existing) = config
        .definitions
        .iter_mut()
        .find(|existing| existing.filetype == definition.filetype)
    {
        *existing = definition;
    } else {
        config.definitions.push(definition);
    }
}

fn normalize_extension(extension: &str) -> String {
    extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
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
}
