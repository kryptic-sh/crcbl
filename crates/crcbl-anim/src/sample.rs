//! Sampling: a clip and a time in seconds become one local transform per joint.
//!
//! # The three cases that are not errors
//!
//! All three are shapes a well-formed document takes, so each has a defined
//! answer rather than a `Result`:
//!
//! * **A channel that names no joint of this skeleton is skipped.** A glTF
//!   animation drives *nodes*, and a document's nodes are not all joints of the
//!   skin being posed — the mesh node, a camera, the props hanging off a hand,
//!   and every joint of a *second* skin are all animated by channels in the
//!   same clip. Refusing the clip over one would make the common document
//!   unplayable; skipping the channel plays what this skeleton has.
//! * **A joint no channel drives keeps its rest pose.** Not the identity: a
//!   joint's rest transform is where the bone sits when nothing moves it, and
//!   identity would collapse every undriven bone onto its parent's origin. This
//!   also covers the partially-driven joint, which is the more common case — a
//!   rotation-only channel must leave the joint's rest translation alone, and
//!   does, because [`Clip::sample_into`] rewrites the whole pose from
//!   [`Joint::rest`](crate::Joint::rest) before any channel is applied.
//! * **A time outside the clip holds the nearest keyframe.** Clamped, never
//!   wrapped. Looping is the *player's* decision and is one modulo at the call
//!   site; a clip that must not loop — a jump, a death — has to hold its final
//!   pose, and a sampler that wrapped internally would leave no way to say so.
//!   Appendix C defines the interpolation modes only for `t_k < t_c < t_{k+1}`,
//!   so outside that span there is nothing to interpolate between.
//!
//! # Cost
//!
//! Sampling runs once per frame per character, so it allocates nothing: the
//! [`Pose`] is built once against the skeleton and refilled in place, and every
//! keyframe value is read out of the clip's own arrays. The keyframe segment is
//! found by **binary search** ([`slice::partition_point`]) rather than a linear
//! scan or a remembered cursor — a scan is `O(n)` in a clip's keyframe count on
//! every channel of every frame, and a cursor would have to be per-playback
//! state that a shared clip cannot hold.

use glam::{Quat, Vec3};

use crate::clip::CUBIC_SPLINE_VALUES_PER_KEYFRAME;
use crate::{Clip, Interpolation, Skeleton, Track, Trs};

/// One local transform per joint, in palette order.
///
/// Built once from a skeleton and refilled by [`Clip::sample_into`], which is
/// what keeps sampling allocation-free. It is the input to
/// [`Palette::compute`](crate::Palette::compute).
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    locals: Vec<Trs>,
}

impl Pose {
    /// A pose sized for this skeleton and holding its rest transforms.
    #[must_use]
    pub fn new(skeleton: &Skeleton) -> Self {
        Self {
            locals: skeleton.joints().iter().map(|joint| joint.rest).collect(),
        }
    }

    /// The local transforms, in palette order.
    #[inline]
    #[must_use]
    pub fn locals(&self) -> &[Trs] {
        &self.locals
    }

    /// The local transforms, mutably — for a caller posing joints itself.
    #[inline]
    #[must_use]
    pub fn locals_mut(&mut self) -> &mut [Trs] {
        &mut self.locals
    }

    /// How many joints this pose covers.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.locals.len()
    }

    /// Whether this pose covers no joints.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.locals.is_empty()
    }
}

/// Where a requested time falls in one channel's keyframe array.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Segment {
    /// The time is *at* keyframe `k`, or clamped to it. §C:
    ///
    /// > When the current (requested) timestamp exists in the animation data,
    /// > its associated property value **MUST** be used as-is, without
    /// > interpolation.
    ///
    /// Carried as its own case rather than left to fall out of the arithmetic
    /// because for a rotation it does not: the slerp of §C.3 divides by
    /// `sin(a)` and would answer a keyframe's own value only to within a few
    /// ulps.
    Exact(usize),
    /// The time is strictly inside the segment from keyframe `k` to `k + 1`.
    Between {
        /// The index of the earlier keyframe.
        k: usize,
        /// `t = (t_c - t_k) / t_d`, the segment-normalized interpolation
        /// factor, strictly between 0 and 1.
        t: f32,
        /// `t_d = t_{k + 1} - t_k`, the duration of the interpolation segment.
        /// Needed by `CUBICSPLINE`, whose tangents are scaled by it.
        t_d: f32,
    },
}

