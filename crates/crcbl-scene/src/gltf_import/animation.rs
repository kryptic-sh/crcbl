//! A glTF document's animation clips: its keyframes as the file holds them,
//! and [`read_clips`], which gathers them into [`GltfClip`]s.

use std::path::Path;

use crcbl_assets::StorageError;
use gltf::animation::Interpolation;
use gltf::animation::util::ReadOutputs;

use crate::gltf_check::malformed;

/// One entry of the document's `animations` array: a name and the channels that
/// make it move.
///
/// Nothing here is resampled, retimed or sorted. These are the file's own
/// keyframes, in the file's own seconds, which is what the animation rules in
/// `docs/notes/simulation.md` ask of the source stage — the cook that turns
/// them into fixed-rate curves is a later one and needs the samples it started
/// from.
#[derive(Clone, Debug, PartialEq)]
pub struct GltfClip {
    name: Option<String>,
    channels: Vec<GltfChannel>,
}

impl GltfClip {
    /// The name the document gave this animation, if it gave one.
    #[inline]
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The clip's channels, in file order.
    ///
    /// Several channels can drive one node — a translation curve and a rotation
    /// curve on the same joint is the usual case — so this is not keyed by node.
    #[inline]
    #[must_use]
    pub fn channels(&self) -> &[GltfChannel] {
        &self.channels
    }
}

/// One animation channel: what it drives, when, and with what.
///
/// A glTF channel is a target plus a sampler, and the sampler is flattened into
/// this type rather than shared: samplers are already per-animation, and a
/// channel that had to look one up by index would be a fourth array to keep in
/// step.
#[derive(Clone, Debug, PartialEq)]
pub struct GltfChannel {
    node: usize,
    times: Vec<f32>,
    interpolation: GltfInterpolation,
    samples: GltfSamples,
}

impl GltfChannel {
    /// Which of [`GltfScene::nodes`](super::GltfScene::nodes) this channel drives.
    ///
    /// Always a valid index — a channel naming a node that does not exist makes
    /// the document malformed.
    #[inline]
    #[must_use]
    pub const fn node(&self) -> usize {
        self.node
    }

    /// The keyframe times, in seconds, as the file gives them.
    ///
    /// Never empty: an accessor with a count of zero is refused before this is
    /// built. The specification requires them to ascend and this importer does
    /// not verify that it did — nothing here searches them, and a player that
    /// does has to decide what a file that disobeys should look like.
    #[inline]
    #[must_use]
    pub fn times(&self) -> &[f32] {
        &self.times
    }

    /// How to read between two keyframes.
    #[inline]
    #[must_use]
    pub const fn interpolation(&self) -> GltfInterpolation {
        self.interpolation
    }

    /// The sampled values, one per keyframe — or three per keyframe under
    /// [`GltfInterpolation::CubicSpline`], which stores an in-tangent, the
    /// value and an out-tangent for each.
    ///
    /// That ratio is checked at import, so a channel whose samples do not line
    /// up with its times makes the document malformed rather than arriving here
    /// for a player to trip over.
    #[inline]
    #[must_use]
    pub const fn samples(&self) -> &GltfSamples {
        &self.samples
    }
}

/// How an animation sampler reads between two keyframes — glTF's
/// `interpolation`.
///
/// This crate's own enum rather than `gltf`'s, for the reason `crcbl-scene`
/// keeps every other one: [`GltfScene`](super::GltfScene) is what leaves this crate, and a
/// consumer of it should not have to depend on the parser to match on a result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GltfInterpolation {
    /// Linearly between the two surrounding keyframes — spherically, for a
    /// rotation.
    Linear,
    /// The earlier keyframe's value, until the later one.
    Step,
    /// A cubic spline, whose tangents share the sample array: see
    /// [`GltfChannel::samples`].
    CubicSpline,
}

/// What an animation channel drives, and the values it drives it with.
///
/// One variant per glTF `target.path`, and the variant *is* the path: a channel
/// carrying rotations is a rotation channel, so a consumer matches once rather
/// than matching a path and then trusting an array to agree with it.
#[derive(Clone, Debug, PartialEq)]
pub enum GltfSamples {
    /// Node translations, in the document's own units.
    Translations(Vec<[f32; 3]>),
    /// Node rotations, as `xyzw` quaternions — the order the format stores
    /// them, and the order [`glam::Quat::from_array`] reads.
    ///
    /// Normalised to `f32` whichever of the legal component types the file
    /// used: the specification permits a quaternion stored as normalized
    /// signed or unsigned bytes or shorts.
    Rotations(Vec<[f32; 4]>),
    /// Node scales, per axis.
    Scales(Vec<[f32; 3]>),
    /// Morph-target weights, `targets` of them per keyframe, flat.
    ///
    /// Read even though the targets themselves are not — see the [module
    /// docs](super).
    MorphWeights(Vec<f32>),
}

