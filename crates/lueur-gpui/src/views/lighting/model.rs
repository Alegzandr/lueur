//! What the Lighting page knows about the lights, after
//! `src/lib/state/derived.ts`, `src/lib/scenes.ts` and `lighting/modes.ts`:
//! what each light does, which lights the settings go to, the moods, the
//! shows and the room's photographs. Plain functions over the config and
//! what plays, so they can be tested without a window.

use crate::i18n::{self, Lang};
use crate::state::Lueur;
use lueur_core::config::{AppConfig, SavedScene, SceneLight};
use lueur_core::scene::{LightState, MOODS, NATURAL, Tint};
use lueur_shell::service::EffectStatus;

/// The animated modes, in the order they are offered: the screen, then the
/// shows (`SHOWS` in scenes.ts).
pub const ANIMATED: [&str; 6] = ["ambilight", "music", "rainbow", "sunset", "ocean", "candle"];

/// The shows whose pace can be chosen (`has_speed` in show/mod.rs).
pub const PACED: [&str; 3] = ["rainbow", "sunset", "ocean"];

/// The whites the Warmth slider spans.
pub const MIN_KELVIN: u16 = 2200;
pub const MAX_KELVIN: u16 = 6500;

/// Saturated enough to read as a choice, spread evenly round the wheel,
/// with the key of each one's name.
pub const PALETTE: [([u8; 3], &str); 9] = [
    ([255, 59, 48], "colour_red"),
    ([255, 122, 0], "colour_orange"),
    ([255, 184, 0], "colour_yellow"),
    ([52, 199, 89], "colour_green"),
    ([0, 199, 190], "colour_teal"),
    ([10, 132, 255], "colour_blue"),
    ([94, 92, 230], "colour_indigo"),
    ([191, 90, 242], "colour_purple"),
    ([255, 45, 146], "colour_pink"),
];

/// The three feels of the ambilight, by their smoothing (`presets.ts`).
pub const FEELS: [(&str, f32); 3] = [("instant", 0.9), ("balanced", 0.55), ("calm", 0.22)];

/// The feel the current settings match, if they were not tuned by hand.
pub fn current_feel(c: &AppConfig) -> Option<&'static str> {
    if c.transition.effect != lueur_core::yeelight::Effect::Smooth {
        return None;
    }
    FEELS
        .iter()
        .find(|(_, s)| (c.engine.smoothing - s).abs() < 0.005)
        .map(|(name, _)| *name)
}

pub fn apply_feel(c: &mut AppConfig, feel: &str) {
    if let Some((_, smoothing)) = FEELS.iter().find(|(name, _)| *name == feel) {
        c.engine.smoothing = *smoothing;
        c.transition.effect = lueur_core::yeelight::Effect::Smooth;
    }
}

/// How many times faster than its natural pace a show plays at `speed`
/// (`ShowSettings::pace` in show/mod.rs).
pub fn show_pace(speed: f32) -> f32 {
    4f32.powf((speed.clamp(1.0, 100.0) - 50.0) / 50.0)
}

/// A pace as "×1.5", with a decimal comma in French.
pub fn times(lang: Lang, v: f32) -> String {
    let scale = if v < 1.0 { 100.0 } else { 10.0 };
    let n = (v * scale).round() / scale;
    let n = format!("{n}");
    match lang {
        Lang::Fr => format!("×{}", n.replace('.', ",")),
        Lang::En => format!("×{n}"),
    }
}

/// The colour a white of `kelvin` looks like (Tanner Helland's fit).
pub fn kelvin_rgb(kelvin: u16) -> [u8; 3] {
    let t = f64::from(kelvin) / 100.0;
    let clamp = |v: f64| v.clamp(0.0, 255.0).round() as u8;
    let r = if t <= 66.0 {
        255.0
    } else {
        329.698_727_446 * (t - 60.0).powf(-0.133_204_759_2)
    };
    let g = if t <= 66.0 {
        99.470_802_586_1 * t.ln() - 161.119_568_166_1
    } else {
        288.122_169_528_3 * (t - 60.0).powf(-0.075_514_849_2)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_731_223_1 * (t - 10.0).ln() - 305.044_792_730_7
    };
    [clamp(r), clamp(g), clamp(b)]
}

/// The colour a light set by hand gives.
pub fn tint_rgb(tint: Tint) -> [u8; 3] {
    match tint {
        Tint::White { kelvin } => kelvin_rgb(kelvin),
        Tint::Color { r, g, b } => [r, g, b],
    }
}

