#!/bin/bash
# Builds the release daemon and Portward.app, then stages them so the running
# app can offer "Install update" (no terminal or sudo needed).
# Usage: ./scripts/build.sh [--dmg]   (--dmg also builds the .dmg installer)
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
export PORTWARD_BUILD="$(date +%Y%m%d%H%M%S)-$(git rev-parse --short HEAD 2>/dev/null || echo nogit)"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

cargo build --release -p portward-daemon
cp ui/src-tauri/icons/128x128.png ui/dist/icon.png
# The .dmg installer is only needed to hand the app to someone else; building
# it mounts the disk image and opens a Finder window, so it is opt-in.
BUNDLES=app
[[ "${1:-}" == "--dmg" ]] && BUNDLES=app,dmg
(cd ui && npx -y @tauri-apps/cli@2 build --bundles "$BUNDLES")

STAGE="$HOME/Library/Application Support/Portward/staged"
rm -rf "$STAGE.tmp" && mkdir -p "$STAGE.tmp"
cp target/release/portwardd "$STAGE.tmp/"
cp -R target/release/bundle/macos/Portward.app "$STAGE.tmp/"
cp scripts/install.sh packaging/io.github.portward.daemon.plist "$STAGE.tmp/"
echo "$VERSION $PORTWARD_BUILD" > "$STAGE.tmp/build.txt"
rm -rf "$STAGE" && mv "$STAGE.tmp" "$STAGE"

echo
echo "Versão $VERSION ($PORTWARD_BUILD) pronta."
echo "O app Portward vai mostrar \"Instalar atualização\"."
echo "Primeira instalação: sudo ./scripts/install.sh"
