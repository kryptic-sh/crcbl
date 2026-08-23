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
//!   the native path passes it down to `fill`, where the mixer steps its
//!   voices at the internal rate per output frame — a device at any other
//!   rate keeps the same pitch and duration, so the native path no longer
//!   detunes. The browser reports `AudioContext.sampleRate` and [`web`]
//!   resamples between the two rather than passing it down.

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
        if channels > CHANNELS {
            // Said out loud rather than left to be discovered by listening.
            // `fill_audio` feeds the first two channels and leaves the rest
            // silent, which on a 5.1 device is four speakers that never carry
            // anything — see its docs for why that is the only routing cpal's
            // channel *count* supports.
            crcbl_core::log::warn!(
                "audio device reports {channels} channels; the mixer is {CHANNELS}-channel, \
                 so the first two carry the output and the remaining {} stay silent",
                channels - CHANNELS
            );
        }
        let config: cpal::StreamConfig = supported.into();

        let src = Arc::clone(&source);
        let mut scratch: Vec<f32> = Vec::new();

        let stream = device
            .build_output_stream::<f32, _, _>(
                config,
                {
                    let alive_weak = alive_weak.clone();
                    move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                        if alive_weak.upgrade().is_some() {
                            fill_audio(data, channels, src.as_ref(), sample_rate, &mut scratch);
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
/// Fills one device block from `source`, adapting the mixer's stereo output to
/// whatever channel count the device asked for.
///
/// # The three layouts, and why the third leaves speakers silent
///
/// Stereo is a straight fill. Mono averages the pair, which is a downmix and
/// loses nothing a mono device could have played anyway.
///
/// **Anything wider gets the stereo pair in its first two channels and silence
/// in the rest** — on a 5.1 device that is centre, LFE and both surrounds
/// carrying nothing. That is a deliberate floor, not an oversight, and the
/// reason is that `cpal` reports a channel *count* and not a channel *layout*:
/// `SupportedStreamConfig::channels` is a `u16`, so nothing here knows which
/// index is the centre or whether index 0 is even front-left. Writing an upmix
/// matrix against positions the API never states would be guessing, and a
/// guessed centre channel is worse than a silent one — it duplicates dialogue
/// into a speaker that may be a surround.
///
/// A real upmix wants a layout the platform states (WASAPI's channel mask,
/// CoreAudio's `AudioChannelLayout`), which means going under `cpal` or past
/// it. Until then [`AudioStream::open`] logs the layout it found so the silence
/// is diagnosable instead of merely audible.
fn fill_audio(
    data: &mut [f32],
    channels: usize,
    source: &dyn AudioSource,
    sample_rate: u32,
    scratch: &mut Vec<f32>,
) {
    // `AudioSource::fill` is additive and documented to receive a zeroed
    // buffer; cpal hands back whatever the device buffer last held.
    data.fill(0.0);

    if channels == CHANNELS {
        source.fill(data, sample_rate);
    } else if channels == 1 {
        let block = data.len();
        // The scratch is owned by the stream's callback and reused across
        // blocks: one allocation, then this resize/fill per block — no malloc
        // on the audio thread after the first block. Re-zero every block, or
        // the additive fill stacks the previous block's values in.
        scratch.resize(block * CHANNELS, 0.0);
        scratch.fill(0.0);
        source.fill(scratch, sample_rate);
        for (i, sample) in data.iter_mut().enumerate() {
            *sample = (scratch[i * 2] + scratch[i * 2 + 1]) * 0.5;
        }
    } else {
        let block = data.len() / channels;
        scratch.resize(block * CHANNELS, 0.0);
        scratch.fill(0.0);
        source.fill(scratch, sample_rate);
        for i in 0..block {
            data[i * channels] = scratch[i * CHANNELS];
            data[i * channels + 1] = scratch[i * CHANNELS + 1];
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

    /// **The buffer a source is handed is stereo, at the rate it was told.**
    ///
    /// The observations are made on the stream's polling thread and read back
    /// here, rather than asserted where they are taken. A `fill` that panics
    /// takes down that thread and nobody else: this test used to assert inside
    /// `fill`, and with `assert_eq!(rate, 1)` written into it, it still passed —
    /// three assertions wired to nothing, which is worse than none, because a
    /// green light says somebody checked.
    ///
    /// So `fill` records and the deadline loop below is what fails: first that
    /// a fill happened at all, then what it saw. The loop is
    /// `the_null_stream_fills_its_source_until_it_is_dropped`'s, for the same
    /// reason — a fixed sleep either decides the test on a loaded machine or
    /// makes it slow on every other one.
    #[test]
    fn source_fill_receives_stereo_buffer() {
        use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
        use std::time::{Duration, Instant};

        /// Generous enough that a loaded CI machine does not decide it, short
        /// enough that a stream that never fills fails rather than hangs.
        const DEADLINE: Duration = Duration::from_secs(5);
        /// One poll of the stream's own block period.
        const POLL: Duration = Duration::from_millis(1);

        #[derive(Default)]
        struct Seen {
            fills: AtomicUsize,
            rate: AtomicUsize,
            len: AtomicUsize,
        }

        struct CheckSource(Arc<Seen>);
        impl AudioSource for CheckSource {
            fn fill(&self, buffer: &mut [AudioSample], rate: u32) {
                self.0.rate.store(rate as usize, Relaxed);
                self.0.len.store(buffer.len(), Relaxed);
                // Last, so a reader that sees a non-zero count is looking at
                // values this same call already stored.
                self.0.fills.fetch_add(1, Relaxed);
            }
        }

        let seen = Arc::new(Seen::default());
        let stream = AudioStream::open_null(CheckSource(Arc::clone(&seen)));

        let deadline = Instant::now() + DEADLINE;
        while seen.fills.load(Relaxed) == 0 {
            assert!(
                Instant::now() < deadline,
                "the null stream never called its source, so nothing below was observed"
            );
            std::thread::sleep(POLL);
        }
        drop(stream);

        assert_eq!(
            seen.rate.load(Relaxed),
            INTERNAL_SAMPLE_RATE as usize,
            "the source was told a rate that is not the one the stream runs at"
        );
        let len = seen.len.load(Relaxed);
        assert_ne!(len, 0, "the source was handed an empty buffer");
        assert_eq!(
            len % CHANNELS,
            0,
            "a {len}-sample buffer is not a whole number of {CHANNELS}-channel frames, so \
             the playhead and the device disagree about where a frame starts"
        );
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
            let mut scratch = Vec::new();
            fill_audio(&mut data, channels, &Silent, 48_000, &mut scratch);
            assert!(
                data.iter().all(|s| *s == 0.0),
                "stale samples left for {channels} channels: {data:?}"
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_wider_device_gets_the_stereo_pair_and_documented_silence() {
        /// A source that writes a different constant into each of the two
        /// channels, so the test can tell "the pair landed" from "something
        /// landed" — a source with one value everywhere would pass a routing
        /// that fed the same sample to both.
        struct Split;
        impl AudioSource for Split {
            fn fill(&self, buffer: &mut [AudioSample], _rate: u32) {
                for frame in buffer.chunks_exact_mut(CHANNELS) {
                    frame[0] += 0.25;
                    frame[1] += -0.5;
                }
            }
        }

        // 5.1 and 7.1, the two layouts a desktop actually reports.
        for channels in [6usize, 8] {
            let frames = 8;
            let mut data = vec![0.7f32; frames * channels];
            let mut scratch = Vec::new();
            fill_audio(&mut data, channels, &Split, 48_000, &mut scratch);

            for (index, frame) in data.chunks_exact(channels).enumerate() {
                assert!(
                    (frame[0] - 0.25).abs() < 1e-6 && (frame[1] + 0.5).abs() < 1e-6,
                    "frame {index} of {channels} channels lost the stereo pair: {frame:?}"
                );
                assert!(
                    frame[CHANNELS..].iter().all(|s| *s == 0.0),
                    "frame {index} of {channels} channels: the channels past the stereo pair \
                     are documented silent, and this one carries {:?}. If that is now a real \
                     upmix, `fill_audio`'s docs are the thing to change first — they say why \
                     guessing a layout cpal does not report is worse than silence.",
                    &frame[CHANNELS..]
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn mono_and_multichannel_fills_reuse_the_scratch_correctly() {
        // The observable here is the down/up-mix's correctness across scratch
        // reuse, not the allocation itself: a scratch that is reused but not
        // re-zeroed makes `AudioSource`'s additive fill double the previous
        // block's value into the next one.
        let source = DcSource::new(0.25);
        let mut scratch = Vec::new();

        // Mono, large block first then a smaller one: the scratch shrinks but
        // keeps its capacity, so a missing re-zero would leave the first
        // block's 0.25 in the buffer for the additive fill to stack on — 0.5
        // out instead of 0.25. Downmix is (L + R) * 0.5 = 0.25.
        let mut mono = vec![0.0f32; 16];
        fill_audio(&mut mono, 1, &source, 48_000, &mut scratch);
        for &s in &mono {
            assert_eq!(s, 0.25, "mono, first block: {s}");
        }
        let mut mono_small = vec![0.0f32; 4];
        fill_audio(&mut mono_small, 1, &source, 48_000, &mut scratch);
        for &s in &mono_small {
            assert_eq!(s, 0.25, "mono, reused scratch leaked a stale value: {s}");
        }

        // Six channels, small block first then a larger one so the scratch
        // must *grow* across calls. Only the first two channels carry the
        // downmix; channels 2..=5 must be the zeroing `fill_audio` does on
        // the device buffer every block.
        let mut six_small = vec![7.0f32; 6 * 4];
        fill_audio(&mut six_small, 6, &source, 48_000, &mut scratch);
        let mut six_large = vec![7.0f32; 6 * 20];
        fill_audio(&mut six_large, 6, &source, 48_000, &mut scratch);
        for (name, data) in [("small", &six_small), ("large", &six_large)] {
            for frame in data.chunks_exact(6) {
                assert_eq!(frame[0], 0.25, "{name}: left channel: {frame:?}");
                assert_eq!(frame[1], 0.25, "{name}: right channel: {frame:?}");
                assert!(
                    frame[2..].iter().all(|s| *s == 0.0),
                    "{name}: extra channels not zeroed: {frame:?}",
                );
            }
        }
    }

    /// The null stream really pulls on its source, and dropping it really
    /// stops the pulling — the two halves that make it a stand-in for a device
    /// rather than a constructor that returns.
    ///
    /// Both are observable only through a source that counts its own fills,
    /// and the source is reachable afterwards only because `Arc<T>` is itself
    /// an [`AudioSource`]. This used to sleep 20 ms and drop, which observed
    /// neither half — and a fixed sleep is what
    /// `docs/plan/12-testing.md` says not to wait with. Both waits below poll
    /// for the condition against a deadline instead.
    #[test]
    fn the_null_stream_fills_its_source_until_it_is_dropped() {
        use std::sync::atomic::Ordering::Relaxed;
        use std::time::{Duration, Instant};

        /// Generous enough that a loaded CI machine does not decide it, short
        /// enough that a stream that never fills fails rather than hangs.
        const DEADLINE: Duration = Duration::from_secs(5);
        /// One poll of the stream's own block period, which is a fraction of a
        /// millisecond of audio at [`INTERNAL_SAMPLE_RATE`].
        const POLL: Duration = Duration::from_millis(1);
        /// Long enough for a live polling thread to have filled again, so two
        /// equal readings mean it has stopped and not that it was slow.
        const SETTLE: Duration = Duration::from_millis(50);

        let source = Arc::new(DcSource::new(0.0));
        let stream = AudioStream::open_null(Arc::clone(&source));

        let deadline = Instant::now() + DEADLINE;
        while source.fill_count.load(Relaxed) == 0 {
            assert!(
                Instant::now() < deadline,
                "the null stream never called its source"
            );
            std::thread::sleep(POLL);
        }

        drop(stream);

        // The thread checks the liveness handle once per block, so it may fill
        // one more time; what it must not do is keep going.
        let deadline = Instant::now() + DEADLINE;
        let settled = loop {
            let before = source.fill_count.load(Relaxed);
            std::thread::sleep(SETTLE);
            let after = source.fill_count.load(Relaxed);
            if before == after {
                break after;
            }
            assert!(
                Instant::now() < deadline,
                "the null stream kept filling {} blocks after it was dropped",
                after - before
            );
        };
        assert!(settled > 0, "the loop above waited for at least one fill");
    }
}
