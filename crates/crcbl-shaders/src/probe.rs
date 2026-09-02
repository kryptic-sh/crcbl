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
/// Two `uint4` — the counts and the level row — and then a `float4` origin and
/// a `float4` reciprocal spacing for each of [`PROBE_LEVELS`]. `std140` gives
/// each of them one sixteen-byte row, and the total is already a multiple of
/// sixteen, so there is no tail padding to write.
///
/// **The per-level rows are uploaded rather than derived in the shader**, which
/// is the whole of why they are here: where each level stands and how far apart
/// its probes are is one rule, [`ProbeVolume::level_origin`] and
/// [`ProbeVolume::level_inv_spacing`] are where it is written, and a fragment
/// reads the answer instead of recomputing it. `probe_capture`'s octahedral
/// direction table is the precedent.
pub const PROBE_VOLUME_SIZE: usize = 32 + 2 * 16 * PROBE_LEVELS;

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
const PROJECT_L0: f32 = TRANSFER_L0 / (4.0 * std::f32::consts::PI);

/// The same for the linear band: `Â₁ · (Y₁₁/x)²`, with
/// `Y₁₁ = ½√(3/π) · x` and its two siblings carrying the same normalisation on
/// `y` and `z`.
const PROJECT_L1: f32 = 3.0 * TRANSFER_L1 / (4.0 * std::f32::consts::PI);

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
    /// Where probe `(0, 0, 0)` of **level 0**, the finest, is in world space.
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

    /// Where probe `(0, 0, 0)` of `level` is, in world space.
    ///
    /// Every level is centred on level 0's centre and level `k` spans `2^k`
    /// times as much of the world, so a coarser level's first probe stands
    /// further out by half of the extent it gained.
    ///
    /// **Written as an offset from [`origin`](Self::origin) rather than as a
    /// centre minus a half-extent**, so that level 0 returns that field
    /// *exactly*: the offset it adds is a multiplication by zero, where naming
    /// the centre and subtracting it back would round twice. That exactness is
    /// what makes a one-level volume the grid it was.
    #[must_use]
    pub fn level_origin(&self, level: u32) -> [f32; 3] {
        let level = level.min(self.level_count() - 1);
        // `1 - 2^k`: the extent this level gained over level 0, as a multiple
        // of level 0's, and negative because a wider level starts further back.
        let gained = 1.0 - (level as f32).exp2();
        let spacing = self.level_spacing(0);
        let mut at = [0.0f32; 3];
        for axis in 0..3 {
            let last = self.counts[axis].saturating_sub(1) as f32;
            at[axis] = self.origin[axis] + 0.5 * last * spacing[axis] * gained;
        }
        at
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
    /// holds, and then an origin and a reciprocal spacing per level — every
    /// three-component row leaving its fourth lane as the zero the block starts
    /// as. The rows past [`level_count`](Self::level_count) stay zeroed; the
    /// shader clamps its level against the count and never reads them.
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
/// first — index `level_row(level) + (z · counts.y + y) · counts.x + x`. A row
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
    let first_row = volume.level_row(level);
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
        // In `u64` because the shader's `uint` arithmetic wraps where this
        // would panic, and a wrapped index is then clamped into the table
        // below either way.
        let index = u64::from(first_row)
            + (u64::from(z) * u64::from(volume.counts[1]) + u64::from(y))
                * u64::from(volume.counts[0])
            + u64::from(x);
        let bound = u64::from(volume.total().saturating_sub(1));
        let row = index.min(bound);
        let sh = probes
            .get(usize::try_from(row).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or(GpuProbe::ZERO);
        let weight = visibility.weight(
            u32::try_from(row).unwrap_or(u32::MAX),
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
    #[test]
    fn the_clipmap_header_is_the_counts_the_level_row_and_a_pair_per_level() {
        let volume = ProbeVolume {
            origin: [1.0, 2.0, 3.0],
            inv_spacing: [0.25, 0.5, 0.75],
            counts: [3, 3, 3],
            levels: 2,
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

        // **Level 0 is the fields it was given, bit for bit**, which is what
        // makes a one-level volume the uniform grid this type used to be.
        assert_eq!(
            [
                float_at(origins),
                float_at(origins + 4),
                float_at(origins + 8)
            ],
            volume.origin
        );
        assert_eq!(
            [
                float_at(inv_spacings),
                float_at(inv_spacings + 4),
                float_at(inv_spacings + 8)
            ],
            volume.inv_spacing
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
    /// `mesh.slang` weights its gather by visibility and `ssr.slang` does not,
    /// so the two evaluators are genuinely different code; the *selection* in
    /// front of them is one rule, and a copy of it that drifted would fade the
    /// diffuse and the specular reads between different levels at the same
    /// point. This is `crate::ssr`'s screen-space-helper guard applied to the
    /// clipmap, and it holds the bodies rather than the declarations so each
    /// file's doc comment can say what that file uses them for.
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
                .map(|(name, source)| {
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
                                    return (name, source[open..open + offset + 1].to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                    panic!("{name}'s `{signature}` never closes its body")
                })
                .collect();
            assert_eq!(
                bodies[0].1, bodies[1].1,
                "`{signature}` differs between {} and {}; the clipmap's level \
                 pick is copied verbatim and one copy has drifted",
                bodies[0].0, bodies[1].0
            );
        }
    }
}
