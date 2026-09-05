//! Generate `tables/atmosphere.bin` from the integrators that own it, or check
//! that the committed bytes are still what those integrators produce.
//!
//!     cargo run -p crcbl-shaders --example cook-atmosphere            # regenerate
//!     cargo run -p crcbl-shaders --example cook-atmosphere -- --check # verify only
//!
//! # Why there is a generator at all
//!
//! `crcbl_shaders::atmosphere` is Hillaire's sky. Its transmittance and
//! multiple-scattering LUTs depend on the planet and not on the sun, so they
//! are cooked once — and they are cooked rather than computed at start-up for
//! `cook-dfg`'s reason exactly: both integrators take an `exp` per step and a
//! `sin` and a `cos` per direction, a platform's `libm` is not the platform
//! next to it's, and this workspace's goldens are compared across four backends
//! with no tolerance. The tables are therefore **data**: baked once, committed,
//! read by everyone. `cook_table.rs` carries the flow and says why `--check`
//! is within a tolerance and Linux-only.
//!
//! # One artifact for two tables
//!
//! The multiple-scattering integrator reads the transmittance table it has just
//! been handed, so a tree where one is regenerated and the other is not has no
//! meaning. `crcbl_shaders::atmosphere::TABLE_BYTES` lays the two out in one
//! file, which makes that state unrepresentable.
//!
//! # Why an example and not a binary
//!
//! `tools/` rather than `examples/` so it sits beside `compile-shaders.sh` and
//! the other cooks, and an `[[example]]` because that is what those use.

use std::process::ExitCode;

use crcbl_shaders::atmosphere::{
    self, GROUND_RADIUS_KM, MULTISCATTER_SIZE, TABLE_BYTES, TRANSMITTANCE_HEIGHT,
    TRANSMITTANCE_WIDTH,
};

#[path = "cook_table.rs"]
mod cook_table;

/// Where the committed artifact lives, relative to this crate.
const ARTIFACT: &str = "tables/atmosphere.bin";

/// How far a freshly baked entry may sit from the committed one under
/// `--check`.
///
/// **Looser than `cook-dfg`'s `1e-5`, and measured rather than chosen.** Both
/// tables here are sums of tens of `exp` calls rather than means of a thousand
/// bounded samples, so a last-place disagreement between two `libm`s
/// accumulates along the march instead of averaging out; and the
/// multiple-scattering table divides by `1 − f_ms`, which amplifies whatever
/// reached it. The consumer is the sky-view march, whose `f32` entries reach
/// the frame through an eight-bit swapchain: one level of 255 near a typical
/// sky radiance of `0.05` is about `4e-4` of it, so this sits two orders of
/// magnitude under what a pixel can show and orders of magnitude over what
/// rounding can reach.
const TOLERANCE: f32 = 2e-6;

fn main() -> ExitCode {
    let bytes = atmosphere::bake_bytes();
    assert_eq!(
        bytes.len(),
        TABLE_BYTES,
        "the integrators produced a table the format cannot hold"
    );
    report(&bytes);
    cook_table::run("cook-atmosphere", ARTIFACT, &bytes, TOLERANCE)
}

/// What the freshly baked tables look like, so a regeneration is readable
/// rather than silent.
///
/// The numbers are the ones a reader can check against the physics by eye: air
/// takes about a twentieth of the red and a quarter of the blue out of a
/// vertical ray, which is why the sky is blue at all; a horizontal ray is many
/// times deeper; and the multiple-scattering term under a high sun is a small
/// fraction of the sun's own illuminance.
fn report(bytes: &[u8]) {
    let value =
        |at: usize| f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let entry = |index: usize| {
        let at = index * 12;
        [value(at), value(at + 4), value(at + 8)]
    };
    println!(
        "cook-atmosphere: {TRANSMITTANCE_WIDTH}x{TRANSMITTANCE_HEIGHT} transmittance and \
         {MULTISCATTER_SIZE}x{MULTISCATTER_SIZE} multiple scattering, {} bytes",
        bytes.len()
    );
    let zenith = atmosphere::transmittance_at(f64::from(GROUND_RADIUS_KM), 1.0);
    let horizon = atmosphere::transmittance_at(f64::from(GROUND_RADIUS_KM), 0.0);
    println!(
        "cook-atmosphere: from the ground, a vertical ray transmits \
         {:.4}/{:.4}/{:.4} and a horizontal one {:.4}/{:.4}/{:.4}",
        zenith[0], zenith[1], zenith[2], horizon[0], horizon[1], horizon[2]
    );
    // The multiple-scattering table's brightest corner: the sun straight up,
    // read at the ground.
    let overhead = entry(TRANSMITTANCE_WIDTH * TRANSMITTANCE_HEIGHT + MULTISCATTER_SIZE - 1);
    println!(
        "cook-atmosphere: with the sun overhead the ground sees {:.5}/{:.5}/{:.5} of further \
         scattering orders per unit of sun illuminance",
        overhead[0], overhead[1], overhead[2]
    );
}
