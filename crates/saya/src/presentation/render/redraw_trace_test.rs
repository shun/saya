use super::*;
use std::sync::{Mutex, OnceLock};

fn redraw_trace_observation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn redraw_trace_counts_matched_messages_per_category() {
    let _guard = redraw_trace_observation_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reset_redraw_trace_diagnostic_counts();

    trace_redraw_diagnostic(format_args!(
        "workspace redraw skipped for command-line-only overlay: command_prompt=Some(':')"
    ));
    trace_redraw_diagnostic(format_args!("workspace render build started: revision=1"));
    trace_redraw_diagnostic(format_args!("renderer frame requested: panes=1"));
    trace_redraw_diagnostic(format_args!(
        "command-line-only overlay fallback to workspace redraw: error=Io"
    ));
    trace_redraw_diagnostic(format_args!("unrelated diagnostic message"));

    assert_eq!(
        redraw_trace_diagnostic_counts(),
        RedrawTraceCounts {
            command_line_only_overlay: 1,
            workspace_render_build_started: 1,
            renderer_frame_requested: 1,
            command_line_overlay_fallback: 1,
        }
    );
}

#[test]
fn reset_clears_all_redraw_trace_counts() {
    let _guard = redraw_trace_observation_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    trace_redraw_diagnostic(format_args!("workspace render build started: revision=2"));

    reset_redraw_trace_diagnostic_counts();

    assert_eq!(
        redraw_trace_diagnostic_counts(),
        RedrawTraceCounts::default()
    );
}
