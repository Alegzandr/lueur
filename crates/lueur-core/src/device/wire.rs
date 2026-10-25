//! What a stream sends, for the trace log.
//!
//! With `LUEUR_LOG=lueur_core::device::wire=trace` every frame a stream sends
//! is logged with its family and device id: binary frames as hex, JSON and
//! text as they are. A credential that travels with a request (the Hue
//! username in its path) is masked before anything is written.
//!
//! Tracing costs nothing when it is off: each call checks the level first
//! (one atomic load), and only then formats; the hex is written straight
//! into the log line, never into a buffer of its own.

use super::Endpoint;
use std::borrow::Cow;
use std::fmt;

/// What a masked credential reads as in the log.
pub const MASK: &str = "***";

/// Who a stream's frames go to, as the trace log names them.
#[derive(Clone)]
pub struct Wire {
    family: &'static str,
    id: Box<str>,
    /// A credential that may appear in what is sent; never logged.
    secret: Box<str>,
}

impl Wire {
    pub fn new(family: &'static str, id: &str) -> Self {
        Wire {
            family,
            id: id.into(),
            secret: "".into(),
        }
    }

    /// The wire to the bulb `ep` reaches.
    pub fn of(ep: &Endpoint) -> Self {
        Wire::new(ep.kind.family(), &ep.id)
    }

    /// Mask `secret` wherever it appears in a traced request.
    pub fn hiding(mut self, secret: &str) -> Self {
        self.secret = secret.into();
        self
    }

    pub fn family(&self) -> &'static str {
        self.family
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Whether frames are traced right now. Only needed before work that
    /// exists for the trace alone; the methods below check it themselves.
    #[inline]
    pub fn enabled() -> bool {
        log::log_enabled!(log::Level::Trace)
    }

    /// A binary frame, logged as hex.
    #[inline]
    pub fn bytes(&self, frame: &[u8]) {
        log::trace!(family = self.family, id = self.id(), len = frame.len(); "sent {}", Hex(frame));
    }

    /// A JSON or text frame.
    #[inline]
    pub fn text(&self, frame: &str) {
        if Self::enabled() {
            log::trace!(family = self.family, id = self.id(); "sent {}", self.masked(frame.trim_end()));
        }
    }

    /// An HTTP request: its method, path and body.
    #[inline]
    pub fn request(&self, method: &str, path: &str, body: Option<&str>) {
        if Self::enabled() {
            log::trace!(
                family = self.family, id = self.id();
                "sent {method} {} {}", self.masked(path), self.masked(body.unwrap_or_default())
            );
        }
    }

    /// `text` with the credential, if any, replaced by [`MASK`].
    pub fn masked<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if !self.secret.is_empty() && text.contains(&*self.secret) {
            Cow::Owned(text.replace(&*self.secret, MASK))
        } else {
            Cow::Borrowed(text)
        }
    }
}

impl fmt::Debug for Wire {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Wire")
            .field("family", &self.family)
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// Bytes as lower-case hex, formatted where they are written.
pub struct Hex<'a>(pub &'a [u8]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceKind;

    #[test]
    fn bytes_print_as_hex() {
        assert_eq!(Hex(&[0x00, 0x2a, 0xff]).to_string(), "002aff");
        assert_eq!(Hex(&[]).to_string(), "");
    }

    #[test]
    fn a_credential_is_masked_wherever_it_appears() {
        let wire = Wire::new("hue", "hue:b1:3").hiding("s3cr3tUser");
        assert_eq!(
            wire.masked("/api/s3cr3tUser/lights/3/state"),
            "/api/***/lights/3/state"
        );
        assert!(matches!(wire.masked(r#"{"bri":10}"#), Cow::Borrowed(_)));
        // Without a credential, nothing is touched (an empty one would match
        // between every character).
        let open = Wire::new("wiz", "wiz:a1");
        assert_eq!(open.masked("abc"), "abc");
    }

    #[test]
    fn the_debug_form_names_the_bulb_but_not_its_credential() {
        let ep = Endpoint {
            id: "nanoleaf:aa".into(),
            ip: "192.168.1.9".into(),
            port: 16021,
            kind: DeviceKind::Nanoleaf {
                token: "tok3n".into(),
            },
        };
        let wire = Wire::of(&ep).hiding("tok3n");
        assert_eq!((wire.family(), wire.id()), ("nanoleaf", "nanoleaf:aa"));
        assert!(!format!("{wire:?}").contains("tok3n"));
    }
}
