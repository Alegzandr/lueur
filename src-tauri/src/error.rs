//! Errors as the interface receives them.
//!
//! Commands never send prose: they send a stable `code` the frontend turns
//! into a sentence in the user's language, plus an optional `subject` (the
//! bulb names, an address, a display number) that slots into that sentence.
//! `detail` carries the underlying OS or protocol message, untranslated, for
//! anyone who needs to dig further.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: &'static str) -> Self {
        AppError {
            code,
            subject: None,
            detail: None,
        }
    }

    pub fn subject(mut self, s: impl Into<String>) -> Self {
        self.subject = Some(s.into());
        self
    }

    pub fn detail(mut self, d: impl std::fmt::Display) -> Self {
        self.detail = Some(d.to_string());
        self
    }

    /// Something that should never happen (a poisoned lock, a lost task).
    pub fn internal(d: impl std::fmt::Display) -> Self {
        AppError::new("internal").detail(d)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)?;
        if let Some(s) = &self.subject {
            write!(f, " [{s}]")?;
        }
        if let Some(d) = &self.detail {
            write!(f, ": {d}")?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_code_and_only_present_fields() {
        let e = AppError::new("bulb_unreachable").subject("192.168.1.5");
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(
            json,
            r#"{"code":"bulb_unreachable","subject":"192.168.1.5"}"#
        );
    }
}
