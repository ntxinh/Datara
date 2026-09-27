#!/usr/bin/env bash
# User-local fontconfig-devel for systems where the rpm can't be installed
# (no root). Extracts headers + pkgconfig metadata into .sysdeps/ so slint's
# femtovg renderer can build; libfontconfig.so is symlinked to the system lib.
# Skipped entirely when pkg-config already finds fontconfig.
set -euo pipefail
cd "$(dirname "$0")/.."

if pkg-config --exists fontconfig; then
    exit 0
fi

dest=.sysdeps
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

dnf download fontconfig-devel --destdir="$tmp" --quiet
rpm=$(ls "$tmp"/fontconfig-devel-*.x86_64.rpm | head -1)
mkdir -p "$dest"
(cd "$dest" && rpm2cpio "$rpm" | cpio -idm --quiet)

pc="$dest/usr/lib64/pkgconfig/fontconfig.pc"
sed -i "s|^prefix=/usr|prefix=$(pwd)/$dest/usr|; /^Requires.private:/d" "$pc"
ln -sf "$(rpm -ql fontconfig | grep 'libfontconfig\.so\.[0-9]*$' | head -1)" \
    "$dest/usr/lib64/libfontconfig.so"
