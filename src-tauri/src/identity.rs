//! Paired bridges and controllers heard where Lueur cannot vouch for them.
//!
//! Discovery hands a Hue username or a Nanoleaf token only to the address
//! its pairing names, or to a Hue bridge that proved itself elsewhere with
//! its pinned certificate (see `lueur_core::device::Approach`). A paired
//! device heard anywhere else, or at home with another certificate, gets
//! nothing: it is listed unpaired, and kept here until the user says
//! whether it is theirs. For a device with nothing pinned (a Nanoleaf
//! controller, a bridge without HTTPS), their word moves the pairing to the
//! new address. A bridge whose certificate is pinned and that shows another
//! one is an impostor or a reset bridge: no word of the user's can vouch
//! for it, only pairing it again with its button, after forgetting the old
//! pairing.

use crate::error::{AppError, AppResult};
use crate::lights::validate_ip;
use crate::state::AppState;
use lueur_core::device::{DeviceKind, DiscoveredDevice, Pairings, hue, nanoleaf};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

/// Event the frontend listens to, with every [`Unverified`] as payload.
pub const DEVICE_UNVERIFIED: &str = "device-unverified";

/// A paired device heard where nothing proves it is the one paired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unverified {
    /// Its device id, `hue:<bridge>` or `nanoleaf:<controller>`.
    pub id: String,
    pub family: &'static str,
    pub name: String,
    /// Where it was heard.
    pub ip: String,
    /// Where it was paired.
    pub known_ip: String,
    /// Heard at its own address, but with another certificate.
    pub changed: bool,
    /// A bridge with a pinned certificate that showed another (or none):
    /// it can only be paired again, never confirmed.
    pub needs_pairing: bool,
    /// The certificate it showed, pinned if the user confirms. Kept from the
    /// interface, which has no say in it.
    #[serde(skip)]
    pub cert: String,
}

/// The paired devices `found` lists without their credential where nothing
/// vouches for them: away from their pairing's address, or with another
/// certificate than the pinned one.
pub fn unverified(pairings: &Pairings, found: &[DiscoveredDevice]) -> Vec<Unverified> {
    found
        .iter()
        .filter(|d| !d.paired)
        .filter_map(|d| {
            let (known_ip, changed, needs_pairing, cert) = match &d.kind {
                DeviceKind::Hue { cert, .. } => {
                    let own = d.id.strip_prefix(hue::ID_PREFIX)?;
                    let p = pairings.hue.iter().find(|h| h.id == own)?;
                    let other = !p.cert.is_empty() && *cert != p.cert;
                    (p.ip.clone(), d.ip == p.ip && other, other, cert.clone())
                }
                DeviceKind::Nanoleaf { .. } => {
                    let own = d.id.strip_prefix(nanoleaf::ID_PREFIX)?;
                    let p = pairings.nanoleaf.iter().find(|n| n.id == own)?;
                    (p.ip.clone(), false, false, String::new())
                }
                _ => return None,
            };
            (d.ip != known_ip || changed).then(|| Unverified {
                id: d.id.clone(),
                family: d.kind.family(),
                name: d.name.clone(),
                ip: d.ip.clone(),
                known_ip,
                changed,
                needs_pairing,
                cert,
            })
        })
        .collect()
}

/// The user said `u` is theirs: its pairing moves to where it was heard,
/// with the certificate it showed there. A bridge whose certificate is
/// pinned and that showed another, or none, is refused: it can only be an
/// impostor or a reset bridge, and must be paired again.
pub fn confirm(pairings: &mut Pairings, u: &Unverified) -> AppResult<()> {
    let missing = || AppError::new("device_not_pending");
    match u.family {
        "hue" => {
            let own = u.id.strip_prefix(hue::ID_PREFIX).ok_or_else(missing)?;
            let h = pairings
                .hue
                .iter_mut()
                .find(|h| h.id == own)
                .ok_or_else(missing)?;
            if !h.cert.is_empty() && u.cert != h.cert {
                return Err(AppError::new("confirm_needs_pairing"));
            }
            h.ip.clone_from(&u.ip);
            if !u.cert.is_empty() {
                h.cert.clone_from(&u.cert);
            }
        }
        "nanoleaf" => {
            let own = u.id.strip_prefix(nanoleaf::ID_PREFIX).ok_or_else(missing)?;
            let n = pairings
                .nanoleaf
                .iter_mut()
                .find(|n| n.id == own)
                .ok_or_else(missing)?;
            n.ip.clone_from(&u.ip);
        }
        _ => return Err(missing()),
    }
    Ok(())
}