/// A colour of full chroma at `hue` degrees, for the custom colour.
pub fn hue_rgb(hue: f32) -> [u8; 3] {
    let h = (hue.rem_euclid(360.0)) / 60.0;
    let x = 1.0 - ((h % 2.0) - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    [r, g, b].map(|c: f32| (c * 255.0).round() as u8)
}

/// The hue of a colour, in degrees.
pub fn rgb_hue([r, g, b]: [u8; 3]) -> f32 {
    let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    if d == 0.0 {
        return 0.0;
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h * 60.0
}

/// A mode or mood's photograph, by the path the assets give it.
pub fn picture_of(id: &str) -> &'static str {
    match id {
        "ambilight" | "screen" => "modes/screen.webp",
        "music" => "modes/music.webp",
        "rainbow" => "modes/rainbow.webp",
        "sunset" => "modes/sunset.webp",
        "ocean" => "modes/ocean.webp",
        "candle" => "modes/candle.webp",
        "natural" => "modes/natural.webp",
        "daylight" => "modes/daylight.webp",
        "reading" => "modes/reading.webp",
        "relax" => "modes/relax.webp",
        "night" => "modes/night.webp",
        "custom" => "modes/custom.webp",
        _ => "modes/off.webp",
    }
}

/// What a light does now.
#[derive(Debug, Clone, PartialEq)]
pub enum Doing {
    /// Plays the ambilight or a show.
    Mode(String),
    /// Shows a mood, the natural light among them.
    Mood(String),
    /// Shows a white or a colour set by hand.
    Hand(LightState),
    Off,
}

impl Doing {
    /// What it is, in a few words.
    pub fn name(&self, lang: Lang) -> String {
        let t = |key: &str| i18n::text(lang, key).to_string();
        match self {
            Doing::Mode(m) => t(&format!("mode_{m}")),
            Doing::Mood(m) => t(&format!("mood_{m}")),
            Doing::Off => t("light_off"),
            Doing::Hand(s) => match s.tint {
                Tint::White { kelvin } => format!("{} {kelvin} K", t("white")),
                Tint::Color { .. } => t("tint_colour"),
            },
        }
    }

    /// The same thing done, so lights doing it read as one.
    fn same(&self, other: &Doing) -> bool {
        match (self, other) {
            (Doing::Hand(a), Doing::Hand(b)) => a.tint == b.tint,
            _ => self == other,
        }
    }
}

/// How a light looks in the room: its photograph and, when set by hand,
/// the colour laid over it. `key` is the same for lights that look alike.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub key: String,
    pub picture: &'static str,
    pub tint: Option<[u8; 3]>,
}

impl Look {
    pub fn of(d: &Doing) -> Look {
        match d {
            Doing::Mode(m) => Look {
                key: m.clone(),
                picture: picture_of(m),
                tint: None,
            },
            Doing::Mood(m) => Look {
                key: m.clone(),
                picture: picture_of(m),
                tint: None,
            },
            Doing::Off => Look {
                key: "off".into(),
                picture: picture_of("off"),
                tint: None,
            },
            Doing::Hand(s) => {
                let rgb = tint_rgb(s.tint);
                Look {
                    key: format!("custom {rgb:?}"),
                    picture: picture_of("custom"),
                    tint: Some(rgb),
                }
            }
        }
    }

    pub fn of_scene(s: &SceneLight) -> Look {
        match s {
            SceneLight::Mode { mode } => Look::of(&Doing::Mode(mode.clone())),
            SceneLight::Natural => Look::of(&Doing::Mood(NATURAL.into())),
            SceneLight::Hand { state } if state.on => Look::of(&Doing::Hand(*state)),
            SceneLight::Hand { .. } => Look::of(&Doing::Off),
        }
    }
}

/// The looks of a group of lights, each once, in the order first met.
pub fn distinct_looks(looks: impl IntoIterator<Item = Look>, max: usize) -> Vec<Look> {
    let mut out: Vec<Look> = Vec::new();
    for l in looks {
        if !out.iter().any(|o| o.key == l.key) {
            out.push(l);
        }
    }
    out.truncate(max);
    out
}

/// What the page reads: the config, what plays, and the lights picked.
#[derive(Clone, Copy)]
pub struct Room<'a> {
    pub config: &'a AppConfig,
    pub status: &'a EffectStatus,
    pub target: &'a [String],
}

