//! Crucible wind: the field everything that moves in air reads.
//!
//! `docs/plan/56-wind.md`'s rung W1. A gust that bends the grass, lifts a
//! character's hair, ruffles a lake and pushes a crate is **one** gust, and it
//! only arrives at each of them at the moment its position says it should if
//! every consumer samples the same field with the same formula. This crate is
//! that field, and the CPU copy of it is authoritative — decision 6 — so
//! physics, audio and CPU particles read these numbers and the GPU's copy
//! agrees with them within a stated bound rather than the other way round.
//!
//! # What is here, and what is not
//!
//! | Piece | Plan | Here |
//! | ----- | ---- | ---- |
//! | The two authored layers | decision 1 | [`DirectionLayer`], [`IntensityLayer`] |
//! | The weather state | decision 2 | [`Weather`], [`Beaufort`] |
//! | The one integer scroll offset | decision 3 | [`ScrollOffset`] |
//! | The sampling formula | decision 6 | [`WindField::sample`] |
//! | The physics seam | decision 6 | `crcbl_phys::WindQuery`, implemented for [`WindField`] |
//! | The GPU block | decision 6 | [`WindField::gpu_params`] → `crcbl_shaders::wind` |
//!
//! Not here, and deliberately without a placeholder: the motor list (W3), the
//! wake grid (W5), and decision 3's baked tileable gust noise (W2). W1 does
//! carry a **travelling gust front**, because decision 3 states its
//! construction — Crysis's smoothed triangle wave — and that costs no texture
//! and no transcendental, so the coherence the whole scroll offset exists for
//! is observable in a velocity at this rung rather than at the next one.
//!
//! # Two taps, two textures
//!
//! One tap of the coarse direction layer and one of the fine intensity layer,
//! which is W1's whole price. The gust front is arithmetic over the sample
//! position, not a third tap.
//!
//! # Deterministic
//!
//! Every operation in [`WindField::sample`] is one IEEE-754 specifies exactly:
//! multiply, add, `floor`, `abs`, one square root. No `sin`, no `exp`, no
//! `atan2` — decision 9's rule, satisfied by there being no transcendental to
//! route through `crcbl_shaders::trig` in the first place. Two samplings of the
//! same field at the same tick are bit-identical on every target, and a replay
//! lands in the same pose.

pub mod field;
pub mod layers;
pub mod load;
pub mod scroll;
pub mod weather;

pub use field::{MIN_DIRECTION_LENGTH_SQUARED, WindField, smooth_triangle};
pub use layers::{DirectionLayer, IntensityLayer, LayerGrid, MAX_TEXELS};
pub use load::{load_direction_layer, load_intensity_layer};
pub use scroll::{FRACTION_BITS, ScrollOffset, UNITS_PER_METRE};
pub use weather::{Beaufort, DEFAULT_GUST_WAVELENGTH, Weather};

use glam::DVec2;

/// Why a wind field could not be built or loaded.
///
/// One variant per way of being wrong rather than one string, so a caller can
/// tell an asset that has not arrived yet from one that is the wrong shape —
/// and so the message can name the numbers it refused.
#[derive(Debug, thiserror::Error)]
pub enum WindError {
    /// A direction that is zero-length or not finite, so there is nothing to
    /// normalise.
    #[error("a wind direction of {direction} has no direction to normalise")]
    DegenerateDirection {
        /// What was offered.
        direction: DVec2,
    },
    /// A base speed below zero, or not finite. A wind blowing backwards is a
    /// wind pointing the other way, which is what the direction is for.
    #[error("a wind speed of {speed} m/s is not a speed")]
    NegativeSpeed {
        /// What was offered.
        speed: f64,
    },
    /// A gust amplitude outside `0..=1`; above one the gust factor goes
    /// negative and reverses the wind.
    #[error("a gust amplitude of {amplitude} is outside 0..=1, so the gust would reverse the wind")]
    GustAmplitude {
        /// What was offered.
        amplitude: f64,
    },
    /// A gust wavelength that is not a positive finite number of metres.
    #[error("a gust wavelength of {wavelength} m is not a distance between fronts")]
    GustWavelength {
        /// What was offered.
        wavelength: f64,
    },
    /// A layer with a zero dimension, or more texels than [`MAX_TEXELS`].
    #[error("a {width}x{height} wind layer is not one this crate will hold")]
    EmptyLayer {
        /// Texels along +X.
        width: u32,
        /// Texels along +Z.
        height: u32,
    },
    /// A layer whose texels are not a positive finite number of metres apart.
    #[error("a wind layer at {metres_per_texel} m per texel has no scale")]
    TexelScale {
        /// What was offered.
        metres_per_texel: f64,
    },
    /// A pixel buffer that is not four bytes per texel of the grid it was
    /// handed with.
    #[error("a {width}x{height} wind layer is {expected} bytes of RGBA8, and {found} arrived")]
    LayerSize {
        /// Texels along +X.
        width: u32,
        /// Texels along +Z.
        height: u32,
        /// Bytes the grid needs.
        expected: usize,
        /// Bytes that arrived.
        found: usize,
    },
    /// The asset source could not hand over the bytes.
    ///
    /// **[`crcbl_assets::StorageError::Pending`] arrives here too**, and it is
    /// a state rather than a failure: an [`crcbl_assets::AssetSource`] never
    /// blocks, so a caller that gets it polls again next frame. It is not a
    /// variant of its own because the distinction belongs to the store, which
    /// already draws it.
    #[error("the wind layer `{key}` could not be read: {source}")]
    Asset {
        /// The asset key that was asked for.
        key: String,
        /// What the store said.
        source: crcbl_assets::StorageError,
    },
    /// The bytes arrived and were not a readable image.
    #[error("the wind layer `{key}` is not a readable PNG: {source}")]
    Decode {
        /// The asset key the bytes came from.
        key: String,
        /// What the decoder said.
        source: crcbl_sprite::load::LoadError,
    },
}
