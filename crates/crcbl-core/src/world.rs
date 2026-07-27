//! Sector-tiled world positions.
//!
//! Every absolute position in the engine — simulation, physics, streaming,
//! networking — is a [`WorldPos`]: an integer sector index plus an `f64` offset
//! inside that sector. Nothing anywhere holds an absolute `Vec3`. The only
//! sanctioned way to reach `Vec3` is [`WorldPos::relative_to`], which produces
//! a *camera-relative* render-space vector; see its docs.
//!
//! This lands in stage 1, long before physics, because retrofitting galaxy-
//! scale coordinates into an engine that assumed `Vec3` is a rewrite of every
//! system that touches a position.

use core::fmt;
use core::ops::{Add, AddAssign, Sub, SubAssign};

use glam::{DVec3, I64Vec3, Vec3};

/// Base-2 logarithm of [`SECTOR_SIZE`].
///
/// The sector edge is a power of two so that rebasing is *exact*: dividing an
/// `f64` by a power of two only changes the exponent, `floor` of the result is
/// exact, and `local - k * SECTOR_SIZE` is representable, so no bits are lost
/// when a body crosses a sector boundary. A decimal sector size (say 1e6 m)
/// would round on every crossing and the error would accumulate over a session.
pub const SECTOR_SIZE_LOG2: u32 = 20;

/// Edge length of a sector, in metres: `2^20 m` = 1 048 576 m ≈ 1048.6 km.
///
/// The sector grid is one structure with three consumers — streaming unit,
/// broadphase partition and interest-management key (see
/// `docs/plan/05-physics.md`) — so the cell size is chosen to be a *useful
/// spatial cell first*, and the index widened until the extent follows:
///
/// * **As a cell**: an FPS map or a town sits inside one sector; Earth's
///   diameter spans ~12 of them; a planet's surface is a few hundred; a solar
///   system is a sparse hash of the populated ones. At `2^24 m` (16 777 km) the
///   whole Earth fits in **0.76** of a single cell, which is why this is not
///   that: a cell that cannot subdivide a planet cannot stream one, and the
///   per-sector BVH would degenerate into one global tree.
/// * **Extent**: `I64Vec3` gives `2^64` indices per axis, so the addressable
///   span is `2^64 * 2^20 = 2^84 m` ≈ 1.93e25 m ≈ **2.04 billion light-years**
///   per axis. The Milky Way (~100 000 ly) is a rounding error inside that; the
///   galaxy-scale requirement stopped being the binding constraint the moment
///   the index went 64-bit.
/// * **Precision**: local offsets stay in `[0, 2^20)`, where the worst-case
///   `f64` ULP (at the far corner of a sector, exponent 19) is `2^-33 m` =
///   **0.116 nanometres** — finer than an atomic radius, and ~524 000× finer
///   than the `f32` render space that consumes it 1 km from the camera (61 µm
///   ULP). `WorldPos` is nowhere near being the precision bottleneck.
///
/// Every number above is asserted by tests in this module, so this comment
/// cannot quietly become a lie.
pub const SECTOR_SIZE: f64 = (1u64 << SECTOR_SIZE_LOG2) as f64;

/// Largest `f64` strictly below [`SECTOR_SIZE`] — the canonical local offset at
/// the far edge of a sector, used when a position saturates.
///
/// Decrementing the bit pattern of a positive finite float steps exactly one
/// ULP down, giving `2^20 - 2^-33`. Asserted against `f64::next_down` in the
/// tests.
const LOCAL_MAX: f64 = f64::from_bits(SECTOR_SIZE.to_bits() - 1);

/// Largest sector-index difference whose metre conversion is exact: `2^53`, the
/// point where `i64 -> f64` stops being injective.
///
/// Scaling by [`SECTOR_SIZE`] is exact at *any* magnitude (it only shifts the
/// exponent of a power of two), so the sector term of a delta rounds only when
/// the index difference itself leaves `f64`'s integer range. Public because it
/// is the honest boundary on [`WorldPos::delta`], and code that cares (an
/// interest-management cull, a debug assert) should be able to test against it
/// rather than trust prose. In metres that is `2^73` ≈ 998 000 light-years.
pub const EXACT_SECTOR_DELTA: i64 = 1 << 53;

