//! What a bulb is called where its light is shown, after `src/lib/brands.ts`:
//! never its address, which only appears to tell two alike apart.

use lueur_core::config::DeviceBinding;
use lueur_core::device::DiscoveredDevice;

/// A family's brand name, the same in every language.
pub fn family_name(family: &str) -> &'static str {
    match family {
        "yeelight" => "Yeelight",
        "wiz" => "WiZ",
        "lifx" => "LIFX",
        "govee" => "Govee",
        "hue" => "Philips Hue",
        "nanoleaf" => "Nanoleaf",
        "tuya" => "Smart Life",
        _ => "",
    }
}

/// The brand, then the model when the bulb reports one.
fn brand(family: &str, model: &str) -> String {
    [family_name(family), model]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A found bulb's brand and model, as its row describes it.
pub fn device_brand(d: &DiscoveredDevice) -> String {
    brand(d.kind.family(), &d.model)
}

/// A bound bulb as its light describes it: its own name when it says more
/// than the light's, then its brand and model.
pub fn bulb_label(b: &DeviceBinding, light_name: Option<&str>) -> String {
    let brand = brand(b.kind.family(), &b.model);
    if !b.name.is_empty() && Some(b.name.as_str()) != light_name {
        format!("{} · {brand}", b.name)
    } else {
        brand
    }
}

/// Only Yeelight reports what it can do; every other family can stream.
pub fn supports_music(d: &DiscoveredDevice) -> bool {
    if d.kind.family() != "yeelight" {
        return true;
    }
    let has = |m: &str| d.support.iter().any(|s| s == m);
    has("set_music") && has("set_rgb")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lueur_core::device::DeviceKind;

    fn binding(name: &str, model: &str, kind: DeviceKind) -> DeviceBinding {
        DeviceBinding {
            light_id: "a".into(),
            device_id: "x".into(),
            ip: "192.168.1.2".into(),
            port: 55443,
            model: model.into(),
            name: name.into(),
            kind,
        }
    }

    #[test]
    fn a_bulb_is_named_by_its_name_and_brand_never_its_address() {
        let b = binding("Desk", "color", DeviceKind::Yeelight);
        assert_eq!(bulb_label(&b, Some("Lamp")), "Desk · Yeelight color");
        // The same name as its light says nothing more.
        assert_eq!(bulb_label(&b, Some("Desk")), "Yeelight color");
        let b = binding("", "", DeviceKind::Govee);
        assert_eq!(bulb_label(&b, None), "Govee");
        assert!(!bulb_label(&b, None).contains("192"));
    }
}
