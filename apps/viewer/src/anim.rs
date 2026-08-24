//! The document's rig: converted for [`crcbl::anim`], played, and drawn.
//!
//! ```text
//! GltfSkin  ─ joints, inverse binds ─┐
//! GltfNode  ─ hierarchy, rest pose ──┴─▶ Skeleton ─┐
//! GltfClip  ─ channels ──────────────────▶ Clip ───┴─▶ Playable   (on the Model)
//!                                                          │
//!                              Player::advance ────────────┘
//!                                Clip::sample_into ──▶ Pose
//!                                  Palette::compute ──▶ globals ──▶ Player::draw
//! ```
//!
//! # The conversion is this application's, and that is deliberate upstream
//!
//! `crcbl-anim` depends on `glam` and nothing else — in particular not on
//! `crcbl-scene`, so that a browser build which only plays cooked clips never
//! links a glTF parser. Its module docs say the index bookkeeping between the
//! two "belongs to whoever holds both", and this application is the first thing
//! in the tree that does.
//!
//! # The two indices that are not the same index
//!
//! Both mistakes compile, draw something, and are wrong.
//!
//! * **A joint's palette index is its position in the skin's `joints` array**,
//!   and that number is not this code's to choose: a skinned vertex's
//!   `JOINTS_0` attribute indexes that same array, and the skin's
//!   `inverseBindMatrices` are in that same order. So `skeleton_of` below walks
//!   `skin.joints()` in order and renumbers nothing.
//! * **A channel's target is a *node* index**, because a glTF animation drives
//!   nodes rather than joints, and it has to be mapped through the skin's joint
//!   list before it can name a [`Channel`]'s joint — see `joint_of` below.
//!   Handing
//!   the node index straight to [`Channel::new`] builds a clip whose channels
//!   name joints that mostly do not exist, which
//!   [`Clip::sample_into`](crcbl::anim::Clip::sample_into) then skips in
//!   silence: the skeleton simply never moves.
//!
//! # What is dropped without comment, and what is reported
//!
//! A channel naming a node **this skin has not got** is dropped silently: a
//! document's nodes are not all joints of the skin being posed — the mesh node,
//! a camera, a prop hanging off a hand and every joint of a *second* skin are
//! all animated by channels in the same clip — so it is the ordinary shape of a
//! well-formed file rather than a loss. `crcbl-anim`'s
//! [`sample`](crcbl::anim::sample) module documents the same case the same way.
//!
//! Everything else that could not be brought in becomes a [`Skip`], which is
//! the channel this application already reports a lost feature through: the
//! loader prints them to stderr, [`crate::listing`] draws them behind `I`, and
//! [`crate::app`]'s summary counts them. A rig that could not be converted is
//! exactly as much a feature the file asked for and did not get as a texture
//! that could not be resampled.
//!
//! # A skin whose joints are not in parent-before-child order
//!
//! [`Skeleton::new`] refuses it, and this module reports the refusal and poses
//! nothing rather than repairing it. Repairing means renumbering the joints,
//! and the first section is why that cannot be done here: the mesh's `JOINTS_0`
//! attribute and the bind matrices already agree on the old numbers, and
//! neither is something this application can rewrite. A viewer that sorted the
//! array would draw a skeleton matching no vertex in the document — a wrong
//! picture with nothing on screen to say so — where the [`Skip`] is a sentence
//! naming the file, the skin and the joint that broke the order.

use crcbl::anim::{
    Channel, Clip, Interpolation, Joint, Palette, Pose, Skeleton, SkeletonError, Track, Trs,
};
use crcbl::math::{Mat4, Quat, Vec2, Vec3};
use crcbl::render::Camera;
use crcbl::scene::gltf_render::Skip;
use crcbl::scene::{GltfClip, GltfInterpolation, GltfNode, GltfSamples, GltfScene, GltfSkin};
use crcbl::ui::draw_list::DrawList;

/// How long a joint's axis ticks are drawn, as a fraction of the document's
/// largest half extent.
///
/// The overlay has to be legible on a chair and on a cathedral, so nothing here
/// is a length in metres. A twelfth is short enough that two neighbouring
/// joints' tripods do not run into each other on a rig sized for the model, and
/// long enough to read the twist of a bone at the distance
/// [`OrbitCamera::frame`](crcbl::render::OrbitCamera::frame) leaves the camera
/// at.
const JOINT_AXIS_FRACTION: f32 = 1.0 / 12.0;

/// How thick a bone is drawn, in pixels.
const BONE_THICKNESS: f32 = 2.0;

/// How thick a joint's axis ticks are drawn, in pixels — thinner than a bone,
/// so the skeleton reads as the chain first and the tripods as annotation on
/// it.
const AXIS_THICKNESS: f32 = 1.0;

/// The colour a bone is drawn in.
///
/// White, because it is the one part of the overlay that is not a coloured
/// axis, and because the overlay is drawn over whatever the document's own
/// materials are.
const BONE_COLOUR: [f32; 4] = [0.95, 0.95, 0.95, 1.0];