/// Finds the segment a time falls in, by binary search.
///
/// `times` is non-empty, strictly ascending and finite — [`Channel::new`] is
/// what guarantees all three, and the search below is why it does.
///
/// [`Channel::new`]: crate::Channel::new
fn locate(times: &[f32], time: f32) -> Segment {
    let last = times.len() - 1;
    // Clamped, per the module docs. A `NaN` request holds the first keyframe:
    // it compares false against every timestamp, so no segment contains it.
    if time.is_nan() || time <= times[0] {
        return Segment::Exact(0);
    }
    if time >= times[last] {
        return Segment::Exact(last);
    }
    // `times[0] < time < times[last]`, so the partition point is in
    // `1..=last` and `k + 1` is in bounds.
    let k = times.partition_point(|&keyframe| keyframe <= time) - 1;
    if times[k] == time {
        return Segment::Exact(k);
    }
    let t_d = times[k + 1] - times[k];
    Segment::Between {
        k,
        t: (time - times[k]) / t_d,
        t_d,
    }
}

/// The array positions of keyframe `k`'s in-tangent, value and out-tangent
/// under `CUBICSPLINE` — `a_k`, `v_k` and `b_k` in the specification's
/// notation. §C.4:
///
/// > For each timestamp stored in the animation sampler, there are three
/// > associated keyframe values: in-tangent, property value, and out-tangent.
#[inline]
const fn cubic_triple(k: usize) -> (usize, usize, usize) {
    let base = k * CUBIC_SPLINE_VALUES_PER_KEYFRAME;
    (base, base + 1, base + 2)
}

/// The four cubic Hermite basis terms of §C.4, at `t` over a segment of
/// duration `t_d`. The specification's equation, with `a_k`, `v_k` and `b_k`
/// the in-tangent, the value and the out-tangent of keyframe `k`:
///
/// ```text
/// v_t = (2t^3 - 3t^2 + 1) * v_k
///     + t_d(t^3 - 2t^2 + t) * b_k
///     + (-2t^3 + 3t^2) * v_{k+1}
///     + t_d(t^3 - t^2) * a_{k+1}
/// ```
///
/// Returned in that order, so the caller reads as the lines above do. **The
/// two tangent terms are scaled by `t_d`** and this is the whole of what makes
/// the spline agree with the keyframe spacing: drop the factor and a curve
/// authored at 24 fps is evaluated as though every segment lasted one second.
#[inline]
fn hermite(t: f32, t_d: f32) -> [f32; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        2.0 * t3 - 3.0 * t2 + 1.0,
        t_d * (t3 - 2.0 * t2 + t),
        -2.0 * t3 + 3.0 * t2,
        t_d * (t3 - t2),
    ]
}

/// Samples a translation or scale track.
///
/// `LINEAR` here is §C.2, the mode used "when the animation sampler
/// interpolation mode is set to `LINEAR` and the animated property is **not**
/// `rotation`":
///
/// ```text
/// v_t = (1 - t) * v_k + t * v_{k+1}
/// ```
///
/// written out as the specification spells it rather than as a `lerp` helper,
/// so the line above and the line below can be read against each other.
///
/// `STEP` is §C.1, `v_t = v_k` — the earlier keyframe's value held across the
/// whole segment, not the nearer one.
fn sample_vec3(values: &[Vec3], interpolation: Interpolation, segment: Segment) -> Vec3 {
    match segment {
        Segment::Exact(k) => match interpolation {
            Interpolation::Linear | Interpolation::Step => values[k],
            Interpolation::CubicSpline => values[cubic_triple(k).1],
        },
        Segment::Between { k, t, t_d } => match interpolation {
            Interpolation::Step => values[k],
            Interpolation::Linear => values[k] * (1.0 - t) + values[k + 1] * t,
            Interpolation::CubicSpline => {
                let (_, v_k, b_k) = cubic_triple(k);
                let (a_next, v_next, _) = cubic_triple(k + 1);
                let [h00, h10, h01, h11] = hermite(t, t_d);
                values[v_k] * h00 + values[b_k] * h10 + values[v_next] * h01 + values[a_next] * h11
            }
        },
    }
}

