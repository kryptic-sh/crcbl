//! The workgroup size and uniform block `cull.slang` declares, in the layouts
//! that shader declares.
//!
//! Same reason as [`crate::compute_probe`]: the shader fixes a number and a byte
//! layout, every producer of those has to agree with it exactly, and keeping
//! both in the crate that owns the source means there is one place to change
//! rather than one per consumer.
//!
//! What is *not* here is the frustum itself. Extracting six planes from a
//! view-projection matrix needs a matrix type, and this crate has no
//! dependencies at all — not even `glam`. [`crcbl_render::cull`] owns that, and
//! hands the result here as six `[f32; 4]`s.
//!
//! [`crcbl_render::cull`]: https://docs.rs/crcbl-render

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in
/// `shaders/cull.slang`.
///
/// A caller dispatches `instances.div_ceil(WORKGROUP_SIZE)` groups; the shader
/// discards the invocations past [`Params::instance_count`] that the last group
/// brings.
pub const WORKGROUP_SIZE: u32 = 64;

/// Planes in a frustum, and the length of [`Params::planes`].
pub const PLANE_COUNT: usize = 6;

/// Words in the frame's culling-statistics buffer.
///
/// **One buffer, and topic 03 §3.6's single permitted readback.**
/// `cull.slang` adds surviving instances into [`INSTANCE_SURVIVOR_WORD`],
/// `mesh_cluster.slang`'s amplification stage adds each cluster it tested into
/// exactly one of [`CLUSTER_SURVIVOR_WORD`], [`CLUSTER_FRUSTUM_REJECT_WORD`]
/// and [`CLUSTER_CONE_REJECT_WORD`], and `light_cluster.slang` adds the light
/// assignments its budget refused into
/// [`CLUSTER_OVERFLOW_WORD`](crate::light::CLUSTER_OVERFLOW_WORD). A counter of
/// its own for any of them would be another buffer to zero, to barrier and to
/// copy back every frame.
///
/// `clear_counters.slang` is told this number rather than assuming it — see
/// [`crate::clear_counters::Params::stats_words`] — because a clearing pass
/// that zeroed a prefix would leave one of them carrying the previous frame's
/// total, which reads as a plausible count rather than as a failure.
pub const STATS_WORDS: u32 = 8;

/// Which word of the culling statistics counts surviving **instances** — the
/// one `cull.slang` writes.
pub const INSTANCE_SURVIVOR_WORD: u32 = 0;

/// Which word counts surviving **clusters** — the one `mesh_cluster.slang`'s
/// amplification stage writes.
///
/// Zero on a device without `Features::TASK_SHADER`, and on the two indirect
/// geometry paths: there is no amplification stage to add to it, and nothing
/// else in the frame culls a cluster.
pub const CLUSTER_SURVIVOR_WORD: u32 = 1;

/// Which word counts the clusters the **frustum** rejected — the first of
/// `mesh_cluster.slang`'s two per-cluster tests.
///
/// The word that makes [`CLUSTER_SURVIVOR_WORD`] readable. A survivor count on
/// its own says how much of the cut reached the raster and nothing about which
/// test removed the rest, so "27 of 338 survived" is equally consistent with the
/// normal cone doing all of the work and with it doing none.
///
/// Counted for a cluster that was **tested**, which is one the DAG descent
/// selected in a live instance's run. A cluster outside the cut is in none of
/// the three words — see the amplification stage, where that is the whole reason
/// its one atomic is guarded.
///
/// Zero wherever [`CLUSTER_SURVIVOR_WORD`] is, and for the same reason: no
/// amplification stage ran, so nothing rejected a cluster either.
pub const CLUSTER_FRUSTUM_REJECT_WORD: u32 = 3;

/// Which word counts the clusters the **normal cone** rejected — the second of
/// the two tests, reached only by a cluster the frustum kept.
///
/// Ordered, not independent: the two tests are asked in sequence and a cluster
/// lands in the bucket of the first one that refused it. So this is "faced away
/// *and* was on screen", which is the number that says what back-face rejection
/// is worth on this geometry; a cluster that is both behind the camera and
/// facing away is the frustum's.
pub const CLUSTER_CONE_REJECT_WORD: u32 = 4;

