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
//! native  AudioStream::open(source)   ──▶  cpal callback on the OS audio thread
//!         AudioStream::open_null(src) ──▶  polling thread, silence (CI)
//! wasm32  AudioStream::open(source)   ──▶  web::install; the AudioWorklet pulls
//! ```
//!
//! On native the audio thread is opaque to the caller: it runs a real-time
//! callback on the OS audio thread, and `source.fill()` is called inside it.
//! The source must be `Send + Sync + 'static`.
//!
//! In the browser the direction is reversed — the worklet asks for a block and
//! [`web::WebAudioOutput::render`] answers — because a browser page has no
//! thread to give an audio callback and, on the GitHub Pages deploy target, no
//! `SharedArrayBuffer` to share a ring buffer through. [`web`]'s module docs
//! carry the whole JS↔wasm contract; this module's job is only to make
//! [`AudioStream`] mean the same thing on both.
//!
//! # Sample rates
//!
//! Two rates exist and confusing them detunes everything:
//!
//! - [`INTERNAL_SAMPLE_RATE`] is what the mixer's voices are authored for and
//!   the only rate [`AudioSource::fill`] is ever driven at in the browser.
//! - The *device* rate is whatever the hardware runs at. cpal reports it and
//!   the native path passes it straight to `fill`; the browser reports
//!   `AudioContext.sampleRate` and [`web`] resamples between the two rather
//!   than passing it down.

pub mod event;
pub mod mixer;
pub mod qoa;
pub mod spatial;
pub mod synth;
pub mod wav;
pub mod web;

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

/// The engine's fixed internal sample rate, in Hz.
///
/// `docs/plan/13-audio.md` fixes it at 48 kHz: the mixer's voices hold sample
/// data authored for this rate, and the spatial cue grammar's ITD delays and
/// pitch ratios are derived against it. A device that runs at some other rate
/// is resampled at the output edge — see [`web`] for the browser's, which is
/// the first platform where the two genuinely differ.
pub const INTERNAL_SAMPLE_RATE: u32 = 48_000;

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

/// A shared source is a source.
///
/// [`AudioStream::open`] consumes what it is given and hands it to a thread the
/// caller cannot reach, so a source the game also needs to *drive* — a
/// [`Mixer`](mixer::Mixer) it goes on playing voices through — has to be shared
/// rather than moved. This impl is what makes `AudioStream::open(Arc::clone(&mixer))`
/// type-check, and it is blanket rather than one impl on `Arc<Mixer>` because
/// nothing about the reasoning is specific to the mixer.
///
/// `?Sized`, so `Arc<dyn AudioSource>` works too: a caller choosing between
/// sources at run time does not have to name a concrete type.
impl<T: AudioSource + ?Sized> AudioSource for Arc<T> {
    fn fill(&self, buffer: &mut [AudioSample], sample_rate: u32) {
        (**self).fill(buffer, sample_rate);
    }
}

// ---------------------------------------------------------------------------
// AudioStream
// ---------------------------------------------------------------------------

/// An open audio output stream.
///
/// Created by [`AudioStream::open`] or [`AudioStream::open_null`].
/// The stream starts immediately and runs until dropped.
///
/// | Target | `open` | `open_null` |
/// | --- | --- | --- |
/// | native | the system default output device, via cpal | silence at [`INTERNAL_SAMPLE_RATE`] on a polling thread — the headless CI and test path, exactly like `NullBackend` for GPU tests |
/// | `wasm32` | installs the source as [`web`]'s pull target and returns; the AudioWorklet drives it | installs nothing |
///
/// The `wasm32` `open_null` is the one asymmetry, and it is deliberate: there
/// is no thread in a browser page to poll a null device on, so a null stream
/// there simply never calls [`AudioSource::fill`]. That is the same *audible*
/// result as the native null path — silence — but a source with side effects
/// (a queue it drains, a counter it bumps) will not see them. Nothing on
/// `wasm32` needs a null device, because there is no CI machine without an
/// `AudioContext`; the constructor exists so a caller can be target-agnostic.
pub struct AudioStream {
    // Declared first so it drops first: the callback sees a dead weak handle
    // and writes silence for however long it takes the device to stop.
    #[cfg(not(target_arch = "wasm32"))]
    _alive: Arc<()>,
    /// The cpal stream, owned so that dropping `AudioStream` stops playback.
    /// `None` for the null backend, which uses a polling thread instead.
    #[cfg(not(target_arch = "wasm32"))]
    _stream: Option<cpal::Stream>,
    /// Whether this stream is the one that installed [`web`]'s output, and so
    /// the one whose `Drop` must uninstall it.
    #[cfg(target_arch = "wasm32")]
    installed: bool,
}

impl std::fmt::Debug for AudioStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioStream").finish()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl AudioStream {
    /// Open the system default output device.
    ///
    /// Returns `None` if no device is available (headless CI). The stream is
    /// owned by the returned value and stops when it is dropped.
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
                        } else {
                            // Not writing at all would let the device play back
                            // whatever the buffer happened to hold.
                            data.fill(0.0);
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

        Some(Self {
            _alive: alive,
            _stream: Some(stream),
        })
    }

    /// Open a null stream for headless tests and CI.
    ///
    /// Produces silence at [`INTERNAL_SAMPLE_RATE`] stereo using a polling
    /// thread; no hardware audio device is opened.
    #[must_use]
    pub fn open_null(source: impl AudioSource) -> Self {
        let sample_rate = INTERNAL_SAMPLE_RATE;
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
                // `AudioSource` accumulates, so the buffer must be zeroed each
                // block — the same contract the cpal path keeps.
                buffer.fill(0.0);
                source.fill(&mut buffer, sample_rate);
                std::thread::sleep(std::time::Duration::from_micros(
                    (block_size as u64 * 1_000_000) / sample_rate as u64,
                ));
            }
        });

        Self {
            _alive: alive,
            _stream: None,
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl AudioStream {
    /// Install `source` as the browser's audio output.
    ///
    /// Returns `None` when a stream is already open on this thread — a page
    /// gets one output, and silently replacing a live one would strand the
    /// worklet mid-block. Nothing plays until the JS shim calls
    /// `__crcbl_web_audio_configure` and starts pulling; see [`web`].
    #[must_use]
    pub fn open(source: impl AudioSource) -> Option<Self> {
        web::install(source).then_some(Self { installed: true })
    }

    /// A stream that never calls [`AudioSource::fill`].
    ///
    /// See the asymmetry noted on [`AudioStream`]: a browser page has no
    /// thread to poll a null device on.
    #[must_use]
    pub fn open_null(_source: impl AudioSource) -> Self {
        Self { installed: false }
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for AudioStream {
    fn drop(&mut self) {
        if self.installed {
            web::uninstall();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fill_audio(data: &mut [f32], channels: usize, source: &dyn AudioSource, sample_rate: u32) {
    // `AudioSource::fill` is additive and documented to receive a zeroed
    // buffer; cpal hands back whatever the device buffer last held.
    data.fill(0.0);

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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn fill_audio_zeroes_the_device_buffer_on_every_layout() {
        struct Silent;
        impl AudioSource for Silent {
            fn fill(&self, _buffer: &mut [AudioSample], _rate: u32) {}
        }

        // cpal hands back the previous block's contents; a source that adds
        // nothing must leave silence, not the stale buffer.
        for channels in [1usize, CHANNELS, 6] {
            let mut data = vec![0.7f32; 8 * channels];
            fill_audio(&mut data, channels, &Silent, 48_000);
            assert!(
                data.iter().all(|s| *s == 0.0),
                "stale samples left for {channels} channels: {data:?}"
            );
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
