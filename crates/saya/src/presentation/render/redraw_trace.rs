//! redraw 診断トレースの記録と観測カウンタ。
//!
//! トレースはログ出力と同時に常時コンパイルのカウンタへ集計され、
//! bin 側テストからも `redraw_trace_counts` で観測できる。
//! （lib の `#[cfg(test)]` 項目は bin テストから不可視のため、
//! カウンタは意図的に常時コンパイルとする。コストは atomic 加算のみ。）

use std::sync::atomic::{AtomicUsize, Ordering};

/// redraw トレースのカテゴリ別観測カウンタのスナップショット。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RedrawTraceCounts {
    pub command_line_only_overlay: usize,
    pub workspace_render_build_started: usize,
    pub renderer_frame_requested: usize,
    pub command_line_overlay_fallback: usize,
}

static COMMAND_LINE_ONLY_OVERLAY_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
static WORKSPACE_RENDER_BUILD_STARTED_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
static RENDERER_FRAME_REQUESTED_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
static COMMAND_LINE_OVERLAY_FALLBACK_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// redraw 診断トレースを記録する。ログ出力と観測カウンタ更新を同時に行う。
pub fn trace_redraw_diagnostic(args: std::fmt::Arguments<'_>) {
    let message = args.to_string();
    record_redraw_trace(&message);
    log::debug!("[redraw_diagnostic] {message}");
}

fn record_redraw_trace(message: &str) {
    if message.contains("workspace redraw skipped for command-line-only overlay") {
        COMMAND_LINE_ONLY_OVERLAY_TRACE_COUNT.fetch_add(1, Ordering::SeqCst);
    }
    if message.contains("workspace render build started") {
        WORKSPACE_RENDER_BUILD_STARTED_TRACE_COUNT.fetch_add(1, Ordering::SeqCst);
    }
    if message.contains("renderer frame requested") {
        RENDERER_FRAME_REQUESTED_TRACE_COUNT.fetch_add(1, Ordering::SeqCst);
    }
    if message.contains("command-line-only overlay fallback to workspace redraw") {
        COMMAND_LINE_OVERLAY_FALLBACK_TRACE_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

/// 観測カウンタをすべてゼロへ戻す。テストの前処理用。
pub fn reset_redraw_trace_counts() {
    COMMAND_LINE_ONLY_OVERLAY_TRACE_COUNT.store(0, Ordering::SeqCst);
    WORKSPACE_RENDER_BUILD_STARTED_TRACE_COUNT.store(0, Ordering::SeqCst);
    RENDERER_FRAME_REQUESTED_TRACE_COUNT.store(0, Ordering::SeqCst);
    COMMAND_LINE_OVERLAY_FALLBACK_TRACE_COUNT.store(0, Ordering::SeqCst);
}

/// 観測カウンタの現在値を返す。
pub fn redraw_trace_counts() -> RedrawTraceCounts {
    RedrawTraceCounts {
        command_line_only_overlay: COMMAND_LINE_ONLY_OVERLAY_TRACE_COUNT.load(Ordering::SeqCst),
        workspace_render_build_started: WORKSPACE_RENDER_BUILD_STARTED_TRACE_COUNT
            .load(Ordering::SeqCst),
        renderer_frame_requested: RENDERER_FRAME_REQUESTED_TRACE_COUNT.load(Ordering::SeqCst),
        command_line_overlay_fallback: COMMAND_LINE_OVERLAY_FALLBACK_TRACE_COUNT
            .load(Ordering::SeqCst),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redraw_trace_counts_matched_messages_per_category() {
        reset_redraw_trace_counts();

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
            redraw_trace_counts(),
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
        trace_redraw_diagnostic(format_args!("workspace render build started: revision=2"));

        reset_redraw_trace_counts();

        assert_eq!(redraw_trace_counts(), RedrawTraceCounts::default());
    }
}
