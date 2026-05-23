#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

saya_version() {
  sed -n 's/^version = "\(.*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -n 1
}

saya_target_triple() {
  if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
    printf '%s\n' "$CARGO_BUILD_TARGET"
    return
  fi

  rustc -vV | sed -n 's/^host: //p'
}

saya_binary_path() {
  if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
    printf '%s\n' "$REPO_ROOT/target/$CARGO_BUILD_TARGET/release/sy"
    return
  fi

  printf '%s\n' "$REPO_ROOT/target/release/sy"
}

saya_dist_root() {
  printf '%s\n' "${DIST_ROOT:-$REPO_ROOT/target/dist}"
}

saya_release_name() {
  printf 'saya-v%s-%s\n' "$(saya_version)" "$(saya_target_triple)"
}

saya_stage_dir() {
  printf '%s/%s\n' "$(saya_dist_root)" "$(saya_release_name)"
}

saya_build_release_binary() {
  local cargo_args=(build --release --bin sy)

  if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
    cargo_args+=(--target "$CARGO_BUILD_TARGET")
  fi

  (cd "$REPO_ROOT" && cargo "${cargo_args[@]}")
}

saya_copy_home() {
  local saya_home="$1"

  rm -rf "$saya_home"
  mkdir -p "$saya_home/runtime/plugins" "$saya_home/docs"

  cp -R "$REPO_ROOT/plugins/bundled" "$saya_home/runtime/plugins/"
  cp -R "$REPO_ROOT/plugins/types" "$saya_home/runtime/plugins/"
  find "$saya_home/runtime/plugins/bundled" -type f -name '*.test.ts' -delete

  install -m 0644 "$REPO_ROOT/README.md" "$saya_home/docs/README.md"
  install -m 0644 "$REPO_ROOT/README_ja.md" "$saya_home/docs/README_ja.md"
  install -m 0644 "$REPO_ROOT/LICENSE" "$saya_home/docs/LICENSE"
  install -m 0644 \
    "$REPO_ROOT/THIRD_PARTY_NOTICES.md" \
    "$saya_home/docs/THIRD_PARTY_NOTICES.md"
}

saya_stage_release_tree() {
  local stage_dir
  local binary_path

  stage_dir="$(saya_stage_dir)"
  binary_path="$(saya_binary_path)"

  rm -rf "$stage_dir"
  mkdir -p "$stage_dir/bin" "$stage_dir/share"

  install -m 0755 "$binary_path" "$stage_dir/bin/sy"
  saya_copy_home "$stage_dir/share/saya"

  printf '%s\n' "$stage_dir"
}
