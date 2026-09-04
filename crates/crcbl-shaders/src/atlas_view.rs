//! The uniform block and the constants `atlas_view.slang` reads, in the layouts
//! that shader declares.
//!
//! Same reason as [`crate::contact_shadows`]: the shader fixes a byte layout and
//! a set of values, every producer of those has to agree with it exactly, and
//! keeping both in the crate that owns the source means there is one place to
//! change rather than one per consumer.

/// Bytes of the uniform block: two `float4` rows and one `float4` per atlas
/// slot.
///
/// `std140` gives a `float4` one sixteen-byte row and an array of them one row
/// each, and the total is already a multiple of sixteen, so there is no tail
/// padding to write. See [`AtlasViewParams::to_bytes`].
pub const PARAMS_SIZE: usize = 16 + 16 + 16 * crate::mesh::SHADOW_ATLAS_TILES;

/// The reversed-Z far plane the shadow pass clears the atlas to, matching
/// `static const float DEPTH_CLEAR = 0.0;` in `shaders/atlas_view.slang`.
///
/// A texel still holding it is a texel nothing was drawn into.
/// `crcbl_hal::depth::CLEAR` is the seam's spelling of the same value, and
/// `crcbl_render::forward` is where the two are asserted equal.
pub const DEPTH_CLEAR: f32 = 0.0;

/// What the frame outside the atlas's rectangle draws, matching
/// `static const float SURROUND = 0.0;` in `shaders/atlas_view.slang`.
pub const SURROUND: f32 = 0.0;

/// What a texel still at [`DEPTH_CLEAR`] draws, matching
/// `static const float EMPTY_GREY = 0.06;` in `shaders/atlas_view.slang`.
///
/// Above [`SURROUND`] so an empty tile is not the letterbox, and far below
/// [`OCCUPIED_FLOOR`] so no depth a caster could have written lands near it.
pub const EMPTY_GREY: f32 = 0.06;

/// The darkest a texel something **was** drawn into may draw, matching
/// `static const float OCCUPIED_FLOOR = 0.3;` in `shaders/atlas_view.slang`.
///
/// The shader draws `lerp(OCCUPIED_FLOOR, 1.0, depth)`, so this is the value a
/// caster at the far end of a cascade gets rather than the black it would be
/// drawn at its own depth. Mirrored here because it is what the gap between an
/// occupied tile and an empty one is worth, and
/// `crates/crcbl/tests/forward_e2e/shadow.rs` is what measures that gap.
pub const OCCUPIED_FLOOR: f32 = 0.3;

/// How wide an occupied tile's border is, in **frame pixels**, matching
/// `static const float BORDER_PIXELS = 2.0;` in `shaders/atlas_view.slang`.
///
/// Frame pixels rather than atlas texels — the shader's own declaration says
/// why, and it is the difference between a border and nothing.
pub const BORDER_PIXELS: f32 = 2.0;

/// The colour an occupied tile's border draws, matching
/// `static const float3 BORDER_TINT` in `shaders/atlas_view.slang`.
///
/// Amber: red leads blue by more than any grey in this picture can, which is
/// what makes the tile grid readable over an atlas that is dark in places and
/// bright in others.
pub const BORDER_TINT: [f32; 3] = [1.0, 0.55, 0.15];

const _: () = assert!(
    EMPTY_GREY < OCCUPIED_FLOOR,
    "the whole picture is this one distinction: the shader draws every occupied \
     texel at or above OCCUPIED_FLOOR, so a pair that overlapped would be a \
     viewer in which an empty tile and a distant caster are the same rectangle \
     with nothing to say so"
);

const _: () = assert!(
    SURROUND < EMPTY_GREY,
    "the letterbox and an empty tile would draw the same value, so the atlas's \
     own edge is invisible and a reviewer cannot tell how much of the frame the \
     picture covers"
);

/// `atlas_view.slang`'s uniform block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtlasViewParams {
    /// Where the atlas is drawn in the frame, in pixels: `xy` the top-left
    /// corner and `zw` the size. [`AtlasViewParams::letterboxed`] is what
    /// computes it.
    pub view: [f32; 4],
    /// The atlas's extent in texels.
    pub atlas: [f32; 2],
    /// Where each atlas slot's map is, straight off
    /// [`crate::mesh::FrameUniforms::shadow_atlas_rect`]: `xy` the map's size as
    /// a fraction of the atlas and `zw` its origin.
    pub rect: [[f32; 4]; crate::mesh::SHADOW_ATLAS_TILES],
}

