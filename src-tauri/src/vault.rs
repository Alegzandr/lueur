//! Credentials at rest.
//!
//! What bridges and Smart Life granted is kept out of `config.json`: it is
//! sealed (AES-256-GCM) in `credentials.sealed` beside it, under a random key
//! the OS keeps in its own keyring (Windows Credential Manager, the macOS
//! Keychain, the Secret Service on Linux). A copy of the data folder, a
//! backup or another program reading files as the user gets the room but no
//! key to the bulbs.
//!
//! Without a keyring (a Linux session with no Secret Service, a keyring that
//! will not open) the credentials stay in `config.json` as they always did,
//! readable by this user only, and move into the vault at the first save
//! once the keyring answers again. A sealed file whose key is gone (the data
//! folder copied to another computer) is set aside: those bridges are paired
//! and Smart Life imported again, as after an import.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use lueur_core::config::{AppConfig, DeviceBinding};
use lueur_core::device::{DeviceKind, Pairings};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

const SERVICE: &str = "com.lueur.app";
/// Tests keep their own key, away from the app's.
const ACCOUNT: &str = if cfg!(test) {
    "credentials-key-test"
} else {
    "credentials-key"
};
const FILE_NAME: &str = "credentials.sealed";
const NONCE_LEN: usize = 12;

/// What is sealed: every credential, with what it belongs to.
#[derive(Default, Serialize, Deserialize)]
struct Sealed {
    #[serde(default)]
    pairings: Pairings,
    #[serde(default)]
    bindings: Vec<DeviceBinding>,
}

/// Where the credentials are kept this session.
pub enum Vault {
    /// The keyring answers. The key is made at the first save.
    Ready {
        key: Option<[u8; 32]>,
        /// What the sealed file holds, as written: a slider drag saves the
        /// config many times a second, the credentials almost never change.
        written: Option<Vec<u8>>,
        /// The last save could not seal the credentials, which then stayed
        /// in the config.
        failed: bool,
    },
    /// No keyring: the credentials stay in the config.
    Unavailable,
}

fn entry() -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(SERVICE, ACCOUNT)
}

/// The credential string a binding's kind carries, if its family has one.
fn credential_mut(kind: &mut DeviceKind) -> Option<&mut String> {
    match kind {
        DeviceKind::Hue { username, .. } => Some(username),
        DeviceKind::Nanoleaf { token } => Some(token),
        DeviceKind::Tuya { key, .. } => Some(key),
        _ => None,
    }
}

/// Fill in every credential `cfg` lacks from `sealed`. One `cfg` holds is
/// newer (written while the keyring was away) and stays.
///
/// Where a credential comes from the sealed file, so does where it goes:
/// the address and pinned certificate beside it in `config.json` are only a
/// copy anyone with access to the file could have edited, so the sealed
/// ones win.
fn fill(cfg: &mut AppConfig, sealed: Sealed) {
    let p = &sealed.pairings;
    for h in &mut cfg.pairings.hue {
        if let Some(s) = p.hue.iter().find(|x| x.id == h.id)
            && h.username.is_empty()
            && !s.username.is_empty()
        {
            *h = s.clone();
        }
    }
    for n in &mut cfg.pairings.nanoleaf {
        if let Some(s) = p.nanoleaf.iter().find(|x| x.id == n.id)
            && n.token.is_empty()
            && !s.token.is_empty()
        {
            *n = s.clone();
        }
    }
    for t in &mut cfg.pairings.tuya {
        if let Some(s) = p.tuya.iter().find(|x| x.id == t.id)
            && t.key.is_empty()
        {
            t.key.clone_from(&s.key);
        }
    }
    for b in &mut cfg.bindings {
        let Some(s) = sealed.bindings.iter().find(|s| {
            s.light_id == b.light_id
                && s.device_id == b.device_id
                && s.kind.family() == b.kind.family()
        }) else {
            continue;
        };
        let empty = credential_mut(&mut b.kind).is_some_and(|c| c.is_empty());
        let held = credential_mut(&mut s.kind.clone()).is_some_and(|c| !c.is_empty());
        if empty && held {
            b.ip.clone_from(&s.ip);
            b.port = s.port;
            b.kind = s.kind.clone();
        }
    }
}

