//! Clips and the keyframe channels they are made of.

use std::collections::HashSet;
use std::{fmt, mem};

use glam::{Quat, Vec3};

/// How to read between two keyframes.
///
/// One variant per glTF `animation.sampler.interpolation` value. What each
/// means is defined in Appendix C of the specification and implemented in
/// [`crate::sample`]; the variants carry no data because the *arithmetic*
/// differs, not the storage — except that [`CubicSpline`](Self::CubicSpline)
/// stores three values per keyframe, which [`Channel::new`] checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Interpolation {
    /// Linearly between the two surrounding keyframes — *spherically* for a
    /// rotation, which is a different operation and not a detail. See
    /// [`crate::sample`].
    Linear,
    /// The earlier keyframe's value, held until the later one.
    Step,
    /// A cubic Hermite spline whose tangents share the value array: three
    /// values per keyframe, in-tangent then value then out-tangent.
    CubicSpline,
}

/// A channel's values, and which of a joint's three components they drive.
///
/// One variant per glTF `animation.channel.target.path` that a skeleton cares
/// about, and the variant *is* the path — a consumer matches once rather than
/// matching a path and then trusting an array to agree with it. The fourth
/// glTF path, `weights`, drives morph targets rather than joints and has no
/// variant here: this crate poses a skeleton, and morph targets are outside
/// its scope.
///
/// Under [`Interpolation::CubicSpline`] each keyframe occupies three
/// consecutive entries — in-tangent, value, out-tangent — so the array is three
/// times as long as the time array rather than equal to it. §C.4:
///
/// > For each timestamp stored in the animation sampler, there are three
/// > associated keyframe values: in-tangent, property value, and out-tangent.
#[derive(Clone, Debug, PartialEq)]
pub enum Track {
    /// Translations, in the document's own units.
    Translation(Vec<Vec3>),
    /// Rotations, as unit quaternions.
    Rotation(Vec<Quat>),
    /// Scales, per axis.
    Scale(Vec<Vec3>),
}

impl Track {
    /// How many values this track holds, tangents included.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Translation(values) | Self::Scale(values) => values.len(),
            Self::Rotation(values) => values.len(),
        }
    }

    /// Whether this track holds no values at all.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Why a keyframe track could not become a [`Channel`].
///
/// Every variant is a document that cannot be sampled at all, as opposed to the
/// three cases [`crate::sample`] treats as ordinary — a channel naming a joint
/// this skeleton has not got, a joint no channel drives, and a time outside the
/// clip. Those are shapes a well-formed document takes; these are not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipError {
    /// The channel has no keyframes. §C: "let `n` be the total number of
    /// keyframes, `n > 0`".
    NoKeyframes,
    /// The value count does not match the keyframe count.
    ValueCountMismatch {
        /// How many keyframe times there are.
        times: usize,
        /// How many values there are.
        values: usize,
        /// How many values each keyframe occupies: three under
        /// [`Interpolation::CubicSpline`], one otherwise.
        per_keyframe: usize,
    },
    /// A [`Interpolation::CubicSpline`] channel has a single keyframe. §C.4:
    /// "An animation sampler that uses cubic spline interpolation **MUST**
    /// have at least 2 keyframes."
    CubicSplineNeedsTwoKeyframes,
    /// Keyframe times do not strictly ascend.
    ///
    /// Refused rather than tolerated because both halves of sampling depend on
    /// it: the segment is found by binary search, which a non-monotone array
    /// makes meaningless, and the normalized factor `t = (t_c - t_k) / t_d`
    /// divides by the segment duration, which a repeated timestamp makes zero.
    TimesNotAscending {
        /// The index whose time does not exceed its predecessor's.
        keyframe: usize,
    },
    /// A keyframe time is `NaN` or an infinity.
    ///
    /// Checked separately from the ordering so that the sampler's arithmetic is
    /// total: with every timestamp finite, comparing two of them is a total
    /// order and the binary search cannot walk off the array. A `NaN` compares
    /// false against everything, which is how a partition point ends up at zero
    /// and a segment index underflows.
    TimeNotFinite {
        /// The index whose time is not a finite number.
        keyframe: usize,
    },
}

impl fmt::Display for ClipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NoKeyframes => f.write_str("a channel must have at least one keyframe"),
            Self::ValueCountMismatch {
                times,
                values,
                per_keyframe,
            } => write!(
                f,
                "{times} keyframe(s) want {} value(s) ({per_keyframe} each), not {values}",
                times * per_keyframe
            ),
            Self::CubicSplineNeedsTwoKeyframes => {
                f.write_str("cubic spline interpolation needs at least 2 keyframes")
            }
            Self::TimesNotAscending { keyframe } => write!(
                f,
                "keyframe {keyframe} does not come strictly after keyframe {}",
                keyframe - 1
            ),
            Self::TimeNotFinite { keyframe } => {
                write!(
                    f,
                    "keyframe {keyframe} has a time that is not a finite number"
                )
            }
        }
    }
}