/// The document's `animations` array, one [`GltfClip`] per entry.
///
/// Each channel's sampler is flattened into the channel — see [`GltfChannel`]
/// — so a clip is a list of curves rather than a list of curves and a list of
/// the arrays they point at.
pub(super) fn read_clips(
    document: &gltf::Document,
    buffers: &[Vec<u8>],
    key: &Path,
) -> Result<Vec<GltfClip>, StorageError> {
    let buffer = |buffer: gltf::Buffer<'_>| buffers.get(buffer.index()).map(Vec::as_slice);
    let mut clips = Vec::with_capacity(document.animations().len());
    for animation in document.animations() {
        let mut channels = Vec::new();
        for channel in animation.channels() {
            let at = format!(
                "animation {} channel {}",
                animation.index(),
                channel.index()
            );
            let reader = channel.reader(buffer);
            let times: Vec<f32> = reader
                .read_inputs()
                .ok_or_else(|| malformed(key, format!("{at}'s input accessor reads nothing")))?
                .collect();
            let samples = match reader
                .read_outputs()
                .ok_or_else(|| malformed(key, format!("{at}'s output accessor reads nothing")))?
            {
                ReadOutputs::Translations(read) => GltfSamples::Translations(read.collect()),
                // `into_f32` because the spec permits a quaternion stored as a
                // normalized integer, and un-normalising it is the step a
                // hand-read gets wrong.
                ReadOutputs::Rotations(read) => GltfSamples::Rotations(read.into_f32().collect()),
                ReadOutputs::Scales(read) => GltfSamples::Scales(read.collect()),
                ReadOutputs::MorphTargetWeights(read) => {
                    GltfSamples::MorphWeights(read.into_f32().collect())
                }
            };
            let interpolation = match channel.sampler().interpolation() {
                Interpolation::Linear => GltfInterpolation::Linear,
                Interpolation::Step => GltfInterpolation::Step,
                Interpolation::CubicSpline => GltfInterpolation::CubicSpline,
            };
            check_sample_count(&times, &samples, interpolation, &at, key)?;
            channels.push(GltfChannel {
                node: channel.target().node().index(),
                times,
                interpolation,
                samples,
            });
        }
        clips.push(GltfClip {
            name: animation.name().map(str::to_owned),
            channels,
        });
    }
    Ok(clips)
}

/// Refuse a channel whose samples do not line up with its keyframes.
///
/// One value per keyframe, or three under `CUBICSPLINE`, which stores an
/// in-tangent, the value and an out-tangent for each. A morph-target channel is
/// the one that carries several values per keyframe by design — one per target
/// — so it is checked as a multiple rather than an equality.
///
/// Checked here rather than in [`crate::gltf_check`] because this is the count
/// the reader actually produced, and because [`GltfChannel::samples`] promises
/// it: a player that steps a curve indexes both arrays off one keyframe number.
fn check_sample_count(
    times: &[f32],
    samples: &GltfSamples,
    interpolation: GltfInterpolation,
    at: &str,
    key: &Path,
) -> Result<(), StorageError> {
    let per_keyframe = match interpolation {
        GltfInterpolation::Linear | GltfInterpolation::Step => 1,
        GltfInterpolation::CubicSpline => 3,
    };
    let expected = times.len() * per_keyframe;
    let (what, found) = match samples {
        GltfSamples::Translations(values) => ("translation", values.len()),
        GltfSamples::Rotations(values) => ("rotation", values.len()),
        GltfSamples::Scales(values) => ("scale", values.len()),
        // Several weights per keyframe, one per morph target the mesh has, so
        // the count has to divide rather than match.
        GltfSamples::MorphWeights(values) => {
            if values.len() % expected == 0 && !values.is_empty() {
                return Ok(());
            }
            ("morph weight", values.len())
        }
    };
    if found == expected {
        return Ok(());
    }
    Err(malformed(
        key,
        format!(
            "{at} has {} keyframes and {found} {what} values, and {interpolation:?} \
             interpolation wants {expected}",
            times.len()
        ),
    ))
}