/// `cfg` without a single credential, its pairings still listed.
fn strip(cfg: &AppConfig) -> AppConfig {
    let mut cfg = cfg.clone();
    let p = &mut cfg.pairings;
    p.hue.iter_mut().for_each(|h| {
        h.username.clear();
        h.clientkey.clear();
    });
    p.nanoleaf.iter_mut().for_each(|n| n.token.clear());
    p.tuya.iter_mut().for_each(|t| t.key.clear());
    for b in &mut cfg.bindings {
        if let Some(c) = credential_mut(&mut b.kind) {
            c.clear();
        }
        if let DeviceKind::Hue { clientkey, .. } = &mut b.kind {
            clientkey.clear();
        }
    }
    cfg
}

/// Does `cfg` hold any credential?
pub fn holds_credentials(cfg: &AppConfig) -> bool {
    let p = &cfg.pairings;
    p.hue.iter().any(|h| !h.username.is_empty())
        || p.nanoleaf.iter().any(|n| !n.token.is_empty())
        || p.tuya.iter().any(|t| !t.key.is_empty())
        || cfg
            .bindings
            .iter()
            .any(|b| credential_mut(&mut b.kind.clone()).is_some_and(|c| !c.is_empty()))
}

fn seal_with(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
    use aes_gcm::Aes256Gcm;
    use aes_gcm::aead::{Aead, KeyInit};
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let nonce_arr = aes_gcm::Nonce::try_from(nonce.as_slice()).map_err(|_| "nonce")?;
    let body = cipher
        .encrypt(&nonce_arr, plain)
        .map_err(|e| e.to_string())?;
    Ok([nonce.as_slice(), &body].concat())
}

fn open_with(key: &[u8; 32], sealed: &[u8]) -> Result<Vec<u8>, String> {
    use aes_gcm::Aes256Gcm;
    use aes_gcm::aead::{Aead, KeyInit};
    if sealed.len() < NONCE_LEN {
        return Err("too short".into());
    }
    let (nonce, body) = sealed.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let nonce = aes_gcm::Nonce::try_from(nonce).map_err(|_| "nonce")?;
    cipher
        .decrypt(&nonce, body)
        .map_err(|_| "wrong key, or altered".into())
}

/// Set an unreadable sealed file aside rather than overwrite it.
fn set_aside(path: &Path, why: &str) {
    log::error!("credentials unreadable ({why}); set aside");
    let _ = std::fs::rename(path, path.with_extension("sealed.bad"));
}

impl Vault {
    /// Whether the credentials are sealed this session, rather than kept in
    /// `config.json` for want of a keyring.
    pub fn is_sealing(&self) -> bool {
        matches!(self, Vault::Ready { failed: false, .. })
    }

    /// Open the vault for `data_dir` and give `cfg` back its credentials.
    pub fn open(data_dir: &Path, cfg: &mut AppConfig) -> Vault {
        let path = data_dir.join(FILE_NAME);
        let key = match entry().and_then(|e| e.get_secret()) {
            Ok(raw) => match <[u8; 32]>::try_from(raw.as_slice()) {
                Ok(key) => Some(key),
                Err(_) => {
                    log::warn!("keyring holds no usable key; making another");
                    None
                }
            },
            Err(keyring::Error::NoEntry) => None,
            Err(e) => {
                log::warn!("keyring unavailable ({e}); credentials stay in the config");
                return Vault::Unavailable;
            }
        };
        match (std::fs::read(&path), key) {
            (Ok(sealed), Some(key)) => {
                let plain = open_with(&key, B64.decode(&sealed).unwrap_or_default().as_slice())
                    .and_then(|p| serde_json::from_slice::<Sealed>(&p).map_err(|e| e.to_string()));
                match plain {
                    Ok(sealed) => fill(cfg, sealed),
                    Err(e) => set_aside(&path, &e),
                }
            }
            (Ok(_), None) => set_aside(&path, "its key is gone"),
            (Err(_), _) => {}
        }
        Vault::Ready {
            key,
            written: None,
            failed: false,
        }
    }

