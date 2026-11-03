//! The interaural time difference: a fractional delay line per ear, and how a
//! voice moves from one delay to the next.
//!
//! # A change in delay is a change in pitch
//!
//! Reading a delay line whose length moves plays the sound faster or slower
//! while it moves: a delay growing by `Δ` samples over `N` frames plays at
//! `1 − Δ/N` of its speed. Stepped in one frame, that is a click; swept over a
//! long jump, it is an audible pitch glide, which corrupts the pitch cues of
//! the grammar's rules 3 and 4. So a voice moves its ITD one of two ways, over
//! [`ITD_RAMP_FRAMES`] output frames either way:
//!
//! - **A small change sweeps** the delay linearly, reading between taps with
//!   the line's own linear interpolation. No change larger than
//!   [`ITD_SWEEP_LIMIT`] is swept, which keeps the speed within
//!   `ITD_SWEEP_LIMIT / ITD_RAMP_FRAMES` of normal for the length of the ramp.
//! - **A large change, or one that swaps which ear leads, crossfades** two taps
//!   of the same line: the old delay, frozen where it was, fades out while the
//!   new one fades in. Neither tap moves, so nothing is pitched, and a sound
//!   that jumps from one side of the head to the other passes through both
//!   rather than gliding across the middle.
//!
//! A re-aim that arrives during a sweep starts from wherever the sweep had got
//! to. One that arrives during a crossfade waits for it to finish: the output
//! is then a blend of two taps, which no single starting delay describes.
//!
//! # A still voice is the voice it was before
//!
//! Settled, each channel reads one tap at a delay of exactly
//! `(−itd).max(0)` or `itd.max(0)`, through the same interpolation the line
//! always used, and re-sending the ITD a voice already has starts nothing. A
//! voice whose ITD never moves renders the bits it rendered before this module
//! smoothed anything.

use crate::CHANNELS;
use crate::mixer::finite_or;

/// Per-channel delay-line length. One past the longest ITD the mixer will
/// serve: the clamp in [`clamp_itd`] guarantees both fractional taps of any
/// delay stay inside the buffer.
const DELAY_CAPACITY: usize = 64;

/// Largest ITD in samples, per channel. `(capacity - 2)`: a delay of exactly
/// this reads taps at `62` and `63` samples ago, so `capacity - 1` is the
/// deepest tap that still lands in-buffer. Well above the cue grammar's
/// [`CueGrammar::max_itd_samples`](crate::spatial::CueGrammar) default of 30.
pub(crate) const MAX_ITD_DELAY: f32 = (DELAY_CAPACITY - 2) as f32;

/// How many output frames a voice takes to move from one ITD to the next,
/// whether it sweeps or crossfades.
///
/// Shorter than the [occlusion ramp](crate::occlusion::OCCLUSION_RAMP_FRAMES):
/// a delay is a position, and a sound that has moved should be heard to have
/// moved within a few audio blocks, where a wall's muffling can take longer.
/// Long enough that [`ITD_SWEEP_LIMIT`] spread across it bends no pitch a
/// listener hears.
pub const ITD_RAMP_FRAMES: u32 = 1024;

/// The largest per-channel delay change, in samples, that is swept rather than
/// crossfaded.
///
/// With [`ITD_RAMP_FRAMES`] this bounds the pitch a sweep bends:
/// `itd::tests::a_sweep_bends_pitch_by_less_than_a_listener_hears` holds the
/// pair under a listener's threshold. Anything larger crossfades.
pub const ITD_SWEEP_LIMIT: f32 = 2.0;

/// A caller's ITD, clamped to what the delay lines can serve; non-finite is
/// none at all.
fn clamp_itd(itd_samples: f32) -> f32 {
    finite_or(itd_samples, 0.0, -MAX_ITD_DELAY, MAX_ITD_DELAY)
}

/// Each channel's delay for `itd`: positive delays the right channel (source
/// left), negative the left (source right) — each channel's line delays its
/// own ear.
fn channel_delays(itd: f32) -> [f32; CHANNELS] {
    [(-itd).max(0.0), itd.max(0.0)]
}

/// A per-channel circular buffer that answers a fractional delay.
///
/// [`DelayLine::read`] answers the value `delay` samples before the last
/// [`DelayLine::push`], linearly interpolated between the taps at
/// `floor(delay)` and `ceil(delay)` — the standard one-multiply fractional
/// delay. The buffer starts silent, so a channel whose delay is `d` fades in
/// over its first `d` frames, which is what an interaural delay is: the far
/// ear's first arrival is later, not quieter.
#[derive(Debug)]
struct DelayLine {
    buf: Vec<f32>,
    /// The slot the next push overwrites.
    write: usize,
}

impl DelayLine {
    /// A silent line.
    fn new() -> Self {
        Self {
            buf: vec![0.0; DELAY_CAPACITY],
            write: 0,
        }
    }

