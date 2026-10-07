#!/usr/bin/env bash
# Installs the .desktop file and icons into ~/.local/share, with Exec pointing
# to the debug build. Required because the portal only accepts the app ID if a
# matching .desktop file is installed.
set -euo pipefail

APP_ID=app.porthole.Porthole
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/debug/porthole"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
# Terminals in Snap apps (e.g. VS Code) redirect XDG_DATA_HOME to ~/snap/…;
# GNOME Shell never sees the .desktop file there.
case "$DATA" in "$HOME"/snap/*) DATA="$HOME/.local/share" ;; esac

install -Dm644 "$ROOT/data/icons/hicolor/scalable/apps/$APP_ID.svg" \
  "$DATA/icons/hicolor/scalable/apps/$APP_ID.svg"
install -Dm644 "$ROOT/data/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg" \
  "$DATA/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"

mkdir -p "$DATA/applications"
sed "s|^Exec=porthole|Exec=$BIN|" "$ROOT/data/$APP_ID.desktop" \
  > "$DATA/applications/$APP_ID.desktop"

update-desktop-database "$DATA/applications" 2>/dev/null || true
# Refresh on both install and uninstall so cached system icons cannot shadow
# our local overrides. The user theme inherits the system index.theme.
gtk-update-icon-cache --force --ignore-theme-index "$DATA/icons/hicolor"
# Icon-theme consumers watch the theme directory, not individual SVG files.
# Replacing an existing icon must invalidate their in-memory lookup cache too.
touch "$DATA/icons/hicolor"

case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
  de*) echo "Installiert: $DATA/applications/$APP_ID.desktop -> $BIN" ;;
  *) echo "Installed: $DATA/applications/$APP_ID.desktop -> $BIN" ;;
esac
