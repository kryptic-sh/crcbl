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
//! names and `tests/run-tiling-e2e.sh` supplies. It commits no golden image —
//! the claim is a cell *count* read off the frame, not a pixel-exact reference,
//! so it survives the driver-to-driver colour noise a golden would have to
//! tolerate.
//!
//! Its own runner rather than a second `--test` inside `run-render-e2e.sh`,
//! for the reason `run-hal-seam-e2e.sh` is its own script: each runner owns one
//! test binary, and its zero-tests check and its adapter-line grep are written
//! against that binary's single summary. Until that runner existed this file
//! had **never run in CI** — `run-render-e2e.sh` passes `--test render_e2e`, so
//! the sentence above claiming it did was false and the assertion below had
//! only ever been run by hand.

#![cfg(not(target_arch = "wasm32"))]

use crcbl::adapter::{ADAPTER_ENV_VAR, device_type_from_name};
use crcbl::backend::{BACKEND_ENV_VAR, GpuBackend};
use crcbl::hal::{Features, Format};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::ProbeGrid;
use crcbl::render::{
    Camera, Capacities, DirectionalLight, ForwardRenderer, InstanceDesc, MeshDesc, Projection,
    SceneDesc,
};
use crcbl::screenshot::{ForwardScene, OffscreenSetup};
use crcbl_golden::{ChannelOrder, Image};
use crcbl_greybox::{GreyboxColor, greybox_color_material, greybox_page, quad};

/// What this binary calls itself in the lines [`Offscreen`] prints.
///
/// Read by `tests/offscreen/verdict.rs`, which is shared with `render_e2e.rs`
/// and `gltf_e2e.rs` and therefore cannot name any of them.
const SUITE: &str = "crcbl tiling e2e";

// The teardown, out of `tests/offscreen/` rather than in here, because the other
// two suites tear the same fixture down and a second copy is a second place a
// fix has to land. That directory holds no `main.rs`, so Cargo builds no target
// of its own from it.
#[path = "offscreen/verdict.rs"]
mod verdict;

use verdict::Offscreen;

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

