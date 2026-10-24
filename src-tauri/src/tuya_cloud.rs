//! The one-off Smart Life sign-in that imports a user's Tuya bulbs.
//!
//! Tuya bulbs only obey clients that know their *local key*, which the
//! Smart Life app holds and never shows. Its "scan to log in" flow hands it
//! over: Lueur shows a QR code, the user scans it in the Smart Life app and
//! approves, and Lueur receives a short-lived session with which it lists the
//! user's devices and their keys. The session is then ended and discarded;
//! everything after that is local (see `tuya.rs`).
//!
//! This follows Tuya's own device-sharing SDK (`tuya-device-sharing-sdk`, the
//! one Home Assistant uses): every request is signed with HMAC-SHA256, and
//! query, body and result travel AES-GCM sealed under a per-request secret.
//!
//! It is the only cloud traffic Lueur makes besides the update check, and it
//! happens only while the user is signing in.

use aes_gcm::Aes128Gcm;
use aes_gcm::aead::{Aead, KeyInit};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use hmac::{Hmac, Mac};
use lueur_core::device::TuyaDevice;
use lueur_core::device::tuya::TuyaDps;
use md5::{Digest, Md5};
use serde_json::{Value, json};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The partner identity the QR sign-in is issued under.
///
/// This is Home Assistant's, which Tuya's SDK documents for personal use.
/// A public release of Lueur must register its own with Tuya and put it here.
const CLIENT_ID: &str = "HA_3y9q4ak7g4ephrvke";
const SCHEMA: &str = "haauthorize";
const LOGIN_HOST: &str = "https://apigw.iotbing.com";

/// Domains the regional API host the sign-in hands back may belong to.
/// Anything else is refused, so a tampered answer can never point Lueur's
/// requests elsewhere.
const API_DOMAINS: &[&str] = &[
    "tuyaeu.com",
    "tuyaus.com",
    "tuyacn.com",
    "tuyain.com",
    "iotbing.com",
];

/// `https://<host>` with a host under one of [`API_DOMAINS`], and nothing
/// else: no port, path, credentials or query.
pub fn valid_endpoint(url: &str) -> bool {
    let Some(host) = url.strip_prefix("https://") else {
        return false;
    };
    let clean = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    clean
        && API_DOMAINS
            .iter()
            .any(|d| host.strip_suffix(d).is_some_and(|rest| rest.ends_with('.')))
}

const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum CloudError {
    /// The network or Tuya's servers did not answer.
    Unreachable(String),
    /// Tuya answered with an error (a bad user code, an expired QR…).
    Refused(String),
}

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudError::Unreachable(e) => write!(f, "Smart Life unreachable: {e}"),
            CloudError::Refused(e) => write!(f, "Smart Life refused: {e}"),
        }
    }
}

type Result<T> = std::result::Result<T, CloudError>;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .https_only(true)
        .build()
        .into()
}

