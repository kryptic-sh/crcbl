//! Blending: two poses mixed by weight, a locomotion set selected by speed,
//! and the timed fade a state switch needs.
//!
//! The fourth slice of `docs/plan/17-animation.md`, and the one
//! `docs/plan/sample/09-puppet.md`'s milestone 2 is the consumer of: "client
//! blend tree (1D locomotion by speed, crossfades)".
//!
//! ```text
//! Pose  ─┐
//!        ├─ blend_into(weight) ──▶ Pose
//! Pose  ─┘
//!
//! BlendSpace1d   clips at ascending positions on one axis — speed
//!                sample_into(position, phase) ──▶ Pose
//! ```
//!
//! # What is not here
//!
//! No graph, no nodes, no state machine and no events. A blend tree with one
//! shape — the locomotion set — is that shape, and the machinery to describe
//! *other* shapes has no second caller to justify it.
//!
//! **No crossfade either, and its absence is deliberate.** A state switch fades
//! over a duration, and the timer for that is a dozen lines — but a locomotive
//! set that is continuous in speed never switches states, so nothing in this
//! workspace would call it. It arrives with the state machine that needs it,
//! which is the sample's to own: which states exist is the sample's question
//! and not this crate's.
//!
//! # Weights are clamped, never extrapolated
//!
//! Every weight here is read as "where between the two ends", and a value
//! outside `0..=1` holds the nearer end rather than continuing past it. An
//! extrapolated pose is not a pose: it denormalises rotations, inverts scales
//! and has no meaning a caller could have asked for. A `NaN` weight holds the
//! *first* end, which is [`crate::sample`]'s rule for a `NaN` time as well.
//!
//! # The ends are exact
//!
//! [`blend_into`] at weight 0 copies `a` and at weight 1 copies `b`, rather
//! than interpolating and landing near them. That is what lets a blend space
//! sitting on one of its own stops play that clip untouched and a finished
//! crossfade *be* the incoming pose — neither is a pose "close to" the one
//! authored, and a fade that ended a few ulps short of its target would leave
//! every joint of the character permanently, invisibly wrong.

use std::fmt;

use crate::{Clip, Pose, Skeleton, Trs};

/// Blends one joint's transform toward another.
///
/// Translation and scale interpolate linearly. **Rotation takes the shorter
/// arc**, and that is the whole of what makes this function correct.
///
/// A rotation has two quaternion spellings, `q` and `-q`, and they are the same
/// orientation but not the same *path*: the arc from `a` to `b` and the arc
/// from `a` to `-b` go opposite ways round the great circle, one short and one
/// long. Which spelling a pose happens to hold is an accident of whatever
/// produced it — a sampler, an exporter, an earlier blend — so a blend that
/// interpolates toward `b` as given takes the long way round whenever the dot
/// product is negative, which for unrelated poses is about half the time. On
/// screen that is a joint spinning most of the way round to reach a
/// neighbouring angle: the classic blend bug, and one that looks like a
/// content problem rather than a maths one.
///
/// [`Quat::slerp`](glam::Quat::slerp) negates the far end when the dot product
/// is negative, which is exactly that fix.
/// [`Quat::slerp_long`](glam::Quat::slerp_long) is the same function *without* the
/// negation — glam's own name for the wrong answer here — and swapping one for
/// the other is how `takes_the_shorter_arc_between_two_far_apart_rotations`
/// below is shown to be a check that can fail.
///
/// Both ends are unit quaternions, so the result is one too: `slerp` walks the
/// unit sphere, and its near-parallel fallback normalises.
#[inline]
fn blend_trs(a: Trs, b: Trs, weight: f32) -> Trs {
    Trs {
        translation: a.translation.lerp(b.translation, weight),
        rotation: a.rotation.slerp(b.rotation, weight),
        scale: a.scale.lerp(b.scale, weight),
    }
}