/// One colour per joint axis, in `X`, `Y`, `Z` order.
///
/// Red, green, blue in that order, which is the convention every DCC tool's and
/// every engine's joint display uses; a fourth palette invented here would be
/// one more thing for a reader to learn.
const AXIS_COLOURS: [[f32; 4]; 3] = [
    [0.95, 0.35, 0.35, 1.0],
    [0.4, 0.9, 0.45, 1.0],
    [0.4, 0.6, 1.0, 1.0],
];

/// Where a joint's own axes point, in its local space, before
/// [`JOINT_AXIS_FRACTION`] scales them.
const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];

/// The points [`Player::deviation`] measures a joint's motion at: its origin,
/// and one metre out along each of its own axes.
///
/// **The three off-origin probes are the whole point.** A rotation moves no
/// point at its own centre, so a measure taken at joint origins alone reports
/// zero for a leaf joint that is spinning — which is exactly what the browser
/// demo's document does, and would be a readout saying "nothing is happening"
/// over a clip that is playing.
const PROBES: [Vec3; 4] = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z];

/// A document's first skin and first clip, converted into what `crcbl::anim`
/// poses.
///
/// The static half of playback: a fact about the document, built once by
/// [`playable_of`] and carried on the [`Model`](crate::model::Model), which is
/// shared and never mutated. [`Player`] is the half that moves.
#[derive(Clone, Debug)]
pub struct Playable {
    skeleton: Skeleton,
    clip: Clip,
}

impl Playable {
    /// The skeleton, in the skin's own joint order.
    #[must_use]
    pub const fn skeleton(&self) -> &Skeleton {
        &self.skeleton
    }

    /// The clip, with every channel mapped onto that order.
    ///
    /// A clip with no channels at all is what a document declaring a skin and
    /// no animation converts to — see [`playable_of`].
    #[must_use]
    pub const fn clip(&self) -> &Clip {
        &self.clip
    }
}

/// Converts `scene`'s **first** skin and **first** clip, and reports what could
/// not be brought in.
///
/// `None` for a document with no skin: there is no skeleton to pose, and a clip
/// on its own drives nodes this application draws from their own transforms
/// anyway. A document with a skin and no animation converts to a skeleton and
/// an empty clip, which poses the rest pose for ever — a still skeleton is
/// worth drawing, and it is what the overlay then shows.
///
/// # The first of each, and the rest reported rather than silently dropped
///
/// A viewer has no UI to choose a skin or a clip with, and picking one for the
/// visitor is a choice they did not make — so the further ones are named on the
/// skip list, where everything else this application could not honour is named.
/// The alternative, playing all of them, is not "more complete": several skins
/// means several skeletons drawn at once and a clip per skin to choose, which
/// is a rig browser rather than a viewer.
#[must_use]
pub fn playable_of(scene: &GltfScene) -> (Option<Playable>, Vec<Skip>) {
    let mut skips = Vec::new();
    let Some(skin) = scene.skins().first() else {
        return (None, skips);
    };
    let named = match skin.name() {
        Some(name) => format!("skin 0 \"{name}\""),
        None => "skin 0".to_string(),
    };
    if scene.skins().len() > 1 {
        skips.push(Skip {
            feature: "skin",
            at: "the document".to_string(),
            why: format!(
                "{} skins are declared and this viewer poses the first; the joint count on \
                 the listing is still every skin's",
                scene.skins().len(),
            ),
        });
    }
    let skeleton = match skeleton_of(skin, scene.nodes()) {
        Ok(skeleton) => skeleton,
        Err(why) => {
            skips.push(Skip {
                feature: "joints",
                at: named,
                why: format!(
                    "{why}, so this skeleton cannot be posed and is not drawn — the joints \
                     have to be listed parents first, and renumbering them here would leave \
                     the mesh's JOINTS_0 attribute and the skin's inverseBindMatrices naming \
                     different bones",
                ),
            });
            return (None, skips);
        }
    };
    let clip = match scene.clips().first() {
        Some(source) => {
            if scene.clips().len() > 1 {
                skips.push(Skip {
                    feature: "animation",
                    at: "the document".to_string(),
                    why: format!(
                        "{} animations are declared and this viewer plays the first; the \
                         listing names them all",
                        scene.clips().len(),
                    ),
                });
            }
            clip_of(source, skin.joints(), &mut skips)
        }
        // Not an absence worth reporting: a rigged document whose animations
        // live in a second file is an ordinary export, not a loss.
        None => Clip::new(Vec::new()),
    };
    (Some(Playable { skeleton, clip }), skips)
}

