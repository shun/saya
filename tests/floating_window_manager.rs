use saya::floating_window::{
    FloatingAnchor, FloatingBorder, FloatingChrome, FloatingContentRef, FloatingFit,
    FloatingInputOutcome, FloatingLifecycle, FloatingLifecycleEvent, FloatingLifecycleOutcome,
    FloatingMouseOutcome, FloatingPlacement, FloatingRelativeTo, FloatingSize,
    FloatingWindowManager, FloatingZIndex, WorkspaceFocus,
};
use saya::input_router::KeyInput;
use saya::screen_model::PaneRect;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn pane(window_id: i32, x: u16, y: u16, width: u16, height: u16) -> (i32, PaneRect) {
    (
        window_id,
        PaneRect {
            x,
            y,
            width,
            height,
        },
    )
}

fn unique_log_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    std::env::temp_dir().join(format!("saya-floating-window-{name}-{nanos}.log"))
}

#[test]
fn static_lines_float_resolves_editor_relative_geometry_and_truncates_to_grid() {
    let mut manager = FloatingWindowManager::default();

    let id = manager.open_static_lines(
        vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ],
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Editor,
            anchor: FloatingAnchor::NorthWest,
            row: 2,
            col: 4,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: 20,
            height: 6,
        },
        FloatingChrome {
            border: FloatingBorder::Single,
        },
        FloatingZIndex::Hover,
        false,
    );

    let floats = manager.resolve_screen_models(12, 6, &[], None);

    assert_eq!(floats.len(), 1);
    assert_eq!(floats[0].id, id);
    assert_eq!(
        floats[0].rect,
        PaneRect {
            x: 4,
            y: 2,
            width: 8,
            height: 4,
        }
    );
    assert_eq!(floats[0].lines, vec!["first", "second", "third"]);
    assert_eq!(
        floats[0].content,
        FloatingContentRef::StaticLines { content_id: id.0 }
    );
}

#[test]
fn core_window_float_tracks_window_identity_and_rendered_buffer_lines() {
    let mut manager = FloatingWindowManager::default();

    let id = manager.open_core_window(
        42,
        FloatingPlacement::editor_at(1, 2),
        FloatingSize {
            width: 20,
            height: 4,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::User,
        true,
    );

    assert_eq!(
        manager.window_content(id),
        Some(&FloatingContentRef::CoreWindow { window_id: 42 })
    );
    assert!(manager.focus_float(id));
    assert_eq!(manager.focused_core_window_id(), Some(42));

    assert!(manager.replace_core_window_lines(
        id,
        vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
    ));

    let floats = manager.resolve_screen_models(80, 24, &[], None);
    assert_eq!(floats.len(), 1);
    assert_eq!(floats[0].lines, vec!["alpha", "beta", "gamma"]);
    assert_eq!(floats[0].zindex, FloatingZIndex::User.value());
}

#[test]
fn cursor_relative_float_uses_active_pane_cursor_and_anchor_math() {
    let mut manager = FloatingWindowManager::default();

    let id = manager.open_static_lines(
        vec!["hover".to_string()],
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Cursor { window_id: 7 },
            anchor: FloatingAnchor::SouthEast,
            row: -1,
            col: -2,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: 5,
            height: 3,
        },
        FloatingChrome {
            border: FloatingBorder::None,
        },
        FloatingZIndex::Hover,
        false,
    );

    let floats = manager.resolve_screen_models_with_cursors(
        80,
        24,
        &[pane(7, 10, 4, 40, 12)],
        &[(7, 8, 20)],
        Some(7),
    );

    assert_eq!(floats.len(), 1);
    assert_eq!(floats[0].id, id);
    assert_eq!(
        floats[0].rect,
        PaneRect {
            x: 24,
            y: 9,
            width: 5,
            height: 3,
        }
    );
}

