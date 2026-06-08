#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: $0 <out_dir> <crate_version> <target_triple> <output_dir>" >&2
  exit 1
fi

out_dir="$1"
crate_version="$2"
target_triple="$3"
output_dir="$4"
mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"

required_files=(
  "libvimcore.a"
  "bindings.rs"
  "native-source-audit-report.txt"
  "archive-member-audit-report.txt"
  "normal-delegation-proof.txt"
  "ex-delegation-proof.txt"
  "upstream_build_fingerprint.json"
  "upstream_vim_tests.rs"
  "vim_build/auto/config.h"
  "vim_build/auto/osdef.h"
  "vim_build/auto/pathdef.c"
)

for relative in "${required_files[@]}"; do
  if [[ ! -f "$out_dir/$relative" ]]; then
    echo "required artifact file missing: $out_dir/$relative" >&2
    exit 1
  fi
done

staging_dir="$(mktemp -d "${TMPDIR:-/tmp}/vim-core-rs-artifact.XXXXXX")"
trap 'rm -rf "$staging_dir"' EXIT

for relative in "${required_files[@]}"; do
  mkdir -p "$staging_dir/$(dirname "$relative")"
  cp "$out_dir/$relative" "$staging_dir/$relative"
done

upstream_tag="$(sed -n 's/.*"tag":[[:space:]]*"\([^"]*\)".*/\1/p' upstream-metadata.json | head -n1)"
upstream_commit="$(sed -n 's/.*"commit":[[:space:]]*"\([^"]*\)".*/\1/p' upstream-metadata.json | head -n1)"
generated_at_utc="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
manifest_path="$staging_dir/artifact-manifest.json"

{
  echo "{"
  echo "  \"crate_version\": \"${crate_version}\","
  echo "  \"target_triple\": \"${target_triple}\","
  echo "  \"artifact_profile\": \"release\","
  echo "  \"abi_version\": 1,"
  echo "  \"upstream_vim_tag\": \"${upstream_tag}\","
  echo "  \"upstream_vim_commit\": \"${upstream_commit}\","
  echo "  \"generated_at_utc\": \"${generated_at_utc}\","
  echo "  \"files\": {"
  for i in "${!required_files[@]}"; do
    relative="${required_files[$i]}"
    checksum="$(shasum -a 256 "$staging_dir/$relative" | awk '{print $1}')"
    comma=","
    if [[ "$i" -eq "$((${#required_files[@]} - 1))" ]]; then
      comma=""
    fi
    echo "    \"${relative}\": \"${checksum}\"${comma}"
  done
  echo "  }"
  echo "}"
} > "$manifest_path"

artifact_name="vim-core-rs-${crate_version}-${target_triple}.tar.gz"
checksum_name="${artifact_name}.sha256"

(
  cd "$staging_dir"
  COPYFILE_DISABLE=1 tar -czf "$output_dir/$artifact_name" .
)

shasum -a 256 "$output_dir/$artifact_name" | awk '{print $1}' > "$output_dir/$checksum_name"
