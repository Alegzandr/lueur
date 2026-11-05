//! Signs Lueur's update files the way every installed copy checks them.
//!
//! The Tauri versions of Lueur (up to 0.18) and this one verify an update
//! with the same minisign key and the same rules (see
//! `crates/lueur-shell/src/update.rs`): a prehashed minisign signature,
//! base64 encoded as a whole, whose signed (trusted) comment names the
//! version it was made for. The Tauri CLI used to write them; this writes
//! the very same thing, from the same secret key, so nothing installed can
//! tell the difference.
//!
//! The secret key is the one the Tauri CLI generated: a minisign secret key
//! box (`rsign encrypted secret key`), base64 encoded, as the release's
//! secret `TAURI_SIGNING_PRIVATE_KEY` holds it.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use minisign::{SecretKeyBox, sign};
use std::io::Cursor;

/// The untrusted comment the Tauri CLI wrote. Nothing checks it; kept so
/// signatures read the same as before.
const UNTRUSTED_COMMENT: &str = "signature from tauri secret key";

/// What the trusted comment says: when, which file, and for which version.
/// Tab separated, as the Tauri CLI writes it and the updater reads it.
pub fn trusted_comment(timestamp: u64, file_name: &str, version: &str) -> String {
    format!("timestamp:{timestamp}\tfile:{file_name}\tversion:{version}")
}

/// Open the secret key: base64 of the key box, then the box with its
/// password (empty for a key made without one).
pub fn secret_key(key_b64: &str, password: &str) -> Result<minisign::SecretKey, String> {
    let text = B64
        .decode(key_b64.trim())
        .map_err(|e| format!("the key is not base64: {e}"))?;
    let text = String::from_utf8(text).map_err(|e| format!("the key is not text: {e}"))?;
    let key_box = SecretKeyBox::from_string(&text).map_err(|e| format!("not a key: {e}"))?;
    // A password is always passed: `None` would ask for one on a terminal,
    // and the release has none.
    key_box
        .into_secret_key(Some(password.to_string()))
        .map_err(|e| format!("cannot open the key (wrong password?): {e}"))
}

/// The signature of `data`, as the `.sig` file next to the update holds it.
pub fn sign_update(
    key: &minisign::SecretKey,
    data: &[u8],
    file_name: &str,
    version: &str,
    timestamp: u64,
) -> Result<String, String> {
    let comment = trusted_comment(timestamp, file_name, version);
    let signature = sign(
        None,
        key,
        Cursor::new(data),
        Some(&comment),
        Some(UNTRUSTED_COMMENT),
    )
    .map_err(|e| format!("cannot sign: {e}"))?;
    Ok(B64.encode(signature.into_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A throwaway key made by the Tauri CLI (`tauri signer generate -p
    // test-pass`), so these tests prove this crate opens the CLI's keys,
    // as it must open the release's.
    pub const TEST_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHJzaWduIGVuY3J5cHRlZCBzZWNyZXQga2V5ClJXUlRZMEl5OHluRHBvM0hkRWszcEw2SS9yaWhHbTdXU2t1TEFabTkwQk03UG1ZR0dha0FBQkFBQUFBQUFBQUFBQUlBQUFBQWVDdnkrUXlpVjJZOGNnYnl4WnVjZ2l0YndIYmRuK3RML25EeGc1T3paK3Jaczc4OWwzU0sydGoxM1ZyNHZGNjRLK3RteTNwR05iNHJzb1hjK1B6elB1WXFuV2VhLy9LT21GWmJCSGVVdit6WUNTdHMwWWE3UXd0aXRHV3I5bWRZaUxaSCtIUkFTdFU9Cg==";
    const TEST_PASSWORD: &str = "test-pass";

    fn verify(sig_b64: &str, data: &[u8], pubkey_b64: &str) -> minisign_verify::Signature {
        let pubkey = String::from_utf8(B64.decode(pubkey_b64).unwrap()).unwrap();
        let pk = minisign_verify::PublicKey::decode(&pubkey).unwrap();
        let text = String::from_utf8(B64.decode(sig_b64).unwrap()).unwrap();
        let sig = minisign_verify::Signature::decode(&text).unwrap();
        // As the updater checks it.
        pk.verify(data, &sig, true).unwrap();
        sig
    }

    #[test]
    fn the_comment_reads_as_the_tauri_cli_wrote_it() {
        assert_eq!(
            trusted_comment(1793870370, "Lueur_0.19.0_windows_x64_setup.exe", "0.19.0"),
            "timestamp:1793870370\tfile:Lueur_0.19.0_windows_x64_setup.exe\tversion:0.19.0"
        );
    }

    #[test]
    fn a_tauri_key_signs_what_its_public_key_verifies() {
        let key = secret_key(TEST_KEY, TEST_PASSWORD).unwrap();
        let data = b"Lueur update test payload\n";
        let sig = sign_update(&key, data, "payload.bin", "0.19.0", 1).unwrap();
        let pk = B64.encode(
            minisign::PublicKey::from_secret_key(&key)
                .unwrap()
                .to_box()
                .unwrap()
                .into_string(),
        );
        let sig = verify(&sig, data, &pk);
        assert_eq!(
            sig.trusted_comment(),
            "timestamp:1\tfile:payload.bin\tversion:0.19.0"
        );
    }

    #[test]
    fn the_wrong_password_is_refused() {
        assert!(secret_key(TEST_KEY, "not it").is_err());
        assert!(secret_key("not base64!", TEST_PASSWORD).is_err());
    }
}
