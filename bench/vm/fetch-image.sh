#!/usr/bin/env bash
# Fetch the Debian 13 (trixie) generic amd64 cloud image (full kernel,
# includes virtio/DRM drivers — the cloud kernel does not).
set -euo pipefail
cd "$(dirname "$0")"
URL_BASE="https://cloud.debian.org/images/cloud/trixie/latest"
IMG="debian-13-generic-amd64.qcow2"
if [[ -f "$IMG" ]]; then
  echo "$IMG already present"
  exit 0
fi
echo "downloading $URL_BASE/$IMG ..."
curl -fL --retry 3 --progress-bar -o "$IMG.part" "$URL_BASE/$IMG"
mv "$IMG.part" "$IMG"
echo "done: $(du -h "$IMG" | cut -f1)"
