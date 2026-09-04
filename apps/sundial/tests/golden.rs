//! The plaza off a real device, from the fixture camera, against checked-in
//! goldens — and the claims about the shadows in front of them.
//!
//! # A golden alone cannot make a claim about a shadow
//!
//! A plausible dark shape is a plausible dark shape. A PCSS blocker search that
//! never ran draws the same picture the fixed-width disc does; a filter selector
//! wired to one branch draws two identical halves either side of a seam; a depth
//! bias that detached every shadow from its caster leaves a frame full of
//! shadows, just not touching anything. Every one of those produces a frame
//! somebody would bless. So the goldens are the *last* of the assertions here,
//! and the ones before them are about **how wide** the frame's penumbrae are,
//! **which column** ran which filter, **where** the frame is dark, and whether
//! the same tick of the clock draws the same frame twice.
//!
//! `apps/alcove/tests/golden.rs` is the shape all of this follows.
//!
//! # Feature-gated *and* ignored
//!
//! The pair `crcbl`'s `render-e2e` uses. A plain `cargo test --workspace
//! --all-features` on a machine with no GPU must stay green, and
//! `tests/run-sundial-golden.sh` is the only thing that turns both off — and it
//! fails when the suite reports zero tests run.

#![cfg(feature = "golden-e2e")]

use crcbl::console::Value;
use crcbl::hal::{AdapterInfo, Format};
use crcbl::math::Vec3;
use crcbl::render::{Camera, EffectOverride, EffectRequest, ForwardRenderer, RenderEffects};
use crcbl::screenshot::{ForwardScene, OffscreenSetup};
use crcbl_golden::{ChannelOrder, Golden, Image};
use crcbl_sundial::{filter, plaza, sun};

/// The extent the checked-in goldens are blessed at.
const EXTENT: (u32, u32) = (256, 192);

/// The extent the readings that are about *where* the frame is dark are taken
/// at.
///
/// Sixteen times the pixels of [`EXTENT`], and the contact claim is why: the
/// point it reads is five centimetres from the plinth's face, and at [`EXTENT`]
/// a block wide enough to average over covers the face as well as the pavement
/// beside it. A claim about a contact has to be able to resolve the contact.
const CLAIM_EXTENT: (u32, u32) = (1024, 768);

/// The extent the penumbra ladder is measured at, and the review frames written
/// at.
///
/// The narrowest penumbra in the ladder is about three centimetres of pavement.
/// At [`EXTENT`] that is under a pixel — the claim would be a claim about
/// rounding — and at this extent it is a few, which is what the strip average in
/// [`penumbra`] then has something to average.
const REVIEW_EXTENT: (u32, u32) = (1280, 960);

/// Where a review-size frame is written, relative to the workspace root.
const REVIEW_DIR: &str = "target/sundial";

/// The extent the atlas viewer's golden is blessed at.
///
/// **Not [`EXTENT`], and the reason is what the viewer draws.** Every other
/// golden here is a rendering of a scene, where two drivers disagree by a level
/// or so per pixel and `crcbl_golden::Tolerance::RASTERISER` is sized for
/// exactly that. The atlas viewer is a *point sample* of a
/// `crcbl::render::shadow::atlas_extent()` depth image — `atlas_view.slang`
/// `Load`s one texel per pixel and never filters, deliberately, because
/// averaging across a silhouette would draw a height nothing in the scene
/// occupies. So one texel on which radv and lavapipe disagree about coverage is
/// not a level of drift in this picture: it is a whole pixel that reads
/// `OCCUPIED_FLOOR` on one driver and `EMPTY_GREY` on the other.
///
/// What that costs is a function of how much of the atlas the frame samples.
/// **Swept**, blessing on lavapipe and comparing on radv:
///
/// | extent | atlas texels per pixel | grossly wrong | budget |
/// | --- | --- | --- | --- |
/// | 256x192 | 256 | 70 — `0.1424%` | `0.1%` — fails |
/// | 1024x768 | 16 | 312 — `0.0397%` | `0.1%` — passes |
/// | 1280x960 | about 10 | 382 — `0.0311%` | `0.1%` — passes |
///
/// The middle row is this constant. At [`EXTENT`] the picture throws away 255
/// texels in every 256 and is a moire nobody could review anyway; here a
/// reviewer can see the tile grid and the shape of what is in each map, which is
/// what a diagnostic golden is for.
const ATLAS_EXTENT: (u32, u32) = (1024, 768);

/// Half-extents, in pixels, of the block each claim averages over at [`EXTENT`].
const BLOCK: (u32, u32) = (2, 2);

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// One arm of a comparison: which effects, which filter, which seam, which tick
/// of the clock and which pose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Arm {
    /// Which of the render effects this arm draws.
    effects: RenderEffects,
    /// Which filter the near side runs, or `None` for what ships.
    filter: Option<&'static str>,
    /// Where the comparison seam stands in thousandths of the width, or `None`
    /// for a frame comparing nothing.
    ///
    /// Thousandths rather than an `f32` for [`crcbl_sundial::Knobs`]' reason: an
    /// arm is printed into every reading this suite reports and compared against
    /// its neighbours, and both want `Eq`.
    split_permille: Option<u32>,
    /// Which tick of the scripted clock the sun stands at.
    tick: u64,
    /// Whether the frame is taken from [`plaza::counter_camera`] rather than
    /// [`plaza::fixed_camera`].
    counters: bool,
    /// Whether the shadow atlas is drawn over the picture rather than the
    /// picture itself — [`crcbl::render::DebugView::ShadowAtlas`].
    atlas: bool,
}

impl Arm {
    /// **The plaza as the sample ships it**: the default effect stack, the
    /// shipped filter, no seam, at the fixture tick, from the fixture pose.
    ///
    /// [`RenderEffects::DEFAULT_STACK`] rather than [`RenderEffects::all`], and
    /// the contact claim is why — `apps/alcove/tests/golden.rs` argues it in
    /// full: `all()` turns on auto exposure and bloom, and both make a reading
    /// depend on the rest of the frame, so an arm that switches the shadow
    /// passes off is answered by an exposure change and the difference at one
    /// point stops being about that point.
    const fn shipped() -> Self {
        Self {
            effects: RenderEffects::DEFAULT_STACK,
            filter: None,
            split_permille: None,
            tick: sun::FIXTURE_TICK,
            counters: false,
            atlas: false,
        }
    }

    /// The same arm on a named filter.
    const fn on(self, filter: &'static str) -> Self {
        Self {
            filter: Some(filter),
            ..self
        }
    }

    /// The same arm with the comparison seam at `at`.
    fn split_at(self, at: f32) -> Self {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "every caller passes a fraction inside 0..1"
        )]
        let permille = (at * 1000.0).round() as u32;
        Self {
            split_permille: Some(permille),
            ..self
        }
    }

    /// The same arm with the shadow passes out — every claim's control.
    const fn without_shadows(self) -> Self {
        Self {
            effects: RenderEffects::DEFAULT_STACK.difference(RenderEffects::SHADOWS),
            ..self
        }
    }

    /// The same arm at a named tick of the clock.
    const fn at_tick(self, tick: u64) -> Self {
        Self { tick, ..self }
    }

    /// The same arm framed by [`plaza::counter_camera`].
    const fn framed_on_the_counters(self) -> Self {
        Self {
            counters: true,
            ..self
        }
    }

    /// The same arm with the shadow atlas drawn over the picture — what `T` and
    /// the pause panel's `ATLAS` row put up.
    const fn showing_the_atlas(self) -> Self {
        Self {
            atlas: true,
            ..self
        }
    }

    /// Which camera this arm is drawn from.
    fn camera(self) -> Camera {
        if self.counters {
            plaza::counter_camera()
        } else {
            plaza::fixed_camera()
        }
    }

    /// Where the sun stands for this arm.
    fn sky(self) -> sun::Sky {
        sun::Sky::at(self.tick)
    }

    /// Where the seam stands, back as the fraction the console holds.
    fn split(self) -> Option<f32> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a value under a thousand is exact in an f32"
        )]
        self.split_permille.map(|permille| permille as f32 / 1000.0)
    }
}

