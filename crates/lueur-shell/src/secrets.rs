//! Credentials stay in the backend.
//!
//! What bridges and Smart Life granted (Hue usernames, Nanoleaf tokens, Tuya
//! local keys) is all anyone needs to drive the bulbs, and the interface
//! never needs it: it gets copies with every credential blanked, and what it
//! sends back has them filled in again from the pairings. An exported config
//! leaves them out too, so a file shared for help shares no keys; on another
//! computer the bridges are paired and Smart Life imported again.

use lueur_core::config::{AppConfig, DeviceBinding};
use lueur_core::device::{DeviceKind, DiscoveredDevice, Pairings, hue, nanoleaf, tuya};

fn blank(kind: &mut DeviceKind) {
    match kind {
        DeviceKind::Hue {
            username,
            clientkey,
            ..
        } => {
            username.clear();
            clientkey.clear();
        }
        DeviceKind::Nanoleaf { token } => token.clear(),
        DeviceKind::Tuya { key, .. } => key.clear(),
        _ => {}
    }
}

/// The config as the interface (or an export) may see it.
pub fn redact_config(cfg: &AppConfig) -> AppConfig {
    let mut cfg = cfg.clone();
    cfg.pairings = Pairings::default();
    for b in &mut cfg.bindings {
        blank(&mut b.kind);
    }
    cfg
}

pub fn redact_devices(devices: &mut [DiscoveredDevice]) {
    for d in devices {
        blank(&mut d.kind);
    }
}

/// Fill in `kind`'s credential for device `id` at `ip` from the pairings.
/// False when no pairing holds one (the credential is left as it was).
///
/// Hue usernames and Nanoleaf tokens only go to the address their pairing
/// names (where it was granted, or where the device proved itself or the
/// user confirmed it moved): a binding naming a paired id at another host
/// gets nothing. A Hue binding also takes its bridge's pinned certificate,
/// whatever it held, so no copy can bring it back to the clear.
pub fn restore(id: &str, ip: &str, kind: &mut DeviceKind, pairings: &Pairings) -> bool {
    match kind {
        DeviceKind::Hue {
            username,
            cert,
            clientkey,
            ..
        } => {
            // `hue:<bridge>:<light>`
            let bridge = id
                .strip_prefix(hue::ID_PREFIX)
                .and_then(|rest| rest.split(':').next());
            match pairings
                .hue
                .iter()
                .find(|h| Some(h.id.as_str()) == bridge && h.ip == ip)
            {
                Some(h) => {
                    username.clone_from(&h.username);
                    cert.clone_from(&h.cert);
                    clientkey.clone_from(&h.clientkey);
                }
                None => return false,
            }
        }
        DeviceKind::Nanoleaf { token } => {
            let own = id.strip_prefix(nanoleaf::ID_PREFIX);
            match pairings
                .nanoleaf
                .iter()
                .find(|n| Some(n.id.as_str()) == own && n.ip == ip)
            {
                Some(n) => token.clone_from(&n.token),
                None => return false,
            }
        }
        // A bulb re-added in Smart Life gets a new key, and maybe new DPs.
        DeviceKind::Tuya { key, dps, .. } => {
            let own = id.strip_prefix(tuya::ID_PREFIX);
            match pairings.tuya.iter().find(|t| Some(t.id.as_str()) == own) {
                Some(t) => {
                    key.clone_from(&t.key);
                    *dps = t.dps;
                }
                None => return false,
            }
        }
        _ => {}
    }
    true
}

/// Give every binding its credential back: from the pairings, else from the
/// binding in `previous` for the same device, address and family, else none.
/// A Hue or Nanoleaf binding whose device has a pairing only ever gets the
/// pairing's credential, at the pairing's address: a binding left at an old
/// address gets nothing back from `previous`.
pub fn restore_bindings(cfg: &mut AppConfig, previous: &[DeviceBinding]) {
    let AppConfig {
        bindings, pairings, ..
    } = cfg;
    for b in bindings {
        if restore(&b.device_id, &b.ip, &mut b.kind, pairings) {
            continue;
        }
        blank(&mut b.kind);
        if has_pairing(b, pairings) {
            continue;
        }
        let before = previous.iter().find(|p| {
            p.device_id == b.device_id && p.ip == b.ip && p.kind.family() == b.kind.family()
        });
        if let Some(before) = before {
            // Same family: only the credential differs, take it whole.
            let credential = before.kind.clone();
            match (&mut b.kind, credential) {
                (
                    DeviceKind::Hue {
                        username,
                        cert,
                        clientkey,
                        ..
                    },
                    DeviceKind::Hue {
                        username: u,
                        cert: c,
                        clientkey: k,
                        ..
                    },
                ) => {
                    *username = u;
                    *cert = c;
                    *clientkey = k;
                }
                (DeviceKind::Nanoleaf { token }, DeviceKind::Nanoleaf { token: t }) => *token = t,
                (DeviceKind::Tuya { key, .. }, DeviceKind::Tuya { key: k, .. }) => *key = k,
                _ => {}
            }
        }
    }
}

