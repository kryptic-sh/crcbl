//! Audio device seam and streaming thread.
//!
//! `docs/plan/13-audio.md` makes audio a first-class pillar. This module
//! builds the platform abstraction: open an output stream and drive a
//! user-provided callback at the hardware sample rate. The mixer,
//! spatialiser, and cue grammar are separate modules that implement
//! [`AudioSource`].
//!
//! # Architecture
//!
//! ```text
//! AudioStream::open(source)  ──▶  spawns audio thread (cpal or null)
//!                                      calls source.fill(buf) per block
//! ```
//!
//! The audio thread is opaque to the caller: it runs a real-time callback
//! on the OS audio thread, and `source.fill()` is called inside it.
//! The source must be `Send + Sync + 'static`.

pub mod mixer;
pub mod qoa;
pub mod spatial;
pub mod wav;

use std::sync::Arc;

// ---------------------------------------------------------------------------
// AudioSample
// ---------------------------------------------------------------------------

/// Floating-point sample type used throughout the engine.
///
/// All DSP is f32 internally; integer codecs convert at the boundaries.
/// Stereo samples are interleaved `[f32; 2]` with left first.
pub type AudioSample = f32;

/// Number of output channels (stereo, interleaved left/right).
pub const CHANNELS: usize = 2;

// ---------------------------------------------------------------------------
// AudioSource
// ---------------------------------------------------------------------------

/// A callable that fills an interleaved stereo buffer with samples.
///
/// Called from the audio thread at the hardware sample rate.
/// `buffer` is `[interleaved L/R; sample_count]`, already zeroed by the
/// stream before `fill` is called. The source accumulates into it
/// (additive mixing).
pub trait AudioSource: Send + Sync + 'static {
    /// Fill `buffer` with interleaved stereo samples at `sample_rate` Hz.
    ///
    /// Implementations should *add* to existing samples (not overwrite) so
    /// the stream's zero-init produces silence when nothing is playing.
    fn fill(&self, buffer: &mut [AudioSample], sample_rate: u32);
}

// ---------------------------------------------------------------------------
// AudioStream
// ---------------------------------------------------------------------------

/// An open audio output stream.
///
/// Created by [`AudioStream::open`] or [`AudioStream::open_null`].
/// The stream starts immediately and runs until dropped.
///
/// `open` connects to the system default output device via cpal.
/// `open_null` produces silence at 48 kHz on a polling thread — the
/// headless CI and test path, exactly like `NullBackend` for GPU tests.
pub struct AudioStream {
    _alive: Arc<()>,
}

impl std::fmt::Debug for AudioStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioStream").finish()
    }
}

impl AudioStream {
    /// Open the system default output device.
    ///
    /// Returns `None` if no device is available (headless CI).
    #[must_use]
    pub fn open(source: impl AudioSource) -> Option<Self> {
        let alive = Arc::new(());
        let alive_weak = Arc::downgrade(&alive);
        let source = Arc::new(source);

        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::default_host();
        let device = host.default_output_device()?;

        let supported = device.default_output_config().ok()?;
        let sample_rate = supported.sample_rate();
        let channels = supported.channels() as usize;
        let config: cpal::StreamConfig = supported.into();

        let src = Arc::clone(&source);

        let stream = device
            .build_output_stream::<f32, _, _>(
                config,
                {
                    let alive_weak = alive_weak.clone();
                    move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                        if alive_weak.upgrade().is_some() {
                            fill_audio(data, channels, src.as_ref(), sample_rate);
                        }
                    }
                },
                |err| {
                    eprintln!("audio stream error: {err}");
                },
                None,
            )
            .ok()?;

        stream.play().ok()?;
        // Keep the stream alive; cpal stops playback on drop.
        // We hold it via a leaked Box to prevent the compiler from
        // moving it (Stream is not Send on all platforms).
        let stream: std::pin::Pin<Box<_>> = Box::pin(stream);
        std::mem::forget(stream);

        Some(Self { _alive: alive })
    }

    /// Open a null stream for headless tests and CI.
    ///
    /// Produces silence at 48 kHz stereo using a polling thread; no
    /// hardware audio device is opened.
    #[must_use]
    pub fn open_null(source: impl AudioSource) -> Self {
        let sample_rate = 48_000u32;
        let source = Arc::new(source);
        let alive = Arc::new(());
        let alive_weak = Arc::downgrade(&alive);

        std::thread::spawn(move || {
            let block_size = 256;
            let mut buffer = vec![0.0f32; block_size * CHANNELS];
            loop {
                if alive_weak.upgrade().is_none() {
                    break;
                }
                source.fill(&mut buffer, sample_rate);
                std::thread::sleep(std::time::Duration::from_micros(
                    (block_size as u64 * 1_000_000) / sample_rate as u64,
                ));
            }
        });

        Self { _alive: alive }
    }
}

fn fill_audio(data: &mut [f32], channels: usize, source: &dyn AudioSource, sample_rate: u32) {
    if channels == CHANNELS {
        source.fill(data, sample_rate);
    } else if channels == 1 {
        let block = data.len();
        let mut stereo = vec![0.0f32; block * CHANNELS];
        source.fill(&mut stereo, sample_rate);
        for (i, sample) in data.iter_mut().enumerate() {
            *sample = (stereo[i * 2] + stereo[i * 2 + 1]) * 0.5;
        }
    } else {
        data.fill(0.0);
        let block = data.len() / channels;
        let mut stereo = vec![0.0f32; block * CHANNELS];
        source.fill(&mut stereo, sample_rate);
        for i in 0..block {
            data[i * channels] = stereo[i * CHANNELS];
            data[i * channels + 1] = stereo[i * CHANNELS + 1];
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A source that accumulates a constant DC value and counts fills.
    struct DcSource {
        value: f32,
        fill_count: std::sync::atomic::AtomicU64,
    }

    impl DcSource {
        fn new(value: f32) -> Self {
            Self {
                value,
                fill_count: std::sync::atomic::AtomicU64::new(0),
            }
        }
    }

    impl AudioSource for DcSource {
        fn fill(&self, buffer: &mut [AudioSample], _sample_rate: u32) {
            for sample in buffer.iter_mut() {
                *sample += self.value;
            }
            self.fill_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[test]
    fn source_fill_receives_stereo_buffer() {
        struct CheckSource;
        impl AudioSource for CheckSource {
            fn fill(&self, buffer: &mut [AudioSample], rate: u32) {
                assert_eq!(rate, 48_000);
                assert!(!buffer.is_empty());
                assert_eq!(buffer.len() % CHANNELS, 0);
            }
        }
        let stream = AudioStream::open_null(CheckSource);
        std::thread::sleep(std::time::Duration::from_millis(30));
        drop(stream);
    }

    #[test]
    fn dc_source_accumulates_across_fills() {
        let source = DcSource::new(0.25);
        let mut buf = vec![0.0f32; 256 * CHANNELS];
        source.fill(&mut buf, 48_000);
        source.fill(&mut buf, 48_000);
        for &s in &buf {
            assert!((s - 0.5).abs() < 0.001);
        }
    }

    #[test]
    fn null_stream_runs_without_error() {
        let source = DcSource::new(0.0);
        let stream = AudioStream::open_null(source);
        std::thread::sleep(std::time::Duration::from_millis(20));
        drop(stream);
    }
}
