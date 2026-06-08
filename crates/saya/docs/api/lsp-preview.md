# LSP preview

This page documents the preview Language Server Protocol integration for `saya`.
It covers the TypeScript startup setup, the runtime host boundary, the current
feature set, LSIF support, and headless verification commands.

> **Note:** This is a preview feature currently under active development.

The LSP preview is split across a TypeScript plugin and host-mediated runtime
capabilities. Startup code declares language server configuration. At runtime,
the bundled plugin owns server selection, JSON-RPC request construction,
response interpretation, and UI routing. The Rust host owns language server
startup, stdio transport, JSON-RPC response routing, lifecycle cleanup, and
permission checks through the managed `saya.lsp.connect()` session API. LSIF
lookup remains separate through `saya.lsif.request()`.

## Startup setup

Import `plugins/bundled/lsp-client/index.ts` from your `init.ts` file to register the
preview LSP command set and language server definitions. It doesn't register
keymaps or buffer lifecycle event handlers unless you opt in with `keymap` and
`enableBufferEvents`.

```ts
import { setupSayaLspClient } from "./plugins/bundled/lsp-client/index.ts";

setupSayaLspClient({
  enableBufferEvents: true,
  keymap: {
    hover: "K",
    definition: "gd",
    references: "gR",
    documentSymbol: "gO",
    nextDiagnostic: "]d",
    previousDiagnostic: "[d",
  },
  trace: "messages",
  positionEncoding: "utf-16",
  languageIdByExtension: {
    go: "go",
  },
  servers: [
    {
      name: "gopls",
      command: "gopls",
      args: ["serve"],
      languages: ["go"],
      filePatterns: ["**/*.go"],
      rootMarkers: ["go.mod", ".git"],
      initializationOptions: {},
    },
  ],
});
```

The setup function validates command names, bridge command names, language IDs,
server commands, root URIs, environment entries, position encodings, and trace
modes during startup evaluation. Invalid configuration fails startup evaluation
with a structured configuration error instead of reaching the runtime bridge.

### `gopls` example

Use `gopls` as the reference real-server configuration for local smoke tests.
The server is matched to Go buffers by extension and language ID, and the
workspace root is detected from `go.mod` before falling back to `.git`.

```ts
import { setupSayaLspClient } from "./plugins/bundled/lsp-client/index.ts";

setupSayaLspClient({
  enableBufferEvents: true,
  keymap: {
    hover: "K",
    definition: "gd",
    documentSymbol: "gO",
  },
  clientName: "saya-gopls",
  languageIdByExtension: {
    go: "go",
  },
  servers: {
    go: {
      name: "gopls",
      command: "gopls",
      args: ["serve"],
      languages: ["go"],
      filePatterns: ["**/*.go"],
      rootMarkers: ["go.mod", ".git"],
      initializationOptions: {
        semanticTokens: true,
      },
      trace: "messages",
      positionEncoding: "utf-16",
    },
  },
});
```

When the current buffer path ends in `.go`, the plugin resolves the language ID
to `go`, picks the `gopls` server definition, finds the nearest workspace root,
and sends live server requests through a host-managed LSP session.

## Runtime boundary

The runtime boundary is a managed LSP session API, not the old single-call
`saya.lsp.request` bridge and not a general `saya.process.spawn()` contract.
The TypeScript plugin selects a server and builds the LSP payloads. It then
calls `saya.lsp.connect({ server, initializeParams })` to open a host-owned
session. The returned client exposes `request(method, params)`,
`notify(method, params)`, `takeNotifications()`, and `close()`.

The Rust host validates the selected server definition, starts the language
server with piped stdio, owns JSON-RPC framing and response routing, queues
server notifications, and cleans up the process when the session closes or the
runtime stops. This keeps arbitrary process management out of the LSP preview
contract while preserving TypeScript-level policy and UI flexibility.

The live request construction includes these important inputs.

- `lspVersion`, currently sent as `"3.17"`.
- `method`, such as `initialize`, `textDocument/hover`, or `shutdown`.
- `clientName`, `languageId`, `rootUri`, and `positionEncoding`.
- `server`, containing the selected command, arguments, environment, working
  directory, root markers, and initialization options.
- `buffer` and `editor`, as read-only snapshots used to compute document URIs
  and LSP positions.

