#!/usr/bin/env bash
set -euo pipefail

APP_ID=app.porthole.Porthole
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
# Terminals in Snap-Apps (z. B. VS Code) biegen XDG_DATA_HOME nach ~/snap/…
# um; dort sieht die GNOME Shell die .desktop-Datei nie.
case "$DATA" in "$HOME"/snap/*) DATA="$HOME/.local/share" ;; esac

rm -f "$DATA/applications/$APP_ID.desktop" \
      "$DATA/icons/hicolor/scalable/apps/$APP_ID.svg" \
      "$DATA/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"
rm -f "$DATA/icons/hicolor/icon-theme.cache"
update-desktop-database "$DATA/applications" 2>/dev/null || true

echo "Entfernt."
