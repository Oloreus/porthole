# Porthole

Fast region screenshots for Ubuntu/GNOME on Wayland: capture → preview → copy / save / delete. Fully local, no X11/XWayland, no cloud.

## Usage

The app's interface is currently German only; button and menu labels are quoted as they appear, with their meaning in brackets.

| Action | How |
|---|---|
| Take a screenshot | `Alt+S` (global, GNOME 48 and later – [Ubuntu 24.04](#ubuntu-2404-gnome-46)) · tray menu · camera button in the Porthole window · `porthole --capture` |
| Select a region | Drag with the left mouse button held down; releasing captures |
| Cancel | `Esc` or right-click |
| Copy (as image) | `Ctrl+C` or "Kopieren" (Copy) in the preview window |
| Save (PNG) | `Ctrl+S` or "Speichern" (Save) |
| Discard | `Delete`, "Löschen" (Delete) or close the window – no confirmation |
| Zoom the preview | `Ctrl+mouse wheel` (anchored at the pointer, 10 % to 500 %) · double-click: fit ↔ 100 % |
| Pan the view | Drag the image with the left mouse button · mouse wheel (with `Shift` for horizontal) |
| Fit / 100 % | `Ctrl+0` / `Ctrl+1` |
| Highlight areas | Pick rectangle, ellipse or arrow in the tool bar of the preview window, then drag on the image |
| Add text | Pick the text tool, click on the image, type · `Enter` finishes, `Shift+Enter` starts a new line, `Esc` discards |
| Shape and text color | Palette in the tool bar; the last choice is remembered |
| Undo last shape or text | `Ctrl+Z` or the undo button |
| Back to pan/zoom | `Esc` or the pointer tool · the middle mouse button pans with any tool |

Zoom only affects the display: copy and save always deliver the original at full resolution, with the drawn shapes and text burned in (without any, the original bytes unchanged). 100 % means one image pixel per screen pixel.

Every screenshot gets its own preview window; the windows are the buffer (RAM only, no temporary files). From GNOME 48 on, the shortcut can be changed under **Settings → Apps → Porthole → Global Shortcuts**. Autostart: tray menu → "Beim Anmelden starten" (Start at login).

## Installation

Supported: Ubuntu 24.04 LTS and newer (GTK ≥ 4.14, libadwaita ≥ 1.5).

```bash
cargo deb                                   # builds target/debian/porthole_*.deb
sudo apt install ./target/debian/porthole_*.deb
```

Build the `.deb` on the oldest target release (Ubuntu 24.04): the package dependencies are derived from the library versions on the build machine – a package built on 25.04+ can't be installed on 24.04. The other way round, a 24.04 package also runs on newer releases.

The package recommends `wl-clipboard` and `xclip`: terminal programs (e.g. Claude Code) read copied screenshots from the clipboard only via `wl-paste`/`xclip`, and Ubuntu ships neither. `apt install` pulls them in automatically, `dpkg -i` doesn't.

On the **first** screenshot, GNOME asks for permission once. This dialog only appears while a Porthole window has focus – so trigger the first screenshot with the camera button in the Porthole window (the app points this out). After that, everything works from the background.

## Development

Prerequisites (Ubuntu 24.04+):

```bash
sudo apt install rustup libgtk-4-dev libadwaita-1-dev pkg-config
rustup default stable && rustup component add clippy
cargo install cargo-deb --locked           # only for packaging
```

```bash
cargo build
build-aux/install-dev.sh    # once: .desktop + icons into ~/.local/share (points to target/debug)
cargo run -- --background   # background instance; further invocations talk to it
cargo run -- --capture
cargo test && cargo clippy -- -D warnings
```

**Terminal of a Snap app (e.g. VS Code):** Everything started from there runs in the Snap app's scope (`snap.code.code-….scope`). The portal would then take Porthole for VS Code, the shell wouldn't associate the windows with any app (no dock entry), and the permission dialog would be refused ("Only the focused app is allowed to show a system access dialog" in `journalctl --user`). Porthole detects this at startup and relaunches itself via `systemd-run --user` as its own unit `app-app.porthole.Porthole@<pid>` (`src/scope.rs`); with a terminal attached it uses `--pty`, so the log and Ctrl+C work as usual. This makes `cargo run` work from VS Code too.

The unit name isn't arbitrary: without the portal registry (xdg-desktop-portal < 1.20, i.e. Ubuntu 24.04) the portal derives the app ID from it (`app-<app ID>@….service`). With any other name the app ID is empty – a granted screenshot permission would then apply to *all* unidentifiable programs. To check and, if necessary, remove it:

```bash
gdbus call --session -d org.freedesktop.impl.portal.PermissionStore -o /org/freedesktop/impl/portal/PermissionStore \
  -m org.freedesktop.impl.portal.PermissionStore.Lookup screenshot screenshot            # entry '' must not appear
gdbus call --session -d org.freedesktop.impl.portal.PermissionStore -o /org/freedesktop/impl/portal/PermissionStore \
  -m org.freedesktop.impl.portal.PermissionStore.DeletePermission screenshot screenshot ''
```

Porthole is single-instance: if an instance is already running, all further launches (`cargo run`, app menu, `--capture`) go to it – after a new build, quit the old one first (tray → "Beenden" (Quit)).

During development, a custom shortcut (Ubuntu 24.04) needs the full path as its command, e.g. `/path/to/porthole/target/debug/porthole --capture` – `porthole` is only in `$PATH` after installing the `.deb`.

The installed `.desktop` file is mandatory: only with it does xdg-desktop-portal accept the app ID `app.porthole.Porthole`, which the screenshot permission and the global shortcut are tied to. Before installing the `.deb`, remove the dev entry with `build-aux/uninstall-dev.sh` (otherwise it shadows the system entry).

### Translations

Texts in the code are English and go through `tr("…")` (`src/i18n.rs`). `po/de.po` holds the German translations in gettext format and is compiled into the binary – nothing to install. The language follows `LC_ALL`, then `LC_MESSAGES`, then `LANG`; anything other than German falls back to English. When adding or changing a text, add or update its entry in `po/de.po`: the `msgid` must match the English text exactly, and placeholders like `{detail}` must be kept. For another language, add `po/<lang>.po` and register it in `CATALOGS` in `src/i18n.rs`.

Not everything is translated yet: buttons, tray menu and notifications (other than capture errors) are still hardcoded German.

## Architecture in brief

- **Freeze frame:** First the screenshot portal captures the whole desktop, then Porthole shows it per monitor as a dimmed full-screen still image and crops locally. This way the overlay can never end up in the picture. (A Wayland client can neither see "through itself" nor position windows freely.)
- `capture/` – backends behind `trait CaptureBackend`, UI-free. Currently `PortalScreenshotBackend`.
- `geometry.rs`, `monitors.rs` – coordinate spaces stage ↔ image ↔ window, purely ratio-based; layout straight from Mutter. Unit-tested including multi-monitor/mixed DPI.
- `viewport.rs` – zoom/pan state of the preview (fit vs. manual zoom, pointer-anchored zoom, pan limits), GTK-free and unit-tested. `ui/zoom_view.rs` uses it to draw the unmodified texture; scaling happens per frame on the GPU.
- `annotations.rs` – shapes and text drawn on a screenshot, kept in image pixels and GTK-free. Text is turned into glyph outlines (pango, hinting off), so it scales like the shapes. The same render nodes draw them in the preview (under the zoom/pan transform) and into the exported image (software renderer, `Screenshot::with_annotations`), so the preview matches the result.
- `controller.rs` – state machine `Idle → Capturing → Selecting → Cropping → Idle`.
- `ui/` – selection overlay, preview window, small main window. `services/` – clipboard, saving, shortcuts, tray, autostart.

## Ubuntu 24.04 (GNOME 46)

- **No global shortcut via the portal:** the GlobalShortcuts backend only exists from GNOME 48 on. Porthole detects this and, on first launch, shows a one-time notice with a button to the keyboard settings. There, under **Keyboard → View and Customize Shortcuts → Custom Shortcuts**, add a shortcut with the command `porthole --capture` (e.g. `Alt+S`). The menu entry "Settings → Apps → Global Shortcuts" doesn't exist in GNOME 46.
- The tray icon, screenshot portal and everything else work as on GNOME 48 (Ubuntu ships the AppIndicator extension enabled).
- After upgrading to GNOME 48+, Porthole registers the portal shortcut automatically; remove the custom shortcut then, otherwise both are bound.

## Known quirks (GNOME 48)

- **~0.6 s until selection mode, white flash + shutter sound:** the GNOME portal backend enforces both (it waits for the 500 ms flash animation). The sound can be turned off system-wide: `gsettings set org.gnome.desktop.sound event-sounds false`.
- **"BindShortcuts meldet …" in the log** (BindShortcuts reports "Other"): `xdg-desktop-portal-gnome` 48.0 responds with an error code even though the shortcut is bound; Porthole therefore listens anyway.
- **Permission denied by accident:** `porthole --reset-permission`, then trigger again via the camera button.
- **If the global shortcut fails:** as under [Ubuntu 24.04](#ubuntu-2404-gnome-46), add a custom shortcut with the command `porthole --capture` in Settings → Keyboard.
- Selecting across monitor boundaries isn't supported (the selection stays on the monitor where it started). Multi-monitor operation is calculated and unit-tested, but not yet verified on real hardware.
