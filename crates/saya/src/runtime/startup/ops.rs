//! startup 設定収集用の deno ops 群と wire 型・パーサ・extension 登録。

use super::*;

#[op2(fast)]
pub(super) fn op_collect_startup_tabstop(
    state: &mut OpState,
    #[number] value: i64,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup tabstop option from runtime: value={}",
        value
    );

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Option {
            name: SayaOptionName::TabSize,
            value: SayaOptionValue::Number(value),
        });

    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_line_numbers(
    state: &mut OpState,
    value: bool,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup number option from runtime: value={}",
        value
    );

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Option {
            name: SayaOptionName::LineNumbers,
            value: SayaOptionValue::Boolean(value),
        });

    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_number_width(
    state: &mut OpState,
    #[number] value: i64,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup numberwidth option from runtime: value={}",
        value
    );

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Option {
            name: SayaOptionName::NumberWidth,
            value: SayaOptionValue::Number(value),
        });

    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_bool_option(
    state: &mut OpState,
    #[string] name: String,
    value: bool,
) -> Result<(), JsErrorBox> {
    collect_startup_option(
        state,
        &name,
        SayaOptionValue::Boolean(value),
        "boolean startup option",
    )
}

#[op2(fast)]
pub(super) fn op_collect_startup_number_option(
    state: &mut OpState,
    #[string] name: String,
    #[number] value: i64,
) -> Result<(), JsErrorBox> {
    collect_startup_option(
        state,
        &name,
        SayaOptionValue::Number(value),
        "number startup option",
    )
}

#[op2(fast)]
pub(super) fn op_collect_startup_string_option(
    state: &mut OpState,
    #[string] name: String,
    #[string] value: String,
) -> Result<(), JsErrorBox> {
    collect_startup_option(
        state,
        &name,
        SayaOptionValue::String(value),
        "string startup option",
    )
}

