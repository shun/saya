use super::*;

pub(super) fn visible_content_height_for(size: FloatingSize, chrome: FloatingChrome) -> u16 {
    match chrome.border {
        FloatingBorder::None => size.height.max(1),
        FloatingBorder::Single => size.height.saturating_sub(2).max(1),
    }
}

pub(super) fn visible_content_width_for(size: FloatingSize, chrome: FloatingChrome) -> u16 {
    match chrome.border {
        FloatingBorder::None => size.width.max(1),
        FloatingBorder::Single => size.width.saturating_sub(2).max(1),
    }
}

pub(super) fn resolve_float_rect(
    window: &FloatingWindow,
    terminal_width: u16,
    terminal_height: u16,
    panes: &[(i32, PaneRect)],
    cursors: &[(i32, u16, u16)],
    active_window_id: Option<i32>,
) -> Option<PaneRect> {
    if terminal_width == 0
        || terminal_height == 0
        || window.size.width == 0
        || window.size.height == 0
    {
        log::debug!(
            "[floating_window] skipped float with empty grid or size: id={}, grid=({},{}), size=({},{})",
            window.id.0,
            terminal_width,
            terminal_height,
            window.size.width,
            window.size.height
        );
        return None;
    }

    let target = resolve_target_rect(
        window.placement.relative_to,
        terminal_width,
        terminal_height,
        panes,
        cursors,
        active_window_id,
    )?;
    let (base_x, base_y) = anchor_origin(target, window.placement.anchor, window.size);
    let x = base_x.saturating_add(i32::from(window.placement.col));
    let y = base_y.saturating_add(i32::from(window.placement.row));
    let x = x.max(0).min(i32::from(terminal_width.saturating_sub(1))) as u16;
    let y = y.max(0).min(i32::from(terminal_height.saturating_sub(1))) as u16;
    let width = window.size.width.min(terminal_width.saturating_sub(x));
    let height = window.size.height.min(terminal_height.saturating_sub(y));

    (width > 0 && height > 0).then_some(PaneRect {
        x,
        y,
        width,
        height,
    })
}

pub(super) fn resolve_target_rect(
    relative_to: FloatingRelativeTo,
    terminal_width: u16,
    terminal_height: u16,
    panes: &[(i32, PaneRect)],
    cursors: &[(i32, u16, u16)],
    active_window_id: Option<i32>,
) -> Option<PaneRect> {
    match relative_to {
        FloatingRelativeTo::Editor => Some(PaneRect {
            x: 0,
            y: 0,
            width: terminal_width,
            height: terminal_height,
        }),
        FloatingRelativeTo::Window { window_id } => panes
            .iter()
            .find(|(id, _)| *id == window_id)
            .map(|(_, rect)| *rect),
        FloatingRelativeTo::Cursor { window_id } => {
            let window_id = active_window_id.unwrap_or(window_id);
            let pane = panes
                .iter()
                .find(|(id, _)| *id == window_id)
                .map(|(_, rect)| *rect)?;
            let (_, row, col) = cursors
                .iter()
                .find(|(id, _, _)| *id == window_id)
                .copied()
                .unwrap_or((window_id, 0, 0));
            Some(PaneRect {
                x: pane.x.saturating_add(col),
                y: pane.y.saturating_add(row),
                width: 1,
                height: 1,
            })
        }
        FloatingRelativeTo::BufferPosition { window_id, .. } => panes
            .iter()
            .find(|(id, _)| *id == window_id)
            .map(|(_, rect)| *rect),
    }
}

pub(super) fn anchor_origin(
    target: PaneRect,
    anchor: FloatingAnchor,
    size: FloatingSize,
) -> (i32, i32) {
    let target_x = i32::from(target.x);
    let target_y = i32::from(target.y);
    let target_right = target_x + i32::from(target.width);
    let target_bottom = target_y + i32::from(target.height);
    let width = i32::from(size.width);
    let height = i32::from(size.height);

    match anchor {
        FloatingAnchor::NorthWest => (target_x, target_y),
        FloatingAnchor::NorthEast => (target_right - width, target_y),
        FloatingAnchor::SouthWest => (target_x, target_bottom - height),
        FloatingAnchor::SouthEast => (target_right - width, target_bottom - height),
    }
}

pub(super) fn rect_contains(rect: PaneRect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

pub(super) fn lifecycle_matches_event(
    window: &FloatingWindow,
    event: FloatingLifecycleEvent,
) -> bool {
    match (window.lifecycle, event) {
        (FloatingLifecycle::Manual, _) | (FloatingLifecycle::ReplaceByGroup(_), _) => false,
        (
            FloatingLifecycle::CloseOnCursorMove,
            FloatingLifecycleEvent::CursorMoved { window_id, .. },
        ) => window_related_to_window(window, window_id),
        (FloatingLifecycle::CloseOnInsert, FloatingLifecycleEvent::InsertStarted { window_id }) => {
            window_related_to_window(window, window_id)
        }
        (FloatingLifecycle::CloseOnBufferChange, FloatingLifecycleEvent::BufferChanged { .. }) => {
            true
        }
        (FloatingLifecycle::CloseOnEvents(events), event) => {
            if !events.matches(event) {
                return false;
            }
            match event {
                FloatingLifecycleEvent::CursorMoved { window_id, .. }
                | FloatingLifecycleEvent::InsertStarted { window_id }
                | FloatingLifecycleEvent::ModeChanged { window_id, .. } => {
                    window_related_to_window(window, window_id)
                }
                FloatingLifecycleEvent::WindowLeft { from_window_id, .. } => {
                    window_related_to_window(window, from_window_id)
                }
                FloatingLifecycleEvent::BufferChanged { .. } => true,
            }
        }
        _ => false,
    }
}

pub(super) fn window_related_to_window(window: &FloatingWindow, window_id: i32) -> bool {
    match window.placement.relative_to {
        FloatingRelativeTo::Editor => true,
        FloatingRelativeTo::Window {
            window_id: related_window_id,
        }
        | FloatingRelativeTo::Cursor {
            window_id: related_window_id,
        }
        | FloatingRelativeTo::BufferPosition {
            window_id: related_window_id,
            ..
        } => related_window_id == window_id,
    }
}

pub(super) fn focus_target_name(restore_window_id: Option<i32>) -> &'static str {
    if restore_window_id.is_some() {
        "Pane"
    } else {
        "None"
    }
}