impl<'a> Room<'a> {
    pub fn of(l: &'a Lueur) -> Self {
        Room {
            config: &l.config,
            status: &l.status,
            target: &l.target,
        }
    }

    pub fn bound(&self, id: &str) -> bool {
        self.config.binding_for(id).is_some()
    }

    pub fn name_of(&self, id: &str) -> String {
        self.config
            .room
            .lights
            .iter()
            .find(|l| l.id == id)
            .map_or_else(|| id.to_string(), |l| l.name.clone())
    }

    /// What a light was last set to; a light nobody has set reads as a warm
    /// white, on.
    pub fn state(&self, id: &str) -> LightState {
        self.config.states.get(id).copied().unwrap_or_default()
    }

    /// True while the running ambilight or show sets this light's colour.
    pub fn driven(&self, id: &str) -> bool {
        self.status.running && self.status.light_ids.iter().any(|l| l == id)
    }

    /// A light an effect drives is on, whatever it was last set to by hand.
    pub fn on(&self, id: &str) -> bool {
        self.driven(id) || self.state(id).on
    }

    /// The mode a light plays, or None while it is set by hand.
    pub fn mode_of(&self, id: &str) -> Option<&'a str> {
        self.status
            .groups
            .iter()
            .find(|g| g.light_ids.iter().any(|l| l == id))
            .map(|g| g.mode)
    }

    pub fn reconnecting(&self, id: &str) -> bool {
        self.status.reconnecting.iter().any(|l| l == id)
    }

    /// What else drives this light's bulb: another computer running Lueur
    /// (its name), another app (an empty string), or nothing.
    pub fn elsewhere(&self, id: &str) -> Option<String> {
        self.status
            .elsewhere
            .iter()
            .find(|e| e.light_id == id)
            .map(|e| e.by.clone().unwrap_or_default())
    }

    /// The mood a light shows now, if it shows one: the natural light while
    /// it follows it, or the mood its state is.
    pub fn mood_of(&self, id: &str) -> Option<&'static str> {
        if self.driven(id) {
            return None;
        }
        if self.config.natural.contains(id) {
            return Some(NATURAL);
        }
        let s = self.config.states.get(id)?;
        MOODS.iter().find(|m| *s == m.state()).map(|m| m.id)
    }

    pub fn doing_of(&self, id: &str) -> Doing {
        if let Some(m) = self.mode_of(id) {
            return Doing::Mode(m.to_string());
        }
        let s = self.state(id);
        if !s.on {
            return Doing::Off;
        }
        match self.mood_of(id) {
            Some(m) => Doing::Mood(m.to_string()),
            None => Doing::Hand(s),
        }
    }

    /// How bright a light is, 1..=100: as set by hand, or the brightness of
    /// the effect it plays, which every light playing it shares.
    pub fn bright_of(&self, id: &str) -> u8 {
        match self.mode_of(id) {
            Some("ambilight") => (self.config.engine.brightness * 100.0).round() as u8,
            Some(_) => self.config.show.bright,
            None => self.state(id).bright,
        }
    }

    /// The lights picked that still have a bulb, or None for the whole room.
    pub fn picked(&self) -> Option<Vec<String>> {
        let ids: Vec<String> = self
            .target
            .iter()
            .filter(|id| self.bound(id))
            .cloned()
            .collect();
        (!ids.is_empty()).then_some(ids)
    }

    /// The lights an effect goes to: those picked, or every light with a
    /// bulb that takes part in effects.
    pub fn effect_targets(&self) -> Vec<String> {
        self.picked().unwrap_or_else(|| {
            self.config
                .room
                .lights
                .iter()
                .filter(|l| l.enabled && self.bound(&l.id))
                .map(|l| l.id.clone())
                .collect()
        })
    }

    /// The lights a mood goes to: those picked, or every light with a bulb.
    pub fn mood_targets(&self) -> Vec<String> {
        self.picked().unwrap_or_else(|| {
            self.config
                .bindings
                .iter()
                .map(|b| b.light_id.clone())
                .collect()
        })
    }

    /// What a group of lights does, in a few words: the one thing they all
    /// do, or that they do several. True while that is an effect.
    pub fn summary(&self, ids: &[String], lang: Lang) -> Option<(String, bool)> {
        let first = self.doing_of(ids.first()?);
        if !ids.iter().all(|id| self.doing_of(id).same(&first)) {
            return Some((i18n::text(lang, "mode_several").to_string(), false));
        }
        Some((first.name(lang), matches!(first, Doing::Mode(_))))
    }

    /// True while every light of the scene does what it did when saved.
    pub fn shows_scene(&self, scene: &SavedScene) -> bool {
        let mut lights = scene.lights.iter().filter(|(id, _)| self.bound(id));
        let mut any = false;
        let all = lights.all(|(id, s)| {
            any = true;
            let mode = self.mode_of(id);
            match s {
                SceneLight::Mode { mode: m } => mode == Some(m.as_str()),
                _ if mode.is_some() => false,
                SceneLight::Natural => self.state(id).on && self.config.natural.contains(id),
                SceneLight::Hand { state } => same_state(&self.state(id), state),
            }
        });
        any && all
    }

    /// What one light does now, as a scene records it.
    pub fn record_of(&self, id: &str) -> SceneLight {
        if let Some(mode) = self.mode_of(id) {
            return SceneLight::Mode {
                mode: mode.to_string(),
            };
        }
        let state = self.state(id);
        if state.on && self.config.natural.contains(id) {
            return SceneLight::Natural;
        }
        SceneLight::Hand { state }
    }

    /// The parts of the config a running effect only reads at start: the
    /// room, which bulb drives which light, and the capture setup. The wall
    /// colour and the frame rate are pushed live.
    pub fn topology(&self) -> String {
        let mut room = self.config.room.clone();
        room.wall = lueur_core::color::Rgb::WHITE;
        let mut capture = self.config.capture;
        capture.target_fps = 0;
        serde_json::to_string(&(room, &self.config.bindings, capture)).unwrap_or_default()
    }
}

