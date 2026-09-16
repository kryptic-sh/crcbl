//! The committed layer pair is still what its generator produces.
//!
//! `crates/crcbl-wind/assets/direction.png` and `intensity.png` are cooked by
//! `crates/crcbl-wind/tools/cook-wind-layers.rs` out of
//! `crates/crcbl-wind/tools/authored.rs`, and an artifact that has drifted from
//! its generator is one nobody can regenerate — the next person to run the tool
//! silently changes every test that reads the layers.
//!
//! This is `cook-wind-layers --check` under `cargo test`, so the check runs
//! wherever the suite does rather than only where someone remembers to run a
//! tool. It compares **decoded pixels** rather than file bytes: a PNG encoder
//! chooses filters and a compression level, and this is a claim about the
//! image.

use std::path::Path;

use crcbl_assets::MemorySource;
use crcbl_sprite::load::decode_png;
use crcbl_wind::{load_direction_layer, load_intensity_layer};

#[path = "../tools/authored.rs"]
mod authored;

use authored::{
    DIRECTION_METRES_PER_TEXEL, DIRECTION_TEXELS, INTENSITY_METRES_PER_TEXEL, INTENSITY_TEXELS,
    SHELTER_CENTRE, direction_texels, intensity_texels,
};

/// The committed direction layer's bytes.
const DIRECTION_PNG: &[u8] = include_bytes!("../assets/direction.png");

/// The committed intensity layer's bytes.
const INTENSITY_PNG: &[u8] = include_bytes!("../assets/intensity.png");

#[test]
fn the_committed_direction_layer_is_what_its_generator_writes() {
    let decoded = decode_png(DIRECTION_PNG).expect("the committed file is a PNG");
    assert_eq!(
        (decoded.width, decoded.height),
        (DIRECTION_TEXELS, DIRECTION_TEXELS)
    );
    assert_eq!(
        decoded.pixels,
        direction_texels(),
        "assets/direction.png has drifted from tools/authored.rs; \
         rerun `cargo run -p crcbl-wind --example cook-wind-layers`"
    );
}

#[test]
fn the_committed_intensity_layer_is_what_its_generator_writes() {
    let decoded = decode_png(INTENSITY_PNG).expect("the committed file is a PNG");
    assert_eq!(
        (decoded.width, decoded.height),
        (INTENSITY_TEXELS, INTENSITY_TEXELS)
    );
    assert_eq!(
        decoded.pixels,
        intensity_texels(),
        "assets/intensity.png has drifted from tools/authored.rs; \
         rerun `cargo run -p crcbl-wind --example cook-wind-layers`"
    );
}

/// The generator carries its own copy of the smoothed triangle wave, because a
/// cook tool cannot depend on the crate it cooks for. Two copies is where a
/// drift starts, so this is the comparison that stops one.
#[test]
fn the_generators_wave_is_the_librarys() {
    for step in -1000..1000 {
        let u = f64::from(step) * 0.0071;
        assert_eq!(
            authored::smooth_triangle(u),
            crcbl_wind::smooth_triangle(u),
            "the two copies of the smoothed triangle wave disagree at {u}"
        );
    }
}

/// The hollow is *exactly* zero over a whole neighbourhood, not merely small.
///
/// "Calm means calm" is a claim about zero, and an intensity of `1/255` reads
/// as calm on a screen while moving a leaf. This asserts the texels themselves,
/// so a change to the generator that softened the hollow fails here rather than
/// in the field test that reads it.
#[test]
fn the_sheltered_hollow_is_a_block_of_exact_zeroes() {
    let pixels = intensity_texels();
    let size = INTENSITY_TEXELS;
    let centre = (
        (SHELTER_CENTRE.0 * f64::from(size)) as u32,
        (SHELTER_CENTRE.1 * f64::from(size)) as u32,
    );
    let mut zeroes = 0;
    for row in centre.1.saturating_sub(2)..=centre.1 + 2 {
        for column in centre.0.saturating_sub(2)..=centre.0 + 2 {
            let red = pixels[((row * size + column) * 4) as usize];
            assert_eq!(
                red, 0,
                "texel ({column}, {row}) in the hollow is {red}, not calm"
            );
            zeroes += 1;
        }
    }
    assert_eq!(zeroes, 25, "the loop covered the block it claims to");
}

/// The two files load through the asset seam at the scales their generator
/// wrote them for, and the grids that come out are the ones the rest of the
/// suite samples.
#[test]
fn the_pair_loads_through_the_asset_seam() {
    let mut source = MemorySource::new();
    source
        .insert(Path::new("wind/direction.png"), DIRECTION_PNG.to_vec())
        .expect("a legal asset key");
    source
        .insert(Path::new("wind/intensity.png"), INTENSITY_PNG.to_vec())
        .expect("a legal asset key");

    let direction = load_direction_layer(
        &source,
        Path::new("wind/direction.png"),
        DIRECTION_METRES_PER_TEXEL,
    )
    .expect("a direction layer");
    let intensity = load_intensity_layer(
        &source,
        Path::new("wind/intensity.png"),
        INTENSITY_METRES_PER_TEXEL,
    )
    .expect("an intensity layer");

    assert_eq!(direction.grid().extent_metres().x, 256.0);
    assert_eq!(intensity.grid().extent_metres().x, 128.0);
}
