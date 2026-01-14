#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_BIN="$ROOT_DIR/target/release/te"
LOCAL_BIN="$ROOT_DIR/bin/te"
INSTALL_BIN="$HOME/.local/bin/te"

cd "$ROOT_DIR"

cargo build --release

install -d "$ROOT_DIR/bin"
install -m 755 "$TARGET_BIN" "$LOCAL_BIN"

install -d "$HOME/.local/bin"
install -m 755 "$TARGET_BIN" "$INSTALL_BIN"

printf "Built:     %s\n" "$LOCAL_BIN"
printf "Installed: %s\n" "$INSTALL_BIN"
command -v te >/dev/null 2>&1 && printf "On PATH:  te -> %s\n" "$(command -v te)" || true
