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
//! # A zero volume adds exactly zero
//!
//! [`ProbeVolume::default`](crate::probe::ProbeVolume::default) is the grid a
//! scene with no probes uploads: no
//! probes on any axis, so every fetch resolves to row 0, and a row of zeroes
//! dots to zero whatever the normal. `x + 0 == x` exactly on every target, so a
//! frame drawn with no probes is bit-identical to one drawn before this module
//! existed — which is what lets the data path land with no golden re-blessed.

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

/// Bytes the grid header occupies inside
/// [`FrameUniforms`](crate::mesh::FrameUniforms).
///
/// Two `float4` and a `uint4`. `std140` gives each of them one sixteen-byte
/// row, and the total is already a multiple of sixteen, so there is no tail
/// padding to write.
pub const PROBE_VOLUME_SIZE: usize = 48;

/// The irradiance transfer coefficient of the constant band — Ramamoorthi &
/// Hanrahan 2001's `Â₀`, which is `π`.
///
/// `Â_l = 2π ∫₀¹ P_l(t) t dt`, the clamped cosine's zonal-harmonic expansion,
/// and `P₀(t) = 1` gives `2π · ½`. The checkable consequence is that a constant
/// environment of radiance `L` reaches a surface as an irradiance of `πL` from
/// every direction — this module's
/// `a_constant_environment_integrates_to_pi_times_its_radiance` is that
/// statement as a test.
const TRANSFER_L0: f32 = std::f32::consts::PI;

/// The same for the linear band — `Â₁`, which is `2π/3`.
///
/// `P₁(t) = t` gives `2π ∫₀¹ t² dt`. Its checkable consequence is that a
/// radiance field `L(ω) = b + c(ω·u)` — which L1 represents *exactly*, so no
/// truncation error stands between the two — reaches a surface as
/// `E(n) = πb + (2π/3)c(u·n)`.
const TRANSFER_L1: f32 = 2.0 * std::f32::consts::PI / 3.0;

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

/// The grid the probes are laid out on, matching the `probe_*` members of
/// `struct FrameUniforms` in `shaders/mesh.slang`.
///
/// A uniform grid rather than a 3D texture, and the reason is in
/// `docs/plan/18-render-features.md`: hardware trilinear filter weights are
/// vendor tables, which is the exact class of filtered read the occlusion and
/// reflection designs spent their determinism arguments avoiding. An eight-tap
/// manual interpolation over a storage buffer costs less and risks nothing.
///
/// [`Default`] is the **degenerate volume**: no probes on any axis, which
/// resolves every fetch to row 0 and — with that row zeroed — adds exactly
/// nothing. It is what a scene with no probes uploads, and there is no branch
/// anywhere selecting it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProbeVolume {
    /// Where probe `(0, 0, 0)` is, in world space.
    pub origin: [f32; 3],
    /// One over the grid spacing on each axis, in world units.
    ///
    /// **Inverted host-side** so the shader multiplies where it would otherwise
    /// divide, once per fragment. An axis whose spacing is zero has no
    /// reciprocal and the honest spelling of it is this field's zero, which is
    /// what [`Default`] leaves and what collapses the grid onto probe 0.
    pub inv_spacing: [f32; 3],
    /// How many probes the grid holds on each axis.
    ///
    /// A count of zero and a count of one behave alike — both address probe 0
    /// and interpolate nothing — because the last addressable probe on an axis
    /// is `max(count - 1, 0)` either way.
    pub counts: [u32; 3],
}

impl ProbeVolume {
    /// How many probes the grid holds: the product of its counts, saturating.
    ///
    /// **Not a field**, so it cannot disagree with [`counts`](Self::counts). It
    /// is written into the header's spare `uint` lane all the same, because the
    /// shader clamps every fetch against it — the second line of defence that
    /// makes a row read a fact about the buffer rather than a promise about the
    /// host.
    #[must_use]
    pub fn total(&self) -> u32 {
        self.counts[0]
            .saturating_mul(self.counts[1])
            .saturating_mul(self.counts[2])
    }

