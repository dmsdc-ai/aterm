#!/bin/bash
# Real-time aterm log monitor with color highlighting
# Usage: ./bin/log-monitor.sh [path-to-log]

LOG="${1:-$HOME/.local/state/aterm/logs/latest.log}"

if [[ ! -f "$LOG" ]]; then
  echo "No log file found: $LOG"
  echo "Start the app first: ./bin/run-debug.sh"
  exit 1
fi

echo "[log-monitor] Watching: $LOG"
echo "[log-monitor] Ctrl+C to stop"
echo "---"

tail -f "$LOG" | sed -E \
  -e "s/(ERROR|PANIC|panic|FATAL)/$(printf '\033[1;31m')&$(printf '\033[0m')/g" \
  -e "s/(WARN|warning)/$(printf '\033[1;33m')&$(printf '\033[0m')/g" \
  -e "s/(INFO|info)/$(printf '\033[1;32m')&$(printf '\033[0m')/g" \
  -e "s/(DEBUG|debug)/$(printf '\033[1;36m')&$(printf '\033[0m')/g" \
  -e "s/(PTY|pty|notify|Notify)/$(printf '\033[1;35m')&$(printf '\033[0m')/g" \
  -e "s/(IME|ime|Commit|Preedit)/$(printf '\033[1;34m')&$(printf '\033[0m')/g"
