//! Generate `tables/sky_prefilter.bin` from the integrator that owns it, or
//! check that the committed bytes are still what that integrator produces.
//!
//!     cargo run -p crcbl-shaders --example cook-sky-prefilter            # regenerate
//!     cargo run -p crcbl-shaders --example cook-sky-prefilter -- --check # verify only
//!
//! `crcbl_shaders::sky_prefilter::bake` importance-samples the GGX lobe, which
//! is `cook-dfg`'s integrator pointed at the sky's blend instead of the
//! Fresnel split, and it is committed for the same reason: the table is data
//! four backends read the same bytes of. `cook_table.rs` carries the flow and
//! says why `--check` is within a tolerance and Linux-only.

use std::process::ExitCode;

use crcbl_shaders::sky_prefilter::{self, PREFILTER_BYTES, PREFILTER_SAMPLES, PREFILTER_SIZE};

#[path = "cook_table.rs"]
mod cook_table;

/// Where the committed artifact lives, relative to this crate.
const ARTIFACT: &str = "tables/sky_prefilter.bin";

/// How far a freshly baked entry may sit from the committed one under `--check`.
///
/// `cook-dfg`'s number, on its reasoning: the entries are shares in `[0, 1]`
/// read by an eight-bit target whose step is about `4e-3`, and a `libm`
/// disagreement moves the mean of `PREFILTER_SAMPLES` samples by far less than
/// this.
const TOLERANCE: f32 = 1e-5;

fn main() -> ExitCode {
    let bytes = sky_prefilter::bake_bytes();
    assert_eq!(
        bytes.len(),
        PREFILTER_BYTES,
        "the integrator produced a table the format cannot hold"
    );
    report(&bytes);
    cook_table::run("cook-sky-prefilter", ARTIFACT, &bytes, TOLERANCE)
}

/// What the freshly baked table looks like, so a regeneration is readable
/// rather than silent: a mirror facing up must see the zenith and nothing
/// else, and the roughest lobe facing up is where the horizon shows.
fn report(bytes: &[u8]) {
    let entry = |up: usize, roughness: usize| {
        let at = (roughness * PREFILTER_SIZE + up) * 8;
        let value = |offset: usize| {
            f32::from_le_bytes([
                bytes[at + offset],
                bytes[at + offset + 1],
                bytes[at + offset + 2],
                bytes[at + offset + 3],
            ])
        };
        [value(0), value(4)]
    };
    let top = PREFILTER_SIZE - 1;
    let [mirror_far, mirror_opposite] = entry(top, 0);
    let [rough_far, rough_opposite] = entry(top, top);
    println!(
        "cook-sky-prefilter: {PREFILTER_SIZE}x{PREFILTER_SIZE} table, {PREFILTER_SAMPLES} \
         samples per texel, {} bytes",
        bytes.len()
    );
    println!(
        "cook-sky-prefilter: facing the zenith, a mirror sees {mirror_far:.4} of it and \
         {mirror_opposite:.4} of the ground"
    );
    println!(
        "cook-sky-prefilter: the roughest lobe sees {rough_far:.4} of the zenith, \
         {rough_opposite:.4} of the ground, and the horizon for the rest — {:.1}%",
        100.0 * (1.0 - rough_far - rough_opposite)
    );
}
