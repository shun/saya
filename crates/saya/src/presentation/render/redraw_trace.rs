//! redraw 診断トレースの記録と観測カウンタ。
//!
//! トレースはログ出力と同時に常時コンパイルのカウンタへ集計され、
//! 実行ファイル境界の診断やホスト orchestration の検証からも観測できる。
//! カウンタは意図的に常時コンパイルとする。コストは atomic 加算のみ。

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

/// 観測カウンタをすべてゼロへ戻す。
pub fn reset_redraw_trace_diagnostic_counts() {
    COMMAND_LINE_ONLY_OVERLAY_TRACE_COUNT.store(0, Ordering::SeqCst);
    WORKSPACE_RENDER_BUILD_STARTED_TRACE_COUNT.store(0, Ordering::SeqCst);
    RENDERER_FRAME_REQUESTED_TRACE_COUNT.store(0, Ordering::SeqCst);
    COMMAND_LINE_OVERLAY_FALLBACK_TRACE_COUNT.store(0, Ordering::SeqCst);
}

/// 観測カウンタの現在値を返す。
pub fn redraw_trace_diagnostic_counts() -> RedrawTraceCounts {
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
#[path = "redraw_trace_test.rs"]
mod tests;
