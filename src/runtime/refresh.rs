use crate::runtime::live::RuntimeDispatchReport;

/// runtime callback の完了を host 側の redraw 要求に変換する。
///
/// 現在の host/application 層は runtime 完了後に再投影する責務を持つため、
/// 1 件以上の handler が実行された dispatch は redraw を要求する。
pub fn runtime_dispatch_requests_redraw(report: &RuntimeDispatchReport) -> bool {
    let requests_redraw = report.handler_count > 0;
    log::debug!(
        "[runtime_refresh] runtime dispatch completed: event={:?}, handler_count={}, requests_redraw={}",
        report.event,
        report.handler_count,
        requests_redraw
    );
    requests_redraw
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::live::RuntimeEventName;

    #[test]
    fn runtime_dispatch_with_handlers_requests_redraw() {
        let report = RuntimeDispatchReport {
            event: RuntimeEventName::BufferOpen,
            handler_count: 1,
        };

        assert!(runtime_dispatch_requests_redraw(&report));
    }

    #[test]
    fn runtime_dispatch_without_handlers_does_not_request_redraw() {
        let report = RuntimeDispatchReport {
            event: RuntimeEventName::BufferWritePost,
            handler_count: 0,
        };

        assert!(!runtime_dispatch_requests_redraw(&report));
    }
}
