//! How a re-aimed voice moves its level, pan and speed: a linear ramp from
//! where it is to the new mix, rather than a step at the block boundary.
//!
//! A gain that steps clicks, by the step times whatever the sample was; a
//! pitch that steps changes the tone's frequency between one frame and the
//! next. So [`Mixer::set_mix`](crate::mixer::Mixer::set_mix) moves volume,
//! both channel gains and the playback rate linearly over
//! [`ITD_RAMP_FRAMES`] output frames, the occlusion ramp's pattern: each
//! frame's value is worked out from the ramp's two ends, not accumulated, so
//! where a ramp is does not depend on how the device cut it into blocks, and
//! its last frame lands on the target exactly.
//!
//! **The ITD's length, not one of its own**: level, pan and ITD are one
//! position cue sent in one `set_mix`, and moving them over the same frames
//! keeps the level and time differences between the ears agreeing all the way
//! through a move. Pitch takes the same length: a few audio blocks, quick
//! enough to follow an emitter and long enough that the frequency glides
//! rather than jumps.
//!
//! A ramped playback rate is a read position that accelerates rather than
//! jumps: the mixer advances it by each frame's rate, so the position is the
//! running sum of the rates and stays continuous through the ramp.
//!
//! # A still voice is the voice it was before
//!
//! Settled, a ramp answers the target itself, the same `f32`s the voice
//! multiplied by before anything was smoothed. Re-sending the mix a voice
//! already has starts nothing, and a re-aim that arrives mid-ramp starts from
//! the values the last frame used.

use crate::itd::ITD_RAMP_FRAMES;

/// The ramped parameters, in the order a ramp holds them: volume, the left
/// and right gains, and the pitch ratio.
pub(crate) type MixParams = [f32; 4];

/// The ramp one voice carries. Its far end is not stored: it is the voice's
/// own target, passed in to every call, so the target the voice reports and
/// the one it ramps toward cannot disagree.
#[derive(Debug)]
pub(crate) struct MixRamp {
    /// The parameters where the ramp began.
    from: MixParams,
    /// Frames into the ramp; [`ITD_RAMP_FRAMES`] once settled.
    frame: u32,
}

impl MixRamp {
    /// Settled at whatever target it is given.
    pub(crate) const fn settled() -> Self {
        Self {
            from: [0.0; 4],
            frame: ITD_RAMP_FRAMES,
        }
    }

    /// The parameters the last frame used, ramping toward `to`. Before the
    /// ramp's first frame, that is where it began.
    fn now(&self, to: MixParams) -> MixParams {
        if self.frame >= ITD_RAMP_FRAMES {
            return to;
        }
        let t = self.frame as f32 / ITD_RAMP_FRAMES as f32;
        std::array::from_fn(|i| self.from[i] + (to[i] - self.from[i]) * t)
    }

    /// The target moved from `old` to `new`: ramp to it from wherever the
    /// voice is now. An unchanged target leaves the ramp as it is.
    pub(crate) fn retarget(&mut self, old: MixParams, new: MixParams) {
        if old == new {
            return;
        }
        self.from = self.now(old);
        self.frame = 0;
    }

    /// Advance the ramp one frame toward `to` and answer that frame's
    /// parameters.
    pub(crate) fn step(&mut self, to: MixParams) -> MixParams {
        if self.frame < ITD_RAMP_FRAMES {
            self.frame += 1;
        }
        self.now(to)
    }
}

#[cfg(test)]
mod tests;
