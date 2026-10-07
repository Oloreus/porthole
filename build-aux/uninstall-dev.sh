#!/usr/bin/env bash
set -euo pipefail

APP_ID=app.porthole.Porthole
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
# Terminals in Snap apps (e.g. VS Code) redirect XDG_DATA_HOME to ~/snap/…;
# GNOME Shell never sees the .desktop file there.
case "$DATA" in "$HOME"/snap/*) DATA="$HOME/.local/share" ;; esac

rm -f "$DATA/applications/$APP_ID.desktop" \
      "$DATA/icons/hicolor/scalable/apps/$APP_ID.svg" \
      "$DATA/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"
if [[ -d "$DATA/icons/hicolor" ]]; then
  gtk-update-icon-cache --force --ignore-theme-index "$DATA/icons/hicolor"
  touch "$DATA/icons/hicolor"
fi
update-desktop-database "$DATA/applications" 2>/dev/null || true

case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
  de*) echo "Entfernt." ;;
  *) echo "Removed." ;;
esac
