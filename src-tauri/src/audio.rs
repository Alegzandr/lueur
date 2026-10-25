//! What the computer is playing, for the music show.
//!
//! On Windows Lueur listens to the speakers themselves (a WASAPI loopback of
//! an output, the way OBS records desktop audio), so it follows any music or
//! game with nothing to set up: the default output, another one, or only
//! what one application plays (see [`apps`]). Other systems offer no such tap
//! to an ordinary app; there an input (by default the microphone) hears the
//! room instead.

#[cfg(windows)]
mod apps;

use crate::error::AppError;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{DeviceId, ErrorKind, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use lueur_core::config::AudioSource;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use std::time::{Duration, Instant};

/// Whether this system can listen to its outputs, and to applications one
/// by one, rather than to its inputs.
pub const LOOPBACK: bool = cfg!(windows);

/// How long an application's tap may hear nothing before it checks it still
/// listens to the right process, and how often it checks again while that
/// lasts.
#[cfg(windows)]
const QUIET: Duration = Duration::from_secs(3);

/// A running capture. Samples pile up, mixed down to mono, until taken.
/// Not `Send` on every platform: open it on the thread that reads it.
pub struct AudioTap {
    capture: Capture,
    buffer: Arc<Mutex<Vec<f32>>>,
    /// Set when the capture broke: the device went away, the stream died.
    failed: Arc<AtomicBool>,
    pub sample_rate: u32,
}

enum Capture {
    /// A device's stream. The default device's follows the default as it
    /// changes.
    Stream { _stream: Stream },
    /// One application's sound, from the process `pid` it runs as.
    #[cfg(windows)]
    App {
        _loopback: apps::Loopback,
        exe: String,
        pid: u32,
        /// The last time anything louder than silence came through.
        heard: Instant,
        /// The last time it checked, quiet, that `pid` is still the one.
        checked: Instant,
    },
}

/// Why a source could not be listened to.
#[derive(Debug)]
pub enum AudioError {
    /// The device picked is not plugged in: its name.
    DeviceMissing(String),
    /// The application picked plays nothing, or is not open: its name.
    #[cfg_attr(not(windows), allow(dead_code))]
    AppClosed(String),
    Failed(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioError::DeviceMissing(name) => write!(f, "{name} is not plugged in"),
            AudioError::AppClosed(name) => write!(f, "{name} is not playing"),
            AudioError::Failed(e) => f.write_str(e),
        }
    }
}

impl From<AudioError> for AppError {
    fn from(e: AudioError) -> Self {
        match e {
            AudioError::DeviceMissing(name) => AppError::new("audio_device_missing").subject(name),
            AudioError::AppClosed(name) => AppError::new("audio_app_closed").subject(name),
            AudioError::Failed(e) => AppError::new("audio_unavailable").detail(e),
        }
    }
}

fn failed(e: impl std::fmt::Display) -> AudioError {
    AudioError::Failed(e.to_string())
}

impl AudioTap {
    pub fn open(source: &AudioSource) -> Result<Self, AudioError> {
        let broke = Arc::new(AtomicBool::new(false));
        let (capture, tap, sample_rate) = match source {
            #[cfg(windows)]
            AudioSource::App { exe, name } => {
                let pid =
                    apps::process_of(exe).ok_or_else(|| AudioError::AppClosed(name.clone()))?;
                let tap = Tap::new(apps::SAMPLE_RATE, apps::CHANNELS);
                let loopback =
                    apps::Loopback::open(pid, tap.clone(), Arc::clone(&broke)).map_err(failed)?;
                let capture = Capture::App {
                    _loopback: loopback,
                    exe: exe.clone(),
                    pid,
                    heard: Instant::now(),
                    checked: Instant::now(),
                };
                (capture, tap, apps::SAMPLE_RATE)
            }
            _ => {
                let host = cpal::default_host();
                let device = device(&host, source)?;
                #[cfg(windows)]
                let config = device.default_output_config().map_err(failed)?;
                #[cfg(not(windows))]
                let config = device.default_input_config().map_err(failed)?;
                let sample_rate = config.sample_rate();
                let tap = Tap::new(sample_rate, config.channels());
                let stream_config = config.config();
                let on_break = Arc::clone(&broke);
                let stream = match config.sample_format() {
                    SampleFormat::F32 => {
                        tap.clone().stream::<f32>(&device, stream_config, on_break)
                    }
                    SampleFormat::I16 => {
                        tap.clone().stream::<i16>(&device, stream_config, on_break)
                    }
                    SampleFormat::U16 => {
                        tap.clone().stream::<u16>(&device, stream_config, on_break)
                    }
                    SampleFormat::I32 => {
                        tap.clone().stream::<i32>(&device, stream_config, on_break)
                    }
                    other => return Err(failed(format!("unsupported sample format {other}"))),
                }
                .map_err(failed)?;
                stream.play().map_err(failed)?;
                (Capture::Stream { _stream: stream }, tap, sample_rate)
            }
        };
        Ok(AudioTap {
            capture,
            buffer: tap.buffer,
            failed: broke,
            sample_rate,
        })
    }

    /// Swap everything captured since the last call into `out`.
    ///
    /// The two buffers trade places rather than one being taken: the capture
    /// callback runs on the audio thread, and must find room already there
    /// instead of growing a fresh vector every time.
    pub fn take_into(&mut self, out: &mut Vec<f32>) {
        out.clear();
        if let Ok(mut b) = self.buffer.lock() {
            std::mem::swap(&mut *b, out);
        }
        #[cfg(windows)]
        if let Capture::App { heard, .. } = &mut self.capture
            && out.iter().any(|s| s.abs() > 1e-5)
        {
            *heard = Instant::now();
        }
    }