/// Blends `a` toward `b` joint by joint, writing the result into `out`.
///
/// `weight` is where the result sits between the two: 0 is `a`, 1 is `b`.
/// Outside `0..=1`, and for `NaN`, see the [module docs](self) — the nearer end
/// is copied through **exactly**.
///
/// Allocates nothing: `out` is built once against the skeleton and refilled,
/// the same arrangement [`Clip::sample_into`] uses.
///
/// `out` may not be `a` or `b` — the borrow checker is what says so, and there
/// is no in-place form because every caller here has a spare pose to write into
/// already.
///
/// # Panics
///
/// If the three poses do not all cover the same number of joints. A pose from a
/// different skeleton has different bones at the same palette indices, so there
/// is no blend of the two to give.
pub fn blend_into(a: &Pose, b: &Pose, weight: f32, out: &mut Pose) {
    assert_eq!(
        a.len(),
        b.len(),
        "these two poses were built for skeletons with different joint counts"
    );
    assert_eq!(
        out.len(),
        a.len(),
        "this destination pose was built for a skeleton with a different joint count"
    );
    if weight.is_nan() || weight <= 0.0 {
        out.locals_mut().copy_from_slice(a.locals());
        return;
    }
    if weight >= 1.0 {
        out.locals_mut().copy_from_slice(b.locals());
        return;
    }
    for ((local, &from), &to) in out.locals_mut().iter_mut().zip(a.locals()).zip(b.locals()) {
        *local = blend_trs(from, to, weight);
    }
}

/// Where a position landed in a [`BlendSpace1d`]: the two stops around it and
/// how far between them it sits.
///
/// The sample's HUD reads this as well as [`BlendSpace1d::sample_into`] does,
/// which is why it is a value rather than something private to the sampling
/// call: a blend weight nobody can see is a blend nobody can check.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blend {
    /// The palette index of the stop below the position.
    pub lower: usize,
    /// The palette index of the stop above it.
    ///
    /// `lower + 1`, except in a space with a single stop, where it is `lower`.
    pub upper: usize,
    /// Where between the two the position sits: 0 is `lower`, 1 is `upper`.
    ///
    /// Always in `0..=1`. A position off either end of the space clamps to the
    /// end stop, so the number saturates rather than running past it.
    pub weight: f32,
}

/// Why a set of stops could not become a [`BlendSpace1d`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendSpaceError {
    /// There were no stops at all.
    Empty,
    /// A stop's position is not finite.
    NotFinite {
        /// Which stop, by index.
        stop: usize,
    },
    /// A stop's position is not strictly above the one before it.
    OutOfOrder {
        /// Which stop, by index.
        stop: usize,
    },
}

impl fmt::Display for BlendSpaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Empty => write!(f, "a blend space with no stops has no pose to give"),
            Self::NotFinite { stop } => {
                write!(f, "stop {stop} is at a position that is not finite")
            }
            Self::OutOfOrder { stop } => write!(
                f,
                "stop {stop} is not at a position above the stop before it"
            ),
        }
    }
}

impl std::error::Error for BlendSpaceError {}

/// One stop of a [`BlendSpace1d`]: a clip and the axis position it is authored
/// for.
#[derive(Clone, Debug, PartialEq)]
struct Stop {
    position: f32,
    clip: Clip,
}

