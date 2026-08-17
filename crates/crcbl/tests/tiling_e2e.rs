//! Physical-size texture tiling, observed on the GPU.
//!
//! The win condition for `GpuMaterial::TILING_PHYSICAL`: a 2 m surface shows
//! twice the grid cells of a 1 m surface. The physical mode glues the texture to
//! world space — one greybox tile per `tile_metres` of surface — so a larger
//! face carries more of the grid, where authored-UV tiling stretches the single
//! tile across a face however large and shows the *same* cell count at both
//! sizes. This test counts cells off the rendered frame and asserts the 2 m
//! surface roughly doubles them, so it goes red against that authored fallback
//! rather than passing whether the mode works or not.
//!
//! `#[ignore]` like `render_e2e.rs`: it needs a real GPU, which `CRCBL_GPU`
//! names and `tests/run-render-e2e.sh` supplies. It commits no golden image —
//! the claim is a cell *count* read off the frame, not a pixel-exact reference,
//! so it survives the driver-to-driver colour noise a golden would have to
//! tolerate.

#![cfg(not(target_arch = "wasm32"))]

use crcbl::hal::Format;
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::ProbeGrid;
use crcbl::render::{
    Camera, Capacities, DirectionalLight, ForwardRenderer, InstanceDesc, MeshDesc, Projection,
    SceneDesc,
};
use crcbl::screenshot::{ForwardScene, OffscreenSetup};
use crcbl_golden::{ChannelOrder, Image};
use crcbl_greybox::{GreyboxColor, greybox_color_material, greybox_page, quad};

/// The square offscreen frame each surface is drawn into, in pixels.
///
/// 512 so that even the 2 m surface — whose one-metre tile is half the frame —
/// draws its grid lines at least a couple of pixels wide, which nearest sampling
/// of the 1024² page needs to land a line on the row being counted.
const EDGE: u32 = 512;

/// How much of each surface the orthographic window frames, as a fraction of the
/// whole face.
///
/// Under one, so the window sits strictly inside the face and the counted row
/// never runs off its edge onto the clear colour. The grid is world-locked, so
/// framing the *whole* of a larger face is the entire experiment: a 2 m face
/// fills the window with twice the world extent of a 1 m one, hence twice the
/// grid.
const FRAME_FRACTION: f32 = 0.94;

/// The Rec. 601 luma of a readback pixel, alpha ignored.
fn luma(pixel: [u8; 4]) -> f32 {
    0.299 * f32::from(pixel[0]) + 0.587 * f32::from(pixel[1]) + 0.114 * f32::from(pixel[2])
}

/// How many bright grid cells the frame shows across its width.
///
/// Reduced column-first: each column's value is its *brightest* pixel over the
/// whole height, which extracts the vertical grid lines while ignoring the
/// horizontal ones — a horizontal line darkens a whole row, but never a whole
/// column, so a column between two vertical lines stays bright somewhere. The
/// count is then the number of bright runs in that column profile, one per cell.
///
/// The threshold is the midpoint of the profile's own range rather than a fixed
/// level, so it tracks the frame's exposure instead of assuming one — a line is
/// dark relative to its field whatever the lighting made the absolute values.
fn cells_across(image: &Image) -> usize {
    let (width, height) = (image.width(), image.height());
    let profile: Vec<f32> = (0..width)
        .map(|x| {
            (0..height)
                .map(|y| luma(image.pixel(x, y).expect("in bounds")))
                .fold(f32::NEG_INFINITY, f32::max)
        })
        .collect();

    let min = profile.iter().copied().fold(f32::INFINITY, f32::min);
    let max = profile.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        max - min > 20.0,
        "the frame is nearly uniform (brightest column {min}..{max}); no grid was drawn to count"
    );
    let threshold = min + (max - min) * 0.5;

    let mut cells = 0usize;
    let mut in_field = false;
    for &value in &profile {
        let bright = value > threshold;
        if bright && !in_field {
            cells += 1;
        }
        in_field = bright;
    }
    cells
}

/// Draws one greybox-grey floor quad of `size_m` under the physical tiling
/// material, framed head-on, and returns the grid cells it shows across the
/// width.
///
/// The quad is the `+Y`-facing greybox [`quad`], so `TILING_PHYSICAL` samples it
/// by world `x,z` — and the orthographic camera looks straight down it, so a
/// world metre maps to a fixed span of pixels and the cell count read off the
/// frame is the cell count on the surface.
fn tiled_surface_cells(size_m: f32) -> usize {
    let mut setup = OffscreenSetup::open_forward(EDGE, EDGE, move |device, queue, format| {
        let scene = SceneDesc {
            meshes: vec![MeshDesc {
                label: "tiled floor".into(),
                geometry: quad(size_m, size_m),
            }],
            materials: vec![greybox_color_material(GreyboxColor::Grey)],
            page: greybox_page(),
            probes: ProbeGrid::default(),
            capacities: Capacities::default(),
        };
        let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
        renderer
            .add_instance(&InstanceDesc {
                mesh: 0,
                material: 0,
                transform: Mat4::IDENTITY,
            })
            .expect("a default instance pool holds one quad");
        Ok(ForwardScene {
            camera: Camera {
                // Straight down the floor's normal. `up` is `-Z` rather than the
                // usual `+Y` because that axis is now the view direction.
                eye: Vec3::new(0.0, 4.0, 0.0),
                target: Vec3::ZERO,
                up: Vec3::NEG_Z,
                projection: Projection::Orthographic {
                    half_height: size_m * 0.5 * FRAME_FRACTION,
                    near: 0.1,
                    far: 10.0,
                },
            },
            // Straight down onto the floor and a healthy ambient, so the grid's
            // dark lines and bright field separate by albedo rather than by any
            // shading gradient that could confuse the count.
            sun: DirectionalLight {
                direction: Vec3::Y,
                color: Vec3::splat(1.2),
                ambient: Vec3::splat(0.35),
            },
            renderer: Box::new(renderer),
        })
    })
    .unwrap_or_else(|why| panic!("a GPU backend opens for the tiling test: {why}"));

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    setup.finish().expect("the device reaches idle");

    let order = match format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => ChannelOrder::Bgra,
        _ => ChannelOrder::Rgba,
    };
    let image =
        Image::from_readback(width, height, &pixels, order).expect("the readback is one image");
    cells_across(&image)
}

/// A 2 m surface shows about twice the grid cells of a 1 m one — the whole of
/// what physical tiling buys over stretching one authored tile.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-render-e2e.sh"]
fn a_two_metre_surface_shows_twice_the_tiles_of_a_one_metre_surface() {
    let one_metre = tiled_surface_cells(1.0);
    let two_metre = tiled_surface_cells(2.0);

    // Anti-vacuity: the 1 m tile's quarter-metre sub-grid must resolve at all,
    // or there is nothing for the 2 m surface to have doubled.
    assert!(
        one_metre >= 3,
        "the 1 m surface shows only {one_metre} grid cells across; its quarter-metre sub-grid \
         did not draw, so the doubling claim below has no floor to stand on"
    );

    // The physical claim. Authored-UV tiling stretches one tile across a face of
    // any size and shows the same count at both — a ratio of 1, which this
    // rejects. Physical tiling holds the tile at one metre, so the 2 m face
    // carries twice the cells.
    assert!(
        two_metre as f32 >= one_metre as f32 * 1.7,
        "a 2 m surface shows {two_metre} cells across and a 1 m surface {one_metre}; physical \
         tiling should roughly double them, so this reads as the texture stretching to fit the \
         face rather than tiling by its size"
    );
}
