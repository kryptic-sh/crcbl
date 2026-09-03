//! The reflective shadow map: the sun's near cascade drawn a second time as
//! what it reflects, where it is and which way it faces.
//!
//! ```text
//!  cascade 0's matrix + its already-generated draws
//!            │
//!            ▼
//!   mesh.slang: rsmFragmentMain ──▶ albedo  (Rgba16Float)
//!                                ├─▶ normal  (Rgba8Unorm, n*0.5+0.5)
//!                                ├─▶ world   (Rgba32Float, w = coverage)
//!                                └─▶ depth   (D32Float, discarded)
//!                                        │
//!                            crate::probe_gather ──▶ every probe row
//! ```
//!
//! `docs/plan/50-irradiance-probes.md`'s raster updater, first half. This module
//! owns the map's extent, the descriptions of its four attachments and the
//! arithmetic that says how much world one of its texels covers;
//! [`crate::forward`] records the pass, because the pipeline and the draws are
//! that module's.
//!
//! # It is its own render pass, not extra targets on `shadow`
//!
//! [`crate::forward`]'s `shadow` pass has one attachment, a depth, and every
//! pipeline it runs is fragment-free — `depthVertexMain` and the tile-reset
//! triangle alike. Colour attachments there would span the whole
//! [`shadow::atlas_extent`](crate::shadow::atlas_extent) for data only cascade
//! 0's one tile is read from, and they would give all fourteen of the atlas's
//! light tiles a fragment stage. So this is a pass of its own at its own extent,
//! reusing cascade 0's matrix — through the bind group whose uniform block
//! already holds it — and the draws that cascade's cull already generated.
//!
//! # Its extent is [`crcbl_shaders::probe_gather::RSM_SIDE`]
//!
//! Small, because every probe gathers every texel every frame — see that
//! constant, which is where the reason and the sweep live.

use crcbl_hal::{Format, ImageUsage};
use crcbl_shaders::probe_gather::RSM_SIDE;

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

/// The map's extent in texels, square.
#[must_use]
pub fn extent() -> (u32, u32) {
    (RSM_SIDE, RSM_SIDE)
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
pub fn albedo_target() -> TransientImageDesc {
    TransientImageDesc::new(
        extent(),
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
pub fn normal_target() -> TransientImageDesc {
    TransientImageDesc::new(
        extent(),
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
pub fn world_target() -> TransientImageDesc {
    TransientImageDesc::new(
        extent(),
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
pub fn depth_target() -> TransientImageDesc {
    TransientImageDesc::new(
        extent(),
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

    /// Every attachment is the same square, which the graph requires of one
    /// pass's attachments — `GraphError::AttachmentExtentMismatch` is what
    /// would otherwise say so at compile time, on the frame that recorded it.
    #[test]
    fn every_attachment_is_the_same_square() {
        let extent = extent();
        assert_eq!(extent.0, extent.1);
        for desc in [
            albedo_target(),
            normal_target(),
            world_target(),
            depth_target(),
        ] {
            assert_eq!(desc.extent, extent);
            assert_eq!(desc.samples, 1);
            assert_eq!(desc.mip_levels, 1);
        }
    }

    /// The three colour targets carry the formats `mesh.slang`'s `RsmOutput`
    /// says they do, and the depth is not sampled.
    ///
    /// A pipeline built for a format the attachment is not in is a validation
    /// error on WebGPU and a silent reinterpretation elsewhere, so the pair has
    /// to be stated somewhere — `MeshModules::RSM_TARGETS` is the other half.
    #[test]
    fn the_targets_carry_the_formats_the_shader_writes() {
        assert_eq!(albedo_target().format, Format::Rgba16Float);
        assert_eq!(normal_target().format, Format::Rgba8Unorm);
        assert_eq!(world_target().format, Format::Rgba32Float);
        assert_eq!(depth_target().format, Format::D32Float);
        assert!(
            !depth_target().usage.contains(ImageUsage::SAMPLED),
            "nothing reads the map's depth, so it must not claim to be sampled"
        );
    }
}