/// **The run is the run it says it is**: the backend and the adapter the frame
/// was drawn on are the ones the environment asked for, and both are announced.
///
/// The same pair `render_e2e.rs` makes, and for the same reason: every backend
/// tiles this quad identically by construction, so a run that silently fell
/// back to another one counts the same cells and proves nothing about the one
/// that was wanted. The printed line is load-bearing outside this file —
/// `run-tiling-e2e.sh` greps it, fails when it is missing, and compares the pin
/// it exported against the one that arrived, which is the one failure this
/// process cannot see for itself: an unset pin and a pin that never reached the
/// process are the same thing from in here.
fn assert_pins_arrived(setup: &OffscreenSetup) {
    let backend = setup.backend();
    let adapter = setup.adapter();
    let requested_adapter = crcbl::adapter::pin();
    eprintln!(
        "crcbl tiling e2e: device on adapter {id} {name:?} type={kind:?} ({ADAPTER_ENV_VAR}={pin})",
        id = adapter.id.0,
        name = adapter.name,
        kind = adapter.device_type,
        pin = requested_adapter.as_deref().unwrap_or("<unset>"),
    );

    if let Ok(requested) = std::env::var(BACKEND_ENV_VAR) {
        let opened = GpuBackend::from_name(&backend.to_string())
            .expect("every backend the registry can open has a GpuBackend spelling");
        assert_eq!(
            Some(opened),
            GpuBackend::from_name(&requested),
            "{BACKEND_ENV_VAR}={requested} was asked for and {backend} drew the frame"
        );
    }
    if let Some(requested) = requested_adapter.as_deref() {
        let want = device_type_from_name(requested)
            .unwrap_or_else(|| panic!("{ADAPTER_ENV_VAR}={requested} is not a device class"));
        assert_eq!(
            adapter.device_type, want,
            "{ADAPTER_ENV_VAR}={requested} was asked for and adapter {} ({:?}) drew the frame",
            adapter.name, adapter.device_type
        );
    }
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
    // A logger before anything opens, for `render_e2e.rs`'s reason: without one
    // every `log::info!` a backend emits on the way to a device goes nowhere,
    // and on a runner nobody can log into that output is the whole diagnosis.
    crcbl::core::log::init_logging();

    let setup = OffscreenSetup::open_forward(EDGE, EDGE, move |device, queue, format| {
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
    let mut setup = Offscreen::guard(SUITE, setup);

    assert_pins_arrived(&setup);

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    // Before the cell count is read off the frame: a device lost during the
    // frame, and a specification violation the layer refused, both surface here
    // and nowhere else — so a run that counted cells first would report a wrong
    // picture where the real answer is that the frame was never legal.
    setup.finish();

    let order = match format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => ChannelOrder::Bgra,
        _ => ChannelOrder::Rgba,
    };
    let image =
        Image::from_readback(width, height, &pixels, order).expect("the readback is one image");
    cells_across(&image)
}

/// How far along the floor [`grazing_floor_contrast`]'s camera looks, in
/// metres: the quad's half-extent, so the frame's far band is floor rather than
/// clear.
const GRAZING_HALF_LENGTH: f32 = 8.0;

/// How high above the floor the grazing camera sits, in metres.
///
/// Low, so the far band's texture footprint is many texels long along the view
/// for every texel across it — the shape anisotropic filtering exists for. At
/// this height the floor's far edge sits under two degrees below the horizon,
/// which the band below is placed just under.
const GRAZING_EYE_HEIGHT: f32 = 0.5;

/// The rows of a [`grazing_floor_contrast`] frame that hold the far floor, as
/// fractions of the height from the top.
///
/// The camera looks level, so the horizon is the frame's middle row and the
/// floor's far edge lands a few rows under it. Measured on radv before the
/// band was placed: the rows just under the edge are flat on both halves —
/// the grid's lines are millimetres wide and no footprint resolves them at
/// fifteen metres — and the bottom fifth is sharp on both, where a tile is
/// tens of pixels tall and the footprint is nearly square. Between them the
/// isotropic half held a contrast of six to fifteen and the anisotropic one
/// fifty to sixty-seven, and that stretch is this band.
const FAR_BAND: std::ops::Range<f32> = 0.60..0.70;

/// The least far-band contrast the anisotropic half may show, and the least
/// multiple of the isotropic half's it must be.
///
/// Both, because either alone can be met by a pair this test is meant to
/// fail: two flat bands pass a ratio, and a frame where the band is nearer than
/// it should be passes a floor with the isotropic half sharp as well. Against
/// the measured pair — fifty granted, six withheld, and llvmpipe within one of
/// each — the floor sits two fifths under the granted figure and the ratio at a
/// quarter of the measured one.
const FAR_BAND_CONTRAST_FLOOR: f32 = 30.0;
const FAR_BAND_CONTRAST_RATIO: f32 = 2.0;

/// How much line contrast the far band holds: the range of its per-column
/// brightest pixel.
///
/// [`cells_across`]'s profile over a band of rows rather than the whole frame,
/// reduced to its range instead of counted, because on the blurred half of the
/// pair there are no runs to count — the profile is flat at the grid's mean
/// grey and the count would be an assertion on noise.
fn band_contrast(image: &Image, rows: std::ops::Range<u32>) -> f32 {
    let profile: Vec<f32> = (0..image.width())
        .map(|x| {
            rows.clone()
                .map(|y| luma(image.pixel(x, y).expect("in bounds")))
                .fold(f32::NEG_INFINITY, f32::max)
        })
        .collect();
    let min = profile.iter().copied().fold(f32::INFINITY, f32::min);
    let max = profile.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    max - min
}

/// Draws the greybox floor from a camera lying almost on it and returns the
/// far band's line contrast, on a device opened with `optional_features`, and
/// whether that device holds `SAMPLER_ANISOTROPY` — which is what the renderer
/// read to pick the sampler's anisotropy.
///
/// The same quad and material as [`tiled_surface_cells`], under a perspective
/// camera looking along the floor instead of down at it — the one view where
/// the page's sampler is asked for a footprint far longer than it is wide, and
/// so the one view where its anisotropy is visible.
///
/// **Asked and granted are not the same set on every backend.** A device that
/// was asked for the feature and lacks it fails here by name, because the half
/// this test is about cannot be drawn without it. A device that was *not*
/// asked and holds it anyway is reported, not refused: `crcbl-vk` switches a
/// feature on at creation and so intersects the adapter's set with the ask,
/// but D3D12 and Metal have nothing to enable and report the adapter's caps
/// verbatim, which is what `DeviceDesc::optional_features` permits ("check
/// `Device::caps` afterwards"). On those the caller's control is not a control,
/// and it is the caller that decides what that costs.
fn grazing_floor_contrast(optional_features: Features) -> (f32, bool) {
    crcbl::core::log::init_logging();

    let setup = OffscreenSetup::open_forward_with(
        EDGE,
        EDGE,
        optional_features,
        move |device, queue, format| {
            let scene = SceneDesc {
                meshes: vec![MeshDesc {
                    label: "grazing floor".into(),
                    geometry: quad(GRAZING_HALF_LENGTH * 2.0, GRAZING_HALF_LENGTH * 2.0),
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
                    eye: Vec3::new(0.0, GRAZING_EYE_HEIGHT, GRAZING_HALF_LENGTH),
                    // Level, so the horizon is the middle row and `FAR_BAND`
                    // is placed against it.
                    target: Vec3::new(0.0, GRAZING_EYE_HEIGHT, -GRAZING_HALF_LENGTH),
                    up: Vec3::Y,
                    projection: Projection::Perspective {
                        fov_y: std::f32::consts::FRAC_PI_3,
                        near: 0.05,
                    },
                },
                sun: DirectionalLight {
                    direction: Vec3::Y,
                    color: Vec3::splat(1.2),
                    ambient: Vec3::splat(0.35),
                },
                renderer: Box::new(renderer),
            })
        },
    )
    .unwrap_or_else(|why| panic!("a GPU backend opens for the grazing floor: {why}"));
    let mut setup = Offscreen::guard(SUITE, setup);
    assert_pins_arrived(&setup);
    let granted = setup.caps().features.contains(Features::SAMPLER_ANISOTROPY);
    let asked = optional_features.contains(Features::SAMPLER_ANISOTROPY);
    assert!(
        granted || !asked,
        "the device was asked for SAMPLER_ANISOTROPY and does not offer it, so the \
         anisotropic half of this pair cannot be drawn on it"
    );

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    setup.finish();

    let order = match format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => ChannelOrder::Bgra,
        _ => ChannelOrder::Rgba,
    };
    let image =
        Image::from_readback(width, height, &pixels, order).expect("the readback is one image");
    let rows = |fraction: f32| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "a fraction of a frame height, in bounds"
        )]
        let row = (height as f32 * fraction) as u32;
        row
    };
    let contrast = band_contrast(&image, rows(FAR_BAND.start)..rows(FAR_BAND.end));
    eprintln!(
        "crcbl tiling e2e: grazing floor with anisotropy {} — far band contrast {contrast:.1}",
        match (asked, granted) {
            (true, _) => "asked and granted",
            (false, true) => "not asked, granted anyway",
            (false, false) => "withheld",
        }
    );
    (contrast, granted)
}