/// Two states alike: off is off, whatever the brightness and tint.
fn same_state(a: &LightState, b: &LightState) -> bool {
    a.on == b.on && (!a.on || (a.bright == b.bright && a.tint == b.tint))
}

/// The scene called `name`, whatever its case and spacing.
pub fn scene_named<'a>(scenes: &'a [SavedScene], name: &str) -> Option<&'a SavedScene> {
    let key = name.trim().to_lowercase();
    scenes.iter().find(|s| s.name.trim().to_lowercase() == key)
}

/// Whether going from `now` to `next` only changed the brightness of a light
/// that stays on: such a light keeps following the natural light (mirrors
/// `set_by_hand` in lights.rs).
pub fn dimmed_only(now: &LightState, next: &LightState) -> bool {
    now.on && next.on && now.tint == next.tint
}

/// Where a light stands against the screen, in screen half-widths: across it
/// (-1 is the left end, +1 the right) and in front of it.
pub fn seen_from(
    screen: &lueur_core::geometry::Screen,
    light: &lueur_core::geometry::Light,
) -> (f32, f32) {
    let half = (screen.width * 0.5).max(f32::EPSILON);
    let rel = light.pos - screen.center;
    (
        rel.dot(screen.right_axis()) / half,
        rel.dot(screen.facing()) / half,
    )
}

