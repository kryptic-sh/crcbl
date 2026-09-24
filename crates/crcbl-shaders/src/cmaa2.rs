//! The constants and the uniform block the three `cmaa2_*.slang` sources read,
//! in the layouts those shaders declare.
//!
//! Same reason as [`crate::fxaa`]: the shaders fix numbers and a byte layout,
//! every producer of those has to agree with them exactly, and keeping the
//! mirror in the crate that owns the sources means there is one place to change
//! rather than one per consumer.
//!
//! # What the tier stores
//!
//! The antialiasing ladder's CMAA2 rung is three passes over two
//! buffers: one edge word per pixel, and a fixed-point accumulation of
//! [`ACCUM_WORDS`] per pixel. **Neither is a list and neither has a capacity**
//! — both are indexed by the pixel they belong to, so both are exactly as long
//! as the frame and nothing can arrive that does not fit.
//!
//! That is a property the tier is built for rather than a convenience. An
//! earlier shape appended edge pixels and blend items to buffers sized as a
//! fraction of the frame and dropped what arrived past the capacity, and which
//! entries won the room was the device's scheduling — so a frame with enough
//! edges to fill a list was not a function of its inputs.
//! `crates/crcbl/tests/mesh_e2e/cmaa2.rs`'s
//! `a_dense_edge_frame_resolves_to_the_same_bytes_every_time` is what holds the
//! shape that replaced it.

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in every
/// `cmaa2_*.slang` compute entry point.
pub const WORKGROUP_SIZE: u32 = 64;

/// Bytes of the uniform block: two `uint`s and the tail `std140` pads them to.
///
/// Eight bytes of value. `std140` rounds a uniform block's size up to a
/// multiple of 16, so the buffer is sixteen and [`Cmaa2Params::to_bytes`]
/// leaves the second half zero — a shader reads neither half of it.
pub const PARAMS_SIZE: usize = 16;

/// Words per pixel in the accumulation: three colour channels and the weight
/// they were premultiplied by.
pub const ACCUM_WORDS: u32 = 4;

/// Bytes in one word of any of this tier's buffers.
pub const WORD_BYTES: u64 = 4;

/// What a fixed-point weight of one is, matching
/// `BLEND_FIXED_POINT_SCALE` in `shaders/cmaa2_shapes.slang`.
///
/// Two to the twentieth, and the choice is bounded from both sides. **Below**:
/// one part in 1048576 is three orders of magnitude finer than the `1/255` step
/// of an eight-bit target, so no accumulated colour can be quantised into a
/// different written texel. **Above**: the sum a pixel can be given is bounded
/// by [`MAX_BLEND_WEIGHT`], [`MAX_LINE_LENGTH`] and [`WALK_DIRECTIONS`], and
/// this module's `the_fixed_point_scale_is_finer_than_the_target_and_wider_than_the_sum`
/// is where that bound is arithmetic against `u32::MAX` rather than a claim.
///
/// It is a power of two, which is what makes the conversion back exact — see
/// `INV_BLEND_FIXED_POINT_SCALE` in `shaders/cmaa2_apply.slang`.
pub const BLEND_FIXED_POINT_SCALE: u32 = 1 << 20;

/// The longest run `cmaa2_shapes.slang` will classify, in pixels.
///
/// A bound on a loop rather than a quality knob; that shader's constant carries
/// the argument for leaving a longer run unclassified rather than truncating
/// it.
pub const MAX_LINE_LENGTH: u32 = 64;

/// How many runs can reach one pixel's accumulation.
///
/// Four, and they are the four walks `cmaa2_shapes.slang`'s `blend_line` can be
/// entered from with this pixel in its reach: the horizontal run this pixel is
/// an element of, the horizontal run the pixel *below* it is an element of —
/// which hands its share across the boundary they share — and the same pair
/// down the two columns. A run is maximal, so a pixel is an element of at most
/// one in each axis, and it is the across-the-boundary partner of at most one
/// more in each.
pub const WALK_DIRECTIONS: u32 = 4;

/// The luma delta across a pixel boundary, in display space, below which
/// `cmaa2_edges.slang` marks no edge.
///
/// **This tree's number rather than Intel's.** It is the threshold the retired
/// SMAA tier used on the same fixture, kept because it is the one that has been
/// measured here; `crates/crcbl/tests/mesh_e2e/cmaa2.rs` is what holds it and
/// `docs/backlog.md` records that the reference's own value is unverified.
pub const EDGE_THRESHOLD: f32 = 0.1;

/// How much stronger a neighbouring boundary has to be before this one is not
/// marked.
///
/// The local-contrast adaptation both CMAA2 and SMAA carry, in the form this
/// tree measured — see [`EDGE_THRESHOLD`] on where the number comes from.
pub const LOCAL_CONTRAST_ADAPTATION_FACTOR: f32 = 2.0;