/// Move each Hue pairing to the address discovery reached its bridge at
/// with its username, pin the certificate a pairing older than pinning was
/// first seen with, and bring the bindings in line. True when a pairing
/// changed.
///
/// Discovery only lists a bridge's lamps away from home once its pinned
/// certificate answered there; a Nanoleaf controller never moves this way
/// (see `identity.rs`).
pub fn follow_moves(cfg: &mut AppConfig, found: &[DiscoveredDevice]) -> bool {
    let changed = move_pairings(&mut cfg.pairings, found);
    if changed {
        follow_pairings(cfg);
    }
    changed
}

/// Bring every Hue and Nanoleaf binding in line with its device's pairing:
/// the credential and pinned certificate where the binding points at the
/// pairing's address, no credential anywhere else. Bindings of devices
/// without a pairing are left as they are.
pub fn follow_pairings(cfg: &mut AppConfig) {
    let AppConfig {
        bindings, pairings, ..
    } = cfg;
    for b in bindings {
        if has_pairing(b, pairings) && !restore(&b.device_id, &b.ip, &mut b.kind, pairings) {
            blank(&mut b.kind);
        }
    }
}

/// Whether `b` is a Hue or Nanoleaf binding whose device has a pairing.
fn has_pairing(b: &DeviceBinding, pairings: &Pairings) -> bool {
    match &b.kind {
        DeviceKind::Hue { .. } => {
            let bridge = b
                .device_id
                .strip_prefix(hue::ID_PREFIX)
                .and_then(|rest| rest.split(':').next());
            pairings.hue.iter().any(|h| Some(h.id.as_str()) == bridge)
        }
        DeviceKind::Nanoleaf { .. } => {
            let own = b.device_id.strip_prefix(nanoleaf::ID_PREFIX);
            pairings.nanoleaf.iter().any(|n| Some(n.id.as_str()) == own)
        }
        _ => false,
    }
}

fn move_pairings(pairings: &mut Pairings, found: &[DiscoveredDevice]) -> bool {
    let mut changed = false;
    for d in found {
        let DeviceKind::Hue { username, cert, .. } = &d.kind else {
            continue;
        };
        if username.is_empty() {
            continue;
        }
        // `hue:<bridge>:<light>`
        let bridge =
            d.id.strip_prefix(hue::ID_PREFIX)
                .and_then(|rest| rest.split(':').next());
        let Some(h) = pairings
            .hue
            .iter_mut()
            .find(|h| Some(h.id.as_str()) == bridge)
        else {
            continue;
        };
        // A pinned certificate is never replaced from here: only a new
        // pairing or the user's confirmation does that.
        if !h.cert.is_empty() && h.cert != *cert {
            continue;
        }
        // Only a certificate pinned before vouches for a new address.
        let moved = h.ip != d.ip;
        if moved && h.cert.is_empty() {
            continue;
        }
        if h.cert.is_empty() && !cert.is_empty() {
            h.cert.clone_from(cert);
            changed = true;
        }
        if moved {
            h.ip.clone_from(&d.ip);
            changed = true;
        }
    }
    changed
}

