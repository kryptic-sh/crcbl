//! Generate `tables/dfg.bin` from the integrator that owns it, or check that the
//! committed bytes are still what that integrator produces.
//!
//!     cargo run -p crcbl-shaders --example cook-dfg            # regenerate
//!     cargo run -p crcbl-shaders --example cook-dfg -- --check # verify only
//!
//! # Why there is a generator at all
//!
//! `crcbl_shaders::dfg::bake` importance-samples the GGX lobe, which takes a
//! `sin`, a `cos` and a square root per sample. This workspace's goldens are
//! compared across four backends with no tolerance, and a platform's `libm` is
//! not the platform next to it's — so a table computed on each machine would be
//! four tables and every reflective pixel would disagree somewhere in its last
//! place. The table is therefore **data**: baked once, committed, read by
//! everyone. That is the arrangement `clusters/dunes.dag` is already under, and
//! `--check` is what stops the artifact and the integrator drifting apart.
//!
//! # `--check` is pinned to one platform, deliberately
//!
//! `cook_table.rs` says why: the integrator is not bit-portable, so CI runs the
//! check in the `test (linux)` job only and the comparison is within
//! [`TOLERANCE`] rather than byte for byte.
//!
//! # Why an example and not a binary
//!
//! `tools/` rather than `examples/` so it sits beside `compile-shaders.sh` and
//! `cook-clusters.rs`, which are this crate's other two generators of committed
//! artifacts, and an `[[example]]` because that is what those use.

use std::process::ExitCode;

use crcbl_shaders::dfg::{self, DFG_BYTES, DFG_SAMPLES, DFG_SIZE};

#[path = "cook_table.rs"]
mod cook_table;

/// Where the committed artifact lives, relative to this crate.
const ARTIFACT: &str = "tables/dfg.bin";

/// How far a freshly baked entry may sit from the committed one under `--check`.
///
/// The table's values are fractions of arriving light, and the coarsest thing
/// that reads them is an `Rgba8Unorm` reflection attachment whose quantisation
/// is about `4e-3`. A disagreement between two platforms' `sin` and `cos` moves
/// a sample by a last place and the mean of `DFG_SAMPLES` of them by far less
/// than that. So this sits between the two by orders of magnitude on both
/// sides: it cannot be reached by rounding and it cannot hide a changed
/// integrand.
const TOLERANCE: f32 = 1e-5;

fn main() -> ExitCode {
    let bytes = dfg::bake_bytes();
    assert_eq!(
        bytes.len(),
        DFG_BYTES,
        "the integrator produced a table the format cannot hold"
    );
    report(&bytes);
    cook_table::run("cook-dfg", ARTIFACT, &bytes, TOLERANCE)
}

/// What the freshly baked table looks like, so a regeneration is readable
/// rather than silent.
///
/// The four numbers are the ones a reader would want to sanity-check by eye: a
/// mirror must return everything, the roughest surface must lose a lot, and the
/// compensation factor is what puts that back.
fn report(bytes: &[u8]) {
    let entry = |index: usize| {
        let at = index * 8;
        let value = |offset: usize| {
            f32::from_le_bytes([
                bytes[at + offset],
                bytes[at + offset + 1],
                bytes[at + offset + 2],
                bytes[at + offset + 3],
            ])
        };
        value(0) + value(4)
    };
    let head_on = DFG_SIZE - 1;
    let smoothest = head_on;
    let roughest = (DFG_SIZE - 1) * DFG_SIZE + head_on;
    println!(
        "cook-dfg: {DFG_SIZE}x{DFG_SIZE} table, {DFG_SAMPLES} samples per texel, \
         {} bytes",
        bytes.len()
    );
    println!(
        "cook-dfg: head-on directional albedo — {:.4} at the smoothest row, \
         {:.4} at the roughest",
        entry(smoothest),
        entry(roughest)
    );
    println!(
        "cook-dfg: so a white rough conductor is short by {:.1}% until the \
         compensation puts it back",
        100.0 * (1.0 - entry(roughest))
    );
}
