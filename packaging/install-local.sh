#!/usr/bin/env bash
# Install Datara desktop-integration assets into ~/.local for dev use.
# Usage: ./packaging/install-local.sh   (run from repo root; also installs
# target/release/datara into ~/.local/bin if it exists)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
APPID="io.github.ntxinh.Datara"

install -Dm644 "$ROOT/packaging/share/$APPID.desktop" \
    "$DATA/applications/$APPID.desktop"
install -Dm644 "$ROOT/packaging/share/$APPID.metainfo.xml" \
    "$DATA/metainfo/$APPID.metainfo.xml"
install -Dm644 "$ROOT/assets/icons/datara.svg" \
    "$DATA/icons/hicolor/scalable/apps/$APPID.svg"
for size in 32 48 64 128 256 512; do
    install -Dm644 "$ROOT/assets/icons/datara-$size.png" \
        "$DATA/icons/hicolor/${size}x${size}/apps/$APPID.png"
done

if [ -f "$ROOT/target/release/datara" ]; then
    install -Dm755 "$ROOT/target/release/datara" "$HOME/.local/bin/datara"
fi

update-desktop-database "$DATA/applications" 2>/dev/null || true
gtk-update-icon-cache -f -t "$DATA/icons/hicolor" 2>/dev/null || true
echo "Installed $APPID desktop assets under $DATA"
