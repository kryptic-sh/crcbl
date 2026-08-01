//! AudioWorklet output for the browser (P5.5).
//!
//! The browser owns the pull. An `AudioWorkletProcessor` is called on the
//! audio thread once per render quantum and asks this module for a block of
//! frames; [`WebAudioOutput::render`] runs the engine's
//! [`AudioSource`] and leaves the result in wasm memory for the worklet to
//! copy out. Nothing here blocks, allocates on the steady path, or panics.
//!
//! # Why there is no `SharedArrayBuffer`
//!
//! `docs/plan/10-wasm-webgpu.md`'s *Correction (design review, 2026-07-27)*
//! settles this: GitHub Pages cannot set the COOP/COEP headers that
//! `SharedArrayBuffer` requires, so the deploy target has none — and an
//! AudioWorklet feed built on an SAB ring buffer would work on a dev server
//! and be silent on the site the phase gate names.
//!
//! The design that needs no shared memory is the one where **the audio thread
//! and the wasm instance are the same thread**: an `AudioWorkletGlobalScope`
//! can instantiate a `WebAssembly.Module` and call its exports synchronously
//! from `process()`. There is then no cross-thread buffer to share, no
//! producer/consumer ring, and no `Atomics.wait` — the pull is an ordinary
//! function call, and this module is an ordinary Rust function behind it.
//! [Shape B](#shape-b-main-thread-render--transfer-queue) below covers the
//! fallback for pages that cannot instantiate in the worklet; it uses
//! `postMessage` with transferred buffers, still no `SharedArrayBuffer`, and
//! the exact same wasm-side ABI.
//!
//! # Sample rate and block size
//!
//! The browser dictates both and neither matches the engine:
//!
//! - `AudioContext.sampleRate` is whatever the OS device runs at. 48 kHz is
//!   typical, 44.1 kHz is common, and nothing in the spec promises either.
//! - The worklet's render quantum is [`RENDER_QUANTUM_FRAMES`] (128) frames,
//!   an order of magnitude smaller than the 256–1024 a cpal callback asks for.
//!
//! The engine's mixer has one internal rate, [`INTERNAL_SAMPLE_RATE`], and its
//! voices play back sample data authored for it — feeding a 48 kHz asset to a
//! 44.1 kHz device by simply passing the device rate down would detune every
//! sound by 147 cents and break the pitch cues rules 3 and 4 of the cue grammar
//! depend on. So the source is **always** driven at [`INTERNAL_SAMPLE_RATE`],
//! and this module resamples to the device rate with a linear interpolator
//! whose phase and carry frames persist across blocks (no discontinuity at a
//! block boundary, no frame dropped or repeated between them). When the two
//! rates are equal the interpolator is bit-exact pass-through — the fraction is
//! zero on every frame — so the common case costs nothing but a copy.
//!
//! Block size is handled by decoupling it from the *source* block: a
//! 128-frame request at 44.1 kHz consumes ~139.4 source frames, and the
//! fractional remainder is carried, not rounded away.
//!
//! # The JS↔wasm contract
//!
//! Everything below is the JS shim slice's half of the job. Symbols are
//! exported from the wasm module by these exact names, with
//! `#[unsafe(no_mangle)]` applied **only** on `wasm32` — a native binary that
//! links the engine does not export `__crcbl_web_*`, which was a review finding
//! against `crcbl-shell`'s equivalent module.
//!
//! ## Exports
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_web_audio_configure`](shim::__crcbl_web_audio_configure) | `(f64, i32) -> i32` | `(sampleRate, maxFrames)`. Allocates the buffers. `1` on success, `0` on rejection. Idempotent: re-calling with the same arguments keeps the resampler's phase. |
//! | [`__crcbl_web_audio_render`](shim::__crcbl_web_audio_render) | `(i32) -> i32` | Renders up to `frames` frames. Returns how many are actually valid. |
//! | [`__crcbl_web_audio_channel`](shim::__crcbl_web_audio_channel) | `(i32) -> i32` | Byte offset into wasm memory of channel `c`'s planar `f32` block, or `0`. |
//! | [`__crcbl_web_audio_channels`](shim::__crcbl_web_audio_channels) | `() -> i32` | Channel count. Always [`CHANNELS`] (2), but read it rather than assume it. |
//! | [`__crcbl_web_audio_max_frames`](shim::__crcbl_web_audio_max_frames) | `() -> i32` | The configured block capacity, or `0` when unconfigured. |
//! | [`__crcbl_web_audio_sample_rate`](shim::__crcbl_web_audio_sample_rate) | `() -> f64` | The device rate in force, or `0.0` when unconfigured. |
//! | [`__crcbl_web_audio_render_rate`](shim::__crcbl_web_audio_render_rate) | `() -> i32` | [`INTERNAL_SAMPLE_RATE`]. Diagnostics only — the resampling is this side's job. |
//! | [`__crcbl_web_audio_underruns`](shim::__crcbl_web_audio_underruns) | `() -> i32` | Saturating count of blocks that came back short. |
//! | [`__crcbl_web_audio_stop`](shim::__crcbl_web_audio_stop) | `() -> i32` | Drops the output and its buffers. `1` if there was one. |
//!
//! An export the shim is **not** given: there is no `play(sound_id)` here.
//! What is playing is the engine's business, posted to the worklet by the
//! application over `port.postMessage` and turned into voices by the
//! [`AudioSource`] this module drives.
//!
//! ## Memory ownership
//!
//! Every buffer belongs to wasm. `__crcbl_web_audio_channel` hands out an
//! offset into the module's linear memory; JS **reads** `frames` `f32`s from
//! it and never writes. The offsets are stable from a successful `configure`
//! until the next `configure` that actually changes something, or until
//! `stop` — `render` never reallocates, so a view built once per `configure`
//! survives every block.
//!
//! One caveat that is a real bug when missed: a `Float32Array` over
//! `memory.buffer` is **detached** if wasm memory grows. A worklet that caches
//! the view must compare `view.buffer !== memory.buffer` each block and rebuild
//! when they differ. Rebuilding unconditionally is also fine — it is a few
//! nanoseconds against 2.7 ms of audio.
//!
//! ## Call ordering
//!
//! 1. The wasm instance exists and the application has installed its source —
//!    [`install`], which [`AudioStream::open`](crate::AudioStream::open) calls
//!    for you on `wasm32`. Until then every call below answers `0`.
//! 2. `__crcbl_web_audio_configure(sampleRate, maxFrames)` — once, from the
//!    processor's constructor rather than from `process()`, because it is the
//!    one call that allocates.
//! 3. Per `process()`: `__crcbl_web_audio_render(quantum)` → for each channel
//!    `__crcbl_web_audio_channel(c)` → copy.
//! 4. `__crcbl_web_audio_stop()` when the node goes away.
//!
//! `configure` may be repeated — a device change reconfigures — and is a no-op
//! when the arguments are unchanged, so calling it at the top of `process()` is
//! harmless if that is easier to arrange.
//!
//! ## Underrun
//!
//! `render` returns the number of frames it produced, which is **less** than
//! requested when the output is unconfigured, when `frames` exceeds the
//! configured `maxFrames`, or when `frames` is zero. The worklet's obligation
//! is then fixed: **zero-fill the remainder of the quantum**. It must not
//! repeat the previous block (a buzz), and must not return `false` from
//! `process()` (that tears the node down for a transient). Every short block
//! increments `__crcbl_web_audio_underruns`, which is what a debug HUD should
//! show rather than the audio glitching silently.
//!
//! ## Shape A: instantiate in the worklet (primary)
//!
//! ```text
//! // main thread
//! const mod = await WebAssembly.compileStreaming(fetch('crcbl.wasm'));
//! const ctx = new AudioContext();
//! await ctx.audioWorklet.addModule('crcbl-audio-worklet.js');
//! const node = new AudioWorkletNode(ctx, 'crcbl-audio', {
//!   numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2],
//! });
//! node.connect(ctx.destination);
//! node.port.postMessage({ module: mod, sampleRate: ctx.sampleRate });
//!
//! // crcbl-audio-worklet.js, inside AudioWorkletGlobalScope
//! class CrcblAudio extends AudioWorkletProcessor {
//!   constructor() {
//!     super();
//!     this.port.onmessage = (e) => {
//!       // Synchronous instantiation of an already-compiled Module is allowed.
//!       this.inst = new WebAssembly.Instance(e.data.module, imports);
//!       this.mem  = this.inst.exports.memory;
//!       this.ex   = this.inst.exports;
//!       this.ex.__crcbl_web_audio_boot();          // the app's own export
//!       this.ex.__crcbl_web_audio_configure(sampleRate, 128);
//!       this.chans = this.ex.__crcbl_web_audio_channels();
//!     };
//!   }
//!   process(_inputs, outputs) {
//!     const out = outputs[0];
//!     if (!this.ex) { for (const c of out) c.fill(0); return true; }
//!     const want = out[0].length;
//!     const got  = this.ex.__crcbl_web_audio_render(want);
//!     const heap = new Float32Array(this.mem.buffer);   // cheap; never stale
//!     for (let c = 0; c < out.length; c++) {
//!       if (c < this.chans && got > 0) {
//!         const off = this.ex.__crcbl_web_audio_channel(c) >> 2;
//!         out[c].set(heap.subarray(off, off + got));
//!       }
//!       out[c].fill(0, got);                           // the underrun rule
//!     }
//!     return true;
//!   }
//! }
//! registerProcessor('crcbl-audio', CrcblAudio);
//! ```
//!
//! `__crcbl_web_audio_boot` is **the application's** export, not this crate's:
//! only the app knows what its [`AudioSource`] is. It constructs one and calls
//! [`AudioStream::open`](crate::AudioStream::open) (or [`install`] directly).
//! The name is a convention this doc suggests so the shim can be written before
//! the app.
//!
//! `sampleRate` and `currentTime` are globals of `AudioWorkletGlobalScope`;
//! `sampleRate` there is the `AudioContext`'s and is what `configure` wants.
//!
//! ## Shape B: main-thread render + transfer queue
//!
//! For a page that cannot instantiate the module inside the worklet (a
//! `wasm-bindgen`-shaped bundle whose imports the worklet scope cannot supply,
//! say), the same exports are called on the **main** thread and the audio
//! crosses by `postMessage` with a transferred `ArrayBuffer`:
//!
//! - The worklet keeps a small FIFO of `Float32Array` blocks and drains 128
//!   frames per `process()`, zero-filling on empty — the same underrun rule.
//! - Each `process()` posts `{ want }` back; the main thread answers with
//!   `render` + a copy out of wasm memory, transferring the copy.
//! - It needs a lead of at least two quanta (~5.3 ms) because the main thread
//!   may be mid-frame; that latency is the price of not having `SharedArrayBuffer`,
//!   and is why shape A is the primary.
//!
//! Nothing on the Rust side changes between the two: `render(frames)` is the
//! whole seam, which is exactly why it is shaped as a pull rather than as a
//! callback the device owns.

