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
//! Because the integrator is not bit-portable, a `--check` that demanded an
//! exact match everywhere would fail on macOS and Windows for a reason that is
//! not a defect. CI runs it in the `test (linux)` job only, beside
//! `cook-clusters --check`, and this tool compares **within a tolerance** rather
//! than byte for byte so that a developer on another machine gets a useful
//! answer instead of a false alarm. The tolerance is far below the quantisation
//! of anything that reads the table and far above any `libm` disagreement — see
//! [`TOLERANCE`].
//!
//! # Why an example and not a binary
//!
//! `tools/` rather than `examples/` so it sits beside `compile-shaders.sh` and
//! `cook-clusters.rs`, which are this crate's other two generators of committed
//! artifacts, and an `[[example]]` because that is what those use.

use std::path::PathBuf;
use std::process::ExitCode;

use crcbl_shaders::dfg::{self, DFG_BYTES, DFG_SAMPLES, DFG_SIZE};

/// Where the committed artifact lives, relative to this crate.
///
/// Resolved against `CARGO_MANIFEST_DIR` rather than the working directory, for
/// `cook-clusters`' reason: a `--check` that silently compared against nothing
/// because of a `cd` is worse than no check.
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
    let check = match std::env::args().skip(1).collect::<Vec<String>>().as_slice() {
        [] => false,
        [flag] if flag == "--check" => true,
        arguments => {
            eprintln!("cook-dfg: unexpected arguments {arguments:?}");
            eprintln!("usage: cook-dfg [--check]");
            return ExitCode::from(2);
        }
    };

    let bytes = dfg::bake_bytes();
    assert_eq!(
        bytes.len(),
        DFG_BYTES,
        "the integrator produced a table the format cannot hold"
    );
    report(&bytes);

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(ARTIFACT);
    if check {
        let committed = match std::fs::read(&path) {
            Ok(committed) => committed,
            Err(error) => {
                eprintln!("cook-dfg: cannot read {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        };
        if committed.len() != bytes.len() {
            eprintln!(
                "cook-dfg: {ARTIFACT} is {} bytes and the integrator produces {}",
                committed.len(),
                bytes.len()
            );
            eprintln!("  Regenerate with: cargo run -p crcbl-shaders --example cook-dfg");
            return ExitCode::FAILURE;
        }
        match worst_difference(&committed, &bytes) {
            Some((at, committed_value, fresh_value)) => {
                eprintln!(
                    "cook-dfg: {ARTIFACT} holds {committed_value} where the integrator \
                     produces {fresh_value} at value {at}, past a tolerance of {TOLERANCE}"
                );
                eprintln!(
                    "  the committed table is not what `crcbl_shaders::dfg::bake` \
                     produces."
                );
                eprintln!("  Regenerate with: cargo run -p crcbl-shaders --example cook-dfg");
                ExitCode::FAILURE
            }
            None => {
                println!("cook-dfg: {ARTIFACT} matches the integrator to {TOLERANCE}");
                ExitCode::SUCCESS
            }
        }
    } else {
        if let Some(directory) = path.parent()
            && let Err(error) = std::fs::create_dir_all(directory)
        {
            eprintln!("cook-dfg: cannot create {}: {error}", directory.display());
            return ExitCode::FAILURE;
        }
        if let Err(error) = std::fs::write(&path, &bytes) {
            eprintln!("cook-dfg: cannot write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        println!("cook-dfg: wrote {ARTIFACT}");
        ExitCode::SUCCESS
    }
}

/// The first value past [`TOLERANCE`], as `(index, committed, fresh)`.
///
/// Decoded as `f32`s rather than compared as bytes, because two `f32`s a last
/// place apart differ in three of their four bytes and a byte offset says
/// nothing about how far apart the numbers are.
fn worst_difference(committed: &[u8], fresh: &[u8]) -> Option<(usize, f32, f32)> {
    let value_at = |bytes: &[u8], index: usize| {
        let at = index * 4;
        f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    };
    (0..committed.len() / 4).find_map(|index| {
        let (left, right) = (value_at(committed, index), value_at(fresh, index));
        ((left - right).abs() > TOLERANCE).then_some((index, left, right))
    })
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
