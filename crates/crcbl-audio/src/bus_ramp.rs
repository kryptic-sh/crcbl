//! How a bus fader moves: a linear ramp from the gain the bus is at to the
//! one [`Mixer::set_bus_gain`](crate::mixer::Mixer::set_bus_gain) asked for,
//! rather than a step at the block boundary.
//!
//! A gain that steps clicks, by the step times whatever every voice on the bus
//! was playing. So a bus moves over [`ITD_RAMP_FRAMES`] output frames, the
//! length a voice's own level ramps over in [`crate::mix_ramp`]: a fader and a
//! re-aim are both a level moving under a sound that is already playing, and
//! one length keeps them sounding alike. Each frame's gain is worked out from
//! the ramp's two ends, not accumulated, so where a ramp is does not depend on
//! how the device cut it into blocks, and its last frame lands on the target
//! exactly.
//!
//! # A still fader is the fader it was before
//!
//! Settled, a ramp answers the target itself, the same `f32` the mixer
//! multiplied by before anything was smoothed. Re-sending the gain a bus
//! already has starts nothing, and a move that arrives mid-ramp starts from
//! the gain the last frame used.
//!
//! **A gain set before the first block takes effect without a ramp**, as
//! [`Voice::with_mix`](crate::mixer::Voice::with_mix) does for a voice: a
//! player's saved level was never heard at any other, so there is nothing to
//! move from.

use crate::itd::ITD_RAMP_FRAMES;
use crate::mixer::Bus;

/// One bus's gain, and the ramp it is on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BusRamp {
    /// The gain where the ramp began.
    from: f32,
    /// The gain the ramp ends on: what the bus was set to.
    to: f32,
    /// Frames into the ramp; [`ITD_RAMP_FRAMES`] once settled.
    frame: u32,
}

impl BusRamp {
    /// Settled at `gain`.
    pub(crate) const fn settled(gain: f32) -> Self {
        Self {
            from: gain,
            to: gain,
            frame: ITD_RAMP_FRAMES,
        }
    }

    /// The gain the bus was set to, whether or not the ramp has reached it.
    pub(crate) const fn target(&self) -> f32 {
        self.to
    }

    /// The gain `ahead` frames from where the ramp stands: `0` is the gain the
    /// last frame used, `1` the next frame's.
    pub(crate) fn at(&self, ahead: usize) -> f32 {
        let frame = (self.frame as usize).saturating_add(ahead);
        if frame >= ITD_RAMP_FRAMES as usize {
            return self.to;
        }
        let t = frame as f32 / ITD_RAMP_FRAMES as f32;
        self.from + (self.to - self.from) * t
    }

    /// Ramp to `to` from wherever the bus is now. An unchanged target leaves
    /// the ramp as it is.
    fn retarget(&mut self, to: f32) {
        if to == self.to {
            return;
        }
        self.from = self.at(0);
        self.to = to;
        self.frame = 0;
    }

    /// Move the ramp on by `frames` output frames.
    fn advance(&mut self, frames: usize) {
        let frame = (self.frame as usize).saturating_add(frames);
        self.frame = frame.min(ITD_RAMP_FRAMES as usize) as u32;
    }
}

/// Every bus's ramp, indexed by [`Bus::index`], and whether audio has started.
#[derive(Debug)]
pub(crate) struct BusGains {
    ramps: [BusRamp; Bus::ALL.len()],
    /// Whether a block has been mixed. Until one has, a gain is set without a
    /// ramp.
    started: bool,
}

impl BusGains {
    /// Every bus settled at unity, so a mixer nobody has configured sounds
    /// exactly as it did before buses existed.
    pub(crate) const fn new() -> Self {
        Self {
            ramps: [BusRamp::settled(1.0); Bus::ALL.len()],
            started: false,
        }
    }

    /// `bus`'s ramp.
    pub(crate) const fn get(&self, bus: Bus) -> BusRamp {
        self.ramps[bus.index()]
    }

    /// Set `bus` to `gain`: ramped once audio has started, at once before.
    pub(crate) fn set(&mut self, bus: Bus, gain: f32) {
        let ramp = &mut self.ramps[bus.index()];
        if self.started {
            ramp.retarget(gain);
        } else {
            *ramp = BusRamp::settled(gain);
        }
    }

    /// Hand out the ramps a block of `frames` frames is mixed with, and move
    /// every bus on past that block.
    pub(crate) fn begin_block(&mut self, frames: usize) -> [BusRamp; Bus::ALL.len()] {
        self.started = true;
        let block = self.ramps;
        for ramp in &mut self.ramps {
            ramp.advance(frames);
        }
        block
    }
}

#[cfg(test)]
mod tests;
