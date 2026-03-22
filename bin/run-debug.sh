#!/bin/bash
# Launch aterm-v3 with real-time log capture
# Usage: ./bin/run-debug.sh [release|debug]

set -euo pipefail

MODE="${1:-debug}"
LOG_DIR="$HOME/.local/state/aterm/logs"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/aterm-$(date +%Y%m%d-%H%M%S).log"
LATEST="$LOG_DIR/latest.log"

if [[ "$MODE" == "release" ]]; then
  BIN="target/release/aterm-v3"
  [[ -f "$BIN" ]] || cargo build --release --bin aterm-v3
else
  BIN="target/debug/aterm-v3"
  [[ -f "$BIN" ]] || cargo build --bin aterm-v3
fi

# Symlink latest log
ln -sf "$LOG_FILE" "$LATEST"

echo "[aterm-debug] Logging to: $LOG_FILE"
echo "[aterm-debug] Monitor: tail -f $LATEST"
echo "[aterm-debug] Or run: ./bin/log-monitor.sh"
echo "---"

# Launch with RUST_LOG and stderr to log file
RUST_LOG="${RUST_LOG:-aterm_v3=debug}" RUST_BACKTRACE=1 \
  "$BIN" 2>"$LOG_FILE"
