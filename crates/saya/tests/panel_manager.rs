use saya::presentation::floating_window::{
    FloatingCursor, FloatingInlineStyle, FloatingInlineStyleKind,
};
use saya::presentation::panel::{
    PanelCloseBehavior, PanelContent, PanelContentRef, PanelManager, PanelNode, PanelOpenRequest,
    PanelPosition, PanelSize,
};
use saya::terminal::emulator::{TerminalCellStyle, TerminalColor};

#[test]
fn panel_manager_resolves_side_and_edge_panel_rects_without_using_float_identity() {
    let mut manager = PanelManager::default();

    let right = manager.open(PanelOpenRequest {
        id: "agent".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Percent(35),
        content: PanelContent::Lines {
            lines: vec!["codex".to_string()],
        },
        focus: true,
    });
    let bottom = manager.open(PanelOpenRequest {
        id: "logs".to_string(),
        position: PanelPosition::Bottom,
        size: PanelSize::Cells(5),
        content: PanelContent::Lines {
            lines: vec!["build".to_string()],
        },
        focus: false,
    });

    let models = manager.resolve_screen_models(100, 30);

    assert_eq!(right.numeric_id, 1);
    assert_eq!(bottom.numeric_id, 2);
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, right.numeric_id);
    assert_eq!(models[0].rect.x, 65);
    assert_eq!(models[0].rect.y, 0);
    assert_eq!(models[0].rect.width, 35);
    assert_eq!(models[0].rect.height, 30);
    assert_eq!(models[1].id, bottom.numeric_id);
    assert_eq!(models[1].rect.x, 0);
    assert_eq!(models[1].rect.y, 25);
    assert_eq!(models[1].rect.width, 100);
    assert_eq!(models[1].rect.height, 5);
    assert_eq!(manager.focused_panel_id(), Some("agent"));
}

#[test]
fn panel_manager_replaces_stable_plugin_id_and_routes_terminal_text() {
    let mut manager = PanelManager::default();
    let first = manager.open(PanelOpenRequest {
        id: "agent".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Cells(30),
        content: PanelContent::Terminal {
            terminal_id: 42,
            close_behavior: PanelCloseBehavior::Detach,
        },
        focus: true,
    });
    let second = manager.open(PanelOpenRequest {
        id: "agent".to_string(),
        position: PanelPosition::Left,
        size: PanelSize::Cells(20),
        content: PanelContent::Lines {
            lines: vec!["replacement".to_string()],
        },
        focus: false,
    });

    assert_eq!(first.numeric_id, second.numeric_id);
    assert!(manager.focus("agent"));
    assert_eq!(manager.focused_panel_id(), Some("agent"));
    assert_eq!(manager.focused_terminal_id(), None);
    assert_eq!(manager.send("agent", "hello\n").unwrap(), None);
    assert_eq!(
        manager.panel("agent").expect("panel should exist").content,
        PanelContentRef::Lines {
            lines: vec!["replacement".to_string()]
        }
    );

    manager.open(PanelOpenRequest {
        id: "agent".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Cells(30),
        content: PanelContent::Terminal {
            terminal_id: 77,
            close_behavior: PanelCloseBehavior::Kill,
        },
        focus: true,
    });
    assert_eq!(manager.focused_terminal_id(), Some(77));
    assert!(manager.unfocus());
    assert_eq!(manager.focused_panel_id(), None);
    assert_eq!(manager.focused_terminal_id(), None);
    assert!(!manager.unfocus());
    assert!(manager.focus("agent"));
    assert_eq!(manager.focused_terminal_id(), Some(77));
    assert_eq!(manager.send("agent", "hello\n").unwrap(), Some(77));
}

#[test]
fn panel_manager_requests_terminal_lines_for_visible_panel_content_height() {
    let mut manager = PanelManager::default();

    manager.open(PanelOpenRequest {
        id: "terminal-demo".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Percent(35),
        content: PanelContent::Terminal {
            terminal_id: 42,
            close_behavior: PanelCloseBehavior::Kill,
        },
        focus: true,
    });

    let requests = manager.terminal_view_requests(120, 40);

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].id, "terminal-demo");
    assert_eq!(requests[0].terminal_id, 42);
    assert_eq!(
        requests[0].content_width, 40,
        "right terminal panels should refresh all visible content columns inside the panel border"
    );
    assert_eq!(
        requests[0].content_height, 38,
        "terminal panels should refresh all visible content rows inside the panel border"
    );
}