impl std::error::Error for ClipError {}

/// One keyframe track, bound to one joint and one of its three components.
///
/// A glTF channel is a target plus a sampler; the sampler is flattened in here
/// rather than shared, which is the shape `crcbl_scene::GltfChannel` already
/// arrives in.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    joint: usize,
    times: Vec<f32>,
    interpolation: Interpolation,
    track: Track,
}

impl Channel {
    /// Takes a joint's palette index, its keyframe times in seconds, an
    /// interpolation mode and the values.
    ///
    /// The joint index is **not** checked here, and cannot be: a clip is built
    /// without a skeleton in hand, and the same clip is meant to be playable on
    /// more than one. [`Clip::sample_into`](crate::Clip::sample_into) is where
    /// a channel that names no joint of *this* skeleton is skipped.
    ///
    /// # Errors
    ///
    /// [`ClipError`] for a track that could not be sampled at any time at all —
    /// see that type for the shapes it refuses and why each is fatal rather
    /// than tolerated.
    pub fn new(
        joint: usize,
        times: Vec<f32>,
        interpolation: Interpolation,
        track: Track,
    ) -> Result<Self, ClipError> {
        if times.is_empty() {
            return Err(ClipError::NoKeyframes);
        }
        if interpolation == Interpolation::CubicSpline && times.len() < 2 {
            return Err(ClipError::CubicSplineNeedsTwoKeyframes);
        }
        let per_keyframe = values_per_keyframe(interpolation);
        if track.len() != times.len() * per_keyframe {
            return Err(ClipError::ValueCountMismatch {
                times: times.len(),
                values: track.len(),
                per_keyframe,
            });
        }
        for (keyframe, &time) in times.iter().enumerate() {
            if !time.is_finite() {
                return Err(ClipError::TimeNotFinite { keyframe });
            }
            if keyframe > 0 && time <= times[keyframe - 1] {
                return Err(ClipError::TimesNotAscending { keyframe });
            }
        }
        Ok(Self {
            joint,
            times,
            interpolation,
            track,
        })
    }

    /// Which joint of the skeleton this channel drives, by palette index.
    #[inline]
    #[must_use]
    pub const fn joint(&self) -> usize {
        self.joint
    }

    /// The keyframe times, in seconds. Never empty, and strictly ascending.
    #[inline]
    #[must_use]
    pub fn times(&self) -> &[f32] {
        &self.times
    }

    /// How to read between two keyframes.
    #[inline]
    #[must_use]
    pub const fn interpolation(&self) -> Interpolation {
        self.interpolation
    }

    /// The values, and which component they drive.
    #[inline]
    #[must_use]
    pub const fn track(&self) -> &Track {
        &self.track
    }

    /// The time of this channel's last keyframe.
    #[inline]
    #[must_use]
    pub fn end_time(&self) -> f32 {
        // Non-empty by construction, so the last element exists.
        self.times[self.times.len() - 1]
    }
}

/// How many entries of a [`Track`] one `CUBICSPLINE` keyframe occupies.
///
/// The in-tangent, the value and the out-tangent, in that order — §C.4. Named
/// rather than written out at each use because two places depend on it: this
/// module checks the array length against it, and [`crate::sample`] indexes
/// with it.
pub(crate) const CUBIC_SPLINE_VALUES_PER_KEYFRAME: usize = 3;

/// How many values one keyframe of a channel occupies.
///
/// Not public: it is a property of the storage layout, and [`Track::len`] is
/// what a caller wanting to reason about the array uses.
#[inline]
const fn values_per_keyframe(interpolation: Interpolation) -> usize {
    match interpolation {
        Interpolation::Linear | Interpolation::Step => 1,
        Interpolation::CubicSpline => CUBIC_SPLINE_VALUES_PER_KEYFRAME,
    }
}

/// A set of channels over one skeleton, and the time span they cover.
///
/// Two clips are equal when their channels and durations are, and print as
/// their channels and duration: the sampling plan is derived from the channels
/// and is not part of what a clip is.
#[derive(Clone, Default)]
pub struct Clip {
    channels: Vec<Channel>,
    duration: f32,
    /// The channels [`Clip::sample_into`](crate::Clip::sample_into) evaluates,
    /// by index into `channels`, grouped so that channels with the same
    /// keyframe times sit together. Built once, by [`schedule`].
    pub(crate) schedule: Vec<usize>,
    /// Where each group of `schedule` ends; each starts where the one before
    /// it ended.
    ///
    /// Every channel of a group has the same keyframe times, bit for bit.
    /// Sampling finds a time's segment from the times alone, so the whole group
    /// lands in one segment and the search is done once for all of it. Baked
    /// and exported clips commonly key every channel on one shared timeline,
    /// which turns one search per channel into one per sample.
    pub(crate) timeline_ends: Vec<usize>,
}

