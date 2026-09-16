//! The two authored layers, and the bilinear tap that reads one.
//!
//! `docs/plan/56-wind.md`'s decision 1: a coarse **direction** layer holding a
//! horizontal vector per texel — "stored as a vector, never an angle" — and a
//! finer **intensity** layer holding a speed multiplier, "zero meaning calm".
//! Both arrive as ordinary RGBA8 images; [`crate::load`] is what turns an asset
//! into one.
//!
//! # Why the filter is written here rather than borrowed
//!
//! The GPU half of this rung reads the same two images through a linear sampler
//! in `repeat` addressing, and the CPU copy is the authoritative one
//! (decision 6). So this module is the CPU statement of what that sampler does:
//! texel centres at half-integer texture coordinates, the four neighbours
//! wrapped into range, and the weights the two fractional parts give. Anything
//! else here would be a disagreement with hardware that no amount of tolerance
//! in the agreement test could absorb, because it would be a disagreement about
//! *which texels*.

use glam::DVec2;

use crate::WindError;

/// The largest layer this module will decode, in texels.
///
/// A guard on the product, applied before a `Vec` is reserved, so a caller that
/// hands over a plausible-looking width and height cannot ask for an allocation
/// this host answers by aborting. 4096² is four times the largest layer
/// decision 1 contemplates (a 256² direction layer at 8–16 m per texel covers
/// two to four kilometres).
pub const MAX_TEXELS: u64 = 4096 * 4096;

/// Where a layer's texels sit in the world.
///
/// The grid is over the **XZ plane** — `docs/plan/56-wind.md`'s field is
/// horizontal, and [`crate::WindField::sample`] drops `y` on the way in. It
/// repeats in both axes, so a layer covers the whole world however small it is;
/// what changes with size is how far you travel before the pattern comes round.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerGrid {
    /// Texels along +X.
    pub width: u32,
    /// Texels along +Z.
    pub height: u32,
    /// Metres of world per texel, on both axes.
    ///
    /// Held as an `f64` bit pattern would be, but validated on construction:
    /// see [`LayerGrid::new`].
    metres_per_texel: f64,
}

impl LayerGrid {
    /// Describes a grid of `width × height` texels at `metres_per_texel`.
    ///
    /// # Errors
    ///
    /// [`WindError::EmptyLayer`] if either dimension is zero, or the two
    /// multiply past [`MAX_TEXELS`]; [`WindError::TexelScale`] if
    /// `metres_per_texel` is not a positive finite number.
    pub fn new(width: u32, height: u32, metres_per_texel: f64) -> Result<Self, WindError> {
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_TEXELS {
            return Err(WindError::EmptyLayer { width, height });
        }
        if !metres_per_texel.is_finite() || metres_per_texel <= 0.0 {
            return Err(WindError::TexelScale { metres_per_texel });
        }
        Ok(Self {
            width,
            height,
            metres_per_texel,
        })
    }

    /// Metres of world per texel.
    #[inline]
    #[must_use]
    pub const fn metres_per_texel(self) -> f64 {
        self.metres_per_texel
    }

    /// Metres the grid covers before it repeats, along X then Z.
    #[inline]
    #[must_use]
    pub fn extent_metres(self) -> DVec2 {
        DVec2::new(
            f64::from(self.width) * self.metres_per_texel,
            f64::from(self.height) * self.metres_per_texel,
        )
    }

    /// Texture coordinates per metre, along X then Z.
    ///
    /// The reciprocal of [`Self::extent_metres`], and what the GPU half adds a
    /// camera-relative position by — see [`crate::WindField::gpu_params`].
    #[inline]
    #[must_use]
    pub fn uv_per_metre(self) -> DVec2 {
        let extent = self.extent_metres();
        DVec2::new(1.0 / extent.x, 1.0 / extent.y)
    }

    /// Texture coordinates of a world position, wrapped into `0..1`.
    #[inline]
    #[must_use]
    pub fn uv(self, x: f64, z: f64) -> DVec2 {
        let unwrapped = DVec2::new(x, z) * self.uv_per_metre();
        DVec2::new(fract(unwrapped.x), fract(unwrapped.y))
    }

    /// Texels this grid holds.
    #[inline]
    #[must_use]
    pub const fn texel_count(self) -> usize {
        self.width as usize * self.height as usize
    }

