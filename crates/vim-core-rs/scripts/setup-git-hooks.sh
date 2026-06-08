#!/bin/sh

set -eu

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

git config core.hooksPath .githooks
chmod +x .githooks/pre-commit

echo "Configured Git hooks for $repo_root"
echo "pre-commit will now run rustfmt on staged Rust files before each commit."