/// Which channels a sample evaluates, and in which groups.
///
/// **A channel overwritten by a later one is left out.** Two channels driving
/// the same component of the same joint leave the later one's value, per
/// [`Clip::new`], and sampling a channel has no other effect — so the earlier
/// is dropped here rather than evaluated and discarded. That is also what lets
/// the rest be evaluated in any order: no two of them write the same place.
///
/// The rest are grouped by keyframe times, compared **bit for bit** rather than
/// as numbers. Equal bits are what makes sharing the search provably the same
/// answer, since the search reads nothing else; a `-0.0` against a `0.0` is two
/// timelines, which costs a second search and changes nothing.
fn schedule(channels: &[Channel]) -> (Vec<usize>, Vec<usize>) {
    let mut written = HashSet::new();
    let mut live: Vec<usize> = (0..channels.len())
        .rev()
        .filter(|&index| {
            let channel = &channels[index];
            written.insert((channel.joint, mem::discriminant(&channel.track)))
        })
        .collect();
    let bits = |index: usize| channels[index].times.iter().map(|time| time.to_bits());
    // Equal time arrays sort next to each other; within one, channel order.
    live.sort_by(|&a, &b| bits(a).cmp(bits(b)).then(a.cmp(&b)));
    let mut ends: Vec<usize> = Vec::new();
    for (position, &index) in live.iter().enumerate() {
        let shares = position > 0 && bits(live[position - 1]).eq(bits(index));
        match ends.last_mut() {
            Some(end) if shares => *end = position + 1,
            _ => ends.push(position + 1),
        }
    }
    (live, ends)
}

impl PartialEq for Clip {
    fn eq(&self, other: &Self) -> bool {
        // The plan is left out: it follows from the channels, but compares
        // their times by bits, so two clips with equal channels may differ in
        // it — a `0.0` key against a `-0.0` one.
        self.channels == other.channels && self.duration == other.duration
    }
}

impl fmt::Debug for Clip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Clip")
            .field("channels", &self.channels)
            .field("duration", &self.duration)
            .finish()
    }
}

impl Clip {
    /// Takes the channels, in any order.
    ///
    /// Several channels may drive one joint — a translation curve and a
    /// rotation curve on the same joint is the usual case — so this is not keyed
    /// by joint, and no channel is checked against another. Two channels driving
    /// the *same* component of the same joint is a malformed document rather
    /// than a shape with a defined meaning; the later one in this array wins,
    /// which is arbitrary and is documented only so that it is not surprising.
    #[must_use]
    pub fn new(channels: Vec<Channel>) -> Self {
        let duration = channels
            .iter()
            .map(Channel::end_time)
            .fold(0.0_f32, f32::max);
        let (schedule, timeline_ends) = schedule(&channels);
        Self {
            channels,
            duration,
            schedule,
            timeline_ends,
        }
    }

    /// The clip's channels, in the order they were given.
    #[inline]
    #[must_use]
    pub fn channels(&self) -> &[Channel] {
        &self.channels
    }

    /// The time of the last keyframe of any channel, in seconds.
    ///
    /// Zero for a clip with no channels. This is a *duration* and not a range:
    /// a clip whose first keyframe sits after zero holds that keyframe from
    /// zero, per [`crate::sample`]'s clamping rule, so the playable span always
    /// starts at zero.
    #[inline]
    #[must_use]
    pub const fn duration(&self) -> f32 {
        self.duration
    }
}

#[cfg(test)]
mod tests {
    use super::{Channel, Clip, ClipError, Interpolation, Track};
    use glam::{Quat, Vec3};

    #[test]
    fn refuses_a_channel_with_no_keyframes() {
        let error = Channel::new(
            0,
            Vec::new(),
            Interpolation::Linear,
            Track::Scale(Vec::new()),
        )
        .expect_err("a channel with no keyframes has no value at any time");
        assert_eq!(error, ClipError::NoKeyframes);
    }

    #[test]
    fn refuses_a_value_count_that_does_not_match() {
        let error = Channel::new(
            0,
            vec![0.0, 1.0],
            Interpolation::Linear,
            Track::Translation(vec![Vec3::ZERO]),
        )
        .expect_err("two keyframes want two values under LINEAR");
        assert_eq!(
            error,
            ClipError::ValueCountMismatch {
                times: 2,
                values: 1,
                per_keyframe: 1,
            }
        );
    }