#[test]
fn window_relative_float_uses_target_pane_rectangle() {
    let mut manager = FloatingWindowManager::default();

    manager.open_static_lines(
        vec!["window anchored".to_string()],
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Window { window_id: 3 },
            anchor: FloatingAnchor::NorthEast,
            row: 1,
            col: -2,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: 8,
            height: 4,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::User,
        false,
    );

    let floats = manager.resolve_screen_models(80, 24, &[pane(3, 5, 2, 30, 10)], Some(3));

    assert_eq!(
        floats[0].rect,
        PaneRect {
            x: 25,
            y: 3,
            width: 8,
            height: 4,
        }
    );
}

#[test]
fn zindex_and_creation_order_determine_render_and_hit_test_order() {
    let mut manager = FloatingWindowManager::default();

    let first = manager.open_static_lines(
        vec!["first".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        false,
    );
    let second = manager.open_static_lines(
        vec!["second".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Completion,
        true,
    );
    let third = manager.open_static_lines(
        vec!["third".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Completion,
        true,
    );

    let floats = manager.resolve_screen_models(80, 24, &[], None);

    assert_eq!(
        floats.iter().map(|float| float.id).collect::<Vec<_>>(),
        vec![first, second, third]
    );
    assert_eq!(
        manager.hit_test(2, 2, 80, 24, &[], None),
        Some(third),
        "newer float in the same z-index band should be topmost"
    );
}

#[test]
fn hit_test_ignores_non_focusable_float_and_falls_through() {
    let mut manager = FloatingWindowManager::default();

    manager.open_static_lines(
        vec!["pass through".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Completion,
        false,
    );

    assert_eq!(manager.hit_test(2, 2, 80, 24, &[], None), None);
}

#[test]
fn debug_log_records_open_resolve_and_hit_test_decisions() {
    let log_path = unique_log_path("manager");
    unsafe {
        std::env::set_var("SAYA_LOG_FILE", &log_path);
        std::env::set_var("SAYA_LOG", "debug");
    }
    let _ = saya::diagnostic_log::init_from_env();

    let mut manager = FloatingWindowManager::default();
    let hover = manager.open_static_lines(
        vec!["logged".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    let lifecycle = manager.open_static_lines_with_lifecycle(
        vec!["logged lifecycle".to_string()],
        FloatingLifecycle::CloseOnCursorMove,
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Cursor { window_id: 7 },
            anchor: FloatingAnchor::NorthWest,
            row: 0,
            col: 0,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    assert!(manager.focus_float(lifecycle));

    let _ = manager.resolve_screen_models(40, 10, &[], None);
    assert_eq!(manager.hit_test(2, 2, 40, 10, &[], None), Some(hover));
    manager.apply_lifecycle_event(
        FloatingLifecycleEvent::CursorMoved {
            window_id: 7,
            row: 1,
            col: 1,
        },
        Some(7),
    );

    let log = std::fs::read_to_string(&log_path).expect("floating window log should be written");
    assert!(
        log.contains("[floating_window] opening float"),
        "open decision log must be observable: {log}"
    );
    assert!(
        log.contains("[floating_window] resolved float"),
        "placement resolution log must be observable: {log}"
    );
    assert!(
        log.contains("[floating_window] hit test"),
        "hit-test decision log must be observable: {log}"
    );
    assert!(
        log.contains("[floating_window] lifecycle closing float"),
        "lifecycle close decision log must be observable: {log}"
    );
    assert!(
        log.contains("content_kind=static-lines"),
        "content kind must be present in lifecycle logs: {log}"
    );
    assert!(
        log.contains("trigger=CursorMoved"),
        "trigger must be present in lifecycle logs: {log}"
    );
    assert!(
        log.contains("focus_target=Pane"),
        "focus target must be present in lifecycle logs: {log}"
    );
}

#[test]
fn focused_static_lines_float_scrolls_visible_lines_and_clamps() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines(
        vec![
            "line-1".to_string(),
            "line-2".to_string(),
            "line-3".to_string(),
            "line-4".to_string(),
        ],
        FloatingPlacement::editor_at(0, 0),
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    assert!(manager.focus_float(id));

    assert_eq!(
        manager.handle_focused_static_lines_key(&KeyInput::PageDown),
        FloatingInputOutcome::Consumed
    );
    let floats = manager.resolve_screen_models(80, 24, &[], None);
    assert_eq!(floats[0].lines, vec!["line-2", "line-3", "line-4"]);

    assert_eq!(
        manager.handle_focused_static_lines_key(&KeyInput::PageDown),
        FloatingInputOutcome::Consumed
    );
    let floats = manager.resolve_screen_models(80, 24, &[], None);
    assert_eq!(
        floats[0].lines,
        vec!["line-2", "line-3", "line-4"],
        "scroll offset should clamp at max visible range"
    );

    assert_eq!(
        manager.handle_focused_static_lines_key(&KeyInput::Up),
        FloatingInputOutcome::Consumed
    );
    let floats = manager.resolve_screen_models(80, 24, &[], None);
    assert_eq!(
        floats[0].lines,
        vec!["line-1", "line-2", "line-3", "line-4"]
    );
}

#[test]
fn focused_static_lines_float_closes_on_escape_and_restores_pass_through() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines(
        vec!["close me".to_string()],
        FloatingPlacement::editor_at(0, 0),
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    assert!(manager.focus_float(id));

    assert_eq!(
        manager.handle_focused_static_lines_key(&KeyInput::Escape),
        FloatingInputOutcome::Closed { id }
    );

    assert!(manager.resolve_screen_models(80, 24, &[], None).is_empty());
    assert_eq!(manager.focus(), None);
    assert_eq!(
        manager.handle_focused_static_lines_key(&KeyInput::Down),
        FloatingInputOutcome::Ignored
    );
}

#[test]
fn focused_static_lines_float_restores_pane_focus_on_escape_when_requested() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines(
        vec!["close me".to_string()],
        FloatingPlacement::editor_at(0, 0),
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    assert!(manager.focus_float(id));

    assert_eq!(
        manager.handle_focused_static_lines_key_with_restore(&KeyInput::Escape, Some(12)),
        FloatingInputOutcome::Closed { id }
    );

    assert_eq!(
        manager.focus(),
        Some(WorkspaceFocus::Pane { window_id: 12 }),
        "closing a focused float should restore the requested pane focus"
    );
}

#[test]
fn close_focused_closes_current_float_and_restores_pane_focus() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines(
        vec!["close me".to_string()],
        FloatingPlacement::editor_at(0, 0),
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    assert!(manager.focus_float(id));

    assert_eq!(manager.close_focused(7), Some(id));

    assert!(manager.resolve_screen_models(80, 24, &[], None).is_empty());
    assert_eq!(manager.focus(), Some(WorkspaceFocus::Pane { window_id: 7 }));
}

#[test]
fn mouse_focus_uses_topmost_focusable_mouse_float() {
    let mut manager = FloatingWindowManager::default();
    let lower = manager.open_static_lines(
        vec!["lower".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    let upper = manager.open_static_lines(
        vec!["upper".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Completion,
        true,
    );

    assert_eq!(
        manager.focus_topmost_at(2, 2, 80, 24, &[], None),
        FloatingMouseOutcome::Focused { id: upper }
    );
    assert_eq!(
        manager.focus(),
        Some(WorkspaceFocus::Float { float_id: upper })
    );
    assert_ne!(lower, upper);
}

#[test]
fn mouse_focus_passes_through_non_mouse_float_to_lower_mouse_float() {
    let mut manager = FloatingWindowManager::default();
    let lower = manager.open_static_lines(
        vec!["lower".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    let upper = manager.open_static_lines(
        vec!["upper".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Completion,
        true,
    );
    assert!(manager.set_mouse_enabled(upper, false));

    assert_eq!(
        manager.focus_topmost_at(2, 2, 80, 24, &[], None),
        FloatingMouseOutcome::Focused { id: lower }
    );
    assert_eq!(
        manager.focus(),
        Some(WorkspaceFocus::Float { float_id: lower })
    );
}

#[test]
fn mouse_focus_passes_through_when_no_mouse_float_handles_cell() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines(
        vec!["pass through".to_string()],
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 10,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Completion,
        true,
    );
    assert!(manager.set_mouse_enabled(id, false));

    assert_eq!(
        manager.focus_topmost_at(2, 2, 80, 24, &[], None),
        FloatingMouseOutcome::PassThrough
    );
    assert_eq!(manager.focus(), None);
}

#[test]
fn non_focusable_float_cannot_take_focus() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines(
        vec!["pass through".to_string()],
        FloatingPlacement::editor_at(0, 0),
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        false,
    );

    assert!(!manager.focus_float(id));
    assert_eq!(manager.focus(), None);
    manager.clear_focus_to_pane(12);
    assert_eq!(
        manager.focus(),
        Some(WorkspaceFocus::Pane { window_id: 12 })
    );
}

#[test]
fn manual_lifecycle_is_default_and_survives_editor_events() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines(
        vec!["manual".to_string()],
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Cursor { window_id: 7 },
            anchor: FloatingAnchor::NorthWest,
            row: 0,
            col: 0,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    assert!(manager.focus_float(id));

    assert_eq!(
        manager.apply_lifecycle_event(
            FloatingLifecycleEvent::CursorMoved {
                window_id: 7,
                row: 1,
                col: 2,
            },
            Some(7),
        ),
        FloatingLifecycleOutcome { closed: vec![] }
    );
    assert_eq!(
        manager.apply_lifecycle_event(
            FloatingLifecycleEvent::InsertStarted { window_id: 7 },
            Some(7)
        ),
        FloatingLifecycleOutcome { closed: vec![] }
    );
    assert_eq!(
        manager.apply_lifecycle_event(
            FloatingLifecycleEvent::BufferChanged {
                buffer_id: 99,
                revision: 2,
            },
            Some(7),
        ),
        FloatingLifecycleOutcome { closed: vec![] }
    );

    assert_eq!(
        manager.resolve_screen_models(80, 24, &[pane(7, 0, 0, 40, 12)], Some(7))[0].id,
        id
    );
    assert_eq!(
        manager.focus(),
        Some(WorkspaceFocus::Float { float_id: id })
    );
}

#[test]
fn close_on_cursor_move_closes_matching_float_and_restores_pane_focus() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines_with_lifecycle(
        vec!["hover".to_string()],
        FloatingLifecycle::CloseOnCursorMove,
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Cursor { window_id: 7 },
            anchor: FloatingAnchor::NorthWest,
            row: 1,
            col: 0,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    assert!(manager.focus_float(id));

    assert_eq!(
        manager.apply_lifecycle_event(
            FloatingLifecycleEvent::CursorMoved {
                window_id: 7,
                row: 3,
                col: 4,
            },
            Some(7),
        ),
        FloatingLifecycleOutcome { closed: vec![id] }
    );

    assert!(
        manager
            .resolve_screen_models(80, 24, &[pane(7, 0, 0, 40, 12)], Some(7))
            .is_empty()
    );
    assert_eq!(manager.focus(), Some(WorkspaceFocus::Pane { window_id: 7 }));
}

#[test]
fn close_on_cursor_move_keeps_unrelated_window_float() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines_with_lifecycle(
        vec!["other-window-hover".to_string()],
        FloatingLifecycle::CloseOnCursorMove,
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Cursor { window_id: 8 },
            anchor: FloatingAnchor::NorthWest,
            row: 0,
            col: 0,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        false,
    );

    assert_eq!(
        manager.apply_lifecycle_event(
            FloatingLifecycleEvent::CursorMoved {
                window_id: 7,
                row: 3,
                col: 4,
            },
            Some(7),
        ),
        FloatingLifecycleOutcome { closed: vec![] }
    );
    assert_eq!(
        manager.resolve_screen_models(
            80,
            24,
            &[pane(7, 0, 0, 40, 12), pane(8, 40, 0, 40, 12)],
            Some(7)
        )[0]
        .id,
        id
    );
}

#[test]
fn close_on_insert_closes_matching_float() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines_with_lifecycle(
        vec!["insert hint".to_string()],
        FloatingLifecycle::CloseOnInsert,
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Window { window_id: 4 },
            anchor: FloatingAnchor::NorthWest,
            row: 0,
            col: 0,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::User,
        false,
    );

    assert_eq!(
        manager.apply_lifecycle_event(FloatingLifecycleEvent::InsertStarted { window_id: 4 }, None),
        FloatingLifecycleOutcome { closed: vec![id] }
    );
    assert!(
        manager
            .resolve_screen_models(80, 24, &[pane(4, 0, 0, 40, 12)], Some(4))
            .is_empty()
    );
}

#[test]
fn close_on_buffer_change_closes_buffer_sensitive_float() {
    let mut manager = FloatingWindowManager::default();
    let id = manager.open_static_lines_with_lifecycle(
        vec!["diagnostic".to_string()],
        FloatingLifecycle::CloseOnBufferChange,
        FloatingPlacement {
            relative_to: FloatingRelativeTo::BufferPosition {
                window_id: 5,
                line: 10,
                column: 2,
            },
            anchor: FloatingAnchor::NorthWest,
            row: 0,
            col: 0,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        false,
    );

    assert_eq!(
        manager.apply_lifecycle_event(
            FloatingLifecycleEvent::BufferChanged {
                buffer_id: 11,
                revision: 6,
            },
            None,
        ),
        FloatingLifecycleOutcome { closed: vec![id] }
    );
    assert!(
        manager
            .resolve_screen_models(80, 24, &[pane(5, 0, 0, 40, 12)], Some(5))
            .is_empty()
    );
}

#[test]
fn replace_by_group_replaces_only_matching_group() {
    let mut manager = FloatingWindowManager::default();
    let first_hover = manager.open_static_lines_with_lifecycle(
        vec!["old hover".to_string()],
        FloatingLifecycle::ReplaceByGroup("hover"),
        FloatingPlacement::editor_at(1, 1),
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );
    let completion = manager.open_static_lines_with_lifecycle(
        vec!["completion".to_string()],
        FloatingLifecycle::ReplaceByGroup("completion"),
        FloatingPlacement::editor_at(2, 1),
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Completion,
        false,
    );
    assert!(manager.focus_float(first_hover));

    let second_hover = manager.open_static_lines_with_lifecycle(
        vec!["new hover".to_string()],
        FloatingLifecycle::ReplaceByGroup("hover"),
        FloatingPlacement::editor_at(3, 1),
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        true,
    );

    let ids = manager
        .resolve_screen_models(80, 24, &[], None)
        .into_iter()
        .map(|float| float.id)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![second_hover, completion]);
    assert_eq!(
        manager.focus(),
        None,
        "replacing the focused float must clear stale float focus"
    );
}

#[test]
fn stale_hover_and_diagnostic_like_floats_close_on_cursor_move() {
    let mut manager = FloatingWindowManager::default();
    let hover = manager.open_static_lines_with_lifecycle(
        vec!["hover".to_string()],
        FloatingLifecycle::CloseOnCursorMove,
        FloatingPlacement {
            relative_to: FloatingRelativeTo::Cursor { window_id: 3 },
            anchor: FloatingAnchor::NorthWest,
            row: 0,
            col: 0,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        false,
    );
    let diagnostic = manager.open_static_lines_with_lifecycle(
        vec!["diagnostic".to_string()],
        FloatingLifecycle::CloseOnCursorMove,
        FloatingPlacement {
            relative_to: FloatingRelativeTo::BufferPosition {
                window_id: 3,
                line: 20,
                column: 2,
            },
            anchor: FloatingAnchor::NorthWest,
            row: 0,
            col: 0,
            fit: FloatingFit::TruncateToGrid,
        },
        FloatingSize {
            width: 20,
            height: 3,
        },
        FloatingChrome::borderless(),
        FloatingZIndex::Hover,
        false,
    );

    assert_eq!(
        manager.apply_lifecycle_event(
            FloatingLifecycleEvent::CursorMoved {
                window_id: 3,
                row: 21,
                col: 2,
            },
            None,
        ),
        FloatingLifecycleOutcome {
            closed: vec![hover, diagnostic]
        }
    );
    assert!(
        manager
            .resolve_screen_models(80, 24, &[pane(3, 0, 0, 40, 12)], Some(3))
            .is_empty()
    );
}