/// An absolute position: sector index plus a local offset in metres.
///
/// # Canonical form
///
/// A normalized `WorldPos` keeps `local` in `[0, SECTOR_SIZE)` on every axis —
/// a half-open, **non-negative** range rather than a range centred on zero.
/// That makes `sector` literally `floor(absolute / SECTOR_SIZE)`, i.e. the
/// index of the grid cell the position is in, which is exactly what the
/// streaming loader, the broadphase partition and interest management want as a
/// key (one spatial structure, three consumers — see `docs/plan/05-physics.md`).
/// A centred range would buy one bit of precision and cost that identity, plus
/// an off-by-half in every consumer that hashes a cell.
///
/// The fields are public because this is a plain data type, but a hand-built
/// value may be denormalized. Arithmetic through the operators below always
/// returns normalized values; [`WorldPos::new`] normalizes on construction, and
/// [`WorldPos::normalize`] canonicalizes anything.
///
/// # Out-of-range positions
///
/// A position that would leave the addressable volume **saturates** at the
/// boundary sector (`i64::MIN` / `i64::MAX`); it never wraps. Wrapping would
/// teleport a ship from one edge of the universe to the other and every
/// downstream distance test would silently agree with it. Saturation is lossy
/// but monotone, obvious in a debugger, and idempotent under repeated
/// normalization.
#[derive(Clone, Copy, PartialEq, Default)]
pub struct WorldPos {
    /// Sector index on each axis.
    pub sector: I64Vec3,
    /// Offset within the sector, in metres. Normalized to `[0, SECTOR_SIZE)`.
    pub local: DVec3,
}

impl WorldPos {
    /// The origin of sector `(0, 0, 0)`.
    pub const ORIGIN: Self = Self {
        sector: I64Vec3::ZERO,
        local: DVec3::ZERO,
    };

    /// Builds a normalized position from its parts.
    #[inline]
    #[must_use]
    pub fn new(sector: I64Vec3, local: DVec3) -> Self {
        Self { sector, local }.normalize()
    }

    /// Builds a position from an offset relative to the origin of sector
    /// `(0, 0, 0)`.
    ///
    /// Convenient for small worlds and tests. Note that the argument is a plain
    /// `DVec3` *offset*, not a render-space `Vec3`.
    #[inline]
    #[must_use]
    pub fn from_offset(offset: DVec3) -> Self {
        Self::new(I64Vec3::ZERO, offset)
    }

    /// Builds a position at the origin of `sector`.
    #[inline]
    #[must_use]
    pub const fn from_sector(sector: I64Vec3) -> Self {
        Self {
            sector,
            local: DVec3::ZERO,
        }
    }

    /// Returns the canonical form of this position: `local` inside
    /// `[0, SECTOR_SIZE)` with the overflow carried into `sector`.
    ///
    /// Exact — no bits are lost — because [`SECTOR_SIZE`] is a power of two.
    /// Idempotent, including for saturated and non-finite inputs.
    ///
    /// A non-finite `local` is a bug upstream: debug builds assert, release
    /// builds propagate the NaN/infinity rather than inventing a plausible
    /// position.
    #[inline]
    #[must_use]
    pub fn normalize(self) -> Self {
        debug_assert!(
            self.local.is_finite(),
            "WorldPos::local must be finite, got {}",
            self.local
        );
        let (sx, lx) = normalize_axis(self.sector.x, self.local.x);
        let (sy, ly) = normalize_axis(self.sector.y, self.local.y);
        let (sz, lz) = normalize_axis(self.sector.z, self.local.z);
        Self {
            sector: I64Vec3::new(sx, sy, sz),
            local: DVec3::new(lx, ly, lz),
        }
    }

    /// Canonicalizes in place. Same operation as [`WorldPos::normalize`], named
    /// for the physics-side vocabulary ("rebase on sector crossing").
    #[inline]
    pub fn rebase(&mut self) {
        *self = self.normalize();
    }

    /// Whether `local` is already inside `[0, SECTOR_SIZE)` on every axis.
    #[inline]
    #[must_use]
    pub fn is_normalized(&self) -> bool {
        let inside = |v: f64| (0.0..SECTOR_SIZE).contains(&v);
        inside(self.local.x) && inside(self.local.y) && inside(self.local.z)
    }

