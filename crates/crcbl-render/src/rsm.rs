//! The reflective shadow maps: the sun's near cascade and every shadowed
//! punctual light's faces, drawn a second time as what they reflect, where they
//! are and which way they face.
//!
//! ```text
//!  cascade 0's matrix + its already-generated draws ─┐
//!  each light face's matrix + its slot's draws ──────┤
//!            │                                       │
//!            ▼                                       ▼
//!   mesh.slang: rsmFragmentMain ──▶ albedo  (Rgba16Float)
//!                                ├─▶ normal  (Rgba8Unorm, n*0.5+0.5)
//!                                ├─▶ world   (Rgba32Float, w = coverage)
//!                                └─▶ depth   (D32Float, discarded)
//!                                        │
//!                            crate::probe_gather ──▶ every probe row
//! ```
//!
//! `docs/plan/50-irradiance-probes.md`'s raster updater, first half. This module
//! owns both maps' extents, the descriptions of their attachments and the
//! arithmetic that says how much world a sun texel covers; [`crate::forward`]
//! records the passes, because the pipeline and the draws are that module's.
//!
//! # Two maps, because the two halves are priced differently
//!
//! One frame draws one cascade and up to
//! [`shadow::LIGHT_TILES`](crate::shadow::LIGHT_TILES) punctual faces, and the
//! gather walks every texel of every one of them for every probe. So the
//! punctual half's cost is quadratic in a side that is multiplied by the faces a
//! frame lights, where the sun's is quadratic in a side paid once — and the two
//! sides are [`crcbl_shaders::probe_gather::PUNCTUAL_RSM_SIDE`] and
//! [`crcbl_shaders::probe_gather::RSM_SIDE`], which are separate numbers for
//! exactly that reason.
//!
//! The punctual map is an **atlas** laid out on
//! [`crate::shadow`]'s own grid: tile `t` of the light region is at
//! [`crate::rsm::punctual_tile`]'s answer, which is
//! [`shadow::tile_origin`](crate::shadow::tile_origin) scaled to this map's
//! side. A tile index therefore maps the same way in both images, and a face
//! drawn into the shadow atlas's tile `n` is the face this map holds at its
//! tile `n`.
//!
//! # They are their own render passes, not extra targets on `shadow`
//!
//! [`crate::forward`]'s `shadow` pass has one attachment, a depth, and every
//! pipeline it runs is fragment-free — `depthVertexMain` and the tile-reset
//! triangle alike. Colour attachments there would span the whole
//! [`shadow::atlas_extent`](crate::shadow::atlas_extent) at the tile side that
//! atlas is drawn at, which is orders of magnitude more texels than a gather
//! that walks every one of them for every probe can afford. So these are passes
//! of their own at their own extents, reusing the very matrices — through the
//! bind groups whose uniform blocks already hold them — and the very draws the
//! atlas's own culls already generated.
//!
//! # Both extents are small, and each for its own reason
//!
//! Every probe gathers every texel of every producer every frame, so the whole
//! updater's cost is a texel count. [`crcbl_shaders::probe_gather::RSM_SIDE`]
//! and [`crcbl_shaders::probe_gather::PUNCTUAL_RSM_SIDE`] are where the reasons
//! and the sweeps live.

use crcbl_hal::{Format, ImageUsage};
use crcbl_shaders::probe_gather::{PUNCTUAL_RSM_SIDE, RSM_SIDE};

use crate::shadow;
use crate::transient::TransientImageDesc;

crcbl_console::convar! {
    /// Draw the reflective shadow map and gather it into the probes: on ships.
    ///
    /// **The on/off pair for the measurement, not the feature's switch.** What
    /// decides whether a volume is updated at all is
    /// [`ProbeUpdate`](crate::scene::ProbeUpdate) on the volume itself, and that
    /// module's docs say why a console variable cannot be it. This is what a
    /// pricing run turns off so the two passes' cost can be read off the frame
    /// with everything else held still — `docs/plan/50-irradiance-probes.md`
    /// carries the numbers.
    pub static r_probe_bounce: bool = true;
}