/// Clips placed along one axis, blended by where a value falls between them.
///
/// The axis is the caller's: for `docs/plan/sample/09-puppet.md` it is the
/// character's measured ground speed in metres per second, with idle at 0, a
/// walk at the walk speed and a run at the run speed. A character accelerating
/// through the set passes smoothly across it because the weight between two
/// stops is continuous in the position, and it is *measured* speed rather than
/// a state flag that selects the pose — which is the difference between
/// locomotion that tracks the controller and locomotion that snaps when a
/// boolean flips.
///
/// # Every stop is sampled at the same phase, not the same time
///
/// [`sample_into`](Self::sample_into) takes a **phase** in `0..=1` and samples
/// each contributing clip at `phase * clip.duration()`. That is what keeps the
/// blend from destroying the motion it is blending: an idle of one length and a
/// walk of another, played on a shared clock, drift against each other until
/// the two poses being mixed are at unrelated points of their cycles, and the
/// average of a left step and a right step is a character standing still with
/// its legs apart. Sampled at a shared phase they stay in step, and the blend
/// of two half-cycles is a half-cycle.
///
/// Wrapping the phase is the caller's, for the reason [`crate::sample`] gives
/// for not looping inside the sampler: a clip that must play once and hold has
/// no wrap to apply, and the modulo is one line at the call site.
///
/// # Cost
///
/// Two scratch poses, allocated once by [`new`](Self::new) and refilled. A
/// position sitting exactly on a stop samples that one clip straight into the
/// caller's pose and blends nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendSpace1d {
    stops: Vec<Stop>,
    lower_pose: Pose,
    upper_pose: Pose,
}

impl BlendSpace1d {
    /// Takes the stops as `(position, clip)` pairs, in ascending position
    /// order.
    ///
    /// # Errors
    ///
    /// [`BlendSpaceError`] if there are no stops, if a position is not finite,
    /// or if the positions do not strictly ascend.
    ///
    /// **Refused rather than sorted**, which is [`Skeleton::new`]'s stance for
    /// the same reason: a caller that handed them over out of order is a caller
    /// whose idea of the axis disagrees with its clips, and silently reordering
    /// would produce a space that blends the wrong pair at every speed with
    /// nothing on screen to say so. Two stops at the *same* position are
    /// refused too — there is no weight between them, and which one wins would
    /// be an accident of the search.
    pub fn new(stops: Vec<(f32, Clip)>, skeleton: &Skeleton) -> Result<Self, BlendSpaceError> {
        if stops.is_empty() {
            return Err(BlendSpaceError::Empty);
        }
        for (index, &(position, _)) in stops.iter().enumerate() {
            if !position.is_finite() {
                return Err(BlendSpaceError::NotFinite { stop: index });
            }
            if index > 0 && position <= stops[index - 1].0 {
                return Err(BlendSpaceError::OutOfOrder { stop: index });
            }
        }
        Ok(Self {
            stops: stops
                .into_iter()
                .map(|(position, clip)| Stop { position, clip })
                .collect(),
            lower_pose: Pose::new(skeleton),
            upper_pose: Pose::new(skeleton),
        })
    }

    /// How many stops this space has. Never zero — [`new`](Self::new) refuses a
    /// space with none.
    ///
    /// What a caller reporting a blend across the *whole* space needs, since
    /// [`locate`](Self::locate) answers within one segment of it.
    #[inline]
    #[must_use]
    pub fn stops(&self) -> usize {
        self.stops.len()
    }