/// A 2 m surface shows about twice the grid cells of a 1 m one — the whole of
/// what physical tiling buys over stretching one authored tile.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-tiling-e2e.sh"]
fn a_two_metre_surface_shows_twice_the_tiles_of_a_one_metre_surface() {
    let one_metre = tiled_surface_cells(1.0);
    let two_metre = tiled_surface_cells(2.0);
    eprintln!(
        "crcbl tiling e2e: a 1 m surface shows {one_metre} cells across and a 2 m surface \
         {two_metre}"
    );

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

/// **The device's anisotropy reaches the page's sampler.** The same grazing
/// floor drawn on a device granted `SAMPLER_ANISOTROPY` keeps its far grid
/// lines where the device without it has blurred them to the grid's mean.
///
/// A pair rather than a threshold on one frame, because "sharp" has no absolute
/// number: what the far band holds depends on the rasteriser's level-of-detail
/// arithmetic, which the specification bounds rather than fixes. The withheld
/// half is the control — `ForwardRenderer::anisotropy_for` answers one for it,
/// so its sampler is the trilinear one every backend agrees on — and the
/// granted half has to beat it by [`FAR_BAND_CONTRAST_RATIO`]. A renderer that
/// asked for the feature and then built the sampler at one draws the two
/// halves alike and fails here; nothing else can see a sampler's anisotropy on
/// any backend.
///
/// **The control exists only where the seam withholds a feature.** On D3D12
/// and Metal a device holds every feature its adapter has whether or not it
/// was asked — [`grazing_floor_contrast`] says why — so the "withheld" half
/// there is drawn anisotropically too and the ratio would compare a frame with
/// itself. There the anisotropic half is held to [`FAR_BAND_CONTRAST_FLOOR`]
/// alone, which the sabotage that forces the clamp to one still fails (six
/// against a floor of thirty), and the line printed says the control was not
/// one. A renderer knob that set the anisotropy below the device's would give
/// every backend the pair; `docs/backlog.md`'s `anisotropic_filtering` entry
/// is where that knob is owed.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-tiling-e2e.sh"]
fn the_far_floor_keeps_its_grid_where_the_device_filters_anisotropically() {
    let (withheld, control_is_anisotropic) = grazing_floor_contrast(
        OffscreenSetup::OPTIONAL_FEATURES.difference(Features::SAMPLER_ANISOTROPY),
    );
    let (granted, _) = grazing_floor_contrast(OffscreenSetup::OPTIONAL_FEATURES);
    assert!(
        granted > FAR_BAND_CONTRAST_FLOOR,
        "the far band keeps its lines under anisotropic filtering: contrast {granted:.1}, \
         wanting at least {FAR_BAND_CONTRAST_FLOOR}"
    );
    if control_is_anisotropic {
        eprintln!(
            "crcbl tiling e2e: this backend enables every feature its adapter has, so the \
             withheld half ({withheld:.1}) is no isotropic control; the floor stood alone"
        );
    } else {
        assert!(
            granted > withheld * FAR_BAND_CONTRAST_RATIO,
            "the far band keeps its lines under anisotropic filtering: contrast {granted:.1} \
             granted against {withheld:.1} withheld, wanting {FAR_BAND_CONTRAST_RATIO}× the \
             control"
        );
    }
}