/// Whether the updater's two passes are recorded at all.
///
/// Off leaves the rows as the last gather wrote them, which for a scene that has
/// never gathered is the authored rows the table was filled with — so switching
/// it off at the start of a run is the control the measurement wants.
pub(crate) fn enabled() -> bool {
    r_probe_bounce.get_bool()
}

/// The sun's map's extent in texels, square.
#[must_use]
pub fn extent() -> (u32, u32) {
    (RSM_SIDE, RSM_SIDE)
}

/// The punctual map's extent in texels: [`crate::shadow`]'s tile grid at
/// [`PUNCTUAL_RSM_SIDE`] a tile.
///
/// The whole grid rather than the light region alone, so a tile index means the
/// same thing in both images — see [`punctual_tile`], which is why the
/// [`crate::shadow::CASCADES`] tiles the sun would occupy are
/// cleared here and drawn into by nothing.
#[must_use]
pub fn punctual_extent() -> (u32, u32) {
    (
        PUNCTUAL_RSM_SIDE * shadow::ATLAS_COLUMNS,
        PUNCTUAL_RSM_SIDE * shadow::ATLAS_ROWS,
    )
}

/// Where light tile `tile` of the punctual map is, in texels.
///
/// [`shadow::tile_origin`] scaled from [`shadow::TILE`] to
/// [`PUNCTUAL_RSM_SIDE`], through [`shadow::light_tile`] — so the one place the
/// atlas's "cascades first, then the lights" split is written stays that
/// function, and a face drawn into the shadow atlas's tile `n` is the face this
/// map holds at its tile `n`.
///
/// `tile` is the light region's own index, which is what
/// [`Assignment::base`](crate::shadow::Assignment) counts in.
#[must_use]
pub fn punctual_tile(tile: usize) -> shadow::TileRect {
    let (x, y) = shadow::tile_origin(shadow::light_tile(tile));
    shadow::TileRect {
        x: x / shadow::TILE * PUNCTUAL_RSM_SIDE,
        y: y / shadow::TILE * PUNCTUAL_RSM_SIDE,
        side: PUNCTUAL_RSM_SIDE,
    }
}

/// How much world area one texel of the map covers, in square world units.
///
/// `reach` is cascade 0's own reach — the radius of the sphere
/// [`Cascades::new`](crate::shadow::Cascades::new) fits it to, which is
/// `Cascades::far[0]`. The cascade's orthographic box is exactly `2 · reach`
/// across on both axes (`cascade_matrix` builds it from `centre ± radius`), so
/// one texel of an `RSM_SIDE`-wide map covers `(2 · reach / RSM_SIDE)²`.
///
/// **This is the number the whole bounce is scaled by.** It is a sample's solid
/// angle at a probe that it multiplies — see
/// [`GatherParams::texel_area`](crcbl_shaders::probe_gather::GatherParams::texel_area)
/// — so getting it wrong scales every probe's rows by the same factor and leaves
/// a room that is merely brighter or dimmer than it should be.
#[must_use]
pub fn texel_area(reach: f32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "the map is a few dozen texels a side"
    )]
    let side = 2.0 * reach / RSM_SIDE as f32;
    side * side
}

/// `RsmOutput::albedo`'s attachment: the diffuse albedo, rendered into and then
/// read by the gather.
///
/// `Rgba16Float` and not narrower, because an albedo is
/// `input.color * material.base_color * texel` and the first two of those are
/// unclamped floats — `RsmOutput` in `mesh.slang` carries the argument.
#[must_use]
pub fn albedo_target(extent: (u32, u32)) -> TransientImageDesc {
    TransientImageDesc::new(
        extent,
        Format::Rgba16Float,
        ImageUsage::COLOR_ATTACHMENT.union(ImageUsage::SAMPLED),
    )
}