    /// The position of each stop, in ascending order.
    #[inline]
    #[must_use]
    pub fn positions(&self) -> impl ExactSizeIterator<Item = f32> + '_ {
        self.stops.iter().map(|stop| stop.position)
    }

    /// Which two stops a position falls between, and how far between them.
    ///
    /// A position off either end clamps to the end stop; a `NaN` position holds
    /// the first, which is [`crate::sample`]'s rule for a `NaN` time.
    #[must_use]
    pub fn locate(&self, position: f32) -> Blend {
        let last = self.stops.len() - 1;
        if last == 0 {
            return Blend {
                lower: 0,
                upper: 0,
                weight: 0.0,
            };
        }
        if position.is_nan() || position <= self.stops[0].position {
            return Blend {
                lower: 0,
                upper: 1,
                weight: 0.0,
            };
        }
        if position >= self.stops[last].position {
            return Blend {
                lower: last - 1,
                upper: last,
                weight: 1.0,
            };
        }
        // Strictly inside the space, so the partition point is in `1..=last`
        // and the segment below it exists.
        let upper = self
            .stops
            .partition_point(|stop| stop.position <= position)
            .max(1);
        let lower = upper - 1;
        let span = self.stops[upper].position - self.stops[lower].position;
        Blend {
            lower,
            upper,
            weight: (position - self.stops[lower].position) / span,
        }
    }

    /// Samples the set at `position` and `phase`, writing one local transform
    /// per joint into `pose`.
    ///
    /// `phase` runs `0..=1` over each contributing clip's own duration — see
    /// the [type docs](Self) for why that and not a shared time in seconds.
    ///
    /// # Panics
    ///
    /// If `pose`, or the scratch this space was built with, was not built for a
    /// skeleton of this size — [`Clip::sample_into`]'s own condition, for the
    /// same reason.
    pub fn sample_into(&mut self, position: f32, phase: f32, skeleton: &Skeleton, pose: &mut Pose) {
        let blend = self.locate(position);
        // A position sitting on a stop is that clip, untouched: no scratch, no
        // blend, and no chance of a pose a few ulps off the one authored.
        if blend.weight <= 0.0 {
            self.stops[blend.lower].sample_into(phase, skeleton, pose);
            return;
        }
        if blend.weight >= 1.0 {
            self.stops[blend.upper].sample_into(phase, skeleton, pose);
            return;
        }
        self.stops[blend.lower].sample_into(phase, skeleton, &mut self.lower_pose);
        self.stops[blend.upper].sample_into(phase, skeleton, &mut self.upper_pose);
        blend_into(&self.lower_pose, &self.upper_pose, blend.weight, pose);
    }
}

impl Stop {
    /// Samples this stop's clip at `phase` of its own duration.
    #[inline]
    fn sample_into(&self, phase: f32, skeleton: &Skeleton, pose: &mut Pose) {
        self.clip
            .sample_into(phase * self.clip.duration(), skeleton, pose);
    }
}

#[cfg(test)]
mod tests {
    use super::{Blend, BlendSpace1d, BlendSpaceError, blend_into, blend_trs};
    use crate::{Channel, Clip, Interpolation, Joint, Pose, Skeleton, Track, Trs};
    use glam::{Mat4, Quat, Vec3};

    /// The angle used wherever a test needs a rotation whose shorter arc and
    /// longer arc are told apart by more than float noise.
    const WIDE_ANGLE: f32 = 3.0;

    fn chain(joints: usize) -> Skeleton {
        Skeleton::new(
            (0..joints)
                .map(|index| Joint {
                    parent: index.checked_sub(1),
                    inverse_bind: Mat4::IDENTITY,
                    rest: Trs::IDENTITY,
                })
                .collect(),
        )
        .expect("a chain whose parents precede their children is well ordered")
    }

    /// A one-joint clip holding joint 0 at a constant translation along `+X`,
    /// over one second.
    fn held_at(x: f32) -> Clip {
        Clip::new(vec![
            Channel::new(
                0,
                vec![0.0, 1.0],
                Interpolation::Linear,
                Track::Translation(vec![Vec3::new(x, 0.0, 0.0); 2]),
            )
            .expect("two keyframes and two values"),
        ])
    }

    fn posed(skeleton: &Skeleton, locals: &[Trs]) -> Pose {
        let mut pose = Pose::new(skeleton);
        pose.locals_mut().copy_from_slice(locals);
        pose
    }

    // -- blend_into ---------------------------------------------------------

    #[test]
    fn weight_zero_is_the_first_pose_exactly() {
        let skeleton = chain(2);
        let a = posed(
            &skeleton,
            &[
                Trs {
                    translation: Vec3::new(1.0, 2.0, 3.0),
                    rotation: Quat::from_rotation_y(0.4),
                    scale: Vec3::new(0.5, 0.5, 0.5),
                },
                Trs::IDENTITY,
            ],
        );
        let b = posed(&skeleton, &[Trs::IDENTITY, Trs::IDENTITY]);
        let mut out = Pose::new(&skeleton);
        blend_into(&a, &b, 0.0, &mut out);
        assert_eq!(out, a);
    }

