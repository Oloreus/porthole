# Porthole

Schnelle Bereichs-Screenshots für Ubuntu/GNOME unter Wayland: aufnehmen → Vorschau → kopieren / speichern / löschen. Vollständig lokal, kein X11/XWayland, keine Cloud.

## Bedienung

| Aktion | Wie |
|---|---|
| Screenshot starten | `Alt+S` (global, ab GNOME 48 – [Ubuntu 24.04](#ubuntu-2404-gnome-46)) · Tray-Menü · Kamera-Button im Porthole-Fenster · `porthole --capture` |
| Bereich wählen | Mit gedrückter linker Maustaste aufziehen, Loslassen nimmt auf |
| Abbrechen | `ESC` oder Rechtsklick |
| Kopieren (als Bild) | `Strg+C` oder „Kopieren" im Vorschaufenster |
| Speichern (PNG) | `Strg+S` oder „Speichern" |
| Verwerfen | `Entf`, „Löschen" oder Fenster schließen – ohne Rückfrage |
| Vorschau zoomen | `Strg+Mausrad` (am Mauszeiger verankert, 10 % bis 500 %) · Doppelklick: eingepasst ↔ 100 % |
| Ausschnitt verschieben | Bild mit linker Maustaste ziehen · Mausrad (mit `Umschalt` horizontal) |
| Einpassen / 100 % | `Strg+0` / `Strg+1` |

Zoom betrifft nur die Anzeige: Kopieren und Speichern liefern immer das Original in voller Auflösung. 100 % heißt ein Bildpixel je Bildschirmpixel.

Jeder Screenshot bekommt ein eigenes Vorschaufenster; die Fenster sind der Zwischenspeicher (nur im RAM, keine temporären Dateien). Das Tastenkürzel lässt sich ab GNOME 48 unter **Einstellungen → Apps → Porthole → Globale Tastenkürzel** ändern. Autostart: Tray-Menü → „Beim Anmelden starten".

## Installation

Unterstützt: Ubuntu 24.04 LTS und neuer (GTK ≥ 4.14, libadwaita ≥ 1.5).

```bash
cargo deb                                   # baut target/debian/porthole_*.deb
sudo apt install ./target/debian/porthole_*.deb
```

Das `.deb` auf der ältesten Zielversion bauen (Ubuntu 24.04): Die Paketabhängigkeiten werden aus den Bibliotheksversionen der Build-Maschine abgeleitet – ein auf 25.04+ gebautes Paket lässt sich auf 24.04 nicht installieren. Umgekehrt läuft ein 24.04-Paket auch auf neueren Versionen.

Das Paket empfiehlt `wl-clipboard` und `xclip`: Terminal-Programme (z. B. Claude Code) lesen kopierte Screenshots nur über `wl-paste`/`xclip` aus der Zwischenablage, und Ubuntu bringt beide nicht mit. `apt install` zieht sie automatisch mit, `dpkg -i` nicht.

Beim **ersten** Screenshot fragt GNOME einmalig um Erlaubnis. Dieser Dialog erscheint nur, wenn ein Porthole-Fenster den Fokus hat – deshalb den ersten Screenshot über den Kamera-Button im Porthole-Fenster auslösen (die App weist darauf hin). Danach funktioniert alles aus dem Hintergrund.

## Entwickeln

Voraussetzungen (Ubuntu 24.04+):

```bash
sudo apt install rustup libgtk-4-dev libadwaita-1-dev pkg-config
rustup default stable && rustup component add clippy
cargo install cargo-deb --locked           # nur fürs Paketieren
```

```bash
cargo build
build-aux/install-dev.sh    # einmalig: .desktop + Icons nach ~/.local/share (zeigt auf target/debug)
cargo run -- --background   # Hintergrundinstanz; weitere Aufrufe sprechen mit ihr
cargo run -- --capture
cargo test && cargo clippy -- -D warnings
```

**Terminal einer Snap-App (z. B. VS Code):** Alles, was von dort startet, läuft im Scope der Snap-App (`snap.code.code-….scope`). Das Portal hielte Porthole dann für VS Code, die Shell ordnete die Fenster keiner App zu (kein Dock-Eintrag), und der Freigabedialog würde verweigert („Only the focused app is allowed to show a system access dialog“ in `journalctl --user`). Porthole erkennt das beim Start und startet sich per `systemd-run --user` als eigene Unit `app-app.porthole.Porthole@<pid>` neu (`src/scope.rs`); mit Terminal über `--pty`, sodass Log und Strg+C wie gewohnt funktionieren. `cargo run` geht damit auch aus VS Code.

Der Unit-Name ist nicht beliebig: Ohne Portal-Registry (xdg-desktop-portal < 1.20, also Ubuntu 24.04) leitet das Portal die App-ID aus ihm ab (`app-<App-ID>@….service`). Bei jedem anderen Namen ist die App-ID leer – eine erteilte Screenshot-Freigabe gälte dann für *alle* nicht zuordenbaren Programme. Prüfen und ggf. entfernen:

```bash
gdbus call --session -d org.freedesktop.impl.portal.PermissionStore -o /org/freedesktop/impl/portal/PermissionStore \
  -m org.freedesktop.impl.portal.PermissionStore.Lookup screenshot screenshot            # Eintrag '' darf nicht auftauchen
gdbus call --session -d org.freedesktop.impl.portal.PermissionStore -o /org/freedesktop/impl/portal/PermissionStore \
  -m org.freedesktop.impl.portal.PermissionStore.DeletePermission screenshot screenshot ''
```

Porthole ist Single-Instance: Läuft schon eine Instanz, landen alle weiteren Starts (`cargo run`, App-Menü, `--capture`) bei ihr – nach einem neuen Build die alte vorher beenden (Tray → Beenden).

Ein eigenes Tastenkürzel (Ubuntu 24.04) braucht während der Entwicklung den vollen Pfad als Befehl, z. B. `/pfad/zu/porthole/target/debug/porthole --capture` – `porthole` liegt erst nach Installation des `.deb` im `$PATH`.

Die installierte `.desktop`-Datei ist Pflicht: nur damit akzeptiert das xdg-desktop-portal die App-ID `app.porthole.Porthole`, an der Screenshot-Berechtigung und globales Tastenkürzel hängen. Vor der Installation des `.deb` den Dev-Eintrag mit `build-aux/uninstall-dev.sh` entfernen (er überdeckt sonst den Systemeintrag).

## Architektur in Kürze

- **Freeze-Frame:** Erst nimmt das Screenshot-Portal den ganzen Desktop auf, dann zeigt Porthole ihn je Monitor als Vollbild-Standbild mit Abdunklung und schneidet lokal zu. Das Overlay kann so nie im Bild landen. (Ein Wayland-Client kann weder „durch sich hindurch" sehen noch Fenster frei platzieren.)
- `capture/` – Backends hinter `trait CaptureBackend`, UI-frei. Aktuell `PortalScreenshotBackend`.
- `geometry.rs`, `monitors.rs` – Koordinatenräume Stage ↔ Bild ↔ Fenster, rein verhältnisbasiert; Layout direkt von Mutter. Unit-getestet inkl. Mehrschirm/Mixed-DPI.
- `viewport.rs` – Zoom-/Pan-Zustand der Vorschau (Einpassen vs. manueller Zoom, zeigerverankerter Zoom, Pan-Grenzen), GTK-frei und unit-getestet. `ui/zoom_view.rs` zeichnet damit die unveränderte Textur; skaliert wird je Frame auf der GPU.
- `controller.rs` – Zustandsautomat `Idle → Capturing → Selecting → Cropping → Idle`.
- `ui/` – Auswahl-Overlay, Vorschaufenster, kleines Hauptfenster. `services/` – Zwischenablage, Speichern, Tastenkürzel, Tray, Autostart.

## Ubuntu 24.04 (GNOME 46)

- **Kein globales Tastenkürzel über das Portal:** Das GlobalShortcuts-Backend gibt es erst ab GNOME 48. Porthole erkennt das und zeigt beim ersten Start einmalig einen Hinweis mit Button zu den Tastatureinstellungen. Dort unter **Tastatur → Tastenkürzel anzeigen und anpassen → Eigene Tastenkürzel** ein Kürzel mit dem Befehl `porthole --capture` anlegen (z. B. `Alt+S`). Der Menüpunkt „Einstellungen → Apps → Globale Tastenkürzel“ existiert in GNOME 46 nicht.
- Tray-Icon, Screenshot-Portal und alles andere funktionieren wie unter GNOME 48 (Ubuntu liefert die AppIndicator-Erweiterung aktiviert mit).
- Nach einem Upgrade auf GNOME 48+ meldet Porthole das Portal-Kürzel automatisch an; das eigene Kürzel dann wieder entfernen, sonst sind beide belegt.

## Bekannte Eigenheiten (GNOME 48)

- **~0,6 s bis zum Auswahlmodus, weißer Blitz + Auslöser-Ton:** Das GNOME-Portal-Backend erzwingt beides (wartet die 500-ms-Blitzanimation ab). Ton systemweit abschaltbar: `gsettings set org.gnome.desktop.sound event-sounds false`.
- **„BindShortcuts meldet Other" im Log:** `xdg-desktop-portal-gnome` 48.0 antwortet mit Fehlercode, obwohl das Kürzel gebunden ist; Porthole lauscht deshalb trotzdem.
- **Berechtigung versehentlich verweigert:** `porthole --reset-permission`, dann erneut über den Kamera-Button auslösen.
- **Falls das globale Kürzel ausfällt:** Wie unter [Ubuntu 24.04](#ubuntu-2404-gnome-46) in Einstellungen → Tastatur ein eigenes Tastenkürzel mit dem Befehl `porthole --capture` anlegen.
- Auswahl über Monitorgrenzen hinweg wird nicht unterstützt (Auswahl bleibt auf dem Monitor, auf dem sie begann). Mehrschirm-Betrieb ist berechnet und unit-getestet, aber noch nicht auf echter Hardware geprüft.
