//! [`Scene::Occluders`]: topic 03 §3.3's proving
//! scene for the occlusion cull, held to its golden on every backend and every
//! geometry path this machine reaches.
//!
//! One frame cannot show the cull hiding anything — its first phase has no
//! previous frame — so what the golden holds is the scene and the occlusion
//! pipelines, drawn on every backend. The path and its claims are
//! `tests/mesh_e2e/occlusion_cull.rs`'s.

use crcbl::screenshot::Scene;
use crcbl_golden::Image;

use super::EXTENT;

/// The anti-vacuity colour count for [`Scene::Occluders`].
const MIN_COLORS_OCCLUDERS: usize = 16;

/// Whether a pixel is the tinted walls' blue: blue its largest channel by a
/// margin.
fn is_wall(pixel: [u8; 4]) -> bool {
    let [r, g, b, _] = pixel.map(i32::from);
    b > r + 32 && b > g + 32
}

/// Whether a pixel is the floor's green.
fn is_floor(pixel: [u8; 4]) -> bool {
    let [r, g, b, _] = pixel.map(i32::from);
    g > r + 16 && g > b + 16
}

/// **The front wall fills the left of the frame and the crates show past its
/// end on the right** — the one frame's evidence that the walls hide what they
/// were built to and that the field behind is in the scene at all.
///
/// Counted over the band a crate stands in, at the golden's frame: on the left
/// the wall is every pixel, and on the right, past the wall's end, some pixels
/// are neither wall nor floor — the crates' faces. A frame whose field was culled
/// whole would have none, and one whose wall was lost would have floor on the
/// left.
fn the_scene_is_in_the_frame(image: &Image) {
    let pixel = |x, y| image.pixel(x, y).expect("inside the frame");
    let band = 100..116;
    let left: Vec<(u32, u32)> = band
        .clone()
        .flat_map(|y| (20..140).map(move |x| (x, y)))
        .collect();
    let walled = left.iter().filter(|&&(x, y)| is_wall(pixel(x, y))).count();
    assert_eq!(
        walled,
        left.len(),
        "the front wall covers {walled} of the {} pixels left of its end",
        left.len()
    );
    let crates = band
        .flat_map(|y| (180..250).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let at = pixel(x, y);
            !is_wall(at) && !is_floor(at)
        })
        .count();
    assert!(
        crates >= 200,
        "past the front wall's end {crates} pixels are neither wall nor floor, so the crates \
         behind it are not in the frame"
    );
}

/// [`Scene::Occluders`] drawn, against the reference in `tests/golden/`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_occluders_scene_draws_its_walls_and_matches_its_golden() {
    super::draw_scene_and_match_its_golden(
        Scene::Occluders,
        "occluders",
        EXTENT,
        MIN_COLORS_OCCLUDERS,
        the_scene_is_in_the_frame,
    );
}

/// [`Scene::Occluders`] on every geometry path this machine can reach.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn the_occluders_scene_draws_the_same_frame_on_every_geometry_path() {
    super::draw_scene_on_every_geometry_path(
        Scene::Occluders,
        "occluders",
        MIN_COLORS_OCCLUDERS,
        the_scene_is_in_the_frame,
    );
}
