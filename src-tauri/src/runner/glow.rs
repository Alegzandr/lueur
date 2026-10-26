//! What every driven light shows, for the window to draw.

use lueur_core::color::BulbOutput;
use std::collections::HashMap;
use std::sync::Mutex;

/// What a driven light shows, one zone of it: the colour or the white sent to
/// its bulb, before the walls' correction, for the window to draw.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Glow {
    pub rgb: Option<[u8; 3]>,
    pub kelvin: Option<u16>,
    /// 0..=100.
    pub bright: u8,
}

impl From<BulbOutput> for Glow {
    fn from(o: BulbOutput) -> Self {
        match o {
            BulbOutput::Color { rgb, bright } => Glow {
                rgb: Some([rgb.r, rgb.g, rgb.b]),
                kelvin: None,
                bright,
            },
            BulbOutput::White { kelvin, bright } => Glow {
                rgb: None,
                kelvin: Some(kelvin),
                bright,
            },
        }
    }
}

/// The last frame each driven light was sent, zone by zone, by room light id.
static GLOWS: Mutex<Option<HashMap<String, Vec<Glow>>>> = Mutex::new(None);

/// What every driven light shows right now (see [`Glow`]).
pub fn glows() -> HashMap<String, Vec<Glow>> {
    GLOWS
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default()
}

/// Remember the frame just sent to `id`. Reuses the light's entry, so a
/// frame costs no allocation once the effect has started.
pub(super) fn record_glow(id: &str, zones: &[BulbOutput]) {
    let Ok(mut guard) = GLOWS.lock() else { return };
    let all = guard.get_or_insert_with(HashMap::new);
    match all.get_mut(id) {
        Some(glow) => {
            glow.clear();
            glow.extend(zones.iter().map(|&o| Glow::from(o)));
        }
        None => {
            all.insert(
                id.to_owned(),
                zones.iter().map(|&o| Glow::from(o)).collect(),
            );
        }
    }
}

/// Forget the lights a loop drove once it ends: they show nothing of it now.
pub(super) fn forget_glows(ids: &[String]) {
    if let Ok(mut guard) = GLOWS.lock()
        && let Some(all) = guard.as_mut()
    {
        for id in ids {
            all.remove(id);
        }
    }
}
