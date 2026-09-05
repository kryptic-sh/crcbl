//! The irradiance probe row and the grid header `mesh.slang` reads, in the
//! layouts that shader declares.
//!
//! Same argument as [`crate::mesh`]: `struct GpuProbe` and the probe members of
//! `struct FrameUniforms` fix a byte layout, every producer of those bytes has
//! to agree with it exactly, and keeping the layout in the crate that owns the
//! source means there is one place to change rather than one per consumer.
//!
//! # What a row holds, and why evaluating it is three dot products
//!
//! `docs/plan/18-render-features.md`'s irradiance-probe design: a static grid of
//! **L1 spherical-harmonic** probes, trilinearly interpolated, added to the flat
//! ambient term. Four coefficients per channel, packed so that the irradiance a
//! normal receives is `dot(sh, float4(N, 1))` — no `pow` and no trigonometry,
//! which is the rule `ggx_lobe` is already held to and the one that file's
//! determinism argument rests on.
//!
//! # The coefficients are irradiance, not radiance
//!
//! Ramamoorthi & Hanrahan 2001, *An Efficient Representation for Irradiance
//! Environment Maps*, is where the arithmetic comes from. Irradiance is the
//! incident radiance convolved with a clamped cosine,
//!
//! ```text
//! E(n) = ∫ L(ω) max(n·ω, 0) dω
//! ```
//!
//! and that convolution is a *per-band scale* in the spherical-harmonic basis:
//! `E_lm = Â_l L_lm`, with `Â_l = 2π ∫₀¹ P_l(t) t dt`. This module's
//! `TRANSFER_L0` and `TRANSFER_L1` are the first two, and the whole reason to
//! fold them into the stored value is that the shader then evaluates a dot
//! product rather than a convolution.
//!
//! So [`GpuProbe::accumulate`](crate::probe::GpuProbe::accumulate) is where an
//! author turns an environment into a
//! probe, and it is the only correct way to fill one: the band scales, the
//! basis normalisations and their product are exactly the transcription a
//! caller would get subtly wrong, and nothing else in this tree knows what the
//! right answer was. [`irradiance_at`](crate::probe::irradiance_at) is the Rust
//! mirror of what `mesh.slang` evaluates.
//!
//! # The grid is a clipmap, and this module owns where its levels stand
//!
//! `docs/plan/50-irradiance-probes.md`'s layered density:
//! [`ProbeVolume`](crate::probe::ProbeVolume) is a small number of concentric
//! levels of one grid, each twice the last. Two rules follow from that and both
//! are written here once:
//!
//! * **Where a level stands and how far apart its probes are** —
//!   [`ProbeVolume::level_origin`](crate::probe::ProbeVolume::level_origin) and
//!   [`ProbeVolume::level_inv_spacing`](crate::probe::ProbeVolume::level_inv_spacing).
//!   The shaders do not derive these: the rows go into the frame block and a
//!   fragment reads them, which is what `crcbl_render::probe_capture` already
//!   does with the octahedral direction table.
//! * **Which level a fragment reads and how much of it** —
//!   [`ProbeVolume::level_reach`](crate::probe::ProbeVolume::level_reach) and
//!   [`ProbeVolume::level_of`](crate::probe::ProbeVolume::level_of). This one a
//!   fragment must evaluate for itself, so `mesh.slang` and `ssr.slang` carry
//!   `probe_level_reach` and `probe_level_of` — the same body in both, held
//!   character for character by
//!   `the_shaders_pick_a_level_the_way_this_module_does`, and held to these on
//!   the device by `crcbl/tests/render_e2e.rs`'s
//!   `a_fragment_crossing_a_clipmap_level_fades_into_it`.
//!
//! # A zero volume adds exactly zero
//!
//! [`ProbeVolume::default`](crate::probe::ProbeVolume::default) is the grid a
//! scene with no probes uploads: no
//! probes on any axis, so every fetch resolves to row 0, and a row of zeroes
//! dots to zero whatever the normal. `x + 0 == x` exactly on every target, so a
//! frame drawn with no probes is bit-identical to one drawn before this module
//! existed — which is what lets the data path land with no golden re-blessed.

use crate::probe_visibility::ProbeVisibility;

/// Bytes per [`GpuProbe`], and the stride of the probe-table storage buffer.
///
/// Three `float4`s, no padding: one per colour channel. `std430` rounds a
/// struct's size up to its alignment, which is the `float4`'s 16, and this is
/// already a multiple of it — so unlike
/// [`MATERIAL_STRIDE`](crate::mesh::MATERIAL_STRIDE) there is no tail the row
/// does not use.
pub const PROBE_STRIDE: usize = 48;

const _: () = assert!(
    PROBE_STRIDE == 3 * 4 * size_of::<f32>(),
    "a probe row is one float4 per colour channel and nothing else"
);

const _: () = assert!(
    PROBE_STRIDE.is_multiple_of(16),
    "a std430 struct containing a float4 is 16-byte aligned, so its stride must \
     be a multiple of 16 or every row after the first lands short"
);

/// How many clipmap levels a [`ProbeVolume`] header carries room for.
///
/// `docs/plan/50-irradiance-probes.md`'s layered density: three or four levels,
/// each the same probe count, level `k` spaced `2^k` times level 0 and centred
/// on the same point — dense probes near the middle and sparse ones out at the
/// edge. Four is the top of the range that decision names, and it is the array
/// length the header reserves; [`ProbeVolume::level_count`] is what holds a
/// volume asking for more down to it.
pub const PROBE_LEVELS: usize = 4;

/// How much of a level's half-extent the blend into the next one occupies, as a
/// fraction of that half-extent.
///
/// **What decides it is continuity rather than taste.** The levels are
/// concentric and each is twice the last, so a point on level `k`'s own
/// boundary stands at exactly half of level `k+1`'s half-extent — and the read
/// there has to be *purely* level `k+1`, because that is what the fragment one
/// step further out computes. The band therefore has to be over by half an
/// extent, which is what the assertion below says.
///
/// `mesh.slang` and `ssr.slang` declare it as `PROBE_LEVEL_BAND`, and
/// `the_level_blend_constants_match_the_ones_the_shaders_declare` is what holds
/// the three in step.
pub const LEVEL_BAND: f32 = 0.25;

const _: () = assert!(
    LEVEL_BAND > 0.0 && LEVEL_BAND <= 0.5,
    "a band wider than half an extent is still fading level k when the fragment \
     beside it has already gone wholly to level k+1, which is the step this \
     blend exists to remove; a band of zero is that step outright"
);

/// Bytes the grid header occupies inside
/// [`FrameUniforms`](crate::mesh::FrameUniforms).
///
/// Two `uint4` — the counts and the level row — and then a `float4` origin, a
/// `float4` reciprocal spacing and a `uint4` scroll offset for each of
/// [`PROBE_LEVELS`]. `std140` gives each of them one sixteen-byte row, and the
/// total is already a multiple of sixteen, so there is no tail padding to write.
///
/// **The per-level rows are uploaded rather than derived in the shader**, which
/// is the whole of why they are here: where each level stands and how far apart
/// its probes are is one rule, [`ProbeVolume::level_origin`] and
/// [`ProbeVolume::level_inv_spacing`] are where it is written, and a fragment
/// reads the answer instead of recomputing it. `probe_capture`'s octahedral
/// direction table is the precedent. The scroll offset is there on the same
/// terms and one more: [`ProbeVolume::level_offset`] has already reduced it into
/// `0..count`, so the shader's wrap is one compare and one subtract rather than
/// a signed remainder.
pub const PROBE_VOLUME_SIZE: usize = 32 + 3 * 16 * PROBE_LEVELS;

/// The irradiance transfer coefficient of the constant band — Ramamoorthi &
/// Hanrahan 2001's `Â₀`, which is `π`.
///
/// `Â_l = 2π ∫₀¹ P_l(t) t dt`, the clamped cosine's zonal-harmonic expansion,
/// and `P₀(t) = 1` gives `2π · ½`. The checkable consequence is that a constant
/// environment of radiance `L` reaches a surface as an irradiance of `πL` from
/// every direction — this module's
/// `a_constant_environment_integrates_to_pi_times_its_radiance` is that
/// statement as a test.
///
/// [`GpuProbe::radiance`] divides the stored constant coefficient by this value
/// before evaluating a specular probe lookup.
pub const TRANSFER_L0: f32 = std::f32::consts::PI;

/// The same for the linear band — `Â₁`, which is `2π/3`.
///
/// `P₁(t) = t` gives `2π ∫₀¹ t² dt`. Its checkable consequence is that a
/// radiance field `L(ω) = b + c(ω·u)` — which L1 represents *exactly*, so no
/// truncation error stands between the two — reaches a surface as
/// `E(n) = πb + (2π/3)c(u·n)`.
///
/// [`GpuProbe::radiance`] divides the stored linear coefficients by this value
/// before evaluating a specular probe lookup.
pub const TRANSFER_L1: f32 = 2.0 * std::f32::consts::PI / 3.0;

/// What one sample of the environment adds to a probe's constant band.
///
/// `Â₀ · Y₀₀²`, and `Y₀₀ = ½√(1/π)` is constant over the sphere: projecting
/// onto the band and evaluating the band both carry a factor of `Y₀₀`, so what
/// survives into a stored *irradiance* coefficient is their product.
///
/// **Public because a shader has to declare it.**
/// `docs/plan/50-irradiance-probes.md`'s updater does the projection on the
/// device, so `shaders/probe_gather.slang` carries a copy of this number and
/// [`crate::probe_gather`]'s
/// `the_gather_projects_a_sample_the_way_this_module_does` is what holds the two
/// together. Nothing else in the tree knows what the right answer was.
pub const PROJECT_L0: f32 = TRANSFER_L0 / (4.0 * std::f32::consts::PI);

/// The same for the linear band: `Â₁ · (Y₁₁/x)²`, with
/// `Y₁₁ = ½√(3/π) · x` and its two siblings carrying the same normalisation on
/// `y` and `z`.
///
/// Public on [`PROJECT_L0`]'s terms exactly, and pinned to the shader by the
/// same test.
pub const PROJECT_L1: f32 = 3.0 * TRANSFER_L1 / (4.0 * std::f32::consts::PI);

/// One probe's irradiance, matching `struct GpuProbe` in `shaders/mesh.slang`.
///
/// Each channel is `(L1x, L1y, L1z, L0)` — the linear band first and the
/// constant band in `w` — so that evaluating the channel is
/// `dot(sh, float4(N, 1))` with no shuffle on either side.
///
/// `PartialEq` but not `Eq`, for [`GpuMaterial`](crate::mesh::GpuMaterial)'s
/// reason: every field is a float.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuProbe {
    /// The red channel's four coefficients.
    pub sh_r: [f32; 4],
    /// The green channel's.
    pub sh_g: [f32; 4],
    /// The blue channel's.
    pub sh_b: [f32; 4],
}

impl GpuProbe {
    /// The probe that adds nothing: every coefficient zero.
    ///
    /// Named rather than left to [`Default`] at the call sites that mean *this*
    /// specific thing — a row nobody has written, and the value the whole
    /// additive-zero property rests on.
    pub const ZERO: Self = Self {
        sh_r: [0.0; 4],
        sh_g: [0.0; 4],
        sh_b: [0.0; 4],
    };

    /// Adds one sample of the surrounding environment to this probe.
    ///
    /// `direction` is a **unit** vector pointing from the probe *towards* where
    /// the light comes from, `radiance` is the linear RGB radiance arriving
    /// along it, and `solid_angle` is how much of the sphere the sample stands
    /// for. Summing over a partition of the sphere is the projection: the whole
    /// environment integrated against the basis, with the clamped-cosine
    /// transfer already folded in.
    ///
    /// This is the only correct way to fill a row. The two weights are this
    /// module's `PROJECT_L0` and `PROJECT_L1`, and their derivation from the two
    /// transfer coefficients is the arithmetic a caller doing it by hand would
    /// get plausibly wrong — see the [module docs](self).
    pub fn accumulate(&mut self, direction: [f32; 3], radiance: [f32; 3], solid_angle: f32) {
        let constant = solid_angle * PROJECT_L0;
        let linear = solid_angle * PROJECT_L1;
        for (band, value) in [
            (&mut self.sh_r, radiance[0]),
            (&mut self.sh_g, radiance[1]),
            (&mut self.sh_b, radiance[2]),
        ] {
            for axis in 0..3 {
                band[axis] += linear * value * direction[axis];
            }
            band[3] += constant * value;
        }
    }

