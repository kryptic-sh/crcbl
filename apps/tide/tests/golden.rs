//! The courtyard off a real device, from the fixed camera, against checked-in
//! goldens — and the claims about its water in front of them.
//!
//! # A golden alone cannot make a claim about water
//!
//! A pale blue pool is a pale blue pool. A body that was never handed to the
//! renderer, a medium that never reached the shader, and a surface that
//! absorbed nothing with depth each draw a picture somebody would bless. So the
//! goldens are the last check here, and the claims before them are relations
//! between bands of the frame: where the water is and is not, the deep end
//! against the shallow end, and each medium preset against the others.
//! `crates/crcbl/tests/render_e2e/still_pool.rs` is the engine's version of the
//! same argument for its own fixture; these are about tide's courtyard.
//!
//! # Feature-gated *and* ignored
//!
//! The pair every sample's golden suite uses. A plain `cargo test --workspace
//! --all-features` on a machine with no GPU must stay green, and
//! `tests/run-tide-golden.sh` is the only thing that turns both off.

#![cfg(feature = "golden-e2e")]

use crcbl::hal::Format;
use crcbl::math::Vec3;
use crcbl::render::{Camera, ForwardRenderer};
use crcbl::screenshot::{ForwardScene, OffscreenSetup};
use crcbl::shaders::tonemap::TonemapCurve;
use crcbl_golden::{ChannelOrder, Golden, Image};
use crcbl_tide::medium::Preset;
use crcbl_tide::scene::{self, Scene, Stage};

/// The extent the checked-in goldens are blessed at.
const EXTENT: (u32, u32) = (256, 192);

/// The extent the claims are read at: four times the goldens' width, so a band
/// a few pixels across sits well inside the surface it is about.
const CLAIM_EXTENT: (u32, u32) = (1024, 768);

/// The half-extent of every band read here, in pixels at [`CLAIM_EXTENT`].
const BAND: (u32, u32) = (8, 4);

/// Where a review frame is written, relative to the workspace root.
const REVIEW_DIR: &str = "target/tide";

/// One frame to draw: which medium, whether the pool keeps its body, and which
/// tonemap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Arm {
    /// The medium the courtyard's water is in.
    medium: Preset,
    /// Whether the body stays; `false` draws the same renderer with its water
    /// taken away — the control every claim is read against.
    water: bool,
    /// Whether the frame is drawn under the clamp rather than the shipped
    /// curve — `crates/crcbl/tests/render_e2e.rs`' `scene_referred` argument,
    /// since a claim comparing channels should not read them through a curve
    /// that mixes them.
    scene_referred: bool,
}

impl Arm {
    /// The courtyard as a run opens on it.
    const fn shipped() -> Self {
        Self {
            medium: Preset::ClearPool,
            water: true,
            scene_referred: false,
        }
    }

    /// The same, in `medium`.
    const fn in_medium(self, medium: Preset) -> Self {
        Self { medium, ..self }
    }

    /// The same, with the body taken away.
    const fn dry(self) -> Self {
        Self {
            water: false,
            ..self
        }
    }

    /// The same, under the clamp.
    const fn referred(self) -> Self {
        Self {
            scene_referred: true,
            ..self
        }
    }
}

/// Draws one arm of the courtyard from the fixed camera and reads it back.
fn draw(extent: (u32, u32), arm: Arm) -> Image {
    crcbl::core::log::init_logging();
    let mut setup = OffscreenSetup::open_forward_with(
        extent.0,
        extent.1,
        OffscreenSetup::OPTIONAL_FEATURES,
        |device, queue, format| {
            Ok(ForwardScene {
                camera: scene::fixed_camera(),
                sun: scene::sun(),
                renderer: Box::new(build(device, queue, format, arm)?),
            })
        },
    )
    .unwrap_or_else(|why| panic!("a GPU backend opens for tide's courtyard: {why}"));

    let caps = setup.caps();
    let adapter = setup.adapter().clone();
    // Printed unconditionally: `tools/run-sample-golden.sh` reads it back.
    eprintln!(
        "tide golden: device on adapter {id} {name:?} type={kind:?}",
        id = adapter.id.0,
        name = adapter.name,
        kind = adapter.device_type,
    );
    eprintln!(
        "tide golden: {} {:?} / {:?} / {:?} at {}x{}, arm {arm:?}",
        setup.backend(),
        caps.geometry_path(),
        caps.binding_model(),
        caps.lighting_path(),
        extent.0,
        extent.1,
    );

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    setup.finish().expect("the device reaches idle");
    assert_eq!(
        (width, height),
        extent,
        "the readback is not the extent asked for"
    );
    let order = if format == Format::Bgra8UnormSrgb || format == Format::Bgra8Unorm {
        ChannelOrder::Bgra
    } else {
        ChannelOrder::Rgba
    };
    Image::from_readback(width, height, &pixels, order).expect("the readback is one image")
}