    /// The vector from `origin` to `self`, in metres.
    ///
    /// This — not any absolute coordinate — is what physics, queries and
    /// rendering actually consume.
    ///
    /// # Precision
    ///
    /// The sector difference is taken in `i128`, so it cannot wrap even between
    /// opposite corners of the addressable volume, and the largest possible
    /// delta (`2^84 m`) is nowhere near `f64` overflow. The result is therefore
    /// always finite and never wrong by wraparound.
    ///
    /// Scaling the index difference by [`SECTOR_SIZE`] is *always* exact — it
    /// only shifts the exponent of a power of two. What is not always exact is
    /// the `i128 -> f64` conversion of the difference itself, which is
    /// injective only below `2^53`. So:
    ///
    /// * **|Δsector| ≤ 2^53** (a separation up to `2^73 m` ≈ **998 000
    ///   light-years** — the Milky Way and its satellites, ten times over): the
    ///   sector term is exact, and the only rounding is the `local` difference
    ///   and the final addition, i.e. sub-nanometre.
    /// * **Beyond that**: the delta degrades to `f64`'s ordinary relative
    ///   precision, about `d * 2^-53` for a separation of `d` metres — `2^31 m`
    ///   ≈ 2.1 million km ≈ 0.014 AU at the full 2.04-billion-light-year span.
    ///   Honest, finite and monotone, but a question nobody should be asking:
    ///   anything that far away should have been culled on sector distance long
    ///   before a metre-valued delta was wanted.
    ///
    /// The boundary is pinned by a test.
    #[inline]
    #[must_use]
    pub fn delta(self, origin: Self) -> DVec3 {
        DVec3::new(
            delta_axis(self.sector.x, self.local.x, origin.sector.x, origin.local.x),
            delta_axis(self.sector.y, self.local.y, origin.sector.y, origin.local.y),
            delta_axis(self.sector.z, self.local.z, origin.sector.z, origin.local.z),
        )
    }

    /// Distance between two positions, in metres.
    #[inline]
    #[must_use]
    pub fn distance(self, other: Self) -> f64 {
        self.delta(other).length()
    }

    /// Squared distance between two positions, in metres².
    ///
    /// Beware the dynamic range: squaring an intergalactic delta stays finite
    /// in `f64` (`(2^84)^2 = 2^168`) but is only useful for comparisons.
    #[inline]
    #[must_use]
    pub fn distance_squared(self, other: Self) -> f64 {
        self.delta(other).length_squared()
    }

    /// Converts to camera-relative **render space**.
    ///
    /// This is the *only* sanctioned path from a `WorldPos` to a `Vec3`
    /// anywhere in the engine. A plain `Vec3` never holds an absolute position:
    /// it is always an offset from the render origin (typically the camera's
    /// `WorldPos`), which is what keeps the GPU's `f32` transforms free of
    /// jitter no matter where in the galaxy the camera is.
    ///
    /// The delta is computed in `f64` and narrowed to `f32` exactly once, here.
    /// Anything far enough away for that narrowing to matter is far enough away
    /// to have been culled.
    #[inline]
    #[must_use]
    pub fn relative_to(self, origin: Self) -> Vec3 {
        self.delta(origin).as_vec3()
    }

    /// Moves by `offset` metres, renormalizing.
    #[inline]
    #[must_use]
    pub fn translated(self, offset: DVec3) -> Self {
        Self {
            sector: self.sector,
            local: self.local + offset,
        }
        .normalize()
    }
}

/// Canonicalizes one axis. See [`WorldPos::normalize`].
#[inline]
fn normalize_axis(sector: i64, local: f64) -> (i64, f64) {
    // Exact for every finite `local`: the divisor is a power of two, so the
    // quotient is exact and `floor` of it is exact. NaN yields NaN, whose
    // `as i128` cast is 0, leaving the position untouched (and idempotent).
    let mut carry = (local / SECTOR_SIZE).floor();
    let mut rest = local - carry * SECTOR_SIZE;
    // `rest` is exact whenever `carry * SECTOR_SIZE` is, which holds across the
    // whole addressable range. The one exception is a `local` so small and
    // negative that `local + SECTOR_SIZE` rounds up to exactly `SECTOR_SIZE`;
    // carry one more sector in that case so the half-open range still holds.
    if rest >= SECTOR_SIZE {
        carry += 1.0;
        rest = 0.0;
    } else if rest < 0.0 {
        // Unreachable with exact arithmetic; clamp rather than emit a
        // denormalized value if a platform ever disagrees.
        rest = 0.0;
    }

    // `i128` so the carry cannot wrap the index: `f64 -> i128` saturates (and
    // maps NaN to 0), and the sum of two `i64`-range values always fits. An
    // out-of-range position then clamps to the edge of the addressable volume
    // instead of wrapping to the far side of it.
    let carried = sector as i128 + carry as i128;
    if carried < i64::MIN as i128 {
        (i64::MIN, 0.0)
    } else if carried > i64::MAX as i128 {
        (i64::MAX, LOCAL_MAX)
    } else {
        (carried as i64, rest)
    }
}