fn read_json(
    resp: std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<Value> {
    resp.map_err(|e| CloudError::Unreachable(e.to_string()))?
        .body_mut()
        .read_json::<Value>()
        .map_err(|e| CloudError::Unreachable(e.to_string()))
}

/// Tuya's envelope: `{"success": true, "result": …}` or a code and message.
fn result_of(v: Value) -> Result<Value> {
    if v.get("success").and_then(Value::as_bool) == Some(true) {
        Ok(v)
    } else {
        let code = v.get("code").map(Value::to_string).unwrap_or_default();
        let msg = v.get("msg").and_then(Value::as_str).unwrap_or("");
        Err(CloudError::Refused(
            format!("{code} {msg}").trim().to_string(),
        ))
    }
}

/// User codes and QR tokens are short alphanumerics; anything else is refused
/// before it reaches a URL.
pub fn valid_code(s: &str) -> bool {
    (4..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// A sign-in waiting for its QR code to be scanned.
pub struct Pending {
    pub token: String,
    /// What the QR code encodes.
    pub qr: String,
}

/// Ask for a sign-in QR code for the account with `user_code` (shown in the
/// Smart Life app under Me → Settings → Account and Security → User Code).
pub fn start_login(user_code: &str) -> Result<Pending> {
    let url = format!("{LOGIN_HOST}/v1.0/m/life/home-assistant/qrcode/tokens");
    let v = result_of(read_json(
        agent()
            .post(&url)
            .query("clientid", CLIENT_ID)
            .query("usercode", user_code)
            .query("schema", SCHEMA)
            .send_empty(),
    )?)?;
    let token = v
        .pointer("/result/qrcode")
        .and_then(Value::as_str)
        .filter(|t| valid_code(t))
        .ok_or_else(|| CloudError::Refused("no QR token".into()))?
        .to_string();
    Ok(Pending {
        qr: format!("tuyaSmart--qrLogin?token={token}"),
        token,
    })
}

/// A signed-in session, good for the few requests the import needs.
pub struct Session {
    endpoint: String,
    access_token: String,
    refresh_token: String,
    terminal_id: String,
}

/// Whether the QR code has been scanned and approved yet.
pub fn poll_login(token: &str, user_code: &str) -> Result<Option<Session>> {
    let url = format!("{LOGIN_HOST}/v1.0/m/life/home-assistant/qrcode/tokens/{token}");
    let v = read_json(
        agent()
            .get(&url)
            .query("clientid", CLIENT_ID)
            .query("usercode", user_code)
            .call(),
    )?;
    // Until the scan, Tuya answers with an unsuccessful envelope.
    let Ok(v) = result_of(v) else {
        return Ok(None);
    };
    let field = |k: &str| {
        v.pointer(&format!("/result/{k}"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let endpoint = field("endpoint").trim_end_matches('/').to_string();
    if !valid_endpoint(&endpoint) {
        return Err(CloudError::Refused(format!(
            "unexpected API host {endpoint}"
        )));
    }
    Ok(Some(Session {
        endpoint,
        access_token: field("access_token"),
        refresh_token: field("refresh_token"),
        terminal_id: field("terminal_id"),
    }))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    // HMAC takes keys of any length, so this cannot fail.
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("any key length");
    mac.update(msg);
    mac.finalize().into_bytes().to_vec()
}

/// The 12-character nonce alphabet the SDK draws from.
const NONCE_CHARS: &[u8] = b"ABCDEFGHJKMNPQRSTWXYZabcdefhijkmnprstwxyz2345678";

fn seal(plain: &str, secret: &[u8]) -> Result<String> {
    let mut raw = [0u8; 12];
    getrandom::fill(&mut raw).map_err(|e| CloudError::Unreachable(e.to_string()))?;
    let nonce: Vec<u8> = raw
        .iter()
        .map(|b| NONCE_CHARS[usize::from(*b) % NONCE_CHARS.len()])
        .collect();
    let cipher =
        Aes128Gcm::new_from_slice(secret).map_err(|e| CloudError::Refused(e.to_string()))?;
    let nonce_arr = aes_gcm::Nonce::try_from(nonce.as_slice())
        .map_err(|_| CloudError::Refused("nonce".into()))?;
    let sealed = cipher
        .encrypt(&nonce_arr, plain.as_bytes())
        .map_err(|e| CloudError::Refused(e.to_string()))?;
    Ok(format!("{}{}", B64.encode(&nonce), B64.encode(sealed)))
}

fn open(sealed: &str, secret: &[u8]) -> Result<String> {
    let bad = |what: &str| CloudError::Refused(format!("unreadable answer ({what})"));
    let raw = B64.decode(sealed).map_err(|_| bad("base64"))?;
    if raw.len() < 12 {
        return Err(bad("short"));
    }
    let (nonce, body) = raw.split_at(12);
    let cipher = Aes128Gcm::new_from_slice(secret).map_err(|_| bad("key"))?;
    let nonce = aes_gcm::Nonce::try_from(nonce).map_err(|_| bad("nonce"))?;
    let plain = cipher.decrypt(&nonce, body).map_err(|_| bad("seal"))?;
    String::from_utf8(plain).map_err(|_| bad("utf-8"))
}

impl Session {
    /// One signed, sealed request; returns the unsealed `result`.
    fn request(
        &self,
        method: &str,
        path: &str,
        query: Option<Value>,
        body: Option<Value>,
    ) -> Result<Value> {
        let rid = uuid::Uuid::new_v4().to_string();
        let hash_key = hex(&Md5::digest(format!("{rid}{}", self.refresh_token)));
        let secret = hex(&hmac_sha256(rid.as_bytes(), hash_key.as_bytes()));
        let secret = &secret.as_bytes()[..16];

        let query_enc = match &query {
            Some(q) => seal(&q.to_string(), secret)?,
            None => String::new(),
        };
        let body_enc = match &body {
            Some(b) => seal(&b.to_string(), secret)?,
            None => String::new(),
        };

        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
            .to_string();
        let headers = [
            ("X-appKey", CLIENT_ID),
            ("X-requestId", rid.as_str()),
            ("X-sid", ""),
            ("X-time", time.as_str()),
            ("X-token", self.access_token.as_str()),
        ];
        let mut to_sign = headers
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("||");
        to_sign.push_str(&query_enc);
        to_sign.push_str(&body_enc);
        let sign = hex(&hmac_sha256(hash_key.as_bytes(), to_sign.as_bytes()));

        let url = format!("{}{path}", self.endpoint);
        let agent = agent();
        let resp = match method {
            "POST" => {
                let mut req = agent.post(&url);
                for (k, v) in headers {
                    req = req.header(k, v);
                }
                req = req.header("X-sign", &sign);
                if !query_enc.is_empty() {
                    req = req.query("encdata", &query_enc);
                }
                if body_enc.is_empty() {
                    req.send_empty()
                } else {
                    req.send_json(json!({ "encdata": body_enc }))
                }
            }
            _ => {
                let mut req = agent.get(&url);
                for (k, v) in headers {
                    req = req.header(k, v);
                }
                req = req.header("X-sign", &sign);
                if !query_enc.is_empty() {
                    req = req.query("encdata", &query_enc);
                }
                req.call()
            }
        };
        let v = result_of(read_json(resp)?)?;
        match v.get("result") {
            Some(Value::String(sealed)) => {
                let plain = open(sealed, secret)?;
                Ok(serde_json::from_str(&plain).unwrap_or(Value::String(plain)))
            }
            Some(other) => Ok(other.clone()),
            None => Ok(Value::Null),
        }
    }

    fn get(&self, path: &str, query: Option<Value>) -> Result<Value> {
        self.request("GET", path, query, None)
    }

    /// Every colour bulb in the user's homes that can be driven locally.
    pub fn bulbs(&self) -> Result<Vec<TuyaDevice>> {
        let homes = self.get("/v1.0/m/life/users/homes", None)?;
        let mut out: Vec<TuyaDevice> = Vec::new();
        for home in homes.as_array().into_iter().flatten() {
            let Some(home_id) = home.get("ownerId").map(id_string) else {
                continue;
            };
            let devices = self.get(
                "/v1.0/m/life/ha/home/devices",
                Some(json!({ "homeId": home_id })),
            )?;
            for d in devices.as_array().into_iter().flatten() {
                if let Some(bulb) = self.bulb(d)?
                    && !out.iter().any(|b| b.id == bulb.id)
                {
                    out.push(bulb);
                }
            }
        }
        Ok(out)
    }

    fn bulb(&self, d: &Value) -> Result<Option<TuyaDevice>> {
        let text = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let (id, key) = (text("id"), text("local_key"));
        // Sub-devices sit behind a gateway and have no LAN address of their own.
        let sub = d.get("sub").and_then(Value::as_bool).unwrap_or(false);
        if id.is_empty() || key.is_empty() || sub || !valid_code(&id) {
            return Ok(None);
        }
        // DP numbers come from the local strategy, keyed by status code.
        let status = self.get(&format!("/v1.0/m/life/devices/{id}/status"), None)?;
        let mut codes: BTreeMap<u16, String> = BTreeMap::new();
        for rel in status
            .get("dpStatusRelationDTOS")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if rel.get("supportLocal").and_then(Value::as_bool) == Some(false) {
                return Ok(None);
            }
            let dp = rel
                .get("dpId")
                .and_then(Value::as_u64)
                .and_then(|n| u16::try_from(n).ok());
            let code = rel.get("statusCode").and_then(Value::as_str);
            if let (Some(dp), Some(code)) = (dp, code) {
                codes.insert(dp, code.to_string());
            }
        }
        let Some(dps) = TuyaDps::from_codes(&codes) else {
            return Ok(None);
        };
        Ok(Some(TuyaDevice {
            id,
            name: text("name"),
            key,
            product: text("product_name"),
            dps,
        }))
    }

    /// End the session on Tuya's side; Lueur keeps no token.
    pub fn end(self) {
        let body = json!({ "accessToken": self.access_token, "terminalId": self.terminal_id });
        let _ = self.request("POST", "/v1.0/m/token/terminal/expire", None, Some(body));
    }
}

fn id_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The QR code as an SVG the interface can show as is.
pub fn qr_svg(content: &str) -> Option<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(content.as_bytes()).ok()?;
    Some(
        code.render::<svg::Color>()
            .min_dimensions(200, 200)
            .quiet_zone(true)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_then_open_round_trips() {
        let secret = b"0123456789abcdef";
        let sealed = seal(r#"{"homeId":"42"}"#, secret).unwrap();
        // 12 nonce bytes are 16 base64 characters, then the ciphertext.
        assert!(sealed.len() > 16);
        let nonce = B64.decode(&sealed[..16]).unwrap();
        assert!(nonce.iter().all(|b| NONCE_CHARS.contains(b)));
        // The answer format is base64(nonce ‖ ciphertext) as one string.
        let mut joined = nonce;
        joined.extend(B64.decode(&sealed[16..]).unwrap());
        assert_eq!(
            open(&B64.encode(joined), secret).unwrap(),
            r#"{"homeId":"42"}"#
        );
    }

    #[test]
    fn codes_are_checked() {
        assert!(valid_code("CavB15p"));
        assert!(!valid_code("ab"));
        assert!(!valid_code("abc/../def"));
        assert!(!valid_code("abc&x=1"));
    }

    #[test]
    fn only_tuya_hosts_are_trusted() {
        assert!(valid_endpoint("https://apigw.tuyaeu.com"));
        assert!(valid_endpoint("https://apigw-weaz.tuyaeu.com"));
        assert!(!valid_endpoint("http://apigw.tuyaeu.com"));
        assert!(!valid_endpoint("https://tuyaeu.com.evil.example"));
        assert!(!valid_endpoint("https://eviltuyaeu.com"));
        assert!(!valid_endpoint("https://apigw.tuyaeu.com/x"));
        assert!(!valid_endpoint("https://user@apigw.tuyaeu.com"));
        assert!(!valid_endpoint("https://apigw.tuyaeu.com:444"));
    }

    #[test]
    fn hmac_matches_a_known_vector() {
        // RFC 4231 test case 2.
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn qr_renders_as_svg() {
        let svg = qr_svg("tuyaSmart--qrLogin?token=abc").unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));
    }
}