/// Drop the pairing of bridge `u`, whose pinned certificate it no longer
/// shows, so it can be paired again from scratch. Only for such a bridge:
/// any other pairing stays.
pub fn forget(pairings: &mut Pairings, u: &Unverified) -> AppResult<()> {
    let own = (u.family == "hue" && u.needs_pairing)
        .then(|| u.id.strip_prefix(hue::ID_PREFIX))
        .flatten()
        .ok_or_else(|| AppError::new("device_not_pending"))?;
    let before = pairings.hue.len();
    pairings
        .hue
        .retain(|h| h.id != own || h.cert.is_empty() || h.cert == u.cert);
    if pairings.hue.len() == before {
        return Err(AppError::new("device_not_pending"));
    }
    Ok(())
}

/// Keep what the search that found `found` could not vouch for, and tell
/// the interface when there is something to ask.
pub fn note(app: &AppHandle, found: &[DiscoveredDevice]) -> AppResult<()> {
    let state = app.state::<AppState>();
    let list = unverified(&state.lock_config()?.pairings, found);
    *state.unverified.lock().map_err(|_| AppError::poisoned())? = list.clone();
    if !list.is_empty() {
        let _ = app.emit(DEVICE_UNVERIFIED, list);
    }
    Ok(())
}

/// The devices waiting for the user's word, from the last search.
#[tauri::command]
pub fn unverified_devices(state: State<'_, AppState>) -> AppResult<Vec<Unverified>> {
    Ok(state
        .unverified
        .lock()
        .map_err(|_| AppError::poisoned())?
        .clone())
}

/// The user says the device `id` the last search could not vouch for is
/// theirs. Only one that search listed can be confirmed, at the address and
/// with the certificate it was heard with: the interface names it, nothing
/// more.
#[tauri::command]
pub fn confirm_device(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let u = state
        .unverified
        .lock()
        .map_err(|_| AppError::poisoned())?
        .iter()
        .find(|u| u.id == id)
        .cloned()
        .ok_or_else(|| AppError::new("device_not_pending"))?;
    validate_ip(&u.ip)?;
    {
        let mut cfg = state.lock_config()?;
        confirm(&mut cfg.pairings, &u)?;
        // Lights bound at the old address lose the credential; the next
        // search moves them, and they get it back there.
        crate::secrets::follow_pairings(&mut cfg);
    }
    state
        .unverified
        .lock()
        .map_err(|_| AppError::poisoned())?
        .retain(|x| x.id != id);
    state.persist()
}