use std::cell::RefCell;
use std::fmt;

use crate::{AudioSample, AudioSource, CHANNELS, INTERNAL_SAMPLE_RATE};

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// The Web Audio render quantum: `process()` is always called for this many
/// frames.
///
/// Fixed by the spec, and small — 2.67 ms at 48 kHz. It is a default, not an
/// assumption: [`WebAudioOutput::configure`] takes the block size the caller
/// actually saw, and `render` is told the frame count on every call.
pub const RENDER_QUANTUM_FRAMES: usize = 128;

/// The largest block [`WebAudioOutput::configure`] will size buffers for.
///
/// A bound rather than a limit anyone should hit: it exists so a nonsense
/// `maxFrames` from JS cannot ask wasm to allocate arbitrarily much.
pub const MAX_BLOCK_FRAMES: usize = 8_192;

/// The lowest device sample rate accepted.
pub const MIN_DEVICE_SAMPLE_RATE: u32 = 8_000;

/// The highest device sample rate accepted.
pub const MAX_DEVICE_SAMPLE_RATE: u32 = 384_000;

/// Source frames the resampler keeps between blocks.
///
/// Two: the frame the next output position sits on, and — when the device rate
/// is above [`INTERNAL_SAMPLE_RATE`], so one source frame spans several output
/// frames — the one after it, which the next block interpolates towards. Losing
/// that second frame instead of carrying it would drop a source sample at every
/// block boundary.
const MAX_CARRY_FRAMES: usize = 2;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why [`WebAudioOutput::configure`] refused.
///
/// Both cases mean the shim passed something the browser does not report, so
/// the honest answer is to stay unconfigured and let `render` report underruns,
/// rather than to guess a rate and detune everything.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum WebAudioError {
    /// `sampleRate` was not finite, or outside
    /// `[MIN_DEVICE_SAMPLE_RATE, MAX_DEVICE_SAMPLE_RATE]`.
    SampleRate {
        /// What was asked for.
        requested: f64,
    },
    /// `maxFrames` was zero or above [`MAX_BLOCK_FRAMES`].
    BlockSize {
        /// What was asked for.
        requested: usize,
    },
}