    /// Write the next sample.
    fn push(&mut self, sample: f32) {
        self.buf[self.write] = sample;
        self.write = (self.write + 1) % DELAY_CAPACITY;
    }

    /// The value `delay` samples before the last push, `0` being that push.
    ///
    /// [`clamp_itd`] keeps every delay a voice asks for in
    /// `[0, MAX_ITD_DELAY]`, and a sweep's frame is clamped there again, so
    /// both taps are always in-buffer and the second never aliases the slot
    /// the next push overwrites.
    fn read(&self, delay: f32) -> f32 {
        let k = delay.floor() as usize;
        let frac = delay - k as f32;
        let newest = self.write + DELAY_CAPACITY - 1;
        let at = |k: usize| self.buf[(newest - k) % DELAY_CAPACITY];
        at(k) * (1.0 - frac) + at(k + 1) * frac
    }
}

/// The ITD state one voice carries: its target, the move toward it, and the
/// delay line per ear.
#[derive(Debug)]
pub(crate) struct VoiceItd {
    /// The ITD the voice is at or moving toward, clamped.
    target: f32,
    /// The target changed since a frame last resolved it.
    retarget: bool,
    /// Per-channel delays where the move began.
    from: [f32; CHANNELS],
    /// Per-channel delays where the move ends.
    to: [f32; CHANNELS],
    /// Per-channel delays read for the frame just processed. During a
    /// crossfade, the delays it started from.
    now: [f32; CHANNELS],
    /// The move under way is a crossfade, not a sweep.
    crossfade: bool,
    /// Frames into the move; [`ITD_RAMP_FRAMES`] once settled.
    frame: u32,
    lines: [DelayLine; CHANNELS],
}

impl VoiceItd {
    /// No ITD, and settled there.
    pub(crate) fn new() -> Self {
        Self {
            target: 0.0,
            retarget: false,
            from: [0.0; CHANNELS],
            to: [0.0; CHANNELS],
            now: [0.0; CHANNELS],
            crossfade: false,
            frame: ITD_RAMP_FRAMES,
            lines: std::array::from_fn(|_| DelayLine::new()),
        }
    }

    /// The ITD this voice is at or moving toward, as clamped.
    pub(crate) const fn target(&self) -> f32 {
        self.target
    }

    /// Move toward `itd_samples` from wherever the voice is now, clamped as
    /// [`clamp_itd`] does.
    pub(crate) fn set(&mut self, itd_samples: f32) {
        self.target = clamp_itd(itd_samples);
        self.retarget = true;
    }

    /// Be at `itd_samples` from the next frame, with no move: a voice that
    /// starts off to one side was never heard anywhere else.
    pub(crate) fn start_at(&mut self, itd_samples: f32) {
        self.target = clamp_itd(itd_samples);
        self.retarget = false;
        self.to = channel_delays(self.target);
        self.from = self.to;
        self.now = self.to;
        self.crossfade = false;
        self.frame = ITD_RAMP_FRAMES;
    }

    /// Whether a crossfade is under way.
    const fn crossfading(&self) -> bool {
        self.crossfade && self.frame < ITD_RAMP_FRAMES
    }

    /// Turn a new target into a move: a sweep from the delays in force, or a
    /// crossfade from them when the move is large or swaps the leading ear.
    fn resolve(&mut self) {
        self.retarget = false;
        let to = channel_delays(self.target);
        if to == self.to {
            return;
        }
        let itd_now = self.now[1] - self.now[0];
        let swaps = itd_now * self.target < 0.0;
        let large = self
            .now
            .iter()
            .zip(&to)
            .any(|(now, to)| (to - now).abs() > ITD_SWEEP_LIMIT);
        self.from = self.now;
        self.to = to;
        self.crossfade = swaps || large;
        self.frame = 0;
    }

    /// Push one frame's samples through the lines and answer each ear's
    /// delayed sample, advancing the move by a frame.
    pub(crate) fn process(&mut self, samples: [f32; CHANNELS]) -> [f32; CHANNELS] {
        if self.retarget && !self.crossfading() {
            self.resolve();
        }
        let mut t = 1.0;
        if self.frame < ITD_RAMP_FRAMES {
            self.frame += 1;
            if self.frame == ITD_RAMP_FRAMES {
                self.now = self.to;
                self.crossfade = false;
            } else {
                t = self.frame as f32 / ITD_RAMP_FRAMES as f32;
                if !self.crossfade {
                    for (now, (from, to)) in self.now.iter_mut().zip(self.from.iter().zip(&self.to))
                    {
                        *now = (from + (to - from) * t).clamp(0.0, MAX_ITD_DELAY);
                    }
                }
            }
        }
        let crossfading = self.crossfading();
        std::array::from_fn(|ch| {
            let line = &mut self.lines[ch];
            line.push(samples[ch]);
            if crossfading {
                line.read(self.from[ch]) * (1.0 - t) + line.read(self.to[ch]) * t
            } else {
                line.read(self.now[ch])
            }
        })
    }
}

#[cfg(test)]
mod tests;