/// Which word counts the survivors the **first** occlusion phase marked hidden
/// — `cull.slang`'s `occlusionMain`, testing against the previous frame's
/// pyramid.
///
/// Not a count of instances the frame dropped: the second phase tests every one
/// of these again, and [`OCCLUSION_LATE_REJECT_WORD`] is what it still found
/// hidden. The difference between the two is what the first phase got wrong —
/// disocclusions, a camera cut, anything that moved — and costs a late draw
/// rather than a pixel.
pub const OCCLUSION_EARLY_REJECT_WORD: u32 = 5;

/// Which word counts the survivors the **second** occlusion phase still found
/// hidden — `cull.slang`'s `lateMain` — which are the frustum survivors the
/// frame did not draw.
pub const OCCLUSION_LATE_REJECT_WORD: u32 = 6;

/// Which word counts the instances the small-feature test dropped, which are
/// not survivors at all.
pub const SMALL_FEATURE_REJECT_WORD: u32 = 7;

// Each counter owns one word. A further counter would need `STATS_WORDS` raised
// with it, and this is what says so at build time. The light grid's own word is
// asserted the same way beside its declaration in [`crate::light`].
const _: () = assert!(INSTANCE_SURVIVOR_WORD < STATS_WORDS);
const _: () = assert!(CLUSTER_SURVIVOR_WORD < STATS_WORDS);
const _: () = assert!(CLUSTER_FRUSTUM_REJECT_WORD < STATS_WORDS);
const _: () = assert!(CLUSTER_CONE_REJECT_WORD < STATS_WORDS);
const _: () = assert!(OCCLUSION_EARLY_REJECT_WORD < STATS_WORDS);
const _: () = assert!(OCCLUSION_LATE_REJECT_WORD < STATS_WORDS);
const _: () = assert!(SMALL_FEATURE_REJECT_WORD < STATS_WORDS);
const _: () = assert!(INSTANCE_SURVIVOR_WORD != CLUSTER_SURVIVOR_WORD);
const _: () = assert!(INSTANCE_SURVIVOR_WORD != CLUSTER_FRUSTUM_REJECT_WORD);
const _: () = assert!(INSTANCE_SURVIVOR_WORD != CLUSTER_CONE_REJECT_WORD);
const _: () = assert!(CLUSTER_SURVIVOR_WORD != CLUSTER_FRUSTUM_REJECT_WORD);
const _: () = assert!(CLUSTER_SURVIVOR_WORD != CLUSTER_CONE_REJECT_WORD);
const _: () = assert!(CLUSTER_FRUSTUM_REJECT_WORD != CLUSTER_CONE_REJECT_WORD);

/// Faces of a point light's cube, and the number of face tags a survivor
/// carries — `crcbl_render::shadow::POINT_FACES`, which is the shader's
/// `SHADOW_POINT_FACES`.
pub const FACE_COUNT: usize = crate::mesh::SHADOW_POINT_FACES;

/// Side planes kept per point-light face — see [`Features::FACES`].
pub const FACE_PLANES: usize = 4;

/// Length of [`Params::face_planes`].
pub const FACE_PLANE_SLOTS: usize = FACE_COUNT * FACE_PLANES;

/// The low bits of a survivor-list entry, which are the instance index.
///
/// **Every bit above it is a tag** the cull writes about the survivor — its
/// point-light faces, or its occlusion state — so an instance capacity reaching
/// it is one `crcbl_render::DrawGen::new` refuses rather than one whose indices
/// would read as tags.
pub const ENTRY_INDEX_MASK: u32 = (1 << 24) - 1;

/// Where a survivor's six face tags start: bit `ENTRY_FACE_SHIFT + f` is face
/// `f` of a point light, written under [`Features::FACES`].
pub const ENTRY_FACE_SHIFT: u32 = 24;

