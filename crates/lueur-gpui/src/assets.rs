//! The icons and images, built into the binary (see `build.rs`).

use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ASSETS
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ASSETS
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::new_static(p))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::View;

    /// Every glyph the interface names is there, in every weight.
    #[test]
    fn the_sidebar_glyphs_are_built_in() {
        for view in View::ALL {
            for weight in ["regular", "bold", "fill"] {
                let path = format!("icons/{}-{weight}.svg", view.icon());
                assert!(Assets.load(&path).unwrap().is_some(), "{path}");
            }
        }
        assert!(Assets.load("logo-mark.svg").unwrap().is_some());
    }
}