/// Samples a rotation track.
///
/// **`LINEAR` on a rotation is not the linear interpolation of §C.2.** §C.3
/// gives it its own section, "Spherical Linear Interpolation", which
///
/// > is used when the animation sampler interpolation mode is set to `LINEAR`
/// > and the animated property is `rotation`, i.e., values of the animated
/// > property are unit quaternions.
///
/// with `a = arccos(|v_k · v_{k+1}|)` and `s` the sign of that dot product:
///
/// ```text
/// v_t = sin(a(1 - t))/sin(a) * v_k + s * sin(at)/sin(a) * v_{k+1}
/// ```
///
/// [`Quat::slerp`] is that expression. It negates `v_{k+1}` when the dot
/// product is negative and takes the arccosine of the resulting non-negative
/// dot, which is the same thing as the `|·|` and the `s` above and has the
/// effect the specification's implementation note describes — "ensure that the
/// spherical interpolation follows the short path along the great circle". Near
/// `a = 0` it falls back to a normalized linear interpolation, which is the
/// other implementation note in the same section: "When `a` is close to zero,
/// spherical linear interpolation turns into regular linear interpolation", and
/// which "Implementations **MAY** approximate these equations to reach
/// application-specific accuracy and/or performance targets" permits.
///
/// `CUBICSPLINE` on a rotation is the same polynomial as for a vector, and then
/// §C.4's extra sentence:
///
/// > When the animation sampler targets a node's rotation property, the
/// > interpolated quaternion **MUST** be normalized before applying the result
/// > to the node's rotation.
///
/// The polynomial is evaluated on the four components independently and does
/// not preserve length, so without that normalization a spline-interpolated
/// rotation also scales the joint.
fn sample_quat(values: &[Quat], interpolation: Interpolation, segment: Segment) -> Quat {
    match segment {
        Segment::Exact(k) => match interpolation {
            Interpolation::Linear | Interpolation::Step => values[k],
            Interpolation::CubicSpline => values[cubic_triple(k).1],
        },
        Segment::Between { k, t, t_d } => match interpolation {
            Interpolation::Step => values[k],
            Interpolation::Linear => values[k].slerp(values[k + 1], t),
            Interpolation::CubicSpline => {
                let (_, v_k, b_k) = cubic_triple(k);
                let (a_next, v_next, _) = cubic_triple(k + 1);
                let [h00, h10, h01, h11] = hermite(t, t_d);
                let spline = values[v_k] * h00
                    + values[b_k] * h10
                    + values[v_next] * h01
                    + values[a_next] * h11;
                spline.normalize()
            }
        },
    }
}

impl Clip {
    /// Samples every channel at `time` seconds and writes one local transform
    /// per joint into `pose`.
    ///
    /// Allocates nothing: `pose` is reused frame to frame. It is reset to the
    /// skeleton's rest transforms first, so the result depends only on this
    /// clip and this time — a pose carried over from another clip leaves
    /// nothing behind on a joint this one does not drive.
    ///
    /// See the [module docs](self) for what happens to a channel naming a joint
    /// this skeleton has not got, to a joint no channel drives, and to a time
    /// outside the clip.
    ///
    /// # Panics
    ///
    /// If `pose` was not built for a skeleton of this size. [`Pose::new`] is
    /// the only constructor, so that means a pose from a *different* skeleton,
    /// which no clamping could make meaningful — the palette indices would name
    /// different bones.
    pub fn sample_into(&self, time: f32, skeleton: &Skeleton, pose: &mut Pose) {
        assert_eq!(
            pose.locals.len(),
            skeleton.len(),
            "this pose was built for a skeleton with a different joint count"
        );
        for (local, joint) in pose.locals.iter_mut().zip(skeleton.joints()) {
            *local = joint.rest;
        }
        for channel in self.channels() {
            // A channel naming a joint outside this skeleton drives something
            // else in the source document; see the module docs.
            let Some(local) = pose.locals.get_mut(channel.joint()) else {
                continue;
            };
            let interpolation = channel.interpolation();
            let segment = locate(channel.times(), time);
            match channel.track() {
                Track::Translation(values) => {
                    local.translation = sample_vec3(values, interpolation, segment);
                }
                Track::Rotation(values) => {
                    local.rotation = sample_quat(values, interpolation, segment);
                }
                Track::Scale(values) => {
                    local.scale = sample_vec3(values, interpolation, segment);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Segment, locate};

    const TIMES: [f32; 4] = [0.0, 1.0, 4.0, 5.0];

    #[test]
    fn finds_the_segment_a_time_is_inside() {
        assert_eq!(
            locate(&TIMES, 2.0),
            Segment::Between {
                k: 1,
                t: 1.0 / 3.0,
                t_d: 3.0,
            }
        );
    }

    #[test]
    fn reports_a_time_that_is_a_keyframe_as_exact() {
        assert_eq!(locate(&TIMES, 4.0), Segment::Exact(2));
    }

    #[test]
    fn clamps_a_time_outside_the_channel() {
        assert_eq!(locate(&TIMES, -100.0), Segment::Exact(0));
        assert_eq!(locate(&TIMES, 100.0), Segment::Exact(3));
    }

    #[test]
    fn holds_the_first_keyframe_for_a_nan_time() {
        assert_eq!(locate(&TIMES, f32::NAN), Segment::Exact(0));
    }

    #[test]
    fn handles_a_single_keyframe() {
        assert_eq!(locate(&[2.5], 0.0), Segment::Exact(0));
        assert_eq!(locate(&[2.5], 2.5), Segment::Exact(0));
        assert_eq!(locate(&[2.5], 9.0), Segment::Exact(0));
    }
}