    /// What to write to `config.json` for `cfg`: without its credentials once
    /// they are sealed beside it, else `cfg` itself, so none is ever lost.
    pub fn seal(&mut self, data_dir: &Path, cfg: &AppConfig) -> AppConfig {
        let Vault::Ready {
            key: slot,
            written,
            failed,
        } = self
        else {
            return cfg.clone();
        };
        let key = match slot {
            Some(key) => *key,
            None => {
                let mut key = [0u8; 32];
                let made = getrandom::fill(&mut key)
                    .map_err(|e| e.to_string())
                    .and_then(|()| {
                        entry()
                            .and_then(|e| e.set_secret(&key))
                            .map_err(|e| e.to_string())
                    });
                if let Err(e) = made {
                    log::warn!("cannot store a key in the keyring ({e})");
                    *self = Vault::Unavailable;
                    return cfg.clone();
                }
                *slot = Some(key);
                key
            }
        };
        let sealed = Sealed {
            pairings: cfg.pairings.clone(),
            bindings: cfg.bindings.clone(),
        };
        let plain = match serde_json::to_vec(&sealed) {
            Ok(plain) => plain,
            Err(e) => {
                log::warn!("cannot seal the credentials ({e}); kept in the config");
                *failed = true;
                return cfg.clone();
            }
        };
        if written.as_ref() == Some(&plain) {
            return strip(cfg);
        }
        let sealed = seal_with(&key, &plain).and_then(|body| {
            write(data_dir, B64.encode(body).as_bytes()).map_err(|e| e.to_string())
        });
        match sealed {
            Ok(()) => {
                *written = Some(plain);
                *failed = false;
                strip(cfg)
            }
            Err(e) => {
                log::warn!("cannot seal the credentials ({e}); kept in the config");
                *failed = true;
                cfg.clone()
            }
        }
    }
}

/// Write the sealed file the way the config is written: whole, on disk, then
/// in place, readable by this user only.
fn write(data_dir: &Path, body: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let path = data_dir.join(FILE_NAME);
    let tmp = data_dir.join(format!("{FILE_NAME}.tmp"));
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(&tmp)
        .and_then(|mut f| f.write_all(body).and_then(|()| f.sync_all()))?;
    std::fs::rename(&tmp, &path)
}

