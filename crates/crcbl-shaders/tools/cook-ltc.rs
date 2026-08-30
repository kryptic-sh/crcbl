//! Generate `tables/ltc.bin` from the fit that owns it, or check that the
//! committed bytes are still what that fit produces.
//!
//!     cargo run -p crcbl-shaders --example cook-ltc            # regenerate
//!     cargo run -p crcbl-shaders --example cook-ltc -- --check # verify only
//!
//! # Why there is a generator at all
//!
//! `cook-dfg`'s header argues this in full and none of it is different here:
//! the fit importance-samples a GGX lobe and minimises an error over it, so it
//! is `sin`, `cos`, `powf` and a `sqrt` per sample on a machine whose `libm` is
//! not CI's — for goldens blessed once and compared on four backends. So the
//! table is data, baked on one machine and committed, and the shader reads
//! bytes rather than evaluating a fit.
//!
//! # It is the slowest of the four, and that is the fit rather than the table
//!
//! The three tables beside it are quadratures: one sweep per texel and done.
//! This one runs a derivative-free minimisation per texel, each step of which
//! is a fresh pair of importance-sampled sweeps, so it is thousands of times
//! the arithmetic for the same 64 squared entries. Run it in release when you
//! are regenerating; `--check` in CI is the same work and is why this step sits
//! in a job that has already built the workspace.
//!
//! # Why an example and not a binary
//!
//! `cook-dfg`'s reason: `tools/` beside `compile-shaders.sh`, which is the
//! other generator of a committed artifact in this crate, and an `[[example]]`
//! because a `[[bin]]` cannot see this crate's dev-dependencies.

use std::process::ExitCode;

use crcbl_shaders::ltc::{self, LTC_BYTES, LTC_SAMPLES, LTC_SIZE};

#[path = "cook_table.rs"]
mod cook_table;

/// Where the committed artifact lives, relative to this crate.
const ARTIFACT: &str = "tables/ltc.bin";

/// How far a freshly fitted entry may sit from the committed one under
/// `--check`.
///
/// **Looser than `cook-dfg`'s `1e-5`, and the reason is the minimiser rather
/// than the sampling.** A quadrature's answer moves by a last place when a
/// platform's `libm` does; a Nelder-Mead run's answer moves by however far the
/// simplex had left to walk when it stopped, which its own tolerance bounds at
/// `1e-5` in each parameter — and one parameter's `1e-5` reaches the packed
/// entries through a division by `m11`, which is small for a smooth lobe. The
/// figure below is measured rather than guessed: the largest disagreement
/// between two runs of this fit on this machine is far under it, and the
/// smallest disagreement that would change a half float in the uploaded image
/// is far over it.
const TOLERANCE: f32 = 1e-3;

fn main() -> ExitCode {
    let bytes = ltc::bake_bytes();
    assert_eq!(
        bytes.len(),
        LTC_BYTES,
        "the fit produced a table the format cannot hold"
    );
    report(&bytes);
    cook_table::run("cook-ltc", ARTIFACT, &bytes, TOLERANCE)
}

/// What the freshly fitted table looks like, so a regeneration is readable
/// rather than silent.
fn report(bytes: &[u8]) {
    let entry = |n_dot_v: usize, roughness: usize| {
        let at = (roughness * LTC_SIZE + n_dot_v) * 16;
        let mut out = [0.0f32; 4];
        for (slot, value) in out.iter_mut().enumerate() {
            let word = at + slot * 4;
            *value = f32::from_le_bytes([
                bytes[word],
                bytes[word + 1],
                bytes[word + 2],
                bytes[word + 3],
            ]);
        }
        out
    };

    let mut widest = 0.0f32;
    for roughness in 0..LTC_SIZE {
        for n_dot_v in 0..LTC_SIZE {
            for value in entry(n_dot_v, roughness) {
                widest = widest.max(value.abs());
            }
        }
    }

    let head_on_rough = entry(LTC_SIZE - 1, LTC_SIZE - 1);
    let head_on_smooth = entry(LTC_SIZE - 1, 0);
    println!(
        "cook-ltc: {LTC_SIZE}x{LTC_SIZE} table, {LTC_SAMPLES}x{LTC_SAMPLES} samples per objective, \
         {} bytes",
        bytes.len()
    );
    println!(
        "cook-ltc: head on, the roughest lobe is [{:.4}, {:.4}, {:.4}, {:.4}] and the smoothest \
         is [{:.4}, {:.4}, {:.4}, {:.4}]",
        head_on_rough[0],
        head_on_rough[1],
        head_on_rough[2],
        head_on_rough[3],
        head_on_smooth[0],
        head_on_smooth[1],
        head_on_smooth[2],
        head_on_smooth[3]
    );
    println!("cook-ltc: the widest entry is {widest:.4}, which the image stores as a half float");
}
