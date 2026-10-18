#!/usr/bin/env bash
# Quick compile check of the Linux code in the build machine.
set -euo pipefail
src="$(cd "$(dirname "$0")/../.." && pwd)"
work="$HOME/ropeway"
export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p "$work"
rsync -a --delete --exclude node_modules --exclude target --exclude dist --exclude releases \
  --exclude 'src-tauri/gen/android' --exclude 'src-tauri/target' --exclude user_revisions "$src/" "$work/"
cd "$work"
mkdir -p dist
cargo check -p ropeway --message-format short 2>&1 | grep -E "^(error|warning)|error:|warning:" | head -60 || true
cargo check -p ropeway 2>&1 | tail -2