    /// The four texels a bilinear tap at `(x, z)` reads, and their weights.
    ///
    /// Texel centres sit at half-integer texture coordinates, which is why the
    /// half is subtracted before the floor: a sample exactly on a centre must
    /// weight that texel 1 and its neighbours 0, and without the shift it would
    /// straddle two. Indices are wrapped with a Euclidean remainder, so a
    /// negative world coordinate reads the same texels the positive one a whole
    /// extent away does — which is what `repeat` addressing means.
    #[must_use]
    fn taps(self, x: f64, z: f64) -> Taps {
        let u = x / self.metres_per_texel - 0.5;
        let v = z / self.metres_per_texel - 0.5;
        let (u0, fx) = (u.floor(), fract(u));
        let (v0, fz) = (v.floor(), fract(v));
        let column = |offset: f64| wrap(u0 + offset, self.width);
        let row = |offset: f64| wrap(v0 + offset, self.height);
        let (x0, x1) = (column(0.0), column(1.0));
        let (z0, z1) = (row(0.0), row(1.0));
        let width = self.width as usize;
        Taps {
            index: [
                z0 * width + x0,
                z0 * width + x1,
                z1 * width + x0,
                z1 * width + x1,
            ],
            weight: [
                (1.0 - fx) * (1.0 - fz),
                fx * (1.0 - fz),
                (1.0 - fx) * fz,
                fx * fz,
            ],
        }
    }
}

/// The four texels one bilinear tap reads, and what each is worth.
struct Taps {
    /// Flat indices, in the order `(x0,z0)`, `(x1,z0)`, `(x0,z1)`, `(x1,z1)`.
    index: [usize; 4],
    /// Weights in the same order; they sum to one.
    weight: [f64; 4],
}

/// The fractional part of `value`, always in `0..1` — `value - floor(value)`.
///
/// `f64::fract` is not this: it keeps the sign, so `(-0.25).fract()` is `-0.25`
/// and a texture coordinate built from it would read the wrong texel on the
/// negative side of the origin. Both `floor` and the subtraction are exact
/// operations IEEE-754 pins down, so this is the same number on every target.
#[inline]
#[must_use]
pub fn fract(value: f64) -> f64 {
    let fraction = value - value.floor();
    // `value - value.floor()` can round up to exactly 1 for a `value` just
    // below an integer, whose floor is then a whole unit away. Clamping keeps
    // the half-open range this function promises.
    if fraction >= 1.0 { 0.0 } else { fraction }
}

/// Wraps a texel coordinate into `0..limit`.
fn wrap(coordinate: f64, limit: u32) -> usize {
    let limit = i64::from(limit);
    // `as` saturates rather than wrapping, and a coordinate that saturated is
    // one `rem_euclid` still maps into range — so there is no index to bound
    // afterwards.
    let wrapped = (coordinate as i64).rem_euclid(limit);
    wrapped as usize
}

/// The coarse layer: a horizontal vector per texel, filtered as a vector.
///
/// Decision 1 is emphatic that this is not an angle, and the reason survives
/// into the CPU copy: the average of two vectors is a vector, and the average
/// of two angles is not — 350° and 10° average to 180°, the exact opposite of
/// the answer. What a texel holds is a **deflection** from the weather's base
/// direction, composed with it by [`crate::WindField::sample`]; `(1, 0)` is "no
/// deflection", so a blank layer reproduces the prevailing wind everywhere.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectionLayer {
    grid: LayerGrid,
    /// One deflection per texel, row-major from `-Z`, held at the precision the
    /// GPU's unorm conversion produces so the two read the same numbers.
    texels: Vec<[f32; 2]>,
}

impl DirectionLayer {
    /// Decodes a direction layer from tightly packed RGBA8.
    ///
    /// Red carries the deflection's X and green its Z, each mapped from `0..1`
    /// to `-1..1` — the ordinary tangent-space normal-map encoding, so an
    /// authoring tool that can write a normal map can write this. Blue and
    /// alpha are unread; decision 1 lists an updraft bias and a turbulence
    /// scale as candidates for them, and neither is decided.
    ///
    /// # Errors
    ///
    /// [`WindError::LayerSize`] if `pixels` is not exactly four bytes per texel
    /// of `grid`.
    pub fn from_rgba8(grid: LayerGrid, pixels: &[u8]) -> Result<Self, WindError> {
        let texels = decode(grid, pixels, |rgba| {
            [unorm_to_signed(rgba[0]), unorm_to_signed(rgba[1])]
        })?;
        Ok(Self { grid, texels })
    }

