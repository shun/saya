# Third-party notices

This file documents the third-party licensing situation for code that `saya`
depends on or redistributes through its build and binary outputs.

`saya` source code is licensed under Apache License 2.0. Third-party code keeps
its own license terms.

## `vim-core-rs`

`saya` depends on a local sibling checkout of `vim-core-rs` through a path
dependency declared in [Cargo.toml](Cargo.toml).

At the time this notice was written, `vim-core-rs` states the following
repository-level license split.

- Original `vim-core-rs` code is licensed under Apache License 2.0.
- Vendored and modified Vim sources remain subject to the Vim License.

See these files in the sibling repository for the authoritative text.

- [../vim-core-rs/LICENSE](../vim-core-rs/LICENSE)
- [../vim-core-rs/LICENSE-vim](../vim-core-rs/LICENSE-vim)
- [../vim-core-rs/README.md](../vim-core-rs/README.md)

## Upstream Vim code through `vim-core-rs`

`vim-core-rs` vendors and modifies upstream Vim sources. Because `saya` links
to `vim-core-rs`, redistributions of `saya` binaries may also need to carry the
relevant Vim notices and license text.

When you redistribute `saya` binaries or source packages that include
`vim-core-rs` outputs, include at least these files from the sibling
`vim-core-rs` repository.

- `LICENSE`
- `LICENSE-vim`
- Any additional third-party notice file shipped by `vim-core-rs`

## Distribution guidance

Use these rules when you package or redistribute `saya`.

- Treat `saya` source code as Apache License 2.0.
- Do not relabel `vim-core-rs` or vendored Vim code under the `saya` license.
- Include the `vim-core-rs` license materials when distributing binaries that
  incorporate `vim-core-rs`.
- Review `vim-core-rs` notices again before release, because upstream notice
  requirements can change over time.

## No legal advice

This file is a repository notice, not legal advice. If you plan to distribute
commercial builds or public binary releases, review the full dependency
licensing set before release.