    /// **The far-apart joint is the one that pins this down.** The shorter-arc
    /// flip negates the far end, so an interpolation evaluated at weight 1 over
    /// a pair more than 180° apart answers `-b`: the same *rotation* as `b`, and
    /// not the same *value*. A pose carries its spelling into whatever blends it
    /// next, so "the incoming pose, as given" has to mean the quaternion that
    /// was handed over. Copying `b` through is what makes that true, and this is
    /// the case that goes red when the copy is replaced by a clamp.
    #[test]
    fn weight_one_is_the_second_pose_exactly() {
        let skeleton = chain(2);
        let a = posed(
            &skeleton,
            &[
                Trs::IDENTITY,
                Trs {
                    rotation: Quat::from_rotation_z(WIDE_ANGLE),
                    ..Trs::IDENTITY
                },
            ],
        );
        let b = posed(
            &skeleton,
            &[
                Trs {
                    translation: Vec3::new(-4.0, 0.25, 8.0),
                    rotation: Quat::from_rotation_x(1.1),
                    scale: Vec3::new(2.0, 3.0, 4.0),
                },
                Trs {
                    rotation: Quat::from_rotation_z(-WIDE_ANGLE),
                    ..Trs::IDENTITY
                },
            ],
        );
        assert!(
            a.locals()[1].rotation.dot(b.locals()[1].rotation) < 0.0,
            "joint 1 must be more than 180 degrees apart for this to pin the copy down"
        );
        let mut out = Pose::new(&skeleton);
        blend_into(&a, &b, 1.0, &mut out);
        assert_eq!(out, b);
    }

    /// Off either end, and for a `NaN`, the nearer end is copied through — not
    /// extrapolated past. See the module docs.
    #[test]
    fn a_weight_outside_the_range_holds_the_nearer_end() {
        let skeleton = chain(1);
        let a = posed(&skeleton, &[Trs::IDENTITY]);
        let b = posed(
            &skeleton,
            &[Trs {
                translation: Vec3::new(10.0, 0.0, 0.0),
                ..Trs::IDENTITY
            }],
        );
        let mut out = Pose::new(&skeleton);
        blend_into(&a, &b, -5.0, &mut out);
        assert_eq!(out, a);
        blend_into(&a, &b, 5.0, &mut out);
        assert_eq!(out, b);
        blend_into(&a, &b, f32::NAN, &mut out);
        assert_eq!(out, a);
    }

    /// A pose blended with itself is that pose. Not "close to" it: a character
    /// standing at one end of a blend space, or holding a state a fade has
    /// finished into, must be posed exactly as the clip authored it.
    #[test]
    fn blending_a_pose_with_itself_is_that_pose() {
        let skeleton = chain(3);
        let pose = posed(
            &skeleton,
            &[
                Trs {
                    translation: Vec3::new(0.0, 1.5, 0.0),
                    rotation: Quat::from_rotation_z(0.9),
                    scale: Vec3::ONE,
                },
                Trs {
                    translation: Vec3::new(0.0, -0.75, 0.2),
                    rotation: Quat::from_rotation_x(-2.2),
                    scale: Vec3::new(1.25, 1.25, 1.25),
                },
                Trs::IDENTITY,
            ],
        );
        let mut out = Pose::new(&skeleton);
        for weight in [0.0, 0.25, 0.5, 0.75, 1.0] {
            blend_into(&pose, &pose, weight, &mut out);
            assert_eq!(out, pose, "self-blend at weight {weight} moved a joint");
        }
    }

