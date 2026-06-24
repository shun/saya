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
