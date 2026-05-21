use saya::presentation::panel::{
    PanelCloseBehavior, PanelContent, PanelContentRef, PanelManager, PanelOpenRequest,
    PanelPosition, PanelSize,
};

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