#[test]
fn panel_manager_clamps_terminal_panel_content_height_to_one_row() {
    let mut manager = PanelManager::default();

    manager.open(PanelOpenRequest {
        id: "tiny-terminal".to_string(),
        position: PanelPosition::Bottom,
        size: PanelSize::Cells(1),
        content: PanelContent::Terminal {
            terminal_id: 7,
            close_behavior: PanelCloseBehavior::Kill,
        },
        focus: false,
    });

    let requests = manager.terminal_view_requests(120, 40);

    assert_eq!(requests[0].content_height, 1);
}

#[test]
fn panel_manager_projects_focused_terminal_cursor_from_terminal_content_ref() {
    let mut manager = PanelManager::default();

    manager.open(PanelOpenRequest {
        id: "terminal-demo".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Percent(35),
        content: PanelContent::Terminal {
            terminal_id: 42,
            close_behavior: PanelCloseBehavior::Kill,
        },
        focus: true,
    });
    assert!(manager.replace_terminal_lines(
        "terminal-demo",
        vec!["prompt".to_string(), "> input".to_string()]
    ));
    assert!(
        manager
            .replace_terminal_cursor("terminal-demo", Some(FloatingCursor { line: 1, column: 3 }))
    );

    let models = manager.resolve_floating_screen_models(120, 40);

    assert_eq!(
        models[0].cursor,
        Some(FloatingCursor { line: 1, column: 3 })
    );
}

#[test]
fn panel_manager_projects_terminal_inline_styles_to_floating_screen_model() {
    let mut manager = PanelManager::default();

    manager.open(PanelOpenRequest {
        id: "terminal-demo".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Percent(35),
        content: PanelContent::Terminal {
            terminal_id: 42,
            close_behavior: PanelCloseBehavior::Kill,
        },
        focus: true,
    });
    assert!(manager.replace_terminal_lines("terminal-demo", vec!["styled".to_string()]));
    let styles = vec![FloatingInlineStyle {
        kind: FloatingInlineStyleKind::TerminalCell(TerminalCellStyle {
            foreground: Some(TerminalColor::Indexed(1)),
            background: Some(TerminalColor::Indexed(4)),
            bold: true,
            underline: true,
            inverse: false,
        }),
        line: 0,
        column_start: 0,
        column_end: 6,
    }];
    assert!(manager.replace_terminal_inline_styles("terminal-demo", styles.clone()));

    let models = manager.resolve_floating_screen_models(120, 40);

    assert_eq!(models[0].inline_styles, styles);
}

#[test]
fn panel_manager_renders_structured_view_nodes_without_terminal_input_mode() {
    let mut manager = PanelManager::default();

    manager.open(PanelOpenRequest {
        id: "dashboard".to_string(),
        position: PanelPosition::Right,
        size: PanelSize::Percent(35),
        content: PanelContent::View {
            nodes: vec![
                PanelNode::Heading {
                    text: "Weather".to_string(),
                },
                PanelNode::Text {
                    text: "16C".to_string(),
                },
                PanelNode::Badge {
                    label: "rain".to_string(),
                },
                PanelNode::Progress {
                    label: Some("build".to_string()),
                    value: 50,
                },
                PanelNode::Divider,
                PanelNode::Button {
                    label: "Refresh".to_string(),
                },
                PanelNode::Image {
                    src: "/tmp/moon.png".to_string(),
                    alt: Some("Moon phase".to_string()),
                },
            ],
        },
        focus: true,
    });

    assert_eq!(manager.focused_panel_id(), Some("dashboard"));
    assert_eq!(manager.focused_terminal_id(), None);
    assert_eq!(manager.send("dashboard", "ignored").unwrap(), None);
    assert_eq!(
        manager.snapshots()[0].kind,
        "view",
        "view panels should be listed with an explicit content kind"
    );

    let models = manager.resolve_screen_models(100, 30);
    assert_eq!(
        models[0].lines,
        vec![
            "Weather".to_string(),
            "16C".to_string(),
            "[rain]".to_string(),
            "build [#####-----] 50%".to_string(),
            "--------".to_string(),
            "[ Refresh ]".to_string(),
            "[image: Moon phase] /tmp/moon.png".to_string(),
        ]
    );
    assert_eq!(
        models[0].content,
        PanelContentRef::View {
            nodes: vec![
                PanelNode::Heading {
                    text: "Weather".to_string(),
                },
                PanelNode::Text {
                    text: "16C".to_string(),
                },
                PanelNode::Badge {
                    label: "rain".to_string(),
                },
                PanelNode::Progress {
                    label: Some("build".to_string()),
                    value: 50,
                },
                PanelNode::Divider,
                PanelNode::Button {
                    label: "Refresh".to_string(),
                },
                PanelNode::Image {
                    src: "/tmp/moon.png".to_string(),
                    alt: Some("Moon phase".to_string()),
                },
            ],
        }
    );
}
