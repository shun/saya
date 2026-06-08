# vim-core-rs VFS and VFD guide

This reference explains the host-driven storage and process integration
contracts. Read this before touching `src/vfs.rs`, `src/vfd.rs`, host-action
handling, or VFS and job tests.

## Virtual file system (VFS)

`vim-core-rs` implements a host-driven Virtual File System. When commands like `:edit` or `:write` are executed on a virtual buffer, the core generates a `CoreHostAction::VfsRequest`. The host application must process this request asynchronously or synchronously and send the result back using `submit_vfs_response`.

### Key Methods
- `take_pending_host_action(&mut self) -> Option<CoreHostAction>`
- `submit_vfs_response(&mut self, response: CoreVfsResponse) -> Result<CoreCommandOutcome, CoreCommandError>`

### Inspection
- `buffer_binding(&self, buf_id: i32) -> Option<CoreBufferBinding>`: Checks VFS binding for a buffer.
- `vfs_request_ledger(&self) -> Vec<CoreRequestEntry>`
- `vfs_transaction_log(&self) -> Vec<VfsLogEntry>`

### Typical VFS flow

1. **Resolve**: Core emits `CoreVfsRequest::Resolve`. Host responds with `CoreVfsResponse::Resolved` (yielding a `document_id`) or `ResolvedLocalFallback`.
2. **Load**: Core emits `CoreVfsRequest::Load`. Host responds with `CoreVfsResponse::Loaded` containing the text.
3. **Save**: Core emits `CoreVfsRequest::Save`. Host responds with `CoreVfsResponse::Saved`.

Errors like `CoreVfsResponse::Failed` or `TimedOut` handle the failure paths gracefully.

### Behaviors that are easy to miss

- A VFS-backed quit may be deferred until save completion.
- `:wq` and related commands can yield save requests first and a `Quit` action
  only after the save is applied.
- A second quit while save is pending may be denied. The transaction log records
  these transitions.
- Stale save responses are part of the contract. Do not assume any `Saved`
  response is always accepted.
- Local fallback is distinct from resolved VFS ownership. Tests cover both.

When debugging VFS behavior, inspect both `vfs_request_ledger()` and
`vfs_transaction_log()` because buffer state alone is often insufficient.

## Useful VFS types

```rust
pub enum CoreVfsRequest {
    Resolve { .. },
    Load { .. },
    Save { .. },
}

pub enum CoreVfsResponse {
    Resolved { .. },
    ResolvedLocalFallback { .. },
    Loaded { .. },
    Saved { .. },
    Failed { .. },
    TimedOut { .. },
    Cancelled { .. },
}
```

`CoreBufferInfo` and `CoreBufferBinding` expose whether a buffer is local or
VFS-backed, its `document_id`, pending operation, deferred close, and last VFS
error.

## Virtual file descriptors (VFD) and job control

Vim supports background jobs. `vim-core-rs` abstracts this using VFDs so the
host can run processes and pipe their I/O back into Vim.

### Job Lifecycle
When Vim starts a job (e.g., `jobstart()`), it emits:
```rust
CoreHostAction::JobStart(CoreJobStartRequest {
    job_id: i32,
    argv: Vec<String>,
    cwd: Option<String>,
    vfd_in: i32,
    vfd_out: i32,
    vfd_err: i32,
})
```

The host must:
1. Spawn the process.
2. Read the process's stdout/stderr and inject it into the core using:
   `session.inject_vfd_data(vfd_out, data_bytes)`
3. Monitor the process exit.
4. Notify the core when the job finishes using:
   `session.notify_job_status(job_id, JobStatus::Finished, exit_code)`

The core does not spawn or monitor the process for you. `JobStart` is a request
to the host, not a confirmation that a child process already exists.

If Vim stops the job (e.g., `jobstop()`), it emits:
```rust
CoreHostAction::JobStop { job_id: i32 }
```
The host should then kill the process.

### Enums
```rust
pub enum JobStatus {
    Running = 0,
    Finished = 1,
    Failed = 2,
}
```

### Validation guidance

- Use `tests/vfs_contract.rs` for storage flows.
- Use `tests/job_api_contract.rs` for end-to-end job bridging.
- Use `tests/job_contract.rs` for simpler job host-action expectations.
