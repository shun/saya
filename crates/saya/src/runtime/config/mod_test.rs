use std::path::PathBuf;

use super::*;

// ==== タスク 8.2: JSON フォールバックパーサを実 production 経路で評価する ====
//
// これらは `evaluate_capability_source`（本番で JSON 設定を扱う経路）を直接駆動し、
// 手書き JSON パーサ (`parse_config_json` / `parse_capability_program`) の回帰を検出する。

#[test]
fn evaluate_capability_source_parses_json_tabstop_option() {
    let source = ConfigSourceResult::Loaded {
        path: PathBuf::from("test.json"),
        source: "{ \"tabstop\": 4 }".to_string(),
    };

    let result = evaluate_capability_source(&source);

    match result {
        CapabilityLoadResult::Success { commands, .. } => {
            assert_eq!(commands.len(), 1);
            assert_eq!(
                commands[0],
                ConfigCommand::SetOption {
                    name: ConfigOptionName::TabSize,
                    value: ConfigOptionValue::Number(4),
                },
                "tabstop オプションが正しくパースされること"
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn evaluate_capability_source_parses_json_number_option() {
    let source = ConfigSourceResult::Loaded {
        path: PathBuf::from("test.json"),
        source: "{ \"number\": true }".to_string(),
    };

    let result = evaluate_capability_source(&source);

    match result {
        CapabilityLoadResult::Success { commands, .. } => {
            assert_eq!(commands.len(), 1);
            assert_eq!(
                commands[0],
                ConfigCommand::SetOption {
                    name: ConfigOptionName::LineNumbers,
                    value: ConfigOptionValue::Boolean(true),
                }
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn evaluate_capability_source_parses_json_numberwidth_option() {
    let source = ConfigSourceResult::Loaded {
        path: PathBuf::from("test.json"),
        source: "{ \"numberwidth\": 6 }".to_string(),
    };

    let result = evaluate_capability_source(&source);

    match result {
        CapabilityLoadResult::Success { commands, .. } => {
            assert_eq!(commands.len(), 1);
            assert_eq!(
                commands[0],
                ConfigCommand::SetOption {
                    name: ConfigOptionName::NumberWidth,
                    value: ConfigOptionValue::Number(6),
                }
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn evaluate_capability_source_parses_multiple_json_options() {
    let source = ConfigSourceResult::Loaded {
        path: PathBuf::from("test.json"),
        source: "{ \"tabstop\": 2, \"number\": false }".to_string(),
    };

    let result = evaluate_capability_source(&source);

    match result {
        CapabilityLoadResult::Success { commands, .. } => {
            assert_eq!(commands.len(), 2, "複数オプションが全てパースされること");
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn evaluate_capability_source_rejects_vim_script_syntax() {
    let source = ConfigSourceResult::Loaded {
        path: PathBuf::from("init.vim"),
        source: "set tabstop=4\nset number\n".to_string(),
    };

    let result = evaluate_capability_source(&source);

    match result {
        CapabilityLoadResult::EvalFailed { path, message } => {
            assert_eq!(path, PathBuf::from("init.vim"));
            assert!(
                message.contains("Vim script"),
                "Vim script 拒否メッセージを含むこと: {}",
                message
            );
        }
        other => panic!("Vim script は EvalFailed を返すこと, got: {:?}", other),
    }
}

#[test]
fn evaluate_capability_source_rejects_noremap_vim_script() {
    let source = ConfigSourceResult::Loaded {
        path: PathBuf::from("config.vim"),
        source: "nnoremap <leader>f :Files<CR>".to_string(),
    };

    let result = evaluate_capability_source(&source);

    assert!(
        matches!(result, CapabilityLoadResult::EvalFailed { .. }),
        "noremap 構文は拒否されること"
    );
}

#[test]
fn evaluate_capability_source_returns_empty_commands_for_empty_config() {
    let source = ConfigSourceResult::Loaded {
        path: PathBuf::from("empty.json"),
        source: "{}".to_string(),
    };

    let result = evaluate_capability_source(&source);

    match result {
        CapabilityLoadResult::Success { commands, .. } => {
            assert!(
                commands.is_empty(),
                "空の設定は空のコマンドリストを返すこと"
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn evaluate_capability_source_returns_default_used_for_no_config() {
    let source = ConfigSourceResult::Default;

    let result = evaluate_capability_source(&source);

    assert_eq!(
        result,
        CapabilityLoadResult::DefaultUsed,
        "設定なしは DefaultUsed を返すこと"
    );
}

#[test]
fn evaluate_capability_source_propagates_read_failure() {
    let source = ConfigSourceResult::ReadFailed {
        path: PathBuf::from("missing.json"),
        message: "file not found".to_string(),
    };

    let result = evaluate_capability_source(&source);

    assert_eq!(
        result,
        CapabilityLoadResult::ReadFailed {
            path: PathBuf::from("missing.json"),
            message: "file not found".to_string(),
        },
        "読み込み失敗がそのまま伝播すること"
    );
}

// ==== タスク 1.x / 2.x / 5.x: TypeScript-first capability API ====

#[test]
fn evaluate_capability_source_parses_startup_registry_in_source_order() {
    let source = ConfigSourceResult::Loaded {
        path: PathBuf::from("init.ts"),
        source: r#"
                saya.options.tabstop = 4;
                saya.options.number = true;
                saya.options.numberwidth = 6;
                saya.options.cmdheight = 3;
                saya.keymap.set("normal", "x", "dd");
                saya.commands.register("writeCurrent", () => {
                    saya.commands.execute("write");
                });
                saya.events.on("bufferOpen", (payload) => {
                    console.log(payload);
                });
            "#
        .to_string(),
    };

    let result = evaluate_capability_source(&source);

    match result {
        CapabilityLoadResult::Success {
            registry, commands, ..
        } => {
            assert_eq!(
                commands.len(),
                4,
                "startup option は 4 件の command に正規化されること"
            );
            assert_eq!(
                registry.entries(),
                &[
                    StartupRegistryEntry::Option {
                        name: SayaOptionName::TabSize,
                        value: SayaOptionValue::Number(4),
                    },
                    StartupRegistryEntry::Option {
                        name: SayaOptionName::LineNumbers,
                        value: SayaOptionValue::Boolean(true),
                    },
                    StartupRegistryEntry::Option {
                        name: SayaOptionName::NumberWidth,
                        value: SayaOptionValue::Number(6),
                    },
                    StartupRegistryEntry::Option {
                        name: SayaOptionName::MessageHeight,
                        value: SayaOptionValue::Number(3),
                    },
                    StartupRegistryEntry::Keymap {
                        mode: SayaKeyMode::Normal,
                        lhs: "x".to_string(),
                        action: SayaKeymapAction::Literal("dd".to_string()),
                    },
                    StartupRegistryEntry::Command {
                        name: "writeCurrent".to_string(),
                        callback_source: "saya.commands.execute(\"write\");".to_string(),
                    },
                    StartupRegistryEntry::Event {
                        name: "bufferOpen".to_string(),
                        callback_source: "console.log(payload);".to_string(),
                    },
                ]
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn evaluate_capability_source_rejects_runtime_only_surface_at_startup() {
    let source = ConfigSourceResult::Loaded {
        path: PathBuf::from("init.ts"),
        source: r#"
                saya.commands.execute("write");
            "#
        .to_string(),
    };

    let result = evaluate_capability_source(&source);

    assert!(matches!(
        result,
        CapabilityLoadResult::UnsupportedCapability { .. }
    ));
}

#[test]
fn evaluate_capability_source_normalizes_vim_aliases_to_formal_names() {
    let source = ConfigSourceResult::Loaded {
        path: PathBuf::from("init.ts"),
        source: r#"
                saya.options.tabstop = 2;
                saya.options.number = true;
                saya.options.nuw = 5;
            "#
        .to_string(),
    };

    let result = evaluate_capability_source(&source);

    match result {
        CapabilityLoadResult::Success {
            registry, commands, ..
        } => {
            assert_eq!(
                commands.len(),
                3,
                "alias option も既存 boot 経路向け command に正規化されること"
            );
            assert_eq!(
                registry.entries(),
                &[
                    StartupRegistryEntry::Option {
                        name: SayaOptionName::TabSize,
                        value: SayaOptionValue::Number(2),
                    },
                    StartupRegistryEntry::Option {
                        name: SayaOptionName::LineNumbers,
                        value: SayaOptionValue::Boolean(true),
                    },
                    StartupRegistryEntry::Option {
                        name: SayaOptionName::NumberWidth,
                        value: SayaOptionValue::Number(5),
                    },
                ]
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

#[test]
fn evaluate_capability_source_rejects_filesystem_and_network_capabilities() {
    let source = ConfigSourceResult::Loaded {
        path: PathBuf::from("init.ts"),
        source: r#"
                saya.filesystem.readText("/tmp/notes.txt");
            "#
        .to_string(),
    };

    let result = evaluate_capability_source(&source);

    assert!(matches!(
        result,
        CapabilityLoadResult::UnsupportedCapability { .. }
    ));
}

#[test]
fn evaluate_capability_source_normalizes_ts_option_to_boot_command() {
    let source = ConfigSourceResult::Loaded {
        path: PathBuf::from("init.ts"),
        source: "saya.options.tabstop = 6;".to_string(),
    };

    let result = evaluate_capability_source(&source);

    match result {
        CapabilityLoadResult::Success { commands, .. } => {
            assert_eq!(
                commands,
                vec![ConfigCommand::SetOption {
                    name: ConfigOptionName::TabSize,
                    value: ConfigOptionValue::Number(6),
                }]
            );
        }
        other => panic!("Success を返すこと, got: {:?}", other),
    }
}

// ==== タスク 8.3: 設定コマンドを起動時の editor 状態へ適用する ====

#[test]
fn apply_tab_size_command_updates_state() {
    let commands = vec![ConfigCommand::SetOption {
        name: ConfigOptionName::TabSize,
        value: ConfigOptionValue::Number(4),
    }];
    let mut state = ConfigApplyState::default_state();
    assert_eq!(state.tab_size, 8, "既定値は 8 であること");

    let result = apply_config_commands(&commands, &mut state);

    assert_eq!(state.tab_size, 4, "tabstop が 4 に変更されること");
    assert!(result.is_fully_applied());
    assert_eq!(result.applied_count, 1);
}

#[test]
fn apply_line_numbers_command_updates_state() {
    let commands = vec![ConfigCommand::SetOption {
        name: ConfigOptionName::LineNumbers,
        value: ConfigOptionValue::Boolean(true),
    }];
    let mut state = ConfigApplyState::default_state();
    assert!(!state.line_numbers, "既定値は false であること");

    let result = apply_config_commands(&commands, &mut state);

    assert!(state.line_numbers, "number が true に変更されること");
    assert!(result.is_fully_applied());
}

#[test]
fn apply_number_width_command_updates_state() {
    let commands = vec![ConfigCommand::SetOption {
        name: ConfigOptionName::NumberWidth,
        value: ConfigOptionValue::Number(6),
    }];
    let mut state = ConfigApplyState::default_state();
    assert_eq!(state.number_width, 4, "既定値は 4 であること");

    let result = apply_config_commands(&commands, &mut state);

    assert_eq!(state.number_width, 6, "numberwidth が 6 に変更されること");
    assert!(result.is_fully_applied());
}

#[test]
fn apply_message_height_command_updates_state() {
    let commands = vec![ConfigCommand::SetOption {
        name: ConfigOptionName::MessageHeight,
        value: ConfigOptionValue::Number(3),
    }];
    let mut state = ConfigApplyState::default_state();
    assert_eq!(state.message_height, 5, "既定値は 5 であること");

    let result = apply_config_commands(&commands, &mut state);

    assert_eq!(state.message_height, 3, "cmdheight が 3 に変更されること");
    assert!(result.is_fully_applied());
}

#[test]
fn apply_key_mapping_command_adds_to_state() {
    let commands = vec![ConfigCommand::MapKey {
        mode: ConfigKeyMode::Normal,
        lhs: "<leader>f".to_string(),
        rhs: ":find ".to_string(),
    }];
    let mut state = ConfigApplyState::default_state();

    let result = apply_config_commands(&commands, &mut state);

    assert_eq!(state.key_mappings.len(), 1);
    assert_eq!(state.key_mappings[0].lhs, "<leader>f");
    assert_eq!(state.key_mappings[0].rhs, ":find ");
    assert_eq!(state.key_mappings[0].mode, ConfigKeyMode::Normal);
    assert!(result.is_fully_applied());
}

#[test]
fn apply_commands_in_deterministic_order() {
    let commands = vec![
        ConfigCommand::SetOption {
            name: ConfigOptionName::TabSize,
            value: ConfigOptionValue::Number(2),
        },
        ConfigCommand::SetOption {
            name: ConfigOptionName::LineNumbers,
            value: ConfigOptionValue::Boolean(true),
        },
        ConfigCommand::MapKey {
            mode: ConfigKeyMode::Insert,
            lhs: "jk".to_string(),
            rhs: "\x1b".to_string(),
        },
    ];
    let mut state = ConfigApplyState::default_state();

    let result = apply_config_commands(&commands, &mut state);

    // 適用順が固定されていること
    assert_eq!(state.tab_size, 2);
    assert!(state.line_numbers);
    assert_eq!(state.key_mappings.len(), 1);
    assert_eq!(result.applied_count, 3);
    assert!(result.is_fully_applied());
}

#[test]
fn apply_rejects_invalid_tab_size() {
    let commands = vec![ConfigCommand::SetOption {
        name: ConfigOptionName::TabSize,
        value: ConfigOptionValue::Number(0),
    }];
    let mut state = ConfigApplyState::default_state();

    let result = apply_config_commands(&commands, &mut state);

    assert!(!result.is_fully_applied());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(state.tab_size, 8, "不正な値の場合は既定値が維持されること");
}

#[test]
fn apply_rejects_type_mismatch() {
    let commands = vec![ConfigCommand::SetOption {
        name: ConfigOptionName::TabSize,
        value: ConfigOptionValue::Boolean(true),
    }];
    let mut state = ConfigApplyState::default_state();

    let result = apply_config_commands(&commands, &mut state);

    assert!(!result.is_fully_applied());
    assert_eq!(result.errors.len(), 1);
}

#[test]
fn apply_empty_commands_is_noop() {
    let commands: Vec<ConfigCommand> = Vec::new();
    let mut state = ConfigApplyState::default_state();

    let result = apply_config_commands(&commands, &mut state);

    assert!(result.is_fully_applied());
    assert_eq!(result.applied_count, 0);
    assert_eq!(state.tab_size, 8, "空コマンドでは状態が変わらないこと");
}

// 設定ファイル -> 初期 options/状態への反映と失敗時 fallback は、
// 本番起動経路 `prepare_launch`（実 deno_core 経由）を駆動する
// `tests/integration_config.rs` で検証する。
