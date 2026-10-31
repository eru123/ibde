# Reusable headless-sway test rig for layer-shell clients (spike, later
# shell tests). Build once, run cheap:
#   docker build -t ibde-sway-test bench/docker/sway-test
FROM debian:trixie
RUN apt-get update -qq && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
        sway fonts-dejavu-core libcairo2 libpangocairo-1.0-0 \
        libwayland-client0 procps && \
    rm -rf /var/lib/apt/lists/*
RUN mkdir -p /tmp/xdg && chmod 700 /tmp/xdg
ENV XDG_RUNTIME_DIR=/tmp/xdg WLR_BACKENDS=headless WLR_RENDERER=pixman \
    XKB_DEFAULT_LAYOUT=us
COPY entry.sh /entry.sh
ENTRYPOINT ["bash", "/entry.sh"]