    /// The grid this layer's texels sit on.
    #[inline]
    #[must_use]
    pub const fn grid(&self) -> LayerGrid {
        self.grid
    }

    /// The bilinearly filtered deflection at a world position on the XZ plane.
    ///
    /// **Not renormalised here.** The filtered vector is shorter than its
    /// neighbours wherever they disagree, and decision 1 budgets one square
    /// root for the renormalise *after* the tap — which is where
    /// [`crate::WindField::sample`] spends it, once, on the composed direction
    /// rather than twice.
    #[must_use]
    pub fn sample(&self, x: f64, z: f64) -> DVec2 {
        let taps = self.grid.taps(x, z);
        let mut sum = DVec2::ZERO;
        for (index, weight) in taps.index.into_iter().zip(taps.weight) {
            let texel = self.texels[index];
            sum += DVec2::new(f64::from(texel[0]), f64::from(texel[1])) * weight;
        }
        sum
    }
}

/// The fine layer: a speed multiplier per texel, zero meaning calm.
#[derive(Debug, Clone, PartialEq)]
pub struct IntensityLayer {
    grid: LayerGrid,
    /// One multiplier per texel in `0..=1`, row-major from `-Z`.
    texels: Vec<f32>,
}

impl IntensityLayer {
    /// Decodes an intensity layer from tightly packed RGBA8.
    ///
    /// Red carries the multiplier over `0..=1`; green, blue and alpha are
    /// unread, and decision 1's gust susceptibility and shelter term are the
    /// candidates for them.
    ///
    /// # Errors
    ///
    /// [`WindError::LayerSize`] if `pixels` is not exactly four bytes per texel
    /// of `grid`.
    pub fn from_rgba8(grid: LayerGrid, pixels: &[u8]) -> Result<Self, WindError> {
        let texels = decode(grid, pixels, |rgba| unorm(rgba[0]))?;
        Ok(Self { grid, texels })
    }

    /// The grid this layer's texels sit on.
    #[inline]
    #[must_use]
    pub const fn grid(&self) -> LayerGrid {
        self.grid
    }

    /// The bilinearly filtered multiplier at a world position on the XZ plane.
    #[must_use]
    pub fn sample(&self, x: f64, z: f64) -> f64 {
        let taps = self.grid.taps(x, z);
        let mut sum = 0.0;
        for (index, weight) in taps.index.into_iter().zip(taps.weight) {
            sum += f64::from(self.texels[index]) * weight;
        }
        sum
    }
}

/// Turns RGBA8 into one texel each, refusing a buffer that is the wrong size.
fn decode<T>(
    grid: LayerGrid,
    pixels: &[u8],
    texel: impl Fn(&[u8]) -> T,
) -> Result<Vec<T>, WindError> {
    let expected = grid.texel_count() * 4;
    if pixels.len() != expected {
        return Err(WindError::LayerSize {
            width: grid.width,
            height: grid.height,
            expected,
            found: pixels.len(),
        });
    }
    Ok(pixels.chunks_exact(4).map(texel).collect())
}

/// One unorm byte as the `f32` a GPU's `rgba8unorm` fetch produces.
///
/// The division by 255 is the conversion Vulkan, Metal, D3D12 and WebGPU all
/// specify, so this is not an approximation of what the sampler does — it is
/// the same arithmetic, and the two agree exactly at a texel centre.
#[inline]
#[must_use]
fn unorm(byte: u8) -> f32 {
    f32::from(byte) / 255.0
}