    /// The irradiance this probe gives a surface facing `normal`, in linear
    /// RGB.
    ///
    /// The three dot products the packing exists for, clamped at zero. **The
    /// clamp is not defensive**: L1 is a truncation, and a truncated series
    /// rings — an environment concentrated in one direction evaluates negative
    /// on the far side, which is a surface that would subtract light. The
    /// literature's alternative is [an ambient
    /// cube](https://web.archive.org/web/20200417075719/https://steamcdn-a.akamaihd.net/apps/valve/2004/GDC2004_Half-Life2_Shading.pdf),
    /// which never rings and costs six coefficients per channel to L1's four;
    /// `docs/plan/18-render-features.md` records it as the drop-in if one
    /// `max` ever stops being enough.
    #[must_use]
    pub fn irradiance(&self, normal: [f32; 3]) -> [f32; 3] {
        let channel = |sh: &[f32; 4]| {
            (sh[0] * normal[0] + sh[1] * normal[1] + sh[2] * normal[2] + sh[3]).max(0.0)
        };
        [
            channel(&self.sh_r),
            channel(&self.sh_g),
            channel(&self.sh_b),
        ]
    }

    /// The approximate L1 radiance this irradiance probe represents in
    /// `direction`, in linear RGB.
    ///
    /// [`Self::accumulate`] stores irradiance coefficients after the clamped
    /// cosine transfer. Specular reflection needs incident radiance instead, so
    /// undo that transfer per band before evaluating the same L1 basis: `xyz` is
    /// divided by [`TRANSFER_L1`] and `w` by [`TRANSFER_L0`]. Like
    /// [`Self::irradiance`], this clamps L1 ringing rather than allowing a
    /// reflection to subtract light.
    #[must_use]
    pub fn radiance(&self, direction: [f32; 3]) -> [f32; 3] {
        let channel = |sh: &[f32; 4]| {
            (sh[0] / TRANSFER_L1 * direction[0]
                + sh[1] / TRANSFER_L1 * direction[1]
                + sh[2] / TRANSFER_L1 * direction[2]
                + sh[3] / TRANSFER_L0)
                .max(0.0)
        };
        [
            channel(&self.sh_r),
            channel(&self.sh_g),
            channel(&self.sh_b),
        ]
    }

    /// The bytes one probe-table element holds, in `std430` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PROBE_STRIDE] {
        let mut bytes = [0u8; PROBE_STRIDE];
        let mut at = 0usize;
        for value in self.sh_r.iter().chain(&self.sh_g).chain(&self.sh_b) {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        debug_assert_eq!(at, PROBE_STRIDE);
        bytes
    }

    /// The inverse of [`GpuProbe::to_bytes`].
    ///
    /// So a test can decode what the table actually holds rather than trusting
    /// a host-side copy of it, which is why
    /// [`GpuMaterial::from_bytes`](crate::mesh::GpuMaterial::from_bytes) exists
    /// too.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; PROBE_STRIDE]) -> Self {
        let float_at = |offset: usize| {
            f32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("four bytes of a fixed-size array")),
            )
        };
        let band = |base: usize| {
            [
                float_at(base),
                float_at(base + 4),
                float_at(base + 8),
                float_at(base + 12),
            ]
        };
        Self {
            sh_r: band(0),
            sh_g: band(16),
            sh_b: band(32),
        }
    }

    /// This probe blended towards `other` by `t`, coefficient by coefficient.
    ///
    /// Spelled as HLSL's `lerp` is defined — `a + t(b - a)` — so that the Rust
    /// mirror and the shader agree at the one place the design's determinism
    /// argument needs them to: `t == 0.0` returns `self` **exactly**, which is
    /// what makes the far corner of a grid cell weigh nothing at the boundary
    /// where the cell index changes.
    fn lerp(self, other: Self, t: f32) -> Self {
        let band = |a: [f32; 4], b: [f32; 4]| {
            let mut out = [0.0f32; 4];
            for (index, slot) in out.iter_mut().enumerate() {
                *slot = a[index] + t * (b[index] - a[index]);
            }
            out
        };
        Self {
            sh_r: band(self.sh_r, other.sh_r),
            sh_g: band(self.sh_g, other.sh_g),
            sh_b: band(self.sh_b, other.sh_b),
        }
    }
}

/// One axis of the toroidal wrap: cell `cell` of a level whose scroll offset on
/// this axis is `offset` stands in row `cell + offset`, brought back inside
/// `0..count` — **the mirror of `probe_wrap` in `shaders/mesh.slang`**.
///
/// One compare and one subtract rather than a remainder, and that is exact
/// rather than cheap: [`ProbeVolume::level_offset`] has already reduced the
/// offset below the count and a cell is below it by construction, so the sum is
/// under twice the count and one subtraction is the whole modulo. An axis with
/// no probes has a count of zero, a cell of zero and an offset of zero, and
/// answers zero.
fn wrap(cell: u32, offset: u32, count: u32) -> u32 {
    let at = cell.saturating_add(offset);
    if at >= count { at - count } else { at }
}

/// How many whole probe steps each clipmap level has scrolled, on each axis —
/// [`ProbeVolume::steps`]'s type.
///
/// One row per level the header has room for rather than per level a volume
/// actually has, so the array is the same shape whatever
/// [`ProbeVolume::level_count`] answers and a level that gained a row does not
/// change this type. The rows past the live levels are read by nothing.
pub type ProbeSteps = [[i32; 3]; PROBE_LEVELS];

/// The clipmap the probes are laid out on, matching the `probe_*` members of
/// `struct FrameUniforms` in `shaders/mesh.slang`.
///
/// A uniform grid rather than a 3D texture, and the reason is in
/// `docs/plan/18-render-features.md`: hardware trilinear filter weights are
/// vendor tables, which is the exact class of filtered read the occlusion and
/// reflection designs spent their determinism arguments avoiding. An eight-tap
/// manual interpolation over a storage buffer costs less and risks nothing.
///
/// # Several levels of it, concentric, each twice the last
///
/// `docs/plan/50-irradiance-probes.md`'s layered density. One uniform grid over
/// a whole scene is either too coarse near the middle or too large away from
/// it, so the volume is [`levels`](Self::levels) of the same grid: the same
/// [`counts`](Self::counts) every level, level `k` spaced `2^k` times level 0,
/// and every level centred on level 0's own centre. A fragment reads the finest
/// level that contains it, blended over a band at that level's edge — see
/// [`level_of`](Self::level_of), which is the rule and the mirror of what the
/// shaders evaluate.
///
/// **The rows stay one buffer.** `docs/plan/43-render-standards.md`'s §5 C1 is
/// that there is one storage buffer, so a level is a *range* of it —
/// [`level_row`](Self::level_row) is where each begins, and the visibility image
/// keeps one layer per row across every level, so a capture covers the whole
/// clipmap in the call it always made.
///
/// # It scrolls, and a scroll moves as few rows as it can
///
/// A level re-centres on a tracked point by whole probe steps —
/// [`follow`](Self::follow) — and the addressing wraps around it rather than
/// moving anything: [`row`](Self::row) adds that level's
/// [`level_offset`](Self::level_offset) to a cell and brings it back inside the
/// counts, so a level that stepped `k` probes along an axis has `k` slabs
/// holding a probe that is somewhere new and the rest still naming the probes
/// they held. [`exposed`](Self::exposed) is which, and it is what
/// `crcbl_render::probe_capture` re-captures.
///
/// # One level is the uniform grid this used to be
///
/// A volume of one level has nothing coarser to blend towards, so it clamps at
/// its edge exactly as the single grid did, its rows start at 0, and its
/// spacing is the one it was given. Nothing about a one-level read differs from
/// what this type evaluated before the clipmap existed, which is what let the
/// levels land with no golden re-blessed.
///
/// [`Default`] is the **degenerate volume**: no probes on any axis, which
/// resolves every fetch to row 0 and — with that row zeroed — adds exactly
/// nothing. It is what a scene with no probes uploads, and there is no branch
/// anywhere selecting it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProbeVolume {
    /// Where probe `(0, 0, 0)` of **level 0**, the finest, would be in world
    /// space with the volume where it was authored — see
    /// [`steps`](Self::steps), which moves it.
    pub origin: [f32; 3],
    /// One over **level 0**'s grid spacing on each axis, in world units.
    ///
    /// **Inverted host-side** so the shader multiplies where it would otherwise
    /// divide, once per fragment. An axis whose spacing is zero has no
    /// reciprocal and the honest spelling of it is this field's zero, which is
    /// what [`Default`] leaves and what collapses the grid onto probe 0.
    pub inv_spacing: [f32; 3],
    /// How many probes each level holds on each axis. **Every level holds the
    /// same count** — that is what makes a coarser level cover more world
    /// rather than hold more probes.
    ///
    /// A count of zero and a count of one behave alike — both address probe 0
    /// and interpolate nothing — because the last addressable probe on an axis
    /// is `max(count - 1, 0)` either way.
    pub counts: [u32; 3],
    /// How many clipmap levels the volume has.
    ///
    /// Zero and one behave alike, on [`counts`](Self::counts)' terms: both are
    /// a single grid that clamps at its own edge, so a caller that has one grid
    /// and nothing to say about levels can leave this at [`Default`]'s zero.
    /// More than [`PROBE_LEVELS`] is held to [`PROBE_LEVELS`] rather than
    /// reserving rows the header has no room to name —
    /// [`level_count`](Self::level_count) is the one place that is decided.
    pub levels: u32,
    /// How many **whole probe steps** each level has scrolled from where
    /// [`origin`](Self::origin) authored it, on each axis.
    ///
    /// `docs/plan/50-irradiance-probes.md`'s scrolling: a level re-centres on a
    /// tracked point by moving a whole number of its own probe spacings, so the
    /// probes that stay inside it stand at exactly the world positions they
    /// stood at before — the arithmetic is `origin + step · spacing` and a step
    /// is an integer, so nothing rounds. [`level_origin`](Self::level_origin) is
    /// where it moves the level and [`row`](Self::row) is where it wraps the
    /// rows around it.
    ///
    /// **Whole steps are what make it toroidal.** A level that moved by a
    /// fraction of a spacing would put every probe somewhere new and invalidate
    /// the whole level; moving by `k` steps leaves `count - k` of them exactly
    /// where they were, addressable at the rows they already occupy.
    ///
    /// [`Default`]'s zeroes are the volume that has never scrolled, and its
    /// every row is where it was before this field existed.
    pub steps: ProbeSteps,
}

impl ProbeVolume {
    /// How many levels the volume actually has: at least one, and never more
    /// than the header has rows for.
    ///
    /// **Not a field**, so it cannot disagree with [`levels`](Self::levels),
    /// and it is what every other method here counts in.
    #[must_use]
    pub fn level_count(&self) -> u32 {
        self.levels.clamp(1, PROBE_LEVELS as u32)
    }

    /// How many probes **one** level holds: the product of its counts,
    /// saturating.
    #[must_use]
    pub fn per_level(&self) -> u32 {
        self.counts[0]
            .saturating_mul(self.counts[1])
            .saturating_mul(self.counts[2])
    }

    /// How many probes the whole clipmap holds: one level's worth per level,
    /// saturating.
    ///
    /// **Not a field**, so it cannot disagree with [`counts`](Self::counts) and
    /// [`levels`](Self::levels). It is written into the header's spare `uint`
    /// lane all the same, because the shader clamps every fetch against it —
    /// the second line of defence that makes a row read a fact about the buffer
    /// rather than a promise about the host.
    #[must_use]
    pub fn total(&self) -> u32 {
        self.per_level().saturating_mul(self.level_count())
    }

    /// The first row `level` owns in the one probe table.
    ///
    /// The levels are laid end to end in the order they are numbered, finest
    /// first, so this is the level's index times [`per_level`](Self::per_level)
    /// — and it is also the first *layer* of the visibility image that level
    /// owns, because `crcbl_render::probe_capture` writes its layers in exactly
    /// this order.
    #[must_use]
    pub fn level_row(&self, level: u32) -> u32 {
        level
            .min(self.level_count() - 1)
            .saturating_mul(self.per_level())
    }

    /// One over `level`'s spacing on each axis.
    ///
    /// Level `k` is spaced `2^k` times level 0, so this is level 0's reciprocal
    /// halved `k` times — **exact**, because halving a float is exact, and an
    /// axis with no reciprocal keeps its zero and stays collapsed on every
    /// level.
    #[must_use]
    pub fn level_inv_spacing(&self, level: u32) -> [f32; 3] {
        let scale = (-(level.min(self.level_count() - 1) as f32)).exp2();
        self.inv_spacing.map(|inv| inv * scale)
    }