    /// Whether the tap no longer hears what it was opened on: the device
    /// went away, or the application it followed was closed (or opened
    /// again, as another process).
    pub fn lost(&mut self) -> bool {
        if self.failed.load(Ordering::Relaxed) {
            return true;
        }
        match &mut self.capture {
            Capture::Stream { .. } => false,
            // Only a tap that hears nothing looks further: one that hears
            // music is listening to something.
            #[cfg(windows)]
            Capture::App {
                exe,
                pid,
                heard,
                checked,
                ..
            } => {
                if heard.elapsed() < QUIET || checked.elapsed() < QUIET {
                    return false;
                }
                *checked = Instant::now();
                apps::process_of(exe) != Some(*pid)
            }
        }
    }
}

/// The default device of the kind Lueur listens to here.
fn default_device(host: &cpal::Host) -> Option<cpal::Device> {
    #[cfg(windows)]
    return host.default_output_device();
    #[cfg(not(windows))]
    return host.default_input_device();
}

/// The device `source` names.
fn device(host: &cpal::Host, source: &AudioSource) -> Result<cpal::Device, AudioError> {
    match source {
        AudioSource::Device { id, name } => id
            .parse::<DeviceId>()
            .ok()
            .and_then(|id| host.device_by_id(&id))
            .ok_or_else(|| AudioError::DeviceMissing(name.clone())),
        _ => default_device(host).ok_or_else(|| failed("no audio device")),
    }
}

/// What the music show can listen to.
#[derive(Debug, Clone, Serialize)]
pub struct AudioSources {
    /// Outputs on Windows (and applications next to them), inputs elsewhere.
    pub loopback: bool,
    pub devices: Vec<AudioDevice>,
    /// The applications that have played sound since they opened.
    pub apps: Vec<AudioApp>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioApp {
    pub exe: String,
    pub name: String,
}

/// Every device and application the music show could listen to right now.
pub fn sources() -> AudioSources {
    let host = cpal::default_host();
    let default = default_device(&host).and_then(|d| d.id().ok());
    #[cfg(windows)]
    let devices = host.output_devices();
    #[cfg(not(windows))]
    let devices = host.input_devices();
    let mut devices: Vec<AudioDevice> = devices
        .map(|list| {
            list.filter_map(|d| {
                let id = d.id().ok()?;
                let name = d.description().ok()?.name().to_string();
                Some(AudioDevice {
                    is_default: Some(&id) == default.as_ref(),
                    id: id.to_string(),
                    name,
                })
            })
            .collect()
        })
        .unwrap_or_else(|e| {
            log::warn!("audio devices: {e}");
            Vec::new()
        });
    devices.sort_by_cached_key(|d| d.name.to_lowercase());
    #[cfg(windows)]
    let apps = apps::list();
    #[cfg(not(windows))]
    let apps = Vec::new();
    AudioSources {
        loopback: LOOPBACK,
        devices,
        apps,
    }
}

/// Where the capture callback mixes its samples down to.
#[derive(Clone)]
struct Tap {
    buffer: Arc<Mutex<Vec<f32>>>,
    channels: usize,
    /// Most samples kept; the oldest go first.
    cap: usize,
}

impl Tap {
    fn new(sample_rate: u32, channels: u16) -> Self {
        // A second of sound at most: if nobody reads, the oldest goes.
        let cap = sample_rate as usize;
        Tap {
            buffer: Arc::new(Mutex::new(Vec::with_capacity(cap))),
            channels: usize::from(channels.max(1)),
            cap,
        }
    }

    /// Mix interleaved `data` down to mono into the buffer.
    fn feed<T>(&self, data: &[T])
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let Ok(mut buf) = self.buffer.lock() else {
            return;
        };
        for frame in data.chunks(self.channels) {
            let sum: f32 = frame.iter().map(|&s| s.to_sample::<f32>()).sum();
            buf.push(sum / frame.len() as f32);
        }
        self.trim(&mut buf);
    }

    /// `frames` of silence, which a loopback reports without samples.
    #[cfg(windows)]
    fn silence(&self, frames: usize) {
        let Ok(mut buf) = self.buffer.lock() else {
            return;
        };
        let len = buf.len();
        buf.resize(len + frames, 0.0);
        self.trim(&mut buf);
    }

    fn trim(&self, buf: &mut Vec<f32>) {
        if buf.len() > self.cap {
            let extra = buf.len() - self.cap;
            buf.drain(..extra);
        }
    }

    /// An input stream of `T` samples feeding the buffer, mixed down to
    /// mono. `failed` is set if it breaks.
    fn stream<T>(
        self,
        device: &cpal::Device,
        config: StreamConfig,
        failed: Arc<AtomicBool>,
    ) -> Result<Stream, cpal::Error>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        device.build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| self.feed(data),
            move |e| {
                log::warn!("audio: {e}");
                // The rest (a route that changed under the default device,
                // a refused real-time priority) leave the stream playing.
                if matches!(
                    e.kind(),
                    ErrorKind::DeviceNotAvailable
                        | ErrorKind::StreamInvalidated
                        | ErrorKind::HostUnavailable
                ) {
                    failed.store(true, Ordering::Relaxed);
                }
            },
            None,
        )
    }
}
