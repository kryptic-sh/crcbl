//! The regenerate-or-check flow every cooked table shares.
//!
//! `cook-dfg` and `cook-sky-prefilter` each own an integrator and a report;
//! what they do with the bytes is the same: parse `[--check]`, and either
//! write the artifact or hold it to the integrator within a tolerance. One copy
//! here, included into each tool by `#[path]`, since an example cannot depend
//! on another example and the library has no business carrying a CLI.
//!
//! # `--check` compares within a tolerance, deliberately
//!
//! Every integrator here takes a `sin` and a `cos` per sample, and a
//! platform's `libm` is not the platform next to it's — so a `--check` that
//! demanded an exact match everywhere would fail on macOS and Windows for a
//! reason that is not a defect. CI runs the checks in the `test (linux)` job
//! only, and this compares decoded `f32`s within the tolerance each tool
//! chooses, so a developer on another machine gets a useful answer instead of
//! a false alarm.
//!
//! **A tolerance of zero compares bytes.** `cook-smaa`'s tables are bytes,
//! not `f32`s, and its generator takes no transcendental at all — so the only
//! honest comparison is exact, and decoding pairs of its bytes as floats would
//! be nonsense. A tool that passes `0.0` gets the first differing byte named
//! instead.

use std::path::PathBuf;
use std::process::ExitCode;

/// Regenerate or check `artifact` (relative to the crate) against `fresh`.
///
/// `tool` names the caller in every message. Returns the process's exit code:
/// success, failure, or 2 for arguments the tool does not take. A `tolerance`
/// of zero compares bytes; a positive one decodes `f32`s — the header says why
/// both exist.
pub fn run(tool: &str, artifact: &str, fresh: &[u8], tolerance: f32) -> ExitCode {
    let check = match std::env::args().skip(1).collect::<Vec<String>>().as_slice() {
        [] => false,
        [flag] if flag == "--check" => true,
        arguments => {
            eprintln!("{tool}: unexpected arguments {arguments:?}");
            eprintln!("usage: {tool} [--check]");
            return ExitCode::from(2);
        }
    };

    // Resolved against `CARGO_MANIFEST_DIR` rather than the working directory,
    // for `cook-clusters`' reason: a `--check` that silently compared against
    // nothing because of a `cd` is worse than no check.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(artifact);
    if check {
        let committed = match std::fs::read(&path) {
            Ok(committed) => committed,
            Err(error) => {
                eprintln!("{tool}: cannot read {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        };
        if committed.len() != fresh.len() {
            eprintln!(
                "{tool}: {artifact} is {} bytes and the integrator produces {}",
                committed.len(),
                fresh.len()
            );
            eprintln!("  Regenerate with: cargo run -p crcbl-shaders --example {tool}");
            return ExitCode::FAILURE;
        }
        let difference = if tolerance > 0.0 {
            worst_difference(&committed, fresh, tolerance).map(|(at, committed, fresh)| {
                format!(
                    "holds {committed} where the integrator produces {fresh} at value {at}, \
                     past a tolerance of {tolerance}"
                )
            })
        } else {
            committed
                .iter()
                .zip(fresh)
                .position(|(committed, fresh)| committed != fresh)
                .map(|at| {
                    format!(
                        "holds {} where the generator produces {} at byte {at}",
                        committed[at], fresh[at]
                    )
                })
        };
        match difference {
            Some(difference) => {
                eprintln!("{tool}: {artifact} {difference}");
                eprintln!("  the committed table is not what its generator produces.");
                eprintln!("  Regenerate with: cargo run -p crcbl-shaders --example {tool}");
                ExitCode::FAILURE
            }
            None if tolerance > 0.0 => {
                println!("{tool}: {artifact} matches the integrator to {tolerance}");
                ExitCode::SUCCESS
            }
            None => {
                println!("{tool}: {artifact} matches the generator byte for byte");
                ExitCode::SUCCESS
            }
        }
    } else {
        if let Some(directory) = path.parent()
            && let Err(error) = std::fs::create_dir_all(directory)
        {
            eprintln!("{tool}: cannot create {}: {error}", directory.display());
            return ExitCode::FAILURE;
        }
        if let Err(error) = std::fs::write(&path, fresh) {
            eprintln!("{tool}: cannot write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        println!("{tool}: wrote {artifact}");
        ExitCode::SUCCESS
    }
}

/// The first value past `tolerance`, as `(index, committed, fresh)`.
///
/// Decoded as `f32`s rather than compared as bytes, because two `f32`s a last
/// place apart differ in three of their four bytes and a byte offset says
/// nothing about how far apart the numbers are.
fn worst_difference(committed: &[u8], fresh: &[u8], tolerance: f32) -> Option<(usize, f32, f32)> {
    let value_at = |bytes: &[u8], index: usize| {
        let at = index * 4;
        f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    };
    (0..committed.len() / 4).find_map(|index| {
        let (left, right) = (value_at(committed, index), value_at(fresh, index));
        ((left - right).abs() > tolerance).then_some((index, left, right))
    })
}