/// `RsmOutput::normal`'s attachment: the world normal encoded `n * 0.5 + 0.5`.
///
/// `Rgba8Unorm`, which is what an eight-bit target can carry a signed direction
/// in and is the encoding the normals debug view already uses. The gather
/// renormalises what it reads, because a quantised direction is not a unit
/// vector.
#[must_use]
pub fn normal_target(extent: (u32, u32)) -> TransientImageDesc {
    TransientImageDesc::new(
        extent,
        Format::Rgba8Unorm,
        ImageUsage::COLOR_ATTACHMENT.union(ImageUsage::SAMPLED),
    )
}

/// `RsmOutput::world`'s attachment: the world position, with a coverage flag in
/// `w`.
///
/// **`Rgba32Float`, and it is a position rather than a reconstruction.** The
/// seam has no plain-float read of a depth image —
/// [`SampleType::Depth`](crcbl_hal::SampleType::Depth) is a comparison-sampler
/// slot — so the gather cannot recover a position from this pass's depth
/// attachment. `probe_capture.slang` set the precedent and its header argues it.
#[must_use]
pub fn world_target(extent: (u32, u32)) -> TransientImageDesc {
    TransientImageDesc::new(
        extent,
        Format::Rgba32Float,
        ImageUsage::COLOR_ATTACHMENT.union(ImageUsage::SAMPLED),
    )
}

