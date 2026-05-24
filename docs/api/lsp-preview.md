# LSP preview

This page documents the preview Language Server Protocol integration for `saya`.
It covers the TypeScript startup setup, the runtime host boundary, the current
feature set, LSIF support, and headless verification commands.

> **Note:** This is a preview feature currently under active development.

The LSP preview is split across a TypeScript plugin and host-owned Rust
services. Startup code declares language server configuration, while the host
owns process execution, JSON-RPC framing, document synchronization, diagnostics,
and LSIF lookup.

## Startup setup

Import `plugins/saya-lsp-client.ts` from your `init.ts` file to register the
preview LSP command set and language server definitions. It doesn't register
keymaps or buffer lifecycle event handlers unless you opt in with `keymap` and
`enableBufferEvents`.

```ts
import { setupSayaLspClient } from "./plugins/saya-lsp-client.ts";

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
import { setupSayaLspClient } from "./plugins/saya-lsp-client.ts";

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
and sends requests through `saya.lsp.request`.

## Runtime boundary

The TypeScript plugin doesn't spawn language server processes. It builds typed
bridge requests and sends them through the runtime `saya.lsp.request` surface.
The host-side LSP session manager owns the process lifecycle and keeps long
running server work outside the startup layer.

The runtime bridge request includes these important fields.

- `source`, as `"lsp"` for live language servers or `"lsif"` for LSIF lookup.
- `lspVersion`, currently sent as `"3.17"`.
- `method`, such as `initialize`, `textDocument/hover`, or `shutdown`.
- `clientName`, `languageId`, `rootUri`, and `positionEncoding`.
- `server`, containing the selected command, arguments, environment, working
  directory, root markers, and initialization options.
- `buffer` and `editor`, as read-only snapshots used to compute document URIs
  and LSP positions.

The host validates the bridge payload before dispatch. For live LSP requests,
the session manager reuses one initialized server session for each workspace and
server definition, queues work until initialization completes, routes responses
by JSON-RPC request ID, redacts document text from diagnostics, and shuts the
process down with `shutdown` followed by `exit`.

## Commands and keymaps

`setupSayaLspClient()` registers these default commands.

- `lsp.initialize`
- `lsp.initialized`
- `lsp.hover`
- `lsp.definition`
- `lsp.references`
- `lsp.documentSymbol`
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

- LSP 3.17 `Content-Length` JSON-RPC framing.
- `initialize`, `initialized`, `shutdown`, and `exit` lifecycle handling.
- `textDocument/didOpen`, `textDocument/didChange`, `textDocument/didSave`, and
  `textDocument/didClose` using full-document synchronization.
- `textDocument/hover`, `textDocument/definition`, `textDocument/references`,
  and `textDocument/documentSymbol` requests.
- `textDocument/publishDiagnostics` ingestion and next/previous diagnostic
  navigation.
- Hover and diagnostic floating surfaces, definition navigation, reference list
  output, and document symbol outline output.
- Workspace root detection through runtime `saya.workspace.findRoot()`.
- Multiple language server definitions selected by language ID or file pattern.
- UTF-16, UTF-8, and UTF-32 position encoding conversion.
- Structured diagnostic events for process lifecycle, JSON-RPC requests,
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
- Cancellation exists for stale requests, but there is not yet a user-facing
  cancellation command.
- Real-server smoke coverage exists for `gopls`, but CI-safe behavior relies on
  fake-server tests.

### Not supported

These LSP areas are outside the current preview scope.

- Completion, signature help, rename, formatting, code actions, semantic tokens,
  inlay hints, call hierarchy, type hierarchy, workspace symbols, and workspace
  edits.
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
work from TypeScript startup configuration through the host bridge:

- `initialize`, `initialized`, `shutdown`, and `exit`.
- Full-document `didOpen`, `didChange`, `didSave`, and `didClose` sync.
- Hover, definition, references, and document symbols.
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

The preview documents `setupSayaLspClient()` and `saya.lsp.request()` as a
provisional API. Before documenting the API as stable, review these
compatibility points and update this page if any point changes:

- Keep the `source`, `lspVersion`, `method`, `clientName`, `rootUri`,
  `languageId`, `trace`, `positionEncoding`, `dumpPath`, `textDocument`,
  `server`, `position`, `params`, `buffer`, `editor`, and `event` fields
  backward-compatible.
- Keep `protocolVersion` as a deserialization alias for `lspVersion` on the host
  side.
- Keep command names, opt-in keymap defaults, and opt-in buffer lifecycle event
  registration documented as preview behavior, not stable extension points.
- Treat server selection, capability negotiation, and LSIF lookup options as
  preview configuration until command gating, diagnostics UX, and output
  surfaces are stabilized.

## LSIF support

Enable LSIF lookup separately from live LSP by passing `lsif.enabled` and a dump
path. LSIF requests use the same runtime bridge shape with `source: "lsif"`.

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
gtimeout 180s cargo test --test lsp_transport -- --nocapture
gtimeout 180s cargo test --test lsp_session -- --nocapture
gtimeout 180s cargo test --test lsp_runtime_bridge_contract -- --nocapture
gtimeout 240s cargo test --test startup_runtime_scaffold \
  repository_lsp_client_plugin_bridge_fake_server_covers_protocol_flow_for_ci \
  -- --nocapture
gtimeout 120s cargo test --test lsif_index -- --nocapture
gtimeout 240s cargo test --no-run
```

For the opt-in real-server smoke path, install `go` and `gopls`, then run the
ignored startup scaffold test that reaches `gopls` through the preview bridge.

```bash
SAYA_RUN_GOPLS_SMOKE=1 gtimeout 240s cargo test --test startup_runtime_scaffold \
  repository_lsp_client_plugin_bridge_smoke_reaches_gopls_with_logs \
  -- --ignored --exact --nocapture
```

The `gopls` smoke test validates real initialize, initialized, didOpen, hover,
documentSymbol, didChange, didSave, didClose, and shutdown traffic. It is useful
for local validation, but fake-server tests remain the deterministic protocol
coverage.

## Next steps

Use this preview API only for local experimentation until the public LSP
contract is stabilized. Before expanding it, complete the partially supported
items that you want to promote into the stable API.