/// Builds the skeleton a skin describes, in the skin's own joint order.
///
/// The hierarchy is not in the skin — a joint is a node like any other, so
/// where it sits is [`GltfNode::local_transform`] and what hangs off it is
/// [`GltfNode::children`] — which is why this takes the node array as well.
///
/// A joint whose parent node is not itself a joint of *this* skin is a root
/// here, which is what [`Joint::parent`]'s docs describe: glTF's joints share a
/// common root node and that node need not be a joint.
///
/// # Errors
///
/// Whatever [`Skeleton::new`] refused the joints with — see the [module
/// docs](self) for why the refusal is reported rather than repaired.
fn skeleton_of(skin: &GltfSkin, nodes: &[GltfNode]) -> Result<Skeleton, SkeletonError> {
    let joints = skin.joints();
    let parents = parents_of(nodes);
    let built = joints
        .iter()
        .enumerate()
        .map(|(index, &node)| Joint {
            parent: parents[node].and_then(|parent| joint_of(joints, parent)),
            // Exactly as long as `joints` by the importer's own contract; the
            // identity fallback is the specification's meaning for a skin that
            // declares no matrices at all, so it is also the only answer that
            // is not an invention if the two arrays ever disagreed.
            inverse_bind: skin
                .inverse_binds()
                .get(index)
                .copied()
                .unwrap_or(Mat4::IDENTITY),
            // The *rest* pose, which is the joint node's own transform with no
            // channel driving it — not the bind pose, which is the inverse
            // matrix above. A joint no channel touches keeps this, and so does
            // the translation of a joint only a rotation channel drives.
            rest: Trs::from_mat4(nodes[node].local_transform()),
        })
        .collect();
    Skeleton::new(built)
}

/// Which node is each node's parent, or `None` for a node nothing declares as a
/// child.
///
/// Built by walking the children lists, because that is the only direction glTF
/// stores the tree in. Every index is valid: `crcbl-scene`'s `gltf_check`
/// refuses a document whose node names a child that does not exist, before
/// [`import_gltf`](crcbl::scene::import_gltf) builds anything.
///
/// `pub(crate)` because [`crate::model`] wants the same walk: it composes a
/// node's world transform by climbing this, and a second copy of the loop is
/// where the two would come to disagree about what a root is.
pub(crate) fn parents_of(nodes: &[GltfNode]) -> Vec<Option<usize>> {
    let mut parents = vec![None; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        for &child in node.children() {
            parents[child] = Some(index);
        }
    }
    parents
}

/// Which palette index of this skin `node` is, or `None` for a node the skin
/// has not got.
///
/// A scan rather than a map: a skin's joint list is tens of entries, this is
/// asked once per joint and once per channel at load, and a `HashMap` built for
/// it would cost more to fill than the scans it saves.
fn joint_of(joints: &[usize], node: usize) -> Option<usize> {
    joints.iter().position(|&joint| joint == node)
}

/// Converts one animation onto a skin's joint order, appending a [`Skip`] for
/// every channel that could not come with it.
fn clip_of(source: &GltfClip, joints: &[usize], skips: &mut Vec<Skip>) -> Clip {
    let at = |index: usize| match source.name() {
        Some(name) => format!("animation 0 \"{name}\" channel {index}"),
        None => format!("animation 0 channel {index}"),
    };
    let mut channels = Vec::with_capacity(source.channels().len());
    for (index, channel) in source.channels().iter().enumerate() {
        // The ordinary case, and silent: see the module docs.
        let Some(joint) = joint_of(joints, channel.node()) else {
            continue;
        };
        let Some(track) = track_of(channel.samples()) else {
            skips.push(Skip {
                feature: "weights",
                at: at(index),
                why: "this application poses skeletons and draws no morph targets, so a \
                      channel driving target weights animates nothing"
                    .to_string(),
            });
            continue;
        };
        match Channel::new(
            joint,
            channel.times().to_vec(),
            interpolation_of(channel.interpolation()),
            track,
        ) {
            Ok(channel) => channels.push(channel),
            // `Channel::new` refuses only samplers that cannot be read at *any*
            // time — a keyframe count that does not match its values, times
            // that do not ascend. The rest of the clip still plays.
            Err(why) => skips.push(Skip {
                feature: "animation.sampler",
                at: at(index),
                why: format!("{why}, so this channel is not played"),
            }),
        }
    }
    Clip::new(channels)
}

/// The values a channel drives its joint with, or `None` for one this
/// application cannot pose a skeleton with.
///
/// `None` is glTF's fourth `target.path`, `weights`, which drives morph targets
/// rather than joints: `crcbl::anim::Track` has no variant for it, deliberately,
/// and neither does this engine have morph targets to drive.
///
/// **Rotations are normalized on the way in.** The specification requires a
/// rotation keyframe to be a unit quaternion, and one stored as normalized
/// bytes or shorts — which it permits — decodes to within about a thousandth of
/// unit rather than to it. That much error scales a joint slightly, and it is
/// the slerp in [`crcbl::anim::sample`] that would carry it. A quaternion with
/// no length at all is not a rotation and cannot be normalized into one, so it
/// arrives as the identity, which is the only value that leaves the joint posed
/// rather than filling the palette with `NaN`.
fn track_of(samples: &GltfSamples) -> Option<Track> {
    Some(match samples {
        GltfSamples::Translations(values) => {
            Track::Translation(values.iter().copied().map(Vec3::from_array).collect())
        }
        GltfSamples::Rotations(values) => {
            Track::Rotation(values.iter().copied().map(unit_quaternion).collect())
        }
        GltfSamples::Scales(values) => {
            Track::Scale(values.iter().copied().map(Vec3::from_array).collect())
        }
        GltfSamples::MorphWeights(_) => return None,
    })
}