For live LSP requests, the TypeScript manager reuses one initialized managed
session for each server definition, queues work until initialization completes,
redacts document text from diagnostics, and shuts the server down with
`shutdown`, `exit`, and `close()`.

LSIF lookup remains host-side and uses the runtime `saya.lsif.request()`
surface.

## Commands and keymaps

`setupSayaLspClient()` registers these default commands.

- `lsp.initialize`
- `lsp.initialized`
- `lsp.hover`
- `lsp.definition`
- `lsp.references`
- `lsp.documentSymbol`
- `lsp.completion`
- `lsp.completionResolve`
- `lsp.signatureHelp`
- `lsp.formatting`
- `lsp.rangeFormatting`
- `lsp.rename`
- `lsp.codeAction`
- `lsp.codeActionResolve`
- `lsp.nextDiagnostic`
- `lsp.previousDiagnostic`
- `lsp.shutdown`
- `lsif.hover`
- `lsif.definition`

It registers keymaps only when the `keymap` option is present. When `keymap` is
present, omitted entries use these standard bindings.

- `K` for hover
- `gd` for definition
- `gR` for references
- `gO` for document symbols
- `<C-k>` in insert mode for signature help
- `gq` in normal mode for formatting
- `gq` in visual mode for range formatting
- `grn` for rename
- `gra` for code action
- `]d` for next diagnostic
- `[d` for previous diagnostic
- `gK` for LSIF hover when LSIF is enabled
- `gD` for LSIF definition when LSIF is enabled

Buffer lifecycle events are also explicit. Set `enableBufferEvents: true` to
subscribe to `bufferOpen`, `bufferChanged`, `bufferWritePost`, and
`bufferClosed`. When it is omitted or `false`, the plugin still registers LSP
commands, but it doesn't start document synchronization from editor events.

Command names remain defaulted because they are internal command identifiers.
They don't activate LSP behavior until a user maps a key, enables buffer events,
or executes a command. `languageId`, `trace`, `positionEncoding`,
`completionTriggerCharacters`, and formatting defaults are protocol request
defaults used after explicit command or event activation.

## Feature status

The preview currently implements the core request path and a small user-facing
surface. Treat this list as the supported feature boundary until the public API
is stabilized.

### Supported

These features are implemented and covered by host-side or bridge contract
tests.

- Host-managed LSP 3.17 `Content-Length` JSON-RPC framing.
- `initialize`, `initialized`, `shutdown`, and `exit` lifecycle handling.
- `textDocument/didOpen`, `textDocument/didChange`, `textDocument/didSave`, and
  `textDocument/didClose` using full-document synchronization.
- `textDocument/hover`, `textDocument/definition`, `textDocument/references`,
  and `textDocument/documentSymbol` requests.
- `textDocument/completion`, `completionItem/resolve`,
  `textDocument/signatureHelp`, `textDocument/formatting`,
  `textDocument/rangeFormatting`, `textDocument/rename`,
  `textDocument/codeAction`, and `codeAction/resolve` request paths.
- `textDocument/publishDiagnostics` ingestion and next/previous diagnostic
  navigation.
- Hover and diagnostic floating surfaces, definition navigation, reference list
  output, and document symbol outline output.
- Completion integration through the bundled completion plugin when the LSP
  completion source is configured.
- Workspace root detection through runtime `saya.workspace.findRoot()`.
- Multiple language server definitions selected by language ID or file pattern.
- UTF-16, UTF-8, and UTF-32 position encoding conversion.
- Structured diagnostic events for managed session lifecycle, JSON-RPC requests,
  notifications, responses, timeout, shutdown, and session dispatch.

### Partially supported

These features exist, but they don't yet have the complete UX or protocol
coverage expected from a stable LSP client.

- Capability negotiation is recorded after `initialize`, but the preview does
  not yet gate every command on every advertised server capability.
- Diagnostics are displayed and navigable, but advanced severity filtering, code
  actions, and related information rendering are not implemented.
- References and document symbols use host-provided output surfaces, not a
  stable quickfix API.
- Signature help, formatting, rename, and code actions have protocol request
  paths, but their user experience and capability gating are still preview
  quality.
- Cancellation exists for stale requests, but there is not yet a user-facing
  cancellation command.
- Real-server smoke testing is manual. CI-safe behavior relies on fake-server
  tests.

### Not supported

These LSP areas are outside the current preview scope.

