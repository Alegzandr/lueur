//! The room's photographs (`assets/modes`), decoded once and kept, as Slint
//! cannot read WebP itself, and their blurred copies for the backdrop.

use std::cell::RefCell;
use std::collections::HashMap;

include!(concat!(env!("OUT_DIR"), "/photos.rs"));

thread_local! {
    static CACHE: RefCell<HashMap<(String, u32), slint::Image>> = RefCell::new(HashMap::new());
}

/// The photograph `name` (`screen`, `relax`, `off`…), or an empty picture
/// if there is none by that name.
pub fn photo(name: &str) -> slint::Image {
    blurred(name, 0)
}

/// The photograph `name`, `sigma` pixels out of focus (0 for sharp).
pub fn blurred(name: &str, sigma: u32) -> slint::Image {
    let key = (name.to_string(), sigma);
    if let Some(image) = CACHE.with_borrow(|c| c.get(&key).cloned()) {
        return image;
    }
    let image = decode(name, sigma).unwrap_or_default();
    CACHE.with_borrow_mut(|c| c.insert(key, image.clone()));
    image
}

fn decode(name: &str, sigma: u32) -> Option<slint::Image> {
    let (_, bytes) = PHOTOS.iter().find(|(n, _)| *n == name)?;
    let mut rgba = image::load_from_memory(bytes).ok()?.into_rgba8();
    if sigma > 0 {
        // Shrunk first: a blur of that size loses nothing a full-size copy
        // would keep, and costs a fraction of it.
        let (w, h) = rgba.dimensions();
        let small = image::imageops::resize(&rgba, w / 4, h / 4, image::imageops::FilterType::Triangle);
        rgba = image::imageops::blur(&small, sigma as f32 / 4.0);
    }
    let (w, h) = rgba.dimensions();
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(rgba.as_raw(), w, h);
    Some(slint::Image::from_rgba8(buffer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_photograph_decodes() {
        for (name, _) in PHOTOS {
            assert!(decode(name, 0).is_some(), "{name}");
        }
        assert!(PHOTOS.iter().any(|(n, _)| *n == "screen"));
    }
}
