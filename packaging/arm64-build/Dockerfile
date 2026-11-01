# arm64 cross-builder for IBDE (Pi-class target proof).
#   docker build -t ibde-arm64 packaging/arm64-build
#   docker run --rm -v /apps/ibde:/src ibde-arm64
# Strategy: EXTRACT arm64 .debs into a sysroot (installing :arm64 dev
# packages conflicts with the amd64 ones — apt exit 100).
FROM debian:trixie
RUN apt-get update -qq \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
       build-essential crossbuild-essential-arm64 pkg-config curl ca-certificates \
       devscripts \
    && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /tmp/arm64debs && cd /tmp/arm64debs \
    && apt-get update -qq >/dev/null \
    && for p in libpango-1.0-0 libpangocairo-1.0-0 libpangoft2-1.0-0 \
                libcairo2 libpixman-1-0 libfontconfig1 libfreetype6 \
                libwayland-client0 libwayland-server0 libxkbcommon0 \
                libglib2.0-0 libthai0 libdatrie1 \
                libgraphite2-3 libharfbuzz0b libpng16-16 libxcb1 \
                libxau0 libxdmcp6 libbsd0 libmd0 libsharpyuv0 \
                libbrotli1 libbzip2-1.0 libexpat1 \
                libglib2.0-dev libcairo2-dev libpango1.0-dev \
                libwayland-dev libxkbcommon-dev; do \
         apt-get download "$p:arm64" 2>/dev/null || true; done \
    && mkdir -p /sysroot/usr \
    && for d in *.deb; do dpkg-deb -x "$d" /sysroot; done \
    && rm -rf /tmp/arm64debs \
    && mkdir -p /sysroot/usr/share/pkgconfig \
    && cp -a /usr/share/pkgconfig/. /sysroot/usr/share/pkgconfig/ 2>/dev/null || true
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal \
    --target aarch64-unknown-linux-gnu
ENV PATH=/root/.cargo/bin:$PATH
WORKDIR /src
ENV PKG_CONFIG_SYSROOT_DIR=/sysroot \
    PKG_CONFIG_LIBDIR=/sysroot/usr/lib/aarch64-linux-gnu/pkgconfig:/sysroot/usr/share/pkgconfig \
    TARGET_CC=aarch64-linux-gnu-gcc \
    RUSTFLAGS="-C linker=aarch64-linux-gnu-gcc"
ENTRYPOINT ["bash", "-c", "cargo build --release --target aarch64-unknown-linux-gnu && ls -la target/aarch64-unknown-linux-gnu/release/ | head -12"]