impl fmt::Display for WebAudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SampleRate { requested } => write!(
                f,
                "device sample rate {requested} is not in \
                 [{MIN_DEVICE_SAMPLE_RATE}, {MAX_DEVICE_SAMPLE_RATE}]"
            ),
            Self::BlockSize { requested } => write!(
                f,
                "block size {requested} is not in [1, {MAX_BLOCK_FRAMES}]"
            ),
        }
    }
}

impl std::error::Error for WebAudioError {}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Everything sized by a [`WebAudioOutput::configure`] call.
///
/// Separate from the output itself so that "unconfigured" is a state the type
/// system knows about: before the worklet reports a rate there is nothing to
/// render into, and `render` says so by returning zero rather than by inventing
/// a rate.
#[derive(Debug)]
struct Config {
    /// `AudioContext.sampleRate`, rounded to an integer.
    device_rate: u32,
    /// The largest block `render` will produce.
    max_frames: usize,
    /// Source frames consumed per output frame:
    /// [`INTERNAL_SAMPLE_RATE`] / `device_rate`.
    step: f64,
    /// Interleaved source frames for the block being rendered. Sized once,
    /// never grown; `render` writes into a prefix of it.
    scratch: Vec<AudioSample>,
    /// How many frames `scratch` can hold.
    scratch_frames: usize,
    /// Source frames carried from the previous block, interleaved.
    carry: [AudioSample; MAX_CARRY_FRAMES * CHANNELS],
    /// How many of `carry`'s frames are live: 1 or 2.
    carry_frames: usize,
    /// Where the next output frame sits between `carry[0]` and `carry[1]`,
    /// in `[0, 1)`.
    phase: f64,
    /// De-interleaved output, one `Vec` per channel. Planar because that is
    /// what `outputs[0][c]` is, and de-interleaving here turns the worklet's
    /// per-sample JS loop into one `set()` per channel.
    planar: [Vec<AudioSample>; CHANNELS],
    /// Frames of `planar` the last `render` filled.
    rendered: usize,
}

// ---------------------------------------------------------------------------
// WebAudioOutput
// ---------------------------------------------------------------------------

/// The browser's end of the [`AudioSource`] seam: a pull the worklet drives.
///
/// Owns the source, the resampler state, and the buffers the worklet copies
/// out of. Plain Rust with no browser in it — which is the point, because it
/// means the pull path is driven directly from tests on a machine with no
/// audio device at all.
pub struct WebAudioOutput {
    source: Box<dyn AudioSource>,
    config: Option<Config>,
    underruns: u64,
}

impl fmt::Debug for WebAudioOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebAudioOutput")
            .field("device_sample_rate", &self.device_sample_rate())
            .field("max_frames", &self.max_frames())
            .field("underruns", &self.underruns)
            .finish_non_exhaustive()
    }
}

impl WebAudioOutput {
    /// Wrap `source`. Nothing is allocated until [`configure`](Self::configure)
    /// — the browser has not said what rate it runs at yet.
    #[must_use]
    pub fn new(source: impl AudioSource) -> Self {
        Self {
            source: Box::new(source),
            config: None,
            underruns: 0,
        }
    }

    /// Adopt the browser's sample rate and block size.
    ///
    /// The one call that allocates, which is why the contract asks the worklet
    /// to make it from the processor's constructor. Idempotent: repeating it
    /// with the arguments already in force keeps the buffers *and the
    /// resampler's phase*, so a shim that calls it every block does not restart
    /// the interpolator 375 times a second.
    ///
    /// # Errors
    ///
    /// [`WebAudioError`] when `sample_rate` is not a rate a device reports, or
    /// `max_frames` is zero or absurd. The output stays in whatever state it
    /// was in; it does not become unconfigured.
    pub fn configure(&mut self, sample_rate: f64, max_frames: usize) -> Result<(), WebAudioError> {
        let rounded = if sample_rate.is_finite() {
            sample_rate.round()
        } else {
            return Err(WebAudioError::SampleRate {
                requested: sample_rate,
            });
        };
        if rounded < f64::from(MIN_DEVICE_SAMPLE_RATE)
            || rounded > f64::from(MAX_DEVICE_SAMPLE_RATE)
        {
            return Err(WebAudioError::SampleRate {
                requested: sample_rate,
            });
        }
        if max_frames == 0 || max_frames > MAX_BLOCK_FRAMES {
            return Err(WebAudioError::BlockSize {
                requested: max_frames,
            });
        }
        // Range-checked against MIN/MAX_DEVICE_SAMPLE_RATE just above.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let device_rate = rounded as u32;

        if self
            .config
            .as_ref()
            .is_some_and(|cfg| cfg.device_rate == device_rate && cfg.max_frames == max_frames)
        {
            return Ok(());
        }

        let step = f64::from(INTERNAL_SAMPLE_RATE) / f64::from(device_rate);
        // `render` needs at most `floor(phase + frames * step) + 2` source
        // frames, and `phase < 1`, so this bound holds for every block it will
        // ever be asked for. Sized once so the steady path never allocates.
        // `max_frames <= MAX_BLOCK_FRAMES` and `step <= 6`, so the product is
        // small and positive.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let scratch_frames = (max_frames as f64 * step).ceil() as usize + MAX_CARRY_FRAMES + 1;

        self.config = Some(Config {
            device_rate,
            max_frames,
            step,
            scratch: vec![0.0; scratch_frames * CHANNELS],
            scratch_frames,
            carry: [0.0; MAX_CARRY_FRAMES * CHANNELS],
            // One frame of silence to interpolate away from; the alternative is
            // a first block that reads an uninitialised "previous" frame.
            carry_frames: 1,
            phase: 0.0,
            planar: std::array::from_fn(|_| vec![0.0; max_frames]),
            rendered: 0,
        });
        Ok(())
    }