/// The tag `cull.slang`'s `lateMain` sets on a survivor the second occlusion
/// phase found visible after the first found it hidden.
pub const ENTRY_RESCUED: u32 = 1 << 30;

/// The tag `cull.slang`'s `occlusionMain` sets on a survivor the first phase
/// found hidden behind the previous frame's pyramid.
pub const ENTRY_OCCLUDED: u32 = 1 << 31;

const _: () = assert!(ENTRY_FACE_SHIFT as usize + FACE_COUNT <= 30);
const _: () = assert!(ENTRY_INDEX_MASK < 1 << ENTRY_FACE_SHIFT);

/// Levels of the farthest-depth pyramid `cull.slang` binds in set 1.
///
/// Eight, which reaches a coarsest level of 7×4 texels on a 1920×1080 prepass
/// and costs eight of the sixteen sampled textures a WebGPU stage guarantees.
pub const OCCLUSION_MAX_LEVELS: u32 = 8;

/// Texels the projected rectangle is widened by on every side before the
/// pyramid is read. See `cull.slang`'s declaration.
pub const OCCLUSION_GUARD_TEXELS: i32 = 1;

/// How much nearer than the stored depth, as a fraction of it, an instance's
/// nearest corner must be to count as hidden — `2^-12`. See `cull.slang`'s
/// declaration.
pub const OCCLUSION_DEPTH_SLACK: f32 = 1.0 / 4096.0;

/// A projected coordinate at or past this many pixels from the origin makes a
/// box unprojectable. See `cull.slang`'s declaration.
pub const PROJECTION_LIMIT: f32 = 16_777_216.0;

/// What a frame's cull dispatches do beyond the frustum — `CullParams::features`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Features(u32);

impl Features {
    /// Nothing beyond the frustum: the bits every cull had before this block
    /// grew, and every shadow cascade's and spot's.
    pub const NONE: Self = Self(0);
    /// Tag each survivor with the point-light faces its box reaches, read by
    /// `computeMain` from [`Params::face_planes`].
    pub const FACES: Self = Self(1);
    /// The farthest-depth pyramid holds the previous frame, so `occlusionMain`
    /// may test against it through [`Params::previous_view_proj`].
    pub const OCCLUSION_HISTORY: Self = Self(2);
    /// `occlusionMain` drops an instance whose projected box's longer side is
    /// under [`Params::small_feature_pixels`].
    pub const SMALL_FEATURE: Self = Self(4);