    /// The tangents are the whole difference: the same two timestamps want six
    /// values under `CUBICSPLINE` and two under `LINEAR`.
    #[test]
    fn cubic_spline_wants_three_values_per_keyframe() {
        let error = Channel::new(
            0,
            vec![0.0, 1.0],
            Interpolation::CubicSpline,
            Track::Translation(vec![Vec3::ZERO, Vec3::ZERO]),
        )
        .expect_err("two cubic-spline keyframes want six values");
        assert_eq!(
            error,
            ClipError::ValueCountMismatch {
                times: 2,
                values: 2,
                per_keyframe: 3,
            }
        );

        Channel::new(
            0,
            vec![0.0, 1.0],
            Interpolation::CubicSpline,
            Track::Translation(vec![Vec3::ZERO; 6]),
        )
        .expect("six values is what two cubic-spline keyframes want");
    }

    #[test]
    fn refuses_one_cubic_spline_keyframe() {
        let error = Channel::new(
            0,
            vec![0.0],
            Interpolation::CubicSpline,
            Track::Rotation(vec![Quat::IDENTITY; 3]),
        )
        .expect_err("the specification requires at least two");
        assert_eq!(error, ClipError::CubicSplineNeedsTwoKeyframes);
    }

    #[test]
    fn refuses_times_that_do_not_ascend() {
        let error = Channel::new(
            0,
            vec![0.0, 2.0, 1.0],
            Interpolation::Step,
            Track::Scale(vec![Vec3::ONE; 3]),
        )
        .expect_err("a binary search over a non-monotone array means nothing");
        assert_eq!(error, ClipError::TimesNotAscending { keyframe: 2 });
    }

    /// A repeated timestamp is refused as well as a descending one: it is what
    /// would make the segment duration zero and the normalized factor infinite.
    #[test]
    fn refuses_a_repeated_time() {
        let error = Channel::new(
            0,
            vec![0.0, 1.0, 1.0],
            Interpolation::Linear,
            Track::Scale(vec![Vec3::ONE; 3]),
        )
        .expect_err("a zero-length segment divides by zero");
        assert_eq!(error, ClipError::TimesNotAscending { keyframe: 2 });
    }

    #[test]
    fn refuses_a_time_that_is_not_finite() {
        let error = Channel::new(
            0,
            vec![0.0, f32::NAN],
            Interpolation::Linear,
            Track::Scale(vec![Vec3::ONE; 2]),
        )
        .expect_err("a NaN timestamp makes every comparison in the sampler false");
        assert_eq!(error, ClipError::TimeNotFinite { keyframe: 1 });
    }

    fn lift(joint: usize, start: f32) -> Channel {
        Channel::new(
            joint,
            vec![start, 1.0],
            Interpolation::Linear,
            Track::Translation(vec![Vec3::ZERO, Vec3::Y]),
        )
        .expect("well formed")
    }

    /// Timelines are grouped by bits, not by value: a `0.0` start and a `-0.0`
    /// start are two groups, while two arrays equal in bits are one.
    #[test]
    fn groups_timelines_by_bits() {
        let clip = Clip::new(vec![lift(0, 0.0), lift(1, -0.0), lift(2, 0.0)]);
        assert_eq!(clip.timeline_ends.len(), 2, "{:?}", clip.timeline_ends);
        assert_eq!(clip.schedule.len(), 3);
    }

    /// The plan is not part of a clip's equality: equal channels are equal
    /// clips, as before the plan existed, even where the plan differs.
    #[test]
    fn equality_ignores_the_plan() {
        let positive = Clip::new(vec![lift(0, 0.0), lift(1, 0.0)]);
        let negative = Clip::new(vec![lift(0, 0.0), lift(1, -0.0)]);
        assert_ne!(positive.timeline_ends, negative.timeline_ends);
        assert_eq!(positive, negative);
    }

    #[test]
    fn duration_is_the_last_keyframe_of_any_channel() {
        let short = Channel::new(
            0,
            vec![0.0, 1.0],
            Interpolation::Step,
            Track::Scale(vec![Vec3::ONE; 2]),
        )
        .expect("well formed");
        let long = Channel::new(
            1,
            vec![0.0, 4.5],
            Interpolation::Step,
            Track::Scale(vec![Vec3::ONE; 2]),
        )
        .expect("well formed");
        assert_eq!(Clip::new(vec![short, long]).duration(), 4.5);
        assert_eq!(Clip::new(Vec::new()).duration(), 0.0);
    }
}
