//! Where the committed layer pair comes from.
//!
//! `crates/crcbl-wind/assets/direction.png` and `intensity.png` are not
//! hand-painted and are not photographs of anything: they are this file, run
//! once. `docs/plan/56-wind.md`'s decision 1 has both layers "authored or
//! cooked from terrain", and there is no terrain in this rung to cook from, so
//! the pair is cooked from a description instead — a valley that channels the
//! wind along it, and a sheltered hollow where the air is still.
//!
//! An artifact with no producer beside it is one nobody can regenerate, which
//! is why this exists rather than two opaque PNGs. It is shared by two callers:
//! `tools/cook-wind-layers.rs`, which writes the files, and
//! `tests/authored_layers.rs`, which decodes the committed ones and holds them
//! to what this produces.
//!
//! # No transcendental, for a reason that is not the shading rule
//!
//! Nothing here reaches a pixel, so `docs/plan/44-lighting.md`'s rule does not
//! bind a cook tool. What binds it is that the test comparing the committed
//! bytes against this function runs on Linux, macOS and Windows, and `sin` on
//! those three is three implementations that differ in the last place. A single
//! last-place difference lands on a rounding boundary sooner or later over five
//! thousand texels, and the failure would read as a corrupt asset. Everything
//! below is multiply, add, `floor`, `abs`, `sqrt` and `round`, each of which
//! IEEE-754 pins down exactly.

/// Texels along each axis of the direction layer.
///
/// 32 at [`DIRECTION_METRES_PER_TEXEL`] covers 256 m, which is the smallest
/// thing that reads as a valley at the 8–16 m per texel decision 1's research
/// suggests for this layer.
pub const DIRECTION_TEXELS: u32 = 32;

/// Metres of world per direction-layer texel — the coarse end of decision 1's
/// 8–16 m suggestion.
pub const DIRECTION_METRES_PER_TEXEL: f64 = 8.0;

/// Texels along each axis of the intensity layer.
pub const INTENSITY_TEXELS: u32 = 64;

/// Metres of world per intensity-layer texel — decision 1's 1–2 m suggestion
/// for the fine layer, at its coarse end so 64 texels still cover 128 m.
pub const INTENSITY_METRES_PER_TEXEL: f64 = 2.0;

/// The sheltered hollow's centre, in texture coordinates.
pub const SHELTER_CENTRE: (f64, f64) = (0.3, 0.7);

/// Texture-coordinate radius inside which the intensity is **exactly** zero.
///
/// 0.10 of a 128 m layer is 12.8 m, which is more than three texels of margin
/// around the centre — so a bilinear tap anywhere near the middle of the
/// hollow reads four zero texels and answers zero, rather than answering
/// nearly-zero and making "calm means calm" a claim about a tolerance.
pub const SHELTER_RADIUS: f64 = 0.10;

/// Texture-coordinate radius at which the shelter has no effect left.
pub const SHELTER_EDGE: f64 = 0.22;

/// Steepest deflection the valley applies, as the sine of the turn: 0.5 is 30°.
pub const MAX_DEFLECTION: f64 = 0.5;

/// Crysis's smoothed triangle wave — period one, range `0..=1`.
///
/// The same construction `crcbl_wind::smooth_triangle` is, spelled again here
/// because a cook tool that depended on the crate it cooks assets for could not
/// be run before that crate compiles. It is four lines and it is checked
/// against the library's copy by `tests/authored_layers.rs`.
#[must_use]
pub fn smooth_triangle(u: f64) -> f64 {
    let fraction = {
        let shifted = u + 0.5;
        shifted - shifted.floor()
    };
    let s = (fraction * 2.0 - 1.0).abs();
    s * s * (3.0 - 2.0 * s)
}

/// A value in `0..=1` as the unorm byte a GPU reads back as that value.
#[must_use]
pub fn to_unorm(value: f64) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The direction layer's RGBA8 texels, row-major.
///
/// **A valley along +X.** The deflection's Z component is a smoothed triangle
/// wave across the layer, modulated by a second one along it, so the wind is
/// steered up to [`MAX_DEFLECTION`] to one side and then the other as you cross
/// the valley — and the pattern tiles, because both waves have a whole number
/// of periods over the layer. The X component is whatever makes the pair a unit
/// vector, which is a square root and not an angle: decision 9's last line.
#[must_use]
pub fn direction_texels() -> Vec<u8> {
    let size = f64::from(DIRECTION_TEXELS);
    let mut pixels = Vec::with_capacity((DIRECTION_TEXELS * DIRECTION_TEXELS * 4) as usize);
    for row in 0..DIRECTION_TEXELS {
        for column in 0..DIRECTION_TEXELS {
            let u = (f64::from(column) + 0.5) / size;
            let v = (f64::from(row) + 0.5) / size;
            let across = smooth_triangle(v + 0.5 * smooth_triangle(2.0 * u));
            let turn = (2.0 * across - 1.0) * MAX_DEFLECTION;
            let along = (1.0 - turn * turn).sqrt();
            pixels.extend_from_slice(&[
                to_unorm((along + 1.0) / 2.0),
                to_unorm((turn + 1.0) / 2.0),
                0,
                255,
            ]);
        }
    }
    pixels
}

/// The intensity layer's RGBA8 texels, row-major.
///
/// **A gust pattern with a hollow cut out of it.** The pattern is a smoothed
/// triangle wave running diagonally, which tiles for the same reason the
/// direction layer's does; the hollow is a disc around [`SHELTER_CENTRE`] that
/// takes the intensity to exactly zero inside [`SHELTER_RADIUS`] and lets it
/// back to full by [`SHELTER_EDGE`]. Distance is measured the short way round
/// each axis, so the disc survives the layer repeating.
#[must_use]
pub fn intensity_texels() -> Vec<u8> {
    let size = f64::from(INTENSITY_TEXELS);
    let mut pixels = Vec::with_capacity((INTENSITY_TEXELS * INTENSITY_TEXELS * 4) as usize);
    for row in 0..INTENSITY_TEXELS {
        for column in 0..INTENSITY_TEXELS {
            let u = (f64::from(column) + 0.5) / size;
            let v = (f64::from(row) + 0.5) / size;
            let pattern = 0.35 + 0.65 * smooth_triangle(3.0 * u + v);
            let intensity = pattern * shelter(u, v);
            pixels.extend_from_slice(&[to_unorm(intensity), 0, 0, 255]);
        }
    }
    pixels
}

/// How much of the wind survives the hollow at a texture coordinate: `0` in the
/// middle of it, `1` outside it, smoothstepped between.
#[must_use]
pub fn shelter(u: f64, v: f64) -> f64 {
    let wrapped = |value: f64, centre: f64| {
        let delta = (value - centre).abs();
        // The short way round a layer that repeats.
        if delta > 0.5 { 1.0 - delta } else { delta }
    };
    let du = wrapped(u, SHELTER_CENTRE.0);
    let dv = wrapped(v, SHELTER_CENTRE.1);
    let distance = (du * du + dv * dv).sqrt();
    if distance <= SHELTER_RADIUS {
        return 0.0;
    }
    if distance >= SHELTER_EDGE {
        return 1.0;
    }
    let t = (distance - SHELTER_RADIUS) / (SHELTER_EDGE - SHELTER_RADIUS);
    t * t * (3.0 - 2.0 * t)
}
