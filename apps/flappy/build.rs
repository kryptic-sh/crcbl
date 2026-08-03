//! Bakes `assets/*.crpix` into the PNG + Aseprite-sidecar pair the engine
//! reads, and generates the table `src/art.rs` includes.
//!
//! ```text
//! assets/bird.crpix ──parse──▶ CrpixArt ──bake──▶ $OUT_DIR/bird.png
//!                                              └─▶ $OUT_DIR/bird.json
//!                                    │
//!                                    └──▶ $OUT_DIR/art_data.rs  (include_bytes!)
//! ```
//!
//! **The body of this script is [`crcbl_sprite::bake::bake_dir`].** It was
//! written out five times — the four samples and `crates/crcbl-render` — and
//! the copies differed only in their `ASSETS` array, the visibility of the
//! statics they emitted and the name of the file they wrote. Those three are
//! parameters now. What is left here is the part that is genuinely this
//! crate's: which sheets exist, and why they are split the way they are.
//!
//! # Why the baked files are not committed
//!
//! `docs/specs/crcbl/pix.md` is explicit that `.crpix` is a **build input** and
//! that nothing downstream knows it exists. Committing the PNG beside the text
//! would create two sources of truth for the same picture, and the one a
//! reviewer reads in a diff would be the one that is not loaded. Baking here
//! means the text is the art: edit it, and the next build regenerates
//! everything derived from it.
//!
//! # Why a generated table rather than four `include_bytes!` written by hand
//!
//! Because whether a sheet *has* a sidecar is a property of the art, not of the
//! game: [`crcbl_sprite::crpix::CrpixArt::needs_metadata`] says no for a single
//! still frame with no clips and no nine-slice, which is what `hills` and
//! `ground` are today and is not what they have to stay. A hand-written
//! `include_str!("…/hills.json")` would fail to compile the day someone adds a
//! clip's worth of animation to a background — or, worse, keep compiling
//! against a stale file. The script that knows the answer writes it down.
//!
//! # This runs on the host, for every target
//!
//! `apps/flappy` builds for `wasm32-unknown-unknown`, and a build script is
//! compiled and run for the *host* whatever the target is. That is what makes
//! this legal: the browser build gets the same bytes, already baked, through
//! `include_bytes!`, and nothing reads a file at runtime on any target.

use std::path::PathBuf;

/// The tick rate the frame holds are baked against.
///
/// A `.crpix` counts holds in **simulation ticks** and an Aseprite sidecar
/// counts milliseconds, so the pair has to be converted on the way out and back
/// on the way in — and the two conversions must use the same rate or every
/// hold changes. `src/art.rs`'s `ART_TICK_HZ` is the other half, and
/// `the_art_bakes_to_the_sheets_it_declares` asserts the authored hold survives,
/// which is the check that catches the two drifting apart.
const ART_TICK_HZ: u32 = 60;

/// The sheets, by file stem. Each is `assets/<stem>.crpix`.
const ASSETS: [&str; 4] = ["bird", "pipe", "hills", "ground"];

fn main() {
    crcbl_sprite::bake::bake_dir(&crcbl_sprite::bake::BakeDir {
        manifest_dir: &PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("cargo always sets CARGO_MANIFEST_DIR"),
        ),
        out_dir: &PathBuf::from(std::env::var("OUT_DIR").expect("cargo always sets OUT_DIR")),
        stems: &ASSETS,
        tick_hz: ART_TICK_HZ,
        visibility: crcbl_sprite::bake::Visibility::Public,
        table_name: "art_data.rs",
        source_label: "apps/flappy/assets",
    });
}
