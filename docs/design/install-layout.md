# Install layout design

This page defines how `saya` release archives, local install scripts, and
package-manager installs place the `sy` binary and the files that must ship
with it.

> **Note:** This is a preview feature currently under active development.

## Goals

The install layout keeps the editor executable, bundled runtime assets, and
human-facing documentation in one predictable application home. The layout must
work for GitHub release archives, package managers, and local development
scripts without embedding bundled TypeScript plugins into the Rust binary.

The design follows the same broad model as editors that install a binary plus a
runtime directory. The binary is relocatable when it is installed next to its
`share/saya` tree, and package managers can still place the same tree under
their own prefix.

## Install prefix

Local install scripts default to the user prefix at `~/.local`. The installer
must let users override the prefix with `PREFIX`.

```text
PREFIX="$HOME/.local"
```

An installed tree uses this shape.

```text
<prefix>/
├── bin/
│   └── sy
└── share/
    └── saya/
        ├── runtime/
        │   └── plugins/
        │       ├── bundled/
        │       └── types/
        └── docs/
            ├── README.md
            ├── README_ja.md
            ├── LICENSE
            └── THIRD_PARTY_NOTICES.md
```

The default local install therefore places `sy` at `$HOME/.local/bin/sy` and
places `SAYA_HOME` at `$HOME/.local/share/saya`.

## `SAYA_HOME`

`SAYA_HOME` is the application home that contains the runtime and bundled
documentation for the installed editor. For a normal install, `SAYA_HOME` is:

```text
<prefix>/share/saya
```

The default local install resolves it to:

```text
$HOME/.local/share/saya
```

The `runtime` directory contains files that `sy` reads at runtime. The `docs`
directory contains human-facing documentation and redistribution notices that
ship with the release archive.

```text
SAYA_HOME/
├── runtime/
│   └── plugins/
│       ├── bundled/
│       └── types/
└── docs/
    ├── README.md
    ├── README_ja.md
    ├── LICENSE
    └── THIRD_PARTY_NOTICES.md
```

## Runtime lookup

The host must resolve `SAYA_HOME` before it reads bundled plugin manifests or
TypeScript plugin sources. The lookup order is:

1. Use the `SAYA_HOME` environment variable when it is set.
2. Use the executable-relative path `../share/saya` from the resolved `sy`
   binary path.
3. Use a build-time `SAYA_DEFAULT_HOME` value when the build sets one.
4. Use the user-local fallback at `$HOME/.local/share/saya`.
5. Use the system fallbacks at `/usr/local/share/saya` and `/usr/share/saya`.
6. Use the repository development fallback during local builds only.

The executable-relative lookup keeps GitHub release archives relocatable after
extraction. The build-time value lets package managers compile a fixed default
when they need one. The environment variable gives users and tests a direct
override.

## Release archive layout

GitHub release archives are the primary binary distribution shape. A release
archive contains the executable and its `SAYA_HOME` tree.

```text
saya-v0.1.0-aarch64-apple-darwin/
├── bin/
│   └── sy
└── share/
    └── saya/
        ├── runtime/
        │   └── plugins/
        │       ├── bundled/
        │       │   ├── completion/
        │       │   ├── dired/
        │       │   └── lsp-client/
        │       └── types/
        └── docs/
            ├── README.md
            ├── README_ja.md
            ├── LICENSE
            └── THIRD_PARTY_NOTICES.md
```

Package managers can install the same shape under their own prefix. For
example, a package manager can place `sy` in `<prefix>/bin` and the application
home in `<prefix>/share/saya`.

## Distribution scripts

The repository provides scripts for release staging, archive packaging, local
copy installs, and local symlink installs.

`scripts/build-dist` builds a release binary and stages the release tree under
`target/dist/saya-v<version>-<target>/`.

```text
scripts/build-dist
```

`scripts/package-release` builds the same release tree and writes a `.tar.gz`
archive under `target/dist/`.

```text
scripts/package-release
CARGO_BUILD_TARGET=aarch64-apple-darwin scripts/package-release
```

`scripts/install-local` builds a release binary and copies the install tree into
`$PREFIX`. When `PREFIX` is unset, it uses `$HOME/.local`.

```text
scripts/install-local
PREFIX=/usr/local scripts/install-local
```

`scripts/link-local` builds a release binary, creates a staged `SAYA_HOME` tree
inside `target/`, and symlinks the installed paths into `$PREFIX`.

```text
scripts/link-local
PREFIX="$HOME/.local" scripts/link-local
```

The link workflow is for local development. It keeps the installed `sy` and
runtime tree connected to the checkout without changing the release archive
shape.

## Cargo install policy

`cargo install` installs executable binaries, not an application runtime tree.
For that reason, `cargo install saya` is not the primary distribution path for
complete user installs.

The supported complete install paths are GitHub release archives and package
managers that install both `bin/sy` and `share/saya`. A `cargo install` build
can still be useful for Rust developers, but it needs an existing `SAYA_HOME`
or a separate setup step that installs the runtime files.

## Bundled plugin policy

Bundled plugins are versioned with `saya`, but they are distributed as runtime
files under `SAYA_HOME/runtime/plugins/bundled`, not as external plugin manager
installs.

The host must not fetch bundled plugins from the network during startup. If
bundled runtime files are missing, `sy` must report a clear installation or
runtime-home error instead of silently falling back to external plugin
resolution.

External plugins remain under the plugin manager cache and lockfile model. They
are not part of the release archive's bundled runtime tree.

## Next steps

If you need to understand how bundled and external plugins differ after the
runtime files are installed, read [Plugin model](plugin-model.md). If you need
the startup cache behavior, read [Plugin manager API](../api/plugin-manager.md).