/// Forget the pairing of the bridge `id` the last search found showing
/// another certificate than the pinned one, so the user can pair it again
/// with its button (a bridge they reset). Its lights lose their username.
#[tauri::command]
pub fn forget_device(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let u = state
        .unverified
        .lock()
        .map_err(|_| AppError::poisoned())?
        .iter()
        .find(|u| u.id == id)
        .cloned()
        .ok_or_else(|| AppError::new("device_not_pending"))?;
    {
        let mut cfg = state.lock_config()?;
        forget(&mut cfg.pairings, &u)?;
        crate::secrets::follow_pairings(&mut cfg);
    }
    state
        .unverified
        .lock()
        .map_err(|_| AppError::poisoned())?
        .retain(|x| x.id != id);
    state.persist()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lueur_core::device::{HueBridge, NanoleafPairing};

    const PIN: &str = "c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1";
    const OTHER: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

    fn pairings(cert: &str) -> Pairings {
        Pairings {
            hue: vec![HueBridge {
                id: "b1".into(),
                ip: "192.168.1.2".into(),
                username: "hue-secret".into(),
                cert: cert.into(),
                clientkey: String::new(),
            }],
            nanoleaf: vec![NanoleafPairing {
                id: "n1".into(),
                ip: "192.168.1.3".into(),
                token: "nano-secret".into(),
            }],
            tuya: Vec::new(),
        }
    }

    fn heard(id: &str, ip: &str, kind: DeviceKind, paired: bool) -> DiscoveredDevice {
        DiscoveredDevice {
            id: id.into(),
            ip: ip.into(),
            port: 80,
            model: String::new(),
            name: "Hue Bridge".into(),
            support: Vec::new(),
            fw_ver: None,
            kind,
            paired,
            faster: false,
        }
    }

    fn bridge(ip: &str, cert: &str) -> DiscoveredDevice {
        let kind = DeviceKind::Hue {
            username: String::new(),
            light: String::new(),
            cert: cert.into(),
            clientkey: String::new(),
        };
        heard("hue:b1", ip, kind, false)
    }

    fn controller(ip: &str) -> DiscoveredDevice {
        let kind = DeviceKind::Nanoleaf {
            token: String::new(),
        };
        heard("nanoleaf:n1", ip, kind, false)
    }

    #[test]
    fn a_paired_device_heard_elsewhere_is_asked_about() {
        let p = pairings("");
        let list = unverified(
            &p,
            &[bridge("192.168.1.66", ""), controller("192.168.1.67")],
        );
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].known_ip, "192.168.1.2");
        assert_eq!(list[0].ip, "192.168.1.66");
        assert!(!list[0].changed);
        assert_eq!(list[1].family, "nanoleaf");
        assert!(!serde_json::to_string(&list).unwrap().contains("secret"));
    }

    #[test]
    fn another_certificate_at_home_is_asked_about() {
        let list = unverified(&pairings(PIN), &[bridge("192.168.1.2", OTHER)]);
        assert_eq!(list.len(), 1);
        assert!(list[0].changed);
        assert!(!serde_json::to_string(&list).unwrap().contains(OTHER));
    }

    #[test]
    fn nothing_is_asked_about_what_is_where_it_should_be() {
        // At home with its own certificate, but without lamps (it forgot
        // us), or never paired: pairing is offered, not a question.
        assert!(unverified(&pairings(PIN), &[bridge("192.168.1.2", PIN)]).is_empty());
        assert!(unverified(&pairings(""), &[bridge("192.168.1.2", PIN)]).is_empty());
        assert!(unverified(&pairings(""), &[controller("192.168.1.3")]).is_empty());
        let mut stranger = bridge("192.168.1.66", "");
        stranger.id = "hue:b9".into();
        assert!(unverified(&pairings(""), &[stranger]).is_empty());
        // A lamp listed with its username was reached as it should be.
        let lamp = heard(
            "hue:b1:3",
            "192.168.1.66",
            DeviceKind::Hue {
                username: String::new(),
                light: "3".into(),
                cert: PIN.into(),
                clientkey: String::new(),
            },
            true,
        );
        assert!(unverified(&pairings(PIN), &[lamp]).is_empty());
    }

    #[test]
    fn confirming_moves_the_pairing_and_pins_what_was_seen() {
        let mut p = pairings("");
        let u = unverified(&p, &[bridge("192.168.1.66", OTHER)]).remove(0);
        confirm(&mut p, &u).unwrap();
        assert_eq!(p.hue[0].ip, "192.168.1.66");
        assert_eq!(p.hue[0].cert, OTHER);
        assert_eq!(p.hue[0].username, "hue-secret");

        let u = unverified(&p, &[controller("192.168.1.67")]).remove(0);
        confirm(&mut p, &u).unwrap();
        assert_eq!(p.nanoleaf[0].ip, "192.168.1.67");
    }

    #[test]
    fn confirming_never_brings_a_pinned_bridge_back_to_the_clear() {
        let mut p = pairings(PIN);
        let u = unverified(&p, &[bridge("192.168.1.66", "")]).remove(0);
        let err = confirm(&mut p, &u).unwrap_err();
        assert_eq!(err.code, "confirm_needs_pairing");
        assert_eq!(p, pairings(PIN));
    }

    #[test]
    fn only_a_paired_device_can_be_confirmed() {
        let mut p = pairings("");
        let mut u = unverified(&p, &[bridge("192.168.1.66", "")]).remove(0);
        u.id = "hue:b9".into();
        assert_eq!(confirm(&mut p, &u).unwrap_err().code, "device_not_pending");
        u.family = "tuya";
        assert_eq!(confirm(&mut p, &u).unwrap_err().code, "device_not_pending");
        assert_eq!(p, pairings(""));
    }

    #[test]
    fn a_pinned_bridge_with_another_certificate_is_never_confirmed() {
        for ip in ["192.168.1.2", "192.168.1.66"] {
            let mut p = pairings(PIN);
            let u = unverified(&p, &[bridge(ip, OTHER)]).remove(0);
            assert!(u.needs_pairing, "{ip}");
            assert_eq!(
                confirm(&mut p, &u).unwrap_err().code,
                "confirm_needs_pairing"
            );
            assert_eq!(p, pairings(PIN));
        }
        // Moved with its own certificate (reached, but without lamps): fine.
        let mut p = pairings(PIN);
        let u = unverified(&p, &[bridge("192.168.1.66", PIN)]).remove(0);
        assert!(!u.needs_pairing);
        confirm(&mut p, &u).unwrap();
        assert_eq!(p.hue[0].ip, "192.168.1.66");
    }

    #[test]
    fn only_a_bridge_that_lost_its_certificate_can_be_forgotten() {
        let mut p = pairings(PIN);
        let u = unverified(&p, &[bridge("192.168.1.2", OTHER)]).remove(0);
        forget(&mut p, &u).unwrap();
        assert!(p.hue.is_empty());
        assert_eq!(p.nanoleaf.len(), 1);

        let mut p = pairings("");
        let u = unverified(&p, &[bridge("192.168.1.66", OTHER)]).remove(0);
        assert_eq!(forget(&mut p, &u).unwrap_err().code, "device_not_pending");
        let u = unverified(&p, &[controller("192.168.1.67")]).remove(0);
        assert_eq!(forget(&mut p, &u).unwrap_err().code, "device_not_pending");
        assert_eq!(p, pairings(""));
    }
}
