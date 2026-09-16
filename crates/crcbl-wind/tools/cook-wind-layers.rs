//! Generate `assets/direction.png` and `assets/intensity.png`, or check that
//! the committed files are still what [`authored`] produces.
//!
//!     cargo run -p crcbl-wind --example cook-wind-layers            # regenerate
//!     cargo run -p crcbl-wind --example cook-wind-layers -- --check # verify only
//!
//! # Why there is a generator at all
//!
//! `docs/plan/56-wind.md`'s decision 1 wants the two layers "authored or cooked
//! from terrain", and a committed PNG whose provenance is a sentence in a
//! commit message is one nobody can regenerate or reason about. [`authored`] is
//! the provenance: a valley that channels the wind along it and a hollow where
//! the air is still, written in arithmetic.
//!
//! # `--check` here compares *pixels*, not bytes
//!
//! A PNG encoder is free to choose filters and a compression level, and the one
//! this workspace pins is free to choose differently in its next version. What
//! must not change is the image, so the comparison decodes both sides. The
//! standing check is `tests/authored_layers.rs`, which does the same under
//! `cargo test`; this mode is here so the tool that writes the files can also
//! say whether they need writing.
//!
//! # Why an example and not a binary
//!
//! `tools/` rather than `examples/` so it sits beside the module it shares with
//! the test, and an `[[example]]` because a binary cannot see the
//! dev-dependency that encodes a PNG. That is the arrangement
//! `crates/crcbl-shaders/tools/cook-dfg.rs` is already under.

use std::path::Path;
use std::process::ExitCode;

use crcbl_sprite::bake::encode_png;
use crcbl_sprite::load::decode_png;

#[path = "authored.rs"]
mod authored;

use authored::{
    DIRECTION_METRES_PER_TEXEL, DIRECTION_TEXELS, INTENSITY_METRES_PER_TEXEL, INTENSITY_TEXELS,
    direction_texels, intensity_texels,
};

/// One artifact this tool owns.
struct Artifact {
    /// Where it lives, relative to this crate.
    path: &'static str,
    /// Texels along each axis.
    size: u32,
    /// Metres of world per texel. Not in the file — a PNG has no world scale —
    /// so it is printed here, because a layer whose scale nobody wrote down is
    /// a layer nobody can load at the size it was drawn for.
    metres_per_texel: f64,
    /// What [`authored`] says it should hold.
    pixels: Vec<u8>,
}

fn main() -> ExitCode {
    let artifacts = [
        Artifact {
            path: "assets/direction.png",
            size: DIRECTION_TEXELS,
            metres_per_texel: DIRECTION_METRES_PER_TEXEL,
            pixels: direction_texels(),
        },
        Artifact {
            path: "assets/intensity.png",
            size: INTENSITY_TEXELS,
            metres_per_texel: INTENSITY_METRES_PER_TEXEL,
            pixels: intensity_texels(),
        },
    ];
    let checking = std::env::args().any(|argument| argument == "--check");
    let mut failed = false;
    for artifact in &artifacts {
        let encoded = encode_png(&artifact.pixels, artifact.size, artifact.size)
            .expect("the cooked texels are a whole number of RGBA8 rows");
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(artifact.path);
        if checking {
            match std::fs::read(&path) {
                Ok(committed) => {
                    let decoded = decode_png(&committed).expect("the committed file is a PNG");
                    if decoded.width != artifact.size
                        || decoded.height != artifact.size
                        || decoded.pixels != artifact.pixels
                    {
                        eprintln!(
                            "cook-wind-layers: {} is not what authored.rs produces; \
                             rerun without --check",
                            artifact.path
                        );
                        failed = true;
                    } else {
                        println!(
                            "cook-wind-layers: {} matches ({} bytes on disk, {} m per texel)",
                            artifact.path,
                            committed.len(),
                            artifact.metres_per_texel
                        );
                    }
                }
                Err(why) => {
                    eprintln!(
                        "cook-wind-layers: {} could not be read: {why}",
                        artifact.path
                    );
                    failed = true;
                }
            }
        } else {
            std::fs::write(&path, &encoded).expect("the assets directory is writable");
            println!(
                "cook-wind-layers: wrote {} ({}x{} at {} m per texel, {} bytes)",
                artifact.path,
                artifact.size,
                artifact.size,
                artifact.metres_per_texel,
                encoded.len()
            );
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