/// How much of the `U`-shape reconstruction `cmaa2_shapes.slang` applies.
///
/// This transcription's number; that shader's constant argues it.
pub const U_SHAPE_WEIGHT: f32 = 0.5;

/// The largest share one contribution may carry: half a pixel.
pub const MAX_BLEND_WEIGHT: f32 = 0.5;

/// The uniform block, matching `struct Cmaa2Params` in every `cmaa2_*.slang`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cmaa2Params {
    /// Width of the image being filtered, in texels. Every buffer this tier
    /// owns is indexed `y * viewport_x + x`.
    pub viewport_x: u32,
    /// Its height.
    pub viewport_y: u32,
}

impl Cmaa2Params {
    /// The block for a frame of `width` by `height`.
    ///
    /// The extent is floored at one texel: a zero extent would make the pixel
    /// count zero, and a dispatch of no groups is something Metal rejects
    /// outright rather than treating as a no-op.
    #[must_use]
    pub fn for_extent(width: u32, height: u32) -> Self {
        Self {
            viewport_x: width.max(1),
            viewport_y: height.max(1),
        }
    }

    /// The block as the bytes a uniform buffer holds, in `std140` order.
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        for (slot, value) in [self.viewport_x, self.viewport_y].into_iter().enumerate() {
            let at = slot * 4;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `cmaa2_*.slang`.
    const SOURCES: [(&str, &str); 3] = [
        (
            "cmaa2_edges.slang",
            include_str!("../shaders/cmaa2_edges.slang"),
        ),
        (
            "cmaa2_shapes.slang",
            include_str!("../shaders/cmaa2_shapes.slang"),
        ),
        (
            "cmaa2_apply.slang",
            include_str!("../shaders/cmaa2_apply.slang"),
        ),
    ];

    /// The block every source declares, member for member.
    ///
    /// Nothing else can catch a rename or a reorder: the shaders compile either
    /// way and the buffer is bound either way, and a block whose members moved
    /// would read a height as a width and dispatch over the wrong frame.
    /// Reading the sources is the check, and they are hash-pinned by the
    /// manifest, so they are the files the committed artifacts were built from.
    #[test]
    fn every_cmaa2_source_declares_the_block_to_bytes_writes() {
        for (name, source) in SOURCES {
            for member in ["uint viewport_x;", "uint viewport_y;"] {
                assert!(
                    source.contains(member),
                    "{name} does not declare `{member}`"
                );
            }
            assert!(
                source.contains("ConstantBuffer<Cmaa2Params> params D3D12_REGISTER("),
                "{name} does not bind the block `to_bytes` writes"
            );
        }
    }

    /// **The two compute sources bind four resources and stop**, which is the
    /// shape the tier's determinism rests on.
    ///
    /// The buffers this tier has left are both indexed by pixel, so neither can
    /// overflow and neither drops. A fifth binding would be a working buffer
    /// that is not one-per-pixel — the append lists that used to sit at
    /// bindings 4 and 6 are what this refuses — and nothing else here would
    /// notice, because a dropped entry is a pixel that merely keeps its own
    /// colour. It is also what makes one bind-group layout serve both files:
    /// `crcbl_render::cmaa2` builds exactly one, with these four entries.
    #[test]
    fn the_two_compute_sources_bind_the_same_four_resources_and_no_more() {
        for name in ["cmaa2_edges.slang", "cmaa2_shapes.slang"] {
            let source = SOURCES
                .into_iter()
                .find(|(source_name, _)| *source_name == name)
                .expect("a listed source")
                .1;
            for binding in 0..4 {
                assert!(
                    source.contains(&format!("[[vk::binding({binding}, 0)]]")),
                    "{name} does not declare binding {binding}"
                );
            }
            assert!(
                !source.contains("[[vk::binding(4, 0)]]"),
                "{name} declares a fifth binding, so this tier has a working buffer \
                 that is not one word per pixel"
            );
        }
    }

    /// The two compute entry points declare the workgroup size this crate
    /// sizes their dispatches with.
    ///
    /// A mismatch is this tier's quietest failure: a dispatch sized against the
    /// wrong number covers a *prefix* of the frame, so the picture is right at
    /// the top and unfiltered below a line nothing names.
    #[test]
    fn the_compute_sources_declare_the_workgroup_size_this_module_names() {
        let declaration = format!("[numthreads({WORKGROUP_SIZE}, 1, 1)]");
        for name in ["cmaa2_edges.slang", "cmaa2_shapes.slang"] {
            let source = SOURCES
                .into_iter()
                .find(|(source_name, _)| *source_name == name)
                .expect("a listed source")
                .1;
            assert!(
                source.contains(&declaration),
                "{name} does not declare `{declaration}`; WORKGROUP_SIZE has drifted from it"
            );
        }
    }

    /// Every constant a shader and this module both name is written the same in
    /// both.
    ///
    /// The shaders have no `#include`, so each of these is two copies of one
    /// number — and a drift in any of them is a filter that scales a weight
    /// nothing divides back or walks a run of a length the host did not price.
    #[test]
    fn the_shared_constants_are_spelled_the_same_in_the_sources() {
        let edges = SOURCES[0].1;
        let shapes = SOURCES[1].1;
        let apply = SOURCES[2].1;
        for (name, source, declaration) in [
            (
                "cmaa2_edges.slang",
                edges,
                format!("static const uint ACCUM_WORDS = {ACCUM_WORDS};"),
            ),
            (
                "cmaa2_edges.slang",
                edges,
                format!("static const float EDGE_THRESHOLD = {EDGE_THRESHOLD:?};"),
            ),
            (
                "cmaa2_edges.slang",
                edges,
                format!(
                    "static const float LOCAL_CONTRAST_ADAPTATION_FACTOR = \
                     {LOCAL_CONTRAST_ADAPTATION_FACTOR:?};"
                ),
            ),
            (
                "cmaa2_shapes.slang",
                shapes,
                format!("static const uint ACCUM_WORDS = {ACCUM_WORDS};"),
            ),
            (
                "cmaa2_shapes.slang",
                shapes,
                format!("static const uint MAX_LINE_LENGTH = {MAX_LINE_LENGTH};"),
            ),
            (
                "cmaa2_shapes.slang",
                shapes,
                format!(
                    "static const float BLEND_FIXED_POINT_SCALE = {:?};",
                    BLEND_FIXED_POINT_SCALE as f32
                ),
            ),
            (
                "cmaa2_shapes.slang",
                shapes,
                format!("static const float U_SHAPE_WEIGHT = {U_SHAPE_WEIGHT:?};"),
            ),
            (
                "cmaa2_shapes.slang",
                shapes,
                format!("static const float MAX_BLEND_WEIGHT = {MAX_BLEND_WEIGHT:?};"),
            ),
            (
                "cmaa2_apply.slang",
                apply,
                format!("static const uint ACCUM_WORDS = {ACCUM_WORDS};"),
            ),
            (
                "cmaa2_apply.slang",
                apply,
                format!(
                    "static const float INV_BLEND_FIXED_POINT_SCALE = 1.0 / {:?};",
                    BLEND_FIXED_POINT_SCALE as f32
                ),
            ),
        ] {
            assert!(
                source.contains(&declaration),
                "{name} does not declare `{declaration}`"
            );
        }
    }

    /// Each member lands in the word the shader will read it from.
    #[test]
    fn every_member_is_written_at_the_offset_the_block_declares() {
        let bytes = Cmaa2Params {
            viewport_x: 1920,
            viewport_y: 1080,
        }
        .to_bytes();
        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(
            PARAMS_SIZE % 16,
            0,
            "std140 rounds a uniform block's size up to a multiple of 16"
        );
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        assert_eq!(uint_at(0), 1920, "viewport_x at offset 0");
        assert_eq!(uint_at(4), 1080, "viewport_y at offset 4");
        assert_eq!(uint_at(8), 0, "the std140 tail is padding and stays zero");
        assert_eq!(uint_at(12), 0, "the std140 tail is padding and stays zero");
    }

    /// An extent of nothing still asks for a frame, because a dispatch of no
    /// groups is something Metal refuses outright.
    #[test]
    fn a_zero_extent_is_floored_at_one_texel() {
        assert_eq!(
            Cmaa2Params::for_extent(0, 0),
            Cmaa2Params {
                viewport_x: 1,
                viewport_y: 1,
            }
        );
    }

    /// The fixed-point scale's two bounds, as arithmetic rather than as prose.
    ///
    /// Below: one step of the scale is finer than one step of an eight-bit
    /// channel.
    ///
    /// Above: the widest total one pixel's weight word can be given, over-bound
    /// by giving it **every element** of a run of [`MAX_LINE_LENGTH`] in each
    /// of the [`WALK_DIRECTIONS`] a run can reach it from — which is far more
    /// than the four contributions the walk can actually leave, one per
    /// direction — and each at the largest share [`MAX_BLEND_WEIGHT`] allows.
    /// The three colour words are the same weights scaled by a channel
    /// saturated into `[0, 1]`, so the weight word bounds all four.
    #[test]
    fn the_fixed_point_scale_is_finer_than_the_target_and_wider_than_the_sum() {
        assert!(1.0 / f64::from(BLEND_FIXED_POINT_SCALE) < 1.0 / 255.0);
        let per_contribution = f64::from(MAX_BLEND_WEIGHT) * f64::from(BLEND_FIXED_POINT_SCALE);
        let widest = f64::from(WALK_DIRECTIONS) * f64::from(MAX_LINE_LENGTH) * per_contribution;
        assert!(
            widest < f64::from(u32::MAX),
            "a pixel can be given up to {widest} of fixed point and a u32 holds {}",
            f64::from(u32::MAX)
        );
    }
}