/// One axis of [`WorldPos::delta`].
#[inline]
fn delta_axis(sector: i64, local: f64, origin_sector: i64, origin_local: f64) -> f64 {
    // `i128` keeps `i64::MAX - i64::MIN` from wrapping. The `i128 -> f64` cast
    // is the only lossy step, and only past `2^53` sectors — see
    // [`WorldPos::delta`].
    let sectors = sector as i128 - origin_sector as i128;
    sectors as f64 * SECTOR_SIZE + (local - origin_local)
}

impl Add<DVec3> for WorldPos {
    type Output = Self;

    /// Translation by a metre offset, renormalizing.
    #[inline]
    fn add(self, offset: DVec3) -> Self {
        self.translated(offset)
    }
}

impl AddAssign<DVec3> for WorldPos {
    #[inline]
    fn add_assign(&mut self, offset: DVec3) {
        *self = self.translated(offset);
    }
}

impl Sub<DVec3> for WorldPos {
    type Output = Self;

    #[inline]
    fn sub(self, offset: DVec3) -> Self {
        self.translated(-offset)
    }
}

impl SubAssign<DVec3> for WorldPos {
    #[inline]
    fn sub_assign(&mut self, offset: DVec3) {
        *self = self.translated(-offset);
    }
}

impl Sub<WorldPos> for WorldPos {
    type Output = DVec3;

    /// The relative vector between two positions — see [`WorldPos::delta`].
    #[inline]
    fn sub(self, origin: Self) -> DVec3 {
        self.delta(origin)
    }
}

