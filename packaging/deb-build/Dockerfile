# cargo-deb builder for IBDE (Phase 2 packaging).
#   docker build -t ibde-deb-build packaging/deb-build
#   docker run --rm -v /apps/ibde:/src ibde-deb-build
FROM debian:trixie
RUN apt-get update -qq && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
      build-essential pkg-config curl ca-certificates \
      libcairo2-dev libpango1.0-dev libwayland-dev libxkbcommon-dev && \
    rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
ENV PATH=/root/.cargo/bin:$PATH
RUN cargo install --locked cargo-deb
WORKDIR /src
ENTRYPOINT ["bash", "-c", "cargo deb -p ibde-shell -p ibde-terminal && ls -la target/debian/"]
