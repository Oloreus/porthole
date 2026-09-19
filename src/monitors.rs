//! Monitor-Layout im Stage-Koordinatenraum, direkt von Mutter
//! (`org.gnome.Mutter.DisplayConfig`). Das ist die maßgebliche Quelle: Der
//! Portal-Screenshot ist ein Rendering genau dieser Stage. GDKs Geometrie
//! stimmt damit nur im Logical-Layout sicher überein und dient als Fallback
//! (andere Desktops, Mutter nicht erreichbar).

use std::collections::HashMap;

use gtk::{gio, glib};

use crate::geometry::Rect;

const BUS_NAME: &str = "org.gnome.Mutter.DisplayConfig";
const OBJECT_PATH: &str = "/org/gnome/Mutter/DisplayConfig";

/// Mutter: Stage in logischen Pixeln (Modusgröße / Scale).
const LAYOUT_MODE_LOGICAL: u32 = 1;

/// Stage-Rechteck je Anschluss (z. B. "HDMI-1", wie `GdkMonitor::connector`).
pub type StageLayout = HashMap<String, Rect>;

pub async fn query() -> Option<StageLayout> {
    let connection = gio::bus_get_future(gio::BusType::Session).await.ok()?;
    let state = connection
        .call_future(
            Some(BUS_NAME),
            OBJECT_PATH,
            BUS_NAME,
            "GetCurrentState",
            None,
            None,
            gio::DBusCallFlags::NONE,
            1000,
        )
        .await
        .ok()?;
    parse_current_state(&state)
}

/// Antworttyp: `(u serial, a((ssss)a(siiddada{sv})a{sv}) monitors,
/// a(iiduba(ssss)a{sv}) logical_monitors, a{sv} properties)`
fn parse_current_state(state: &glib::Variant) -> Option<StageLayout> {
    if state.n_children() != 4 {
        return None;
    }
    let (monitors, logical_monitors, properties) =
        (state.child_value(1), state.child_value(2), state.child_value(3));

    // Ohne Angabe gilt Physical-Layout (Verhalten älterer Mutter-Versionen).
    let logical_layout = glib::VariantDict::new(Some(&properties))
        .lookup::<u32>("layout-mode")
        .ok()
        .flatten()
        == Some(LAYOUT_MODE_LOGICAL);

    // Aktuelle Modusgröße je Anschluss.
    let mut mode_sizes: HashMap<String, (f64, f64)> = HashMap::new();
    for monitor in monitors.iter() {
        let connector = monitor.child_value(0).child_value(0).str()?.to_owned();
        for mode in monitor.child_value(1).iter() {
            let is_current = glib::VariantDict::new(Some(&mode.child_value(6)))
                .lookup::<bool>("is-current")
                .ok()
                .flatten()
                .unwrap_or(false);
            if is_current {
                let width = mode.child_value(1).get::<i32>()?;
                let height = mode.child_value(2).get::<i32>()?;
                mode_sizes.insert(connector.clone(), (f64::from(width), f64::from(height)));
            }
        }
    }

    let mut layout = StageLayout::new();
    for logical in logical_monitors.iter() {
        let x = f64::from(logical.child_value(0).get::<i32>()?);
        let y = f64::from(logical.child_value(1).get::<i32>()?);
        let scale = logical.child_value(2).get::<f64>()?;
        let transform = logical.child_value(3).get::<u32>()?;

        // Bei Spiegelung teilen sich mehrere Anschlüsse ein Rechteck.
        for member in logical.child_value(5).iter() {
            let connector = member.child_value(0).str()?.to_owned();
            let (mut width, mut height) = *mode_sizes.get(&connector)?;
            // Transformationen 1, 3, 5, 7 sind Drehungen um 90°/270°.
            if transform % 2 == 1 {
                std::mem::swap(&mut width, &mut height);
            }
            if logical_layout && scale > 0.0 {
                width /= scale;
                height /= scale;
            }
            layout.insert(connector, Rect::new(x, y, width, height));
        }
    }

    (!layout.is_empty()).then_some(layout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Option<StageLayout> {
        let ty = glib::VariantTy::new("(ua((ssss)a(siiddada{sv})a{sv})a(iiduba(ssss)a{sv})a{sv})")
            .unwrap();
        parse_current_state(&glib::Variant::parse(Some(ty), text).unwrap())
    }

    #[test]
    fn single_monitor_physical_layout() {
        // Gekürzte echte Antwort des Zielsystems (HDMI-1, 2560x1440 @ 100 %).
        let layout = parse(
            "(1, [(('HDMI-1', 'GBT', 'G34WQC', '0'), \
              [('2560x1440@59.951', 2560, 1440, 59.95, 1.0, [1.0, 2.0], {'is-current': <true>}), \
               ('1920x1080@60.000', 1920, 1080, 60.0, 1.0, [1.0, 2.0], {})], {})], \
              [(0, 0, 1.0, 0, true, [('HDMI-1', 'GBT', 'G34WQC', '0')], {})], \
              {'layout-mode': <uint32 2>})",
        )
        .unwrap();
        assert_eq!(layout["HDMI-1"], Rect::new(0.0, 0.0, 2560.0, 1440.0));
    }

    #[test]
    fn mixed_scales_logical_layout() {
        let layout = parse(
            "(1, [(('DP-1', 'A', 'B', '0'), [('m', 2560, 1440, 60.0, 1.0, [1.0], {'is-current': <true>})], {}), \
                  (('DP-2', 'A', 'B', '1'), [('m', 3840, 2160, 60.0, 1.5, [1.0], {'is-current': <true>})], {})], \
              [(0, 0, 1.0, 0, true, [('DP-1', 'A', 'B', '0')], {}), \
               (2560, 0, 1.5, 0, false, [('DP-2', 'A', 'B', '1')], {})], \
              {'layout-mode': <uint32 1>})",
        )
        .unwrap();
        assert_eq!(layout["DP-1"], Rect::new(0.0, 0.0, 2560.0, 1440.0));
        assert_eq!(layout["DP-2"], Rect::new(2560.0, 0.0, 2560.0, 1440.0));
    }

    #[test]
    fn physical_layout_keeps_mode_size_and_rotation_swaps() {
        let layout = parse(
            "(1, [(('DP-1', 'A', 'B', '0'), [('m', 3840, 2160, 60.0, 2.0, [1.0], {'is-current': <true>})], {})], \
              [(0, 0, 2.0, 1, true, [('DP-1', 'A', 'B', '0')], {})], \
              {'layout-mode': <uint32 2>})",
        )
        .unwrap();
        assert_eq!(layout["DP-1"], Rect::new(0.0, 0.0, 2160.0, 3840.0));
    }
}