    /// The device rate in force, or `None` before [`configure`](Self::configure).
    #[must_use]
    pub fn device_sample_rate(&self) -> Option<u32> {
        self.config.as_ref().map(|cfg| cfg.device_rate)
    }

    /// The rate the [`AudioSource`] is driven at — always
    /// [`INTERNAL_SAMPLE_RATE`], whatever the device runs at.
    #[must_use]
    pub const fn render_sample_rate(&self) -> u32 {
        INTERNAL_SAMPLE_RATE
    }

    /// Whether the device rate differs from [`INTERNAL_SAMPLE_RATE`], so the
    /// interpolator is doing real work rather than passing frames through.
    #[must_use]
    pub fn is_resampling(&self) -> bool {
        self.config
            .as_ref()
            .is_some_and(|cfg| cfg.device_rate != INTERNAL_SAMPLE_RATE)
    }

    /// The configured block capacity, or `None` before
    /// [`configure`](Self::configure).
    #[must_use]
    pub fn max_frames(&self) -> Option<usize> {
        self.config.as_ref().map(|cfg| cfg.max_frames)
    }

    /// How many blocks came back short. See the module docs' underrun rule.
    #[must_use]
    pub const fn underruns(&self) -> u64 {
        self.underruns
    }

    /// Channel `channel` of the last [`render`](Self::render), `frames` long.
    ///
    /// Empty when the channel does not exist, when nothing has been rendered,
    /// or before [`configure`](Self::configure). This is the safe Rust twin of
    /// [`shim::__crcbl_web_audio_channel`], and what the tests read.
    #[must_use]
    pub fn channel(&self, channel: usize) -> &[AudioSample] {
        match &self.config {
            Some(cfg) if channel < CHANNELS => &cfg.planar[channel][..cfg.rendered],
            _ => &[],
        }
    }

    /// Render up to `frames` frames and return how many are valid.
    ///
    /// The whole seam: shape A calls this from `process()` on the audio thread,
    /// shape B calls it on the main thread. It never allocates, never locks
    /// anything the caller does not already hold, and never panics — a short
    /// return is how it reports every failure, because the audio thread has
    /// nowhere to put an error.
    ///
    /// The source is filled at [`INTERNAL_SAMPLE_RATE`] and the result
    /// resampled to the device rate; see the module docs.
    pub fn render(&mut self, frames: usize) -> usize {
        // Disjoint field borrows: `config` here, `source` below, `underruns`
        // after. Nothing hands out a `&mut` to shared state.
        let Some(cfg) = self.config.as_mut() else {
            self.underruns = self.underruns.saturating_add(1);
            return 0;
        };
        let requested = frames;
        let frames = frames.min(cfg.max_frames);
        if frames == 0 {
            cfg.rendered = 0;
            self.underruns = self.underruns.saturating_add(1);
            return 0;
        }

        let step = cfg.step;
        let phase = cfg.phase;
        // Positions are computed from `phase` rather than accumulated per
        // frame, so rounding cannot drift within a block.
        let last = phase + (frames - 1) as f64 * step;
        let end = phase + frames as f64 * step;
        // Source frames needed: enough to interpolate the last output frame
        // (`floor(last) + 1`), and enough to reach the frame the next block
        // starts from (`floor(end)`). `+ 1` turns the highest index into a
        // count.
        let highest = last.floor().max(end.floor() - 1.0) + 1.0;
        // `phase >= 0` and `step > 0`, so this is positive; the bound against
        // the scratch buffer is checked immediately below.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let total = (highest + 1.0) as usize;

        if total > cfg.scratch_frames {
            // Unreachable given `configure`'s sizing; treated as an underrun
            // rather than as an index panic, because this runs on the audio
            // thread.
            debug_assert!(total <= cfg.scratch_frames, "scratch undersized");
            cfg.rendered = 0;
            self.underruns = self.underruns.saturating_add(1);
            return 0;
        }

        // Seed the block with the frames carried across the boundary, then ask
        // the source for the rest. `fill` is additive over a zeroed buffer —
        // the same contract the cpal path keeps.
        let carried = cfg.carry_frames * CHANNELS;
        cfg.scratch[..carried].copy_from_slice(&cfg.carry[..carried]);
        let tail = &mut cfg.scratch[carried..total * CHANNELS];
        tail.fill(0.0);
        self.source.fill(tail, INTERNAL_SAMPLE_RATE);

        // Resample and de-interleave in one pass.
        let scratch = &cfg.scratch;
        let (left, right) = cfg.planar.split_at_mut(1);
        let (left, right) = (&mut left[0], &mut right[0]);
        for i in 0..frames {
            let position = phase + i as f64 * step;
            // `position >= 0` and `floor(position) + 1 < total`, which is what
            // `highest` is for; the fraction is in `[0, 1)`.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let index = position as usize;
            #[allow(clippy::cast_possible_truncation)]
            let fraction = (position - index as f64) as f32;
            let base = index * CHANNELS;
            let (l0, r0) = (scratch[base], scratch[base + 1]);
            let (l1, r1) = (scratch[base + CHANNELS], scratch[base + CHANNELS + 1]);
            left[i] = sane(l0 + (l1 - l0) * fraction);
            right[i] = sane(r0 + (r1 - r0) * fraction);
        }

        // Carry the tail of the block forward. `end`'s integer part is the
        // frame the next block's `phase` is measured from.
        // `end > 0` and `floor(end) <= total - 1` by the `highest` bound.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let base = end.floor() as usize;
        cfg.phase = end - end.floor();
        let carry_frames = (total - base).min(MAX_CARRY_FRAMES);
        debug_assert!(
            total - base <= MAX_CARRY_FRAMES,
            "carry longer than the resampler's lookahead"
        );
        let start = total - carry_frames;
        cfg.carry[..carry_frames * CHANNELS]
            .copy_from_slice(&scratch[start * CHANNELS..total * CHANNELS]);
        cfg.carry_frames = carry_frames;
        cfg.rendered = frames;

        if frames < requested {
            self.underruns = self.underruns.saturating_add(1);
        }
        frames
    }
}

