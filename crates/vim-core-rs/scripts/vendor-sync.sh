#!/bin/bash
# vendor/vim_src を vendor/upstream/vim (metadata 固定 tag を clone) + patches/ から生成するスクリプト
#
# 使い方:
#   ./scripts/vendor-sync.sh apply            # upstream clone → コピー → パッチ適用
#   ./scripts/vendor-sync.sh verify           # パッチが当たるか dry-run 確認
#   ./scripts/vendor-sync.sh refresh          # 手動修正後にパッチを再生成
#   ./scripts/vendor-sync.sh update <tag>     # upstream を新タグで再取得して再生成
#   ./scripts/vendor-sync.sh status           # 現在の状態を表示
#
# 詳細: docs/VENDOR_MAINTENANCE.md

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
UPSTREAM="$REPO_ROOT/vendor/upstream/vim"
TARGET="$REPO_ROOT/vendor/vim_src"
PATCHES="$REPO_ROOT/patches"
ALLOWLIST="$REPO_ROOT/vim-source-allowlist.txt"
METADATA="$REPO_ROOT/upstream-metadata.json"
VIM_REPO_URL="${VIM_REPO_URL:-https://github.com/vim/vim.git}"

# --- ユーティリティ ---

die() {
  echo "エラー: $*" >&2
  exit 1
}

read_metadata_field() {
  local field="$1"
  [ -f "$METADATA" ] || return 0
  sed -n "s/.*\"$field\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$METADATA" | head -n1
}

# vendor/upstream/vim は submodule ではなく、upstream-metadata.json に固定された
# tag/commit から都度 clone する（gitignore 対象・ビルド成果物扱い）。
ensure_upstream() {
  if [ -d "$UPSTREAM/.git" ]; then
    return 0
  fi
  local tag commit
  tag="$(read_metadata_field tag)"
  commit="$(read_metadata_field commit)"
  [ -n "$tag$commit" ] || die "upstream-metadata.json に tag/commit がありません"
  echo "==> upstream vim を取得: ${tag:-$commit}"
  rm -rf "$UPSTREAM"
  mkdir -p "$(dirname "$UPSTREAM")"
  if [ -n "$tag" ]; then
    git clone --depth 1 --branch "$tag" "$VIM_REPO_URL" "$UPSTREAM"
  else
    git clone "$VIM_REPO_URL" "$UPSTREAM"
  fi
  if [ -n "$commit" ]; then
    local got
    got="$(cd "$UPSTREAM" && git rev-parse HEAD)"
    if [ "$got" != "$commit" ]; then
      echo "    metadata commit に固定: $commit"
      (cd "$UPSTREAM" && git fetch --depth 1 origin "$commit" && git checkout --quiet "$commit")
    fi
  fi
}

get_submodule_tag() {
  (cd "$UPSTREAM" && git describe --tags --exact-match 2>/dev/null || git rev-parse --short HEAD)
}

get_submodule_commit() {
  (cd "$UPSTREAM" && git rev-parse HEAD)
}

write_upstream_metadata() {
  local tag="$1"
  local commit="$2"

  cat > "$METADATA" <<EOF
{
  "tag": "$tag",
  "commit": "$commit"
}
EOF
}