    #[test]
    fn a_half_blend_is_halfway_along_each_component() {
        let skeleton = chain(1);
        let a = posed(
            &skeleton,
            &[Trs {
                translation: Vec3::new(0.0, 0.0, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            }],
        );
        let b = posed(
            &skeleton,
            &[Trs {
                translation: Vec3::new(4.0, -2.0, 0.0),
                rotation: Quat::from_rotation_z(1.0),
                scale: Vec3::splat(3.0),
            }],
        );
        let mut out = Pose::new(&skeleton);
        blend_into(&a, &b, 0.5, &mut out);
        let local = out.locals()[0];
        assert!((local.translation - Vec3::new(2.0, -1.0, 0.0)).length() < 1e-6);
        assert!((local.scale - Vec3::splat(2.0)).length() < 1e-6);
        let half = Quat::from_rotation_z(0.5);
        assert!(
            local.rotation.dot(half).abs() > 1.0 - 1e-6,
            "halfway between identity and a 1 rad turn is a 0.5 rad turn, got {:?}",
            local.rotation
        );
    }

    /// **The shorter arc, and the check that this crate takes it.**
    ///
    /// The two rotations below are `WIDE_ANGLE` radians apart in opposite
    /// directions about `+Z`, so the angle between them is more than 180° the
    /// short way round the circle and their quaternion dot product is negative.
    /// The short path is the one that goes *through* `±π` — the halfway pose
    /// turns further from zero than either end — and the long path is the one
    /// that comes back through zero.
    ///
    /// Halfway along the short path is `π`, so the blended rotation sends `+X`
    /// to very nearly `-X`. Halfway along the long path is the identity, which
    /// leaves `+X` alone. One test point tells the two apart completely.
    ///
    /// **Red-checked** by replacing [`Quat::slerp`] in `blend_trs` with
    /// [`Quat::slerp_long`], which is glam's same function without the sign
    /// flip: the assertion below goes red and its message reports `+X`
    /// unmoved.
    #[test]
    fn takes_the_shorter_arc_between_two_far_apart_rotations() {
        let from = Quat::from_rotation_z(WIDE_ANGLE);
        let to = Quat::from_rotation_z(-WIDE_ANGLE);
        assert!(
            from.dot(to) < 0.0,
            "this pair must be more than 180 degrees apart for the test to mean anything, \
             dot is {}",
            from.dot(to)
        );

        let skeleton = chain(1);
        let a = posed(
            &skeleton,
            &[Trs {
                rotation: from,
                ..Trs::IDENTITY
            }],
        );
        let b = posed(
            &skeleton,
            &[Trs {
                rotation: to,
                ..Trs::IDENTITY
            }],
        );
        let mut out = Pose::new(&skeleton);
        blend_into(&a, &b, 0.5, &mut out);

        let turned = out.locals()[0].rotation * Vec3::X;
        assert!(
            (turned - Vec3::NEG_X).length() < 1e-5,
            "halfway round the short arc turns +X to -X; it landed at {turned:?}, which is \
             the long way round through the identity"
        );
    }

    /// Every blended rotation is still a unit quaternion — a denormalised one
    /// scales the joint it poses, and the mesh with it.
    #[test]
    fn blended_rotations_stay_normalised() {
        let skeleton = chain(1);
        let a = posed(
            &skeleton,
            &[Trs {
                rotation: Quat::from_rotation_z(WIDE_ANGLE),
                ..Trs::IDENTITY
            }],
        );
        let b = posed(
            &skeleton,
            &[Trs {
                rotation: Quat::from_euler(glam::EulerRot::XYZ, -WIDE_ANGLE, 1.0, 2.0),
                ..Trs::IDENTITY
            }],
        );
        let mut out = Pose::new(&skeleton);
        for step in 0..=20 {
            let weight = step as f32 / 20.0;
            blend_into(&a, &b, weight, &mut out);
            let length = out.locals()[0].rotation.length();
            assert!(
                (length - 1.0).abs() < 1e-5,
                "at weight {weight} the rotation had length {length}"
            );
        }
    }

    #[test]
    fn blend_trs_moves_translation_and_scale_linearly() {
        let a = Trs {
            translation: Vec3::new(-2.0, 0.0, 6.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(1.0, 1.0, 1.0),
        };
        let b = Trs {
            translation: Vec3::new(2.0, 4.0, -2.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::new(5.0, 5.0, 5.0),
        };
        let quarter = blend_trs(a, b, 0.25);
        assert!((quarter.translation - Vec3::new(-1.0, 1.0, 4.0)).length() < 1e-6);
        assert!((quarter.scale - Vec3::splat(2.0)).length() < 1e-6);
    }

    #[test]
    #[should_panic(expected = "different joint counts")]
    fn refuses_poses_from_different_skeletons() {
        let two = chain(2);
        let three = chain(3);
        let a = Pose::new(&two);
        let b = Pose::new(&three);
        let mut out = Pose::new(&two);
        blend_into(&a, &b, 0.5, &mut out);
    }

    // -- BlendSpace1d -------------------------------------------------------

    fn locomotion(skeleton: &Skeleton) -> BlendSpace1d {
        BlendSpace1d::new(
            vec![
                (0.0, held_at(0.0)),
                (2.0, held_at(10.0)),
                (6.0, held_at(30.0)),
            ],
            skeleton,
        )
        .expect("three stops at ascending finite positions")
    }

    #[test]
    fn locates_a_position_inside_a_segment() {
        let skeleton = chain(1);
        let space = locomotion(&skeleton);
        assert_eq!(
            space.locate(1.0),
            Blend {
                lower: 0,
                upper: 1,
                weight: 0.5,
            }
        );
        assert_eq!(
            space.locate(5.0),
            Blend {
                lower: 1,
                upper: 2,
                weight: 0.75,
            }
        );
    }

    #[test]
    fn clamps_a_position_off_either_end() {
        let skeleton = chain(1);
        let space = locomotion(&skeleton);
        assert_eq!(
            space.locate(-100.0),
            Blend {
                lower: 0,
                upper: 1,
                weight: 0.0,
            }
        );
        assert_eq!(
            space.locate(100.0),
            Blend {
                lower: 1,
                upper: 2,
                weight: 1.0,
            }
        );
        assert_eq!(space.locate(f32::NAN).weight, 0.0);
    }

    #[test]
    fn a_single_stop_space_is_that_one_stop() {
        let skeleton = chain(1);
        let space = BlendSpace1d::new(vec![(4.0, held_at(7.0))], &skeleton)
            .expect("one stop is a space with nothing to blend");
        assert_eq!(
            space.locate(-1.0),
            Blend {
                lower: 0,
                upper: 0,
                weight: 0.0,
            }
        );
        assert_eq!(space.stops(), 1);
    }

    /// **The property the demo is for**: the pose the space gives follows the
    /// position continuously, and a sweep across the axis produces a monotone
    /// run of poses rather than three plateaus with jumps between them.
    #[test]
    fn a_sweep_across_the_axis_moves_the_pose_continuously() {
        let skeleton = chain(1);
        let mut space = locomotion(&skeleton);
        let mut pose = Pose::new(&skeleton);
        let mut previous = f32::NEG_INFINITY;
        let mut distinct = 0;
        for step in 0..=60 {
            let position = step as f32 / 10.0;
            space.sample_into(position, 0.5, &skeleton, &mut pose);
            let x = pose.locals()[0].translation.x;
            assert!(
                x >= previous,
                "the pose went backwards at speed {position}: {x} after {previous}"
            );
            if x > previous {
                distinct += 1;
            }
            previous = x;
        }
        assert!(
            distinct > 40,
            "a blended sweep should take a new value at nearly every step; it took {distinct}"
        );
        assert!(
            (previous - 30.0).abs() < 1e-4,
            "the top of the space is the top stop's clip, got {previous}"
        );
    }

    /// A position sitting on a stop plays that clip untouched, so the ends of
    /// the space are the authored poses rather than blends that land near them.
    #[test]
    fn a_position_on_a_stop_is_that_clip_exactly() {
        let skeleton = chain(1);
        let mut space = locomotion(&skeleton);
        let mut blended = Pose::new(&skeleton);
        let mut direct = Pose::new(&skeleton);
        space.sample_into(2.0, 0.25, &skeleton, &mut blended);
        held_at(10.0).sample_into(0.25, &skeleton, &mut direct);
        assert_eq!(blended, direct);
    }

    /// Each stop is sampled at its own duration times the phase, so clips of
    /// different lengths stay in step. The two clips here are two seconds and
    /// one second long, and at phase 0.5 each is at *its own* midpoint.
    #[test]
    fn stops_are_sampled_at_a_shared_phase_not_a_shared_time() {
        let skeleton = chain(1);
        let slow = Clip::new(vec![
            Channel::new(
                0,
                vec![0.0, 2.0],
                Interpolation::Linear,
                Track::Translation(vec![Vec3::ZERO, Vec3::new(0.0, 8.0, 0.0)]),
            )
            .expect("two keyframes and two values"),
        ]);
        let fast = Clip::new(vec![
            Channel::new(
                0,
                vec![0.0, 1.0],
                Interpolation::Linear,
                Track::Translation(vec![Vec3::ZERO, Vec3::new(0.0, 8.0, 0.0)]),
            )
            .expect("two keyframes and two values"),
        ]);
        let mut space = BlendSpace1d::new(vec![(0.0, slow), (1.0, fast)], &skeleton)
            .expect("two stops at ascending finite positions");
        let mut pose = Pose::new(&skeleton);
        space.sample_into(0.5, 0.5, &skeleton, &mut pose);
        assert!(
            (pose.locals()[0].translation.y - 4.0).abs() < 1e-5,
            "both clips are at their own midpoint, so the blend is too; got {:?}",
            pose.locals()[0].translation
        );
    }

    #[test]
    fn refuses_a_space_with_no_stops() {
        let skeleton = chain(1);
        assert_eq!(
            BlendSpace1d::new(Vec::new(), &skeleton).expect_err("no stops, no pose"),
            BlendSpaceError::Empty
        );
    }

    #[test]
    fn refuses_stops_that_do_not_ascend() {
        let skeleton = chain(1);
        let error = BlendSpace1d::new(
            vec![
                (0.0, held_at(0.0)),
                (3.0, held_at(1.0)),
                (1.0, held_at(2.0)),
            ],
            &skeleton,
        )
        .expect_err("stop 2 sits below stop 1");
        assert_eq!(error, BlendSpaceError::OutOfOrder { stop: 2 });

        let repeated = BlendSpace1d::new(vec![(1.0, held_at(0.0)), (1.0, held_at(1.0))], &skeleton)
            .expect_err("two stops at one position have no weight between them");
        assert_eq!(repeated, BlendSpaceError::OutOfOrder { stop: 1 });
    }

    #[test]
    fn refuses_a_stop_that_is_not_finite() {
        let skeleton = chain(1);
        let error = BlendSpace1d::new(
            vec![(0.0, held_at(0.0)), (f32::NAN, held_at(1.0))],
            &skeleton,
        )
        .expect_err("a stop nothing compares against orders nothing");
        assert_eq!(error, BlendSpaceError::NotFinite { stop: 1 });
    }

    #[test]
    fn errors_say_which_stop() {
        assert_eq!(
            BlendSpaceError::OutOfOrder { stop: 4 }.to_string(),
            "stop 4 is not at a position above the stop before it"
        );
    }

    #[test]
    fn reports_its_stops() {
        let skeleton = chain(1);
        let space = locomotion(&skeleton);
        assert_eq!(space.stops(), 3);
        assert_eq!(space.positions().collect::<Vec<_>>(), vec![0.0, 2.0, 6.0]);
    }
}
