#!/bin/bash
# Removes Portward and gives the system its original DNS settings back.
# Usage: sudo ./scripts/uninstall.sh [--purge]   (--purge also deletes config and history)
set -uo pipefail
if [[ $EUID -ne 0 ]]; then echo "Rode com sudo: sudo $0"; exit 1; fi
LABEL=io.github.portward.daemon
PLIST=/Library/LaunchDaemons/$LABEL.plist
BIN=/usr/local/libexec/portward/portwardd

# Stopping the daemon restores DNS and removes the pf anchor.
launchctl bootout system "$PLIST" 2>/dev/null
sleep 2
# Safety net in case the daemon was not running.
[[ -x "$BIN" ]] && "$BIN" restore-dns
/sbin/pfctl -a com.apple/portward -F all 2>/dev/null

rm -f "$PLIST"
rm -rf /usr/local/libexec/portward /Applications/Portward.app
if [[ "${1:-}" == "--purge" ]]; then
  rm -rf "/Library/Application Support/Portward" /Library/Logs/Portward
fi
echo "Portward removido. DNS do sistema restaurado."