    /// `level`'s spacing on each axis, in world units.
    ///
    /// The reciprocal of [`level_inv_spacing`](Self::level_inv_spacing), and an
    /// axis with no reciprocal has no spacing to step along either — the zero
    /// that collapses a grid onto probe 0.
    #[must_use]
    pub fn level_spacing(&self, level: u32) -> [f32; 3] {
        self.level_inv_spacing(level)
            .map(|inv| if inv == 0.0 { 0.0 } else { 1.0 / inv })
    }

    /// How many whole probe steps `level` has scrolled on each axis —
    /// [`steps`](Self::steps)' row for it, held to the levels the volume has.
    #[must_use]
    pub fn level_steps(&self, level: u32) -> [i32; 3] {
        self.steps[level.min(self.level_count() - 1) as usize]
    }

    /// The same steps reduced into `0..count`, which is what the header carries
    /// and what [`row`](Self::row) wraps a cell by.
    ///
    /// **Reduced here rather than in the shader**, because `(c + s) mod n` is
    /// `(c + (s mod n)) mod n` and a cell is already inside `0..count`: with the
    /// offset reduced too, the sum is under `2·count` and the wrap the shader
    /// runs is one compare and one subtract instead of a signed remainder. An
    /// axis with no probes has nothing to reduce against and answers zero.
    #[must_use]
    pub fn level_offset(&self, level: u32) -> [u32; 3] {
        let steps = self.level_steps(level);
        let mut offset = [0u32; 3];
        for axis in 0..3 {
            let count = self.counts[axis];
            if count == 0 {
                continue;
            }
            // `rem_euclid` rather than `%`, because a level that scrolled
            // backwards has a negative step and a negative row is not a row.
            offset[axis] = (steps[axis].rem_euclid(count as i32)) as u32;
        }
        offset
    }

    /// Which row of the probe table the probe at cell `cell` of `level` stands
    /// in — **the host mirror of `probe_row` in `shaders/mesh.slang`**.
    ///
    /// The level's own range of the table, and within it the cell wrapped by
    /// [`level_offset`](Self::level_offset): that wrap is the whole of the
    /// toroidal addressing. A level that has scrolled `k` steps along an axis
    /// has moved `k` of its rows to the far side and left the others naming the
    /// probes they already held, so a scroll rewrites `k` slabs rather than the
    /// level — see [`exposed`](Self::exposed), which is that rule.
    ///
    /// The row is **not** clamped against [`total`](Self::total) here; the
    /// shader clamps its own fetch, and a caller filling the table wants to know
    /// that it asked about a row that is not there rather than to be handed the
    /// last one.
    #[must_use]
    pub fn row(&self, level: u32, cell: [u32; 3]) -> u32 {
        let offset = self.level_offset(level);
        let mut wrapped = [0u32; 3];
        for axis in 0..3 {
            wrapped[axis] = wrap(cell[axis], offset[axis], self.counts[axis]);
        }
        self.level_row(level).saturating_add(
            (wrapped[2] * self.counts[1] + wrapped[1]) * self.counts[0] + wrapped[0],
        )
    }

    /// Where probe `(0, 0, 0)` of `level` is, in world space.
    ///
    /// Every level is centred on level 0's centre and level `k` spans `2^k`
    /// times as much of the world, so a coarser level's first probe stands
    /// further out by half of the extent it gained — and then
    /// [`steps`](Self::steps) moves the whole level by a whole number of its own
    /// spacings.
    ///
    /// **Written as an offset from [`origin`](Self::origin) rather than as a
    /// centre minus a half-extent**, so that an unscrolled level 0 returns that
    /// field *exactly*: both offsets it adds are a multiplication by zero, where
    /// naming the centre and subtracting it back would round twice. That
    /// exactness is what makes a one-level volume the grid it was.
    #[must_use]
    pub fn level_origin(&self, level: u32) -> [f32; 3] {
        let level = level.min(self.level_count() - 1);
        // `1 - 2^k`: the extent this level gained over level 0, as a multiple
        // of level 0's, and negative because a wider level starts further back.
        let gained = 1.0 - (level as f32).exp2();
        let spacing = self.level_spacing(0);
        let scrolled = self.level_spacing(level);
        let steps = self.level_steps(level);
        let mut at = [0.0f32; 3];
        for axis in 0..3 {
            let last = self.counts[axis].saturating_sub(1) as f32;
            at[axis] = self.origin[axis]
                + 0.5 * last * spacing[axis] * gained
                + steps[axis] as f32 * scrolled[axis];
        }
        at
    }

    /// The point every level is centred on when nothing has scrolled — the
    /// place [`steps_towards`](Self::steps_towards) measures a tracked point
    /// against.
    ///
    /// One point for the whole clipmap rather than one per level: a level's
    /// authored origin stands back by half the extent it gained and its own
    /// half-extent brings it forward by exactly as much, so every level's
    /// authored centre is this.
    #[must_use]
    pub fn centre(&self) -> [f32; 3] {
        let spacing = self.level_spacing(0);
        let mut at = [0.0f32; 3];
        for axis in 0..3 {
            let last = self.counts[axis].saturating_sub(1) as f32;
            at[axis] = self.origin[axis] + 0.5 * last * spacing[axis];
        }
        at
    }

    /// The [`steps`](Self::steps) that put every level's centre as near
    /// `point` as a whole probe step of that level allows.
    ///
    /// **The nearest whole step, not the one that merely contains the point.**
    /// Rounding is what makes the follow settle: a rule that stepped only when
    /// the point left the level would leave it hard against one face, and the
    /// next step back would be a scroll for a hand's width of movement.
    ///
    /// An axis a level cannot step along — no probes on it, or no spacing —
    /// keeps its zero rather than dividing by nothing, which is the same axis
    /// [`level_reach`](Self::level_reach) declines to measure. A `point` that is
    /// not finite steps nowhere, for the same reason: a `NaN` compared against a
    /// bound is neither inside nor outside it.
    #[must_use]
    pub fn steps_towards(&self, point: [f32; 3]) -> ProbeSteps {
        let centre = self.centre();
        let mut steps = ProbeSteps::default();
        for level in 0..self.level_count() {
            let spacing = self.level_spacing(level);
            for axis in 0..3 {
                if self.counts[axis] == 0 || spacing[axis] == 0.0 {
                    continue;
                }
                let want = (point[axis] - centre[axis]) / spacing[axis];
                if !want.is_finite() {
                    continue;
                }
                // `as` saturates at the ends of `i32`, which is the honest
                // answer for a point a billion spacings away: the level is
                // wholly new either way, and `exposed` says so.
                steps[level as usize][axis] = want.round() as i32;
            }
        }
        steps
    }

    /// The rows a move from this volume's [`steps`](Self::steps) to `steps`
    /// leaves holding a probe that is not where that row's probe used to be.
    ///
    /// **The invalidation rule, and the one place it is written.** A level that
    /// steps `k` probes along one axis covers `k` world positions it did not
    /// cover before; each of them wraps onto a row that held the probe `count`
    /// steps behind it, so exactly `k` **slabs** — `k · counts.y · counts.z`
    /// rows for a step along `x` — carry a probe whose visibility map is about
    /// somewhere else, and the other `count - k` slabs are addressable at the
    /// rows they already had. A step on more than one axis exposes the union of
    /// its axes' slabs, because a probe is new if any one of its axes is.
    ///
    /// A step of `count` or more along an axis exposes that level whole, which
    /// is the same arithmetic rather than a special case: no world index of the
    /// new box is in the old one.
    ///
    /// The rows come back in the new box's own cell order, level by level, and
    /// no row appears twice — a cell is one row.
    #[must_use]
    pub fn exposed(&self, steps: &ProbeSteps) -> Vec<u32> {
        let mut rows = Vec::new();
        let moved = Self {
            steps: *steps,
            ..*self
        };
        for level in 0..self.level_count() {
            let before = self.level_steps(level);
            let after = moved.level_steps(level);
            let mut delta = [0i32; 3];
            for axis in 0..3 {
                delta[axis] = after[axis].saturating_sub(before[axis]);
            }
            if delta == [0, 0, 0] {
                continue;
            }
            for z in 0..self.counts[2] {
                for y in 0..self.counts[1] {
                    for x in 0..self.counts[0] {
                        let cell = [x, y, z];
                        // The cell this probe would have been in before the
                        // move. Outside the old box on any axis is a probe the
                        // level did not have, whatever the other two say.
                        let kept = (0..3).all(|axis| {
                            let was = i64::from(cell[axis]) + i64::from(delta[axis]);
                            was >= 0 && was < i64::from(self.counts[axis])
                        });
                        if !kept {
                            rows.push(moved.row(level, cell));
                        }
                    }
                }
            }
        }
        rows
    }

    /// Re-centres every level on `point` by whole probe steps, and answers the
    /// rows that move — [`steps_towards`](Self::steps_towards) and
    /// [`exposed`](Self::exposed) applied together.
    ///
    /// An empty answer is a volume that did not move, which is the ordinary
    /// frame: the tracked point has to travel half a probe spacing before the
    /// nearest whole step changes at all.
    ///
    /// **It moves the header and nothing else.** The rows the answer names hold
    /// a visibility map about where their probe used to stand until something
    /// captures them again; `crcbl_render::probe_capture` is what does that, and
    /// this returning the list is what tells it which.
    pub fn follow(&mut self, point: [f32; 3]) -> Vec<u32> {
        let steps = self.steps_towards(point);
        let rows = self.exposed(&steps);
        self.steps = steps;
        rows
    }

    /// Where probe `cell` of `level` stands, in world space.
    ///
    /// The grid's own arithmetic run backwards: the header carries the
    /// *reciprocal* spacing, so this multiplies by its reciprocal in turn. An
    /// axis whose reciprocal is zero has no spacing to step along and every
    /// probe on it stands at the origin's coordinate — which is what the
    /// degenerate volume a scene with no probes uploads collapses to, and what
    /// `probe_irradiance` in `shaders/mesh.slang` computes for the same cell.
    ///
    /// It is what the visibility test needs and the irradiance lookup never
    /// did: a Chebyshev bound is about the distance from a *probe* to a
    /// surface, where a trilinear blend only ever needed the fraction between
    /// two of them.
    #[must_use]
    pub fn position(&self, level: u32, cell: [u32; 3]) -> [f32; 3] {
        let origin = self.level_origin(level);
        let spacing = self.level_spacing(level);
        let mut at = [0.0f32; 3];
        for axis in 0..3 {
            at[axis] = origin[axis] + cell[axis] as f32 * spacing[axis];
        }
        at
    }

    /// How far out of **level 0** `world_position` stands, measured in that
    /// level's own half-extents: zero at the centre, one on its boundary.
    ///
    /// The Chebyshev norm of the point in the level's normalised coordinates,
    /// which is the shape that makes the whole clipmap one number: every level
    /// is concentric and twice the last, so level `k`'s reach is this one
    /// halved `k` times and [`level_of`](Self::level_of) needs no second
    /// derivation. `mesh.slang` and `ssr.slang` evaluate it as
    /// `probe_level_reach`, off the level-0 rows this type uploads.
    ///
    /// **An axis with no extent is contained everywhere.** A grid one probe
    /// deep, or one whose reciprocal spacing is zero, has no boundary on that
    /// axis to be outside of — it clamps onto probe 0 for every point — so it
    /// contributes nothing here rather than dividing by its zero extent.
    #[must_use]
    pub fn level_reach(&self, world_position: [f32; 3]) -> f32 {
        // Level 0's own rows rather than the fields behind them, so this reads
        // exactly what the header hands the shaders.
        let origin = self.level_origin(0);
        let inv_spacing = self.level_inv_spacing(0);
        let mut reach = 0.0f32;
        for axis in 0..3 {
            let last = self.counts[axis].saturating_sub(1) as f32;
            if last == 0.0 || inv_spacing[axis] == 0.0 {
                continue;
            }
            let grid = (world_position[axis] - origin[axis]) * inv_spacing[axis];
            reach = reach.max((2.0 * grid / last - 1.0).abs());
        }
        reach
    }