/// Draws one arm and reads the frame back.
fn draw(extent: (u32, u32), arm: Arm) -> (Image, String, AdapterInfo) {
    // A logger before anything opens: without one, every line a backend emits on
    // the way to a device goes nowhere.
    crcbl::core::log::init_logging();

    // **The console is written here and put back below.** These are process
    // globals; the runner gives every test a process of its own, and this pair
    // is what keeps two arms inside one test from inheriting each other's knobs.
    filter::reset();
    if let Some(name) = arm.filter {
        filter::var(filter::FILTER)
            .set(&Value::Enum(name))
            .expect("the engine declares that filter");
    }
    if let Some(at) = arm.split() {
        filter::var(filter::SPLIT)
            .set(&Value::Float(at))
            .expect("the seam is inside its own range");
    }

    let mut setup = OffscreenSetup::open_forward_with(
        extent.0,
        extent.1,
        OffscreenSetup::OPTIONAL_FEATURES,
        |device, queue, format| {
            Ok(ForwardScene {
                camera: arm.camera(),
                sun: arm.sky().light(),
                renderer: Box::new(build(device, queue, format, arm)?),
            })
        },
    )
    .unwrap_or_else(|why| panic!("a GPU backend opens for sundial's plaza: {why}"));

    let backend = setup.backend();
    let caps = setup.caps();
    let adapter = setup.adapter().clone();
    // Printed unconditionally and read with `--success-output immediate`: on a
    // green run — the run where the selected path is worth knowing — nextest
    // captures this and it is otherwise invisible.
    eprintln!(
        "sundial golden: device on adapter {id} {name:?} type={kind:?}",
        id = adapter.id.0,
        name = adapter.name,
        kind = adapter.device_type,
    );
    let paths = format!(
        "{backend} {:?} / {:?} / {:?}",
        caps.geometry_path(),
        caps.binding_model(),
        caps.lighting_path(),
    );
    eprintln!(
        "sundial golden: {paths} at {}x{}, arm {arm:?}, sun {}",
        extent.0,
        extent.1,
        arm.sky().row(),
    );

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    // Before any assertion: `finish` waits the device idle, and a device lost
    // during the frame surfaces there and nowhere else.
    setup.finish().expect("the device reaches idle");
    filter::reset();

    assert_eq!(
        (width, height),
        extent,
        "the swapchain handed back an extent nothing was measured at"
    );
    let order = if format == Format::Bgra8UnormSrgb || format == Format::Bgra8Unorm {
        ChannelOrder::Bgra
    } else {
        ChannelOrder::Rgba
    };
    let image = Image::from_readback(width, height, &pixels, order)
        .expect("the readback is exactly one image");
    (image, paths, adapter)
}

/// The plaza, made resident and placed, on a device the caller opened.
fn build(
    device: &dyn crcbl::hal::Device,
    queue: crcbl::hal::QueueHandle,
    format: Format,
    arm: Arm,
) -> Result<ForwardRenderer, crcbl::screenshot::OffscreenError> {
    let scene = plaza::plaza();
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    // The **programmatic** layer of topic 39's resolution order, which is the
    // one a test has any business driving.
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::all().difference(arm.effects), Some(false)),
        ..EffectRequest::default()
    });
    if let Err(error) = plaza::place(&mut renderer) {
        renderer.destroy(device);
        return Err(crcbl::screenshot::OffscreenError::Hal(
            crcbl::hal::HalError::InvalidDescriptor(format!(
                "sundial's plaza does not fit its own instance pool: {error}"
            )),
        ));
    }
    // The punctual lights, which the atlas allocator then gives runs of tiles
    // to — `plaza`'s `every_light_in_the_plaza_is_given_a_run_of_tiles` is what
    // holds the three of them inside the budget with no GPU.
    renderer.set_lights(&plaza::lights());
    // The atlas viewer, which is a **pass** rather than a lane of the frame
    // block — so an arm that does not ask for it records no pass at all and the
    // four goldens blessed before it existed are untouched by its being here.
    renderer.set_atlas_view(arm.atlas);
    Ok(renderer)
}

// ---------------------------------------------------------------------------
// Reading the frame
// ---------------------------------------------------------------------------

/// Where a world point lands in the frame, in pixels.
///
/// Through the very same [`Camera::view_projection`] the frame was drawn with,
/// so a claim about a surface is a claim about the pixels that surface actually
/// covers.
fn project(camera: &Camera, extent: (u32, u32), point: Vec3) -> (u32, u32) {
    #[allow(clippy::cast_precision_loss)]
    let aspect = extent.0 as f32 / extent.1 as f32;
    let clip = camera.view_projection(aspect) * point.extend(1.0);
    assert!(
        clip.w > 0.0,
        "{point:?} is behind the camera, so nothing in the frame is about it"
    );
    let ndc = clip.truncate() / clip.w;
    #[allow(clippy::cast_precision_loss)]
    let (width, height) = (extent.0 as f32, extent.1 as f32);
    let x = (ndc.x + 1.0) * 0.5 * width;
    let y = (1.0 - ndc.y) * 0.5 * height;
    assert!(
        x >= 0.0 && x < width && y >= 0.0 && y < height,
        "{point:?} projects to ({x:.1}, {y:.1}), outside a {width}x{height} frame — \
         the claim about it would be about a pixel that is not there"
    );
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (x as u32, y as u32)
}

/// Mean luminance of a block of pixels around `centre`, out of 255.
fn brightness(image: &Image, centre: (u32, u32), half: (u32, u32)) -> f32 {
    let (mut total, mut count) = (0.0f32, 0u32);
    let x0 = centre.0.saturating_sub(half.0);
    let y0 = centre.1.saturating_sub(half.1);
    let x1 = (centre.0 + half.0).min(image.width().saturating_sub(1));
    let y1 = (centre.1 + half.1).min(image.height().saturating_sub(1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let pixel = image
                .pixel(x, y)
                .unwrap_or_else(|| panic!("({x}, {y}) is inside the frame"));
            total += (f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2])) / 3.0;
            count += 1;
        }
    }
    assert!(count > 0, "an empty block at {centre:?} measures nothing");
    #[allow(clippy::cast_precision_loss)]
    {
        total / count as f32
    }
}

/// [`BLOCK`] scaled to `extent`.
fn block_for(extent: (u32, u32)) -> (u32, u32) {
    let scale = (extent.0 / EXTENT.0).max(1);
    (BLOCK.0 * scale, BLOCK.1 * scale)
}

/// Writes a frame where a reviewer can open it.
fn save(image: &Image, name: &str, extent: (u32, u32)) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(REVIEW_DIR);
    std::fs::create_dir_all(&dir).expect("target/ is writable");
    let path = dir.join(format!("{name}-{}x{}.png", extent.0, extent.1));
    image.save_png(&path).expect("the review frame is writable");
    eprintln!("sundial golden: {name} frame at {}", path.display());
}

/// Mean absolute luminance difference down one column of two frames.
///
/// The unit the seam claim is measured in: a column either agrees with a
/// reference frame or it does not, and reducing it to one number is what makes
/// "to the column" a thing a test can say.
fn column_difference(a: &Image, b: &Image, x: u32) -> f32 {
    assert_eq!(
        (a.width(), a.height()),
        (b.width(), b.height()),
        "two frames of different sizes have no columns in common"
    );
    let mut total = 0.0f32;
    for y in 0..a.height() {
        let (pa, pb) = (
            a.pixel(x, y).expect("inside the frame"),
            b.pixel(x, y).expect("inside the frame"),
        );
        let mean = |p: [u8; 4]| (f32::from(p[0]) + f32::from(p[1]) + f32::from(p[2])) / 3.0;
        total += (mean(pa) - mean(pb)).abs();
    }
    #[allow(clippy::cast_precision_loss)]
    {
        total / a.height() as f32
    }
}

// ---------------------------------------------------------------------------
// Measuring a penumbra
// ---------------------------------------------------------------------------

/// How far out from a shadow's centre [`penumbra`] walks, in metres.
///
/// Past the caster's own half-width and the widest penumbra the ladder reaches,
/// and short of the next counter along — the counters stand 1.25 m apart, and
/// `plaza`'s `the_counters_shadows_stand_apart_from_each_other_and_from_
/// everything_else` is what holds that.
const SCAN_METRES: f32 = 0.55;

/// How far apart the samples along that walk stand, in metres.
///
/// Under a pixel at [`REVIEW_EXTENT`] from [`plaza::counter_camera`], so the
/// profile is sampled at least as finely as the frame resolves it.
const SCAN_STEP: f32 = 0.004;

/// Where along the shadow's edge the strip average is taken, in metres either
/// side of the centre.
///
/// The claim is about the profile **across** the edge; averaging a few samples
/// **along** it costs nothing and takes the readback's per-pixel noise out of
/// the two crossings [`penumbra`] then looks for.
const STRIP: [f32; 5] = [-0.06, -0.03, 0.0, 0.03, 0.06];

/// What one walk across a shadow's edge found.
#[derive(Clone, Copy, Debug)]
struct Profile {
    /// Mean luminance at the shadow's centre, out of 255.
    umbra: f32,
    /// The brightest reading anywhere along the walk, out of 255.
    lit: f32,
    /// How far apart the fifth and the ninety-fifth per cent crossings stand, in
    /// metres of pavement.
    width: f32,
}

