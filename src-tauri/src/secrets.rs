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
        DeviceKind::Hue { username, .. } => username.clear(),
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

/// Fill in `kind`'s credential for device `id` from the pairings. False when
/// no pairing holds one (the credential is left as it was).
pub fn restore(id: &str, kind: &mut DeviceKind, pairings: &Pairings) -> bool {
    match kind {
        DeviceKind::Hue { username, .. } => {
            // `hue:<bridge>:<light>`
            let bridge = id
                .strip_prefix(hue::ID_PREFIX)
                .and_then(|rest| rest.split(':').next());
            match pairings.hue.iter().find(|h| Some(h.id.as_str()) == bridge) {
                Some(h) => username.clone_from(&h.username),
                None => return false,
            }
        }
        DeviceKind::Nanoleaf { token } => {
            let own = id.strip_prefix(nanoleaf::ID_PREFIX);
            match pairings
                .nanoleaf
                .iter()
                .find(|n| Some(n.id.as_str()) == own)
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
/// binding in `previous` for the same device and family, else none.
pub fn restore_bindings(cfg: &mut AppConfig, previous: &[DeviceBinding]) {
    let AppConfig {
        bindings, pairings, ..
    } = cfg;
    for b in bindings {
        if restore(&b.device_id, &mut b.kind, pairings) {
            continue;
        }
        blank(&mut b.kind);
        let before = previous
            .iter()
            .find(|p| p.device_id == b.device_id && p.kind.family() == b.kind.family());
        if let Some(before) = before {
            // Same family: only the credential differs, take it whole.
            let credential = before.kind.clone();
            match (&mut b.kind, credential) {
                (DeviceKind::Hue { username, .. }, DeviceKind::Hue { username: u, .. }) => {
                    *username = u;
                }
                (DeviceKind::Nanoleaf { token }, DeviceKind::Nanoleaf { token: t }) => *token = t,
                (DeviceKind::Tuya { key, .. }, DeviceKind::Tuya { key: k, .. }) => *key = k,
                _ => {}
            }
        }
    }
}

/// Add what `imported` was granted to `pairings`, for exports made before
/// credentials were left out of them. Entries without a credential are
/// skipped; an imported one replaces this computer's for the same device.
pub fn merge_pairings(pairings: &mut Pairings, imported: Pairings) {
    for h in imported.hue.into_iter().filter(|h| !h.username.is_empty()) {
        pairings.hue.retain(|x| x.id != h.id);
        pairings.hue.push(h);
    }
    for n in imported
        .nanoleaf
        .into_iter()
        .filter(|n| !n.token.is_empty())
    {
        pairings.nanoleaf.retain(|x| x.id != n.id);
        pairings.nanoleaf.push(n);
    }
    for t in imported.tuya.into_iter().filter(|t| !t.key.is_empty()) {
        pairings.tuya.retain(|x| x.id != t.id);
        pairings.tuya.push(t);
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

    fn binding(id: &str, kind: DeviceKind) -> DeviceBinding {
        DeviceBinding {
            light_id: format!("l-{id}"),
            device_id: id.into(),
            ip: "192.168.1.9".into(),
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
                    DeviceKind::Hue {
                        username: "hue-secret".into(),
                        light: "3".into(),
                    },
                ),
                binding(
                    "nanoleaf:n1",
                    DeviceKind::Nanoleaf {
                        token: "nano-secret".into(),
                    },
                ),
                binding(
                    "tuya:t1",
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
    fn old_exports_bring_their_pairings() {
        let mut here = Pairings::default();
        let mut imported = pairings();
        imported.hue.push(HueBridge {
            id: "b2".into(),
            ip: "192.168.1.4".into(),
            username: String::new(),
        });
        merge_pairings(&mut here, imported);
        assert_eq!(here, pairings());
    }
}
