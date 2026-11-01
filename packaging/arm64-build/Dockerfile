# arm64 cross-builder for IBDE (Pi-class target proof).
#   docker build -t ibde-arm64 packaging/arm64-build
#   docker run --rm -v /apps/ibde:/src ibde-arm64
FROM debian:trixie
RUN dpkg --add-architecture arm64 \
    && apt-get update -qq \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
       build-essential crossbuild-essential-arm64 pkg-config curl ca-certificates \
       libpango-1.0-dev:arm64 libpangocairo-1.0-0:arm64 libcairo2-dev:arm64 \
       libwayland-dev:arm64 libwayland-client0:arm64 libxkbcommon-dev:arm64 \
       libglib2.0-dev:arm64 \
    && rm -rf /var/lib/apt/lists/*
# arm64 libs land in the multiarch paths; build an isolated sysroot view
RUN mkdir -p /sysroot/usr \
    && cp -a /usr/lib/aarch64-linux-gnu /sysroot/usr/ \
    && cp -a /usr/include /sysroot/usr/ 2>/dev/null || true \
    && for d in /usr/share/pkgconfig /usr/lib/aarch64-linux-gnu/pkgconfig; do \
         mkdir -p /sysroot$d && cp -a $d/. /sysroot$d/ 2>/dev/null || true; done
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal \
    --target aarch64-unknown-linux-gnu
ENV PATH=/root/.cargo/bin:$PATH
WORKDIR /src
ENV PKG_CONFIG_SYSROOT_DIR=/sysroot \
    PKG_CONFIG_LIBDIR=/sysroot/usr/lib/aarch64-linux-gnu/pkgconfig:/sysroot/usr/share/pkgconfig \
    TARGET_CC=aarch64-linux-gnu-gcc \
    RUSTFLAGS="-C linker=aarch64-linux-gnu-gcc"
ENTRYPOINT ["bash", "-c", "cargo build --release --target aarch64-unknown-linux-gnu && ls -la target/aarch64-unknown-linux-gnu/release/ | head -12"]
