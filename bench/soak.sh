#!/usr/bin/env bash
# ibde-bench soak — long-run stability + memory-drift gate.
# Boots the composed session under headless sway and churns input for
# N minutes (default 10), sampling RSS/PSS every 30s. Gates:
#   - zero crashes (both processes alive at the end)
#   - RSS drift < 3% between first and last sample (per process)
# Usage: bench/soak.sh [--minutes N] [--json PATH]
set -euo pipefail
cd "$(dirname "$0")/.."

MINUTES=10
JSON_OUT="bench/results/soak.json"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --minutes) MINUTES="$2"; shift 2 ;;
    --json) JSON_OUT="$2"; shift 2 ;;
    *) echo "unknown arg $1"; exit 1 ;;
  esac
done
mkdir -p bench/results bench/shots
SECONDS_TOTAL=$((MINUTES * 60))

SAMPLES=$(docker run --rm \
  -e IBDE_OUTPUT=1280x800 \
  -v "$PWD:/src:ro" \
  ibde-sway-test bash -c '
set -u
MIN='"$MINUTES"' ; SECONDS_TOTAL='"$SECONDS_TOTAL"'
/src/target/release/ibde-shell --seconds $((SECONDS_TOTAL + 90)) >/tmp/shell.log 2>&1 &
SHELL_PID=$!
/src/target/release/ibde-terminal --exec cat --seconds $((SECONDS_TOTAL + 90)) --cols 60 --rows 12 >/tmp/term.log 2>&1 &
TERM_PID=$!
sleep 3

sample() {
  local pid=$1 tag=$2
  local rss pss
  rss=$(awk "/^VmRSS/{print \$2}" /proc/$pid/status 2>/dev/null || echo 0)
  pss=$(awk "/^Pss:/{print \$2}" /proc/$pid/smaps_rollup 2>/dev/null || echo 0)
  echo "$tag $rss $pss"
}

INTERVAL=30
N=$((SECONDS_TOTAL / INTERVAL))
for i in $(seq 0 $N); do
  sample $SHELL_PID shell
  sample $TERM_PID term
  # input churn: keys + clicks throughout the run
  if (( i % 2 == 0 )); then
    /src/target/release/kb-inject --after-ms 50 --delay-ms 10 \
      --text "soak" --special enter >/dev/null 2>&1 || true
  else
    /src/target/release/ptr-inject --x 640 --y 762 --after-ms 50 >/dev/null 2>&1 || true
  fi
  sleep $INTERVAL
done

ALIVE_SHELL=0; ALIVE_TERM=0
[ -d /proc/$SHELL_PID ] && ALIVE_SHELL=1
[ -d /proc/$TERM_PID ] && ALIVE_TERM=1
echo "alive $ALIVE_SHELL $ALIVE_TERM"
wait 2>/dev/null || true
')

# gate on post-warm-up drift: the first 60s fills glyph caches and
# bounded scrollback (by-design growth), so compare minute-1 vs last
WARM=$((60 / 30)) # samples to skip (INTERVAL=30)
FIRST_SHELL=$(echo "$SAMPLES" | awk '/^shell/{print $2}' | sed -n "$((WARM+1))p")
LAST_SHELL=$(echo "$SAMPLES" | awk '/^shell/{print $2}' | tail -1)
FIRST_TERM=$(echo "$SAMPLES" | awk '/^term/{print $2}' | sed -n "$((WARM+1))p")
LAST_TERM=$(echo "$SAMPLES" | awk '/^term/{print $2}' | tail -1)
ALIVE=$(echo "$SAMPLES" | awk '/^alive/{print $2, $3}')

drift() { # first last -> percent
  local f=$1 l=$2
  [[ $f -gt 0 ]] || echo 0 && python3 -c "print(round(($l-$f)/$f*100,2))"
}
DRIFT_SHELL=$(drift "$FIRST_SHELL" "$LAST_SHELL")
DRIFT_TERM=$(drift "$FIRST_TERM" "$LAST_TERM")

cat > "$JSON_OUT" <<EOF
{
  "profile": "soak-sway-pixman",
  "date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "minutes": $MINUTES,
  "shell": {"rss_kb_start": $FIRST_SHELL, "rss_kb_end": $LAST_SHELL, "drift_pct": $DRIFT_SHELL},
  "terminal": {"rss_kb_start": $FIRST_TERM, "rss_kb_end": $LAST_TERM, "drift_pct": $DRIFT_TERM},
  "alive": "$ALIVE"
}
EOF
cat "$JSON_OUT"

FAIL=0
[[ "$ALIVE" == "1 1" ]] || { echo "GATE FAIL: process died during soak"; FAIL=1; }
python3 -c "import sys; sys.exit(0 if abs($DRIFT_SHELL) < 3 else 1)" || { echo "GATE FAIL: shell drift ${DRIFT_SHELL}%"; FAIL=1; }
python3 -c "import sys; sys.exit(0 if abs($DRIFT_TERM) < 3 else 1)" || { echo "GATE FAIL: terminal drift ${DRIFT_TERM}%"; FAIL=1; }
if (( FAIL )); then exit 1; fi
echo "SOAK PASS ✓ (${MINUTES}min, shell ${FIRST_SHELL}→${LAST_SHELL}kB, term ${FIRST_TERM}→${LAST_TERM}kB)"