/// One `xyzw` rotation keyframe as a unit quaternion — see [`track_of`].
fn unit_quaternion(value: [f32; 4]) -> Quat {
    let quat = Quat::from_array(value);
    let length = quat.length_squared();
    if length.is_finite() && length > 0.0 {
        quat.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// The same interpolation mode, in the animation crate's spelling.
///
/// Two enums for one concept because neither crate depends on the other, which
/// is the whole shape this module exists to bridge.
const fn interpolation_of(mode: GltfInterpolation) -> Interpolation {
    match mode {
        GltfInterpolation::Linear => Interpolation::Linear,
        GltfInterpolation::Step => Interpolation::Step,
        GltfInterpolation::CubicSpline => Interpolation::CubicSpline,
    }
}

/// A [`Playable`] with a playhead: what the frame samples, composes and draws.
///
/// The moving half of playback, so it lives on the
/// [`Viewer`](crate::app::Viewer) rather than on the shared
/// [`Model`](crate::model::Model). The pose and the palette are built once here
/// and refilled in place, which is what makes [`advance`](Self::advance)
/// allocation-free — `crcbl-anim` is written for that and this is the caller
/// that has to keep the bargain.
#[derive(Clone, Debug)]
pub struct Player {
    skeleton: Skeleton,
    clip: Clip,
    pose: Pose,
    palette: Palette,
    /// Each joint's global transform in the **rest** pose, computed once.
    ///
    /// What [`deviation`](Self::deviation) measures against. Kept rather than
    /// recomputed because it cannot change: it is the document's own hierarchy
    /// with no clip applied.
    rest: Vec<Mat4>,
    time: f32,
    deviation: f32,
}

impl Player {
    /// A player parked at the clip's own zero.
    ///
    /// Zero rather than the rest pose: a clip whose first keyframe is not the
    /// rest transform starts *there*, and a first frame drawn at rest would be
    /// a jump nobody asked for.
    #[must_use]
    pub fn new(playable: &Playable) -> Self {
        let skeleton = playable.skeleton().clone();
        let pose = Pose::new(&skeleton);
        let mut palette = Palette::new(&skeleton);
        palette.compute(&skeleton, &pose);
        let rest = palette.globals().to_vec();
        let mut player = Self {
            skeleton,
            clip: playable.clip().clone(),
            pose,
            palette,
            rest,
            time: 0.0,
            deviation: 0.0,
        };
        player.sample();
        player
    }

    /// Carries the playhead forward by `dt` seconds and poses the skeleton at
    /// where it lands.
    ///
    /// **The loop is this modulo and nothing else.**
    /// [`Clip::sample_into`](crcbl::anim::Clip::sample_into) clamps a time
    /// outside the clip to the nearest keyframe and says why: looping is the
    /// player's decision, because a clip that must *not* loop — a jump, a death
    /// — has to hold its last pose, and a sampler that wrapped internally would
    /// leave no way to say so. A viewer shows a clip over and over, so it
    /// wraps.
    ///
    /// A clip with no channels has no duration to wrap against and holds the
    /// rest pose; the guard is what keeps that a still skeleton rather than a
    /// division by zero.
    pub fn advance(&mut self, dt: f32) {
        // A `Duration` cannot produce a non-finite second count, so this is not
        // a case that arises today; it is what keeps a `NaN` out of the pose,
        // out of the heartbeat's number and out of the draw list, where it
        // would silently break a stroke instead of failing.
        if !dt.is_finite() {
            return;
        }
        let duration = self.clip.duration();
        self.time += dt;
        if duration > 0.0 {
            self.time %= duration;
        } else {
            self.time = 0.0;
        }
        self.sample();
    }

    /// Samples the clip at the playhead and composes the palette from it.
    fn sample(&mut self) {
        self.clip
            .sample_into(self.time, &self.skeleton, &mut self.pose);
        self.palette.compute(&self.skeleton, &self.pose);
        self.deviation = deviation(self.palette.globals(), &self.rest);
    }

    /// Where the playhead is, in seconds from the clip's start.
    #[must_use]
    pub const fn time(&self) -> f32 {
        self.time
    }

    /// How far the pose has carried the skeleton from its rest pose, in metres.
    ///
    /// **The number the `[HUD]` line reports and the browser gate reads**, and
    /// it is a property of the *pose* rather than of the clock: it is zero
    /// while the skeleton stands at rest and it moves when — and only when — a
    /// joint does. A playhead would advance just as happily over a pose nothing
    /// ever wrote, which is the failure the gate exists to catch.
    ///
    /// The largest distance, over every joint, that the joint's posed frame
    /// carries one of the module's `PROBES` away from where its rest frame has
    /// it. Metres
    /// because the probes are a metre out, and a maximum rather than a sum so
    /// that the figure is the same for a rig whether or not it has a hundred
    /// joints standing still beside the one that moves.
    #[must_use]
    pub const fn deviation(&self) -> f32 {
        self.deviation
    }

    /// **The joint palette this frame composed** — each joint's global
    /// transform times its inverse bind matrix, in the skin's own joint order.
    ///
    /// What [`SkinRange::palette`](crcbl::render::SkinRange::palette) takes, and
    /// the same array [`deviation`](Self::deviation) and [`draw`](Self::draw)
    /// are derived from: [`advance`](Self::advance) composes it once a frame and
    /// every consumer reads that one composition, so the geometry the GPU
    /// deforms and the skeleton drawn over it cannot be a frame apart.
    ///
    /// **It does not place the character in the world.** These matrices are in
    /// the space the skeleton's root joints hang in — see
    /// [`crate::model::SkinnedInstance::transform`], which is what carries that
    /// space into the world.
    #[must_use]
    pub fn palette(&self) -> &[Mat4] {
        self.palette.matrices()
    }

    /// The skeleton being posed.
    #[must_use]
    pub const fn skeleton(&self) -> &Skeleton {
        &self.skeleton
    }

    /// The clip being played.
    #[must_use]
    pub const fn clip(&self) -> &Clip {
        &self.clip
    }

    /// **Draws the posed skeleton over the frame**: a bone from every joint to
    /// its parent, and each joint's own axes as three short ticks.
    ///
    /// `size` is the document's largest half extent, which is what the tick
    /// length is a fraction of — see the module's `JOINT_AXIS_FRACTION`. A
    /// document with
    /// no size at all draws bones and no ticks rather than three degenerate
    /// segments per joint.
    ///
    /// # Why the axis ticks, when the bones are what a skeleton is
    ///
    /// A bone is drawn between two joint *origins*, and a rotation moves no
    /// point at its own centre — so a leaf joint's swing moves no bone at all.
    /// The browser demo's own document is exactly that case: one clip, turning
    /// the joint at the end of a two-joint chain. Bones alone would draw a
    /// still picture of a clip that is playing, which is the thing this whole
    /// feature exists to disprove.
    ///
    /// # It is drawn in screen space, because [`DrawList`] is
    ///
    /// Every joint position goes through the same
    /// [`Camera::view_projection`] the frame itself was drawn with, so the
    /// overlay cannot drift from the model it annotates. A joint behind the eye
    /// projects to nothing and takes its bone and its ticks with it: there is
    /// no screen position to put them at, and the alternative — dividing by a
    /// negative `w` — draws a mirrored skeleton in front of the camera.
    pub fn draw(&self, list: &mut DrawList, camera: &Camera, extent: (u32, u32), size: f32) {
        if extent.0 == 0 || extent.1 == 0 {
            return;
        }
        let viewport = Vec2::new(extent.0 as f32, extent.1 as f32);
        let view_projection = camera.view_projection(viewport.x / viewport.y);
        let globals = self.palette.globals();
        let tick = size * JOINT_AXIS_FRACTION;
        for (index, joint) in self.skeleton.joints().iter().enumerate() {
            let global = globals[index];
            let Some(origin) = project(
                view_projection,
                viewport,
                global.transform_point3(Vec3::ZERO),
            ) else {
                continue;
            };
            if let Some(parent) = joint.parent
                && let Some(at) = project(
                    view_projection,
                    viewport,
                    globals[parent].transform_point3(Vec3::ZERO),
                )
            {
                list.line(at, origin, BONE_THICKNESS, BONE_COLOUR);
            }
            if tick <= 0.0 {
                continue;
            }
            for (axis, colour) in AXES.iter().zip(AXIS_COLOURS) {
                let tip = global.transform_point3(*axis * tick);
                if let Some(tip) = project(view_projection, viewport, tip) {
                    list.line(origin, tip, AXIS_THICKNESS, colour);
                }
            }
        }
    }
}

/// The largest distance any of [`PROBES`] is carried between the two sets of
/// joint transforms — see [`Player::deviation`].
///
/// The two slices are the same length by construction: both are a palette built
/// for the same skeleton. `zip` rather than an index so that they cannot be
/// read past each other if that ever stops being true.
fn deviation(posed: &[Mat4], rest: &[Mat4]) -> f32 {
    let mut worst = 0.0_f32;
    for (posed, rest) in posed.iter().zip(rest) {
        for probe in PROBES {
            // `f32::max` answers the other operand for a `NaN`, so a document
            // whose keyframes are not numbers reports the finite joints rather
            // than poisoning the whole figure.
            worst = worst.max(
                posed
                    .transform_point3(probe)
                    .distance(rest.transform_point3(probe)),
            );
        }
    }
    worst
}

/// Where a world-space point lands in the frame, in pixels, or `None` for one
/// that has no place in it.
///
/// The projection produces **Y-up** normalised device coordinates; a
/// framebuffer's rows run the other way, which is the flip on `y`.
///
/// `w > 0` and not `w != 0`: under a right-handed perspective projection `w` is
/// the distance in front of the eye, so a point *behind* it divides to a
/// position that looks perfectly reasonable and is the mirror image of where
/// the thing is.
fn project(view_projection: Mat4, viewport: Vec2, point: Vec3) -> Option<Vec2> {
    let clip = view_projection * point.extend(1.0);
    if !clip.w.is_finite() || clip.w <= f32::MIN_POSITIVE {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    let at = Vec2::new(
        (ndc.x + 1.0) * 0.5 * viewport.x,
        (1.0 - ndc.y) * 0.5 * viewport.y,
    );
    at.is_finite().then_some(at)
}

#[cfg(test)]
mod tests {
    use super::{Playable, Player, joint_of, playable_of, track_of, unit_quaternion};
    use crate::{demo_model, fixture};
    use crcbl::anim::Track;
    use crcbl::assets::MemorySource;
    use crcbl::math::{Quat, Vec2, Vec3};
    use crcbl::render::{Camera, Projection};
    use crcbl::scene::{GltfSamples, GltfScene};
    use crcbl::ui::draw_list::{DrawCommand, DrawList};
    use std::path::Path;

    /// The document's own bytes, imported the way [`crate::model`] imports one.
    fn imported(document: Vec<u8>) -> GltfScene {
        let key = Path::new("fixture.glb");
        let mut source = MemorySource::new();
        source.insert(key, document).expect("a legal asset key");
        crcbl::scene::import_gltf(&source, key).expect("the fixture imports")
    }

    /// The demo document's rig, converted, with nothing lost on the way.
    fn demo() -> Playable {
        let (playable, skips) = playable_of(&imported(demo_model::demo_glb()));
        assert_eq!(skips, [], "the demo document converts whole");
        playable.expect("the demo document is rigged")
    }

    /// **The skeleton is in the skin's own joint order, and the hierarchy comes
    /// out of the node tree.**
    ///
    /// The order is the load-bearing half: a skinned vertex's `JOINTS_0`
    /// indexes the skin's `joints` array, so a converter that sorted or
    /// renumbered would leave every weight in the document naming a different
    /// bone. The bind matrices are asserted against the skin's own, in the same
    /// order, because that is the observable that says so — two joints in the
    /// wrong order still count two.
    #[test]
    fn the_skin_becomes_a_skeleton_in_the_skins_own_joint_order() {
        let scene = imported(demo_model::demo_glb());
        let skin = &scene.skins()[0];
        let playable = demo();
        let joints = playable.skeleton().joints();

        assert_eq!(joints.len(), skin.joints().len());
        for (index, joint) in joints.iter().enumerate() {
            assert_eq!(
                joint.inverse_bind,
                skin.inverse_binds()[index],
                "joint {index} carries another joint's bind matrix, so the palette was \
                 reordered",
            );
        }
        assert_eq!(joints[0].parent, None, "the lower joint is the root");
        assert_eq!(
            joints[1].parent,
            Some(0),
            "the upper joint hangs off it, which is in the node tree and not in the skin",
        );
        // The rest pose is the joint node's own transform: the upper joint's
        // node translates it above its parent, and a converter that left every
        // rest at the identity would collapse the chain onto its root.
        assert!(
            joints[1].rest.translation.length() > 0.0,
            "the upper joint's rest transform is the identity: {:?}",
            joints[1].rest,
        );
    }

    /// **A channel's target is a node index, and it arrives naming a joint.**
    ///
    /// The demo document's one channel drives node 4, which is joint 1 — two
    /// different numbers, deliberately, because a converter that passed the
    /// node index straight through builds a clip naming a joint the skeleton
    /// has not got. `Clip::sample_into` skips exactly that channel in silence,
    /// so the failure it produces is not an error anywhere: it is a skeleton
    /// that never moves.
    #[test]
    fn a_channels_node_is_mapped_to_the_joint_it_drives() {
        let scene = imported(demo_model::demo_glb());
        let source = &scene.clips()[0];
        let node = source.channels()[0].node();
        let playable = demo();
        let [channel] = playable.clip().channels() else {
            panic!(
                "the demo document has one channel, and this has {}",
                playable.clip().channels().len(),
            );
        };

        assert_ne!(
            node,
            channel.joint(),
            "the document's node index and the palette index are the same number here, so \
             this test cannot tell a mapping from a passthrough",
        );
        assert_eq!(
            channel.joint(),
            joint_of(scene.skins()[0].joints(), node).expect("the channel drives a joint"),
        );
    }

    /// The node-to-joint map itself, including the answer that makes a channel
    /// get dropped.
    #[test]
    fn a_node_that_is_not_a_joint_of_this_skin_maps_to_nothing() {
        let joints = [3, 4];
        assert_eq!(joint_of(&joints, 3), Some(0));
        assert_eq!(joint_of(&joints, 4), Some(1));
        assert_eq!(joint_of(&joints, 0), None, "the mesh node is not a joint");
        assert_eq!(joint_of(&[], 0), None);
    }

    /// **A channel driving a node the skin has not got is dropped, and it is
    /// not a loss to report.**
    ///
    /// The fixture animates the mesh node as well as the joint, which is what a
    /// document out of any tool looks like: one clip drives the whole scene.
    /// Both channels are in the file and one of them is for this skeleton.
    #[test]
    fn a_channel_on_a_node_outside_the_skin_is_dropped_without_a_skip() {
        let scene = imported(fixture::skinned_glb(true));
        assert_eq!(
            scene.clips()[0].channels().len(),
            2,
            "the fixture is meant to animate the mesh node as well as the joint",
        );

        let (playable, skips) = playable_of(&scene);
        let playable = playable.expect("the fixture is rigged");
        assert_eq!(
            playable.clip().channels().len(),
            1,
            "the mesh node's channel names no joint of this skin and drives nothing",
        );
        assert_eq!(
            skips,
            [],
            "a clip that drives the rest of the scene is an ordinary document, not a loss",
        );
    }

    /// **A skin whose joints are not parents-first is refused, and the refusal
    /// is a sentence a person can act on.**
    ///
    /// `Skeleton::new` is what refuses it — the single forward pass in
    /// `Palette::compute` is why — and this application's answer is to draw the
    /// document without the overlay and put the reason where every other thing
    /// it could not honour goes. See the module docs for why sorting the array
    /// instead would be a wrong picture with nothing on screen to say so.
    #[test]
    fn a_skin_out_of_hierarchy_order_is_refused_and_reported() {
        let (playable, skips) = playable_of(&imported(fixture::skinned_glb(false)));

        assert!(
            playable.is_none(),
            "a skeleton was built from joints Skeleton::new refuses",
        );
        let [skip] = skips.as_slice() else {
            panic!("the refusal has to reach the skip list: {skips:?}");
        };
        assert_eq!(skip.feature, "joints");
        assert!(skip.at.contains("skin 0"), "{skip}");
        assert!(
            skip.why.contains("palette order") && skip.why.contains("JOINTS_0"),
            "the message has to say what is wrong and why it is not repaired here: {skip}",
        );
    }

    /// **A document with a clip and no skin converts to nothing at all**, and
    /// that is the case every unrigged file in the world takes: there is no
    /// skeleton for the channels to pose.
    #[test]
    fn a_clip_with_no_skin_is_not_playable() {
        let (playable, skips) = playable_of(&imported(fixture::unnamed_clip_glb()));
        assert!(playable.is_none());
        assert_eq!(skips, []);

        let (playable, skips) = playable_of(&imported(fixture::quad_glb(Vec3::ZERO)));
        assert!(playable.is_none(), "and neither is a document with no rig");
        assert_eq!(skips, []);
    }

    /// Morph-target weights are the one `target.path` that poses no joint, so
    /// they convert to no track — and the caller turns that into a skip rather
    /// than a channel that would drive a bone with a weight.
    #[test]
    fn morph_weights_are_not_a_joint_track() {
        assert!(track_of(&GltfSamples::MorphWeights(vec![0.0, 1.0])).is_none());
        assert!(matches!(
            track_of(&GltfSamples::Translations(vec![[1.0, 2.0, 3.0]])),
            Some(Track::Translation(_)),
        ));
        assert!(matches!(
            track_of(&GltfSamples::Scales(vec![[1.0, 1.0, 1.0]])),
            Some(Track::Scale(_)),
        ));
    }

    /// A rotation keyframe arrives as a unit quaternion whatever the file
    /// stored — see [`track_of`] for why that is the specification's
    /// requirement rather than tidiness.
    #[test]
    fn a_rotation_keyframe_arrives_normalized() {
        let squashed = unit_quaternion([0.0, 0.0, 0.6, 0.6]);
        assert!(
            squashed.is_normalized(),
            "{squashed:?} is not a unit quaternion",
        );
        // The **dot product**, not `angle_between`. Two quaternions this close
        // have a dot of one to within an ulp, and `acos` at one is where a
        // single ulp becomes a milliradian — so an angle threshold tight enough
        // to mean anything measures the runner's `acos` rather than this
        // conversion, and `a_rotation_keyframe_arrives_normalized` failed on
        // macOS alone for exactly that. `abs` because `q` and `-q` are the same
        // rotation.
        let turn = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        assert!(
            squashed.dot(turn).abs() > 1.0 - 1e-6,
            "normalizing turned it into a different rotation: {squashed:?}",
        );
        assert_eq!(
            unit_quaternion([0.0, 0.0, 0.0, 0.0]),
            Quat::IDENTITY,
            "a quaternion with no length is not a rotation, and NaN is not an answer",
        );
    }

    /// **Playing the clip moves the pose away from rest, and the loop is a
    /// modulo.**
    ///
    /// Every claim the `[HUD]` number makes, in order: it is zero at the clip's
    /// own zero, it moves as the playhead does, it keeps moving across the
    /// swing, and a playhead carried past the clip's duration comes back to the
    /// start rather than holding the last keyframe — which is what
    /// `Clip::sample_into` would do on its own, and is why the modulo is the
    /// caller's.
    #[test]
    fn the_pose_moves_while_the_clip_plays_and_the_playhead_wraps() {
        let playable = demo();
        let duration = playable.clip().duration();
        assert!(duration > 0.0, "the demo document's clip has keyframes");

        let mut player = Player::new(&playable);
        assert!(
            player.deviation() < 1e-6,
            "the clip starts at the rest pose, so the deviation starts at zero: {}",
            player.deviation(),
        );

        player.advance(duration * 0.25);
        let quarter = player.deviation();
        assert!(quarter > 1e-3, "the pose never left rest: {quarter}");

        player.advance(duration * 0.5);
        let three_quarters = player.deviation();
        assert!(
            three_quarters > quarter,
            "the swing stopped part way: {quarter} then {three_quarters}",
        );

        // Onto the clip's end, which is its start again.
        player.advance(duration * 0.25);
        assert!(
            player.time() < 1e-6,
            "the playhead held the end instead of wrapping: {}",
            player.time(),
        );
        assert!(
            player.deviation() < 1e-6,
            "and the pose held with it: {}",
            player.deviation(),
        );
    }

    /// A camera that has the demo document's crate in front of it.
    fn looking_at_the_crate() -> Camera {
        Camera {
            eye: Vec3::new(0.0, 1.5, 4.0),
            target: Vec3::new(0.0, 0.0, 0.0),
            up: Vec3::Y,
            projection: Projection::default(),
        }
    }

    /// Every line the overlay drew, as its two ends.
    fn lines(list: &DrawList) -> Vec<(Vec2, Vec2)> {
        list.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Line { from, to, .. } => Some((*from, *to)),
                _ => None,
            })
            .collect()
    }

    /// **The overlay draws a bone per parented joint and a tripod per joint,
    /// and what it draws moves while the clip plays.**
    ///
    /// The second half is the one that would be missing without the tripods,
    /// and it is not a detail: this document turns the joint at the *end* of
    /// its chain, so every bone in it is between two points that never move.
    /// See [`Player::draw`].
    #[test]
    fn the_overlay_draws_the_skeleton_and_it_moves_with_the_clip() {
        let playable = demo();
        let mut player = Player::new(&playable);
        let camera = looking_at_the_crate();
        let extent = (1280, 720);

        let mut list = DrawList::new();
        player.draw(&mut list, &camera, extent, 2.5);
        let first = lines(&list);
        let bones = playable
            .skeleton()
            .joints()
            .iter()
            .filter(|joint| joint.parent.is_some())
            .count();
        assert_eq!(
            first.len(),
            bones + playable.skeleton().len() * super::AXES.len(),
            "a bone per parented joint and a tick per axis per joint: {first:?}",
        );

        player.advance(playable.clip().duration() * 0.5);
        let mut list = DrawList::new();
        player.draw(&mut list, &camera, extent, 2.5);
        let later = lines(&list);
        assert_eq!(later.len(), first.len(), "the overlay lost a segment");
        assert!(
            first
                .iter()
                .zip(&later)
                .any(|(before, after)| before != after),
            "the overlay drew the same picture over a clip that had moved: {first:?}",
        );
    }

    /// A document with no size draws its bones and no ticks, rather than three
    /// degenerate segments per joint that a stroke has no direction for.
    #[test]
    fn a_document_with_no_size_draws_bones_alone() {
        let playable = demo();
        let player = Player::new(&playable);
        let mut list = DrawList::new();
        player.draw(&mut list, &looking_at_the_crate(), (1280, 720), 0.0);
        assert_eq!(
            lines(&list).len(),
            playable
                .skeleton()
                .joints()
                .iter()
                .filter(|joint| joint.parent.is_some())
                .count(),
        );
    }

    /// A frame with no pixels in it — a minimised window — is drawn into
    /// without dividing by a zero aspect.
    #[test]
    fn a_zero_extent_draws_nothing() {
        let playable = demo();
        let player = Player::new(&playable);
        let mut list = DrawList::new();
        player.draw(&mut list, &looking_at_the_crate(), (0, 720), 2.5);
        assert!(list.is_empty());
    }
}
