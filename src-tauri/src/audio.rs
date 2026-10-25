//! What the computer is playing, for the music show.
//!
//! On Windows Lueur listens to the speakers themselves (a WASAPI loopback of
//! the default output, the way OBS records desktop audio), so it follows any
//! music or game with nothing to set up. Other systems offer no such tap to
//! an ordinary app; there the default input (usually the microphone) hears
//! the room instead.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
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
        let sample_rate = config.sample_rate();
        // A second of sound at most: if nobody reads, the oldest goes.
        let cap = sample_rate as usize;
        let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(cap)));

        let tap = Tap {
            buffer: Arc::clone(&buffer),
            channels,
            cap,
        };
        let stream_config = config.config();
        let stream = match config.sample_format() {
            SampleFormat::F32 => tap.stream::<f32>(&device, stream_config),
            SampleFormat::I16 => tap.stream::<i16>(&device, stream_config),
            SampleFormat::U16 => tap.stream::<u16>(&device, stream_config),
            SampleFormat::I32 => tap.stream::<i32>(&device, stream_config),
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

    /// Swap everything captured since the last call into `out`.
    ///
    /// The two buffers trade places rather than one being taken: the capture
    /// callback runs on the audio thread, and must find room already there
    /// instead of growing a fresh vector every time.
    pub fn take_into(&self, out: &mut Vec<f32>) {
        out.clear();
        if let Ok(mut b) = self.buffer.lock() {
            std::mem::swap(&mut *b, out);
        }
    }
}

/// Where the capture callback mixes its samples down to.
struct Tap {
    buffer: Arc<Mutex<Vec<f32>>>,
    channels: usize,
    /// Most samples kept; the oldest go first.
    cap: usize,
}

impl Tap {
    /// An input stream of `T` samples feeding the buffer, mixed down to mono.
    fn stream<T>(self, device: &cpal::Device, config: StreamConfig) -> Result<Stream, cpal::Error>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let Tap {
            buffer,
            channels,
            cap,
        } = self;
        device.build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let Ok(mut buf) = buffer.lock() else {
                    return;
                };
                for frame in data.chunks(channels) {
                    let sum: f32 = frame.iter().map(|&s| s.to_sample::<f32>()).sum();
                    buf.push(sum / frame.len() as f32);
                }
                if buf.len() > cap {
                    let extra = buf.len() - cap;
                    buf.drain(..extra);
                }
            },
            |e| log::warn!("audio: {e}"),
            None,
        )
    }
}