    /// The word the shader reads.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Both sets of bits.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every bit of `other` is set here.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Bytes of the uniform block.
///
/// Six `float4` (96), four `uint` (112), two `float4x4` (240), four `uint`
/// (256), four `float` (272) and [`FACE_PLANE_SLOTS`] `float4`s (656), which is
/// already the 16-byte multiple `std140` requires of a uniform block's size.
/// Checked against the `Offset` decorations `slangc` emits by this module's
/// `the_cull_params_block_matches_the_offsets_slangc_emits`.
pub const PARAMS_SIZE: usize = 656;

/// The uniform block, matching `struct CullParams` in `shaders/cull.slang`.
///
/// `Default` is a cull of nothing with no features: zero planes reject nothing
/// and zero instances test nothing, so a caller fills what it uses.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Params {
    /// The camera frustum's six half-spaces: `[nx, ny, nz, d]`, with a point
    /// inside when `n · p + d >= 0`.
    ///
    /// **Not normalized, and the shader does not want them to be.** Its test
    /// scales linearly with `n`, so normalizing changes no answer — and under
    /// the engine's reversed-Z infinite projection one plane comes out with a
    /// zero normal, which normalizing would turn into `NaN`. See
    /// [`crcbl_render::cull::Frustum`], which produces these.
    ///
    /// [`crcbl_render::cull::Frustum`]: https://docs.rs/crcbl-render
    pub planes: [[f32; 4]; PLANE_COUNT],
    /// Instances to test. Invocations at or past this index do nothing.
    pub instance_count: u32,
    /// Elements the visible list can hold. A survivor past this is counted and
    /// not written — see the shader, where the counter is deliberately the
    /// unbounded half.
    pub capacity: u32,
    /// The [`GpuInstance::flags`](crate::mesh::GpuInstance::flags) bit that
    /// removes an instance from this cull: one bit of
    /// [`GpuInstance::HIDDEN_VIEWS_MASK`](crate::mesh::GpuInstance::HIDDEN_VIEWS_MASK)
    /// for a camera's cull, and zero for a cull that is no view's — a shadow
    /// cascade's or a shadowed light's — which rejects nothing on it.
    pub hidden_view: u32,
    /// What the dispatches do beyond the frustum.
    pub features: Features,
    /// This frame's world → clip, column-major — the second occlusion phase's
    /// and the small-feature test's projection.
    pub view_proj: [f32; 16],
    /// The previous frame's, which the first occlusion phase reprojects
    /// through.
    pub previous_view_proj: [f32; 16],
    /// The depth prepass's extent in texels, which is level 0 of the pyramid.
    pub target_extent: [u32; 2],
    /// Levels of the farthest-depth pyramid bound for the occlusion entry
    /// points, at most [`OCCLUSION_MAX_LEVELS`]. Zero hides nothing.
    pub pyramid_levels: u32,
    /// The small-feature threshold, in pixels of the projected box's longer
    /// side.
    pub small_feature_pixels: f32,
    /// Under [`Features::FACES`], face `f`'s four side planes at
    /// `f * FACE_PLANES`, on [`planes`](Self::planes)' convention.
    pub face_planes: [[f32; 4]; FACE_PLANE_SLOTS],
}