/// One unorm byte mapped to `-1..1`, the ordinary normal-map encoding.
///
/// 128 is `0.00392` rather than exactly zero: an eight-bit unorm cannot hold
/// the midpoint of a symmetric range, which is a 0.22° bias on a deflection and
/// the reason a direction is renormalised after the tap rather than trusted to
/// arrive unit.
#[inline]
#[must_use]
fn unorm_to_signed(byte: u8) -> f32 {
    unorm(byte) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×2 intensity layer at 1 m per texel: `[0, 1; 2, 3] / 3`.
    fn ramp() -> IntensityLayer {
        let grid = LayerGrid::new(2, 2, 1.0).expect("a real grid");
        let pixels: Vec<u8> = [0u8, 85, 170, 255]
            .into_iter()
            .flat_map(|red| [red, 0, 0, 255])
            .collect();
        IntensityLayer::from_rgba8(grid, &pixels).expect("four texels")
    }

    #[test]
    fn a_tap_on_a_texel_centre_reads_that_texel_alone() {
        let layer = ramp();
        // Texel centres of a 2×2 grid at 1 m per texel are at 0.5 and 1.5.
        assert_eq!(layer.sample(0.5, 0.5), 0.0);
        assert_eq!(layer.sample(1.5, 0.5), f64::from(85.0f32 / 255.0));
        assert_eq!(layer.sample(0.5, 1.5), f64::from(170.0f32 / 255.0));
        assert_eq!(layer.sample(1.5, 1.5), 1.0);
    }

    #[test]
    fn a_tap_halfway_between_two_centres_is_their_mean() {
        let layer = ramp();
        let left = f64::from(0.0f32);
        let right = f64::from(85.0f32 / 255.0);
        let midpoint = layer.sample(1.0, 0.5);
        assert!((midpoint - (left + right) / 2.0).abs() < 1e-15);
    }

    #[test]
    fn the_grid_repeats_in_both_directions() {
        let layer = ramp();
        for (x, z) in [(0.5, 0.5), (0.25, 1.75), (1.9, 0.1)] {
            let here = layer.sample(x, z);
            for (dx, dz) in [(2.0, 0.0), (0.0, 2.0), (-2.0, -2.0), (-8.0, 6.0)] {
                let there = layer.sample(x + dx, z + dz);
                assert!(
                    (here - there).abs() < 1e-15,
                    "({x}, {z}) and ({}, {}) are the same point under repeat: \
                     {here} against {there}",
                    x + dx,
                    z + dz
                );
            }
        }
    }

    #[test]
    fn every_weight_pair_sums_to_one() {
        let grid = LayerGrid::new(7, 3, 2.5).expect("a real grid");
        for step in 0..200 {
            let t = f64::from(step) * 0.37 - 30.0;
            let taps = grid.taps(t, -t * 1.7);
            let total: f64 = taps.weight.iter().sum();
            assert!(
                (total - 1.0).abs() < 1e-12,
                "weights at {t} sum to {total}, not 1"
            );
            for index in taps.index {
                assert!(index < grid.texel_count(), "index {index} is out of range");
            }
        }
    }

    #[test]
    fn a_direction_texel_is_decoded_as_a_vector() {
        let grid = LayerGrid::new(1, 1, 8.0).expect("a real grid");
        // 255 is +1, 128 is the nearest an eight-bit unorm gets to 0.
        let layer = DirectionLayer::from_rgba8(grid, &[255, 128, 0, 255]).expect("one texel");
        let sample = layer.sample(4.0, 4.0);
        assert!((sample.x - 1.0).abs() < 1e-7);
        assert!(
            sample.y.abs() < 0.005,
            "the midpoint bias is under a percent"
        );
    }

    #[test]
    fn a_layer_of_the_wrong_size_is_refused() {
        let grid = LayerGrid::new(2, 2, 1.0).expect("a real grid");
        assert!(matches!(
            IntensityLayer::from_rgba8(grid, &[0; 15]),
            Err(WindError::LayerSize { .. })
        ));
        assert!(matches!(
            DirectionLayer::from_rgba8(grid, &[0; 17]),
            Err(WindError::LayerSize { .. })
        ));
    }

    #[test]
    fn a_grid_with_no_texels_or_no_scale_is_refused() {
        assert!(matches!(
            LayerGrid::new(0, 4, 1.0),
            Err(WindError::EmptyLayer { .. })
        ));
        assert!(matches!(
            LayerGrid::new(4, 0, 1.0),
            Err(WindError::EmptyLayer { .. })
        ));
        assert!(matches!(
            LayerGrid::new(65536, 65536, 1.0),
            Err(WindError::EmptyLayer { .. })
        ));
        assert!(matches!(
            LayerGrid::new(4, 4, 0.0),
            Err(WindError::TexelScale { .. })
        ));
        assert!(matches!(
            LayerGrid::new(4, 4, f64::NAN),
            Err(WindError::TexelScale { .. })
        ));
    }

    #[test]
    fn a_fraction_is_never_negative_and_never_one() {
        for value in [-3.25, -0.5, -1e-18, 0.0, 0.5, 7.75, 1e17] {
            let fraction = fract(value);
            assert!(
                (0.0..1.0).contains(&fraction),
                "fract({value}) is {fraction}"
            );
        }
    }
}
