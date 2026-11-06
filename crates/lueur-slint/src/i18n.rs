//! Interface strings in English and French, from `locales/*.json`.
//!
//! `t(key)` returns the string for the current language; `{name}`
//! placeholders are filled by `fill`. Counted strings are [one, other] pairs
//! read with `tn(key, n)`: French treats 0 and 1 as singular, English only 1.
//! The `.slint` files read the same strings through the `Tr` global, which
//! `install` wires to [`text`].

use lueur_core::config::Language;
use serde::Deserialize;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::LazyLock;

/// French puts a non-breaking space before : ? ! and %.
pub const NB: char = '\u{a0}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
}

impl Lang {
    /// The code the shell keeps for the tray (`service::set_ui_language`).
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
        }
    }

    /// The user's choice, or the system's when automatic.
    pub fn resolve(pref: Language) -> Self {
        match pref {
            Language::En => Lang::En,
            Language::Fr => Lang::Fr,
            Language::System => {
                let system = sys_locale::get_locale().unwrap_or_default();
                if system.to_lowercase().starts_with("fr") {
                    Lang::Fr
                } else {
                    Lang::En
                }
            }
        }
    }
}

thread_local! {
    /// The interface language, on the window's thread.
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::En) };
}

/// The interface language.
pub fn current() -> Lang {
    CURRENT.with(Cell::get)
}

/// Speak `lang` from now on: the Rust side at once, the `.slint` side as
/// its `Tr.rev` changes.
pub fn set_current(lang: Lang, ui: &crate::AppWindow) {
    use slint::ComponentHandle;
    CURRENT.with(|c| c.set(lang));
    let tr = ui.global::<crate::Tr>();
    tr.set_rev(tr.get_rev() + 1);
}

/// Wire the `Tr` global of a window to the catalogs.
pub fn install(tr: &crate::Tr<'_>) {
    tr.set_mac(cfg!(target_os = "macos"));
    tr.on_lookup(|key, _rev| text(current(), &key).into());
}

#[derive(Deserialize)]
struct Catalog {
    strings: HashMap<String, String>,
    counts: HashMap<String, [String; 2]>,
}

fn parse(json: &str) -> Catalog {
    serde_json::from_str(json).expect("locales are valid JSON (checked by the tests)")
}

static EN: LazyLock<Catalog> = LazyLock::new(|| parse(include_str!("../../../locales/en.json")));
static FR: LazyLock<Catalog> = LazyLock::new(|| parse(include_str!("../../../locales/fr.json")));

fn catalog(lang: Lang) -> &'static Catalog {
    match lang {
        Lang::En => &EN,
        Lang::Fr => &FR,
    }
}

/// The string `key` in `lang`. A key missing from the catalogs shows as
/// itself, which the tests keep from shipping.
pub fn text(lang: Lang, key: &str) -> String {
    match catalog(lang).strings.get(key) {
        Some(s) => s.clone(),
        None => {
            log::error!("no string {key}");
            debug_assert!(!cfg!(test), "no string {key}");
            key.to_string()
        }
    }
}

/// The string `key` in the interface language.
pub fn t(key: &str) -> String {
    text(current(), key)
}

/// `{name}` placeholders filled from `vars`; unknown ones are left as they
/// are.
pub fn fill(s: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                match vars.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(v),
                    None => out.push_str(&rest[open..open + close + 2]),
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The counted string `key` for `n`, in `lang`.
pub fn counted(lang: Lang, key: &str, n: usize) -> String {
    let Some([one, other]) = catalog(lang).counts.get(key) else {
        debug_assert!(false, "no counted string {key}");
        return key.to_string();
    };
    let singular = match lang {
        Lang::Fr => n <= 1,
        Lang::En => n == 1,
    };
    fill(if singular { one } else { other }, &[("n", &n.to_string())])
}

/// [`counted`] in the interface language.
pub fn tn(key: &str, n: usize) -> String {
    counted(current(), key, n)
}

/// "45 %" in French, "45%" in English.
pub fn pct(lang: Lang, v: f32) -> String {
    let n = (v * 100.0).round() as i32;
    match lang {
        Lang::Fr => format!("{n}{NB}%"),
        Lang::En => format!("{n}%"),
    }
}

/// An error from the shell as a sentence in `lang`: its code's string with
/// the subject filled in.
pub fn error_text(lang: Lang, e: &lueur_shell::error::AppError) -> String {
    let key = format!("e_{}", e.code);
    let template = catalog(lang)
        .strings
        .get(&key)
        .or_else(|| catalog(lang).strings.get("e_internal"))
        .map(String::as_str)
        .unwrap_or(e.code);
    fill(template, &[("subject", e.subject.as_deref().unwrap_or(""))])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(s: &str) -> Vec<String> {
        let mut found: Vec<String> = s
            .split('{')
            .skip(1)
            .filter_map(|p| p.split_once('}').map(|(name, _)| name.to_string()))
            .filter(|n| n.chars().all(|c| c.is_alphanumeric() || c == '_'))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn both_languages_have_every_string_with_the_same_placeholders() {
        let mut keys: Vec<_> = EN.strings.keys().collect();
        keys.sort();
        let mut fr: Vec<_> = FR.strings.keys().collect();
        fr.sort();
        assert_eq!(keys, fr);
        for key in keys {
            let (en, fr) = (&EN.strings[key], &FR.strings[key]);
            assert!(!en.trim().is_empty() && !fr.trim().is_empty(), "{key}");
            assert_eq!(placeholders(en), placeholders(fr), "{key}");
        }
        let mut counts: Vec<_> = EN.counts.keys().collect();
        counts.sort();
        let mut fr: Vec<_> = FR.counts.keys().collect();
        fr.sort();
        assert_eq!(counts, fr);
    }

    #[test]
    fn french_counts_zero_and_one_as_singular() {
        assert_eq!(counted(Lang::Fr, "n_lights", 0), "0 lumière");
        assert_eq!(counted(Lang::En, "n_lights", 0), "0 lights");
        assert_eq!(counted(Lang::En, "n_lights", 1), "1 light");
        assert_eq!(counted(Lang::Fr, "n_lights", 2), "2 lumières");
    }

    #[test]
    fn placeholders_are_filled_and_unknown_ones_kept() {
        assert_eq!(fill("{a} and {b}", &[("a", "x")]), "x and {b}");
        assert_eq!(fill("no { end", &[]), "no { end");
    }

    #[test]
    fn errors_read_as_sentences() {
        let e = lueur_shell::error::AppError::new("no_such_code_ever");
        assert_eq!(error_text(Lang::En, &e), EN.strings["e_internal"]);
        assert_eq!(pct(Lang::Fr, 0.45), "45\u{a0}%");
    }
}
