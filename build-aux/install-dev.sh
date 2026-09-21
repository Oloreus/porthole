#!/usr/bin/env bash
# Installiert .desktop-Datei und Icons nach ~/.local/share, mit Exec auf den
# Debug-Build. Nötig, weil das Portal die App-ID nur akzeptiert, wenn eine
# passende .desktop-Datei installiert ist.
set -euo pipefail

APP_ID=app.porthole.Porthole
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/debug/porthole"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
# Terminals in Snap-Apps (z. B. VS Code) biegen XDG_DATA_HOME nach ~/snap/…
# um; dort sieht die GNOME Shell die .desktop-Datei nie.
case "$DATA" in "$HOME"/snap/*) DATA="$HOME/.local/share" ;; esac

install -Dm644 "$ROOT/data/icons/hicolor/scalable/apps/$APP_ID.svg" \
  "$DATA/icons/hicolor/scalable/apps/$APP_ID.svg"
install -Dm644 "$ROOT/data/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg" \
  "$DATA/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"

mkdir -p "$DATA/applications"
sed "s|^Exec=porthole|Exec=$BIN|" "$ROOT/data/$APP_ID.desktop" \
  > "$DATA/applications/$APP_ID.desktop"

update-desktop-database "$DATA/applications" 2>/dev/null || true
# Bewusst kein icon-theme.cache im Benutzerverzeichnis anlegen: ohne Cache
# scannt GTK das Verzeichnis direkt; ein Cache würde nach dem Entfernen von
# Icons veralten und auf gelöschte Dateien zeigen.
rm -f "$DATA/icons/hicolor/icon-theme.cache"

echo "Installiert: $DATA/applications/$APP_ID.desktop -> $BIN"
