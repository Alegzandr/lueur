//! Whether the bulbs are really on.
//!
//! Lueur records what each light was last set to, but a bulb can be
//! switched without it: at its wall switch, from its maker's app, or by
//! another computer while this one was off or asleep. A worker asks the
//! bulbs at rest whether they are on, once at launch and then every so
//! often, so the window and the tray show the room as it is
//! (`lights::sync_power`). Asking changes nothing on a bulb.

use crate::lights;
use crate::{Event, Shell};
use std::sync::Arc;
use std::time::Duration;

/// How long after launch the bulbs are first asked: long enough to hear
/// which ones another computer holds (`lease::LISTEN`).
const FIRST: Duration = Duration::from_secs(2);
/// How often they are asked again. A Yeelight allows about one command a
/// second on its control port, so this costs it next to nothing.
const EVERY: Duration = Duration::from_secs(30);

/// Keep what the window shows in line with the bulbs for as long as the app
/// runs.
pub fn spawn(shell: &Arc<Shell>) {
    let shell = Arc::clone(shell);
    let _ = std::thread::Builder::new()
        .name("lueur-power-watch".into())
        .spawn(move || {
            std::thread::sleep(FIRST);
            loop {
                // The sleep timer is dimming the room: leave it to that.
                if !crate::sleep::fading(&shell.state) && lights::sync_power(&shell.state) {
                    shell.emit(Event::LightsChanged);
                    shell.tray_sync();
                }
                std::thread::sleep(EVERY);
            }
        });
}
