#!/usr/bin/env bash
set -euo pipefail

APP_ID=app.porthole.Porthole
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"

rm -f "$DATA/applications/$APP_ID.desktop" \
      "$DATA/icons/hicolor/scalable/apps/$APP_ID.svg" \
      "$DATA/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"
rm -f "$DATA/icons/hicolor/icon-theme.cache"
update-desktop-database "$DATA/applications" 2>/dev/null || true

echo "Entfernt."