# allowlist のグロブパターンにマッチするファイルだけをコピーする
# vendor/vim_src/ 配下の相対パスで判定する
copy_allowlisted_files() {
  local src="$1"
  local dst="$2"
  local allowlist="$3"
  local copied=0

  # allowlist からパターンを読み込み（コメント・空行除外）
  local patterns=()
  while IFS= read -r line; do
    line="${line## }"
    line="${line%% }"
    [[ -z "$line" || "$line" == \#* ]] && continue
    patterns+=("$line")
  done < "$allowlist"

  # src 以下の全ファイルを走査
  while IFS= read -r -d '' file; do
    local rel="${file#$src/}"
    local vendor_rel="vendor/vim_src/$rel"

    # allowlist パターンとマッチするか判定
    local matched=false
    for pattern in "${patterns[@]}"; do
      # シェルグロブでは ** が使えないので、パターンを分解して判定
      # シンプルなケース: vendor/vim_src/src/*.c → src/*.c 部分でマッチ
      local pat_rel="${pattern#vendor/vim_src/}"

      # ** を含むパターン → find 的なマッチ
      if [[ "$pattern" == *"**"* ]]; then
        # vendor/vim_src/src/proto/**/*.pro → src/proto/ 以下の .pro にマッチ
        local prefix="${pat_rel%%\*\**}"
        local suffix="${pat_rel##*\*\*/}"
        if [[ "$rel" == ${prefix}* ]] && [[ "$rel" == *${suffix} ]]; then
          matched=true
          break
        fi
      else
        # vendor/vim_src/src/*.c → src/ 直下の .c にマッチ
        # shellcheck disable=SC2254
        if [[ "$vendor_rel" == $pattern ]]; then
          matched=true
          break
        fi
      fi
    done

    if $matched; then
      local dst_file="$dst/$rel"
      mkdir -p "$(dirname "$dst_file")"
      cp "$file" "$dst_file"
      copied=$((copied + 1))
    fi
  done < <(find "$src" -type f -print0)

  echo "    $copied ファイルをコピー"
}

apply_patches() {
  local target="$1"
  local dry_run="${2:-false}"
  local patch_flag=""
  if $dry_run; then
    patch_flag="--dry-run"
  fi

  if [ ! -d "$PATCHES" ] || [ -z "$(ls "$PATCHES"/*.patch 2>/dev/null)" ]; then
    echo "    パッチファイルなし（スキップ）"
    return 0
  fi

  local failed=0
  for p in "$PATCHES"/*.patch; do
    [ -f "$p" ] || continue
    echo -n "    $(basename "$p") ... "
    if patch -d "$target" -p1 $patch_flag < "$p" > /dev/null 2>&1; then
      echo "OK"
    else
      echo "FAIL"
      failed=1
    fi
  done
  return $failed
}

cleanup_patch_backups() {
  local target="$1"

  find "$target" \( -name '*.orig' -o -name '*.rej' \) -type f -delete
}

# --- コマンド ---

cmd_apply() {
  ensure_upstream

  local tag
  tag=$(get_submodule_tag)
  echo "==> upstream: $tag"

  echo "==> vendor/vim_src を生成（allowlist ベースコピー）"
  # 既存の src/ を削除して pristine コピー
  rm -rf "$TARGET/src"
  mkdir -p "$TARGET"
  copy_allowlisted_files "$UPSTREAM" "$TARGET" "$ALLOWLIST"

  echo "==> パッチ適用"
  if apply_patches "$TARGET"; then
    cleanup_patch_backups "$TARGET"
    mkdir -p "$REPO_ROOT/target"
    touch "$REPO_ROOT/target/.vendor-patched"
    echo "==> 完了"
  else
    die "パッチ適用に失敗。patches/ を確認してください"
  fi
}

cmd_verify() {
  echo "==> パッチ適用チェック（dry-run）"
  if apply_patches "$TARGET" true; then
    echo "==> 全パッチ適用可能"
  else
    die "パッチ適用に失敗するものがあります"
  fi
}

cmd_refresh() {
  ensure_upstream

  echo "==> 現在の差分からパッチ内容を表示"
  echo "    （出力をリダイレクトして patches/ に保存してください）"
  echo ""

  # パッチ対象ファイル一覧（必要に応じて追加）
  local patch_targets=(
    src/ex_getln.c
    src/message.c
    src/option.c
    src/undo.c
    src/eval.c
    src/vim9execute.c
  )

  for f in "${patch_targets[@]}"; do
    if [ -f "$UPSTREAM/$f" ] && [ -f "$TARGET/$f" ]; then
      diff -u "$UPSTREAM/$f" "$TARGET/$f" || true
    fi
  done
}

cmd_update() {
  local new_tag="${1:?使い方: $0 update <tag>}"

  echo "==> upstream vim を $new_tag で取得（clone）"
  rm -rf "$UPSTREAM"
  mkdir -p "$(dirname "$UPSTREAM")"
  git clone --depth 1 --branch "$new_tag" "$VIM_REPO_URL" "$UPSTREAM"

  local resolved_tag
  local resolved_commit
  resolved_tag=$(get_submodule_tag)
  resolved_commit=$(get_submodule_commit)
  write_upstream_metadata "$resolved_tag" "$resolved_commit"
  echo "==> upstream metadata を更新: $resolved_tag ($resolved_commit)"

  echo "==> vendor/vim_src を再生成"
  rm -rf "$TARGET/src"
  mkdir -p "$TARGET"
  copy_allowlisted_files "$UPSTREAM" "$TARGET" "$ALLOWLIST"

  echo "==> パッチ適用を試行"
  if apply_patches "$TARGET"; then
    cleanup_patch_backups "$TARGET"
    mkdir -p "$REPO_ROOT/target"
    touch "$REPO_ROOT/target/.vendor-patched"
    echo ""
    echo "==> 完了"
    echo "    次のステップ:"
    echo "    1. cargo build && cargo test"
    echo "    2. git add upstream-metadata.json patches/"
    echo "    3. git commit -m 'upstream Vim を $new_tag に更新'"
  else
    echo ""
    echo "==> パッチ適用に失敗"
    echo "    次のステップ:"
    echo "    1. vendor/vim_src/src/*.c の手動修正内容を確認"
    echo "    2. ./scripts/vendor-sync.sh refresh > patches/0001-xxx.patch"
    echo "    3. cargo build && cargo test"
    exit 1
  fi
}

cmd_status() {
  echo "=== vendor-sync 状態 ==="
  echo ""
  echo "upstream (vendor/upstream/vim, metadata 固定):"
  echo "  タグ:    $(read_metadata_field tag)"
  echo "  コミット: $(read_metadata_field commit | head -c 12)"
  if [ -d "$UPSTREAM/.git" ]; then
    echo "  clone:   取得済み"
  else
    echo "  clone:   未取得（apply/refresh/update 時に自動 clone）"
  fi
  echo ""

  if [ -f "$REPO_ROOT/target/.vendor-patched" ]; then
    echo "vendor/vim_src: 生成済み（マーカーあり）"
  elif [ -d "$TARGET/src" ]; then
    echo "vendor/vim_src: 存在するがマーカーなし（./scripts/vendor-sync.sh apply 推奨）"
  else
    echo "vendor/vim_src: 未生成（./scripts/vendor-sync.sh apply を実行してください）"
  fi
  echo ""

  if [ -d "$PATCHES" ]; then
    echo "パッチ:"
    local found=false
    for p in "$PATCHES"/*.patch; do
      [ -f "$p" ] || continue
      echo "  $(basename "$p")"
      found=true
    done
    $found || echo "  （なし）"
  else
    echo "パッチ: patches/ ディレクトリなし"
  fi
}

# --- エントリポイント ---

case "${1:-}" in
  apply)   cmd_apply ;;
  verify)  cmd_verify ;;
  refresh) cmd_refresh ;;
  update)  cmd_update "${2:-}" ;;
  status)  cmd_status ;;
  *)
    echo "使い方: $0 {apply|verify|refresh|update <tag>|status}"
    echo ""
    echo "コマンド:"
    echo "  apply          upstream clone → allowlist コピー → パッチ適用"
    echo "  verify         パッチが当たるか dry-run 確認"
    echo "  refresh        現在の差分を表示（パッチ再生成用）"
    echo "  update <tag>   upstream を新タグで再取得して再生成"
    echo "  status         現在の状態を表示"
    echo ""
    echo "詳細: docs/VENDOR_MAINTENANCE.md"
    exit 1
    ;;
esac