impl Params {
    /// The block as the bytes a uniform buffer holds, in `std140` order.
    ///
    /// The tail padding is written rather than left alone, for the reason
    /// [`crate::compute_probe::Params::to_bytes`] gives: a buffer allocated for
    /// this block is [`PARAMS_SIZE`] bytes wide and a partial write leaves the
    /// rest undefined.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut at = 0usize;
        for plane in &self.planes {
            for value in plane {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
        }
        for value in [
            self.instance_count,
            self.capacity,
            self.hidden_view,
            self.features.bits(),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for value in self.view_proj.iter().chain(&self.previous_view_proj) {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for value in [
            self.target_extent[0],
            self.target_extent[1],
            self.pyramid_levels,
            0,
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for value in [self.small_feature_pixels, 0.0, 0.0, 0.0] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for plane in &self.face_planes {
            for value in plane {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
        }
        debug_assert_eq!(at, PARAMS_SIZE, "the block has no tail padding");
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant and the shader must name the same workgroup size.
    ///
    /// Nothing else can catch this, for the reason
    /// [`crate::compute_probe`]'s twin gives: the shader compiles, the dispatch
    /// succeeds, and a mismatch shows up only as a partly-tested instance array
    /// — which reads as "those instances were culled".
    #[test]
    fn the_workgroup_size_matches_the_numthreads_cull_slang_declares() {
        let source = include_str!("../shaders/cull.slang");
        let declaration = format!("[numthreads({WORKGROUP_SIZE}, 1, 1)]");
        assert!(
            source.contains(&declaration),
            "cull.slang does not declare `{declaration}`; WORKGROUP_SIZE has drifted from the \
             shader"
        );
    }

    /// The offsets `slangc` actually emitted for `CullParams`, read out of the
    /// disassembly.
    #[test]
    fn the_cull_params_block_matches_the_offsets_slangc_emits() {
        // `OpDecorate %_arr_v4float_int_6 ArrayStride 16`, and
        // `OpMemberDecorate %CullParams_std140 n Offset …`: 0, 96, 100, 104,
        // 108, 112, 176, 240, 244, 248, 252, 256, 260, 264, 268, 272.
        assert_eq!(PARAMS_SIZE, 656);
        assert_eq!(
            PARAMS_SIZE % 16,
            0,
            "std140 rounds a uniform block's size up to a multiple of 16, so a block that is not \
             one already is a block the shader and the CPU disagree about the width of"
        );

        let mut planes = [[0.0f32; 4]; PLANE_COUNT];
        for (index, plane) in planes.iter_mut().enumerate() {
            *plane = [index as f32, 0.0, 0.0, 0.0];
        }
        let bytes = Params {
            planes,
            instance_count: 7,
            capacity: 9,
            hidden_view: 1 << 10,
            features: Features::FACES.union(Features::SMALL_FEATURE),
            view_proj: core::array::from_fn(|index| 100.0 + index as f32),
            previous_view_proj: core::array::from_fn(|index| 200.0 + index as f32),
            target_extent: [640, 480],
            pyramid_levels: 6,
            small_feature_pixels: 2.5,
            face_planes: core::array::from_fn(|index| [300.0 + index as f32, 0.0, 0.0, -1.0]),
        }
        .to_bytes();
        let float_at =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        let uint_at =
            |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("4"));
        for index in 0..PLANE_COUNT {
            assert_eq!(
                float_at(index * 16),
                index as f32,
                "plane {index} at offset {}",
                index * 16
            );
        }
        assert_eq!(uint_at(96), 7, "instance_count at offset 96");
        assert_eq!(uint_at(100), 9, "capacity at offset 100");
        assert_eq!(uint_at(104), 1 << 10, "hidden_view at offset 104");
        assert_eq!(uint_at(108), 5, "features at offset 108");
        assert_eq!(float_at(112), 100.0, "view_proj at offset 112");
        assert_eq!(float_at(172), 115.0, "and sixteen floats wide");
        assert_eq!(float_at(176), 200.0, "previous_view_proj at offset 176");
        assert_eq!(float_at(236), 215.0, "and sixteen floats wide");
        assert_eq!(uint_at(240), 640, "target_width at offset 240");
        assert_eq!(uint_at(244), 480, "target_height at offset 244");
        assert_eq!(uint_at(248), 6, "pyramid_levels at offset 248");
        assert_eq!(uint_at(252), 0, "then a padding word, written as zero");
        assert_eq!(float_at(256), 2.5, "small_feature_pixels at offset 256");
        assert!(
            bytes[260..272].iter().all(|byte| *byte == 0),
            "three padding floats, written as zero"
        );
        for slot in 0..FACE_PLANE_SLOTS {
            let at = 272 + slot * 16;
            assert_eq!(
                float_at(at),
                300.0 + slot as f32,
                "face plane {slot} at {at}"
            );
            assert_eq!(float_at(at + 12), -1.0, "and its offset last");
        }
    }

    /// `cull.slang`, `draw_gen.slang` and `mesh_cluster.slang` re-declare
    /// `GpuInstance` and `GpuMesh` because there is no shared header — the
    /// compile script hashes one source per artifact, so an `#include` would be
    /// a file whose edits nothing downstream notices. This is what keeps the
    /// copies from drifting: it compares the *fields*, so a reworded doc
    /// comment is not a failure and a renamed, retyped, reordered, added or
    /// removed field is.
    ///
    /// A drift here is not a compile error anywhere. Every file builds; the
    /// shaders simply read different bytes out of the same buffer.
    #[test]
    fn the_shared_structs_are_declared_identically_in_every_shader() {
        let mesh = include_str!("../shaders/mesh.slang");
        let cull = include_str!("../shaders/cull.slang");
        let cluster = include_str!("../shaders/mesh_cluster.slang");
        let others = [
            ("cull.slang", cull),
            ("draw_gen.slang", include_str!("../shaders/draw_gen.slang")),
            ("mesh_cluster.slang", cluster),
        ];
        for name in ["GpuInstance", "GpuMesh"] {
            let declared = struct_fields(mesh, name);
            assert!(
                !declared.is_empty(),
                "`struct {name}` was not found in mesh.slang, so this comparison checked nothing"
            );
            for (source, text) in others {
                assert_eq!(
                    declared,
                    struct_fields(text, name),
                    "`struct {name}` differs between mesh.slang and {source}; the two shaders \
                     would read the same buffer with different layouts"
                );
            }
        }

        // `MeshVertex` has a wider spread than either: `skinning.slang` writes
        // the pool that `mesh.slang` and `mesh_cluster.slang` pull from, so a
        // drift there is one pass writing a layout the other two read with. It
        // is not a compile error in any of the three — every file builds and
        // the skinned mesh simply comes out as noise.
        {
            let declared = struct_fields(mesh, "MeshVertex");
            assert!(
                !declared.is_empty(),
                "`struct MeshVertex` was not found in mesh.slang, so this comparison checked \
                 nothing"
            );
            for (source, text) in [
                ("mesh_cluster.slang", cluster),
                ("skinning.slang", include_str!("../shaders/skinning.slang")),
            ] {
                assert_eq!(
                    declared,
                    struct_fields(text, "MeshVertex"),
                    "`struct MeshVertex` differs between mesh.slang and {source}; one pass \
                     would write the vertex pool with a layout another reads it with"
                );
            }
        }

        // `FrameUniforms` and `GpuLight` have a narrower spread: the frame block
        // is the raster and mesh pipelines' shared uniform buffer, and the light
        // row is read by the fragment stage and written by the clustering pass.
        // A drift in either is the same silent class as the two above — one
        // buffer, two layouts — so each pair is compared over the files that
        // really declare it rather than over all of them.
        for (name, files) in [
            ("FrameUniforms", &[("mesh_cluster.slang", cluster)][..]),
            (
                "GpuLight",
                &[
                    ("mesh_cluster.slang", cluster),
                    (
                        "light_cluster.slang",
                        include_str!("../shaders/light_cluster.slang"),
                    ),
                ][..],
            ),
        ] {
            let declared = struct_fields(mesh, name);
            assert!(
                !declared.is_empty(),
                "`struct {name}` was not found in mesh.slang, so this comparison checked nothing"
            );
            for (source, text) in files {
                assert_eq!(
                    declared,
                    struct_fields(text, name),
                    "`struct {name}` differs between mesh.slang and {source}; the two shaders \
                     would read the same buffer with different layouts"
                );
            }
        }

        // `CullParams` has no copy in `mesh.slang` at all — the raster path
        // never sees a frustum — so its pair is compared on its own. The two
        // stages read the *same* uniform buffer, so a layout drift would hand
        // the amplification stage the instance count where a plane belongs.
        let planes = struct_fields(cull, "CullParams");
        assert!(
            !planes.is_empty(),
            "`struct CullParams` was not found in cull.slang, so this compared nothing"
        );
        assert_eq!(
            planes,
            struct_fields(cluster, "CullParams"),
            "`struct CullParams` differs between cull.slang and mesh_cluster.slang; the \
             instance cull and the cluster cull would read one buffer with two layouts"
        );
    }

    /// **Every shader that touches the culling statistics indexes the word this
    /// crate says it does**, and none of them can be checked by a compiler.
    ///
    /// Both counters live in one buffer, so a shader writing the other's word
    /// produces a plausible number for the wrong thing and a zero for the right
    /// one — and `clear_counters.slang` would zero both either way, so nothing
    /// downstream looks wrong enough to investigate.
    #[test]
    fn the_shaders_index_the_culling_stats_at_the_words_this_crate_says() {
        for (source, text, name, value) in [
            (
                "cull.slang",
                include_str!("../shaders/cull.slang"),
                "INSTANCE_SURVIVOR_WORD",
                INSTANCE_SURVIVOR_WORD,
            ),
            (
                "mesh_cluster.slang",
                include_str!("../shaders/mesh_cluster.slang"),
                "CLUSTER_SURVIVOR_WORD",
                CLUSTER_SURVIVOR_WORD,
            ),
            // The two rejection words are the same class of silence one step
            // further along: the amplification stage picks *one* of three words
            // per tested cluster, so a rejection landing on the survivor word
            // would report clusters as kept that the frame never drew, and the
            // three would still sum to the number tested.
            (
                "mesh_cluster.slang",
                include_str!("../shaders/mesh_cluster.slang"),
                "CLUSTER_FRUSTUM_REJECT_WORD",
                CLUSTER_FRUSTUM_REJECT_WORD,
            ),
            (
                "mesh_cluster.slang",
                include_str!("../shaders/mesh_cluster.slang"),
                "CLUSTER_CONE_REJECT_WORD",
                CLUSTER_CONE_REJECT_WORD,
            ),
            // Reads rather than adds, but reads the same shared block: the
            // draw-argument pass clamps its dispatch against the instance
            // survivor count, and reading the cluster word instead would clamp
            // against a number that is zero on three of the four geometry paths.
            (
                "draw_gen.slang",
                include_str!("../shaders/draw_gen.slang"),
                "INSTANCE_SURVIVOR_WORD",
                INSTANCE_SURVIVOR_WORD,
            ),
        ] {
            let declaration = format!("static const uint {name} = {value};");
            assert!(
                text.contains(&declaration),
                "{source} must declare `{declaration}`, or it counts into the other \
                 shader's word"
            );
        }
    }

    /// **The survivor tags, the new statistics words and the occlusion
    /// constants are the numbers this crate says**, in every shader that spells
    /// one.
    ///
    /// A tag bit two shaders disagree about is a survivor the cull marks hidden
    /// and the scatter draws early anyway — a frame that looks right and
    /// measures nothing — and a slack or guard band that drifted from the host
    /// oracle is a GPU list the oracle comparison reports as a bug in the one
    /// that did not change.
    #[test]
    fn the_occlusion_constants_are_spelled_the_same_in_every_shader() {
        let cull = include_str!("../shaders/cull.slang");
        let draw_gen = include_str!("../shaders/draw_gen.slang");
        for (source, text, declaration) in [
            (
                "cull.slang",
                cull,
                format!("static const uint ENTRY_INDEX_MASK = {ENTRY_INDEX_MASK};"),
            ),
            (
                "draw_gen.slang",
                draw_gen,
                format!("static const uint ENTRY_INDEX_MASK = {ENTRY_INDEX_MASK};"),
            ),
            (
                "cull.slang",
                cull,
                format!("static const uint ENTRY_FACE_SHIFT = {ENTRY_FACE_SHIFT};"),
            ),
            (
                "draw_gen.slang",
                draw_gen,
                format!("static const uint ENTRY_FACE_SHIFT = {ENTRY_FACE_SHIFT};"),
            ),
            (
                "cull.slang",
                cull,
                format!("static const uint ENTRY_RESCUED = {ENTRY_RESCUED};"),
            ),
            (
                "draw_gen.slang",
                draw_gen,
                format!("static const uint ENTRY_RESCUED = {ENTRY_RESCUED};"),
            ),
            (
                "cull.slang",
                cull,
                format!("static const uint ENTRY_OCCLUDED = {ENTRY_OCCLUDED};"),
            ),
            (
                "draw_gen.slang",
                draw_gen,
                format!("static const uint ENTRY_OCCLUDED = {ENTRY_OCCLUDED};"),
            ),
            (
                "cull.slang",
                cull,
                format!("static const uint FACE_COUNT = {FACE_COUNT};"),
            ),
            (
                "draw_gen.slang",
                draw_gen,
                format!("static const uint FACE_COUNT = {FACE_COUNT};"),
            ),
            (
                "cull.slang",
                cull,
                format!("static const uint FACE_PLANES = {FACE_PLANES};"),
            ),
            (
                "cull.slang",
                cull,
                format!("static const uint FACE_PLANE_SLOTS = {FACE_PLANE_SLOTS};"),
            ),
            (
                "mesh_cluster.slang",
                include_str!("../shaders/mesh_cluster.slang"),
                format!("static const uint FACE_PLANE_SLOTS = {FACE_PLANE_SLOTS};"),
            ),
            (
                "cull.slang",
                cull,
                format!(
                    "static const uint OCCLUSION_EARLY_REJECT_WORD = {OCCLUSION_EARLY_REJECT_WORD};"
                ),
            ),
            (
                "cull.slang",
                cull,
                format!(
                    "static const uint OCCLUSION_LATE_REJECT_WORD = {OCCLUSION_LATE_REJECT_WORD};"
                ),
            ),
            (
                "cull.slang",
                cull,
                format!(
                    "static const uint SMALL_FEATURE_REJECT_WORD = {SMALL_FEATURE_REJECT_WORD};"
                ),
            ),
            (
                "cull.slang",
                cull,
                format!("static const uint OCCLUSION_MAX_LEVELS = {OCCLUSION_MAX_LEVELS};"),
            ),
            (
                "cull.slang",
                cull,
                format!("static const int OCCLUSION_GUARD_TEXELS = {OCCLUSION_GUARD_TEXELS};"),
            ),
            (
                "cull.slang",
                cull,
                format!("static const float PROJECTION_LIMIT = {PROJECTION_LIMIT:.1};"),
            ),
            (
                "cull.slang",
                cull,
                format!("static const uint CULL_FACES = {};", Features::FACES.bits()),
            ),
            (
                "cull.slang",
                cull,
                format!(
                    "static const uint CULL_OCCLUSION_HISTORY = {};",
                    Features::OCCLUSION_HISTORY.bits()
                ),
            ),
            (
                "cull.slang",
                cull,
                format!(
                    "static const uint CULL_SMALL_FEATURE = {};",
                    Features::SMALL_FEATURE.bits()
                ),
            ),
        ] {
            assert!(
                text.contains(&declaration),
                "{source} must declare `{declaration}`"
            );
        }
        // The slack as the float the shader parses, rather than as text: the
        // source spells the exact binary fraction, which is not the shortest
        // decimal Rust prints for it.
        let slack = cull
            .split("static const float OCCLUSION_DEPTH_SLACK = ")
            .nth(1)
            .and_then(|rest| rest.split(';').next())
            .and_then(|literal| literal.parse::<f32>().ok());
        assert_eq!(
            slack,
            Some(OCCLUSION_DEPTH_SLACK),
            "cull.slang's OCCLUSION_DEPTH_SLACK is not the host's"
        );
        // Eight pyramid bindings, one per level, in set 1 and in order.
        for level in 1..=OCCLUSION_MAX_LEVELS {
            let declaration = format!(
                "[[vk::binding({}, 1)]]\nDepthTexture2D pyramid_{level} D3D12_REGISTER(",
                level - 1
            );
            assert!(
                cull.contains(&declaration),
                "cull.slang must declare `{declaration}`"
            );
        }
    }

    /// The field lines of `struct name` in `source`, with comments and blank
    /// lines dropped and each line trimmed.
    ///
    /// Empty when the struct is not there at all, which the caller asserts
    /// against — a matcher that silently found nothing would make two files
    /// with no structs in them agree perfectly.
    fn struct_fields(source: &str, name: &str) -> Vec<String> {
        let opening = format!("struct {name}\n{{\n");
        let Some(start) = source.find(&opening) else {
            return Vec::new();
        };
        let body = &source[start + opening.len()..];
        let end = body.find("\n};").unwrap_or(body.len());
        body[..end]
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("//"))
            .map(str::to_string)
            .collect()
    }

    /// The comparison above must be able to see a difference, which a matcher
    /// that returned the same thing for every input could not.
    #[test]
    fn the_struct_comparison_notices_a_changed_field() {
        let original = "struct Thing\n{\n    /// doc\n    uint a;\n    float b;\n};\n";
        let renamed = "struct Thing\n{\n    uint a;\n    float c;\n};\n";
        assert_eq!(
            struct_fields(original, "Thing"),
            vec!["uint a;".to_string(), "float b;".to_string()],
            "comments and blank lines are dropped and fields are not"
        );
        assert_ne!(
            struct_fields(original, "Thing"),
            struct_fields(renamed, "Thing")
        );
        assert!(struct_fields(original, "Absent").is_empty());
    }
}