/// Keep a sample finite and in range.
///
/// The mixer already clips, but [`AudioSource`] is a public trait and an
/// implementation that emits a NaN must produce silence in the device rather
/// than whatever a browser does with a non-finite float. `f32::clamp` alone
/// returns NaN for NaN, which is the bug this shape avoids — the same one the
/// mixer's own clip pass was fixed for.
fn sane(sample: AudioSample) -> AudioSample {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------------
// The installed output
// ---------------------------------------------------------------------------

thread_local! {
    /// The output the `extern "C"` entry points reach, or `None` when the
    /// application has not installed one.
    ///
    /// Thread-local rather than a `static mut` for the same reason
    /// `crcbl-shell`'s bridge is: in shape A this lives on the audio thread and
    /// in shape B on the main thread, and neither should be able to see the
    /// other's. `RefCell` because the entry points need `&mut` and there is no
    /// `&mut` to hand out from a `static`.
    static OUTPUT: RefCell<Option<WebAudioOutput>> = const { RefCell::new(None) };
}

/// Install `source` as this thread's browser audio output.
///
/// Returns `false` if one is already installed — the browser gives a page one
/// `AudioContext` destination worth caring about, and silently replacing a live
/// output would strand whatever the worklet is mid-block on.
/// [`AudioStream::open`](crate::AudioStream::open) calls this on `wasm32`.
pub fn install(source: impl AudioSource) -> bool {
    OUTPUT.with(|slot| {
        let Ok(mut slot) = slot.try_borrow_mut() else {
            return false;
        };
        if slot.is_some() {
            return false;
        }
        *slot = Some(WebAudioOutput::new(source));
        true
    })
}

/// Drop this thread's output and its buffers.
///
/// Returns whether there was one. Every entry point answers `0` afterwards, so
/// a worklet that is mid-block gets an underrun and zero-fills rather than
/// reading a buffer that is being freed.
pub fn uninstall() -> bool {
    OUTPUT.with(|slot| match slot.try_borrow_mut() {
        Ok(mut slot) => slot.take().is_some(),
        Err(_) => false,
    })
}

/// Whether an output is installed on this thread.
#[must_use]
pub fn is_installed() -> bool {
    OUTPUT.with(|slot| slot.try_borrow().is_ok_and(|slot| slot.is_some()))
}

/// Run `f` against the installed output, or answer `absent`.
///
/// `try_borrow_mut` rather than `borrow_mut`: an [`AudioSource`] that reached
/// back into an entry point would otherwise panic inside `process()`, and this
/// path is not allowed to panic. Re-entry answers "no output", which the caller
/// already has to handle.
fn with_output<R>(absent: R, f: impl FnOnce(&mut WebAudioOutput) -> R) -> R {
    OUTPUT.with(|slot| match slot.try_borrow_mut() {
        Ok(mut slot) => match slot.as_mut() {
            Some(output) => f(output),
            None => absent,
        },
        Err(_) => absent,
    })
}

// ---------------------------------------------------------------------------
// extern "C" entry points — called from JS
// ---------------------------------------------------------------------------

/// The JS→wasm ABI. See the [module docs](self) for the full contract.
///
/// `#[unsafe(no_mangle)]` only on `wasm32`, exactly as `crcbl-shell`'s shim
/// does it: these symbols exist for a worklet to call, and exporting
/// `__crcbl_web_audio_*` from every native binary that links the engine is
/// namespace pollution for a caller that cannot be there. The functions
/// themselves stay compiled on native so the tests below can drive them.
///
/// None of them is `unsafe`: nothing here dereferences a pointer the caller
/// supplied. [`shim::__crcbl_web_audio_channel`] *returns* an address, and
/// reading it is JS's obligation under the module docs' memory-ownership rules.
pub mod shim {
    use super::{CHANNELS, INTERNAL_SAMPLE_RATE, uninstall, with_output};

    /// Adopt `sample_rate` (`AudioContext.sampleRate`) and a block capacity of
    /// `max_frames`. Returns `1` on success, `0` if there is no output or the
    /// arguments were rejected.
    ///
    /// Idempotent for unchanged arguments; see
    /// [`WebAudioOutput::configure`](super::WebAudioOutput::configure).
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_audio_configure(sample_rate: f64, max_frames: u32) -> u32 {
        with_output(0, |output| {
            u32::from(output.configure(sample_rate, max_frames as usize).is_ok())
        })
    }

    /// Render up to `frames` frames; returns how many are valid.
    ///
    /// A return below `frames` is an underrun and the caller must zero-fill the
    /// rest of the quantum.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_audio_render(frames: u32) -> u32 {
        with_output(0, |output| {
            u32::try_from(output.render(frames as usize)).unwrap_or(u32::MAX)
        })
    }

    /// The address in wasm memory of channel `channel`'s planar block, or `0`.
    ///
    /// Valid for the frame count [`__crcbl_web_audio_render`] returned, and
    /// stable until the next `configure` that changes something or
    /// [`__crcbl_web_audio_stop`]. Read-only for JS.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_audio_channel(channel: u32) -> *const f32 {
        with_output(std::ptr::null(), |output| {
            let block = output.channel(channel as usize);
            if block.is_empty() {
                std::ptr::null()
            } else {
                block.as_ptr()
            }
        })
    }

    /// The channel count: [`CHANNELS`]. Read it rather than hard-coding 2.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_audio_channels() -> u32 {
        u32::try_from(CHANNELS).unwrap_or(u32::MAX)
    }

    /// The configured block capacity, or `0` when unconfigured.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_audio_max_frames() -> u32 {
        with_output(0, |output| {
            output
                .max_frames()
                .and_then(|frames| u32::try_from(frames).ok())
                .unwrap_or(0)
        })
    }

    /// The device rate in force, or `0.0` when unconfigured.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_audio_sample_rate() -> f64 {
        with_output(0.0, |output| {
            output.device_sample_rate().map_or(0.0, f64::from)
        })
    }

    /// The rate the engine's mixer is driven at: [`INTERNAL_SAMPLE_RATE`].
    ///
    /// Diagnostics only. JS never has to resample — that is
    /// [`WebAudioOutput::render`](super::WebAudioOutput::render)'s job — but a mismatch between this and
    /// [`__crcbl_web_audio_sample_rate`] is the first thing to print when a
    /// demo sounds detuned.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_audio_render_rate() -> u32 {
        INTERNAL_SAMPLE_RATE
    }

    /// How many blocks have come back short, saturating.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_audio_underruns() -> u32 {
        with_output(0, |output| {
            u32::try_from(output.underruns()).unwrap_or(u32::MAX)
        })
    }

    /// Drop the output and its buffers. Returns `1` if there was one.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_audio_stop() -> u32 {
        u32::from(uninstall())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::{Mixer, SoundBank};

    /// A source that writes a per-frame ramp, so a resampler that skips,
    /// repeats or reorders frames is visible in the output.
    ///
    /// Left carries the ramp; right carries its negation, so a channel that
    /// leaks into the other is visible too.
    struct Ramp {
        next: std::sync::atomic::AtomicU64,
    }

    impl Ramp {
        fn new() -> Self {
            Self {
                next: std::sync::atomic::AtomicU64::new(0),
            }
        }
    }

    impl AudioSource for Ramp {
        fn fill(&self, buffer: &mut [AudioSample], sample_rate: u32) {
            assert_eq!(
                sample_rate, INTERNAL_SAMPLE_RATE,
                "the source is always driven at the engine's internal rate"
            );
            for frame in buffer.chunks_exact_mut(CHANNELS) {
                let n = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // Small enough to stay well inside the clip range.
                let value = (n % 1000) as f32 / 1000.0;
                frame[0] += value;
                frame[1] -= value;
            }
        }
    }

    /// A source that counts how many frames it has been asked for.
    struct Counting {
        frames: std::sync::atomic::AtomicUsize,
    }

    impl AudioSource for Counting {
        fn fill(&self, buffer: &mut [AudioSample], _rate: u32) {
            self.frames.fetch_add(
                buffer.len() / CHANNELS,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
    }

    #[test]
    fn nothing_renders_before_the_browser_reports_a_rate() {
        let mut output = WebAudioOutput::new(Ramp::new());
        assert_eq!(output.device_sample_rate(), None);
        assert_eq!(output.max_frames(), None);
        assert_eq!(output.render(RENDER_QUANTUM_FRAMES), 0);
        assert!(output.channel(0).is_empty());
        assert_eq!(
            output.underruns(),
            1,
            "a block with no output is an underrun"
        );
    }

    #[test]
    fn a_matched_rate_is_bit_exact_pass_through() {
        let mut output = WebAudioOutput::new(Ramp::new());
        output
            .configure(f64::from(INTERNAL_SAMPLE_RATE), RENDER_QUANTUM_FRAMES)
            .expect("48 kHz is a rate");
        assert!(!output.is_resampling());
        assert_eq!(output.render_sample_rate(), INTERNAL_SAMPLE_RATE);

        // Four blocks back to back: the ramp must come out unbroken across the
        // block boundaries, which is what the carry frames exist for.
        let mut left = Vec::new();
        for _ in 0..4 {
            assert_eq!(output.render(RENDER_QUANTUM_FRAMES), RENDER_QUANTUM_FRAMES);
            left.extend_from_slice(output.channel(0));
        }
        assert_eq!(left.len(), 4 * RENDER_QUANTUM_FRAMES);
        // The first frame is the silent carry the interpolator starts from;
        // everything after it is the source's own ramp, in order.
        assert_eq!(left[0], 0.0);
        for (i, &sample) in left.iter().enumerate().skip(1) {
            let expected = ((i as u64 - 1) % 1000) as f32 / 1000.0;
            assert!(
                (sample - expected).abs() < 1e-6,
                "frame {i}: got {sample}, expected {expected}"
            );
        }
        assert_eq!(output.underruns(), 0);
    }

    #[test]
    fn channels_stay_separate_through_the_resampler() {
        let mut output = WebAudioOutput::new(Ramp::new());
        output
            .configure(44_100.0, RENDER_QUANTUM_FRAMES)
            .expect("44.1 kHz");
        assert!(output.is_resampling());
        assert_eq!(output.render(RENDER_QUANTUM_FRAMES), RENDER_QUANTUM_FRAMES);
        let (left, right) = (output.channel(0), output.channel(1));
        assert_eq!(left.len(), RENDER_QUANTUM_FRAMES);
        for (i, (&l, &r)) in left.iter().zip(right).enumerate() {
            assert!(
                (l + r).abs() < 1e-6,
                "frame {i}: L={l} R={r} are not mirrored"
            );
        }
    }

    /// The rate mismatch this whole module exists for: a 44.1 kHz device must
    /// consume the 48 kHz stream *faster* than it emits, and the ratio must be
    /// the real one rather than 1.
    #[test]
    fn a_device_rate_below_the_internal_rate_consumes_more_frames_than_it_emits() {
        let source = std::sync::Arc::new(Counting {
            frames: std::sync::atomic::AtomicUsize::new(0),
        });
        struct Shared(std::sync::Arc<Counting>);
        impl AudioSource for Shared {
            fn fill(&self, buffer: &mut [AudioSample], rate: u32) {
                self.0.fill(buffer, rate);
            }
        }

        let mut output = WebAudioOutput::new(Shared(std::sync::Arc::clone(&source)));
        output
            .configure(44_100.0, RENDER_QUANTUM_FRAMES)
            .expect("44.1 kHz");

        let blocks = 100;
        for _ in 0..blocks {
            assert_eq!(output.render(RENDER_QUANTUM_FRAMES), RENDER_QUANTUM_FRAMES);
        }
        let consumed = source.frames.load(std::sync::atomic::Ordering::Relaxed);
        let emitted = blocks * RENDER_QUANTUM_FRAMES;
        // 48000/44100 = 1.0884; over 12800 output frames that is ~13931 source
        // frames, and the carry means the error is a couple of frames total —
        // not a couple of frames *per block*, which is what a resampler that
        // resets its phase each block would show.
        let expected = (emitted as f64 * 48_000.0 / 44_100.0).round() as usize;
        assert!(
            consumed.abs_diff(expected) <= 3,
            "consumed {consumed} source frames for {emitted} output frames, expected ~{expected}"
        );
    }

    /// The opposite mismatch: a 96 kHz device emits two output frames per
    /// source frame, so nothing may be dropped at a block boundary.
    #[test]
    fn a_device_rate_above_the_internal_rate_does_not_drop_frames_at_block_edges() {
        let mut output = WebAudioOutput::new(Ramp::new());
        output
            .configure(96_000.0, RENDER_QUANTUM_FRAMES)
            .expect("96 kHz");

        let mut left = Vec::new();
        for _ in 0..4 {
            assert_eq!(output.render(RENDER_QUANTUM_FRAMES), RENDER_QUANTUM_FRAMES);
            left.extend_from_slice(output.channel(0));
        }
        // Upsampling by exactly 2 makes every second output frame an exact
        // source frame and the ones between them the midpoint. A frame dropped
        // at a boundary shows up as a step twice the size of its neighbours.
        let mut steps: Vec<f32> = left.windows(2).map(|w| w[1] - w[0]).collect();
        // The ramp wraps at 1000 frames and starts from the silent carry, so
        // drop the outliers those two produce before looking at the rest.
        steps.retain(|step| *step > 0.0);
        assert!(
            steps.len() > 500,
            "only {} steps survived the filter — the check would pass vacuously",
            steps.len()
        );
        let nominal = 0.5 / 1000.0;
        for (i, step) in steps.iter().enumerate() {
            assert!(
                (step - nominal).abs() < 1e-6,
                "step {i} is {step}, expected {nominal} — a frame was dropped or repeated"
            );
        }
    }

    #[test]
    fn a_short_block_is_an_underrun_the_caller_can_see() {
        let mut output = WebAudioOutput::new(Ramp::new());
        output
            .configure(48_000.0, RENDER_QUANTUM_FRAMES)
            .expect("48 kHz");

        // More than was configured: the caller gets what fits and must zero
        // the rest itself.
        let got = output.render(RENDER_QUANTUM_FRAMES * 4);
        assert_eq!(got, RENDER_QUANTUM_FRAMES);
        assert_eq!(output.channel(0).len(), RENDER_QUANTUM_FRAMES);
        assert_eq!(output.underruns(), 1);

        // Zero frames is a degenerate quantum, not a crash.
        assert_eq!(output.render(0), 0);
        assert!(output.channel(0).is_empty());
        assert_eq!(output.underruns(), 2);
    }

    #[test]
    fn the_browsers_own_quantum_is_the_default_and_odd_sizes_still_work() {
        for frames in [1usize, 3, 64, RENDER_QUANTUM_FRAMES, 1024] {
            let mut output = WebAudioOutput::new(Ramp::new());
            output.configure(44_100.0, frames).expect("a rate");
            for _ in 0..8 {
                assert_eq!(output.render(frames), frames, "block of {frames}");
            }
            assert_eq!(output.underruns(), 0);
        }
    }

    #[test]
    fn a_rate_the_browser_could_not_have_reported_is_refused() {
        let mut output = WebAudioOutput::new(Ramp::new());
        for rate in [f64::NAN, f64::INFINITY, 0.0, -48_000.0, 1_000_000.0] {
            assert!(
                matches!(
                    output.configure(rate, RENDER_QUANTUM_FRAMES),
                    Err(WebAudioError::SampleRate { .. })
                ),
                "{rate} should be refused"
            );
        }
        assert!(matches!(
            output.configure(48_000.0, 0),
            Err(WebAudioError::BlockSize { .. })
        ));
        assert!(matches!(
            output.configure(48_000.0, MAX_BLOCK_FRAMES + 1),
            Err(WebAudioError::BlockSize { .. })
        ));
        // Still unconfigured, and still refusing to invent a rate.
        assert_eq!(output.device_sample_rate(), None);
        assert_eq!(output.render(RENDER_QUANTUM_FRAMES), 0);
    }

    #[test]
    fn reconfiguring_with_the_same_arguments_keeps_the_resamplers_phase() {
        let mut output = WebAudioOutput::new(Ramp::new());
        output
            .configure(44_100.0, RENDER_QUANTUM_FRAMES)
            .expect("44.1 kHz");
        output.render(RENDER_QUANTUM_FRAMES);
        let first: Vec<f32> = output.channel(0).to_vec();

        // A shim that calls `configure` every block must not restart the
        // interpolator; the second block has to continue the first.
        output
            .configure(44_100.0, RENDER_QUANTUM_FRAMES)
            .expect("idempotent");
        output.render(RENDER_QUANTUM_FRAMES);
        let second: Vec<f32> = output.channel(0).to_vec();
        assert_ne!(first, second, "phase was reset — the block repeated");

        // A different rate does resize, and starts clean.
        output
            .configure(48_000.0, RENDER_QUANTUM_FRAMES)
            .expect("48 kHz");
        assert_eq!(output.device_sample_rate(), Some(48_000));
        assert!(!output.is_resampling());
    }

    #[test]
    fn a_non_finite_source_produces_silence_rather_than_a_non_finite_output() {
        struct Nasty;
        impl AudioSource for Nasty {
            fn fill(&self, buffer: &mut [AudioSample], _rate: u32) {
                for (i, sample) in buffer.iter_mut().enumerate() {
                    *sample = if i % 2 == 0 { f32::NAN } else { 1.0e30 };
                }
            }
        }
        let mut output = WebAudioOutput::new(Nasty);
        output
            .configure(44_100.0, RENDER_QUANTUM_FRAMES)
            .expect("44.1 kHz");
        assert_eq!(output.render(RENDER_QUANTUM_FRAMES), RENDER_QUANTUM_FRAMES);
        for channel in 0..CHANNELS {
            for &sample in output.channel(channel) {
                assert!(sample.is_finite(), "{sample} reached the device");
                assert!((-1.0..=1.0).contains(&sample), "{sample} is out of range");
            }
        }
    }

    #[test]
    fn a_channel_that_does_not_exist_is_empty_rather_than_a_panic() {
        let mut output = WebAudioOutput::new(Ramp::new());
        output
            .configure(48_000.0, RENDER_QUANTUM_FRAMES)
            .expect("48 kHz");
        output.render(RENDER_QUANTUM_FRAMES);
        assert_eq!(output.channel(0).len(), RENDER_QUANTUM_FRAMES);
        assert_eq!(output.channel(1).len(), RENDER_QUANTUM_FRAMES);
        assert!(output.channel(CHANNELS).is_empty());
        assert!(output.channel(usize::MAX).is_empty());
    }

    /// The mixer through the browser path: the same seam the cpal stream uses,
    /// pulled by hand.
    #[test]
    fn the_mixer_reaches_the_browser_through_the_pull() {
        let mut bank = SoundBank::new();
        // A DC block, so what comes out the other end of the resampler is a
        // value rather than a waveform to reason about.
        bank.insert(1, vec![0.5; 4_096 * CHANNELS]);
        let mut mixer = Mixer::new();
        mixer.play(bank.create_voice(1).expect("registered"));

        let mut output = WebAudioOutput::new(mixer);
        output
            .configure(44_100.0, RENDER_QUANTUM_FRAMES)
            .expect("44.1 kHz");
        assert_eq!(output.render(RENDER_QUANTUM_FRAMES), RENDER_QUANTUM_FRAMES);

        // Frame 0 interpolates away from the silent carry; everything after it
        // is the voice.
        for (i, &sample) in output.channel(0).iter().enumerate().skip(1) {
            assert!((sample - 0.5).abs() < 1e-6, "frame {i}: {sample}");
        }
    }

    // -- the thread-local and the entry points ------------------------------

    /// The entry points share one thread-local, so the tests that touch it run
    /// under a mutex and clean up after themselves. `nextest` runs each test in
    /// its own process, but `cargo test` does not.
    static SHIM: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn shim_guard() -> std::sync::MutexGuard<'static, ()> {
        let guard = SHIM.lock().unwrap_or_else(|e| e.into_inner());
        uninstall();
        guard
    }

    #[test]
    fn the_entry_points_answer_zero_until_a_source_is_installed() {
        let _guard = shim_guard();
        assert!(!is_installed());
        assert_eq!(shim::__crcbl_web_audio_configure(48_000.0, 128), 0);
        assert_eq!(shim::__crcbl_web_audio_render(128), 0);
        assert!(shim::__crcbl_web_audio_channel(0).is_null());
        assert_eq!(shim::__crcbl_web_audio_max_frames(), 0);
        assert_eq!(shim::__crcbl_web_audio_sample_rate(), 0.0);
        assert_eq!(shim::__crcbl_web_audio_stop(), 0);
        // These two are constants and answer whether or not anything is
        // installed, so a shim can print them while diagnosing silence.
        assert_eq!(shim::__crcbl_web_audio_channels() as usize, CHANNELS);
        assert_eq!(shim::__crcbl_web_audio_render_rate(), INTERNAL_SAMPLE_RATE);
    }

    #[test]
    fn the_worklets_call_sequence_produces_audio() {
        let _guard = shim_guard();
        assert!(install(Ramp::new()));
        assert!(
            !install(Ramp::new()),
            "a second install must not replace the first"
        );

        // The constructor's call, then a block, exactly as the module docs say.
        assert_eq!(shim::__crcbl_web_audio_configure(44_100.0, 128), 1);
        assert_eq!(shim::__crcbl_web_audio_sample_rate(), 44_100.0);
        assert_eq!(shim::__crcbl_web_audio_max_frames(), 128);

        let got = shim::__crcbl_web_audio_render(128);
        assert_eq!(got, 128);
        let pointer = shim::__crcbl_web_audio_channel(0);
        assert!(!pointer.is_null());
        // The address is stable across blocks — `render` never reallocates,
        // which is what lets a worklet build its view once.
        assert_eq!(shim::__crcbl_web_audio_render(128), 128);
        assert_eq!(shim::__crcbl_web_audio_channel(0), pointer);
        assert!(shim::__crcbl_web_audio_channel(CHANNELS as u32).is_null());
        assert_eq!(shim::__crcbl_web_audio_underruns(), 0);

        // A short block is visible to the shim.
        assert_eq!(shim::__crcbl_web_audio_render(4_096), 128);
        assert_eq!(shim::__crcbl_web_audio_underruns(), 1);

        assert_eq!(shim::__crcbl_web_audio_stop(), 1);
        assert!(!is_installed());
        assert_eq!(shim::__crcbl_web_audio_render(128), 0);
    }

    #[test]
    fn a_bad_rate_from_the_shim_leaves_the_output_silent_rather_than_detuned() {
        let _guard = shim_guard();
        assert!(install(Ramp::new()));
        assert_eq!(shim::__crcbl_web_audio_configure(f64::NAN, 128), 0);
        assert_eq!(shim::__crcbl_web_audio_render(128), 0);
        assert!(shim::__crcbl_web_audio_channel(0).is_null());
        assert!(shim::__crcbl_web_audio_underruns() > 0);
        uninstall();
    }
}
