#!/usr/bin/env bash
# Extract bench measurements from serial.log (between markers).
set -euo pipefail
cd "$(dirname "$0")"
LOG=serial.log
[[ -f "$LOG" ]] || { echo "no serial.log — VM never ran?" >&2; exit 1; }
if ! grep -q "=== ibde-bench baseline ===" "$LOG"; then
  echo "measurement markers not found in serial.log (boot or cloud-init failure)" >&2
  echo "--- last 40 serial lines ---" >&2
  tail -40 "$LOG" >&2
  exit 1
fi
awk '/=== ibde-bench baseline ===/{f=1} f; /=== end ===/{exit}' "$LOG"
