#!/usr/bin/env bash
# ibde-bench v1 — interactive workout (container profile).
#
# Boots the full IBDE session (menubar + dock + terminal) under headless
# sway/pixman, injects a typing burst via kb-inject, screenshots the
# desktop, samples RSS/PSS of both DE processes, and asserts the typed
# text lands in the terminal grid. Emits JSON; exits non-zero on failure.
#
# This is the FUNCTIONAL gate on the dev container. The strict ADR-0002
# memory budgets apply to the floor QEMU profile (bench/vm), where this
# same workout will run once the floor image ships our session.
#
# Usage: bench/run-workout.sh [--json PATH]   (default: bench/results/workout.json)

set -euo pipefail
cd "$(dirname "$0")/.."

JSON_OUT="bench/results/workout.json"
if [[ "${1:-}" == "--json" ]]; then JSON_OUT="$2"; fi
mkdir -p "$(dirname "$JSON_OUT")" bench/shots

TYPED_TEXT="IBDE-WORKOUT-OK-42"

run_inner() {
    docker run --rm \
        -e IBDE_OUTPUT=1280x800 \
        -v "$PWD:/src:ro" \
        ibde-sway-test bash -c '
set -u
TYPED="'"$TYPED_TEXT"'"

/src/target/release/ibde-shell --seconds 14 >/tmp/shell.log 2>&1 &
SHELL_PID=$!
/src/target/release/ibde-terminal --exec cat --seconds 13 --cols 60 --rows 12 --dump >/tmp/term.log 2>&1 &
TERM_PID=$!

sleep 2.5

# idle samples (settle)
sample() { # pid -> "rss_kb pss_kb"
    local pid=$1 rss pss
    rss=$(awk "/^VmRSS/{print \$2}" /proc/$pid/status 2>/dev/null || echo 0)
    pss=$(awk "/^Pss:/{print \$2}" /proc/$pid/smaps_rollup 2>/dev/null || echo 0)
    echo "$rss $pss"
}
read -r SHELL_RSS_IDLE SHELL_PSS_IDLE <<< "$(sample $SHELL_PID)"
read -r TERM_RSS_IDLE  TERM_PSS_IDLE  <<< "$(sample $TERM_PID)"

# interactive burst: 23 keys through the whole input stack
/src/target/release/kb-inject --text "$TYPED" --delay-ms 40 --after-ms 700 2>/dev/null

# peak samples right after the burst + a moment of echo/render churn
sleep 1
read -r SHELL_RSS_PEAK SHELL_PSS_PEAK <<< "$(sample $SHELL_PID)"
read -r TERM_RSS_PEAK  TERM_PSS_PEAK  <<< "$(sample $TERM_PID)"

sleep 1.5
grim /src/bench/shots/workout.png 2>/dev/null

wait $TERM_PID || true
wait $SHELL_PID || true

TYPED_OK=0
grep -aq "$TYPED" /tmp/term.log && TYPED_OK=1
SHELL_SUMMARY=$(grep -a "^SUMMARY" /tmp/shell.log | tail -1 || true)
TERM_SUMMARY=$(grep -a "^SUMMARY" /tmp/term.log | tail -1 || true)

echo "WORKOUT_JSON_BEGIN"
cat <<EOF
{
  "profile": "container-sway-pixman",
  "date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "typed_expected": "$TYPED",
  "typed_ok": $TYPED_OK,
  "shell": {
    "rss_kb_idle": $SHELL_RSS_IDLE, "pss_kb_idle": $SHELL_PSS_IDLE,
    "rss_kb_peak": $SHELL_RSS_PEAK, "pss_kb_peak": $SHELL_PSS_PEAK,
    "summary": "$SHELL_SUMMARY"
  },
  "terminal": {
    "rss_kb_idle": $TERM_RSS_IDLE, "pss_kb_idle": $TERM_PSS_IDLE,
    "rss_kb_peak": $TERM_RSS_PEAK, "pss_kb_peak": $TERM_PSS_PEAK,
    "summary": "$TERM_SUMMARY"
  }
}
EOF
echo "WORKOUT_JSON_END"

[[ $TYPED_OK -eq 1 ]]
'
}

if ! OUT=$(run_inner 2>&1); then
    echo "WORKOUT FAILED — last output:"
    echo "$OUT" | tail -25
    exit 1
fi

echo "$OUT" | sed -n '/WORKOUT_JSON_BEGIN/,/WORKOUT_JSON_END/p' | grep -v WORKOUT_JSON > "$JSON_OUT"
cat "$JSON_OUT"

# sanity thresholds for the container profile (dev machine, generous;
# strict floor budgets live in the QEMU profile). Gates use PSS — the
# honest marginal cost (RSS double-counts shared libs and the mapped
# xkb keymap).
TYPED_OK=$(python3 -c "import json;print(json.load(open('$JSON_OUT'))['typed_ok'])")
SHELL_PSS=$(python3 -c "import json;print(json.load(open('$JSON_OUT'))['shell']['pss_kb_peak'])")
TERM_PSS=$(python3 -c "import json;print(json.load(open('$JSON_OUT'))['terminal']['pss_kb_peak'])")
FAIL=0
[[ "$TYPED_OK" == "1" ]] || { echo "GATE FAIL: typed text not rendered"; FAIL=1; }
(( SHELL_PSS < 8000 )) || { echo "GATE FAIL: shell PSS ${SHELL_PSS}kB >= 8000kB"; FAIL=1; }
(( TERM_PSS < 15000 )) || { echo "GATE FAIL: terminal PSS ${TERM_PSS}kB >= 15000kB"; FAIL=1; }

if (( FAIL )); then exit 1; fi
echo "WORKOUT PASS ✓  shell_pss=${SHELL_PSS}kB terminal_pss=${TERM_PSS}kB typed=ok"
