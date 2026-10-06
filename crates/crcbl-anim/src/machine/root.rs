//! Root motion: the root joint's translation, measured across a step's span
//! and handed back as a velocity for the character controller.

use glam::Vec3;

use super::asset::{MAX_PARAMETERS, Motion};
use super::{Advance, Machine, MachineState, Span};
use crate::Clip;
use crate::blend::locate;

impl Machine {
    /// The velocity the root joint's translation channel authors over the step
    /// `advance` describes, in the root's parent space — model space, for a
    /// root — in units per second.
    ///
    /// The displacement across each span, crossfaded the way the pose is
    /// ([`Advance::weight`] of the current state's, the rest the outgoing
    /// state's), divided by `dt`. A looping span that wrapped measures each
    /// completed cycle whole, so a clip that walks forward keeps walking
    /// forward across its seam instead of snapping back to its start. A clip
    /// that does not drive `joint`'s translation authors no motion — an
    /// in-place clip is zero here, and that is its meaning rather than a gap.
    ///
    /// `state` is the one the step was taken on; its parameters give a 1D
    /// blend's weight. `dt` is the step's own, and a `dt` that is not above
    /// zero answers zero rather than dividing by it.
    ///
    /// **Apply it to the character controller, never to the transform.** See
    /// the [module docs](super).
    #[must_use]
    pub fn root_velocity(
        &self,
        advance: &Advance,
        state: &MachineState,
        joint: usize,
        dt: f32,
    ) -> Vec3 {
        if dt.is_nan() || dt <= 0.0 {
            return Vec3::ZERO;
        }
        let current = self.span_motion(advance.current, &state.values, joint);
        let delta = match advance.source {
            Some(source) => self
                .span_motion(source, &state.values, joint)
                .lerp(current, advance.weight),
            None => current,
        };
        delta / dt
    }

    /// How far `joint` moved across one state's span.
    fn span_motion(&self, span: Span, values: &[f32; MAX_PARAMETERS], joint: usize) -> Vec3 {
        let def = self.state_def(span.state);
        match &def.motion {
            Motion::Clip(clip) => clip_motion(self.clip(*clip), span, def.looping, joint),
            Motion::Blend1d {
                parameter,
                positions,
                clips,
            } => {
                let blend = locate(positions, values[*parameter]);
                let lower = clip_motion(self.clip(clips[blend.lower]), span, def.looping, joint);
                if blend.weight <= 0.0 {
                    return lower;
                }
                let upper = clip_motion(self.clip(clips[blend.upper]), span, def.looping, joint);
                if blend.weight >= 1.0 {
                    upper
                } else {
                    lower.lerp(upper, blend.weight)
                }
            }
        }
    }
}

/// How far `clip` carries `joint` across `span`, sampled at normalised times.
fn clip_motion(clip: &Clip, span: Span, looping: bool, joint: usize) -> Vec3 {
    let at = |phase: f32| clip.translation_of(joint, phase * clip.duration());
    let Some(start) = at(span.start) else {
        return Vec3::ZERO;
    };
    let wraps = span.end.floor();
    if !looping || wraps < 1.0 {
        let end = at(span.end - wraps).unwrap_or(start);
        return end - start;
    }
    // Out to the end of this cycle, whole cycles after it, and in from the
    // start of the last one to where the span stopped.
    let first = at(0.0).unwrap_or(start);
    let last = at(1.0).unwrap_or(start);
    let tail = at(span.end - wraps).unwrap_or(start);
    (last - start) + (last - first) * (wraps - 1.0) + (tail - first)
}
