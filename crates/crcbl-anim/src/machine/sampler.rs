//! The client's half: a [`MachineState`] it was sent becomes a pose.

use super::asset::{MAX_PARAMETERS, Motion};
use super::{Machine, MachineState, StateId};
use crate::blend::{blend_into, locate, sample_at_phase, sample_located};
use crate::{Pose, Skeleton};

/// Poses a skeleton from a [`MachineState`]: the current state's motion at its
/// time, crossfaded with the outgoing state's while a fade is in flight — and,
/// if the machine authors root motion, with the root's translation stripped.
///
/// Reads the state and never writes it, so the client poses a character from
/// the copy the server sent and the two cannot disagree about where in a clip
/// it is. Holds four scratch poses, built once, so a frame allocates nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Sampler {
    lower: Pose,
    upper: Pose,
    from: Pose,
    to: Pose,
    root: Option<usize>,
}

impl Sampler {
    /// Scratch for `skeleton`, stripping `root_motion`'s translation from every
    /// pose if it names a joint.
    ///
    /// **The whole translation is stripped**, every axis: the root's path is
    /// what [`Machine::root_velocity`] hands the controller, and the drawn root
    /// stays at its rest translation so the character is drawn where the
    /// controller put it — once — rather than there plus wherever the clip
    /// walked to. Which axes of the velocity the controller honours is the
    /// caller's decision.
    ///
    /// # Panics
    ///
    /// If `root_motion` names a joint `skeleton` has not got.
    #[must_use]
    pub fn new(skeleton: &Skeleton, root_motion: Option<usize>) -> Self {
        if let Some(root) = root_motion {
            assert!(
                root < skeleton.len(),
                "the root-motion joint {root} is not one of this skeleton's {} joints",
                skeleton.len()
            );
        }
        Self {
            lower: Pose::new(skeleton),
            upper: Pose::new(skeleton),
            from: Pose::new(skeleton),
            to: Pose::new(skeleton),
            root: root_motion,
        }
    }

    /// Writes the pose `state` describes into `pose`.
    ///
    /// # Panics
    ///
    /// If `pose` or this sampler was built for a skeleton of a different size,
    /// or `state` names a state `machine` has not got.
    pub fn sample_into(
        &mut self,
        machine: &Machine,
        state: &MachineState,
        skeleton: &Skeleton,
        pose: &mut Pose,
    ) {
        match state.fade {
            None => sample_state(
                machine,
                state.state,
                state.time,
                &state.values,
                skeleton,
                [&mut self.lower, &mut self.upper],
                pose,
            ),
            Some(fade) => {
                sample_state(
                    machine,
                    fade.from,
                    fade.from_time,
                    &state.values,
                    skeleton,
                    [&mut self.lower, &mut self.upper],
                    &mut self.from,
                );
                sample_state(
                    machine,
                    state.state,
                    state.time,
                    &state.values,
                    skeleton,
                    [&mut self.lower, &mut self.upper],
                    &mut self.to,
                );
                blend_into(&self.from, &self.to, fade.weight(), pose);
            }
        }
        if let Some(root) = self.root {
            pose.locals_mut()[root].translation = skeleton.joints()[root].rest.translation;
        }
    }
}

/// One state's motion at normalised time `time`, into `out`.
fn sample_state(
    machine: &Machine,
    state: StateId,
    time: f32,
    values: &[f32; MAX_PARAMETERS],
    skeleton: &Skeleton,
    scratch: [&mut Pose; 2],
    out: &mut Pose,
) {
    match &machine.state_def(state).motion {
        Motion::Clip(clip) => sample_at_phase(machine.clip(*clip), time, skeleton, out),
        Motion::Blend1d {
            parameter,
            positions,
            clips,
        } => {
            let blend = locate(positions, values[*parameter]);
            sample_located(
                blend,
                [
                    machine.clip(clips[blend.lower]),
                    machine.clip(clips[blend.upper]),
                ],
                time,
                skeleton,
                scratch,
                out,
            );
        }
    }
}
