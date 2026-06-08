# Third-party notices

`vim-core-rs` vendors and modifies portions of Vim. `vim-core-rs` vendors and
modifies portions of Vim as part of a modified Vim distribution. The embedded
upstream baseline is recorded in `upstream-metadata.json`, and the current
repository tracks Vim `v9.2.0437`.

The repository's original code is licensed under Apache License 2.0. This file
only describes the third-party material that remains under other license
terms.

## Vim

This repository distributes source files copied from upstream Vim under
`vendor/vim_src/`, along with build glue and host-integration changes that
produce a modified embedded Vim runtime.

- Upstream project: <https://github.com/vim/vim>
- Bundled license text: `LICENSE-vim`
- Upstream license help reference: `:help uganda`
- Local source for distributed changes: this repository's tracked files,
  including `native/`, `src/`, `build.rs`, and the vendored sources under
  `vendor/vim_src/`
- Contact for this modified Vim distribution:
  <https://github.com/shun/vim-core-rs/issues>

The embedded runtime is built with a `MODIFIED_BY` string so that Vim's
`:version` output and intro screen disclose that this is a modified Vim
distribution and point users to the repository maintainers.

## Optional Tree-sitter packages

The experimental Tree-sitter syntax extraction feature can optionally depend
on upstream Tree-sitter runtime and grammar packages. These packages are
enabled only through Cargo features and are not part of the default build.

- `tree-sitter`: MIT license,
  <https://github.com/tree-sitter/tree-sitter>
- `tree-sitter-md`: MIT license,
  <https://github.com/tree-sitter-grammars/tree-sitter-markdown>
- `tree-sitter-rust`: MIT license,
  <https://github.com/tree-sitter/tree-sitter-rust>
