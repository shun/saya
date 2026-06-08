# ADR 0001: Make embedded mode host-owned and event-driven

This ADR records the repository-level decision for how `vim-core-rs` must
behave when embedded by a host application. It exists to remove ambiguity
about terminal ownership, message delivery, and command observability before
the redesign starts.

- Status: Accepted
- Date: March 26, 2026

## Context

`vim-core-rs` embeds upstream Vim to provide modal editing semantics and state
extraction to a host application. The intended architecture is host-owned:
the host owns rendering, persistence, process orchestration, and user-facing
presentation.

The current implementation still permits native Vim terminal behavior to
escape from the embedded library.

In particular:

- Earlier versions relied on native UI and terminal code paths for messages.
- The current implementation exposes message events through the transaction
  result and the event queue, rather than treating `:messages` or `v:errmsg`
  as the primary delivery path.
- The design now separates state extraction and event observation.
- Command execution exposes a transaction-shaped result.

This breaks the repository scope. The embedded core should provide editing
semantics and host-facing contracts, but it should not directly own terminal
presentation in embedded mode.

## Decision

`vim-core-rs` will redefine embedded execution as a host-owned, event-driven
runtime mode.

This means:

1. Embedded mode must not directly write user-visible output to terminal UI,
   `stdout`, or `stderr`.
2. All user-visible messages must flow through a host-facing event contract
   or be explicitly suppressible by embedded configuration.
3. Command execution must produce a transaction-shaped result that contains:
   - the final snapshot
   - emitted events
   - emitted host actions
4. Snapshot APIs must remain state-only.
5. File, job, quit, redraw, input, and message flows must use explicit host
   contracts.
6. Message scraping from `:messages` and `v:errmsg` must not remain the
   primary delivery path.

## Runtime model

The runtime model is split into two conceptual modes:

- `Embedded`
- `Standalone`

The primary supported architecture for `vim-core-rs` is `Embedded`.

In `Embedded` mode:

- the host owns user-visible rendering
- the host owns persistence
- the host owns process orchestration
- the host owns final message presentation
- the embedded core may update internal screen state for computation, but it
  must not directly present terminal output to the user

## Observable outputs

The embedded core may expose only two categories of observable output:

- state
- events and host actions

State includes:

- buffers
- cursor
- mode
- windows
- syntax
- undo-related state
- pop-up menu state

Events include:

- messages
- bell
- redraw requests
- layout changes
- pending input changes

Host actions include:

- write requests
- quit requests
- input requests
- job start and stop requests
- VFS requests

## Message model

Messages become first-class events.

The library reconstructs host-visible messages through explicit events in the
transaction result and event queue. `:messages` may remain as Vim-compatible
runtime history, but it is not the primary transport for host delivery.

## Command model

Command execution becomes transactional.

A command result must contain:

- the command outcome
- the final snapshot
- emitted events
- emitted host actions

This replaces the current model where hosts combine command execution with
separate post-hoc observation paths.

## Terminal ownership

Embedded mode forbids terminal ownership by the library.

If a native path attempts to perform direct terminal output in embedded mode,
the output must be intercepted, rerouted, suppressed, or treated as an
embedded-mode contract violation. The library must not silently preserve
terminal fallback behavior in embedded mode.

## Consequences

This decision has the following positive effects:

- It removes an entire class of message-leak and terminal-leak bugs.
- It makes host integration deterministic.
- It aligns implementation with repository scope and intended ownership
  boundaries.
- It produces a clearer public API with less hidden behavior.
- It separates state from event delivery cleanly.

This decision also has explicit costs:

- It is intentionally breaking.
- It touches vendored upstream integration points.
- Older scrape-based message handling paths will be removed.
- Some tests and host integrations will require migration.

## Rejected alternatives

The repository rejects the following alternatives:

- Keep the current model and suppress specific messages.
  This treats symptoms instead of fixing the architectural cause.
- Improve post-hoc message scraping by scraping more sources.
  This remains post-hoc observation and cannot guarantee that native side
  effects did not already occur.
- Special-case undo output only.
  Undo is only one example of the larger design problem.
- Preserve terminal fallback in embedded mode.
  Fallback terminal ownership conflicts with host-owned embedded
  architecture and makes observability non-deterministic.

## Implementation notes

The redesign is expected to proceed in phases:

1. Introduce new event-driven public contract types.
2. Add a native event queue and message interception path.
3. Forbid direct terminal writes in embedded mode.
4. Make command execution transactional.
5. Unify host actions with the transaction model.
6. Remove scrape-based compatibility delivery paths.

## Status note

This ADR is accepted. The repository has already begun implementing the
embedded event-driven contract, and current work is converging the remaining
compatibility paths onto that design.