- Semantic tokens, inlay hints, call hierarchy, type hierarchy, workspace
  symbols, and workspace edits.
- Incremental text synchronization.
- Dynamic registration.
- Remote language servers and TCP transports.
- A stable plugin marketplace contract for LSP extensions.

## Release readiness

The first usable preview has a narrow quality bar. CI validates the
deterministic protocol path without installing language servers, and real-server
smoke tests stay opt-in until the public TypeScript API becomes stable.

### Minimum supported preview feature set

The first usable preview supports the LSP features listed in the supported
section only. In practice, that means the preview is usable when these flows
work from TypeScript startup configuration through the runtime process and LSIF
surfaces:

- `initialize`, `initialized`, `shutdown`, and `exit`.
- Full-document `didOpen`, `didChange`, `didSave`, and `didClose` sync.
- Hover, definition, references, document symbols, completion, signature help,
  formatting, range formatting, rename, and code action request paths.
- Publish diagnostics, diagnostic navigation, hover floats, definition
  navigation, reference lists, and document symbol output.
- File URI conversion, workspace root detection, server selection, position
  encoding conversion, and structured diagnostics.

Anything listed as partially supported or not supported is not part of the first
usable preview contract.

### CI reference-server policy

`gopls` is the reference real server for local and nightly smoke validation, but
it is not required for regular CI. Regular CI must run fake-server tests that
cover protocol behavior and the TypeScript-to-host bridge without depending on
Go tooling, network access, or machine-specific language server installs.

Machines without `gopls` installed run the same CI-safe fake-server checks as
other machines. They don't run the ignored real-server smoke test unless an
operator explicitly opts in by setting `SAYA_RUN_GOPLS_SMOKE=1` and selecting
ignored tests.

### Public TypeScript API compatibility

The preview documents `setupSayaLspClient()` and the managed LSP session API
as provisional API. Before documenting the API as stable, review these
compatibility points and update this page if any point changes:

- Keep the LSP setup options, command names, keymap defaults, server selection
  rules, lifecycle event names, and LSIF request fields backward-compatible
  within the preview contract.
- Keep `protocolVersion` as a deserialization alias for `lspVersion` on the host
  side.
- Keep command names, opt-in keymap defaults, and opt-in buffer lifecycle event
  registration documented as preview behavior, not stable extension points.
- Treat server selection, capability negotiation, and LSIF lookup options as
  preview configuration until command gating, diagnostics UX, and output
  surfaces are stabilized.

## LSIF support

Enable LSIF lookup separately from live LSP by passing `lsif.enabled` and a dump
path. LSIF requests use `saya.lsif.request()` with `source: "lsif"`.

```ts
setupSayaLspClient({
  keymap: {
    lsifHover: "gK",
    lsifDefinition: "gD",
  },
  lsif: {
    enabled: true,
    dumpPath: ".cache/index.lsif",
  },
});
```

Current LSIF support is intentionally narrow.

- The indexer reads newline-delimited LSIF 0.6.0 entries from a local dump file.
- It builds only the document, range, hover, definition, and edge indexes needed
  by hover and definition lookup.
- It resolves positions against the current file URI and the current LSP
  position.
- It caches one loaded dump at a time and invalidates the cache when the dump
  path changes.
- It does not implement references, document symbols, diagnostics, monikers,
  packages, project graph traversal, compressed dumps, remote indexes, or
  automatic index generation.

## Headless verification

Use headless tests for repeatable verification. Run commands through `gtimeout`
so a broken language server or fake server cannot hang the shell.

```bash
gtimeout 180s cargo test --test saya_lsp_module -- --nocapture
gtimeout 180s cargo test --test saya_lsp_e2e -- --nocapture
gtimeout 180s cargo test --test lsp_float -- --nocapture
gtimeout 120s cargo test --test lsif_index -- --nocapture
gtimeout 240s cargo test --no-run
```

The `saya_lsp_e2e` test uses a local Perl fake server. It validates initialize,
hover, shutdown, and managed-session routing without requiring `gopls` or
network access. Use ad hoc local `gopls` smoke testing only as extra manual
confidence; fake-server tests remain the deterministic protocol coverage.

## Next steps

Use this preview API only for local experimentation until the public LSP
contract is stabilized. Before expanding it, complete the partially supported
items that you want to promote into the stable API.