/// The courtyard, staged exactly as `apps/tide/src/gpu.rs` stages it, on a
/// device the caller opened.
fn build(
    device: &dyn crcbl::hal::Device,
    queue: crcbl::hal::QueueHandle,
    format: Format,
    arm: Arm,
) -> Result<ForwardRenderer, crcbl::screenshot::OffscreenError> {
    let mut renderer = scene::renderer(device, queue, format)?;
    if let Err(error) = Stage::new(&mut renderer, Scene::Courtyard, arm.medium) {
        renderer.destroy(device);
        return Err(error.into());
    }
    if !arm.water {
        renderer
            .set_water(&[])
            .expect("an empty set of bodies is always a set");
    }
    if arm.scene_referred {
        renderer.set_tonemap_curve(TonemapCurve::Clamp);
    }
    Ok(renderer)
}

// ---------------------------------------------------------------------------
// Reading the frame
// ---------------------------------------------------------------------------

/// Where a world point lands in a frame of `extent`, through the matrices the
/// frame was drawn with.
fn project(camera: &Camera, extent: (u32, u32), point: Vec3) -> (u32, u32) {
    #[expect(clippy::cast_precision_loss, reason = "a frame edge is a few thousand")]
    let (width, height) = (extent.0 as f32, extent.1 as f32);
    let clip = camera.view_projection(width / height) * point.extend(1.0);
    assert!(clip.w > 0.0, "{point} is behind the camera");
    let ndc = clip.truncate() / clip.w;
    let (x, y) = ((ndc.x + 1.0) * 0.5 * width, (1.0 - ndc.y) * 0.5 * height);
    assert!(
        (0.0..width).contains(&x) && (0.0..height).contains(&y),
        "{point} lands at ({x}, {y}), outside a {}x{} frame",
        extent.0,
        extent.1
    );
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "inside the frame, which the assertion above says"
    )]
    (x as u32, y as u32)
}

/// A band's mean on each channel, in code values, about the world point
/// `point` in a [`CLAIM_EXTENT`] frame.
fn band(image: &Image, point: Vec3) -> [f32; 3] {
    let (cx, cy) = project(&scene::fixed_camera(), CLAIM_EXTENT, point);
    let mut total = [0.0f32; 3];
    let mut count = 0u16;
    for y in cy - BAND.1..=cy + BAND.1 {
        for x in cx - BAND.0..=cx + BAND.0 {
            let pixel = image.pixel(x, y).expect("the band is inside the frame");
            for (sum, value) in total.iter_mut().zip(pixel) {
                *sum += f32::from(value);
            }
            count += 1;
        }
    }
    total.map(|sum| sum / f32::from(count))
}

/// The largest per-channel difference between two bands, in levels.
fn apart(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3)
        .map(|channel| (a[channel] - b[channel]).abs())
        .fold(0.0, f32::max)
}

/// Writes a frame where a reviewer can open it.
fn save(image: &Image, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(REVIEW_DIR);
    std::fs::create_dir_all(&dir).expect("target/ is writable");
    let path = dir.join(format!("{name}.png"));
    image.save_png(&path).expect("the review frame is writable");
    eprintln!("tide golden: {name} frame at {}", path.display());
}

/// A point on the water's surface, at `x` along the pool and `z` across it.
fn surface(x: f32, z: f32) -> Vec3 {
    Vec3::new(x, scene::LEVEL, z)
}

/// Where across the pool every basin band is read.
///
/// Clear of the three things that would put something other than water between
/// a band and its mirror: the near wall's shadow on the floor — the sun is a
/// little behind the camera, and the deep end's taller wall throws a longer
/// shadow than the shallow end's — the lane line's refracted image, and the
/// in-front rejection's unrefracted fallback along the near coping. Chosen off a
/// sweep of both frames across the pool's width (radv, 2026-09-15): here the dry
/// deep and shallow bands read the same code values, which
/// [`MIRRORED_WITHOUT_WATER`] then asserts rather than trusts.
const BASIN_Z: f32 = -1.0;

/// How far along the pool, either side of the camera's column, the basin bands
/// are read: a pair near the column and a pair further out, so a claim is not a
/// fact about one patch of floor.
const BASIN_X: [f32; 2] = [1.5, 3.0];

// ---------------------------------------------------------------------------
// The water is where the pool is, and nowhere else
// ---------------------------------------------------------------------------

/// The least a basin band must move, on its largest channel, when the pool's
/// body is taken away, in levels.
///
/// Measured at 28 levels at worst — the shallow end, whose thinner water moves
/// the tile under it least — and 83 in the deep end (radv; 27 and 82 on
/// lavapipe, 2026-09-15). **Shown red** with `Stage` handing the renderer no
/// body: every basin band moved by 0.00.
const WATER_OVER_TILE: f32 = 14.0;

