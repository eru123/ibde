#!/usr/bin/env bash
# Assemble and sign the IBDE apt repository from built .debs.
# Layout (hosted on GitHub Pages):
#   packaging/repo/public-key.asc                    — repo signing key
#   packaging/repo/pool/main/*.deb
#   packaging/repo/dists/stable/main/binary-amd64/Packages{,.gz}
#   packaging/repo/dists/stable/{Release,InRelease,Release.gpg}
# Signing key: "IBDE apt repository <apt@ibde.dev>" (see packaging/repo/SECURITY.md)
set -euo pipefail
cd "$(dirname "$0")/.."

REPO=packaging/repo
SUITE=stable
KEY=apt@ibde.dev

mkdir -p "$REPO/pool/main" "$REPO/dists/$SUITE/main/binary-amd64"
cp -f target/debian/*.deb "$REPO/pool/main/"

cd "$REPO"
dpkg-scanpackages --arch amd64 pool/main /dev/null \
    > "dists/$SUITE/main/binary-amd64/Packages" 2>/dev/null
# uncompressed index only: one hash in Release, no gz/uncompressed
# double-entry ambiguity (costs a few KB at our repo size)

cd "dists/$SUITE"
apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=IBDE \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    -o APT::FTPArchive::Release::Components=main \
    -o APT::FTPArchive::Release::Description="IBDE apt repository" \
    release . > Release

gpg --default-key "$KEY" --clearsign -o InRelease Release
gpg --default-key "$KEY" --detach-sign --armor -o Release.gpg Release

cd ../..
echo "repo assembled: $(ls pool/main/*.deb | wc -l) debs, suite $SUITE"
