#!/bin/bash
# Installs the Portward daemon (launchd, runs as root) and the app.
# Usage: sudo ./scripts/install.sh
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $EUID -ne 0 ]]; then echo "Rode com sudo: sudo $0"; exit 1; fi

LABEL=io.github.portward.daemon
PLIST=/Library/LaunchDaemons/$LABEL.plist
BIN_DIR=/usr/local/libexec/portward
APP=target/release/bundle/macos/Portward.app

if [[ ! -x target/release/portwardd || ! -d "$APP" ]]; then
  echo "Compile antes, como seu usuário (não com sudo): ./scripts/build.sh"; exit 1
fi

# Something else on port 53?
if lsof -nP -iUDP:53 -sUDP:Idle 2>/dev/null | grep -v portwardd | grep -q UDP; then
  echo "Aviso: outro programa já usa a porta 53:"; lsof -nP -iUDP:53 | head -5
fi

launchctl bootout system "$PLIST" 2>/dev/null || true
mkdir -p "$BIN_DIR" /Library/Logs/Portward
install -m 755 -o root -g wheel target/release/portwardd "$BIN_DIR/portwardd"
install -m 644 -o root -g wheel packaging/$LABEL.plist "$PLIST"
rm -rf /Applications/Portward.app
cp -R "$APP" /Applications/Portward.app

launchctl bootstrap system "$PLIST"
launchctl enable system/$LABEL
sleep 2
if launchctl print system/$LABEL | grep -q "state = running"; then
  echo "Portward instalado e rodando."
  echo "Abra /Applications/Portward.app. Log: /Library/Logs/Portward/portwardd.log"
else
  echo "O serviço não subiu. Veja /Library/Logs/Portward/portwardd.log"; exit 1
fi