/// The map's own depth attachment, which resolves which surface a texel keeps
/// and is read by nothing.
///
/// Not [`TransientImageDesc::scene_depth`], which carries `SAMPLED` for the
/// occlusion pass: a usage flag is a claim about what an image may be bound as,
/// and nothing binds this one.
#[must_use]
pub fn depth_target(extent: (u32, u32)) -> TransientImageDesc {
    TransientImageDesc::new(
        extent,
        Format::D32Float,
        ImageUsage::DEPTH_STENCIL_ATTACHMENT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **One texel of the map covers the cascade's footprint divided by the
    /// map's side**, which is the one thing a caller cannot check by looking at
    /// a picture: a wrong area is a bounce that is uniformly too bright.
    ///
    /// Written out against the definition rather than against a remembered
    /// number, and the reach is chosen so the whole thing is exact in binary.
    #[test]
    fn a_texel_covers_the_cascade_divided_by_the_map_s_side() {
        #[expect(
            clippy::cast_precision_loss,
            reason = "the map is a few dozen texels a side"
        )]
        let side = RSM_SIDE as f32;
        // A reach of half the side puts exactly one world unit in a texel.
        assert_eq!(texel_area(0.5 * side), 1.0);
        // And the area is quadratic in the reach, not linear.
        assert_eq!(texel_area(side), 4.0);
    }

    /// Every attachment of one pass covers that pass's whole map, which the
    /// graph requires of one pass's attachments —
    /// `GraphError::AttachmentExtentMismatch` is what would otherwise say so at
    /// compile time, on the frame that recorded it.
    ///
    /// Both maps, because they are two passes and each has four of them.
    #[test]
    fn every_attachment_covers_its_own_map() {
        let sun = extent();
        assert_eq!(sun.0, sun.1, "the sun's map is square");
        for extent in [sun, punctual_extent()] {
            for desc in [
                albedo_target(extent),
                normal_target(extent),
                world_target(extent),
                depth_target(extent),
            ] {
                assert_eq!(desc.extent, extent);
                assert_eq!(desc.samples, 1);
                assert_eq!(desc.mip_levels, 1);
            }
        }
    }

    /// The three colour targets carry the formats `mesh.slang`'s `RsmOutput`
    /// says they do, and the depth is not sampled.
    ///
    /// A pipeline built for a format the attachment is not in is a validation
    /// error on WebGPU and a silent reinterpretation elsewhere, so the pair has
    /// to be stated somewhere — `MeshModules::RSM_TARGETS` is the other half.
    /// Asked of the punctual map's extent as well, because one pipeline draws
    /// both and a target whose format depended on its size would be a pipeline
    /// that fits one of them.
    #[test]
    fn the_targets_carry_the_formats_the_shader_writes() {
        for extent in [extent(), punctual_extent()] {
            assert_eq!(albedo_target(extent).format, Format::Rgba16Float);
            assert_eq!(normal_target(extent).format, Format::Rgba8Unorm);
            assert_eq!(world_target(extent).format, Format::Rgba32Float);
            assert_eq!(depth_target(extent).format, Format::D32Float);
            assert!(
                !depth_target(extent).usage.contains(ImageUsage::SAMPLED),
                "nothing reads a map's depth, so it must not claim to be sampled"
            );
        }
    }

    /// **Every light tile lands inside the punctual map and no two overlap**,
    /// which is the whole of what makes a tile index mean one thing in both
    /// images: the gather reads a producer's rectangle out of this map by the
    /// same index the pass set its viewport from.
    ///
    /// Walked over the light region rather than over the grid, because the
    /// cascades' cells are the part of this image nothing draws into.
    #[test]
    fn every_light_tile_is_its_own_rectangle_inside_the_punctual_map() {
        let (width, height) = punctual_extent();
        let mut seen: Vec<shadow::TileRect> = Vec::new();
        for tile in 0..shadow::LIGHT_TILES {
            let rect = punctual_tile(tile);
            assert_eq!(rect.side, PUNCTUAL_RSM_SIDE);
            assert!(
                rect.x + rect.side <= width && rect.y + rect.side <= height,
                "light tile {tile} at {rect:?} leaves a {width}x{height} map"
            );
            assert!(
                !seen.contains(&rect),
                "light tile {tile} lands on {rect:?}, which another tile already holds"
            );
            seen.push(rect);
        }
    }

    /// **A spot's map is a perspective whose half field of view is the light's
    /// own outer half-angle**, which is the statement `probe_gather.slang`'s
    /// `producer_tangent` derives a texel's solid angle from.
    ///
    /// Checked against [`shadow::spot_matrix`](crate::shadow::spot_matrix)
    /// rather than against a written-out number: a point on the cone's outer
    /// edge has to land exactly on the clip volume's side wall, and a projection
    /// built from any other angle puts it inside or outside. Nothing else in the
    /// tree would notice — a wrong half-angle scales every punctual texel's
    /// solid angle by the same factor, which is a bounce that is uniformly too
    /// bright.
    #[test]
    fn a_spot_s_map_is_a_perspective_of_its_own_outer_angle() {
        use crate::light::SpotLight;
        use glam::{Vec3, Vec4};

        let spot = SpotLight {
            position: Vec3::new(1.0, 2.0, -0.5),
            radius: 9.0,
            color: Vec3::ONE,
            direction: Vec3::new(0.2, -1.0, 0.3),
            inner_angle: 0.2,
            outer_angle: 0.55,
            fill: false,
        };
        let axis = spot.direction.normalize();
        // Any vector across the axis; the cone is round, so which one is
        // arbitrary and the claim holds for all of them.
        let across = axis.cross(Vec3::X).normalize();
        // A point at the cone's outer edge, one unit of axial depth along.
        let edge = spot.position + axis + across * spot.outer_angle.tan();
        let clip: Vec4 = shadow::spot_matrix(&spot) * edge.extend(1.0);
        let ndc = (clip.x / clip.w).hypot(clip.y / clip.w);
        assert!(
            (ndc - 1.0).abs() < 1e-4,
            "a point on the cone's outer edge lands {ndc:.5} of the way across the \
             clip volume — the map's half field of view is not the outer half-angle, \
             and `producer_tangent` derives the wrong solid angle from it"
        );
    }
}