pub(super) fn collect_startup_option(
    state: &mut OpState,
    name: &str,
    value: SayaOptionValue,
    label: &str,
) -> Result<(), JsErrorBox> {
    let definition = crate::runtime::options::SayaOptionRegistry::resolve(name)
        .filter(|definition| definition.startup_public)
        .ok_or_else(|| JsErrorBox::generic(format!("unsupported startup option: {name}")))?;
    if definition.value_type != value.option_type() {
        return Err(JsErrorBox::generic(format!(
            "startup option type mismatch: option={}, expected={:?}, actual={:?}",
            definition.name,
            definition.value_type,
            value.option_type()
        )));
    }
    log::debug!(
        "[startup_runtime] collect {label}: name={}, value={:?}",
        definition.name,
        value
    );
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Option {
            name: definition.name,
            value,
        });
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_keymap(
    state: &mut OpState,
    #[string] mode: String,
    #[string] lhs: String,
    #[string] action: String,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup keymap from runtime: mode={}, lhs={}, action={}",
        mode,
        lhs,
        action
    );

    let mode = match mode.as_str() {
        "normal" => SayaKeyMode::Normal,
        "insert" => SayaKeyMode::Insert,
        "visual" => SayaKeyMode::Visual,
        other => {
            return Err(JsErrorBox::generic(format!(
                "unsupported keymap mode: {}",
                other
            )));
        }
    };

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Keymap {
            mode,
            lhs,
            action: parse_startup_keymap_action(&action),
        });

    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_command(
    state: &mut OpState,
    #[string] name: String,
    #[string] callback_source: String,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup command from runtime: name={}",
        name
    );

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Command {
            name,
            callback_source,
        });

    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_event(
    state: &mut OpState,
    #[string] name: String,
    #[string] callback_source: String,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup event from runtime: name={}",
        name
    );

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Event {
            name,
            callback_source,
        });

    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_ftplugin_enabled(
    state: &mut OpState,
    enabled: bool,
) -> Result<(), JsErrorBox> {
    log::debug!(
        "[startup_runtime] collect startup ftplugin enabled flag: enabled={}",
        enabled
    );
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::FtPlugin {
            action: FtPluginStartupAction::SetEnabled(enabled),
        });
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_ftplugin_definition(
    state: &mut OpState,
    #[string] filetype: String,
    #[string] definition_json: String,
) -> Result<(), JsErrorBox> {
    let filetype = normalize_ftplugin_filetype(&filetype)?;
    let wire: StartupFtPluginDefinitionWire = serde_json::from_str(&definition_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid ftplugin definition: {error}")))?;
    let definition = FtPluginDefinition {
        filetype: filetype.clone(),
        extensions: normalize_ftplugin_extensions(&filetype, wire.extensions)?,
        options: parse_ftplugin_options(wire.options)?,
        enabled: wire.enabled.unwrap_or(true),
    };
    log::debug!(
        "[startup_runtime] collect startup ftplugin definition: filetype={}, extensions={}, options={}, enabled={}",
        definition.filetype,
        definition.extensions.len(),
        definition.options.len(),
        definition.enabled
    );
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::FtPlugin {
            action: FtPluginStartupAction::SetDefinition(definition),
        });
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_ftplugin_disable(
    state: &mut OpState,
    #[string] filetype: String,
) -> Result<(), JsErrorBox> {
    let filetype = normalize_ftplugin_filetype(&filetype)?;
    log::debug!(
        "[startup_runtime] collect startup ftplugin filetype disable: filetype={}",
        filetype
    );
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::FtPlugin {
            action: FtPluginStartupAction::DisableFileType { filetype },
        });
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_statusline(
    state: &mut OpState,
    #[string] config_json: String,
) -> Result<(), JsErrorBox> {
    let wire: StartupStatusLineConfigWire = serde_json::from_str(&config_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid statusline config: {error}")))?;
    let config = StatusLineConfig {
        left: parse_statusline_segments("left", wire.left)?,
        right: parse_statusline_segments("right", wire.right)?,
    };
    log::debug!(
        "[startup_runtime] collect startup statusline: left_segments={}, right_segments={}",
        config.left.len(),
        config.right.len()
    );
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::StatusLine { config });
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_theme_palette(
    state: &mut OpState,
    #[string] palette_json: String,
) -> Result<(), JsErrorBox> {
    let palette = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&palette_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid theme palette: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup theme palette: token_count={}",
        palette.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for (name, value) in palette {
        let Some(value) = value.as_str() else {
            return Err(JsErrorBox::generic(format!(
                "theme palette value must be a string: {name}"
            )));
        };
        registry.push(StartupRegistryEntry::ThemePalette {
            name,
            value: value.to_string(),
        });
    }
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_theme_markdown(
    state: &mut OpState,
    #[string] markdown_json: String,
) -> Result<(), JsErrorBox> {
    let markdown =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&markdown_json)
            .map_err(|error| JsErrorBox::generic(format!("invalid markdown theme: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup markdown theme: style_count={}",
        markdown.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for (name, value) in markdown {
        let key = MarkdownSemanticStyleKey::parse(&name).ok_or_else(|| {
            JsErrorBox::generic(format!("unsupported markdown theme key: {name}"))
        })?;
        let style = parse_theme_text_style("markdown theme style", &name, value)?;
        registry.push(StartupRegistryEntry::ThemeMarkdownStyle { key, style });
    }
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_theme_ui(
    state: &mut OpState,
    #[string] ui_json: String,
) -> Result<(), JsErrorBox> {
    let ui = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&ui_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid ui theme: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup ui theme: style_count={}",
        ui.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for (name, value) in ui {
        let key = UiStyleKey::parse(&name)
            .ok_or_else(|| JsErrorBox::generic(format!("unsupported ui theme key: {name}")))?;
        let style = parse_theme_text_style("ui theme style", &name, value)?;
        registry.push(StartupRegistryEntry::ThemeUiStyle { key, style });
    }
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_theme_syntax(
    state: &mut OpState,
    #[string] syntax_json: String,
) -> Result<(), JsErrorBox> {
    let syntax = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&syntax_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid syntax theme: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup syntax theme: style_count={}",
        syntax.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for (name, value) in syntax {
        let key = SyntaxSemanticStyleKey::parse(&name)
            .ok_or_else(|| JsErrorBox::generic(format!("unsupported syntax theme key: {name}")))?;
        let style = parse_theme_text_style("syntax theme style", &name, value)?;
        registry.push(StartupRegistryEntry::ThemeSyntaxStyle { key, style });
    }
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_theme_languages(
    state: &mut OpState,
    #[string] languages_json: String,
) -> Result<(), JsErrorBox> {
    let languages =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&languages_json)
            .map_err(|error| JsErrorBox::generic(format!("invalid languages theme: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup language theme: language_count={}",
        languages.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for (language, value) in languages {
        let serde_json::Value::Object(language_object) = value else {
            return Err(JsErrorBox::generic(format!(
                "language theme must be an object: {language}"
            )));
        };
        let Some(syntax_value) = language_object.get("syntax") else {
            continue;
        };
        let serde_json::Value::Object(syntax) = syntax_value else {
            return Err(JsErrorBox::generic(format!(
                "language syntax theme must be an object: {language}.syntax"
            )));
        };
        for (name, value) in syntax {
            let key = SyntaxSemanticStyleKey::parse(name).ok_or_else(|| {
                JsErrorBox::generic(format!(
                    "unsupported language syntax theme key: {language}.{name}"
                ))
            })?;
            let style = parse_theme_text_style(
                "language syntax theme style",
                &format!("{language}.syntax.{name}"),
                value.clone(),
            )?;
            registry.push(StartupRegistryEntry::ThemeLanguageSyntaxStyle {
                language: language.clone(),
                key,
                style,
            });
        }
    }
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_theme_filer(
    state: &mut OpState,
    #[string] filer_json: String,
) -> Result<(), JsErrorBox> {
    let filer = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&filer_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid filer theme: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup filer theme: style_count={}",
        filer.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for (name, value) in filer {
        let key = FilerSemanticStyleKey::parse(&name)
            .ok_or_else(|| JsErrorBox::generic(format!("unsupported filer theme key: {name}")))?;
        let style = parse_theme_text_style("filer theme style", &name, value)?;
        registry.push(StartupRegistryEntry::ThemeFilerStyle { key, style });
    }
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_log_file(
    state: &mut OpState,
    #[string] path: String,
) -> Result<(), JsErrorBox> {
    log::debug!("[startup_runtime] collect startup log file: path={}", path);
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::LogFile { path });
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_log_level(
    state: &mut OpState,
    #[string] level: String,
) -> Result<(), JsErrorBox> {
    let Some(level_filter) = parse_startup_log_level(&level) else {
        return Err(JsErrorBox::generic(format!(
            "unsupported log.level: {level}"
        )));
    };
    log::debug!(
        "[startup_runtime] collect startup log level: level={}",
        level_filter
    );
    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::LogLevel {
            level: level_filter,
        });
    Ok(())
}

#[op2(fast)]
pub(super) fn op_collect_startup_plugin_use(
    state: &mut OpState,
    #[string] declarations_json: String,
) -> Result<(), JsErrorBox> {
    collect_startup_plugin_declarations(state, &declarations_json, false)
}

#[op2(fast)]
pub(super) fn op_collect_startup_plugin_lazy(
    state: &mut OpState,
    #[string] declarations_json: String,
) -> Result<(), JsErrorBox> {
    collect_startup_plugin_declarations(state, &declarations_json, true)
}

#[op2(fast)]
pub(super) fn op_collect_startup_warning(
    state: &mut OpState,
    #[string] message: String,
) -> Result<(), JsErrorBox> {
    log::debug!("[startup_runtime] collect startup warning: {}", message);

    state
        .borrow_mut::<StartupRegistry>()
        .push(StartupRegistryEntry::Warning { message });

    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StartupFtPluginDefinitionWire {
    extensions: Option<Vec<String>>,
    options: Option<serde_json::Map<String, serde_json::Value>>,
    enabled: Option<bool>,
}

pub(super) fn normalize_ftplugin_filetype(filetype: &str) -> Result<String, JsErrorBox> {
    let filetype = filetype.trim().to_ascii_lowercase();
    if filetype.is_empty() {
        return Err(JsErrorBox::generic(
            "ftplugin filetype must be a non-empty string",
        ));
    }
    Ok(filetype)
}

pub(super) fn normalize_ftplugin_extensions(
    filetype: &str,
    extensions: Option<Vec<String>>,
) -> Result<Vec<String>, JsErrorBox> {
    let mut normalized = extensions.unwrap_or_else(|| vec![filetype.to_string()]);
    for extension in &mut normalized {
        *extension = extension
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        if extension.is_empty() {
            return Err(JsErrorBox::generic(
                "ftplugin extension must be a non-empty string",
            ));
        }
    }
    Ok(normalized)
}

pub(super) fn parse_ftplugin_options(
    options: Option<serde_json::Map<String, serde_json::Value>>,
) -> Result<Vec<FtPluginOption>, JsErrorBox> {
    let mut parsed = Vec::new();
    for (name, value) in options.unwrap_or_default() {
        let definition = SayaOptionRegistry::resolve(&name)
            .filter(|definition| definition.startup_public)
            .ok_or_else(|| JsErrorBox::generic(format!("unsupported ftplugin option: {name}")))?;
        let value = match definition.value_type {
            SayaOptionType::Boolean => value.as_bool().map(SayaOptionValue::Boolean),
            SayaOptionType::Number => value.as_i64().map(SayaOptionValue::Number),
            SayaOptionType::String => value
                .as_str()
                .map(|value| SayaOptionValue::String(value.to_string())),
        }
        .ok_or_else(|| {
            JsErrorBox::generic(format!(
                "ftplugin option type mismatch: option={}, expected={:?}",
                definition.name, definition.value_type
            ))
        })?;
        parsed.push(FtPluginOption {
            name: definition.name,
            value,
        });
    }
    Ok(parsed)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StartupStatusLineConfigWire {
    left: Option<Vec<String>>,
    right: Option<Vec<String>>,
}

pub(super) fn parse_statusline_segments(
    side: &str,
    segments: Option<Vec<String>>,
) -> Result<Vec<StatusLineSegment>, JsErrorBox> {
    segments
        .unwrap_or_default()
        .into_iter()
        .map(|segment| match segment.as_str() {
            "fileName" => Ok(StatusLineSegment::FileName),
            "mode" => Ok(StatusLineSegment::Mode),
            "filetype" => Ok(StatusLineSegment::FileType),
            "modified" => Ok(StatusLineSegment::Modified),
            other => Err(JsErrorBox::generic(format!(
                "unsupported statusline segment in {side}: {other}"
            ))),
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StartupPluginDeclarationWire {
    name: String,
    source: StartupPluginSourceWire,
    module: String,
    setup: String,
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    events: Vec<String>,
    #[serde(default)]
    options: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(super) enum StartupPluginSourceWire {
    Local { path: String },
    Github { repo: String, rev: Option<String> },
}

pub(super) fn collect_startup_plugin_declarations(
    state: &mut OpState,
    declarations_json: &str,
    lazy: bool,
) -> Result<(), JsErrorBox> {
    let declarations = serde_json::from_str::<Vec<StartupPluginDeclarationWire>>(declarations_json)
        .map_err(|error| JsErrorBox::generic(format!("invalid plugin declarations: {error}")))?;
    log::debug!(
        "[startup_runtime] collect startup plugin declarations: lazy={}, count={}",
        lazy,
        declarations.len()
    );
    let registry = state.borrow_mut::<StartupRegistry>();
    for declaration in declarations {
        let declaration = normalize_startup_plugin_declaration(declaration)?;
        let entry = if lazy {
            StartupRegistryEntry::PluginLazy { declaration }
        } else {
            StartupRegistryEntry::PluginUse { declaration }
        };
        registry.push(entry);
    }
    Ok(())
}

pub(super) fn normalize_startup_plugin_declaration(
    declaration: StartupPluginDeclarationWire,
) -> Result<StartupPluginDeclaration, JsErrorBox> {
    let name = non_empty_plugin_field("plugin name", declaration.name)?;
    let module = non_empty_plugin_field("plugin module", declaration.module)?;
    let setup = non_empty_plugin_field("plugin setup", declaration.setup)?;
    let commands = declaration
        .commands
        .into_iter()
        .map(|command| non_empty_plugin_field("plugin command", command))
        .collect::<Result<Vec<_>, _>>()?;
    let events = declaration
        .events
        .into_iter()
        .map(|event| non_empty_plugin_field("plugin event", event))
        .collect::<Result<Vec<_>, _>>()?;
    let source = match declaration.source {
        StartupPluginSourceWire::Local { path } => StartupPluginSource::Local {
            path: non_empty_plugin_field("plugin local path", path)?,
        },
        StartupPluginSourceWire::Github { repo, rev } => {
            let repo = non_empty_plugin_field("plugin github repo", repo)?;
            if repo.split('/').count() != 2 {
                return Err(JsErrorBox::generic(
                    "plugin github repo must use owner/repository form",
                ));
            }
            StartupPluginSource::Github {
                repo,
                rev: rev.and_then(|value| {
                    let trimmed = value.trim().to_string();
                    (!trimmed.is_empty()).then_some(trimmed)
                }),
            }
        }
    };
    Ok(StartupPluginDeclaration {
        name,
        source,
        module,
        setup,
        commands,
        events,
        options: declaration.options,
    })
}

pub(super) fn non_empty_plugin_field(label: &str, value: String) -> Result<String, JsErrorBox> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Err(JsErrorBox::generic(format!("{label} must be non-empty")));
    }
    Ok(trimmed)
}

pub(super) fn parse_startup_log_level(level: &str) -> Option<LevelFilter> {
    match level.trim().to_ascii_lowercase().as_str() {
        "error" => Some(LevelFilter::Error),
        "warn" => Some(LevelFilter::Warn),
        "info" => Some(LevelFilter::Info),
        "debug" => Some(LevelFilter::Debug),
        "trace" => Some(LevelFilter::Trace),
        _ => None,
    }
}

pub(super) fn parse_theme_text_style(
    label: &str,
    name: &str,
    value: serde_json::Value,
) -> Result<ThemeTextStyleDeclaration, JsErrorBox> {
    let serde_json::Value::Object(object) = value else {
        return Err(JsErrorBox::generic(format!(
            "{label} must be an object: {name}"
        )));
    };
    let mut style = ThemeTextStyleDeclaration::default();
    for (property, value) in object {
        match property.as_str() {
            "fg" => style.fg = Some(theme_string_property(name, &property, value)?),
            "bg" => style.bg = Some(theme_string_property(name, &property, value)?),
            "bold" => style.bold = Some(theme_bool_property(name, &property, value)?),
            "italic" => style.italic = Some(theme_bool_property(name, &property, value)?),
            "underline" => style.underline = Some(theme_bool_property(name, &property, value)?),
            "strikethrough" => {
                style.strikethrough = Some(theme_bool_property(name, &property, value)?)
            }
            other => {
                return Err(JsErrorBox::generic(format!(
                    "unsupported {label} property: {name}.{other}"
                )));
            }
        }
    }
    Ok(style)
}

pub(super) fn theme_string_property(
    style_name: &str,
    property: &str,
    value: serde_json::Value,
) -> Result<String, JsErrorBox> {
    value.as_str().map(ToString::to_string).ok_or_else(|| {
        JsErrorBox::generic(format!(
            "markdown theme style property must be a string: {style_name}.{property}"
        ))
    })
}

pub(super) fn theme_bool_property(
    style_name: &str,
    property: &str,
    value: serde_json::Value,
) -> Result<bool, JsErrorBox> {
    value.as_bool().ok_or_else(|| {
        JsErrorBox::generic(format!(
            "markdown theme style property must be a boolean: {style_name}.{property}"
        ))
    })
}

deno_core::extension!(
    startup_saya_extension,
    ops = [
        op_collect_startup_tabstop,
        op_collect_startup_line_numbers,
        op_collect_startup_number_width,
        op_collect_startup_bool_option,
        op_collect_startup_number_option,
        op_collect_startup_string_option,
        op_collect_startup_keymap,
        op_collect_startup_command,
        op_collect_startup_event,
        op_collect_startup_ftplugin_enabled,
        op_collect_startup_ftplugin_definition,
        op_collect_startup_ftplugin_disable,
        op_collect_startup_statusline,
        op_collect_startup_theme_palette,
        op_collect_startup_theme_ui,
        op_collect_startup_theme_syntax,
        op_collect_startup_theme_languages,
        op_collect_startup_theme_filer,
        op_collect_startup_theme_markdown,
        op_collect_startup_log_file,
        op_collect_startup_log_level,
        op_collect_startup_plugin_use,
        op_collect_startup_plugin_lazy,
        op_collect_startup_warning
    ],
    state = |state| state.put(StartupRegistry::default())
);