/// Walks `+x` from `centre` across a shadow's edge and measures the ramp.
///
/// **Ten to ninety per cent of the step**, which is how an edge's width is
/// stated everywhere else it is stated: the two ends of a penumbra approach the
/// umbra and the lit value asymptotically, so a crossing measured at the very
/// ends is a measurement of where the noise floor is.
///
/// The walk is in **world metres and not in pixels**, and that is what makes the
/// three counters comparable: they stand at different distances from the counter
/// pose's eye, so a width in pixels would carry the projection's scale as well as
/// the filter's.
fn penumbra(image: &Image, camera: &Camera, extent: (u32, u32), centre: Vec3) -> Profile {
    /// Where on the ramp the two crossings are read.
    const LOW: f32 = 0.1;
    /// The far end of the same ramp.
    const HIGH: f32 = 0.9;

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = (SCAN_METRES / SCAN_STEP) as u32;
    let mut ramp = Vec::with_capacity(steps as usize + 1);
    for step in 0..=steps {
        let along = f32::from(u16::try_from(step).expect("the walk is a few hundred steps"));
        let out = along * SCAN_STEP;
        let mut total = 0.0f32;
        for offset in STRIP {
            let at = centre + Vec3::new(out, 0.0, offset);
            total += brightness(image, project(camera, extent, at), (0, 0));
        }
        #[allow(clippy::cast_precision_loss)]
        ramp.push((out, total / STRIP.len() as f32));
    }

    let umbra = ramp[0].1;
    let lit = ramp
        .iter()
        .map(|(_, value)| *value)
        .fold(f32::MIN, f32::max);
    let span = lit - umbra;
    let crossing = |fraction: f32| -> f32 {
        let target = fraction.mul_add(span, umbra);
        ramp.iter()
            .find(|(_, value)| *value >= target)
            .map_or(SCAN_METRES, |(out, _)| *out)
    };
    Profile {
        umbra,
        lit,
        width: crossing(HIGH) - crossing(LOW),
    }
}

// ---------------------------------------------------------------------------
// The penumbra ladder
// ---------------------------------------------------------------------------

/// How much wider than the lowest counter's penumbra the tallest one's must be
/// under `pcss`, as a ratio.
///
/// **Swept, not guessed.** The three penumbrae come out 0.0440, 0.0640 and
/// 0.1080 m of pavement on radv (AMD Radeon RX 7900 XTX, RADV NAVI31) and the
/// same three on lavapipe (llvmpipe) — a ratio of 2.455 on both — so this is set
/// well under what was seen, because it is a floor on a real effect rather than
/// a second golden written in numbers. The run prints all three again on
/// whatever adapter it opened.
///
/// The shape of the ladder is `sun_penumbra_texels`': the estimate is clamped
/// into two-to-eight texels of the cascade the fragment landed in, so the lowest
/// counter reads the lower clamp — which is exactly the fixed width `disc` uses
/// everywhere, and is why the two filters agree on that rung and only that one.
const PCSS_LADDER: f32 = 1.8;

/// How far apart the same two readings may stand under `disc`, as a ratio.
///
/// The control, and the half that makes the ladder a claim about the blocker
/// search rather than about shadows getting blurrier with distance: `disc` takes
/// the same fixed reach at every separation.
///
/// **Swept:** 0.0440, 0.0480 and 0.0440 m on both adapters — a ratio of 1.000
/// between the two ends and a widest spread of 1.09 between any pair, which is
/// one step of [`SCAN_STEP`]. The bound is over that and far under the 2.455
/// `pcss` reaches on the same three casters.
const DISC_FLATNESS: f32 = 1.25;

