//! What the computer is playing, for the music show.
//!
//! On Windows Lueur listens to the speakers themselves (a WASAPI loopback of
//! the default output, the way OBS records desktop audio), so it follows any
//! music or game with nothing to set up. Other systems offer no such tap to
//! an ordinary app; there the default input (usually the microphone) hears
//! the room instead.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use std::io;
use std::sync::{Arc, Mutex};

/// A running capture. Samples pile up, mixed down to mono, until taken.
/// Not `Send` on every platform: open it on the thread that reads it.
pub struct AudioTap {
    _stream: Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
    pub sample_rate: u32,
}

fn err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

impl AudioTap {
    pub fn open() -> io::Result<Self> {
        let host = cpal::default_host();
        #[cfg(windows)]
        let (device, config) = {
            let d = host
                .default_output_device()
                .ok_or_else(|| err("no audio output"))?;
            let c = d.default_output_config().map_err(err)?;
            (d, c)
        };
        #[cfg(not(windows))]
        let (device, config) = {
            let d = host
                .default_input_device()
                .ok_or_else(|| err("no audio input"))?;
            let c = d.default_input_config().map_err(err)?;
            (d, c)
        };

        let channels = usize::from(config.channels().max(1));
        let sample_rate = config.sample_rate().0;
        // A second of sound at most: if nobody reads, the oldest goes.
        let cap = sample_rate as usize;
        let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(cap)));

        fn sink<T: Copy>(
            buffer: Arc<Mutex<Vec<f32>>>,
            channels: usize,
            cap: usize,
            to_f32: impl Fn(T) -> f32,
        ) -> impl FnMut(&[T], &cpal::InputCallbackInfo) {
            move |data: &[T], _| {
                let Ok(mut buf) = buffer.lock() else {
                    return;
                };
                for frame in data.chunks(channels) {
                    let sum: f32 = frame.iter().map(|&s| to_f32(s)).sum();
                    buf.push(sum / frame.len() as f32);
                }
                if buf.len() > cap {
                    let extra = buf.len() - cap;
                    buf.drain(..extra);
                }
            }
        }

        let on_error = |e| eprintln!("lueur: audio: {e}");
        let stream_config = config.config();
        let stream = match config.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                sink(Arc::clone(&buffer), channels, cap, |s: f32| s),
                on_error,
                None,
            ),
            SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                sink(Arc::clone(&buffer), channels, cap, |s: i16| {
                    f32::from(s) / 32768.0
                }),
                on_error,
                None,
            ),
            SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                sink(Arc::clone(&buffer), channels, cap, |s: u16| {
                    (f32::from(s) - 32768.0) / 32768.0
                }),
                on_error,
                None,
            ),
            SampleFormat::I32 => device.build_input_stream(
                &stream_config,
                sink(Arc::clone(&buffer), channels, cap, |s: i32| {
                    s as f32 / 2_147_483_648.0
                }),
                on_error,
                None,
            ),
            other => return Err(err(format!("unsupported sample format {other}"))),
        }
        .map_err(err)?;
        stream.play().map_err(err)?;

        Ok(AudioTap {
            _stream: stream,
            buffer,
            sample_rate,
        })
    }

    /// Everything captured since the last call.
    pub fn take(&self) -> Vec<f32> {
        self.buffer
            .lock()
            .map(|mut b| std::mem::take(&mut *b))
            .unwrap_or_default()
    }
}