    /// Which level a point of [`level_reach`](Self::level_reach) `reach` reads,
    /// and what share of the read belongs to that level rather than to the next
    /// one out.
    ///
    /// **The clipmap's selection rule, and the one place it is written.**
    /// `mesh.slang` and `ssr.slang` carry it as `probe_level_of`, character for
    /// character the same body in both, and
    /// `the_shaders_pick_a_level_the_way_this_module_does` is what holds them
    /// to this.
    ///
    /// The finest level whose reach is inside one is the level, and the share
    /// ramps from zero on that level's boundary to one [`LEVEL_BAND`] of an
    /// extent inside it. The coarsest level has nothing to blend towards and
    /// takes the whole share, which is the clamp-at-the-edge the single grid
    /// always did.
    ///
    /// # Why it does not step
    ///
    /// On level `k`'s boundary the share is exactly zero, so the read there is
    /// *wholly* level `k+1`. A point a hair further out picks level `k+1`,
    /// whose own reach is half of one — and [`LEVEL_BAND`]'s assertion is that
    /// half an extent is past the band, so its share is exactly one and the
    /// read is wholly level `k+1` again. The two sides of the switch compute
    /// the same value, which is the same shape of argument the trilinear cell
    /// index already rests on.
    #[must_use]
    pub fn level_of(&self, reach: f32) -> (u32, f32) {
        let levels = self.level_count();
        for level in 0..levels - 1 {
            let at = reach * (-(level as f32)).exp2();
            if at < 1.0 {
                return (level, ((1.0 - at) / LEVEL_BAND).clamp(0.0, 1.0));
            }
        }
        (levels - 1, 1.0)
    }

    /// The bytes the grid header occupies in the frame block, in `std140`
    /// order.
    ///
    /// The counts and their total, the level count and the rows one level
    /// holds, and then an origin, a reciprocal spacing and a scroll offset per
    /// level — every three-component row leaving its fourth lane as the zero the
    /// block starts as. The rows past [`level_count`](Self::level_count) stay
    /// zeroed; the shader clamps its level against the count and never reads
    /// them.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PROBE_VOLUME_SIZE] {
        let mut bytes = [0u8; PROBE_VOLUME_SIZE];
        let mut at = 0usize;
        let mut put = |value: u32, at: &mut usize| {
            bytes[*at..*at + 4].copy_from_slice(&value.to_le_bytes());
            *at += 4;
        };
        for value in self.counts.into_iter().chain([self.total()]) {
            put(value, &mut at);
        }
        // The ceiling but not the floor, so a volume nobody said anything about
        // levels to writes zero here and the degenerate volume stays a block of
        // zeroes. The shader takes the floor itself, exactly as it does with
        // `max(total, 1)` — one and none are the same single grid on both
        // sides, which is [`level_count`](Self::level_count).
        for value in [self.levels.min(PROBE_LEVELS as u32), self.per_level(), 0, 0] {
            put(value, &mut at);
        }
        for level in 0..PROBE_LEVELS as u32 {
            let live = level < self.level_count();
            let row = if live {
                self.level_origin(level)
            } else {
                [0.0; 3]
            };
            for value in row {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
            // The lane `std140` aligns a three-component vector to sixteen
            // bytes with, and which nothing reads.
            at += 4;
        }
        for level in 0..PROBE_LEVELS as u32 {
            let live = level < self.level_count();
            let row = if live {
                self.level_inv_spacing(level)
            } else {
                [0.0; 3]
            };
            for value in row {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
            at += 4;
        }
        // The scroll offsets, already reduced into `0..count` — see
        // [`level_offset`](Self::level_offset), which is why the shader's wrap
        // is a compare and a subtract.
        for level in 0..PROBE_LEVELS as u32 {
            let live = level < self.level_count();
            let row = if live {
                self.level_offset(level)
            } else {
                [0; 3]
            };
            for value in row {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
            at += 4;
        }
        debug_assert_eq!(at, PROBE_VOLUME_SIZE);
        bytes
    }
}

/// One corner of the trilinear gather: its coefficients already scaled by the
/// weight it keeps, and that weight beside them.
///
/// **Both halves are blended through the same seven `lerp`s**, so what comes out
/// the far end is `Σ trilinearᵢ · visibilityᵢ · coefficientsᵢ` over the eight
/// corners and `Σ trilinearᵢ · visibilityᵢ` — a weighted mean waiting for its
/// divisor. Evaluating the basis once on that mean is what keeps the whole
/// lookup three dot products whatever the grid's size, and it is the more
/// correct of the two orders for [`irradiance_at`]'s own reason: irradiance is
/// linear in the coefficients until the clamp.
#[derive(Clone, Copy)]
struct WeightedProbe {
    /// The row, every coefficient already multiplied by [`Self::weight`].
    sh: GpuProbe,
    /// What this corner counts for.
    weight: f32,
}

impl WeightedProbe {
    /// Blended towards `other` by `t`, on [`GpuProbe::lerp`]'s terms and in the
    /// same spelling.
    fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            sh: self.sh.lerp(other.sh, t),
            weight: self.weight + t * (other.weight - self.weight),
        }
    }
}

/// The irradiance **one level** of the volume gives a surface at
/// `world_position` facing `normal`, in linear RGB — the Rust mirror of
/// `probe_level_irradiance` in `shaders/mesh.slang`.
///
/// Trilinear over the eight probes of that level surrounding the point, each
/// corner's weight scaled by how much of that probe `visibility` says the
/// surface can see, and the interpolation is over the *coefficients* rather
/// than over eight evaluations of them: that is what makes the whole lookup
/// three dot products, seven blends and one clamp, whatever the grid's size.
///
/// A point outside the level clamps to its surface rather than fading out,
/// which is the same choice the shadow atlas's sampler makes at a cascade's
/// edge: an environment does not stop at the last probe, and fading to zero
/// there would put a visible seam in mid-air. [`irradiance_at`] is what makes
/// that clamp invisible where a finer level ends and a coarser one goes on.
///
/// `probes` is the table in `x`-fastest order within a level and finest level
/// first, each level's cells wrapped by its own scroll offset —
/// [`ProbeVolume::row`] is the index. A row
/// past its end reads as [`GpuProbe::ZERO`], which is what the device sees too:
/// the table is created with at least one zeroed row, so the degenerate
/// volume's fetch of row 0 is in bounds there and a scene whose probe count
/// disagrees with its volume is refused before it is uploaded at all.
///
/// # The divisor cannot be zero, and every corner occluded is not black
///
/// [`ProbeVisibility::weight`] never returns less than
/// [`OCCLUDED_WEIGHT`](crate::probe_visibility::OCCLUDED_WEIGHT), so the summed
/// weight cannot reach zero and a fragment whose every corner is occluded gets
/// that constant divided straight back out — the plain trilinear result, which
/// is the light such a fragment had before the visibility test existed. A
/// fragment inside geometry keeps looking like the surface around it rather
/// than becoming a hole.
///
/// # A grid of nothing is still exactly zero
///
/// A scene with no probes reads row 0, which is zeroed, so every corner's
/// coefficients are zero, the weighted sum is zero, and zero divided by a
/// positive weight is zero **exactly** — before the clamp and after it. That is
/// what lets the visibility test land with no golden of a scene without probes
/// re-blessed, exactly as the grid itself did.
#[must_use]
pub fn level_irradiance_at(
    volume: &ProbeVolume,
    probes: &[GpuProbe],
    visibility: &ProbeVisibility,
    level: u32,
    world_position: [f32; 3],
    normal: [f32; 3],
) -> [f32; 3] {
    // The last addressable probe on each axis, which is zero for an axis with
    // no probes at all — so the degenerate volume collapses onto probe 0
    // instead of addressing probe -1.
    let last = volume.counts.map(|count| count.saturating_sub(1) as f32);
    // Where this level stands and how far apart its probes are, taken from the
    // volume rather than derived here — the same rows the header uploads, so
    // the shader is reading the answer this is computing.
    let origin = volume.level_origin(level);
    let inv_spacing = volume.level_inv_spacing(level);
    let mut cell = [0u32; 3];
    let mut fraction = [0.0f32; 3];
    for axis in 0..3 {
        let grid =
            ((world_position[axis] - origin[axis]) * inv_spacing[axis]).clamp(0.0, last[axis]);
        let base = grid.floor();
        cell[axis] = base as u32;
        fraction[axis] = grid - base;
    }

    let corner = |x: u32, y: u32, z: u32| -> WeightedProbe {
        // `ProbeVolume::row` is the addressing — the level's range of the table
        // and the cell wrapped by that level's scroll offset — and the clamp
        // after it is the shader's own, which makes the fetch a fact about the
        // table rather than a promise about the header.
        let bound = volume.total().saturating_sub(1);
        let row = volume.row(level, [x, y, z]).min(bound);
        let sh = probes.get(row as usize).copied().unwrap_or(GpuProbe::ZERO);
        let weight = visibility.weight(
            row,
            volume.position(level, [x, y, z]),
            world_position,
            normal,
        );
        let scale = |band: [f32; 4]| band.map(|value| value * weight);
        WeightedProbe {
            sh: GpuProbe {
                sh_r: scale(sh.sh_r),
                sh_g: scale(sh.sh_g),
                sh_b: scale(sh.sh_b),
            },
            weight,
        }
    };
    // `min` against the axis's last probe rather than `+ 1` outright: at the
    // far face of the grid there is no next probe, and the fraction there is
    // zero, so the corner is weighed out rather than wrapped around.
    let next = [
        (cell[0] + 1).min(last[0] as u32),
        (cell[1] + 1).min(last[1] as u32),
        (cell[2] + 1).min(last[2] as u32),
    ];

    let x0 = corner(cell[0], cell[1], cell[2]).lerp(corner(next[0], cell[1], cell[2]), fraction[0]);
    let x1 = corner(cell[0], next[1], cell[2]).lerp(corner(next[0], next[1], cell[2]), fraction[0]);
    let x2 = corner(cell[0], cell[1], next[2]).lerp(corner(next[0], cell[1], next[2]), fraction[0]);
    let x3 = corner(cell[0], next[1], next[2]).lerp(corner(next[0], next[1], next[2]), fraction[0]);
    let y0 = x0.lerp(x1, fraction[1]);
    let y1 = x2.lerp(x3, fraction[1]);
    let blended = y0.lerp(y1, fraction[2]);

    let channel = |sh: &[f32; 4]| {
        ((sh[0] * normal[0] + sh[1] * normal[1] + sh[2] * normal[2] + sh[3]) / blended.weight)
            .max(0.0)
    };
    [
        channel(&blended.sh.sh_r),
        channel(&blended.sh.sh_g),
        channel(&blended.sh.sh_b),
    ]
}

