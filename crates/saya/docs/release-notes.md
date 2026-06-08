# Release notes

This page records user-facing compatibility notes for unreleased changes.

## Unreleased

### Dired v1 preview API

The local dired API now has a versioned v1 preview contract in
[`docs/api/dired-api-v1.md`](api/dired-api-v1.md). The contract pins the
`saya.filer.*` runtime surface, `setupSayaDired(options)`, destructive
operation confirmation, writable directory buffer preview semantics, and plugin
author guidance.

Dired remains a preview feature for this release because the backend adapter
boundary is still pending. Keeping the preview label avoids promising remote or
adapter-independent filesystem behavior before the backend adapter design is
implemented and tested.

Plugin authors must migrate toward the grouped `commands` and `keymap` setup
options, keep filesystem operations on `saya.filer`, and retain explicit
preview or confirmation for destructive operations.
