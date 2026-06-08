# vim-core-rs usage examples

These examples show how to use the crate as a host. Adapt them to the user task
instead of copying them blindly.

## Handle the host-action loop

```rust
use vim_core_rs::{VimCoreSession, CoreHostAction};

fn main() {
    let mut session = VimCoreSession::new("Initial text").unwrap();

    session.execute_ex_command("set number").unwrap();
    
    // Process pending actions from the core engine
    while let Some(action) = session.take_pending_host_action() {
        match action {
            CoreHostAction::VfsRequest(req) => {
                // Handle Virtual File System request (resolve, load, save)
            }
            CoreHostAction::Quit { force, .. } => {
                // Exit application
                break;
            }
            CoreHostAction::Redraw { .. } => {
                // Trigger UI refresh
            }
            _ => {}
        }
    }
}
```

In real hosts, replace the comments with actual VFS, redraw, quit, and job
handling. The loop is not optional for commands that queue host work.

## Inspect a coherent snapshot

```rust
use vim_core_rs::{VimCoreSession, CoreMode};

fn check_mode() {
    let session = VimCoreSession::new("Hello").unwrap();
    let snapshot = session.snapshot();

    assert_eq!(snapshot.mode, CoreMode::Normal);
    assert_eq!(snapshot.text, "Hello");
}
```

## Capture Vim messages

```rust
use vim_core_rs::{
    CoreEvent, CoreMessageCategory, CoreMessageEvent, CoreMessageSeverity,
    VimCoreSession,
};

fn capture_messages() {
    let mut session = VimCoreSession::new("hello").unwrap();

    let tx = session.execute_ex_command("echomsg 'hello from vim'").unwrap();

    assert!(tx.events.iter().any(|event| matches!(
        event,
        CoreEvent::Message(CoreMessageEvent {
            severity: CoreMessageSeverity::Info,
            category: CoreMessageCategory::UserVisible,
            ref content,
        }) if content.contains("hello from vim")
    )));
}
```

## Inspect a command transaction

```rust
use vim_core_rs::{
    CoreEvent, CoreMessageCategory, CoreMessageEvent, CoreMessageSeverity,
    CoreCommandOutcome, VimCoreSession,
};

fn inspect_transaction() {
    let mut session = VimCoreSession::new("hello").unwrap();

    let tx = session.execute_normal_command("u").unwrap();

    assert!(matches!(tx.outcome, CoreCommandOutcome::BufferChanged { .. }
        | CoreCommandOutcome::NoChange));
    assert!(tx.events.iter().all(|event| match event {
        CoreEvent::Message(CoreMessageEvent {
            severity,
            category,
            ..
        }) => {
            matches!(severity, CoreMessageSeverity::Info | CoreMessageSeverity::Warning | CoreMessageSeverity::Error)
                && matches!(category, CoreMessageCategory::UserVisible | CoreMessageCategory::CommandFeedback)
        }
        _ => true,
    }));
}
```

## Bridge a job through VFD

```rust
use vim_core_rs::{CoreHostAction, JobStatus, VimCoreSession};

fn bridge_job_output() {
    let mut session = VimCoreSession::new("").unwrap();
    session
        .execute_ex_command("let g:job = job_start(['echo', 'hello'])")
        .unwrap();

    while let Some(action) = session.take_pending_host_action() {
        if let CoreHostAction::JobStart(req) = action {
            session.inject_vfd_data(req.vfd_out, b"hello from host\n").unwrap();
            session
                .notify_job_status(req.job_id, JobStatus::Finished, 0)
                .unwrap();
        }
    }
}
```