/// The key of what a light mirrors of the picture (`describe` in
/// geometry.ts).
pub fn describe(m: &lueur_core::geometry::LightMapping) -> &'static str {
    use lueur_core::geometry::ScreenEdge;
    if m.spread > 0.85 {
        "map_whole"
    } else if m.spread > 0.45 {
        "map_broad"
    } else {
        match m.edge {
            ScreenEdge::Left => "map_left",
            ScreenEdge::Right => "map_right",
            ScreenEdge::Center => "map_slice",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lueur_core::config::DeviceBinding;
    use lueur_core::geometry::{Light, Vec2};
    use lueur_shell::service::EffectGroup;

    fn status(groups: Vec<(&'static str, Vec<&str>)>) -> EffectStatus {
        let light_ids: Vec<String> = groups
            .iter()
            .flat_map(|(_, ids)| ids.iter().map(|s| s.to_string()))
            .collect();
        EffectStatus {
            running: !light_ids.is_empty(),
            connected_lights: light_ids.len(),
            mode: None,
            light_ids,
            groups: groups
                .into_iter()
                .map(|(mode, ids)| EffectGroup {
                    mode,
                    light_ids: ids.into_iter().map(String::from).collect(),
                })
                .collect(),
            reconnecting: Vec::new(),
            elsewhere: Vec::new(),
            music_waiting: false,
            hdr: false,
            screen_lost: false,
        }
    }

    fn config(ids: &[&str], bound: &[&str]) -> AppConfig {
        let mut c = AppConfig::default();
        for id in ids {
            c.room.lights.push(Light {
                id: id.to_string(),
                name: id.to_uppercase(),
                pos: Vec2::new(0.0, 0.0),
                height: 0.5,
                zone_override: None,
                enabled: true,
            });
        }
        for id in bound {
            c.bindings.push(DeviceBinding {
                light_id: id.to_string(),
                device_id: format!("dev-{id}"),
                ip: "192.168.1.2".into(),
                port: 55443,
                model: String::new(),
                name: String::new(),
                kind: Default::default(),
            });
        }
        c
    }

    #[test]
    fn a_light_reads_as_its_mood_its_mode_or_by_hand() {
        let mut c = config(&["a", "b", "c"], &["a", "b", "c"]);
        c.states.insert("a".into(), MOODS[2].state());
        c.states.insert(
            "b".into(),
            LightState {
                on: true,
                bright: 50,
                tint: Tint::Color { r: 1, g: 2, b: 3 },
            },
        );
        let s = status(vec![("music", vec!["c"])]);
        let room = Room {
            config: &c,
            status: &s,
            target: &[],
        };
        assert_eq!(room.doing_of("a"), Doing::Mood("relax".into()));
        assert!(matches!(room.doing_of("b"), Doing::Hand(_)));
        assert_eq!(room.doing_of("c"), Doing::Mode("music".into()));
        assert_eq!(room.bright_of("c"), c.show.bright);
        let all = ["a".to_string(), "b".to_string()];
        assert_eq!(
            room.summary(&all, Lang::En).unwrap().0,
            i18n::text(Lang::En, "mode_several").to_string()
        );
        assert!(room.summary(&["c".into()], Lang::En).unwrap().1);
    }

    #[test]
    fn picking_only_counts_lights_with_a_bulb() {
        let c = config(&["a", "b", "c"], &["a", "b"]);
        let s = status(vec![]);
        let target = vec!["c".to_string()];
        let room = Room {
            config: &c,
            status: &s,
            target: &target,
        };
        assert_eq!(room.picked(), None);
        assert_eq!(room.mood_targets(), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn a_scene_shows_while_its_lights_do_what_it_recorded() {
        let c = config(&["a", "b"], &["a", "b"]);
        let s = status(vec![("ambilight", vec!["a"])]);
        let room = Room {
            config: &c,
            status: &s,
            target: &[],
        };
        let scene = SavedScene {
            id: "x".into(),
            name: "Evening".into(),
            lights: [("a", room.record_of("a")), ("b", room.record_of("b"))]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        };
        assert!(room.shows_scene(&scene));
        let idle = status(vec![]);
        let later = Room {
            status: &idle,
            ..room
        };
        assert!(!later.shows_scene(&scene));
        assert!(scene_named(std::slice::from_ref(&scene), "  evening ").is_some());
    }

    #[test]
    fn looks_are_counted_once() {
        let looks = distinct_looks(
            [
                Look::of(&Doing::Off),
                Look::of(&Doing::Off),
                Look::of(&Doing::Mode("music".into())),
            ],
            3,
        );
        assert_eq!(looks.len(), 2);
        assert_eq!(picture_of("ambilight"), "modes/screen.webp");
        assert_eq!(picture_of("nothing"), "modes/off.webp");
    }

    #[test]
    fn colours_and_paces_read_as_the_svelte_page_has_them() {
        assert_eq!(kelvin_rgb(6600), [255, 255, 255]);
        let warm = kelvin_rgb(2200);
        assert!(warm[0] > warm[1] && warm[1] > warm[2]);
        assert_eq!(hue_rgb(0.0), [255, 0, 0]);
        assert!((rgb_hue([0, 0, 255]) - 240.0).abs() < 0.01);
        assert_eq!(times(Lang::Fr, show_pace(75.0)), "×2");
        assert_eq!(times(Lang::Fr, 1.5), "×1,5");
        assert_eq!(times(Lang::En, show_pace(1.0)), "×0.26");
    }

    #[test]
    fn the_feel_follows_the_smoothing() {
        let mut c = AppConfig::default();
        apply_feel(&mut c, "calm");
        assert_eq!(current_feel(&c), Some("calm"));
        c.engine.smoothing = 0.4;
        assert_eq!(current_feel(&c), None);
    }
}
