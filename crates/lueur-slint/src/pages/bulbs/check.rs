//! The check of devices nothing vouches for: a paired Hue bridge or Nanoleaf
//! controller the last search heard where nothing proves it is the one
//! paired (a new address, or another certificate). Lueur sends it nothing
//! until the user says it is theirs; see `crates/lueur-shell/src/identity.rs`.

use super::{SEARCH_MS, pairing, scan};
use crate::i18n::{fill, t};
use crate::state::Lueur;
use lueur_shell::identity::{self, Unverified};

/// A device as it showed itself: a "not mine" holds for the session until
/// it shows up somewhere else or with another certificate.
pub fn sighting(d: &Unverified) -> String {
    format!("{}|{}|{}", d.id, d.ip, d.changed)
}

/// What the check says of a device.
pub fn say(d: &Unverified) -> String {
    let vars = [
        ("name", d.name.as_str()),
        ("ip", d.ip.as_str()),
        ("known", d.known_ip.as_str()),
    ];
    let key = if d.needs_pairing {
        "unverified_pinned"
    } else if d.changed {
        "unverified_changed"
    } else {
        "unverified_moved"
    };
    fill(&t(key), &vars)
}

/// Ask the shell which devices wait for the user's word, once at start.
pub fn load(l: &mut Lueur) {
    l.run_then(
        |s| identity::unverified_devices(s),
        |l, result| {
            if let Ok(list) = result {
                l.unverified = list;
            }
        },
    );
}

/// The devices to check now, those not answered "not mine".
pub fn shown(l: &Lueur) -> Vec<Unverified> {
    l.unverified
        .iter()
        .filter(|d| !l.dismissed_devices.contains(&sighting(d)))
        .cloned()
        .collect()
}

/// Whether the devices to check changed since they were last read out.
pub fn news_due(l: &Lueur) -> bool {
    let seen: Vec<String> = l.unverified.iter().map(sighting).collect();
    seen != l.pages.bulbs.check_seen
}

/// Read out the devices a search brings, not each time the page is drawn.
pub fn note_news(l: &mut Lueur) {
    if !news_due(l) {
        return;
    }
    let fresh: Vec<String> = shown(l)
        .iter()
        .filter(|d| !l.pages.bulbs.check_seen.contains(&sighting(d)))
        .map(say)
        .collect();
    l.pages.bulbs.check_seen = l.unverified.iter().map(sighting).collect();
    l.pages.bulbs.check_news = fresh.join(" ");
}

fn find(l: &Lueur, id: &str) -> Option<Unverified> {
    l.unverified.iter().find(|d| d.id == id).cloned()
}

/// "Not mine": the device is left alone for the session.
pub fn not_mine(l: &mut Lueur, id: &str) {
    if let Some(d) = find(l, id) {
        l.dismissed_devices.push(sighting(&d));
    }
}

/// The user says the device is theirs: it is trusted where it is now.
pub fn confirm(l: &mut Lueur, id: &str) {
    let Some(d) = find(l, id) else { return };
    l.pages.bulbs.check_busy = true;
    let device = d.id.clone();
    l.run_then(
        move |s| identity::confirm_device(s, &device),
        move |l, result| {
            l.pages.bulbs.check_busy = false;
            match result {
                Ok(()) => {
                    l.unverified.retain(|x| x.id != d.id);
                    let text = fill(&l.t("unverified_confirmed"), &[("name", &d.name)]);
                    l.inform(text);
                    // Its lamps come back through the search, at the new
                    // address.
                    scan(l, SEARCH_MS, false, None);
                }
                Err(e) => l.fail(&e),
            }
        },
    );
}

/// Pair it again with its button: the one way back for a bridge that no
/// longer shows its pinned certificate, whose old pairing goes first.
pub fn pair_again(l: &mut Lueur, id: &str) {
    let Some(d) = find(l, id) else { return };
    let Some(found) = l
        .pages
        .bulbs
        .discovered
        .iter()
        .find(|x| x.id == d.id && x.ip == d.ip)
        .cloned()
    else {
        scan(l, SEARCH_MS, false, None);
        return;
    };
    l.pages.bulbs.check_busy = true;
    let (device, forget) = (d.id.clone(), d.needs_pairing);
    l.run_then(
        move |s| {
            if forget {
                identity::forget_device(s, &device)
            } else {
                Ok(())
            }
        },
        move |l, result| {
            l.pages.bulbs.check_busy = false;
            match result {
                Ok(()) => {
                    l.unverified.retain(|x| x.id != d.id);
                    pairing::pair(l, found);
                }
                Err(e) => l.fail(&e),
            }
        },
    );
}
