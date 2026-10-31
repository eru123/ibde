# labwc build environment (W1 compositor stream, ADR-0003).
# Proves we can build labwc from Debian source — the base for the
# Phase 2 keymap-engine patch series (ADR-0008).
#
#   docker build -t ibde-labwc-build bench/docker/labwc-build
FROM debian:trixie
RUN printf 'deb-src http://deb.debian.org/debian trixie main\n' \
      > /etc/apt/sources.list.d/deb-src.list \
    && apt-get update -qq \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
       dpkg-dev build-essential meson ninja-build git pkg-config \
       ca-certificates \
    && DEBIAN_FRONTEND=noninteractive apt-get build-dep -y -qq labwc \
    && apt-get source -qq labwc \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
# build once at image build time to prove the toolchain
RUN cd /labwc-* && meson setup build \
    -Dxwayland=disabled -Dlto=disabled \
    && ninja -C build \
    && ./build/labwc --version
