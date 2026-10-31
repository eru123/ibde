#!/bin/bash
# Run a layer-shell client under headless sway (pixman). Usage:
#   docker run --rm -v /apps/ibde:/src:ro ibde-sway-test <cmd...>
# The client runs with WAYLAND_DISPLAY pointing at the sway socket.
set -e
printf "output HEADLESS-1 mode 1280x32\n" > /tmp/sway.conf
sway -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
SWAY_PID=$!
for i in $(seq 1 30); do
    ls $XDG_RUNTIME_DIR/wayland-* >/dev/null 2>&1 && break
    sleep 0.5
done
SOCK=$(ls $XDG_RUNTIME_DIR/wayland-* 2>/dev/null | head -1)
if [ -z "$SOCK" ]; then
    echo "SWAY FAILED:"; tail -20 /tmp/sway.log; kill $SWAY_PID 2>/dev/null; exit 1
fi
echo "sway headless up on ${SOCK##*/}"
set +e
WAYLAND_DISPLAY=${SOCK##*/} "$@"
RC=$?
# report compositor RSS too (interesting for the budget)
kill $SWAY_PID 2>/dev/null
wait $SWAY_PID 2>/dev/null
exit $RC