/// Whether the credentials are encrypted on disk this session. Settings
/// says so when they are not, for want of a keyring.
#[tauri::command]
pub fn credentials_sealed(state: tauri::State<'_, crate::state::AppState>) -> bool {
    state.credentials_sealed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lueur_core::device::{HueBridge, TuyaDevice};

    fn config() -> AppConfig {
        AppConfig {
            pairings: Pairings {
                hue: vec![HueBridge {
                    id: "b1".into(),
                    ip: "192.168.1.2".into(),
                    username: "hue-secret".into(),
                    cert: String::new(),
                    clientkey: "hue-secret-key".into(),
                }],
                nanoleaf: Vec::new(),
                tuya: vec![TuyaDevice {
                    id: "t1".into(),
                    name: "lamp".into(),
                    key: "tuya-secret".into(),
                    product: String::new(),
                    dps: Default::default(),
                }],
            },
            bindings: vec![DeviceBinding {
                light_id: "l1".into(),
                device_id: "hue:b1:3".into(),
                ip: "192.168.1.2".into(),
                port: 80,
                model: String::new(),
                name: String::new(),
                kind: DeviceKind::Hue {
                    username: "hue-secret".into(),
                    light: "3".into(),
                    cert: String::new(),
                    clientkey: "hue-secret-key".into(),
                },
            }],
            ..AppConfig::default()
        }
    }

    #[test]
    fn stripped_config_holds_nothing_and_comes_back_whole() {
        let full = config();
        let mut bare = strip(&full);
        assert!(!holds_credentials(&bare));
        assert!(!bare.to_json().unwrap().contains("secret"));
        assert_eq!(
            bare.pairings.hue[0].ip, "192.168.1.2",
            "pairings stay listed"
        );
        fill(
            &mut bare,
            Sealed {
                pairings: full.pairings.clone(),
                bindings: full.bindings.clone(),
            },
        );
        assert_eq!(bare, full);
    }

    #[test]
    fn a_newer_credential_in_the_config_stays() {
        let mut cfg = config();
        cfg.pairings.hue[0].username = "newer".into();
        fill(
            &mut cfg,
            Sealed {
                pairings: config().pairings,
                bindings: config().bindings,
            },
        );
        assert_eq!(cfg.pairings.hue[0].username, "newer");
    }

    #[test]
    fn a_credential_goes_where_the_sealed_file_says() {
        // config.json edited behind Lueur's back: another address, no pin.
        let mut bare = strip(&config());
        bare.pairings.hue[0].ip = "192.168.1.66".into();
        bare.pairings.hue[0].cert.clear();
        bare.bindings[0].ip = "192.168.1.66".into();
        let mut sealed = config();
        sealed.pairings.hue[0].cert = "ab".repeat(32);
        if let DeviceKind::Hue { cert, .. } = &mut sealed.bindings[0].kind {
            *cert = "ab".repeat(32);
        }
        fill(
            &mut bare,
            Sealed {
                pairings: sealed.pairings.clone(),
                bindings: sealed.bindings.clone(),
            },
        );
        assert_eq!(bare.pairings, sealed.pairings);
        assert_eq!(bare.bindings, sealed.bindings);
    }

    #[test]
    fn a_failed_seal_is_not_reported_as_sealed() {
        // A file where the data folder should be: nothing can be written.
        let dir = std::env::temp_dir().join(format!("lueur-vault-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::write(&dir, b"not a folder").unwrap();
        let mut vault = Vault::Ready {
            key: Some([7u8; 32]),
            written: None,
            failed: false,
        };
        assert!(vault.is_sealing());
        let on_disk = vault.seal(&dir, &config());
        assert!(holds_credentials(&on_disk), "kept in the config");
        assert!(!vault.is_sealing());
        let _ = std::fs::remove_file(&dir);
    }

    /// The whole round trip through the OS keyring. Touches the real one
    /// (under a test account it removes after), so it only runs on request:
    /// `cargo test -p lueur vault_round_trip -- --ignored`
    #[test]
    #[ignore]
    fn vault_round_trip() {
        let dir = std::env::temp_dir().join(format!("lueur-vault-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = entry().and_then(|e| e.delete_credential());

        let full = config();
        let mut first = full.clone();
        let mut vault = Vault::open(&dir, &mut first);
        assert!(matches!(vault, Vault::Ready { key: None, .. }));
        let on_disk = vault.seal(&dir, &full);
        assert!(
            !holds_credentials(&on_disk),
            "credentials left in the config"
        );
        let file = std::fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        assert!(!file.contains("secret"));
        // A second save with nothing new rewrites nothing.
        let stamp = std::fs::metadata(dir.join(FILE_NAME))
            .unwrap()
            .modified()
            .unwrap();
        vault.seal(&dir, &full);
        let again = std::fs::metadata(dir.join(FILE_NAME))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(stamp, again);

        // The next launch reads them back from the stripped config.
        let mut back = on_disk.clone();
        Vault::open(&dir, &mut back);
        assert_eq!(back, full);

        // The key gone (another computer): the file is set aside, not trusted.
        entry().unwrap().delete_credential().unwrap();
        let mut lost = on_disk;
        Vault::open(&dir, &mut lost);
        assert!(!holds_credentials(&lost));
        assert!(dir.join("credentials.sealed.bad").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sealing_needs_the_key() {
        let key = [7u8; 32];
        let sealed = seal_with(&key, b"hue-secret").unwrap();
        assert!(!sealed.windows(10).any(|w| w == b"hue-secret"));
        assert_eq!(open_with(&key, &sealed).unwrap(), b"hue-secret");
        assert!(open_with(&[8u8; 32], &sealed).is_err());
        let mut altered = sealed.clone();
        *altered.last_mut().unwrap() ^= 1;
        assert!(open_with(&key, &altered).is_err());
        assert!(open_with(&key, b"short").is_err());
    }
}