    /// The bytes the grid header occupies in the frame block, in `std140`
    /// order.
    ///
    /// Three sixteen-byte rows: the origin and the reciprocal spacing each
    /// leave their fourth lane as the zero the block starts as, and the counts'
    /// fourth lane carries [`total`](Self::total).
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PROBE_VOLUME_SIZE] {
        let mut bytes = [0u8; PROBE_VOLUME_SIZE];
        let mut at = 0usize;
        for row in [self.origin, self.inv_spacing] {
            for value in row {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
            // The lane `std140` aligns a three-component vector to sixteen
            // bytes with, and which nothing reads.
            at += 4;
        }
        for value in self.counts.into_iter().chain([self.total()]) {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        debug_assert_eq!(at, PROBE_VOLUME_SIZE);
        bytes
    }
}

/// The irradiance the volume gives a surface at `world_position` facing
/// `normal`, in linear RGB — **the Rust mirror of `probe_irradiance` in
/// `shaders/mesh.slang`**.
///
/// Trilinear over the eight probes surrounding the point, and the interpolation
/// is over the *coefficients* rather than over eight evaluations of them: that
/// is what makes the whole lookup three dot products, seven blends and one
/// clamp, whatever the grid's size.
///
/// A point outside the grid clamps to its surface rather than fading out, which
/// is the same choice the shadow atlas's sampler makes at a cascade's edge: an
/// environment does not stop at the last probe, and fading to zero there would
/// put a visible seam in mid-air.
///
/// `probes` is the table in `x`-fastest order — index
/// `(z · counts.y + y) · counts.x + x`. A row past its end reads as
/// [`GpuProbe::ZERO`], which is what the device sees too: the table is created
/// with at least one zeroed row, so the degenerate volume's fetch of row 0 is
/// in bounds there and a scene whose probe count disagrees with its volume is
/// refused before it is uploaded at all.
#[must_use]
pub fn irradiance_at(
    volume: &ProbeVolume,
    probes: &[GpuProbe],
    world_position: [f32; 3],
    normal: [f32; 3],
) -> [f32; 3] {
    // The last addressable probe on each axis, which is zero for an axis with
    // no probes at all — so the degenerate volume collapses onto probe 0
    // instead of addressing probe -1.
    let last = volume.counts.map(|count| count.saturating_sub(1) as f32);
    let mut cell = [0u32; 3];
    let mut fraction = [0.0f32; 3];
    for axis in 0..3 {
        let grid = ((world_position[axis] - volume.origin[axis]) * volume.inv_spacing[axis])
            .clamp(0.0, last[axis]);
        let base = grid.floor();
        cell[axis] = base as u32;
        fraction[axis] = grid - base;
    }

    let row = |x: u32, y: u32, z: u32| -> GpuProbe {
        // In `u64` because the shader's `uint` arithmetic wraps where this
        // would panic, and a wrapped index is then clamped into the table
        // below either way.
        let index = (u64::from(z) * u64::from(volume.counts[1]) + u64::from(y))
            * u64::from(volume.counts[0])
            + u64::from(x);
        let bound = u64::from(volume.total().saturating_sub(1));
        probes
            .get(usize::try_from(index.min(bound)).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or(GpuProbe::ZERO)
    };
    // `min` against the axis's last probe rather than `+ 1` outright: at the
    // far face of the grid there is no next probe, and the fraction there is
    // zero, so the corner is weighed out rather than wrapped around.
    let next = [
        (cell[0] + 1).min(last[0] as u32),
        (cell[1] + 1).min(last[1] as u32),
        (cell[2] + 1).min(last[2] as u32),
    ];

    let x0 = row(cell[0], cell[1], cell[2]).lerp(row(next[0], cell[1], cell[2]), fraction[0]);
    let x1 = row(cell[0], next[1], cell[2]).lerp(row(next[0], next[1], cell[2]), fraction[0]);
    let x2 = row(cell[0], cell[1], next[2]).lerp(row(next[0], cell[1], next[2]), fraction[0]);
    let x3 = row(cell[0], next[1], next[2]).lerp(row(next[0], next[1], next[2]), fraction[0]);
    let y0 = x0.lerp(x1, fraction[1]);
    let y1 = x2.lerp(x3, fraction[1]);
    y0.lerp(y1, fraction[2]).irradiance(normal)
}

#[cfg(test)]
mod tests {
    use super::*;
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
                    irradiance_at(&volume, &[], position, normal),
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
            irradiance_at(&volume, &[bright], [3.0, 4.0, 5.0], [0.0, 0.0, 1.0]),
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
            irradiance_at(&volume, &probes, [1.0, 0.0, 0.0], normal),
            probes[1].irradiance(normal)
        );
        // And the two sides converge on it rather than jumping.
        let below = irradiance_at(&volume, &probes, [1.0 - 1e-6, 0.0, 0.0], normal);
        let above = irradiance_at(&volume, &probes, [1.0 + 1e-6, 0.0, 0.0], normal);
        for channel in 0..3 {
            assert!(
                (below[channel] - above[channel]).abs() < 1e-4,
                "channel {channel}: {below:?} against {above:?}"
            );
        }
        // The grid really is interpolating, or the two assertions above would
        // hold for a lookup that ignored the position entirely.
        assert_ne!(
            irradiance_at(&volume, &probes, [0.5, 0.0, 0.0], normal),
            probes[0].irradiance(normal)
        );
        assert_ne!(
            irradiance_at(&volume, &probes, [0.5, 0.0, 0.0], normal),
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
        let at =
            |x: f32, y: f32, z: f32| irradiance_at(&volume, &probes, [x, y, z], [1.0, 0.0, 0.0])[0];
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

    /// The grid header's three rows, at the offsets the shader's block puts
    /// them — including the count lane the shader clamps its fetches against.
    #[test]
    fn the_grid_header_is_two_padded_rows_and_the_counts() {
        let volume = ProbeVolume {
            origin: [1.0, 2.0, 3.0],
            inv_spacing: [0.25, 0.5, 0.75],
            counts: [2, 3, 4],
        };
        let bytes = volume.to_bytes();
        assert_eq!(bytes.len(), PROBE_VOLUME_SIZE);
        let float_at = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().expect("4"));
        let word_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4"));
        assert_eq!([float_at(0), float_at(4), float_at(8)], volume.origin);
        assert_eq!(float_at(12), 0.0, "the origin's fourth lane is padding");
        assert_eq!(
            [float_at(16), float_at(20), float_at(24)],
            volume.inv_spacing
        );
        assert_eq!(float_at(28), 0.0, "the spacing's fourth lane is padding");
        assert_eq!([word_at(32), word_at(36), word_at(40)], volume.counts);
        assert_eq!(word_at(44), 24, "the counts' fourth lane carries the total");
        assert_eq!(
            ProbeVolume::default().to_bytes(),
            [0u8; PROBE_VOLUME_SIZE],
            "the degenerate volume is a block of zeroes, which is what an \
             unwritten uniform block already holds"
        );
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
    /// shader's block puts the three rows in the same order — swapping the
    /// origin and the spacing produces a frame lit from somewhere else, which
    /// is a picture.
    #[test]
    fn the_grid_header_matches_the_block_mesh_slang_declares() {
        let source = include_str!("../shaders/mesh.slang");
        let declarations = [
            "float4 probe_origin;",
            "float4 probe_inv_spacing;",
            "uint4 probe_counts;",
        ];
        let mut previous = 0;
        for declaration in declarations {
            let at = source
                .find(declaration)
                .unwrap_or_else(|| panic!("mesh.slang does not declare `{declaration}`"));
            assert!(
                at > previous,
                "mesh.slang declares `{declaration}` in a different order than \
                 `to_bytes` writes it"
            );
            previous = at;
        }
        // And the row itself, whose three channels are what `PROBE_STRIDE`
        // measures.
        for declaration in ["float4 sh_r;", "float4 sh_g;", "float4 sh_b;"] {
            assert!(
                source.contains(declaration),
                "mesh.slang does not declare `{declaration}`"
            );
        }
    }
}
