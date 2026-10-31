#!/usr/bin/env bash
# Build the Phase 0 toolkit spike. Host toolchain: gcc + pkg-config.
set -euo pipefail
cd "$(dirname "$0")"

wayland-scanner client-header ../protocols/wlr-layer-shell-unstable-v1.xml \
    build/layer-shell-protocol.h
wayland-scanner private-code ../protocols/wlr-layer-shell-unstable-v1.xml \
    build/layer-shell-protocol.c
# layer-shell's get_popup references xdg_popup; pull in xdg-shell glue too
wayland-scanner client-header /usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml \
    build/xdg-shell-protocol.h
wayland-scanner private-code /usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml \
    build/xdg-shell-protocol.c

cat > build/protocol.c <<'EOF'
/* wrapper: scanner output needs client headers first */
#include <wayland-client.h>
#include "xdg-shell-protocol.h"
#include "xdg-shell-protocol.c"
#include "layer-shell-protocol.h"
#include "layer-shell-protocol.c"
EOF

gcc -O2 -Wall -Wextra -o build/spike \
    -I build spike.c build/protocol.c \
    $(pkg-config --cflags --libs wayland-client cairo pangocairo) \
    -lm

echo "built: $(pwd)/build/spike"
