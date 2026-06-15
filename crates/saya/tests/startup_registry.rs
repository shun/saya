use saya::presentation::theme::{
    FilerSemanticStyleKey, MarkdownSemanticStyleKey, SyntaxSemanticStyleKey,
    ThemeTextStyleDeclaration, UiStyleKey,
};
use saya::runtime::startup::{
    FtPluginDefinition, FtPluginOption, FtPluginStartupAction, SayaKeyMode, SayaKeymapAction,
    StartupOptionName, StartupOptionValue, StartupPluginSource, StartupRegistryEntry,
    StatusLineConfig, StatusLineSegment, collect_startup_registry, evaluate_startup_module,
};

#[tokio::test(flavor = "current_thread")]
async fn startup_saya_namespace_is_available_to_top_level_module_code() {
    evaluate_startup_module(
        r#"
            if (typeof saya === "undefined") {
                throw new Error("saya namespace is missing");
            }
            saya.options.tabstop = 4;
        "#,
    )
    .await
    .expect("startup module should evaluate with saya namespace");
}

#[tokio::test(flavor = "current_thread")]
async fn startup_unknown_option_warns_without_failing_evaluation() {
    let registry = collect_startup_registry("saya.options.unknownoption = 4;")
        .await
        .expect("unknown option should warn without failing startup evaluation");

    assert!(
        registry.entries().iter().any(|entry| matches!(
            entry,
            StartupRegistryEntry::Warning { message }
                if message.contains("saya.options.unknownoption")
        )),
        "unknown option should be collected as a warning: {:?}",
        registry.entries()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_plugins_use_collects_local_plugin_declaration() {
    let registry = collect_startup_registry(
        r#"
            saya.plugins.use([
                { local: "~/.config/saya/plugins/workspace-tools" },
            ]);
        "#,
    )
    .await
    .expect("startup plugin use declaration should evaluate");

    assert!(registry.entries().iter().any(|entry| matches!(
        entry,
        StartupRegistryEntry::PluginUse { declaration }
            if declaration.name == "workspace-tools"
                && declaration.source == StartupPluginSource::Local {
                    path: "~/.config/saya/plugins/workspace-tools".to_string()
                }
                && declaration.module == "mod.ts"
                && declaration.setup == "setup"
                && declaration.commands.is_empty()
                && declaration.events.is_empty()
    )));
}

#[tokio::test(flavor = "current_thread")]
async fn startup_plugins_lazy_collects_github_triggers_and_options() {
    let registry = collect_startup_registry(
        r#"
            saya.plugins.lazy([
                {
                    github: "shun/saya-git-tools",
                    rev: "v0.1.0",
                    commands: ["GitStatus", "GitBlame"],
                    events: ["bufferOpen"],
                    options: { trace: "messages" },
                },
            ]);
        "#,
    )
    .await
    .expect("startup plugin lazy declaration should evaluate");

    assert!(registry.entries().iter().any(|entry| matches!(
        entry,
        StartupRegistryEntry::PluginLazy { declaration }
            if declaration.name == "saya-git-tools"
                && declaration.source == StartupPluginSource::Github {
                    repo: "shun/saya-git-tools".to_string(),
                    rev: Some("v0.1.0".to_string())
                }
                && declaration.commands == ["GitStatus".to_string(), "GitBlame".to_string()]
                && declaration.events == ["bufferOpen".to_string()]
                && declaration.options.as_ref()
                    .and_then(|options| options.get("trace"))
                    .and_then(|value| value.as_str()) == Some("messages")
    )));
}

#[tokio::test(flavor = "current_thread")]
async fn startup_plugins_rejects_ambiguous_source_declaration() {
    let error = collect_startup_registry(
        r#"
            saya.plugins.use([
                { local: "./plugins/a", github: "owner/repo" },
            ]);
        "#,
    )
    .await
    .expect_err("ambiguous plugin source should fail startup evaluation");

    assert!(
        error.contains("exactly one of local or github"),
        "unexpected error: {error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_statusline_segments_are_collected_for_plugin_customization() {
    let registry = collect_startup_registry(
        r#"
            saya.statusline.set({
                left: ["mode", "fileName"],
                right: ["filetype", "modified"],
            });
        "#,
    )
    .await
    .expect("startup statusline config");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::StatusLine {
            config: StatusLineConfig {
                left: vec![StatusLineSegment::Mode, StatusLineSegment::FileName],
                right: vec![StatusLineSegment::FileType, StatusLineSegment::Modified],
            },
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_ftplugin_controls_are_collected_for_plugin_customization() {
    let registry = collect_startup_registry(
        r#"
            saya.ftplugin.enabled = false;
            saya.ftplugin.set("go", {
                extensions: ["go", ".mod"],
                options: {
                    expandtab: false,
                    softtabstop: 0,
                    shiftwidth: 0,
                },
            });
            saya.ftplugin.disable("python");
        "#,
    )
    .await
    .expect("startup ftplugin config");

    assert_eq!(
        registry.entries(),
        &[
            StartupRegistryEntry::FtPlugin {
                action: FtPluginStartupAction::SetEnabled(false),
            },
            StartupRegistryEntry::FtPlugin {
                action: FtPluginStartupAction::SetDefinition(FtPluginDefinition {
                    filetype: "go".to_string(),
                    extensions: vec!["go".to_string(), "mod".to_string()],
                    options: vec![
                        FtPluginOption {
                            name: StartupOptionName::ExpandTab,
                            value: StartupOptionValue::Boolean(false),
                        },
                        FtPluginOption {
                            name: StartupOptionName::SoftTabStop,
                            value: StartupOptionValue::Number(0),
                        },
                        FtPluginOption {
                            name: StartupOptionName::ShiftWidth,
                            value: StartupOptionValue::Number(0),
                        },
                    ],
                    enabled: true,
                }),
            },
            StartupRegistryEntry::FtPlugin {
                action: FtPluginStartupAction::DisableFileType {
                    filetype: "python".to_string(),
                },
            },
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_saya_namespace_exposes_command_reference_helper_without_runtime_capabilities() {
    let result = evaluate_startup_module(
        r#"
            if (typeof saya === "undefined") {
                throw new Error("saya namespace is missing");
            }
            if (typeof saya.commands.execute !== "function") {
                throw new Error("startup command reference helper is missing");
            }
            const commandRef = saya.commands.execute("writeCurrent");
            if (commandRef !== "__SAYA_STARTUP_COMMAND_REF__:writeCurrent") {
                throw new Error(`unexpected command reference: ${commandRef}`);
            }
        "#,
    )
    .await;

    assert!(
        result.is_ok(),
        "startup namespace should expose only command reference helper semantics"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_surface_is_frozen_and_does_not_expose_runtime_api() {
    evaluate_startup_module(
        r#"
            if (!Object.isFrozen(saya)) {
                throw new Error("startup saya surface should be frozen");
            }
            if (!Object.isFrozen(saya.keymap)) {
                throw new Error("startup keymap surface should be frozen");
            }
            if (!Object.isFrozen(saya.commands)) {
                throw new Error("startup command surface should be frozen");
            }
            if (!Object.isFrozen(saya.events)) {
                throw new Error("startup event surface should be frozen");
            }
            if (!Object.isFrozen(saya.statusline)) {
                throw new Error("startup statusline surface should be frozen");
            }
            if (!Object.isFrozen(saya.ftplugin)) {
                throw new Error("startup ftplugin surface should be frozen");
            }
            if (!Object.isFrozen(saya.theme)) {
                throw new Error("startup theme surface should be frozen");
            }
            if (!Object.isFrozen(saya.plugins)) {
                throw new Error("startup plugins surface should be frozen");
            }
            if (typeof saya.buffer !== "undefined") {
                throw new Error("runtime buffer api leaked into startup namespace");
            }
            if (typeof saya.window !== "undefined") {
                throw new Error("runtime window api leaked into startup namespace");
            }
            if (typeof saya.editor !== "undefined") {
                throw new Error("runtime editor api leaked into startup namespace");
            }
            if (typeof saya.commands.execute !== "function") {
                throw new Error("startup command reference helper is missing");
            }
        "#,
    )
    .await
    .expect("startup surface should stay separated from runtime surface");
}

#[tokio::test(flavor = "current_thread")]
async fn startup_theme_palette_and_markdown_styles_are_collected() {
    let registry = collect_startup_registry(
        r##"
            saya.theme.palette = {
                accent: "#7aa2f7",
                heading2: "#9ece6a",
                code: "#ff9e64",
                link: "#2ac3de",
            };
            saya.theme.markdown = {
                heading: { fg: "accent", bold: true },
                heading2: { fg: "heading2", underline: true },
                inlineCode: { fg: "code" },
                link: { fg: "link", underline: true },
            };
        "##,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[
            StartupRegistryEntry::ThemePalette {
                name: "accent".to_string(),
                value: "#7aa2f7".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "heading2".to_string(),
                value: "#9ece6a".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "code".to_string(),
                value: "#ff9e64".to_string(),
            },
            StartupRegistryEntry::ThemePalette {
                name: "link".to_string(),
                value: "#2ac3de".to_string(),
            },
            StartupRegistryEntry::ThemeMarkdownStyle {
                key: MarkdownSemanticStyleKey::Heading,
                style: ThemeTextStyleDeclaration {
                    fg: Some("accent".to_string()),
                    bold: Some(true),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
            StartupRegistryEntry::ThemeMarkdownStyle {
                key: MarkdownSemanticStyleKey::Heading2,
                style: ThemeTextStyleDeclaration {
                    fg: Some("heading2".to_string()),
                    underline: Some(true),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
            StartupRegistryEntry::ThemeMarkdownStyle {
                key: MarkdownSemanticStyleKey::InlineCode,
                style: ThemeTextStyleDeclaration {
                    fg: Some("code".to_string()),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
            StartupRegistryEntry::ThemeMarkdownStyle {
                key: MarkdownSemanticStyleKey::Link,
                style: ThemeTextStyleDeclaration {
                    fg: Some("link".to_string()),
                    underline: Some(true),
                    ..ThemeTextStyleDeclaration::default()
                },
            },
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_theme_ui_and_syntax_styles_are_collected() {
    let registry = collect_startup_registry(
        r##"
            saya.theme.palette = {
                fg: "#c0caf5",
                bg: "#24283b",
                comment: "#565f89",
                keyword: "#bb9af7",
            };
            saya.theme.ui = {
                text: { fg: "fg", bg: "bg" },
                statusActive: { fg: "bg", bg: "fg", bold: true },
            };
            saya.theme.syntax = {
                comment: { fg: "comment", italic: true },
                statement: { fg: "keyword", bold: true },
            };
        "##,
    )
    .await
    .expect("startup theme ui and syntax config should evaluate");

    assert!(registry.entries().iter().any(|entry| {
        matches!(
            entry,
            StartupRegistryEntry::ThemeUiStyle {
                key: UiStyleKey::Text,
                style,
            } if style.fg.as_deref() == Some("fg") && style.bg.as_deref() == Some("bg")
        )
    }));
    assert!(registry.entries().iter().any(|entry| {
        matches!(
            entry,
            StartupRegistryEntry::ThemeSyntaxStyle {
                key: SyntaxSemanticStyleKey::Statement,
                style,
            } if style.fg.as_deref() == Some("keyword") && style.bold == Some(true)
        )
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn startup_theme_language_syntax_and_filer_styles_are_collected() {
    let registry = collect_startup_registry(
        r##"
            saya.theme.languages = {
                go: {
                    syntax: {
                        function: { fg: "#7aa2f7", bold: true },
                    },
                },
            };
            saya.theme.filer = {
                directory: { fg: "#7aa2f7", bold: true },
                marked: { bg: "#33467c" },
            };
        "##,
    )
    .await
    .expect("startup theme language syntax and filer config should evaluate");

    assert!(registry.entries().iter().any(|entry| {
        matches!(
            entry,
            StartupRegistryEntry::ThemeLanguageSyntaxStyle {
                language,
                key: SyntaxSemanticStyleKey::Function,
                style,
            } if language == "go" && style.fg.as_deref() == Some("#7aa2f7") && style.bold == Some(true)
        )
    }));
    assert!(registry.entries().iter().any(|entry| {
        matches!(
            entry,
            StartupRegistryEntry::ThemeFilerStyle {
                key: FilerSemanticStyleKey::Marked,
                style,
            } if style.bg.as_deref() == Some("#33467c")
        )
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn startup_tab_size_is_collected_in_source_order_and_is_deterministic() {
    let source = r#"
        saya.options.tabstop = 4;
        saya.options.tabstop = 6;
    "#;

    let first = collect_startup_registry(source)
        .await
        .expect("startup registry");
    let second = collect_startup_registry(source)
        .await
        .expect("startup registry");

    assert_eq!(first, second, "same source should yield the same registry");
    assert_eq!(
        first.entries(),
        &[
            StartupRegistryEntry::Option {
                name: StartupOptionName::TabSize,
                value: StartupOptionValue::Number(4),
            },
            StartupRegistryEntry::Option {
                name: StartupOptionName::TabSize,
                value: StartupOptionValue::Number(6),
            },
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_number_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.number = true;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Option {
            name: StartupOptionName::LineNumbers,
            value: StartupOptionValue::Boolean(true),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_numberwidth_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.numberwidth = 6;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Option {
            name: StartupOptionName::NumberWidth,
            value: StartupOptionValue::Number(6),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_hlsearch_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.hlsearch = true;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Option {
            name: StartupOptionName::HlSearch,
            value: StartupOptionValue::Boolean(true),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_hlsearch_alias_hls_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.hls = true;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Option {
            name: StartupOptionName::HlSearch,
            value: StartupOptionValue::Boolean(true),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_cmdheight_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.cmdheight = 3;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Option {
            name: StartupOptionName::MessageHeight,
            value: StartupOptionValue::Number(3),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_mermaidpreview_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.mermaidpreview = false;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Option {
            name: StartupOptionName::MermaidPreview,
            value: StartupOptionValue::Boolean(false),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_mermaid_preview_size_percent_options_are_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.mermaidpreviewwidth = 72;
            saya.options.mermaidpreviewheight = 64;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[
            StartupRegistryEntry::Option {
                name: StartupOptionName::MermaidPreviewWidth,
                value: StartupOptionValue::Number(72),
            },
            StartupRegistryEntry::Option {
                name: StartupOptionName::MermaidPreviewHeight,
                value: StartupOptionValue::Number(64),
            },
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_mermaid_preview_background_option_is_collected() {
    let registry = collect_startup_registry(
        r##"
            saya.options.mermaidpreviewbackground = "#ffffff";
        "##,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Option {
            name: StartupOptionName::MermaidPreviewBackground,
            value: StartupOptionValue::String("#ffffff".to_string()),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_syntax_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.options.syntax = true;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Option {
            name: StartupOptionName::Syntax,
            value: StartupOptionValue::Boolean(true),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_smartindent_and_alias_are_collected_as_vim_style_boolean_options() {
    let registry = collect_startup_registry(
        r#"
            saya.options.smartindent = true;
            saya.options.si = false;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[
            StartupRegistryEntry::Option {
                name: StartupOptionName::SmartIndent,
                value: StartupOptionValue::Boolean(true),
            },
            StartupRegistryEntry::Option {
                name: StartupOptionName::SmartIndent,
                value: StartupOptionValue::Boolean(false),
            },
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_option_aliases_are_normalized_to_formal_names() {
    let registry = collect_startup_registry(
        r#"
            saya.options.tabstop = 2;
            saya.options.number = true;
            saya.options.nuw = 5;
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[
            StartupRegistryEntry::Option {
                name: StartupOptionName::TabSize,
                value: StartupOptionValue::Number(2),
            },
            StartupRegistryEntry::Option {
                name: StartupOptionName::LineNumbers,
                value: StartupOptionValue::Boolean(true),
            },
            StartupRegistryEntry::Option {
                name: StartupOptionName::NumberWidth,
                value: StartupOptionValue::Number(5),
            },
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_keymap_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.keymap.set("normal", "x", "dd");
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Keymap {
            mode: SayaKeyMode::Normal,
            lhs: "x".to_string(),
            action: SayaKeymapAction::Literal("dd".to_string()),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_keymap_registered_command_reference_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.keymap.set("normal", "<leader>w", saya.commands.execute("writeCurrent"));
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Keymap {
            mode: SayaKeyMode::Normal,
            lhs: "<leader>w".to_string(),
            action: SayaKeymapAction::RegisteredCommand("writeCurrent".to_string()),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_keymap_can_bind_manual_mermaid_preview_command() {
    let registry = collect_startup_registry(
        r#"
            saya.keymap.set("normal", "gm", saya.commands.execute("markdown.previewMermaid"));
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Keymap {
            mode: SayaKeyMode::Normal,
            lhs: "gm".to_string(),
            action: SayaKeymapAction::RegisteredCommand("markdown.previewMermaid".to_string()),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_log_file_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.log.file = "/tmp/saya-from-init.log";
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::LogFile {
            path: "/tmp/saya-from-init.log".to_string(),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_log_level_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.log.level = "warn";
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::LogLevel {
            level: log::LevelFilter::Warn,
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_command_registration_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.commands.register("writeCurrent", () => {
                console.log("write");
            });
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Command {
            name: "writeCurrent".to_string(),
            callback_source: "() => {\n                console.log(\"write\");\n            }"
                .to_string(),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn startup_event_subscription_is_collected() {
    let registry = collect_startup_registry(
        r#"
            saya.events.on("bufferOpen", (payload) => {
                console.log(payload);
            });
        "#,
    )
    .await
    .expect("startup registry");

    assert_eq!(
        registry.entries(),
        &[StartupRegistryEntry::Event {
            name: "bufferOpen".to_string(),
            callback_source: "(payload) => {\n                console.log(payload);\n            }"
                .to_string(),
        }]
    );
}