impl fmt::Debug for WorldPos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WorldPos(sector [{}, {}, {}], local [{}, {}, {}] m)",
            self.sector.x, self.sector.y, self.sector.z, self.local.x, self.local.y, self.local.z
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Metres per light-year (IAU: Julian year x speed of light).
    const METRES_PER_LIGHT_YEAR: f64 = 9_460_730_472_580_800.0;

    #[test]
    fn constructors_agree_and_rebase_matches_normalize() {
        assert_eq!(WorldPos::ORIGIN, WorldPos::default());
        assert_eq!(WorldPos::ORIGIN.sector, I64Vec3::ZERO);
        assert!(WorldPos::ORIGIN.is_normalized());

        let offset = DVec3::new(SECTOR_SIZE * 1.5, -1.0, 42.0);
        let from_offset = WorldPos::from_offset(offset);
        assert_eq!(from_offset.sector, I64Vec3::new(1, -1, 0));
        assert_eq!(from_offset - WorldPos::ORIGIN, offset);
        assert_eq!(WorldPos::ORIGIN.translated(offset), from_offset);
        assert_eq!(WorldPos::ORIGIN + offset, from_offset);

        let sector = I64Vec3::new(-4, 5, 6);
        assert_eq!(WorldPos::from_sector(sector).sector, sector);
        assert_eq!(WorldPos::from_sector(sector).local, DVec3::ZERO);
        assert_eq!(
            WorldPos::new(sector, offset),
            WorldPos::from_sector(sector) + offset
        );

        // `rebase` is `normalize` in place.
        let mut raw = WorldPos {
            sector,
            local: offset,
        };
        let normalized = raw.normalize();
        raw.rebase();
        assert_eq!(raw, normalized);
        assert!(
            !WorldPos {
                sector,
                local: offset
            }
            .is_normalized()
        );

        assert_eq!(
            from_offset.distance_squared(WorldPos::ORIGIN),
            offset.length_squared()
        );
        assert!(format!("{from_offset:?}").starts_with("WorldPos(sector [1, -1, 0]"));
    }

    /// The power-of-two claim in the [`SECTOR_SIZE_LOG2`] docs, and the
    /// consequence that matters: scaling a sector index by `SECTOR_SIZE` is
    /// exact at *every* magnitude, because it only shifts the exponent. With an
    /// odd sector size it would not be, and [`WorldPos::delta`] would round at
    /// sector scale (kilometres) rather than at local scale (nanometres).
    #[test]
    fn sector_size_is_a_power_of_two() {
        // Spelled out rather than computed with `powi`, which is a library
        // function whose last bit is not contractually exact.
        assert_eq!(SECTOR_SIZE, 1_048_576.0);
        assert_eq!(SECTOR_SIZE, (1u64 << SECTOR_SIZE_LOG2) as f64);
        assert_eq!(
            SECTOR_SIZE.to_bits() & ((1 << 52) - 1),
            0,
            "mantissa must be all zeroes"
        );
        assert_eq!(LOCAL_MAX, SECTOR_SIZE.next_down());

        // Exact scaling all the way out to the edge of the addressable volume,
        // where the product needs 84 bits — none of which the multiply can
        // disturb. Every input here is itself exactly representable, which is
        // the point: this isolates the *scaling* from the `i128 -> f64`
        // conversion that the next test pins.
        for sectors in [
            1i128,
            -1,
            12_345_678,
            EXACT_SECTOR_DELTA as i128,
            i64::MIN as i128,
        ] {
            let scaled = sectors as f64 * SECTOR_SIZE;
            assert_eq!(
                scaled as i128,
                sectors << SECTOR_SIZE_LOG2,
                "{sectors} sectors did not scale exactly"
            );
        }
    }

    /// The doc comment on [`SECTOR_SIZE`] claims 0.116 nm at the sector edge,
    /// ~2.04 billion ly of extent, and ~12 sectors across Earth. Assert all of
    /// them, so the claims cannot rot.
    #[test]
    fn documented_precision_and_extent_hold() {
        // Worst-case ULP inside a sector: just below the far edge.
        let edge = SECTOR_SIZE.next_down();
        let ulp = edge - edge.next_down();
        assert_eq!(
            ulp,
            1.0 / 8_589_934_592.0,
            "worst-case local ULP should be 2^-33 m"
        );
        assert!(
            (ulp - 0.116e-9).abs() < 0.001e-9,
            "sector-edge precision should be ~0.116 nm, got {ulp} m"
        );
        // ... and it is far finer than f32 render space 1 km from the camera.
        let f32_ulp_at_1km = (1000.0f32.next_up() - 1000.0f32) as f64;
        assert!(
            (f32_ulp_at_1km / ulp - 524_288.0).abs() < 1.0,
            "should be ~524 000x finer than render space, got {}x",
            f32_ulp_at_1km / ulp
        );

        // Sector size as a *cell*: a town fits in one, Earth spans about a
        // dozen. A cell that cannot subdivide a planet cannot stream one.
        const EARTH_DIAMETER_M: f64 = 12_742_000.0;
        let earth_cells = EARTH_DIAMETER_M / SECTOR_SIZE;
        assert!(
            (12.0..13.0).contains(&earth_cells),
            "Earth should span ~12 sectors, got {earth_cells}"
        );
        const {
            assert!(
                4_000.0 < SECTOR_SIZE,
                "an FPS map should fit inside one sector"
            )
        };

        // Addressable extent per axis: the full i64 sector span.
        let sectors = (i64::MAX as f64 - i64::MIN as f64) + 1.0;
        let extent_ly = sectors * SECTOR_SIZE / METRES_PER_LIGHT_YEAR;
        assert!(
            (2.03e9..2.06e9).contains(&extent_ly),
            "extent should be ~2.04 billion ly, got {extent_ly}"
        );
        assert!(extent_ly > 1e5, "the Milky Way must fit many times over");

        // The largest possible delta stays far from f64 overflow.
        let corner = WorldPos::from_sector(I64Vec3::splat(i64::MAX));
        let opposite = WorldPos::from_sector(I64Vec3::splat(i64::MIN));
        assert!(corner.distance(opposite).is_finite());
        assert!(corner.distance_squared(opposite).is_finite());
    }

    /// Where the delta stops being exact, pinned so the doc comment on
    /// [`WorldPos::delta`] stays true. The sector *scaling* is exact forever
    /// (asserted above); the `i128 -> f64` conversion of the index difference
    /// is what gives out, at `2^53`.
    #[test]
    fn delta_is_exact_out_to_two_to_the_fifty_three_sectors() {
        assert_eq!(EXACT_SECTOR_DELTA as f64 as i64, EXACT_SECTOR_DELTA);
        assert_ne!(
            (EXACT_SECTOR_DELTA + 1) as f64 as i64,
            EXACT_SECTOR_DELTA + 1,
            "2^53 + 1 is the first integer f64 cannot hold — the boundary is here"
        );

        // A separation of exactly 2^53 sectors is still metre-exact.
        let a = WorldPos::from_sector(I64Vec3::new(EXACT_SECTOR_DELTA, 0, 0));
        let b = WorldPos::ORIGIN;
        let delta = a - b;
        assert_eq!(
            delta.x as i128,
            (EXACT_SECTOR_DELTA as i128) << SECTOR_SIZE_LOG2
        );
        // ... and a one-metre nudge at that separation still registers.
        assert_eq!((a + DVec3::X) - b, delta + DVec3::X);

        // The documented reach of that exactness, in light-years.
        let reach_ly = delta.x / METRES_PER_LIGHT_YEAR;
        assert!(
            (998_000.0..999_000.0).contains(&reach_ly),
            "exact-delta reach should be ~998 000 ly, got {reach_ly}"
        );

        // Past it, the answer is lossy but still finite, monotone and of the
        // documented relative magnitude (~d * 2^-52).
        let corner = WorldPos::from_sector(I64Vec3::new(i64::MAX, 0, 0));
        let opposite = WorldPos::from_sector(I64Vec3::new(i64::MIN, 0, 0));
        let full = (corner - opposite).x;
        assert!(full.is_finite() && full > 0.0);
        let quantum = full - full.next_down();
        assert_eq!(
            quantum, 2_147_483_648.0,
            "quantization at the full addressable span should be 2^31 m"
        );
    }

    #[test]
    fn normalize_pulls_local_into_range() {
        let pos = WorldPos {
            sector: I64Vec3::new(1, 1, 1),
            local: DVec3::splat(SECTOR_SIZE * 2.5),
        }
        .normalize();
        assert_eq!(pos.sector, I64Vec3::new(3, 3, 3));
        assert_eq!(pos.local, DVec3::splat(SECTOR_SIZE * 0.5));
        assert!(pos.is_normalized());
    }

    #[test]
    fn normalize_handles_negative_offsets() {
        let pos = WorldPos {
            sector: I64Vec3::ZERO,
            local: DVec3::new(-1.0, -SECTOR_SIZE, 0.0),
        }
        .normalize();
        assert_eq!(pos.sector, I64Vec3::new(-1, -1, 0));
        assert_eq!(pos.local, DVec3::new(SECTOR_SIZE - 1.0, 0.0, 0.0));
        assert!(pos.is_normalized());
    }

    /// A tiny negative offset is the classic rebase edge case: `local +
    /// SECTOR_SIZE` rounds to exactly `SECTOR_SIZE`, which is outside the
    /// half-open range.
    #[test]
    fn normalize_keeps_the_half_open_range_at_the_boundary() {
        let pos = WorldPos {
            sector: I64Vec3::ZERO,
            local: DVec3::new(-1e-300, -1.0, 0.0),
        }
        .normalize();
        assert!(pos.is_normalized(), "{pos:?}");
        assert_eq!(pos.sector, I64Vec3::new(0, -1, 0));
        assert_eq!(pos.local.x, 0.0);
    }

    #[test]
    fn out_of_range_saturates_instead_of_wrapping() {
        let far = WorldPos {
            sector: I64Vec3::splat(i64::MAX),
            local: DVec3::splat(SECTOR_SIZE * 4.0),
        }
        .normalize();
        assert_eq!(far.sector, I64Vec3::splat(i64::MAX));
        assert!(far.is_normalized());
        assert_eq!(far, far.normalize(), "saturation must be idempotent");

        let near = WorldPos {
            sector: I64Vec3::splat(i64::MIN),
            local: DVec3::splat(-SECTOR_SIZE),
        }
        .normalize();
        assert_eq!(near.sector, I64Vec3::splat(i64::MIN));
        assert_eq!(near.local, DVec3::ZERO);
        assert_eq!(near, near.normalize());
    }

    #[test]
    fn non_finite_local_survives_normalization_without_inventing_a_position() {
        // The debug assert fires in debug builds; check the release behaviour
        // by driving the axis helper directly.
        let (sector, local) = normalize_axis(7, f64::NAN);
        assert_eq!(sector, 7);
        assert!(local.is_nan());
        let (sector, local) = normalize_axis(0, f64::INFINITY);
        assert_eq!(sector, i64::MAX, "infinity saturates rather than wraps");
        assert_eq!(local, LOCAL_MAX);
    }

    #[test]
    fn delta_is_exact_across_a_sector_boundary() {
        let a = WorldPos::new(I64Vec3::ZERO, DVec3::new(SECTOR_SIZE - 1.0, 0.0, 0.0));
        let b = a + DVec3::new(2.0, 0.0, 0.0);
        assert_eq!(b.sector, I64Vec3::new(1, 0, 0));
        assert_eq!(b.local.x, 1.0);
        assert_eq!(b - a, DVec3::new(2.0, 0.0, 0.0));
        assert_eq!(a.distance(b), 2.0);
    }

    #[test]
    fn delta_does_not_wrap_between_opposite_corners() {
        let a = WorldPos::from_sector(I64Vec3::new(i64::MAX, 0, 0));
        let b = WorldPos::from_sector(I64Vec3::new(i64::MIN, 0, 0));
        let delta = a - b;
        assert!(delta.x > 0.0, "i64 subtraction would have wrapped negative");
        assert_eq!(delta.x, (i64::MAX as f64 - i64::MIN as f64) * SECTOR_SIZE);
    }

    #[test]
    fn relative_to_is_the_render_space_conversion() {
        let camera = WorldPos::new(I64Vec3::new(12, -3, 7), DVec3::new(1000.0, 2000.0, 3000.0));
        let object = camera + DVec3::new(1.5, -2.5, 0.25);
        assert_eq!(object.relative_to(camera), Vec3::new(1.5, -2.5, 0.25));
        // Same answer regardless of how far from the origin the pair sits.
        let far = WorldPos::new(I64Vec3::splat(1_000_000), DVec3::splat(SECTOR_SIZE - 10.0));
        let far_object = far + DVec3::new(1.5, -2.5, 0.25);
        assert_eq!(far_object.relative_to(far), Vec3::new(1.5, -2.5, 0.25));
    }

    #[test]
    fn assign_operators_match_the_value_forms() {
        let base = WorldPos::new(I64Vec3::new(2, 2, 2), DVec3::splat(10.0));
        let offset = DVec3::new(3.0, -4.0, 5.0);
        let mut moved = base;
        moved += offset;
        assert_eq!(moved, base + offset);
        moved -= offset;
        assert_eq!(moved, base);
        assert_eq!(base - offset, base + -offset);
    }

    // --- property tests -------------------------------------------------

    /// Sector indices well inside the i64 range so nothing saturates, and local
    /// offsets spanning the whole sector.
    fn any_pos() -> impl Strategy<Value = WorldPos> {
        (
            -1_000_000i64..1_000_000,
            -1_000_000i64..1_000_000,
            -1_000_000i64..1_000_000,
            0.0..SECTOR_SIZE,
            0.0..SECTOR_SIZE,
            0.0..SECTOR_SIZE,
        )
            .prop_map(|(sx, sy, sz, lx, ly, lz)| {
                WorldPos::new(I64Vec3::new(sx, sy, sz), DVec3::new(lx, ly, lz))
            })
    }

    /// Worst-case rounding error of one `local + offset` addition.
    ///
    /// The addition happens at the magnitude of the *result*, which is at most
    /// one sector plus the offset, so it rounds to that magnitude's ULP — not
    /// to the ULP of the offset. That is inherent to storing a position as a
    /// float, and it is exactly why the sector is bounded: a translation is
    /// accurate to a fraction of a nanometre anywhere in the addressable
    /// volume, whereas an absolute f64 universe coordinate would round to
    /// kilometres.
    /// `f64::EPSILON` is `2^-52`, twice the half-ULP bound, which covers the
    /// rebase subtraction too.
    fn step_bound(offset: DVec3) -> f64 {
        (SECTOR_SIZE + offset.abs().max_element()) * f64::EPSILON
    }

    /// Offsets across the magnitudes a simulation actually produces: sub-
    /// millimetre nudges up to multi-sector jumps.
    fn any_offset() -> impl Strategy<Value = DVec3> {
        let axis = prop_oneof![
            -1e-3..1e-3f64,
            -1e3..1e3f64,
            -1e9..1e9f64,
            -4.0 * SECTOR_SIZE..4.0 * SECTOR_SIZE,
        ];
        (axis.clone(), axis.clone(), axis).prop_map(|(x, y, z)| DVec3::new(x, y, z))
    }

    proptest! {
        #[test]
        fn prop_normalize_is_idempotent(pos in any_pos(), offset in any_offset()) {
            let raw = WorldPos { sector: pos.sector, local: pos.local + offset };
            let once = raw.normalize();
            prop_assert!(once.is_normalized(), "{once:?}");
            prop_assert_eq!(once, once.normalize());
        }

        /// Rebasing must not move the point — *exactly*. This is the property
        /// that justifies the power-of-two sector size: the delta between a
        /// denormalized position and its canonical form is bit-for-bit zero.
        #[test]
        fn prop_normalize_does_not_move_the_point(pos in any_pos(), offset in any_offset()) {
            let raw = WorldPos { sector: pos.sector, local: pos.local + offset };
            prop_assert_eq!(raw.normalize().delta(raw), DVec3::ZERO);
        }

        /// The same exactness claim, but with a sector carry of up to `10^15`
        /// instead of single digits — offsets out to 1e21 m (~100 000 ly) in a
        /// single un-rebased jump, which is what an on-rails body handed a
        /// season's worth of accumulated motion looks like. `carry *
        /// SECTOR_SIZE` needs ~70 significant bits there, and only stays exact
        /// because the low 20 are zero. Exercises the `floor`, the saturating
        /// `f64 -> i128` cast and the large-carry subtraction together.
        #[test]
        fn prop_normalize_is_exact_at_interstellar_offsets(
            sector in -1_000_000_000i64..1_000_000_000,
            local in -1e21..1e21f64,
        ) {
            let raw = WorldPos { sector: I64Vec3::splat(sector), local: DVec3::splat(local) };
            let normalized = raw.normalize();
            prop_assert!(normalized.is_normalized(), "{normalized:?}");
            prop_assert_eq!(normalized.delta(raw), DVec3::ZERO);
        }

        /// The round-trip that every physics step relies on.
        #[test]
        fn prop_add_then_sub_returns_the_offset(pos in any_pos(), offset in any_offset()) {
            let recovered = (pos + offset) - pos;
            // Not bit-exact, and honestly so: `local + offset` rounds at the
            // magnitude of the sum before any rebasing happens. The rebase
            // itself contributes nothing.
            let tolerance = step_bound(offset);
            prop_assert!(
                (recovered - offset).abs().max_element() <= tolerance,
                "recovered {recovered} vs {offset}, tolerance {tolerance}"
            );
        }

        #[test]
        fn prop_delta_is_antisymmetric(a in any_pos(), b in any_pos()) {
            prop_assert_eq!(a - b, -(b - a));
            prop_assert_eq!(a.distance(b), b.distance(a));
        }

        #[test]
        fn prop_distance_matches_delta_length(a in any_pos(), b in any_pos()) {
            prop_assert_eq!(a.distance(b), (a - b).length());
            let d2 = a.distance_squared(b);
            prop_assert!((d2 - a.distance(b).powi(2)).abs() <= d2 * 1e-12);
        }

        /// The accumulation test: a random walk of N steps must land exactly
        /// where the single summed step lands. Any rounding introduced by a
        /// sector crossing would show up here as drift.
        #[test]
        fn prop_random_walk_matches_single_step(
            start in any_pos(),
            steps in proptest::collection::vec(-1e6..1e6f64, 3..90),
        ) {
            let steps: Vec<DVec3> = steps
                .chunks_exact(3)
                .map(|c| DVec3::new(c[0], c[1], c[2]))
                .collect();

            let mut walked = start;
            for step in &steps {
                walked += *step;
            }

            // Sum the steps in f64 first, then apply once. The two paths agree
            // exactly only because rebasing is exact; the sum itself rounds
            // like any f64 addition chain, so compare against the same order.
            let mut total = DVec3::ZERO;
            for step in &steps {
                total += *step;
            }
            let jumped = start + total;

            // Bounded by one rounding per step — and *not* by anything the
            // rebase does. A rebase that lost bits would blow this up by orders
            // of magnitude the moment the walk crossed a sector boundary.
            let drift = (walked - jumped).abs().max_element();
            let bound: f64 = steps.iter().copied().map(step_bound).sum::<f64>()
                + step_bound(total);
            prop_assert!(drift <= bound, "drift {drift} m over {} steps", steps.len());
        }

        /// A walk that retraces its steps returns to where it started, to
        /// within the same per-step rounding bound.
        #[test]
        fn prop_walk_and_back_returns_home(
            start in any_pos(),
            steps in proptest::collection::vec(any_offset(), 1..24),
        ) {
            let mut pos = start;
            for step in &steps {
                pos += *step;
            }
            for step in steps.iter().rev() {
                pos -= *step;
            }
            let drift = (pos - start).abs().max_element();
            // Every step is applied twice, out and back.
            let bound: f64 = 2.0 * steps.iter().copied().map(step_bound).sum::<f64>();
            prop_assert!(drift <= bound, "drift {drift} m, bound {bound}");
        }

        #[test]
        fn prop_relative_to_agrees_with_delta(a in any_pos(), offset in any_offset()) {
            let b = a + offset;
            prop_assert_eq!(b.relative_to(a), (b - a).as_vec3());
        }
    }
}