/// The irradiance the whole clipmap gives a surface at `world_position` facing
/// `normal`, in linear RGB — **the Rust mirror of `probe_irradiance` in
/// `shaders/mesh.slang`**.
///
/// The finest level that contains the point, faded into the next one out over
/// a band at its edge: [`ProbeVolume::level_reach`] and
/// [`ProbeVolume::level_of`] are the rule, [`level_irradiance_at`] is the read
/// within a level, and this is the two of them put together.
///
/// # A one-level volume is exactly one gather
///
/// The coarsest level takes the whole share, so a volume of one level returns
/// [`level_irradiance_at`] of level 0 and evaluates nothing else — which is
/// the arithmetic this function was before the clipmap, unchanged and in the
/// same order. That is what lets the levels land with no golden re-blessed.
///
/// # Two terms rather than a `lerp`, for the reason the reflection design gives
///
/// `coarse · (1 − share) + fine · share` returns `fine` *exactly* at a share of
/// one and `coarse` exactly at zero, where `lerp` would return
/// `coarse + (fine − coarse)` and lose the low bits of either end. That
/// exactness is what makes the switch at a level's boundary meet: the fragment
/// inside reads a share of zero and gets the coarse level whole, and the
/// fragment outside picks that coarse level with a share of one and gets it
/// whole too.
#[must_use]
pub fn irradiance_at(
    volume: &ProbeVolume,
    probes: &[GpuProbe],
    visibility: &ProbeVisibility,
    world_position: [f32; 3],
    normal: [f32; 3],
) -> [f32; 3] {
    let (level, share) = volume.level_of(volume.level_reach(world_position));
    let fine = level_irradiance_at(volume, probes, visibility, level, world_position, normal);
    if share >= 1.0 {
        return fine;
    }
    let coarse = level_irradiance_at(
        volume,
        probes,
        visibility,
        level + 1,
        world_position,
        normal,
    );
    let mut blended = [0.0f32; 3];
    for channel in 0..3 {
        blended[channel] = coarse[channel] * (1.0 - share) + fine[channel] * share;
    }
    blended
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe_visibility::ProbeVisibility;
    use std::f32::consts::PI;

    /// Samples of a sphere, as `(direction, solid_angle)` pairs.
    ///
    /// A uniform grid in `cos θ` and `φ`, which is a partition of *equal* solid
    /// angles — the projection integrand is a polynomial of degree two at most,
    /// so a grid this simple resolves it to well inside the tolerances below.
    fn sphere(rings: usize, sectors: usize) -> Vec<([f32; 3], f32)> {
        let mut samples = Vec::with_capacity(rings * sectors);
        let solid_angle = 4.0 * PI / (rings * sectors) as f32;
        for ring in 0..rings {
            // Cell centres, so no sample lands on a pole where `sin θ` is zero
            // and the ring degenerates.
            let cos_theta = 1.0 - 2.0 * (ring as f32 + 0.5) / rings as f32;
            let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
            for sector in 0..sectors {
                let phi = 2.0 * PI * (sector as f32 + 0.5) / sectors as f32;
                samples.push((
                    [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta],
                    solid_angle,
                ));
            }
        }
        samples
    }

    /// The body of the Slang function `signature` declares in `source`, braces
    /// included — what the two verbatim-copy tests below compare.
    ///
    /// # Panics
    ///
    /// If `source` does not declare `signature`, or the body never closes.
    fn slang_body(name: &str, source: &str, signature: &str) -> String {
        let at = source
            .find(signature)
            .unwrap_or_else(|| panic!("{name} does not declare `{signature}`"));
        let open = source[at..]
            .find('{')
            .unwrap_or_else(|| panic!("{name}'s `{signature}` has no body"))
            + at;
        let mut depth = 0usize;
        for (offset, byte) in source[open..].bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return source[open..open + offset + 1].to_string();
                    }
                }
                _ => {}
            }
        }
        panic!("{name}'s `{signature}` never closes its body")
    }

    /// Unit vectors that are not axis-aligned, so a coefficient permuted
    /// between lanes is a different answer rather than the same one.
    fn directions() -> Vec<[f32; 3]> {
        let mut out = Vec::new();
        for sample in sphere(7, 11) {
            out.push(sample.0);
        }
        out
    }

    /// **Ramamoorthi & Hanrahan's `Â₀ = π`, as the statement a frame can be
    /// wrong about**: an environment of constant radiance `L` reaches every
    /// surface as an irradiance of `πL`, whichever way it faces.
    ///
    /// The expected value is written here from the paper rather than taken from
    /// [`TRANSFER_L0`], so what this checks is the *projection* — the product
    /// of the transfer coefficient and two basis normalisations, which is the
    /// arithmetic that would go wrong silently.
    ///
    /// **The tolerance is quadrature error and `f32` summation, not slack in
    /// the claim.** The largest miss over these directions is under `1e-4` on a
    /// channel whose exact answer is `2π`, where a transfer coefficient wrong by
    /// one per cent misses by two orders of magnitude more — shown by
    /// perturbing it and watching this go red.
    #[test]
    fn a_constant_environment_integrates_to_pi_times_its_radiance() {
        let radiance = [0.25f32, 0.5, 2.0];
        let mut probe = GpuProbe::ZERO;
        for (direction, solid_angle) in sphere(64, 128) {
            probe.accumulate(direction, radiance, solid_angle);
        }

        for normal in directions() {
            let irradiance = probe.irradiance(normal);
            for channel in 0..3 {
                let expected = PI * radiance[channel];
                assert!(
                    (irradiance[channel] - expected).abs() < 1e-3,
                    "channel {channel} facing {normal:?} received {} and a constant \
                     environment integrates to {expected}",
                    irradiance[channel]
                );
            }
        }
    }

    /// **The L1 band's transfer coefficient is `2π/3`**, checked on the one
    /// environment L1 represents with no truncation error at all.
    ///
    /// A radiance field `L(ω) = b + c(ω·u)` is exactly a constant band and a
    /// linear band, so the paper's two coefficients give the irradiance
    /// exactly: `E(n) = πb + (2π/3)c(u·n)`. Both numbers are written out here
    /// rather than read from this module, for the reason the constant-band test
    /// gives — and this is the one that pins `Â₁`, because a probe filled from
    /// a constant environment has no linear band to be wrong about.
    ///
    /// `b` is large enough to keep the field non-negative everywhere, so this
    /// is a radiance field rather than an algebraic identity. The tolerance is
    /// the constant-band test's, for the same reason and with the same margin:
    /// the observed miss is under `2e-4` where `Â₁` wrong by one per cent misses
    /// by `1.3e-2`.
    #[test]
    fn the_linear_band_transfers_through_two_pi_over_three() {
        let source = [0.6f32, 0.8, 0.0];
        let (constant, linear) = (2.0f32, 1.5f32);
        let mut probe = GpuProbe::ZERO;
        for (direction, solid_angle) in sphere(64, 128) {
            let along =
                direction[0] * source[0] + direction[1] * source[1] + direction[2] * source[2];
            let radiance = constant + linear * along;
            assert!(radiance >= 0.0, "the field must be a radiance");
            probe.accumulate(direction, [radiance; 3], solid_angle);
        }

        for normal in directions() {
            let along = normal[0] * source[0] + normal[1] * source[1] + normal[2] * source[2];
            let expected = PI * constant + (2.0 * PI / 3.0) * linear * along;
            let irradiance = probe.irradiance(normal);
            assert!(
                (irradiance[0] - expected).abs() < 1e-3,
                "facing {normal:?} received {} and the paper's coefficients give {expected}",
                irradiance[0]
            );
            assert_eq!(
                irradiance[0], irradiance[1],
                "three equal channels must integrate alike"
            );
        }
    }

    /// Stored probe coefficients are irradiance, so a specular lookup must undo
    /// the constant-band transfer instead of directly evaluating its `w` lane.
    #[test]
    fn a_constant_irradiance_probe_decodes_to_its_radiance() {
        let radiance = [0.25f32, 0.5, 2.0];
        let probe = GpuProbe {
            sh_r: [0.0, 0.0, 0.0, TRANSFER_L0 * radiance[0]],
            sh_g: [0.0, 0.0, 0.0, TRANSFER_L0 * radiance[1]],
            sh_b: [0.0, 0.0, 0.0, TRANSFER_L0 * radiance[2]],
        };

        for direction in directions() {
            let decoded = probe.radiance(direction);
            for channel in 0..3 {
                assert!(
                    (decoded[channel] - radiance[channel]).abs() < 1e-6,
                    "channel {channel} facing {direction:?} decoded {} instead of {}",
                    decoded[channel],
                    radiance[channel]
                );
            }
        }
    }

    /// A specular lookup also has to undo the linear band's different transfer;
    /// direct evaluation scales the constant and linear terms by `π` and `2π/3`.
    #[test]
    fn a_linear_irradiance_probe_decodes_to_its_radiance() {
        let source = [0.6f32, 0.8, 0.0];
        let (constant, linear) = (2.0f32, 1.5f32);
        let band = [
            TRANSFER_L1 * linear * source[0],
            TRANSFER_L1 * linear * source[1],
            TRANSFER_L1 * linear * source[2],
            TRANSFER_L0 * constant,
        ];
        let probe = GpuProbe {
            sh_r: band,
            sh_g: band,
            sh_b: band,
        };

        for direction in [source, [-source[0], -source[1], -source[2]]] {
            let expected = constant
                + linear
                    * (direction[0] * source[0]
                        + direction[1] * source[1]
                        + direction[2] * source[2]);
            let decoded = probe.radiance(direction);
            for channel in decoded {
                assert!(
                    (channel - expected).abs() < 1e-6,
                    "facing {direction:?} decoded {channel} instead of {expected}"
                );
            }
        }
    }

    /// **A probe is brightest facing its light and darkest facing away**, which
    /// is the directionality the whole row exists for — and the far side is
    /// where the truncation rings negative and the clamp is what stops it.
    #[test]
    fn a_single_source_peaks_towards_it_and_is_clamped_away_from_it() {
        let source = [0.0f32, 0.0, 1.0];
        let mut probe = GpuProbe::ZERO;
        probe.accumulate(source, [1.0; 3], 0.5);

        let towards = probe.irradiance(source)[0];
        let across = probe.irradiance([1.0, 0.0, 0.0])[0];
        let away = probe.irradiance([0.0, 0.0, -1.0])[0];
        assert!(
            towards > across && across > away,
            "a probe lit from one direction must fall off away from it: \
             {towards} / {across} / {away}"
        );
        assert_eq!(
            away, 0.0,
            "L1 rings negative behind a source, and the clamp is what stops a \
             surface subtracting light"
        );
        // The ringing is real rather than hypothetical, so the clamp is doing
        // something: the unclamped series *is* negative there.
        let unclamped = probe.sh_b[3] - probe.sh_b[2];
        assert!(unclamped < 0.0, "got {unclamped}");
    }

    /// **The degenerate volume adds exactly zero**, at every position and every
    /// normal — the property every existing golden staying byte-identical rests
    /// on.
    ///
    /// Exact equality rather than a tolerance, because that is the claim: `x +
    /// 0.0 == x` on every target, and anything else moves a frame.
    #[test]
    fn a_zero_volume_adds_exactly_zero() {
        let volume = ProbeVolume::default();
        assert_eq!(volume.total(), 0);
        for position in [[0.0; 3], [1.0, -2.0, 3.5], [-1e4, 1e4, 0.0]] {
            for normal in directions() {
                assert_eq!(
                    irradiance_at(&volume, &[], &ProbeVisibility::NONE, position, normal),
                    [0.0; 3],
                    "at {position:?} facing {normal:?}"
                );
            }
        }
        // And a table that *has* rows, since the renderer creates one of at
        // least a row whatever the scene: the volume is what makes it zero, not
        // the table being empty.
        let bright = GpuProbe {
            sh_r: [1.0, 2.0, 3.0, 4.0],
            sh_g: [5.0, 6.0, 7.0, 8.0],
            sh_b: [9.0, 10.0, 11.0, 12.0],
        };
        assert_ne!(bright.irradiance([0.0, 0.0, 1.0]), [0.0; 3]);
        assert_eq!(
            irradiance_at(
                &volume,
                &[bright],
                &ProbeVisibility::NONE,
                [3.0, 4.0, 5.0],
                [0.0, 0.0, 1.0],
            ),
            bright.irradiance([0.0, 0.0, 1.0]),
            "a one-row table is addressed even by the degenerate volume, so what \
             makes a probe-less scene add nothing is the row being zero"
        );
    }

    /// **The far corner of a cell weighs exactly zero at the boundary where the
    /// cell index changes**, which is the whole of the design's determinism
    /// argument for the grid.
    ///
    /// Two rasterisers can land either side of an integer grid coordinate. The
    /// one in cell `i` at fraction ≈1 and the one in cell `i+1` at fraction ≈0
    /// have to compute the same value, and they do because the blend at zero
    /// returns its first argument bit for bit.
    #[test]
    fn a_cell_boundary_is_continuous() {
        let volume = ProbeVolume {
            origin: [0.0; 3],
            inv_spacing: [1.0; 3],
            counts: [3, 1, 1],
            levels: 1,
            steps: ProbeSteps::default(),
        };
        let probes: Vec<GpuProbe> = (0..3)
            .map(|n| {
                let base = n as f32;
                GpuProbe {
                    sh_r: [base, base + 0.125, base + 0.25, base + 0.375],
                    sh_g: [base + 0.5, base + 0.625, base + 0.75, base + 0.875],
                    sh_b: [base + 1.0, base + 1.125, base + 1.25, base + 1.375],
                }
            })
            .collect();
        let normal = [0.0, 0.0, 1.0];

        // Exactly on the boundary, the value is probe 1's own — the far corner
        // of the cell below carries weight zero and the near corner of the cell
        // above carries all of it.
        assert_eq!(
            irradiance_at(
                &volume,
                &probes,
                &ProbeVisibility::NONE,
                [1.0, 0.0, 0.0],
                normal
            ),
            probes[1].irradiance(normal)
        );
        // And the two sides converge on it rather than jumping.
        let below = irradiance_at(
            &volume,
            &probes,
            &ProbeVisibility::NONE,
            [1.0 - 1e-6, 0.0, 0.0],
            normal,
        );
        let above = irradiance_at(
            &volume,
            &probes,
            &ProbeVisibility::NONE,
            [1.0 + 1e-6, 0.0, 0.0],
            normal,
        );
        for channel in 0..3 {
            assert!(
                (below[channel] - above[channel]).abs() < 1e-4,
                "channel {channel}: {below:?} against {above:?}"
            );
        }
        // The grid really is interpolating, or the two assertions above would
        // hold for a lookup that ignored the position entirely.
        assert_ne!(
            irradiance_at(
                &volume,
                &probes,
                &ProbeVisibility::NONE,
                [0.5, 0.0, 0.0],
                normal
            ),
            probes[0].irradiance(normal)
        );
        assert_ne!(
            irradiance_at(
                &volume,
                &probes,
                &ProbeVisibility::NONE,
                [0.5, 0.0, 0.0],
                normal
            ),
            probes[1].irradiance(normal)
        );
    }

    /// A probe's index is `x`-fastest, and every axis of the grid is
    /// addressable — a volume that read `z` where it meant `x` would light a
    /// room from the wrong wall and still be a picture.
    #[test]
    fn a_probes_index_is_x_fastest() {
        let volume = ProbeVolume {
            origin: [0.0; 3],
            inv_spacing: [1.0; 3],
            counts: [2, 3, 4],
            levels: 1,
            steps: ProbeSteps::default(),
        };
        assert_eq!(volume.total(), 24);
        let probes: Vec<GpuProbe> = (0..24)
            .map(|n| GpuProbe {
                sh_r: [0.0, 0.0, 0.0, n as f32],
                ..GpuProbe::ZERO
            })
            .collect();
        // The constant band alone, so the irradiance a normal reads back is the
        // row's own number.
        let at = |x: f32, y: f32, z: f32| {
            irradiance_at(
                &volume,
                &probes,
                &ProbeVisibility::NONE,
                [x, y, z],
                [1.0, 0.0, 0.0],
            )[0]
        };
        assert_eq!(at(0.0, 0.0, 0.0), 0.0);
        assert_eq!(at(1.0, 0.0, 0.0), 1.0, "x steps by one row");
        assert_eq!(at(0.0, 1.0, 0.0), 2.0, "y steps by the x count");
        assert_eq!(
            at(0.0, 0.0, 1.0),
            6.0,
            "z steps by the x count times the y count"
        );
        assert_eq!(at(1.0, 2.0, 3.0), 23.0, "the far corner is the last row");
        // Outside the grid clamps to its surface rather than wrapping.
        assert_eq!(at(-5.0, -5.0, -5.0), 0.0);
        assert_eq!(at(50.0, 50.0, 50.0), 23.0);
    }

    /// A row survives the round trip through the bytes a buffer holds, and the
    /// three channels land in three different places.
    #[test]
    fn a_probe_row_round_trips_through_its_bytes() {
        let probe = GpuProbe {
            sh_r: [1.0, 2.0, 3.0, 4.0],
            sh_g: [5.0, 6.0, 7.0, 8.0],
            sh_b: [9.0, 10.0, 11.0, 12.0],
        };
        let bytes = probe.to_bytes();
        assert_eq!(bytes.len(), PROBE_STRIDE);
        assert_eq!(GpuProbe::from_bytes(&bytes), probe);
        // Each channel at its own offset, so a writer that put one band twice
        // is a different byte string rather than the same one.
        assert_eq!(&bytes[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[12..16], &4.0f32.to_le_bytes());
        assert_eq!(&bytes[16..20], &5.0f32.to_le_bytes());
        assert_eq!(&bytes[32..36], &9.0f32.to_le_bytes());
        assert_eq!(&bytes[44..48], &12.0f32.to_le_bytes());
        assert_eq!(GpuProbe::ZERO.to_bytes(), [0u8; PROBE_STRIDE]);
        assert_eq!(GpuProbe::default(), GpuProbe::ZERO);
    }

    /// The clipmap header's rows, at the offsets the shader's block puts them —
    /// including the count lane the shader clamps its fetches against and the
    /// per-level rows it reads instead of deriving.
    ///
    /// **Scrolled, and by a different number of steps per level**, so a header
    /// that wrote one level's offset into another's row, or wrote the signed
    /// step where the reduced one belongs, is a different byte string rather
    /// than the same one.
    #[test]
    fn the_clipmap_header_is_the_counts_the_level_row_and_a_trio_per_level() {
        let mut steps = ProbeSteps::default();
        steps[0] = [1, -1, 4];
        steps[1] = [0, 2, -5];
        let volume = ProbeVolume {
            origin: [1.0, 2.0, 3.0],
            inv_spacing: [0.25, 0.5, 0.75],
            counts: [3, 3, 3],
            levels: 2,
            steps,
        };
        let bytes = volume.to_bytes();
        assert_eq!(bytes.len(), PROBE_VOLUME_SIZE);
        let float_at = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().expect("4"));
        let word_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4"));
        assert_eq!([word_at(0), word_at(4), word_at(8)], volume.counts);
        assert_eq!(
            word_at(12),
            54,
            "the counts' fourth lane carries the rows the whole clipmap holds"
        );
        assert_eq!(
            [word_at(16), word_at(20)],
            [2, 27],
            "the level row carries the live level count and one level's rows"
        );
        assert_eq!(
            [word_at(24), word_at(28)],
            [0, 0],
            "the level row's last two lanes are padding"
        );

        // Every level's own pair, against the accessors that decide them — so a
        // header written in the wrong order, or with a level's rows swapped, is
        // a different value rather than the same one.
        let origins = 32;
        let inv_spacings = origins + 16 * PROBE_LEVELS;
        for level in 0..PROBE_LEVELS as u32 {
            let live = level < volume.level_count();
            let origin = if live {
                volume.level_origin(level)
            } else {
                [0.0; 3]
            };
            let inv_spacing = if live {
                volume.level_inv_spacing(level)
            } else {
                [0.0; 3]
            };
            let row = origins + 16 * level as usize;
            assert_eq!(
                [float_at(row), float_at(row + 4), float_at(row + 8)],
                origin,
                "level {level}'s origin"
            );
            assert_eq!(
                float_at(row + 12),
                0.0,
                "an origin's fourth lane is padding"
            );
            let row = inv_spacings + 16 * level as usize;
            assert_eq!(
                [float_at(row), float_at(row + 4), float_at(row + 8)],
                inv_spacing,
                "level {level}'s reciprocal spacing"
            );
            assert_eq!(
                float_at(row + 12),
                0.0,
                "a spacing's fourth lane is padding"
            );
        }

        // The scroll offsets, reduced into `0..count` — a level that stepped
        // backwards writes the row it wrapped onto rather than a negative one,
        // which is the whole reason the reduction is on this side.
        let offsets = inv_spacings + 16 * PROBE_LEVELS;
        assert_eq!(
            [word_at(offsets), word_at(offsets + 4), word_at(offsets + 8)],
            [1, 2, 1],
            "level 0 stepped [1, -1, 4] over counts of three"
        );
        assert_eq!(
            [
                word_at(offsets + 16),
                word_at(offsets + 20),
                word_at(offsets + 24)
            ],
            [0, 2, 1],
            "level 1 stepped [0, 2, -5] over counts of three"
        );
        for level in 0..PROBE_LEVELS as u32 {
            let row = offsets + 16 * level as usize;
            let live = level < volume.level_count();
            assert_eq!(
                [word_at(row), word_at(row + 4), word_at(row + 8)],
                if live {
                    volume.level_offset(level)
                } else {
                    [0; 3]
                },
                "level {level}'s scroll offset"
            );
            assert_eq!(word_at(row + 12), 0, "an offset's fourth lane is padding");
        }

        // **Level 0 is the fields it was given, bit for bit**, once the scroll
        // is taken back out — which is what makes an unscrolled one-level volume
        // the uniform grid this type used to be.
        let still = ProbeVolume {
            steps: ProbeSteps::default(),
            ..volume
        };
        let unscrolled = still.to_bytes();
        let float_of =
            |at: usize| f32::from_le_bytes(unscrolled[at..at + 4].try_into().expect("4"));
        assert_eq!(
            [
                float_of(origins),
                float_of(origins + 4),
                float_of(origins + 8)
            ],
            volume.origin
        );
        assert_eq!(
            [
                float_at(inv_spacings),
                float_at(inv_spacings + 4),
                float_at(inv_spacings + 8)
            ],
            volume.inv_spacing,
            "a scroll moves a level's origin and never its spacing"
        );

        assert_eq!(
            ProbeVolume::default().to_bytes(),
            [0u8; PROBE_VOLUME_SIZE],
            "the degenerate volume is a block of zeroes, which is what an \
             unwritten uniform block already holds"
        );
    }

    /// A constant environment of radiance `level`, projected the only correct
    /// way — so a row is distinguishable from every other and no coefficient is
    /// written by hand.
    fn constant_probe(radiance: [f32; 3]) -> GpuProbe {
        let solid_angle = 4.0 * PI / 6.0;
        let mut probe = GpuProbe::ZERO;
        for direction in [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ] {
            probe.accumulate(direction, radiance, solid_angle);
        }
        probe
    }

    /// A clipmap on the `x` axis alone: `counts` probes a level, spacing one at
    /// level 0, and every level's rows one constant colour so the only thing
    /// that can vary across a sweep is which level was read.
    fn clipmap(counts: u32, levels: u32, radiances: &[[f32; 3]]) -> (ProbeVolume, Vec<GpuProbe>) {
        let volume = ProbeVolume {
            origin: [0.0, 0.0, 0.0],
            // `y` and `z` have no reciprocal, which is a grid one probe deep on
            // both — the shape every fixture in this tree already uses, and the
            // one the level pick has to treat as contained everywhere.
            inv_spacing: [1.0, 0.0, 0.0],
            counts: [counts, 1, 1],
            levels,
            steps: ProbeSteps::default(),
        };
        let mut rows = Vec::with_capacity(volume.total() as usize);
        for level in 0..volume.level_count() {
            for _ in 0..volume.per_level() {
                rows.push(constant_probe(radiances[level as usize]));
            }
        }
        assert_eq!(rows.len(), volume.total() as usize);
        (volume, rows)
    }

    /// **A coarser level is twice as wide, concentric, and level 0 is exactly
    /// the fields the volume was given.**
    ///
    /// The three claims the whole clipmap rests on. The last is the one that
    /// cannot be approximate: a level-0 origin that had been through a centre
    /// and back would move a one-level volume by a rounding level, and every
    /// golden in the tree is a one-level volume.
    #[test]
    fn a_coarser_level_is_twice_as_wide_about_the_same_centre() {
        let volume = ProbeVolume {
            origin: [1.5, -2.25, 4.0],
            inv_spacing: [0.5, 0.25, 2.0],
            counts: [5, 3, 9],
            levels: 4,
            steps: ProbeSteps::default(),
        };
        assert_eq!(volume.level_origin(0), volume.origin);
        assert_eq!(volume.level_inv_spacing(0), volume.inv_spacing);
        assert_eq!(volume.level_row(0), 0);

        let centre = |level: u32| {
            let origin = volume.level_origin(level);
            let spacing = volume.level_spacing(level);
            let mut at = [0.0f32; 3];
            for axis in 0..3 {
                at[axis] = origin[axis] + 0.5 * (volume.counts[axis] - 1) as f32 * spacing[axis];
            }
            at
        };
        let middle = centre(0);
        for level in 1..volume.level_count() {
            assert_eq!(
                centre(level),
                middle,
                "level {level} is not centred where level 0 is"
            );
            for axis in 0..3 {
                assert_eq!(
                    volume.level_spacing(level)[axis],
                    volume.level_spacing(level - 1)[axis] * 2.0,
                    "level {level} does not step twice as far as level {} on axis {axis}",
                    level - 1
                );
            }
            assert_eq!(volume.level_row(level), level * volume.per_level());
        }
        assert_eq!(volume.total(), volume.per_level() * 4);
    }

    /// **The finest level that contains a point is the one it reads**, and the
    /// coarsest takes the whole share however far out the point is.
    ///
    /// The reach is the only input to the pick, so this is the pick's whole
    /// contract: a level whose extent the point is inside, and a share of one
    /// everywhere but the outer band.
    #[test]
    fn the_finest_level_that_contains_a_point_is_the_one_it_reads() {
        let volume = ProbeVolume {
            counts: [5, 1, 1],
            inv_spacing: [1.0, 0.0, 0.0],
            levels: 3,
            ..ProbeVolume::default()
        };
        // Well inside level 0, in its band, on its boundary, inside level 1,
        // and past the whole clipmap.
        for (reach, level, share) in [
            (0.0f32, 0u32, 1.0f32),
            (0.5, 0, 1.0),
            (0.75, 0, 1.0),
            (0.875, 0, 0.5),
            (1.0, 1, 1.0),
            (1.5, 1, 1.0),
            (1.75, 1, 0.5),
            (2.0, 2, 1.0),
            (17.0, 2, 1.0),
        ] {
            assert_eq!(
                volume.level_of(reach),
                (level, share),
                "a reach of {reach} must read level {level} at a share of {share}"
            );
        }
        // A volume of one level has nothing to blend towards and clamps, which
        // is the uniform grid this type used to be.
        let one = ProbeVolume {
            levels: 1,
            ..volume
        };
        for reach in [0.0f32, 0.9, 1.0, 40.0] {
            assert_eq!(one.level_of(reach), (0, 1.0));
        }
    }

    /// **The reach is one at a level's boundary and zero at its centre**, which
    /// is what makes the pick above about the world rather than about a number.
    ///
    /// An axis with no extent contributes nothing: the `y` and `z` of the
    /// fixture below have no reciprocal spacing, and a point far off them is
    /// still inside the level — which is the shape every probe fixture in this
    /// tree has, and the one a naive division by a zero extent would answer
    /// `inf` for.
    #[test]
    fn the_reach_is_one_on_a_levels_boundary() {
        let (volume, _) = clipmap(5, 2, &[[1.0; 3], [1.0; 3]]);
        // Level 0 spans `x` in 0..4 and its centre is 2.
        assert_eq!(volume.level_reach([2.0, 0.0, 0.0]), 0.0);
        assert_eq!(volume.level_reach([4.0, 0.0, 0.0]), 1.0);
        assert_eq!(volume.level_reach([0.0, 0.0, 0.0]), 1.0);
        assert_eq!(volume.level_reach([3.0, 0.0, 0.0]), 0.5);
        assert_eq!(volume.level_reach([6.0, 0.0, 0.0]), 2.0);
        assert_eq!(
            volume.level_reach([2.0, 900.0, -900.0]),
            0.0,
            "an axis with no extent has no boundary to be outside of"
        );
    }

    /// **A fragment walking across a level boundary does not step** — the whole
    /// of what the band buys, as a measurement rather than an assertion.
    ///
    /// The fixture is two levels of one constant colour each, so the trilinear
    /// gather within a level is flat and the *only* thing that can move the
    /// result across the sweep is the level blend. The observable is the largest
    /// difference between two neighbouring samples of a line that crosses level
    /// 0's boundary, against the whole distance the line travels.
    ///
    /// It fails in both directions, which is what makes it a check rather than a
    /// floor: a band of zero puts the whole distance into one step, and a blend
    /// that did nothing leaves the ends equal and misses the travel below.
    #[test]
    fn a_level_boundary_does_not_step() {
        let (volume, rows) = clipmap(5, 2, &[[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
        let visibility = ProbeVisibility::NONE;
        // Level 0's boundary is at `x = 4` and its band opens at `x = 3.5`; the
        // sweep starts inside the flat part and ends outside the level.
        const FROM: f32 = 3.0;
        const TO: f32 = 4.5;
        const SAMPLES: usize = 301;
        let at = |sample: usize| {
            let x = FROM + (TO - FROM) * sample as f32 / (SAMPLES - 1) as f32;
            irradiance_at(&volume, &rows, &visibility, [x, 0.0, 0.0], [0.0, 1.0, 0.0])
        };
        let distance = |a: [f32; 3], b: [f32; 3]| {
            (0..3).fold(0.0f32, |worst, channel| {
                worst.max((a[channel] - b[channel]).abs())
            })
        };

        let mut step = 0.0f32;
        let mut previous = at(0);
        for sample in 1..SAMPLES {
            let value = at(sample);
            step = step.max(distance(previous, value));
            previous = value;
        }
        let travel = distance(at(0), at(SAMPLES - 1));
        eprintln!("crcbl probes: level sweep travels {travel:.4} in steps of at most {step:.4}");

        assert!(
            travel > 1.0,
            "the sweep must actually change level: it travelled {travel:.4}, which is a \
             blend that read one level the whole way"
        );
        // What an even ramp across the band takes per sample, read out of the
        // volume's own geometry rather than written down again: the band is
        // `LEVEL_BAND` of level 0's half-extent, and a ramp spreads the whole
        // travel evenly over it.
        let band = LEVEL_BAND * 0.5 * (volume.counts[0] - 1) as f32 * volume.level_spacing(0)[0];
        let ramp = travel * (TO - FROM) / ((SAMPLES - 1) as f32 * band);
        // Three times the ramp, which the run that landed this clears by a
        // factor of three and which a level *change* — the whole travel in one
        // step — misses by a factor of thirty.
        let ceiling = 3.0 * ramp;
        assert!(
            step <= ceiling,
            "one step of the sweep moved {step:.4} of a {travel:.4} travel, past the \
             {ceiling:.4} that is three times the {ramp:.4} an even ramp across the band \
             would take — the level changed rather than blended"
        );
    }

    /// **A one-level volume evaluates exactly one gather**, bit for bit the same
    /// value the uniform grid gave.
    ///
    /// The claim every golden in this tree rests on across the clipmap's
    /// arrival, and it is an equality rather than a tolerance because the level
    /// blend is the only thing between the two: with one level the share is one,
    /// the coarse read is never taken, and the arithmetic is untouched.
    #[test]
    fn a_one_level_volume_is_the_level_it_has() {
        let (volume, rows) = clipmap(5, 1, &[[0.7, 0.3, 0.1]]);
        let visibility = ProbeVisibility::NONE;
        for x in [-3.0f32, 0.0, 1.25, 2.5, 4.0, 9.0] {
            let world = [x, 0.5, -0.5];
            let normal = [0.0, 1.0, 0.0];
            assert_eq!(
                irradiance_at(&volume, &rows, &visibility, world, normal),
                level_irradiance_at(&volume, &rows, &visibility, 0, world, normal),
                "at x = {x} a one-level volume read something other than its own level"
            );
        }
    }

    /// A count that would overflow the product saturates rather than wrapping
    /// to something small — a wrapped total is a bound a fetch would pass.
    #[test]
    fn an_absurd_grid_saturates_its_total() {
        let volume = ProbeVolume {
            counts: [1 << 16, 1 << 16, 2],
            ..ProbeVolume::default()
        };
        assert_eq!(volume.total(), u32::MAX);
    }

    /// The block `mesh.slang` declares, member for member and in this order.
    ///
    /// The offsets [`ProbeVolume::to_bytes`] writes are only right if the
    /// shader's block puts the rows in the same order — swapping the origin
    /// array and the spacing array produces a frame lit from somewhere else,
    /// which is a picture.
    ///
    /// **Every file that declares the block is checked**, not just the one that
    /// reads it: `mesh_cluster.slang` and `mesh.slang` are one buffer, and
    /// `ssr.slang` carries the same rows in its own.
    #[test]
    fn the_grid_header_matches_the_block_the_shaders_declare() {
        let declarations = [
            "uint4 probe_counts;",
            "uint4 probe_levels;",
            "float4 probe_level_origin[PROBE_LEVELS];",
            "float4 probe_level_inv_spacing[PROBE_LEVELS];",
            "uint4 probe_level_offset[PROBE_LEVELS];",
        ];
        for (name, source) in [
            ("mesh.slang", include_str!("../shaders/mesh.slang")),
            (
                "mesh_cluster.slang",
                include_str!("../shaders/mesh_cluster.slang"),
            ),
            ("ssr.slang", include_str!("../shaders/ssr.slang")),
        ] {
            let mut previous = 0;
            for declaration in declarations {
                let at = source
                    .find(declaration)
                    .unwrap_or_else(|| panic!("{name} does not declare `{declaration}`"));
                assert!(
                    at > previous,
                    "{name} declares `{declaration}` in a different order than \
                     `to_bytes` writes it"
                );
                previous = at;
            }
            assert!(
                source.contains(&format!("static const uint PROBE_LEVELS = {PROBE_LEVELS};")),
                "{name} sizes the per-level arrays with a different level count \
                 than this module writes rows for, so the block's later members \
                 are at offsets the two sides disagree about"
            );
        }
        // And the row itself, whose three channels are what `PROBE_STRIDE`
        // measures.
        let source = include_str!("../shaders/mesh.slang");
        for declaration in ["float4 sh_r;", "float4 sh_g;", "float4 sh_b;"] {
            assert!(
                source.contains(declaration),
                "mesh.slang does not declare `{declaration}`"
            );
        }
    }

    /// **The shaders must name the same band this module does.**
    ///
    /// Nothing else can catch a drift: both files compile with any value, and a
    /// mismatch shows up only as a fragment whose diffuse and specular probe
    /// reads fade between levels over different distances — a picture, and a
    /// plausible one. Reading the source is the check, and the source is
    /// hash-pinned by the manifest, so it is the same file the committed
    /// artifact was built from.
    #[test]
    fn the_level_blend_constants_match_the_ones_the_shaders_declare() {
        let declaration = format!("static const float PROBE_LEVEL_BAND = {LEVEL_BAND};");
        for (name, source) in [
            ("mesh.slang", include_str!("../shaders/mesh.slang")),
            ("ssr.slang", include_str!("../shaders/ssr.slang")),
        ] {
            assert!(
                source.contains(&declaration),
                "{name} does not declare `{declaration}`; the clipmap's blend \
                 band has drifted from the module that owns it"
            );
        }
    }

    /// **The two shaders pick a level with the same arithmetic, character for
    /// character** — and it is the arithmetic
    /// [`ProbeVolume::level_reach`] and [`ProbeVolume::level_of`] mirror.
    ///
    /// `mesh.slang` evaluates its gather against a normal and `ssr.slang`
    /// against a reflected direction, so the two evaluators are genuinely
    /// different code; the *selection* in front of them is one rule, and a copy
    /// of it that drifted would fade the diffuse and the specular reads between
    /// different levels at the same point. This is `crate::ssr`'s
    /// screen-space-helper guard applied to the clipmap, and it holds the bodies
    /// rather than the declarations so each file's doc comment can say what that
    /// file uses them for.
    ///
    /// The weighting *within* a level is shared outright rather than merely
    /// selected the same way — see `crate::probe_visibility`'s
    /// `the_shaders_weigh_a_probe_the_way_this_module_does`.
    #[test]
    fn the_shaders_pick_a_level_the_way_this_module_does() {
        let mesh = include_str!("../shaders/mesh.slang");
        let ssr = include_str!("../shaders/ssr.slang");
        for signature in [
            "float probe_level_reach(float3 world_position, float3 origin, float3 inv_spacing, \
             float3 last)",
            "float2 probe_level_of(float reach, uint levels)",
        ] {
            let bodies: Vec<(&str, String)> = [("mesh.slang", mesh), ("ssr.slang", ssr)]
                .into_iter()
                .map(|(name, source)| (name, slang_body(name, source, signature)))
                .collect();
            assert_eq!(
                bodies[0].1, bodies[1].1,
                "`{signature}` differs between {} and {}; the clipmap's level \
                 pick is copied verbatim and one copy has drifted",
                bodies[0].0, bodies[1].0
            );
        }
    }

    /// **Both shaders wrap a scrolled cell with the body [`wrap`] mirrors**, to
    /// the character.
    ///
    /// The wrap is the whole of the toroidal addressing: drop it and every
    /// fragment of a scrolled level reads the row of the probe that *used* to
    /// stand there, which is a picture rather than an error. Nothing about the
    /// compile catches that, and neither does a golden of an unscrolled scene,
    /// so what pins it is this text and the device sweep
    /// `crates/crcbl/tests/render_e2e.rs`'s
    /// `a_scrolled_volume_reads_the_rows_the_mirror_does` runs beside it.
    ///
    /// # How it was shown to fail
    ///
    /// By deleting the compare from `mesh.slang`'s body — `return at;` — and
    /// running this, which reported
    ///
    /// > mesh.slang's `uint probe_wrap(uint cell, uint offset, uint count)` is
    /// > not the body `crcbl_shaders::probe::wrap` mirrors; the clipmap's
    /// > toroidal addressing has drifted
    ///
    /// with the two bodies printed beside it; and by weakening it to
    /// `at > count`, the off-by-one that leaves one cell of every axis reading
    /// its neighbour's row, which this reported against the mirrored body in the
    /// same words.
    #[test]
    fn the_shaders_wrap_a_scrolled_cell_the_way_this_module_does() {
        let signature = "uint probe_wrap(uint cell, uint offset, uint count)";
        // The body [`wrap`] is the mirror of, written here rather than read out
        // of either file: a drift the two shaders shared would otherwise agree
        // with itself.
        let mirrored =
            "{\n    uint at = cell + offset;\n    return at >= count ? at - count : at;\n}";
        for (name, source) in [
            ("mesh.slang", include_str!("../shaders/mesh.slang")),
            ("ssr.slang", include_str!("../shaders/ssr.slang")),
        ] {
            assert_eq!(
                slang_body(name, source, signature),
                mirrored,
                "{name}'s `{signature}` is not the body `crcbl_shaders::probe::wrap` \
                 mirrors; the clipmap's toroidal addressing has drifted"
            );
        }
    }

    /// **A row is the cell plus the level's scroll offset, modulo the counts**
    /// — swept over every cell of a small level and over offsets that reach
    /// well past the counts in both directions.
    ///
    /// [`ProbeVolume::row`] spends one compare where a remainder would do, and
    /// this is what says the two are the same function: the expected value here
    /// is `rem_euclid` of the *unreduced* step, so a reduction that dropped a
    /// sign or a wrap that fired one cell early is a different row.
    #[test]
    fn a_scrolled_cell_lands_at_its_index_modulo_the_counts() {
        let counts = [3u32, 4, 5];
        for step in [-13i32, -7, -5, -4, -1, 0, 1, 3, 4, 5, 9, 20] {
            let mut steps = ProbeSteps::default();
            // A different step per axis off one sweep value, so a `row` that
            // used the `x` offset on every axis is a different answer.
            steps[0] = [step, step + 2, step - 3];
            let volume = ProbeVolume {
                origin: [0.0; 3],
                inv_spacing: [1.0; 3],
                counts,
                levels: 1,
                steps,
            };
            for z in 0..counts[2] {
                for y in 0..counts[1] {
                    for x in 0..counts[0] {
                        let cell = [x, y, z];
                        let mut wrapped = [0u32; 3];
                        for axis in 0..3 {
                            let index = i64::from(cell[axis]) + i64::from(steps[0][axis]);
                            wrapped[axis] = index.rem_euclid(i64::from(counts[axis])) as u32;
                        }
                        let expected =
                            (wrapped[2] * counts[1] + wrapped[1]) * counts[0] + wrapped[0];
                        assert_eq!(
                            volume.row(0, cell),
                            expected,
                            "cell {cell:?} of a level stepped {:?} landed in the wrong row",
                            steps[0]
                        );
                    }
                }
            }
        }
    }

    /// **A step of `k` probes along one axis exposes exactly `k` slabs, and
    /// every other probe is still addressable at the row it already had.**
    ///
    /// The invalidation rule, both halves, because either alone passes for the
    /// wrong reason: a rule that named every row would satisfy the first half
    /// and re-capture the level, and a rule that named none would satisfy the
    /// second and light the room with maps about somewhere else.
    ///
    /// The retained half is checked against the volume *before* the step: the
    /// probe now at cell `c` is the probe that was at cell `c + k`, and the
    /// claim is that it stands in the same place and in the same row. That is
    /// what makes the recapture a slab rather than a level.
    ///
    /// # How it was shown to fail
    ///
    /// By widening [`ProbeVolume::exposed`]'s retained test to `was >= -1` — one
    /// slab too few, the departing slab counted as one that stayed — and running
    /// this, which reported
    ///
    /// > a step of -2 along axis 0 exposed 20 row(s), and the 2 slab(s) it moves
    /// > are 40 probe(s) of this level
    ///
    /// and, with the widening in the other direction (`was > 0`), the same
    /// assertion the other way round — 60 row(s) against 40. The retained half
    /// was shown by taking the world position check to the unscrolled volume,
    /// which reported the probe that stayed standing a spacing from where it had
    /// been.
    #[test]
    fn a_step_exposes_its_own_slabs_and_leaves_the_rest_where_they_were() {
        let counts = [3u32, 4, 5];
        let before = ProbeVolume {
            origin: [10.0, 20.0, 30.0],
            inv_spacing: [1.0 / 2.0, 1.0 / 4.0, 1.0 / 8.0],
            counts,
            levels: 1,
            steps: ProbeSteps::default(),
        };
        let slab = [
            counts[1] * counts[2],
            counts[0] * counts[2],
            counts[0] * counts[1],
        ];
        for axis in 0..3usize {
            for step in [-9i32, -2, -1, 1, 2, 9] {
                let mut steps = ProbeSteps::default();
                steps[0][axis] = step;
                let rows = before.exposed(&steps);
                let slabs = step.unsigned_abs().min(counts[axis]);
                assert_eq!(
                    rows.len() as u32,
                    slabs * slab[axis],
                    "a step of {step} along axis {axis} exposed {} row(s), and the {slabs} \
                     slab(s) it moves are {} probe(s) of this level",
                    rows.len(),
                    slabs * slab[axis]
                );
                // No row twice, so the count above is a count of probes.
                let mut seen = rows.clone();
                seen.sort_unstable();
                seen.dedup();
                assert_eq!(seen.len(), rows.len(), "a row was exposed twice");

                let after = ProbeVolume { steps, ..before };
                let exposed: std::collections::BTreeSet<u32> = rows.into_iter().collect();
                for z in 0..counts[2] {
                    for y in 0..counts[1] {
                        for x in 0..counts[0] {
                            let cell = [x, y, z];
                            let row = after.row(0, cell);
                            if exposed.contains(&row) {
                                continue;
                            }
                            // The cell this probe stood in before the step.
                            let mut was = cell;
                            was[axis] = u32::try_from(i64::from(cell[axis]) + i64::from(step))
                                .expect("a retained cell is inside the old level");
                            assert!(
                                was[axis] < counts[axis],
                                "cell {cell:?} was kept and its old cell {was:?} is outside \
                                 the level it was in"
                            );
                            assert_eq!(
                                before.row(0, was),
                                row,
                                "the probe now at {cell:?} was at {was:?} and its row moved"
                            );
                            assert_eq!(
                                before.position(0, was),
                                after.position(0, cell),
                                "the probe now at {cell:?} was at {was:?} and it moved in world \
                                 space, so its captured map is about somewhere else"
                            );
                        }
                    }
                }
            }
        }
    }

    /// **A step on more than one axis exposes the union of its axes' slabs**,
    /// which is fewer rows than the sum of them.
    ///
    /// The corner probes are in two slabs at once, and a rule that added the
    /// axes rather than taking their union would name them twice — which the
    /// device would then capture twice, and which the `dedup` above would hide
    /// if this case were never asked.
    #[test]
    fn a_step_on_two_axes_exposes_their_union() {
        let counts = [4u32, 4, 4];
        let volume = ProbeVolume {
            origin: [0.0; 3],
            inv_spacing: [1.0; 3],
            counts,
            levels: 1,
            steps: ProbeSteps::default(),
        };
        let mut steps = ProbeSteps::default();
        steps[0] = [1, 2, 0];
        let rows = volume.exposed(&steps);
        // The complement is the box of probes inside the old level on both
        // moved axes: 3 × 2 × 4.
        let kept = (counts[0] - 1) * (counts[1] - 2) * counts[2];
        assert_eq!(
            rows.len() as u32,
            counts[0] * counts[1] * counts[2] - kept,
            "a step of [1, 2, 0] over a 4×4×4 level keeps a 3×2×4 box"
        );
        let summed = counts[1] * counts[2] + 2 * counts[0] * counts[2];
        assert!(
            (rows.len() as u32) < summed,
            "a step of [1, 2, 0] exposed {} row(s), which is the {summed} its two axes' \
             slabs come to when the corners they share are counted twice",
            rows.len()
        );
    }

    /// **A whole level's worth of steps exposes the whole level**, and so does
    /// any step past it — the same arithmetic rather than a special case.
    #[test]
    fn a_step_past_the_level_exposes_all_of_it() {
        let counts = [3u32, 3, 3];
        let volume = ProbeVolume {
            origin: [0.0; 3],
            inv_spacing: [1.0; 3],
            counts,
            levels: 2,
            steps: ProbeSteps::default(),
        };
        for step in [3i32, 4, 100, -3, -50] {
            let mut steps = ProbeSteps::default();
            steps[0][0] = step;
            assert_eq!(
                volume.exposed(&steps).len() as u32,
                volume.per_level(),
                "a step of {step} over three probes left something addressable"
            );
        }
    }

    /// **The follow re-centres every level on the point, by whole steps of that
    /// level's own spacing** — and a level twice as coarse takes half as many.
    ///
    /// The claim that makes the scroll toroidal at all: the centre lands within
    /// half a spacing of the point, which is the nearest a whole step can get,
    /// and a coarser level is coarser rather than merely further.
    #[test]
    fn a_follow_centres_each_level_within_half_a_step_of_the_point() {
        let mut volume = ProbeVolume {
            origin: [0.0; 3],
            inv_spacing: [1.0 / 2.0, 1.0, 1.0 / 4.0],
            counts: [5, 5, 5],
            levels: 3,
            steps: ProbeSteps::default(),
        };
        let authored = volume.centre();
        for point in [
            [0.0f32, 0.0, 0.0],
            [1.0, -3.0, 17.0],
            [-40.0, 8.5, -0.75],
            [123.0, -64.0, 250.0],
        ] {
            volume.follow(point);
            for level in 0..volume.level_count() {
                let spacing = volume.level_spacing(level);
                // The level's own centre, which is its origin plus half its
                // extent — the same point `centre` names before any scroll.
                let mut at = [0.0f32; 3];
                let origin = volume.level_origin(level);
                for axis in 0..3 {
                    at[axis] = origin[axis]
                        + 0.5 * (volume.counts[axis].saturating_sub(1) as f32) * spacing[axis];
                }
                for axis in 0..3 {
                    assert!(
                        (at[axis] - point[axis]).abs() <= 0.5 * spacing[axis] + 1e-4,
                        "level {level} centred at {at:?} for a point of {point:?}, which is \
                         further than half its spacing of {spacing:?}"
                    );
                    // And the move is a whole number of steps off the authored
                    // centre, which is what leaves the rows that stayed alone.
                    let moved = (at[axis] - authored[axis]) / spacing[axis];
                    assert!(
                        (moved - moved.round()).abs() < 1e-3,
                        "level {level} moved {moved} steps along axis {axis}, which is not whole"
                    );
                }
            }
        }
    }

    /// **A volume that has not moved exposes nothing**, which is the ordinary
    /// frame — and the reason a follow every frame costs a comparison rather
    /// than a capture.
    #[test]
    fn a_follow_that_does_not_move_exposes_nothing() {
        let mut volume = ProbeVolume {
            origin: [0.0; 3],
            inv_spacing: [1.0; 3],
            counts: [4, 4, 4],
            levels: 2,
            steps: ProbeSteps::default(),
        };
        let point = [1.7f32, -0.2, 0.9];
        assert!(
            !volume.follow(point).is_empty(),
            "the first follow moves it"
        );
        let settled = volume.steps;
        assert!(
            volume.follow(point).is_empty(),
            "following the same point twice re-captured the level"
        );
        assert_eq!(volume.steps, settled);
        // And a point a fifth of a spacing further along does not move it
        // either, which is what stops a walking camera scrolling every frame:
        // the level's centre sits 0.2 of a spacing from the point and this
        // takes it to 0.4, still the same nearest whole step.
        assert!(
            volume
                .follow([point[0] + 0.2, point[1], point[2]])
                .is_empty(),
            "a fifth of a spacing re-captured the level"
        );
    }

    /// **An unscrolled volume is the volume it was**, row for row and place for
    /// place — the claim every golden blessed before the scroll existed rests
    /// on.
    #[test]
    fn an_unscrolled_volume_addresses_what_it_always_did() {
        let volume = ProbeVolume {
            origin: [1.5, -2.25, 4.0],
            inv_spacing: [0.5, 0.25, 2.0],
            counts: [5, 3, 9],
            levels: 4,
            steps: ProbeSteps::default(),
        };
        for level in 0..volume.level_count() {
            assert_eq!(volume.level_offset(level), [0; 3]);
            for z in 0..volume.counts[2] {
                for y in 0..volume.counts[1] {
                    for x in 0..volume.counts[0] {
                        assert_eq!(
                            volume.row(level, [x, y, z]),
                            volume.level_row(level)
                                + (z * volume.counts[1] + y) * volume.counts[0]
                                + x,
                            "level {level} cell ({x}, {y}, {z})"
                        );
                    }
                }
            }
        }
        assert_eq!(volume.level_origin(0), volume.origin);
    }
}