/// Add what `imported` was granted to `pairings`, for exports made before
/// credentials were left out of them. Entries without a credential are
/// skipped, and so is any device this computer is already paired with: a
/// file, which anyone can write, never replaces a pairing made here, nor the
/// certificate pinned with it.
pub fn merge_pairings(pairings: &mut Pairings, imported: Pairings) {
    for h in imported.hue.into_iter().filter(|h| !h.username.is_empty()) {
        if !pairings.hue.iter().any(|x| x.id == h.id) {
            pairings.hue.push(h);
        }
    }
    for n in imported
        .nanoleaf
        .into_iter()
        .filter(|n| !n.token.is_empty())
    {
        if !pairings.nanoleaf.iter().any(|x| x.id == n.id) {
            pairings.nanoleaf.push(n);
        }
    }
    for t in imported.tuya.into_iter().filter(|t| !t.key.is_empty()) {
        if !pairings.tuya.iter().any(|x| x.id == t.id) {
            pairings.tuya.push(t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lueur_core::device::{HueBridge, NanoleafPairing, TuyaDevice};

    fn pairings() -> Pairings {
        Pairings {
            hue: vec![HueBridge {
                id: "b1".into(),
                ip: "192.168.1.2".into(),
                username: "hue-secret".into(),
                cert: "c1".repeat(32),
                clientkey: "hue-secret-key".into(),
            }],
            nanoleaf: vec![NanoleafPairing {
                id: "n1".into(),
                ip: "192.168.1.3".into(),
                token: "nano-secret".into(),
            }],
            tuya: vec![TuyaDevice {
                id: "t1".into(),
                name: "lamp".into(),
                key: "tuya-secret".into(),
                product: String::new(),
                dps: Default::default(),
            }],
        }
    }

    fn binding(id: &str, ip: &str, kind: DeviceKind) -> DeviceBinding {
        DeviceBinding {
            light_id: format!("l-{id}"),
            device_id: id.into(),
            ip: ip.into(),
            port: 1,
            model: String::new(),
            name: String::new(),
            kind,
        }
    }

    fn config() -> AppConfig {
        AppConfig {
            pairings: pairings(),
            bindings: vec![
                binding(
                    "hue:b1:3",
                    "192.168.1.2",
                    DeviceKind::Hue {
                        username: "hue-secret".into(),
                        light: "3".into(),
                        cert: "c1".repeat(32),
                        clientkey: "hue-secret-key".into(),
                    },
                ),
                binding(
                    "nanoleaf:n1",
                    "192.168.1.3",
                    DeviceKind::Nanoleaf {
                        token: "nano-secret".into(),
                    },
                ),
                binding(
                    "tuya:t1",
                    "192.168.1.9",
                    DeviceKind::Tuya {
                        key: "tuya-secret".into(),
                        version: "3.3".into(),
                        dps: Default::default(),
                    },
                ),
            ],
            ..AppConfig::default()
        }
    }

    #[test]
    fn redacted_config_holds_no_secret() {
        let json = redact_config(&config()).to_json().unwrap();
        assert!(!json.contains("secret"), "{json}");
    }

    #[test]
    fn redacted_bindings_come_back_whole() {
        let full = config();
        let mut back = redact_config(&full);
        back.pairings = full.pairings.clone();
        restore_bindings(&mut back, &[]);
        assert_eq!(back.bindings, full.bindings);
    }

    #[test]
    fn an_unpaired_binding_keeps_its_previous_credential() {
        let full = config();
        let mut back = redact_config(&full);
        restore_bindings(&mut back, &full.bindings);
        assert_eq!(back.bindings, full.bindings);
        // No pairing and nothing before: nothing is made up.
        let mut bare = redact_config(&full);
        restore_bindings(&mut bare, &[]);
        assert!(!bare.to_json().unwrap().contains("secret"));
    }

    #[test]
    fn credentials_stay_with_the_paired_address() {
        let previous = config().bindings;
        let mut cfg = redact_config(&config());
        cfg.pairings = pairings();
        for b in &mut cfg.bindings {
            b.ip = "192.168.1.66".into();
        }
        restore_bindings(&mut cfg, &previous);
        let json = serde_json::to_string(&cfg.bindings).unwrap();
        assert!(!json.contains("hue-secret"), "{json}");
        assert!(!json.contains("nano-secret"), "{json}");
        // A Tuya key never travels: it only encrypts, so it follows the bulb.
        assert!(json.contains("tuya-secret"));
    }

    #[test]
    fn old_exports_bring_their_pairings() {
        let mut here = Pairings::default();
        let mut imported = pairings();
        imported.hue.push(HueBridge {
            id: "b2".into(),
            ip: "192.168.1.4".into(),
            username: String::new(),
            cert: String::new(),
            clientkey: String::new(),
        });
        merge_pairings(&mut here, imported);
        assert_eq!(here, pairings());
    }

    #[test]
    fn an_import_never_replaces_a_pairing_made_here() {
        let mut here = pairings();
        let mut imported = pairings();
        imported.hue[0].username = "forged".into();
        imported.hue[0].cert = String::new();
        imported.nanoleaf[0].token = "forged".into();
        imported.tuya[0].key = "forged".into();
        merge_pairings(&mut here, imported);
        assert_eq!(here, pairings());
    }

    #[test]
    fn a_binding_takes_its_bridge_s_pinned_certificate() {
        let mut cfg = config();
        // The interface's copy dropped it: that must not bring the clear back.
        for b in &mut cfg.bindings {
            if let DeviceKind::Hue { cert, .. } = &mut b.kind {
                cert.clear();
            }
        }
        restore_bindings(&mut cfg, &[]);
        assert_eq!(cfg.bindings, config().bindings);
    }

    fn lamp(ip: &str, username: &str, cert: &str) -> DiscoveredDevice {
        DiscoveredDevice {
            id: "hue:b1:3".into(),
            ip: ip.into(),
            port: 80,
            model: String::new(),
            name: String::new(),
            support: Vec::new(),
            fw_ver: None,
            kind: DeviceKind::Hue {
                username: username.into(),
                light: "3".into(),
                cert: cert.into(),
                clientkey: String::new(),
            },
            paired: true,
            faster: false,
        }
    }

    #[test]
    fn an_older_pairing_learns_its_certificate_at_home() {
        let mut p = pairings();
        p.hue[0].cert.clear();
        let pin = "ab".repeat(32);
        assert!(move_pairings(
            &mut p,
            &[lamp("192.168.1.2", "hue-secret", &pin)]
        ));
        assert_eq!(p.hue[0].cert, pin);
        assert!(!move_pairings(
            &mut p,
            &[lamp("192.168.1.2", "hue-secret", &pin)]
        ));
    }

    #[test]
    fn only_a_pinned_bridge_follows_a_move() {
        let pin = "c1".repeat(32);
        let mut p = pairings();
        assert!(move_pairings(
            &mut p,
            &[lamp("192.168.1.77", "hue-secret", &pin)]
        ));
        assert_eq!(p.hue[0].ip, "192.168.1.77");

        // Unpinned, it never moves by itself, whatever it is listed with.
        let mut p = pairings();
        p.hue[0].cert.clear();
        assert!(!move_pairings(
            &mut p,
            &[lamp("192.168.1.77", "hue-secret", "")]
        ));
        assert!(!move_pairings(
            &mut p,
            &[lamp("192.168.1.77", "hue-secret", &pin)]
        ));
        assert_eq!(p.hue[0].ip, "192.168.1.2");
        assert!(p.hue[0].cert.is_empty());

        // Another certificate never replaces the pinned one.
        let mut p = pairings();
        assert!(!move_pairings(
            &mut p,
            &[lamp("192.168.1.2", "hue-secret", &"ee".repeat(32))]
        ));
        assert_eq!(p.hue[0].cert, pin);
        // Nor does a listing without the username move anything.
        assert!(!move_pairings(&mut p, &[lamp("192.168.1.77", "", &pin)]));
    }

    #[test]
    fn a_controller_never_follows_a_move_by_itself() {
        let mut p = pairings();
        let mut d = lamp("192.168.1.77", "", "");
        d.id = "nanoleaf:n1".into();
        d.kind = DeviceKind::Nanoleaf {
            token: "nano-secret".into(),
        };
        assert!(!move_pairings(&mut p, &[d]));
        assert_eq!(p.nanoleaf[0].ip, "192.168.1.3");
    }

    #[test]
    fn bindings_follow_their_pairing() {
        let mut cfg = config();
        // The bridge proved itself at a new address: its lamps follow.
        cfg.pairings.hue[0].ip = "192.168.1.77".into();
        cfg.bindings[0].ip = "192.168.1.77".into();
        if let DeviceKind::Hue { cert, .. } = &mut cfg.bindings[0].kind {
            cert.clear();
        }
        // The controller moved, but its binding still names the old address.
        cfg.pairings.nanoleaf[0].ip = "192.168.1.78".into();
        follow_pairings(&mut cfg);
        assert_eq!(cfg.bindings[0].kind, config().bindings[0].kind);
        assert!(!cfg.bindings[1].kind.is_ready(), "a token went elsewhere");
        // A Tuya key is not tied to an address.
        assert_eq!(cfg.bindings[2], config().bindings[2]);
    }

    #[test]
    fn a_binding_left_at_the_old_address_gets_nothing_back() {
        // The bridge was paired again at a new address; a light still bound
        // at the old one must not keep the old username from `previous`.
        let previous = config().bindings;
        let mut cfg = config();
        cfg.pairings.hue[0].ip = "192.168.1.77".into();
        cfg.pairings.hue[0].username = "new-secret".into();
        cfg.pairings.nanoleaf[0].ip = "192.168.1.78".into();
        restore_bindings(&mut cfg, &previous);
        let json = serde_json::to_string(&cfg.bindings).unwrap();
        assert!(!json.contains("hue-secret"), "{json}");
        assert!(!json.contains("new-secret"), "{json}");
        assert!(!json.contains("nano-secret"), "{json}");
        assert!(json.contains("tuya-secret"));
    }
}
