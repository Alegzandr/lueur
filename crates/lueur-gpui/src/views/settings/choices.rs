//! The choices the Settings page offers, worked out from the config and
//! from what the system lists, apart from drawing them so they can be
//! tested.

use lueur_core::config::AudioSource;
use lueur_shell::audio::AudioSources;
use lueur_shell::capture::MonitorInfo;

/// Updates per second the ambilight offers.
const FPS_CHOICES: [u32; 3] = [30, 45, 60];

/// The update rates to offer. A rate set by hand in the config file is
/// offered too, so the control never shows nothing chosen.
pub fn fps_options(current: u32) -> Vec<u32> {
    let mut list: Vec<u32> = FPS_CHOICES.to_vec();
    list.push(current);
    list.sort_unstable();
    list.dedup();
    list
}

/// How dark scenes light the room: the engine's brightness curve, and the
/// key of the words for it.
pub const CURVES: [(f32, &str); 3] = [
    (1.0, "dark_lit"),
    (1.6, "dark_balanced"),
    (2.2, "dark_true"),
];

/// The index in [`CURVES`] of the curve nearest to `at`, so a value the
/// file holds that none of the three matches still shows one chosen.
pub fn nearest_curve(at: f32) -> usize {
    let mut best = 0;
    for (i, (value, _)) in CURVES.iter().enumerate() {
        if (value - at).abs() < (CURVES[best].0 - at).abs() {
            best = i;
        }
    }
    best
}

/// A display, as the picker names it: its name, its size, and whether it
/// is the main one (`main` is that word in the interface language).
pub fn monitor_label(m: &MonitorInfo, main: &str) -> String {
    let mut label = format!("{} · {}×{}", m.name, m.width, m.height);
    if m.is_primary {
        label.push_str(" · ");
        label.push_str(main);
    }
    label
}

/// The words the sound source picker needs, in the interface language.
pub struct SoundWords<'a> {
    /// "Default output" or "Default input".
    pub default: &'a str,
    /// Said after a device that is not plugged in any more.
    pub missing: &'a str,
    /// Said after an application that is closed.
    pub closed: &'a str,
}

/// One choice of the sound source picker.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundChoice {
    pub label: String,
    pub source: AudioSource,
}

/// Whether two sources are the same choice. Names do not count: a device
/// is known by its id and an application by its executable, whose case may
/// differ between two sessions.
pub fn same_source(a: &AudioSource, b: &AudioSource) -> bool {
    match (a, b) {
        (AudioSource::Default, AudioSource::Default) => true,
        (AudioSource::Device { id: x, .. }, AudioSource::Device { id: y, .. }) => x == y,
        (AudioSource::App { exe: x, .. }, AudioSource::App { exe: y, .. }) => {
            x.eq_ignore_ascii_case(y)
        }
        _ => false,
    }
}

/// Every source the music show could listen to: the default device first,
/// then the devices, then the applications. The one picked stays listed,
/// marked as missing, while it is unplugged or closed.
pub fn sound_choices(
    sources: &AudioSources,
    current: &AudioSource,
    words: &SoundWords,
) -> Vec<SoundChoice> {
    // The default device by name, so it reads apart from the same device
    // picked for good (which stays on it when the default moves).
    let default = match sources.devices.iter().find(|d| d.is_default) {
        Some(d) => format!("{} ({})", words.default, d.name),
        None => words.default.to_string(),
    };
    let mut list = vec![SoundChoice {
        label: default,
        source: AudioSource::Default,
    }];
    list.extend(sources.devices.iter().map(|d| SoundChoice {
        label: d.name.clone(),
        source: AudioSource::Device {
            id: d.id.clone(),
            name: d.name.clone(),
        },
    }));
    if let AudioSource::Device { name, .. } = current
        && !list.iter().any(|c| same_source(&c.source, current))
    {
        list.push(SoundChoice {
            label: format!("{name} · {}", words.missing),
            source: current.clone(),
        });
    }
    list.extend(sources.apps.iter().map(|a| SoundChoice {
        label: a.name.clone(),
        source: AudioSource::App {
            exe: a.exe.clone(),
            name: a.name.clone(),
        },
    }));
    if let AudioSource::App { name, .. } = current
        && !list.iter().any(|c| same_source(&c.source, current))
    {
        list.push(SoundChoice {
            label: format!("{name} · {}", words.closed),
            source: current.clone(),
        });
    }
    list
}

/// The index of the choice the config holds.
pub fn chosen_sound(list: &[SoundChoice], current: &AudioSource) -> Option<usize> {
    list.iter().position(|c| same_source(&c.source, current))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lueur_shell::audio::{AudioApp, AudioDevice};

    const WORDS: SoundWords = SoundWords {
        default: "Default output",
        missing: "not found",
        closed: "closed",
    };

    fn sources() -> AudioSources {
        AudioSources {
            loopback: true,
            devices: vec![
                AudioDevice {
                    id: "a".into(),
                    name: "Speakers".into(),
                    is_default: true,
                },
                AudioDevice {
                    id: "b".into(),
                    name: "Headphones".into(),
                    is_default: false,
                },
            ],
            apps: vec![AudioApp {
                exe: "Spotify.exe".into(),
                name: "Spotify".into(),
            }],
        }
    }

    #[test]
    fn a_rate_set_by_hand_is_offered_too() {
        assert_eq!(fps_options(45), vec![30, 45, 60]);
        assert_eq!(fps_options(50), vec![30, 45, 50, 60]);
    }

    #[test]
    fn a_curve_off_the_three_shows_the_nearest() {
        assert_eq!(nearest_curve(1.6), 1);
        assert_eq!(nearest_curve(0.4), 0);
        assert_eq!(nearest_curve(2.0), 2);
        assert_eq!(nearest_curve(9.0), 2);
    }

    #[test]
    fn the_default_device_is_named_and_listed_first() {
        let list = sound_choices(&sources(), &AudioSource::Default, &WORDS);
        assert_eq!(list[0].label, "Default output (Speakers)");
        assert_eq!(list.len(), 4);
        assert_eq!(chosen_sound(&list, &AudioSource::Default), Some(0));
    }

    #[test]
    fn a_source_gone_stays_listed_as_missing() {
        let gone = AudioSource::Device {
            id: "z".into(),
            name: "USB DAC".into(),
        };
        let list = sound_choices(&sources(), &gone, &WORDS);
        assert_eq!(list[3].label, "USB DAC · not found");
        assert_eq!(chosen_sound(&list, &gone), Some(3));

        let closed = AudioSource::App {
            exe: "vlc.exe".into(),
            name: "VLC".into(),
        };
        let list = sound_choices(&sources(), &closed, &WORDS);
        assert_eq!(list.last().unwrap().label, "VLC · closed");
    }

    #[test]
    fn an_application_matches_whatever_the_case_of_its_executable() {
        let picked = AudioSource::App {
            exe: "spotify.exe".into(),
            name: "Spotify".into(),
        };
        let list = sound_choices(&sources(), &picked, &WORDS);
        assert_eq!(list.len(), 4);
        assert_eq!(chosen_sound(&list, &picked), Some(3));
    }

    #[test]
    fn the_main_display_says_so() {
        let m = MonitorInfo {
            index: 0,
            name: "DELL".into(),
            width: 2560,
            height: 1440,
            is_primary: true,
        };
        assert_eq!(monitor_label(&m, "main"), "DELL · 2560×1440 · main");
    }
}
