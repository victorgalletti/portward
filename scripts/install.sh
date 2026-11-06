#!/bin/bash
# Installs the Portward daemon (launchd, runs as root) and the app.
#   sudo ./scripts/install.sh              from the repo (after ./scripts/build.sh)
#   install.sh --from <staged dir>         used by the app's "Install update" button
set -euo pipefail
if [[ $EUID -ne 0 ]]; then echo "Rode com sudo: sudo $0"; exit 1; fi

LABEL=io.github.portward.daemon
PLIST=/Library/LaunchDaemons/$LABEL.plist
BIN_DIR=/usr/local/libexec/portward

if [[ "${1:-}" == "--from" ]]; then
  SRC="$2"
  BIN="$SRC/portwardd"; APP="$SRC/Portward.app"; PL="$SRC/$LABEL.plist"
else
  cd "$(dirname "$0")/.."
  BIN=target/release/portwardd; APP=target/release/bundle/macos/Portward.app; PL=packaging/$LABEL.plist
fi
if [[ ! -x "$BIN" || ! -d "$APP" || ! -f "$PL" ]]; then
  echo "Arquivos não encontrados. Compile antes: ./scripts/build.sh"; exit 1
fi

launchctl bootout system "$PLIST" 2>/dev/null || true
mkdir -p "$BIN_DIR" /Library/Logs/Portward
install -m 755 -o root -g wheel "$BIN" "$BIN_DIR/portwardd"
install -m 644 -o root -g wheel "$PL" "$PLIST"
rm -rf /Applications/Portward.app
cp -R "$APP" /Applications/Portward.app
chown -R root:admin /Applications/Portward.app

launchctl bootstrap system "$PLIST"
launchctl enable system/$LABEL
for _ in 1 2 3 4 5 6 7 8 9 10; do
  if launchctl print system/$LABEL 2>/dev/null | grep -q "state = running"; then
    echo "Portward instalado e rodando: $("$BIN_DIR/portwardd" --version)"
    exit 0
  fi
  sleep 1
done
echo "O serviço não subiu. Veja /Library/Logs/Portward/portwardd.log"; exit 1