/// **The penumbra widens with the gap under `pcss`, and does not under `disc`.**
///
/// `docs/plan/sample/18-sundial.md`'s first acceptance claim, and the one the
/// counters were laid out for: three cubes of one size hanging at graded heights
/// over one plane, so the only thing that differs between their three shadows is
/// the distance from blocker to receiver.
///
/// Both halves are needed and neither is enough. A filter whose width grew with
/// *depth from the eye* would pass the `pcss` half; a `disc` that quietly ran the
/// blocker search would pass nothing. The `disc` arm is what says the widening
/// came from the search rather than from the scene.
/// # How it was shown to fail
///
/// By naming `disc` in the `pcss` arm and `pcss` in the `disc` one, one at a
/// time. The first came back with a ratio of 1.000 against a floor of
/// [`PCSS_LADDER`]; the second with 2.455 against a ceiling of
/// [`DISC_FLATNESS`]. Each half fails on the other half's frames, which is what
/// says the two are measuring the same thing about two different filters.
///
/// # What was measured
///
/// The figures are on [`PCSS_LADDER`] and [`DISC_FLATNESS`], and the run prints
/// them again on whatever adapter it opened.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sundial-golden.sh"]
fn the_penumbra_widens_with_its_casters_height_under_pcss_and_not_under_disc() {
    let extent = REVIEW_EXTENT;
    let camera = plaza::counter_camera();
    let sky = sun::Sky::at(sun::NOON_TICK);

    for (name, least, most) in [
        ("pcss", PCSS_LADDER, f32::MAX),
        ("disc", 0.0, DISC_FLATNESS),
    ] {
        let arm = Arm::shipped()
            .framed_on_the_counters()
            .at_tick(sun::NOON_TICK)
            .on(name);
        let (image, paths, _) = draw(extent, arm);
        save(&image, &format!("counters-{name}"), extent);

        let mut widths = Vec::new();
        for index in 0..plaza::COUNTERS.len() {
            let at = plaza::counter_shadow(index, sky);
            let profile = penumbra(&image, &camera, extent, at);
            eprintln!(
                "sundial golden: {name} counter {index} (gap {gap:.2} m) on {paths} — umbra \
                 {umbra:.2}/255, lit {lit:.2}, penumbra {width:.4} m",
                gap = plaza::COUNTERS[index].1,
                umbra = profile.umbra,
                lit = profile.lit,
                width = profile.width,
            );
            assert!(
                profile.lit > profile.umbra * 1.5,
                "counter {index}'s shadow reads {umbra:.2}/255 at its centre and the pavement \
                 beside it {lit:.2} — there is no edge here to measure the width of",
                umbra = profile.umbra,
                lit = profile.lit,
            );
            assert!(
                profile.width > 0.0,
                "counter {index}'s edge crosses the ramp at one sample under {name}, so its \
                 width is below what this walk resolves"
            );
            widths.push(profile.width);
        }

        let ratio = widths[2] / widths[0];
        eprintln!(
            "sundial golden: {name} on {paths} — penumbrae {:.4} / {:.4} / {:.4} m, tallest over \
             lowest {ratio:.3}",
            widths[0], widths[1], widths[2]
        );
        assert!(
            ratio > least && ratio < most,
            "under {name} the tallest counter's penumbra is {:.4} m and the lowest one's \
             {:.4} m — a ratio of {ratio:.3}, outside {least}..{most}",
            widths[2],
            widths[0],
        );
        if name == "pcss" {
            assert!(
                widths[1] > widths[0] && widths[2] > widths[1],
                "the three penumbrae are {:.4} / {:.4} / {:.4} m, which is not a ladder — two \
                 readings can be ordered by chance and the middle one is what stops that",
                widths[0],
                widths[1],
                widths[2],
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The contact
// ---------------------------------------------------------------------------

/// How much of the contact's light the shadow passes must take away.
///
/// The peter-panning half of `docs/plan/sample/18-sundial.md`'s charter: a
/// shadow biased far enough towards its light detaches from the object casting
/// it, and the first pavement to light up is the strip against the block's face.
///
/// **Swept:** the contact reads 163.17/255 with the shadow term out and 71.91
/// with it in on radv (AMD Radeon RX 7900 XTX, RADV NAVI31) and 163.15 against
/// 71.96 on lavapipe (llvmpipe) — a darkening of 0.5593 and 0.5589 — and 0.5713
/// and 0.5707 at [`REVIEW_EXTENT`], where the block covers less of the strip.
/// The bound is well under the lowest of those.
const CONTACT_DARKENING: f32 = 0.4;

/// How far the open pavement may move, in 0–255 codes, when the shadow passes
/// are switched off.
///
/// The control. Nothing in the plaza reaches
/// [`plaza::OPEN_PAVEMENT`] — not the sun at any tick of the clock, not any of
/// the three punctual lights — so the shadow term there is one and the two
/// frames must agree exactly. A tolerance below one code is what makes this a
/// statement about that point rather than about the frame's average.
///
/// **Swept:** 213.62 against 213.61 out of 255 on radv and 213.51 against 213.49
/// on lavapipe — the same number to a fiftieth of a code, not a number inside a
/// tolerance.
const OPEN_PAVEMENT_TOLERANCE: f32 = 0.5;

/// How near the top of the range the control point may sit, out of 255.
///
/// A control that reads 255 answers nothing: it is as bright as the encoding
/// goes, so an exposure that doubled would leave it where it is and the
/// comparison beside it would still pass. `crate::sun`'s own `INTENSITY` is set
/// by this, and the point reads 213.5 out of 255 on both adapters.
const CONTROL_CEILING: f32 = 250.0;

/// **The pavement against the plinth is dark, and it is the shadow term that
/// darkened it.**
///
/// Two frames and two points. [`plaza::PLINTH_CONTACT`] is pavement five
/// centimetres from the block's near face and inside the block's own shadow at
/// every tick of the clock; [`plaza::OPEN_PAVEMENT`] is pavement nothing in the
/// scene reaches, at any tick, from the sun or from any of the three punctual
/// lights. `plaza`'s own tests are what hold both of those with no GPU.
///
/// Each point is read **with the shadow passes and without them**, rather than
/// against each other: they are different pieces of pavement with different
/// neighbours, and the occlusion pass in the default stack darkens the one
/// beside a block whether a shadow reaches it or not. The difference the passes
/// make at one point is a claim about the passes; the difference between two
/// points is a claim about the scene.
///
/// The open pavement is what stops that being a claim about the whole frame: a
/// tonemap that lost a stop, an exposure that moved, an ambient term that went
/// away — each darkens the contact *and* the control, and only the second
/// assertion here notices.
/// # How it was shown to fail
///
/// By reading the contact block at [`plaza::OPEN_PAVEMENT`] — which is what a
/// shadow that had come off its caster would look like from here, and points the
/// assertion the way it would point. The darkening came out at 0.0001 and the
/// assertion failed. The control was shown to fail separately, by moving
/// [`CONTROL_CEILING`] under the reading: the run then refused a control it
/// could not have detected a brightening at.
///
/// # What was measured
///
/// The figures are on [`CONTACT_DARKENING`] and [`OPEN_PAVEMENT_TOLERANCE`].
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sundial-golden.sh"]
fn the_shadow_reaches_the_pavement_the_plinth_stands_on() {
    let extent = CLAIM_EXTENT;
    let camera = plaza::fixed_camera();
    let block = block_for(extent);
    let contact = project(&camera, extent, plaza::PLINTH_CONTACT);
    let open = project(&camera, extent, plaza::OPEN_PAVEMENT);

    let (shadowed, paths, _) = draw(extent, Arm::shipped());
    let (flat, _, _) = draw(extent, Arm::shipped().without_shadows());

    let (dark, plain) = (
        brightness(&shadowed, contact, block),
        brightness(&flat, contact, block),
    );
    let darkening = (plain - dark) / plain.max(1e-3);
    eprintln!(
        "sundial golden: the contact on {paths} — {plain:.2}/255 with no shadow term, {dark:.2} \
         with it, darkening {darkening:.4}"
    );

    let (control, control_flat) = (
        brightness(&shadowed, open, block),
        brightness(&flat, open, block),
    );
    eprintln!(
        "sundial golden: the open pavement — {control_flat:.2}/255 with no shadow term, \
         {control:.2} with it"
    );
    assert!(
        control_flat < CONTROL_CEILING,
        "the open pavement reads {control_flat:.2}/255, at the top of the range — it cannot get \
         any brighter, so it is not a control on anything"
    );
    assert!(
        (control_flat - control).abs() < OPEN_PAVEMENT_TOLERANCE,
        "the open pavement moved from {control_flat:.2}/255 to {control:.2} when the shadow \
         passes were switched off. Nothing in the plaza casts onto it, so what moved was the \
         whole frame and the darkening above is not about the plinth"
    );
    assert!(
        darkening > CONTACT_DARKENING,
        "the pavement against the plinth reads {plain:.2}/255 with no shadow term and {dark:.2} \
         with it — a darkening of {darkening:.4}, short of {CONTACT_DARKENING}. The block's \
         shadow has come off the block"
    );
}

// ---------------------------------------------------------------------------
// The comparison seam
// ---------------------------------------------------------------------------

/// How many pixels either side of the seam the column-exact comparison skips.
///
/// **Not slack: the antialiasing's footprint**, and taken from
/// `crates/crcbl/tests/forward_e2e/shadow.rs`, which measures the same seam on
/// the engine's own fixture and states the reasoning — SMAA's blend weights
/// reach `MAX_SEARCH_STEPS` texels each way along an edge, so for a band either
/// side of the line a pixel is a mixture of the two filters and belongs to
/// neither reference frame. A **texel** count and not a fraction of the frame,
/// so the same constant holds at every extent.
const SEAM_BLEED: u32 = 32;

/// **The seam runs the console's filter on the left and the shipped one on the
/// right, to the column.**
///
/// The comparison the filter selector exists for, from the sample's side.
/// `crcbl-render`'s own suite shows the seam on the engine's fixture; this shows
/// it on a scene a person can look at, and from the pose where the two filters
/// are furthest apart.
///
/// Three frames — the console's filter everywhere, the shipped one everywhere,
/// and the seamed one — and then every column of the seamed frame is held
/// against **both**. Outside [`SEAM_BLEED`] the agreement is exact, byte for
/// byte, and the disagreement with the other reference is what stops the whole
/// thing being vacuous: two identical references would satisfy the equality half
/// perfectly.
/// # How it was shown to fail
///
/// Twice, once per half. Swapping the two references, so each side is compared
/// against the filter the other side ran, failed at column 469 — the first
/// column near enough the seam to carry a shadow edge at all. Naming the shipped
/// filter on both sides left every column exact and both halves 0.000/255 apart,
/// and the anti-vacuity assertion failed instead, which is the half that catches
/// a selector wired to one branch.
///
/// # What was measured
///
/// 961 of the 1024 columns are compared — the rest are the bleed band — and
/// every one of them is exact on both adapters. The two filters stand 3.110 and
/// 324.498/255 apart down the two halves on radv and 3.101 and 324.678 on
/// lavapipe, so the equality is not an equality of two identical pictures. The
/// left half is the thinner of the two because it is mostly pavement with no
/// shadow edge crossing it, which is why this is asserted per half rather than
/// per column.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sundial-golden.sh"]
fn the_seam_runs_the_console_filter_on_the_left_and_the_shipped_one_on_the_right() {
    let extent = CLAIM_EXTENT;
    let shipped = crcbl::render::shadow::shipped_filter().label();
    let moved = filter::names(filter::FILTER)
        .iter()
        .copied()
        .find(|name| *name != shipped)
        .expect("the engine declares a filter other than the shipped one");

    let pose = Arm::shipped()
        .framed_on_the_counters()
        .at_tick(sun::NOON_TICK);
    let (whole_moved, paths, _) = draw(extent, pose.on(moved));
    let (whole_shipped, _, _) = draw(extent, pose.on(shipped));
    let (seamed, _, _) = draw(extent, pose.on(moved).split_at(filter::SEAM_CENTRE));

    let seam = extent.0 / 2;
    let mut columns = 0u32;
    // What the two filters do to each half on their own, which is what says the
    // exactness below separates anything. Per **half** and not per column: most
    // of this frame is pavement no shadow edge crosses, and the two filters agree
    // to the byte there — a demand that every single column differ would be a
    // demand that the whole frame be a penumbra.
    let mut apart = [0.0f32; 2];
    for x in 0..extent.0 {
        if x.abs_diff(seam) < SEAM_BLEED {
            continue;
        }
        let near_side = x < seam;
        let mine = if near_side {
            &whole_moved
        } else {
            &whole_shipped
        };
        let side = if near_side { moved } else { shipped };
        let agreement = column_difference(&seamed, mine, x);
        assert!(
            agreement == 0.0,
            "column {x} of the seamed frame differs from the whole-frame {side} run by \
             {agreement:.3}/255. With the seam at {} the {} of the frame is meant to be that \
             filter and nothing else",
            filter::SEAM_CENTRE,
            if near_side { "left" } else { "right" },
        );
        apart[usize::from(!near_side)] += column_difference(&whole_moved, &whole_shipped, x);
        columns += 1;
    }
    assert!(
        columns > 0,
        "the bleed band swallowed the whole frame, so nothing was compared"
    );
    eprintln!(
        "sundial golden: the seam on {paths} — {columns} columns exact, the two filters {:.3} \
         and {:.3}/255 apart down the two halves",
        apart[0], apart[1]
    );
    for (side, total) in ["left", "right"].into_iter().zip(apart) {
        assert!(
            total > 0.0,
            "the {moved} and {shipped} filters draw the same {side} half, so the exactness \
             asserted for that side of the seam separates nothing"
        );
    }
}

// ---------------------------------------------------------------------------
// The scripted clock
// ---------------------------------------------------------------------------

/// Which ticks the determinism check draws.
///
/// The fixture tick, one a third of the way round the sweep and one at the top
/// of it — `sun::SWEEP_TICKS` apart in azimuth as well as in elevation, so the
/// frames differ for two reasons rather than one.
const REPLAYED_TICKS: [u64; 3] = [sun::FIXTURE_TICK, 200, sun::NOON_TICK];

/// **The same tick of the clock draws the same frame, and different ticks draw
/// different ones.**
///
/// `docs/plan/sample/18-sundial.md` asks for a sun that is scripted rather than
/// wall-clock, and this is what that is worth having for: a fixture whose sun
/// moved with the frame rate could not be blessed, and one whose sun did not move
/// at all would pass every byte-identity check ever written.
///
/// So both halves. Each tick is drawn twice, in one process, and compared as
/// **bytes** — not by a mean, not through a tolerance, because a scripted clock
/// that reproduces to a tolerance is a clock that does not reproduce. Then each
/// tick's frame is held against the first one's, which is what says the clock is
/// connected to the sun at all.
/// # How it was shown to fail
///
/// Twice, once per half. Drawing the second frame of a pair one tick along broke
/// the byte identity at the first tick. Setting two entries of
/// [`REPLAYED_TICKS`] to the same tick — a sun that does not move — left the
/// identity half perfectly satisfied and failed the difference half, which is
/// the one that catches a clock nothing reads.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sundial-golden.sh"]
fn the_scripted_sun_replays_a_tick_exactly_and_a_different_tick_differently() {
    let extent = EXTENT;
    let mut first: Option<Image> = None;
    for tick in REPLAYED_TICKS {
        let arm = Arm::shipped().at_tick(tick);
        let (once, paths, _) = draw(extent, arm);
        let (twice, _, _) = draw(extent, arm);
        eprintln!(
            "sundial golden: tick {tick} on {paths} — {} drawn twice",
            sun::Sky::at(tick).row()
        );
        assert!(
            once.pixels() == twice.pixels(),
            "tick {tick} drew two different frames in one process. The clock is a pure function \
             of its tick, so the only thing that could have moved between them is something \
             reading the wall"
        );
        match &first {
            None => first = Some(once),
            Some(start) => {
                assert!(
                    start.pixels() != once.pixels(),
                    "tick {tick} draws the same frame as tick {}, so the clock is not reaching \
                     the sun and the byte-identity above is a check on a still picture",
                    REPLAYED_TICKS[0]
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The atlas viewer
// ---------------------------------------------------------------------------

/// Where the atlas is drawn in a frame of `extent`, in pixels: `xy` the corner
/// and `zw` the size.
///
/// The renderer's own letterbox rather than a second one — `begin_frame` writes
/// the viewer's block with this very function — so a change to how the atlas is
/// fitted moves the readings below with it instead of leaving them measuring
/// whatever now falls there. The slot rectangles decide nothing about the
/// letterbox and are passed empty.
fn atlas_on_screen(extent: (u32, u32)) -> [f32; 4] {
    crcbl::shaders::atlas_view::AtlasViewParams::letterboxed(
        extent,
        crcbl::render::shadow::atlas_extent(),
        [[0.0; 4]; crcbl::shaders::mesh::SHADOW_ATLAS_TILES],
    )
    .view
}

/// Where the near cascade's root cell is drawn in a frame of `extent`:
/// `(x, y, width, height)` in pixels, right and bottom exclusive.
///
/// Root cell [`CASCADE_CELL`] placed through [`crcbl::render::shadow`]'s own
/// grid, which is what makes every reading below a reading taken **at** the
/// atlas's geometry rather than one found by looking for amber. The sun's near
/// cascade is rendered on every frame this fixture draws, so the cell is
/// occupied and the viewer borders it.
fn cascade_cell_on_screen(extent: (u32, u32)) -> (u32, u32, u32, u32) {
    let view = atlas_on_screen(extent);
    let (origin_x, origin_y) = crcbl::render::shadow::tile_origin(CASCADE_CELL);
    let (atlas_width, atlas_height) = crcbl::render::shadow::atlas_extent();
    let side = crcbl::render::shadow::TILE;
    #[allow(clippy::cast_precision_loss)]
    let (across, down) = (atlas_width as f32, atlas_height as f32);
    #[allow(clippy::cast_precision_loss)]
    let (origin, span) = (
        (origin_x as f32 / across, origin_y as f32 / down),
        (side as f32 / across, side as f32 / down),
    );
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rect = (
        origin.0.mul_add(view[2], view[0]) as u32,
        origin.1.mul_add(view[3], view[1]) as u32,
        (span.0 * view[2]) as u32,
        (span.1 * view[3]) as u32,
    );
    assert!(
        rect.2 > 0 && rect.3 > 0 && rect.0 + rect.2 <= extent.0 && rect.1 + rect.3 <= extent.1,
        "root cell {CASCADE_CELL} lands at {rect:?}, which is not a rectangle of a {extent:?} frame"
    );
    rect
}

/// The red channel at `at`, which is the grey wherever the viewer drew one.
fn level_at(image: &Image, at: (u32, u32)) -> f32 {
    f32::from(image.pixel(at.0, at.1).expect("inside the frame")[0])
}

/// How far red leads blue at `at`, in 0-255 codes.
///
/// The reading that separates the tile borders from every grey in the picture:
/// `crcbl::shaders::atlas_view::BORDER_TINT` is amber and everything else the
/// viewer draws is a grey, so the two are not on one axis at all.
fn tint_at(image: &Image, at: (u32, u32)) -> f32 {
    let pixel = image.pixel(at.0, at.1).expect("inside the frame");
    f32::from(pixel[0]) - f32::from(pixel[2])
}

/// Which root cell of the atlas the sun's near cascade is rendered into.
///
/// `crcbl::render::shadow::tile_origin`'s order — the cascades take the first
/// cells of the top row — and the same cell
/// `crates/crcbl/tests/forward_e2e/shadow.rs` reads the engine's own fixture at.
const CASCADE_CELL: usize = 0;

/// How far red must lead blue for a pixel to be one of the tile borders, in
/// 0-255 codes.
///
/// **Swept, not guessed**, on both Vulkan adapters this workspace runs locally,
/// over the very frame this check draws, at [`ATLAS_EXTENT`] into an
/// `Rgba8UnormSrgb` swapchain:
///
/// | reading | radv | lavapipe |
/// | --- | --- | --- |
/// | the cascade cell's edge, red over blue | `147.0` | `147.0` |
/// | the cascade cell's middle, red over blue | `0.0` | `0.0` |
/// | the cascade cell's middle, out of 255 | `154.0` | `154.0` |
/// | the letterbox, out of 255 | `0.0` | `0.0` |
///
/// The two adapters agree to the level on every one of them. Floored at roughly
/// half the lead the border actually has, which is still far above anything a
/// grey can produce — the greys measure `0.0`, because a grey has no lead at
/// all.
///
/// `crates/crcbl/tests/forward_e2e/shadow.rs` measures the same amber on the
/// engine's own fixture and reads the same `147.0`, which is the tell that both
/// are looking at `crcbl::shaders::atlas_view::BORDER_TINT` rather than at
/// something their own scene happened to put there.
const ATLAS_BORDER_LEVELS: f32 = 70.0;

/// How far above the letterbox an atlas tile has to draw, in 0-255 codes.
///
/// The other half of what the picture is for. `atlas_view.slang` asserts
/// `SURROUND < EMPTY_GREY` at compile time so that the atlas's own edge is
/// visible; this is that constant assertion read off a device, and it is what
/// says a reviewer can tell how much of the frame the atlas covers.
///
/// **Swept** with the border above: the cell reads `154.0` against a letterbox
/// of `0.0` on both adapters, because a caster stands in the near cascade at the
/// pixel this reads. Floored at roughly half of what an *empty* tile draws
/// instead — `crcbl::shaders::atlas_view::EMPTY_GREY` through the swapchain's
/// encode, which the same sweep in `crates/crcbl/tests/forward_e2e/shadow.rs`
/// measures at `69.0` — because that is the darker of the two states this
/// reading can legitimately be in, and a bound above it would be a bound that
/// fails the day the cascade moves off this caster.
const ATLAS_TILE_OVER_SURROUND: f32 = 32.0;

/// **The atlas viewer draws the atlas, borders the slot the near cascade was
/// rendered into, and letterboxes the rest of the frame to black.**
///
/// `docs/plan/sample/18-sundial.md`'s milestone 1 diagnostic, from the sample's
/// side. `crates/crcbl/tests/forward_e2e/shadow.rs` holds the grey the viewer
/// draws to the depth a CPU readback finds at the very texel the shader sampled;
/// what is added here is that the picture reaches **this** fixture's frame — the
/// plaza, at the pose the goldens are blessed from, through the sample's own
/// effect stack — and a golden of it, so the picture a reviewer presses `T` for
/// is one that cannot change unnoticed.
///
/// Three readings, each placed from the atlas's own geometry rather than found
/// by looking for amber:
///
/// * the middle of the near cascade's cell edge, which has to be the border
///   tint and therefore off the grey axis entirely;
/// * the middle of that same cell, which is a grey and therefore on it, and
///   which has to stand clear of the letterbox;
/// * a pixel outside the letterbox, which is the surround.
///
/// **Anti-vacuity.** A viewer that painted the whole frame amber would pass the
/// border reading on its own, and the second reading is what refuses it; a
/// viewer that drew nothing at all would leave the frame the plaza, which is lit
/// out to its own edges, and the surround reading is what refuses that. The gap
/// between the cell and the surround is `atlas_view.slang`'s own
/// `SURROUND < EMPTY_GREY` — a compile-time assertion there — read off a device
/// here.
///
/// # How it was shown to fail
///
/// Six runs, one per thing this check says.
///
/// * **The viewer never ran**, by building the renderer with
///   `set_atlas_view(false)` whatever the arm asked for: the plaza's own frame
///   *leads blue over red* at the cell's edge, so the border reading came back
///   at `-20.0` against a floor of [`ATLAS_BORDER_LEVELS`].
/// * **The border read where there is no border**, at the cell's middle: `0.0`.
/// * **The grey read on the border**, which is what a picture drawn entirely in
///   the tint would look like from here: `147.0`, and the anti-vacuity half
///   refused it.
/// * **The surround read inside the atlas**: `154.0` where the letterbox is
///   asserted exact.
/// * **The tile's grey read on the letterbox**, which is what a viewer whose
///   empty tiles were the surround would draw: a gap of `0.0` against
///   [`ATLAS_TILE_OVER_SURROUND`].
/// * **The golden compared against another tick of the sun**, which moves the
///   cascades in the atlas and nothing else this check reads: every reading
///   above still passed and the comparison failed at `1.2193%` grossly wrong
///   against a `0.1%` budget. That is the half that says the picture is held to
///   *this* frame rather than to any frame with a grid on it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sundial-golden.sh"]
fn the_atlas_viewer_draws_the_atlas_and_borders_the_cascade_it_rendered() {
    let extent = ATLAS_EXTENT;
    let (image, paths, _) = draw(extent, Arm::shipped().showing_the_atlas());

    let (x, y, width, height) = cascade_cell_on_screen(extent);
    let border_at = (x + width / 2, y);
    let cell_at = (x + width / 2, y + height / 2);
    let surround_at = (0, extent.1 / 2);
    let (border, cell_tint) = (tint_at(&image, border_at), tint_at(&image, cell_at));
    let (cell, surround) = (level_at(&image, cell_at), level_at(&image, surround_at));
    eprintln!(
        "sundial golden: the atlas viewer on {paths} — drawn at {view:?}, root cell \
         {CASCADE_CELL} at {rect:?}; the border at {border_at:?} leads red over blue by \
         {border:.1} and the cell's middle at {cell_at:?} by {cell_tint:.1}; the cell reads \
         {cell:.1}/255 and the letterbox at {surround_at:?} {surround:.1}",
        view = atlas_on_screen(extent),
        rect = (x, y, width, height),
    );

    assert!(
        border > ATLAS_BORDER_LEVELS,
        "the edge of root cell {CASCADE_CELL} leads red over blue by {border:.1}, short of \
         {ATLAS_BORDER_LEVELS} — so the picture has no tile grid on it and which slot holds \
         which map is as unreadable as it was without the viewer"
    );
    assert!(
        cell_tint.abs() < ATLAS_BORDER_LEVELS,
        "the middle of root cell {CASCADE_CELL} leads red over blue by {cell_tint:.1}, which is \
         the border's own colour axis — so the reading above cannot say which of the two it found"
    );
    assert_eq!(
        surround, 0.0,
        "the frame outside the atlas's rectangle reads {surround:.1}/255 rather than the \
         surround, so the viewer is not drawing where this check thinks it is"
    );
    assert!(
        cell - surround > ATLAS_TILE_OVER_SURROUND,
        "the cascade's cell reads {cell:.1}/255 and the letterbox beside it {surround:.1} — a \
         gap of {:.1}, short of {ATLAS_TILE_OVER_SURROUND}. The atlas's own edge is invisible \
         and nothing in the picture says how much of the frame it covers",
        cell - surround,
    );

    // And last, the picture itself, on the four goldens' terms.
    match check_golden(&image, "plaza-atlas", &paths) {
        Ok(line) => eprintln!("sundial golden: {line}"),
        Err(fault) => panic!("{fault}"),
    }
}

// ---------------------------------------------------------------------------
// The goldens
// ---------------------------------------------------------------------------

/// Holds one frame to its checked-in reference, and hands back what it found.
///
/// A `Result` rather than an assertion, and the bless run is why: every frame in
/// the suite is written in one pass, so a `CRCBL_BLESS=1` run that stopped at the
/// first reference would need one run per reference — and the ones it had not
/// reached yet would each be blessed against a different process's state.
fn check_golden(image: &Image, name: &str, label: &str) -> Result<String, String> {
    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"));
    match Golden::new(reference)
        .check(image)
        .expect("the reference is readable")
        .into_result()
    {
        Ok(comparison) => Ok(format!("{name} on {label} — {}", comparison.summary())),
        Err(message) => Err(format!("{name} on {label}: {message}")),
    }
}

/// **The four checked-in frames.**
///
/// Last, and only after the claims above: a golden says the frame did not change,
/// which is worth having and is not evidence that it was ever right. One plaza
/// per rung of the filter ladder at the fixture sun, and one more at the bottom
/// of the sun's arc, where a shadow is over five times its caster's height and a
/// bias artefact has the most room to show.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sundial-golden.sh"]
fn the_plaza_matches_its_goldens() {
    let mut faults = Vec::new();
    let mut arms = vec![(
        "plaza-grazing".to_string(),
        Arm::shipped().at_tick(sun::GRAZING_TICK),
    )];
    for name in filter::names(filter::FILTER) {
        arms.push((format!("plaza-{name}"), Arm::shipped().on(name)));
    }
    assert!(
        arms.len() > 2,
        "the engine declares {:?}, so there is no ladder to bless a frame per rung of",
        filter::names(filter::FILTER)
    );
    for (name, arm) in arms {
        let (image, paths, _) = draw(EXTENT, arm);
        match check_golden(&image, &name, &paths) {
            Ok(line) => eprintln!("sundial golden: {line}"),
            Err(fault) => faults.push(fault),
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}

// ---------------------------------------------------------------------------
// The frames a person looks at
// ---------------------------------------------------------------------------

/// **The plaza reads the same at presentation size**, and the frames are written
/// where a person can look at them.
///
/// Two jobs, and the first is what makes this a test rather than a screenshot
/// script: the contact claim above is a block a few pixels across, and a claim
/// that held because of where one triangle edge landed would hold at one extent
/// and nowhere else.
///
/// The pair worth opening is `plaza` against `no-shadows`: the plaza is one
/// albedo and one plane, so with the passes off the colonnade's feet, the
/// plinth's contact and the counters' three shadows all disappear, and the shadow
/// term is the whole of what makes the scene legible.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sundial-golden.sh"]
fn the_plaza_reads_the_same_at_presentation_size() {
    let extent = REVIEW_EXTENT;
    let camera = plaza::fixed_camera();
    let block = block_for(extent);

    let (shadowed, paths, _) = draw(extent, Arm::shipped());
    let (flat, _, _) = draw(extent, Arm::shipped().without_shadows());
    let (grazing, _, _) = draw(extent, Arm::shipped().at_tick(sun::GRAZING_TICK));
    let (seamed, _, _) = draw(
        extent,
        Arm::shipped().on("box").split_at(filter::SEAM_CENTRE),
    );
    save(&shadowed, "plaza", extent);
    save(&flat, "no-shadows", extent);
    save(&grazing, "grazing", extent);
    save(&seamed, "seam", extent);

    let contact = project(&camera, extent, plaza::PLINTH_CONTACT);
    let open = project(&camera, extent, plaza::OPEN_PAVEMENT);
    let (dark, plain) = (
        brightness(&shadowed, contact, block),
        brightness(&flat, contact, block),
    );
    let darkening = (plain - dark) / plain.max(1e-3);
    eprintln!(
        "sundial golden: the contact at {}x{} on {paths} — {plain:.2}/255 with no shadow term, \
         {dark:.2} with it, darkening {darkening:.4}",
        extent.0, extent.1
    );
    let moved = brightness(&shadowed, open, block) - brightness(&flat, open, block);
    assert!(
        moved.abs() < OPEN_PAVEMENT_TOLERANCE,
        "at {}x{} the open pavement moved by {moved:.2}/255 when the shadow passes were switched \
         off",
        extent.0,
        extent.1,
    );
    assert!(
        darkening > CONTACT_DARKENING,
        "at {}x{} the contact darkens by {darkening:.4}, short of the {CONTACT_DARKENING} the \
         same claim holds to at {}x{}. The claim is about the plaza, not about the sampling",
        extent.0,
        extent.1,
        CLAIM_EXTENT.0,
        CLAIM_EXTENT.1,
    );
}

// ---------------------------------------------------------------------------
// The acne
// ---------------------------------------------------------------------------

/// Where the block both acne readings are taken over sits on the pavement.
///
/// **Open pavement to the `+x` side of the plaza, and every caster in the scene
/// throws away from it at both of the ticks read below.** The sun swings from
/// `+22°` to `0°` between them, so its shadows run towards `-x` and `+z`: the
/// colonnade stands at [`plaza::COLONNADE_X`] and throws further into `-x`, the
/// plinth's shadow lands at `z` past its own near face, the parapet's reaches
/// `z = -6.3` at the bottom of the arc and the three counters' land between
/// `z = -1.9` and `z = 1.4`. This block is clear of all of them, at both ticks,
/// which is not taken on trust: [`LIT_PAVEMENT`] is read off each frame before
/// anything else here is believed.
const ACNE_CENTRE: Vec3 = Vec3::new(3.0, 0.0, -3.5);

/// Half the block's side, in metres of pavement, in `x` and in `z`.
///
/// The largest clear rectangle found on that pavement — 3 m by 2 m, which
/// [`acne_block`] inscribes 167 by 25 pixels of at [`CLAIM_EXTENT`] from
/// [`plaza::fixed_camera`]. Wide rather than square because the block lies most
/// of a plaza away and the plane is seen nearly edge-on, so a metre of `z` is a
/// handful of rows where a metre of `x` is dozens of columns. Growing it in `z`
/// walks into the parapet's shadow at the grazing tick; growing it in `x` walks
/// into the counters' at the steep one.
const ACNE_HALF: (f32, f32) = (1.5, 1.0);

/// How far below the median of its own 5x5 neighbourhood a pixel has to sit to
/// count as a self-shadowing dot, in luma out of 255.
///
/// **The statistic `crcbl_render::shadow`'s `DEPTH_BIAS_TEXELS` and
/// `NORMAL_OFFSET_TEXELS` were both swept with**, and the same threshold
/// `crates/crcbl/tests/mesh_e2e/shadow_tiles.rs` counts a punctual light's
/// speckle at — that file carries the sweep that set it. What it separates is a
/// *dot* from a *gradient*: a shading gradient's own pixels sit on their
/// neighbourhood's median, and a pixel whose shadow map quantised the receiver's
/// own depth away sits below it. A block average or a standard deviation cannot
/// tell the two apart, and this block is a gradient — it spans 3 m of pavement
/// under a sun 10° up, which is 25 luma of legitimate falloff across it.
const SPECKLE_LUMA: f32 = 4.0;

/// How dark the block's mean may be before it is not lit pavement, out of 255.
///
/// The anti-vacuity floor, and the reason the two readings below are readings of
/// acne rather than of a shadow that wandered over the block. Shadowed pavement
/// in this plaza is lit by `crate::sun`'s `AMBIENT` and nothing else, so a block
/// that had drifted into one would measure a smooth dark rectangle, count no
/// dots at either tick, and pass on both counts.
///
/// That is not a hypothetical. Moved onto the plinth's own shadow the block
/// reads `78.28` and `78.56` out of 255 and counts no dot at either tick — the
/// second of the runs the test below records.
///
/// **Swept:** the block reads 185.29 at the grazing tick and 252.46 at the steep
/// one on radv (AMD Radeon RX 7900 XTX, RADV NAVI31), and 185.15 and 252.46 on
/// lavapipe (llvmpipe). Set between those and the 78 the shadowed block reads,
/// and **under what the zero-bias frames in this constant's neighbours
/// measure** — 144.78 and 194.13 — so that a run with acne on it fails on the
/// acne and not here.
const LIT_PAVEMENT: f32 = 120.0;

/// What share of the block may be self-shadowing dots under the **steep** sun,
/// as a percentage.
///
/// The control on the reading itself: a statistic that counted this block's own
/// texture, its dither, or the driver's rounding would count it at every sun
/// angle. Under a sun 55° up the receiver's depth barely moves across a shadow
/// texel and there is nothing for a bias to fail to cover, so the honest answer
/// here is zero.
///
/// **Swept**, at [`CLAIM_EXTENT`] over [`ACNE_HALF`]'s block, against the same
/// frames drawn with both of `crcbl_render::shadow`'s sun bias constants set to
/// zero:
///
/// | reading | radv | radv, zero bias | lavapipe | lavapipe, zero bias |
/// | --- | --- | --- | --- | --- |
/// | dots at the grazing tick, 10.0° up | `0.0000%` | `42.2275%` | `0.0000%` | `42.1078%` |
/// | dots at the steep tick, 55.0° up | `0.0000%` | `23.0180%` | `0.0000%` | `23.0180%` |
/// | grazing over steep, in points | `0.0000` | `19.2096` | `0.0000` | `19.0898` |
/// | the block's mean at the grazing tick | `185.29` | `144.78` | `185.15` | `144.68` |
/// | the block's mean at the steep tick | `252.46` | `194.13` | `252.46` | `193.89` |
///
/// Both adapters count **no** dot at either tick with the shipped bias, so this
/// is floored at about half of what the zero-bias steep frame draws rather than
/// at a multiple of a healthy reading there is none of.
const STEEP_SPECKLE_PERCENT: f32 = 10.0;

/// How many points rougher than the steep frame the grazing frame's block may
/// be.
///
/// [`STEEP_SPECKLE_PERCENT`]'s own table is the sweep: the difference is
/// `0.0000` points on both adapters with the shipped bias and `19.2096` and
/// `19.0898` with it at zero, so this is floored at about the midpoint.
///
/// **This half and the one above catch different failures**, which is why both
/// are here. A bias too small for *any* incidence acnes both frames and the
/// ceiling above is what refuses it; a bias that covers a steep receiver and not
/// a grazing one — the shape acne actually takes, because what has to be covered
/// grows with how fast the receiver climbs across one shadow texel — leaves the
/// steep frame clean and only this difference sees it.
const GRAZING_OVER_STEEP: f32 = 9.0;

/// Where the acne block lands in the frame: a centre and half-extents in pixels,
/// in [`brightness`]' own shape.
///
/// The block's four corners are put through the very [`project`] the rest of
/// this file uses and the largest rectangle **inside** the trapezoid they make
/// is taken — inside rather than around it, because the plane is seen at an
/// angle and a bounding box of the four would reach past the pavement the
/// corners were chosen to keep clear.
fn acne_block(camera: &Camera, extent: (u32, u32)) -> ((u32, u32), (u32, u32)) {
    let corner = |dx: f32, dz: f32| {
        project(
            camera,
            extent,
            ACNE_CENTRE + Vec3::new(dx * ACNE_HALF.0, 0.0, dz * ACNE_HALF.1),
        )
    };
    let (near_left, far_left) = (corner(-1.0, 1.0), corner(-1.0, -1.0));
    let (near_right, far_right) = (corner(1.0, 1.0), corner(1.0, -1.0));
    // The far edge is the higher one in the frame, so the inscribed rectangle
    // starts below whichever of its two corners sits lower.
    let (left, right) = (near_left.0.max(far_left.0), near_right.0.min(far_right.0));
    let (top, bottom) = (far_left.1.max(far_right.1), near_left.1.min(near_right.1));
    assert!(
        left < right && top < bottom,
        "the acne block projects to nothing: {left}..{right} by {top}..{bottom}"
    );
    let half = ((right - left) / 2, (bottom - top) / 2);
    assert!(
        half.0 > 0 && half.1 > 0,
        "the acne block is under two pixels across at {extent:?}, so a 5x5 \
         neighbourhood is bigger than the thing it is a neighbourhood in"
    );
    ((left + half.0, top + half.1), half)
}

/// What share of the block sits more than [`SPECKLE_LUMA`] below the median of
/// its own 5x5 neighbourhood, as a percentage.
///
/// The neighbourhood is read from the whole frame rather than from the block, so
/// a pixel on the block's own edge is compared against its real neighbours; the
/// block decides which pixels are *counted*, not which are looked at.
fn speckle_percent(image: &Image, centre: (u32, u32), half: (u32, u32)) -> f32 {
    /// How far a neighbourhood reaches from the pixel it is about, in pixels.
    ///
    /// Two, which makes it the 5x5 that
    /// `crates/crcbl/tests/mesh_e2e/shadow_tiles.rs` swept [`SPECKLE_LUMA`] over.
    const REACH: i32 = 2;

    let luma = |p: [u8; 4]| (f32::from(p[0]) + f32::from(p[1]) + f32::from(p[2])) / 3.0;
    let (mut dots, mut count) = (0u32, 0u32);
    let mut neighbourhood = Vec::new();
    for y in centre.1 - half.1..=centre.1 + half.1 {
        for x in centre.0 - half.0..=centre.0 + half.0 {
            let value = luma(image.pixel(x, y).expect("the block is inside the frame"));
            neighbourhood.clear();
            for dy in -REACH..=REACH {
                for dx in -REACH..=REACH {
                    let (nx, ny) = (i64::from(x) + i64::from(dx), i64::from(y) + i64::from(dy));
                    if let Some(pixel) = u32::try_from(nx)
                        .ok()
                        .zip(u32::try_from(ny).ok())
                        .and_then(|(nx, ny)| image.pixel(nx, ny))
                    {
                        neighbourhood.push(luma(pixel));
                    }
                }
            }
            neighbourhood.sort_by(f32::total_cmp);
            if neighbourhood[neighbourhood.len() / 2] - value > SPECKLE_LUMA {
                dots += 1;
            }
            count += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    {
        100.0 * dots as f32 / count as f32
    }
}

/// **The pavement under a grazing sun is as smooth as the same pavement under a
/// steep one.**
///
/// `docs/plan/sample/18-sundial.md`'s charter pairs acne with peter-panning, and
/// the contact reading above is only the second of the two. Acne is what a bias
/// too *small* draws: the shadow map quantises a receiver's own depth away
/// across one texel, the receiver compares against a copy of itself and loses,
/// and a large flat surface fills with dots on the period of the shadow texel.
/// It is worst where the receiver climbs fastest across one of those texels,
/// which is where the sun grazes — [`sun::GRAZING_TICK`] is [`sun::MIN_ELEVATION`]
/// exactly, the most grazing sun this clock reaches.
///
/// A golden would catch the dots arriving and could not say what they were, and
/// no average over the block can either: the block is 3 m of pavement seen
/// nearly edge-on, so it carries 25 luma of legitimate falloff across it and its
/// mean and its variance are both mostly that. [`SPECKLE_LUMA`] is the statistic
/// that separates a dot from a gradient, and it is the same one both of
/// `crcbl_render::shadow`'s sun bias constants were swept with.
///
/// **The block is [`ACNE_CENTRE`]'s**, open pavement on the `+x` side that every
/// caster in the plaza throws away from at both ticks — and the block is read
/// **at both ticks**, so the sun's own elevation is the only thing that differs
/// between the two counts.
///
/// # Anti-vacuity
///
/// Three ways this could pass while measuring nothing, and one assertion each.
/// The block could be **in shadow**, where a smooth dark rectangle counts no
/// dots at either tick — [`LIT_PAVEMENT`] is read off both frames. The two ticks
/// could be the **same sun**, where any two readings agree — the elevations are
/// held apart, off [`sun::Sky::at`] rather than off the tick numbers. And the
/// two frames could be **one frame**, where the difference is zero by
/// construction — they are compared as bytes.
///
/// # How it was shown to fail
///
/// Three runs.
///
/// * **Both of `crcbl_render::shadow`'s sun bias constants set to zero**, which
///   is the artefact this exists for. The grazing block went to `42.2275%` dots
///   against a steep `23.0180%` on radv, and the steep half failed first:
///   *"23.0180% of the block is a self-shadowing dot with the sun 55.0° up, past
///   the 10% this reading has any business finding at all."* With
///   [`STEEP_SPECKLE_PERCENT`] lifted out of the way to reach the other half,
///   *"the block is 42.2275% dots with the sun 10.0° up and 23.0180% with it
///   55.0° up — 19.2096 points over, past 9."* The block's mean fell to `144.78`
///   and `194.13` and stayed clear of [`LIT_PAVEMENT`], so it was the acne that
///   fired and not the floor.
/// * **[`ACNE_CENTRE`] moved to `(-0.2, 0, 3.0)` and [`ACNE_HALF`] to
///   `(0.5, 0.2)`**, which is pavement inside the plinth's own shadow at both
///   ticks and is what this reading would look like if it had drifted off the
///   open plaza. Both counts came back `0.0000%` — the claim satisfied — and the
///   block read `78.28` and `78.56` out of 255, which [`LIT_PAVEMENT`] refused.
/// * **Both ticks set to [`sun::GRAZING_TICK`]**: the difference is `0.0000`
///   points whatever the frames contain, and the elevation assertion failed
///   first — *"the two ticks put the sun 10.0° and 10.0° up."*
///
/// And one constant at a time, on radv, which says what this reading is a claim
/// *about*. `NORMAL_OFFSET_TEXELS` alone at zero: `41.5329%` grazing against
/// `0.0000%` steep, the second assertion — the shape acne takes, a bias that
/// covers the steep receiver and not the grazing one. `DEPTH_BIAS_TEXELS` alone
/// at zero: `3.2575%` against `2.9222%`, `0.3353` points over, and the claim
/// **holds** — over this block the normal offset covers a lost depth bias on
/// its own, so a depth bias regression is not something this reading sees.
/// `crates/crcbl/tests/mesh_e2e/shadow_tiles.rs`'s sweep is where that constant
/// is held.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sundial-golden.sh"]
fn the_grazing_sun_leaves_the_open_pavement_as_smooth_as_the_steep_one_does() {
    let extent = CLAIM_EXTENT;
    let camera = plaza::fixed_camera();
    let (centre, half) = acne_block(&camera, extent);

    let (grazing_sky, steep_sky) = (
        sun::Sky::at(sun::GRAZING_TICK),
        sun::Sky::at(sun::NOON_TICK),
    );
    assert!(
        steep_sky.elevation > grazing_sky.elevation * 2.0,
        "the two ticks put the sun {:.1}° and {:.1}° up. A comparison of a grazing sun with a \
         steep one needs the two to be different suns",
        grazing_sky.elevation.to_degrees(),
        steep_sky.elevation.to_degrees(),
    );

    let (grazing, paths, _) = draw(extent, Arm::shipped().at_tick(sun::GRAZING_TICK));
    let (steep, _, _) = draw(extent, Arm::shipped().at_tick(sun::NOON_TICK));
    assert!(
        grazing.pixels() != steep.pixels(),
        "the two ticks drew one frame, so every reading below is the same reading twice"
    );

    let readings = [
        ("grazing", &grazing, grazing_sky),
        ("steep", &steep, steep_sky),
    ];
    let mut dots = [0.0f32; 2];
    for (index, (name, image, sky)) in readings.into_iter().enumerate() {
        let mean = brightness(image, centre, half);
        dots[index] = speckle_percent(image, centre, half);
        eprintln!(
            "sundial golden: the {name} pavement on {paths} — block at {centre:?} +-{half:?}, \
             {} — mean {mean:.2}/255, {:.4}% of it more than {SPECKLE_LUMA} luma under its own \
             neighbourhood",
            sky.row(),
            dots[index],
        );
        assert!(
            mean > LIT_PAVEMENT,
            "the block reads {mean:.2}/255 at the {name} tick, under the {LIT_PAVEMENT} lit \
             pavement stands at. This is a reading of a shadow, and a shadow counts no dots \
             however the bias is set"
        );
    }

    let [grazing_dots, steep_dots] = dots;
    let over = grazing_dots - steep_dots;
    eprintln!(
        "sundial golden: the acne pair on {paths} — {grazing_dots:.4}% grazing against \
         {steep_dots:.4}% steep, {over:.4} points over"
    );
    assert!(
        steep_dots < STEEP_SPECKLE_PERCENT,
        "{steep_dots:.4}% of the block is a self-shadowing dot with the sun {:.1}° up, past the \
         {STEEP_SPECKLE_PERCENT}% this reading has any business finding at all. The sun's shadow \
         bias is too small for every incidence, not merely for a grazing one",
        steep_sky.elevation.to_degrees(),
    );
    assert!(
        over < GRAZING_OVER_STEEP,
        "the block is {grazing_dots:.4}% dots with the sun {:.1}° up and {steep_dots:.4}% with it \
         {:.1}° up — {over:.4} points over, past {GRAZING_OVER_STEEP}. That is acne: the bias \
         covers a steep receiver and not a grazing one",
        grazing_sky.elevation.to_degrees(),
        steep_sky.elevation.to_degrees(),
    );
}
