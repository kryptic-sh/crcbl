//! Generate `tables/smaa_area.bin` and `tables/smaa_search.bin` from the
//! generators that own them, or check that the committed bytes are still what
//! those generators produce.
//!
//!     cargo run -p crcbl-shaders --example cook-smaa            # regenerate
//!     cargo run -p crcbl-shaders --example cook-smaa -- --check # verify only
//!
//! `crcbl_shaders::smaa` transcribes SMAA's reference `AreaTex.py` and
//! `SearchTex.py`, and its header says which slab of the reference texture is
//! committed and why. Neither generator takes a transcendental, so `--check`
//! here is byte-exact: `cook_table.rs`'s zero tolerance, on the terms its
//! header gives.

use std::process::ExitCode;

use crcbl_shaders::smaa::{
    self, AREA_BYTES, AREA_HEIGHT, AREA_TEXEL_BYTES, AREA_WIDTH, SEARCH_BYTES, SEARCH_HEIGHT,
    SEARCH_STEP_SCALE, SEARCH_WIDTH,
};

#[path = "cook_table.rs"]
mod cook_table;

/// Where the committed artifacts live, relative to this crate.
const AREA_ARTIFACT: &str = "tables/smaa_area.bin";
const SEARCH_ARTIFACT: &str = "tables/smaa_search.bin";

/// Bytes are compared exactly — the module header says why nothing here can
/// drift between platforms.
const TOLERANCE: f32 = 0.0;

fn main() -> ExitCode {
    let area = smaa::bake_area();
    let search = smaa::bake_search();
    assert_eq!(
        area.len(),
        AREA_BYTES,
        "the area generator produced a table the format cannot hold"
    );
    assert_eq!(
        search.len(),
        SEARCH_BYTES,
        "the search generator produced a table the format cannot hold"
    );
    report(&area, &search);
    let area = cook_table::run("cook-smaa", AREA_ARTIFACT, &area, TOLERANCE);
    if area != ExitCode::SUCCESS {
        return area;
    }
    cook_table::run("cook-smaa", SEARCH_ARTIFACT, &search, TOLERANCE)
}

/// What the freshly generated tables look like, so a regeneration is readable
/// rather than silent: how much of each block blends anything, and how the
/// search steps are shared out.
fn report(area: &[u8], search: &[u8]) {
    let half = AREA_WIDTH / 2;
    let mut blending = [0usize; 2];
    for y in 0..AREA_HEIGHT {
        for x in 0..AREA_WIDTH {
            let at = (y * AREA_WIDTH + x) * AREA_TEXEL_BYTES;
            if area[at] != 0 || area[at + 1] != 0 {
                blending[usize::from(x >= half)] += 1;
            }
        }
    }
    println!(
        "cook-smaa: {AREA_WIDTH}x{AREA_HEIGHT} area table, {} bytes; {} orthogonal and {} \
         diagonal texels blend something",
        area.len(),
        blending[0],
        blending[1]
    );
    let steps = [0, 1, 2].map(|step| {
        search
            .iter()
            .filter(|&&texel| texel == step * SEARCH_STEP_SCALE)
            .count()
    });
    println!(
        "cook-smaa: {SEARCH_WIDTH}x{SEARCH_HEIGHT} search table, {} bytes; {} texels stop, {} \
         step one, {} step two",
        search.len(),
        steps[0],
        steps[1],
        steps[2]
    );
}