/// The most a band on the dry stone may move when the body is taken away, in
/// levels.
///
/// Measured at zero on the coping and the paving (radv, 2026-09-15): the water
/// pass draws only where the surface is nearer than what is already there, and
/// the stone stands above the water — and at zero on lavapipe too. Not zero,
/// because a claim about "nothing else" should not rest on two frames of one
/// driver agreeing to the bit. **Shown red** with the body grown a metre past
/// the pool's walls and raised five centimetres over the coping: the far coping
/// moved by 18.56 levels and the near one, seen head-on, by 2.00.
const DRY_STONE_TOLERANCE: f32 = 1.0;

/// The bands on dry stone: the coping on the near and far sides of the pool,
/// and the paving beyond the far coping.
fn dry_stone() -> [(&'static str, Vec3); 3] {
    [
        ("near coping", Vec3::new(2.5, 0.0, scene::POOL_NEAR + 0.22)),
        ("far coping", Vec3::new(2.5, 0.0, scene::POOL_FAR - 0.22)),
        ("far paving", Vec3::new(2.5, 0.0, scene::POOL_FAR - 3.0)),
    ]
}

/// **The courtyard draws its water over the pool, and leaves the stone around
/// it as it was.**
///
/// Two frames of one renderer: the courtyard as a run opens on it, and the same
/// with its body taken away. Every basin band moves — the body reached the
/// renderer and the surface drew over the tile — and every band on the coping
/// and the paving does not.
///
/// **Each half shown red by sabotage** (radv, 2026-09-15) — the edit made, this
/// test run, the file restored — with what each read on the constant it
/// reddened: [`WATER_OVER_TILE`] and [`DRY_STONE_TOLERANCE`].
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-tide-golden.sh"]
fn the_courtyard_draws_water_over_the_pool_and_leaves_the_stone_dry() {
    let wet = draw(CLAIM_EXTENT, Arm::shipped().referred());
    let dry = draw(CLAIM_EXTENT, Arm::shipped().referred().dry());
    save(&wet, "courtyard");
    save(&dry, "courtyard-without-water");

    for x in BASIN_X.into_iter().flat_map(|x| [-x, x]) {
        let point = surface(x, BASIN_Z);
        let (under, over) = (band(&dry, point), band(&wet, point));
        let moved = apart(under, over);
        eprintln!("tide golden: basin at {point} — {under:?} dry, {over:?} wet, {moved:.2} apart");
        assert!(
            moved >= WATER_OVER_TILE,
            "the basin at {point} reads {over:?} with the body and {under:?} without it: \
             {moved:.2} level(s) apart against {WATER_OVER_TILE} — the water is not drawn over \
             the pool"
        );
    }
    // Every band read and printed before any is asserted, so a red run says how
    // far each of them moved rather than only the first.
    let stone = dry_stone().map(|(name, point)| {
        let (under, over) = (band(&dry, point), band(&wet, point));
        let moved = apart(under, over);
        eprintln!("tide golden: {name} — {under:?} dry, {over:?} wet, {moved:.2} apart");
        (name, under, over, moved)
    });
    for (name, under, over, moved) in stone {
        assert!(
            moved <= DRY_STONE_TOLERANCE,
            "the {name} reads {over:?} with the body and {under:?} without it: {moved:.2} \
             level(s) apart against {DRY_STONE_TOLERANCE} — the water reaches past the pool"
        );
    }
}

// ---------------------------------------------------------------------------
// The deep end is deeper
// ---------------------------------------------------------------------------

/// How far the shallow end's red must sit above the deep end's at the mirrored
/// band, in levels, in clear water.
///
/// **Absorption with depth, and red because clear water absorbs red fastest** —
/// `crcbl_tide::medium`'s pure-water row loses red roughly six times as fast as
/// green. Measured at 40 levels by the camera's column and 54 further out, on
/// radv and on lavapipe alike (2026-09-15). **Shown red** with the clear pool's
/// absorption zeroed: the two ends read the same red, 0.00 apart.
const DEEP_UNDER_SHALLOW_RED: f32 = 20.0;

/// The most the dry frame's deep and shallow bands may differ, in levels.
///
/// **The premise the depth claim rests on**: without water the mirrored bands
/// are the same tile, seen at the same angle under the same sun, so any gap the
/// water opens is the water's. Measured at zero on radv and on lavapipe
/// (2026-09-15). **Shown red** with the shallow band read 0.6 m further
/// across the pool, off its mirror: 80.11 levels apart.
const MIRRORED_WITHOUT_WATER: f32 = 1.5;

/// **The deep end reads deeper than the shallow end**, at mirrored bands that
/// read the same without water.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-tide-golden.sh"]
fn the_deep_end_reads_deeper_than_the_shallow_end() {
    let red = 0;
    let wet = draw(CLAIM_EXTENT, Arm::shipped().referred());
    let dry = draw(CLAIM_EXTENT, Arm::shipped().referred().dry());
    for x in BASIN_X {
        let (deep, shallow) = (surface(-x, BASIN_Z), surface(x, BASIN_Z));
        let premise = apart(band(&dry, deep), band(&dry, shallow));
        assert!(
            premise <= MIRRORED_WITHOUT_WATER,
            "without water the bands at {deep} and {shallow} read {:?} and {:?}, {premise:.2} \
             level(s) apart — they are not a mirrored pair, so a gap with water proves nothing",
            band(&dry, deep),
            band(&dry, shallow),
        );
        let (deep, shallow) = (band(&wet, deep), band(&wet, shallow));
        let gap = shallow[red] - deep[red];
        eprintln!(
            "tide golden: at x = ±{x} the deep end reads {deep:?} and the shallow {shallow:?}"
        );
        assert!(
            gap >= DEEP_UNDER_SHALLOW_RED,
            "at x = ±{x} the deep end reads {deep:?} and the shallow end {shallow:?}: red is \
             {gap:.2} level(s) apart against {DEEP_UNDER_SHALLOW_RED} — the water is not \
             absorbing with depth"
        );
    }
}

// ---------------------------------------------------------------------------
// Each medium preset is its own water
// ---------------------------------------------------------------------------

/// How far each preset's blue must fall below the one before it at the deep
/// band, in levels, walking the presets in order of dissolved organic matter.
///
/// **Dissolved matter absorbs blue hardest** — `crcbl_tide::medium`'s
/// `Preset::dissolved` is 0, 0.1, 1.5 and 5 per metre across clear pool, lake,
/// pond and swamp, and its exponential puts most of that on the blue channel.
/// Measured at 247, 242, 175 and 109 levels (radv, 2026-09-15): the smallest
/// step is clear pool to lake, at 5, and lavapipe reads the same four. **Shown
/// red** with `Stage` meshing every preset as the clear pool: clear pool to lake
/// fell by 0.00.
const BLUE_STEP: f32 = 3.0;

/// **Each medium preset draws its own water**: at the deep band, blue falls
/// preset by preset as the dissolved matter in it rises.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-tide-golden.sh"]
fn each_medium_preset_draws_its_own_water() {
    let blue = 2;
    let point = surface(-BASIN_X[0], BASIN_Z);
    let mut presets = Preset::ALL.to_vec();
    presets.sort_by(|a, b| a.dissolved().total_cmp(&b.dissolved()));
    assert!(
        presets.len() > 2,
        "a ladder of {} presets has no order",
        presets.len()
    );
    let mut last: Option<(Preset, [f32; 3])> = None;
    for preset in presets {
        let image = draw(CLAIM_EXTENT, Arm::shipped().in_medium(preset).referred());
        save(&image, &format!("courtyard-{}", preset.label()));
        let reading = band(&image, point);
        eprintln!(
            "tide golden: the deep end in {} reads {reading:?}",
            preset.label()
        );
        if let Some((before, earlier)) = last {
            let step = earlier[blue] - reading[blue];
            assert!(
                step >= BLUE_STEP,
                "the deep end reads {earlier:?} in {} and {reading:?} in {}: blue falls by \
                 {step:.2} level(s) against {BLUE_STEP} — the preset is not the water drawn",
                before.label(),
                preset.label(),
            );
        }
        last = Some((preset, reading));
    }
}

// ---------------------------------------------------------------------------
// The goldens
// ---------------------------------------------------------------------------

/// Holds one frame to its checked-in reference, and hands back what it found —
/// a `Result` so a bless run writes every reference in one pass, on sundial's
/// terms.
fn check_golden(image: &Image, name: &str) -> Result<String, String> {
    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"));
    match Golden::new(reference)
        .check(image)
        .expect("the reference is readable")
        .into_result()
    {
        Ok(comparison) => Ok(format!("{name} — {}", comparison.summary())),
        Err(message) => Err(format!("{name}: {message}")),
    }
}

/// **The four checked-in frames**, one courtyard per medium preset under the
/// shipped tonemap.
///
/// Last, and only after the claims above: a golden says the frame did not
/// change, which is worth having and is not evidence that it was ever right.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-tide-golden.sh"]
fn the_courtyard_matches_its_goldens() {
    let mut faults = Vec::new();
    for preset in Preset::ALL {
        let name = format!("courtyard-{}", preset.label());
        let image = draw(EXTENT, Arm::shipped().in_medium(preset));
        match check_golden(&image, &name) {
            Ok(line) => eprintln!("tide golden: {line}"),
            Err(fault) => faults.push(fault),
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}
