#!/bin/bash
# Builds the release daemon and the Portward.app / .dmg bundle.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
cargo build --release -p portward-daemon
cp ui/src-tauri/icons/128x128.png ui/dist/icon.png
(cd ui && npx -y @tauri-apps/cli@2 build --bundles app,dmg)
echo
echo "Daemon: target/release/portwardd"
echo "App:    target/release/bundle/macos/Portward.app"
ls target/release/bundle/dmg/*.dmg 2>/dev/null || true