impl Default for AtlasViewParams {
    /// A block with no rectangle anywhere in it.
    ///
    /// A zero `view` draws the surround over the whole frame, which is the tell
    /// rather than a picture that nearly works: the extent and the atlas's own
    /// size are the two fields only the renderer knows, and a block still
    /// carrying these has not been filled in.
    fn default() -> Self {
        Self {
            view: [0.0; 4],
            atlas: [0.0; 2],
            rect: [[0.0; 4]; crate::mesh::SHADOW_ATLAS_TILES],
        }
    }
}

impl AtlasViewParams {
    /// The block that draws an `atlas`-texel image centred in a `target`-pixel
    /// frame at the atlas's own aspect.
    ///
    /// The letterbox is computed here rather than in the shader because it is
    /// two divisions per frame instead of two per pixel, and because a degenerate
    /// extent is a thing a caller can be told about on the CPU: an atlas or a
    /// frame with a zero side gets a zero-sized view, which draws
    /// [`SURROUND`] everywhere rather than dividing by nothing.
    #[must_use]
    pub fn letterboxed(
        target: (u32, u32),
        atlas: (u32, u32),
        rect: [[f32; 4]; crate::mesh::SHADOW_ATLAS_TILES],
    ) -> Self {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a frame extent is a few thousand pixels and an atlas a few thousand texels"
        )]
        let (target, atlas_size) = (
            (target.0 as f32, target.1 as f32),
            (atlas.0 as f32, atlas.1 as f32),
        );
        let scale = if atlas_size.0 > 0.0 && atlas_size.1 > 0.0 {
            (target.0 / atlas_size.0).min(target.1 / atlas_size.1)
        } else {
            0.0
        };
        let size = (atlas_size.0 * scale, atlas_size.1 * scale);
        Self {
            view: [
                (target.0 - size.0) * 0.5,
                (target.1 - size.1) * 0.5,
                size.0,
                size.1,
            ],
            atlas: [atlas_size.0, atlas_size.1],
            rect,
        }
    }

    /// The block as the bytes a uniform buffer holds.
    ///
    /// Little-endian, and the two padding words after [`atlas`] are written
    /// rather than left alone for [`crate::contact_shadows::ContactShadowParams::to_bytes`]'s
    /// reason: the buffer is [`PARAMS_SIZE`] wide and a partial write leaves the
    /// tail undefined.
    ///
    /// [`atlas`]: Self::atlas
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut at = 0;
        for value in self
            .view
            .into_iter()
            .chain(self.atlas)
            // The two words that close the `atlas` row.
            .chain([0.0, 0.0])
            .chain(self.rect.into_iter().flatten())
        {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        debug_assert_eq!(at, PARAMS_SIZE, "the block is written whole");
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The source the checks below read, hash-pinned by the manifest — so it is
    /// the same file the committed artifact was built from.
    const SOURCE: &str = include_str!("../shaders/atlas_view.slang");

    /// The block the shader declares, member for member.
    ///
    /// Nothing else can catch a rename or a reorder: the shader compiles either
    /// way and the buffer is bound either way, and a block whose members moved
    /// would read the atlas's extent as a letterbox corner and draw the whole
    /// frame surround.
    #[test]
    fn the_uniform_block_matches_the_struct_atlas_view_slang_declares() {
        for member in [
            "float4 view;",
            "float4 atlas;",
            "float4 rect[SHADOW_ATLAS_TILES];",
        ] {
            assert!(
                SOURCE.contains(member),
                "atlas_view.slang does not declare `{member}`"
            );
        }
        assert!(
            SOURCE.contains("ConstantBuffer<AtlasViewParams> params;"),
            "atlas_view.slang does not bind the block `to_bytes` writes"
        );
    }

    /// **The shader sizes its rectangle array with the slot count this crate
    /// declares.**
    ///
    /// [`crate::volumetric`]'s
    /// `the_two_shaders_declare_one_block`-shaped check, for its reason: an
    /// array a slot short leaves the last slot's rectangle unwritten, and one a
    /// slot long reads past the block — and both compile and draw a picture.
    #[test]
    fn the_slot_count_matches_the_one_the_host_declares() {
        let slots = crate::mesh::SHADOW_ATLAS_TILES;
        let declaration = format!("static const uint SHADOW_ATLAS_TILES = {slots};");
        assert!(
            SOURCE.contains(&declaration),
            "atlas_view.slang does not declare `{declaration}`, so its block is a different \
             length from the one `to_bytes` writes"
        );
    }

    /// **The constants and the shader name the same values.**
    ///
    /// [`crate::contact_shadows`]'s
    /// `the_constants_match_the_ones_contact_shadows_slang_declares`, for its
    /// reason: the shader draws whatever these say, and a disagreement shows up
    /// only as a picture whose greys mean something other than what a reader was
    /// told.
    #[test]
    fn the_constants_match_the_ones_atlas_view_slang_declares() {
        for declaration in [
            format!("static const float DEPTH_CLEAR = {DEPTH_CLEAR:.1};"),
            format!("static const float SURROUND = {SURROUND:.1};"),
            format!("static const float EMPTY_GREY = {EMPTY_GREY};"),
            format!("static const float OCCUPIED_FLOOR = {OCCUPIED_FLOOR};"),
            format!("static const float BORDER_PIXELS = {BORDER_PIXELS:.1};"),
            // `{:?}` and not `{}`: `Display` writes `1.0` as `1`, and the
            // shader spells a `float3` component the way Slang requires.
            format!(
                "static const float3 BORDER_TINT = float3({:?}, {:?}, {:?});",
                BORDER_TINT[0], BORDER_TINT[1], BORDER_TINT[2]
            ),
        ] {
            assert!(
                SOURCE.contains(&declaration),
                "atlas_view.slang does not declare `{declaration}`; the mirror has drifted"
            );
        }
    }

    /// The atlas keeps its aspect and is centred, and the block carries the
    /// extent that maps a position in it back to a texel.
    #[test]
    fn the_atlas_is_letterboxed_into_the_frame_at_its_own_aspect() {
        let rect = [[0.0; 4]; crate::mesh::SHADOW_ATLAS_TILES];
        // A square atlas in a 4:3 frame: as tall as the frame, centred across.
        let params = AtlasViewParams::letterboxed((256, 192), (3072, 3072), rect);
        assert_eq!(params.view, [32.0, 0.0, 192.0, 192.0]);
        assert_eq!(params.atlas, [3072.0, 3072.0]);

        // And the other way round, which is the case a frame taller than it is
        // wide gives.
        let portrait = AtlasViewParams::letterboxed((192, 256), (3072, 3072), rect);
        assert_eq!(portrait.view, [0.0, 32.0, 192.0, 192.0]);
    }

    /// A degenerate extent draws the surround rather than dividing by nothing.
    #[test]
    fn a_frame_or_an_atlas_with_no_size_gets_no_rectangle() {
        let rect = [[0.0; 4]; crate::mesh::SHADOW_ATLAS_TILES];
        for (target, atlas) in [((0, 0), (3072, 3072)), ((256, 192), (0, 0))] {
            let params = AtlasViewParams::letterboxed(target, atlas, rect);
            assert!(
                params.view.iter().all(|value| value.is_finite()),
                "{target:?} into {atlas:?} left {:?} in the view",
                params.view
            );
            assert_eq!(params.view[2], 0.0, "{target:?} into {atlas:?}");
            assert_eq!(params.view[3], 0.0, "{target:?} into {atlas:?}");
        }
    }

    /// Each member lands in the word the shader will read it from, and the
    /// padding that closes the second row is written too.
    #[test]
    fn every_member_is_written_at_the_offset_the_block_declares() {
        let params = AtlasViewParams {
            view: [1.0, 2.0, 3.0, 4.0],
            atlas: [5.0, 6.0],
            rect: core::array::from_fn(|slot| {
                #[expect(clippy::cast_precision_loss, reason = "a slot is a small index")]
                let base = 10.0 + slot as f32;
                [base, base + 0.1, base + 0.2, base + 0.3]
            }),
        };
        let bytes = params.to_bytes();
        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[12..16], &4.0f32.to_le_bytes());
        assert_eq!(&bytes[16..20], &5.0f32.to_le_bytes());
        assert_eq!(&bytes[20..24], &6.0f32.to_le_bytes());
        assert_eq!(&bytes[24..32], &[0u8; 8], "the atlas row is not closed");
        for slot in 0..crate::mesh::SHADOW_ATLAS_TILES {
            let at = 32 + 16 * slot;
            assert_eq!(
                &bytes[at..at + 4],
                &params.rect[slot][0].to_le_bytes(),
                "slot {slot} is not at {at}"
            );
        }
    }

    /// A block nobody filled in draws nothing rather than something that nearly
    /// works.
    #[test]
    fn the_default_block_has_no_rectangle_in_it() {
        assert_eq!(AtlasViewParams::default().view, [0.0; 4]);
        assert_eq!(AtlasViewParams::default().atlas, [0.0; 2]);
    }
}
