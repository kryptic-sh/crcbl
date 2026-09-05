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

/// Which of the plaza's poses an arm is drawn from.
///
/// An enum and not a pair of flags: a pose is one choice, and two booleans would
/// let an arm ask for two cameras at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pose {
    /// [`plaza::fixed_camera`] — the fixture pose every golden is blessed from
    /// and every constant in this file was swept on.
    Fixed,
    /// [`plaza::counter_camera`] — where the penumbra ladder is read, and the
    /// only pose PCSS's estimate is unclamped at.
    Counters,
    /// [`plaza::pavement_camera`] — the second pose the two pavement claims are
    /// read from, the contact pair and the cascade walk alike.
    Pavement,
}

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
    /// Which of the plaza's three poses the frame is taken from.
    pose: Pose,
    /// Whether the shadow atlas is drawn over the picture rather than the
    /// picture itself — [`crcbl::render::DebugView::ShadowAtlas`].
    atlas: bool,
    /// Whether the picture is tinted by the cascade each fragment's sun shadow
    /// came from — [`crcbl::render::DebugView::Cascades`].
    cascades: bool,
    /// The sun's constant shadow bias in thousandths of a cascade texel, or
    /// `None` for what the engine ships.
    ///
    /// Thousandths for [`Arm::split_permille`]' reason: an arm is printed into
    /// every reading this suite reports and compared against its neighbours, and
    /// both want `Eq`.
    bias_millitexels: Option<u32>,
    /// The sun's normal offset, in the same thousandths.
    offset_millitexels: Option<u32>,
}

/// A count of cascade texels as the thousandths an [`Arm`] keeps.
fn millitexels(count: f32) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "every caller passes a count inside the variable's own range"
    )]
    {
        (count * 1000.0).round() as u32
    }
}

/// A count of thousandths of a texel, back as the texels the console holds.
fn texels(millitexels: u32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a bias count is a few thousand thousandths"
    )]
    {
        millitexels as f32 / 1000.0
    }
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
            pose: Pose::Fixed,
            atlas: false,
            cascades: false,
            bias_millitexels: None,
            offset_millitexels: None,
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
            pose: Pose::Counters,
            ..self
        }
    }

    /// The same arm framed by [`plaza::pavement_camera`].
    const fn framed_on_the_pavement(self) -> Self {
        Self {
            pose: Pose::Pavement,
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

    /// The same arm with the picture tinted by cascade — what `C`, the pause
    /// panel's `CASCADES` row and the page's own button put up.
    const fn showing_the_cascades(self) -> Self {
        Self {
            cascades: true,
            ..self
        }
    }

    /// The same arm with the sun's constant bias at `count` cascade texels.
    fn biased(self, count: f32) -> Self {
        Self {
            bias_millitexels: Some(millitexels(count)),
            ..self
        }
    }

    /// The same arm with the sun's normal offset at `count` of the same texels.
    fn offset(self, count: f32) -> Self {
        Self {
            offset_millitexels: Some(millitexels(count)),
            ..self
        }
    }

    /// The two bias counts this arm asks for, each back as the texels the
    /// console holds.
    fn biases(self) -> [Option<f32>; 2] {
        [
            self.bias_millitexels.map(texels),
            self.offset_millitexels.map(texels),
        ]
    }

    /// Which camera this arm is drawn from.
    fn camera(self) -> Camera {
        match self.pose {
            Pose::Fixed => plaza::fixed_camera(),
            Pose::Counters => plaza::counter_camera(),
            Pose::Pavement => plaza::pavement_camera(),
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
    for (name, count) in [filter::BIAS, filter::OFFSET].into_iter().zip(arm.biases()) {
        if let Some(count) = count {
            filter::var(name)
                .set(&Value::Float(count))
                .expect("the bias count is inside its own range");
        }
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
    // The cascade overlay, which *is* a lane of the frame block — `mesh.slang`
    // multiplies the shaded picture by `cascade_tint` — so an arm that does not
    // ask for it draws the frame it always drew, byte for byte.
    renderer.set_cascade_view(arm.cascades);
    Ok(renderer)
}

// ---------------------------------------------------------------------------
// Reading the frame
// ---------------------------------------------------------------------------

/// Where a world point lands in the frame, in pixels, or `None` when it is
/// behind the camera or off the frame.
///
/// Through the very same [`Camera::view_projection`] the frame was drawn with,
/// so a claim about a surface is a claim about the pixels that surface actually
/// covers.
fn on_screen(camera: &Camera, extent: (u32, u32), point: Vec3) -> Option<(u32, u32)> {
    #[allow(clippy::cast_precision_loss)]
    let aspect = extent.0 as f32 / extent.1 as f32;
    let clip = camera.view_projection(aspect) * point.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    #[allow(clippy::cast_precision_loss)]
    let (width, height) = (extent.0 as f32, extent.1 as f32);
    let x = (ndc.x + 1.0) * 0.5 * width;
    let y = (1.0 - ndc.y) * 0.5 * height;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (x >= 0.0 && x < width && y >= 0.0 && y < height).then_some((x as u32, y as u32))
}

/// Where a world point lands in the frame, for a reading that has no answer if
/// it lands nowhere.
fn project(camera: &Camera, extent: (u32, u32), point: Vec3) -> (u32, u32) {
    on_screen(camera, extent, point).unwrap_or_else(|| {
        panic!(
            "{point:?} is behind a {}x{} frame or outside it, so the claim about it would be \
             about a pixel that is not there",
            extent.0, extent.1,
        )
    })
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
/// Three frames per rung — the console's filter everywhere, the shipped one
/// everywhere, and the seamed one — and then every column of the seamed frame is
/// held against **both**. Outside [`SEAM_BLEED`] the agreement is exact, byte for
/// byte, and the disagreement with the other reference is what stops the whole
/// thing being vacuous: two identical references would satisfy the equality half
/// perfectly.
///
/// **Every rung the engine declares, and not the first one that is not the
/// shipped rung**, which is `docs/plan/sample/18-sundial.md`'s milestone 4: the
/// filter *ladder* side by side. One pair held would say the selector routes
/// a filter to a side; the ladder held says it routes every filter there, and a
/// rung wired to its neighbour's branch is exactly the failure a single pair
/// cannot see.
///
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
/// 961 of the 1024 columns are compared per rung — the rest are the bleed band —
/// and every one of them is exact on both adapters. What the rung and the shipped
/// filter stand apart down the two halves, in luma out of 255:
///
/// | rung | left, radv | right, radv | left, lavapipe | right, lavapipe |
/// | --- | --- | --- | --- | --- |
/// | `disc` | `3.110` | `324.498` | `3.101` | `324.678` |
/// | `box` | `26.417` | `363.250` | `26.267` | `363.425` |
///
/// so the equality is not an equality of two identical pictures. The left half is
/// the thinner of the two because it is mostly pavement with no shadow edge
/// crossing it, which is why this is asserted per half rather than per column.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sundial-golden.sh"]
fn the_seam_runs_the_console_filter_on_the_left_and_the_shipped_one_on_the_right() {
    let extent = CLAIM_EXTENT;
    let shipped = crcbl::render::shadow::shipped_filter().label();
    let ladder: Vec<&str> = filter::names(filter::FILTER)
        .iter()
        .copied()
        .filter(|name| *name != shipped)
        .collect();
    assert!(
        ladder.len() > 1,
        "the engine declares {:?}, so there is no ladder to put beside the shipped rung — one \
         rung compared would be a comparison and not a ladder",
        filter::names(filter::FILTER)
    );

    let pose = Arm::shipped()
        .framed_on_the_counters()
        .at_tick(sun::NOON_TICK);
    let (whole_shipped, paths, _) = draw(extent, pose.on(shipped));
    let seam = extent.0 / 2;

    for moved in ladder {
        let (whole_moved, _, _) = draw(extent, pose.on(moved));
        let (seamed, _, _) = draw(extent, pose.on(moved).split_at(filter::SEAM_CENTRE));

        let mut columns = 0u32;
        // What the two filters do to each half on their own, which is what says
        // the exactness below separates anything. Per **half** and not per
        // column: most of this frame is pavement no shadow edge crosses, and the
        // two filters agree to the byte there — a demand that every single column
        // differ would be a demand that the whole frame be a penumbra.
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
                 {agreement:.3}/255. With the seam at {} the {} of the frame is meant to be \
                 that filter and nothing else",
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
            "sundial golden: the {moved} seam on {paths} — {columns} columns exact, {moved} and \
             {shipped} {:.3} and {:.3}/255 apart down the two halves",
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

/// How far red leads blue over a block around `centre`, in 0-255 codes.
///
/// A difference of channels rather than of brightness, which is what survives
/// the tonemap and a surface that is not white — and what both readings taken on
/// this axis are about: `crcbl::shaders::atlas_view::BORDER_TINT` is amber where
/// everything else the atlas viewer draws is a grey, and
/// `crcbl::shaders::mesh::CASCADE_TINTS` is red-dominant for the near cascade
/// and blue-dominant for the far one.
fn tint_over(image: &Image, centre: (u32, u32), half: (u32, u32)) -> f32 {
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
            total += f32::from(pixel[0]) - f32::from(pixel[2]);
            count += 1;
        }
    }
    assert!(count > 0, "an empty block at {centre:?} measures nothing");
    #[allow(clippy::cast_precision_loss)]
    {
        total / count as f32
    }
}

/// How far red leads blue at `at`, in 0-255 codes.
fn tint_at(image: &Image, at: (u32, u32)) -> f32 {
    tint_over(image, at, (0, 0))
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
// The cascade overlay
// ---------------------------------------------------------------------------

/// Where the overlay's near reading is taken, on the pavement.
///
/// **Open pavement in front of the plinth and to its `+x` side.** Three things
/// are wanted of it and the test reads all three off the plaza rather than
/// trusting this paragraph: it is inside cascade 0 and clear of the cross-fade
/// band, where the tint is one cascade's own rather than a mixture; the fixed
/// camera can see it, on [`plaza::hidden_from`]'s terms; and no lamp reaches it,
/// so the surface under the tint is lit by the sun alone.
///
/// [`plaza::OPEN_PAVEMENT`] is the other end of the reading and is the plaza's
/// own control point — in full sun at every tick and outside every lamp — which
/// happens to stand well past the split.
const CASCADE_NEAR_READING: Vec3 = Vec3::new(1.2, 0.0, 3.6);

/// How far apart the two readings have to stand on the red-over-blue axis, in
/// 0-255 codes.
///
/// **Swept, not guessed**, at [`EXTENT`] over [`block_for`]'s block, on both
/// Vulkan adapters this workspace runs locally, with the overlay on and off:
///
/// | reading | radv | radv, overlay off | lavapipe | lavapipe, overlay off |
/// | --- | --- | --- | --- | --- |
/// | the cascade 0 reading | `+79.9` | `+6.9` | `+79.6` | `+7.0` |
/// | the reading past the split | `-74.1` | `+7.7` | `-74.2` | `+7.7` |
/// | the gap between the two | `154.0` | `0.8` | `153.8` | `0.7` |
///
/// The two adapters agree to a tenth of a code on every one of them. Floored at
/// about half the gap the overlay opens, which is still two orders above what
/// the plaza's own colour puts between the two places —
/// [`CASCADE_PLAZA_FLATNESS`] is that.
const CASCADE_OVERLAY_LEVELS: f32 = 64.0;

/// How flat the same two readings have to be with the overlay **off**, in the
/// same codes.
///
/// The anti-vacuity constant, and a different measurement rather than the same
/// one loosened: what it bounds is the plaza's *own* spread across the two
/// places, which is what a test measuring the scene instead of the overlay would
/// be reporting. The figures are on [`CASCADE_OVERLAY_LEVELS`].
const CASCADE_PLAZA_FLATNESS: f32 = 4.0;

/// **The cascade overlay tints the plaza by the cascade each fragment's sun
/// shadow came from**, and it reaches this fixture's own frame.
///
/// `docs/plan/sample/18-sundial.md`'s milestone 1 diagnostic, the other half of
/// the pair the atlas viewer is one of. `crates/crcbl/tests/forward_e2e/
/// shadow.rs` holds the overlay to the *band* on the engine's own pavement —
/// three readings across one cross-fade, and the middle one strictly between its
/// neighbours. What is added here is that the picture reaches **this** frame —
/// the plaza, at the pose the goldens are blessed from, through the sample's own
/// effect stack — and a golden of it, so the picture `C`, the pause panel's
/// `CASCADES` row and the page's own button all put up is one that cannot change
/// unnoticed.
///
/// Two readings, placed from [`crcbl::render::Cascades`]' own split rather than
/// written down: one inside cascade 0 and clear of the band, one past the split.
/// `crcbl::shaders::mesh::CASCADE_TINTS` is red-dominant for the near cascade
/// and blue-dominant for the far one, so the two have to come out on opposite
/// sides of the red-over-blue axis.
///
/// **Anti-vacuity.** The two tints have to differ, or no arrangement of readings
/// could tell the cascades apart — they are compared. The same two places with
/// the overlay *off* must not order themselves the same way, or this is a
/// picture of the plaza's own colour and would hold with the overlay deleted —
/// [`CASCADE_PLAZA_FLATNESS`] is that half. And both places have to be lit, or a
/// tint multiplied into a dark pixel is a reading of the dark pixel —
/// [`LIT_PAVEMENT`] is read off the frame with the overlay off.
///
/// # How it was shown to fail
///
/// Three runs, one per thing this check says, all on radv.
///
/// * **The overlay never ran**, by drawing the tinted arm as
///   [`Arm::shipped`]: *"the cascade 0 reading (6.9) is not 64 codes redder
///   than the one past the split (7.7), so the overlay is not reaching this
///   frame."* Those two numbers are the plaza's own colour at the two places,
///   which is what the second assertion is a bound on.
/// * **The flatness half read the tinted frame** rather than the plain one,
///   which is what a control taken off the wrong picture would look like:
///   *"the plaza already separates these two places by 154.0 codes with the
///   overlay off — 79.9 against -74.1."*
/// * **The golden compared against another tick of the sun**, which moves the
///   cascade the far reading falls in and nothing else here: both readings
///   still passed and the comparison failed at `51.6256%` grossly wrong against
///   a `0.1%` budget. That is the half that says the picture is held to *this*
///   frame rather than to any frame with two tints in it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sundial-golden.sh"]
fn the_cascade_overlay_tints_the_plaza_by_the_cascade_its_shadow_came_from() {
    let extent = EXTENT;
    let camera = plaza::fixed_camera();
    let sky = sun::Sky::at(sun::FIXTURE_TICK);
    let reach = plaza::cascade_split(&camera, sky);
    let band = reach * crcbl::shaders::mesh::CASCADE_FADE_FRACTION;
    let tints = crcbl::shaders::mesh::CASCADE_TINTS;
    assert_ne!(
        tints[0], tints[1],
        "the two cascades wear one colour, so no arrangement of the readings below could tell \
         them apart"
    );

    let places = [
        ("cascade 0", CASCADE_NEAR_READING),
        ("past the split", plaza::OPEN_PAVEMENT),
    ];
    for (name, at) in places {
        let apart = at.distance(camera.eye);
        assert!(
            if name == "cascade 0" {
                apart < reach - band
            } else {
                apart > reach
            },
            "the {name} reading stands {apart:.3} m from the eye, and cascade 0 reaches \
             {reach:.3} m and fades out from {:.3} — so it is not where this reading says it is",
            reach - band,
        );
        assert!(
            !plaza::hidden_from(camera.eye, at),
            "the {name} reading at {at:?} is behind something the plaza stands in front of it, so \
             the pixel it names is not the pavement"
        );
        assert!(
            !plaza::lamplit(at),
            "a lamp reaches the {name} reading at {at:?}, so the colour there is the lamp's as \
             well as the tint's"
        );
    }

    let (tinted, paths, _) = draw(extent, Arm::shipped().showing_the_cascades());
    let (plain, _, _) = draw(extent, Arm::shipped());
    let block = block_for(extent);
    let mut readings = Vec::new();
    for (name, at) in places {
        let pixel = project(&camera, extent, at);
        let (on, off) = (
            tint_over(&tinted, pixel, block),
            tint_over(&plain, pixel, block),
        );
        let lit = brightness(&plain, pixel, block);
        eprintln!(
            "sundial golden: the cascade overlay on {paths} — the {name} reading at {at:?} is \
             pixel {pixel:?}, {apart:.3} m from the eye: red over blue {on:.1} with the overlay \
             on and {off:.1} with it off, and the surface under it reads {lit:.2}/255",
            apart = at.distance(camera.eye),
        );
        assert!(
            lit > LIT_PAVEMENT,
            "the {name} reading reads {lit:.2}/255 with no overlay on it, under the \
             {LIT_PAVEMENT} lit pavement stands at — a tint multiplied into a dark pixel is a \
             reading of the dark pixel"
        );
        readings.push((name, on, off));
    }

    let [(_, near_on, near_off), (_, far_on, far_off)] =
        <[(&str, f32, f32); 2]>::try_from(readings.as_slice()).expect("two readings");
    assert!(
        near_on - far_on > CASCADE_OVERLAY_LEVELS,
        "the cascade 0 reading ({near_on:.1}) is not {CASCADE_OVERLAY_LEVELS} codes redder than \
         the one past the split ({far_on:.1}), so the overlay is not reaching this frame — or \
         both places wear one cascade's colour"
    );
    assert!(
        (near_off - far_off).abs() < CASCADE_PLAZA_FLATNESS,
        "the plaza already separates these two places by {:.1} codes with the overlay off — \
         {near_off:.1} against {far_off:.1} — so the ordering above is a picture of the scene's \
         own colour and would hold with the overlay deleted",
        (near_off - far_off).abs(),
    );

    // And last, the picture itself, on the other goldens' terms.
    match check_golden(&tinted, "plaza-cascades", &paths) {
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

// ---------------------------------------------------------------------------
// The bias pair
// ---------------------------------------------------------------------------

/// How far out from [`plaza::PLINTH_CONTACT`], along the plinth's own shadow,
/// the "and the shadow is still there" readings are taken, in metres of
/// pavement.
///
/// The plinth's shadow runs `+z` at [`sun::GRAZING_TICK`] and
/// [`plaza::fixed_camera`] sees a metre of it past the contact before the
/// pavement leaves the bottom of the frame — `project` refuses anything further
/// out, which is what set the last entry. [`plaza::pavement_camera`] frames all
/// five as well, and `plaza`'s own
/// `the_pavement_pose_frames_the_shadows_the_fixed_one_stands_in_front_of` is
/// what holds them there with no GPU. Five stations rather than one because
/// what the deepest of them says is "somewhere out here is still in shadow", and
/// a single station could be the one place a shadow happened to have left.
const BEYOND_CONTACT: [f32; 5] = [0.2, 0.4, 0.6, 0.8, 1.0];

/// What `r_shadow_bias` is pushed to for the peter-panning half, in cascade
/// texels.
///
/// **The count that lifts the plinth's shadow off its own contact while leaving
/// the shadow beyond it**, which is what peter-panning *is*: a lit gap between a
/// caster and the shadow it throws, rather than a shadow that has gone. The band
/// that does both is narrow and this is the middle of it.
///
/// **Swept** at [`CLAIM_EXTENT`] from [`plaza::fixed_camera`] at
/// [`sun::GRAZING_TICK`], with `r_shadow_normal_offset` left where it ships. The
/// shadow term is the frame with the shadow passes off less the frame with them
/// on, over a 5x5 block; `contact` is that term at [`plaza::PLINTH_CONTACT`] and
/// `beyond` the deepest of it over [`BEYOND_CONTACT`]. The shipped rung, per
/// count:
///
/// | `r_shadow_bias` | contact, radv | beyond, radv | contact, lavapipe | beyond, lavapipe |
/// | --- | --- | --- | --- | --- |
/// | 1.5 (ships) | `70.73` | `68.33` | `70.44` | `68.33` |
/// | 80 | `70.73` | `68.33` | `70.44` | `68.33` |
/// | 88 | `8.11` | `68.33` | `8.05` | `68.33` |
/// | 96 (this) | `6.37` | `67.01` | `6.36` | `67.00` |
/// | 104 | `6.09` | `18.73` | `5.97` | `18.87` |
/// | 112 | `6.01` | `0.00` | `5.87` | `0.00` |
///
/// Under 88 the contact keeps its shadow outright; past 104 the shadow has left
/// the whole visible strip and the reading would be a claim about a shadow that
/// is not there. **Eighty-eight is a large number and the plinth is why**: the
/// depth pass keeps front faces, so the surface stored in the map along the ray
/// from this contact to the sun is the plinth's *far* face — the whole 1.2 m
/// depth of the block stands between the contact and the depth it is compared
/// against, and a bias has to cross all of it. A thin caster loses its contact
/// at a small count, which is `apps/lantern`'s wall and
/// `docs/plan/45-shadows.md`'s seventh decision's own fixture.
///
/// **This count on the other arms**, as the pair of terms each shows at it:
///
/// | arm | contact, radv | beyond, radv | contact, lavapipe | beyond, lavapipe |
/// | --- | --- | --- | --- | --- |
/// | the `disc` rung | `0.45` | `67.01` | `0.37` | `67.00` |
/// | the `box` rung | `0.00` | `68.08` | `0.00` | `68.08` |
/// | [`plaza::pavement_camera`] | `6.48` | `65.80` | `6.41` | `65.80` |
/// | the top of the arc | `0.00` | `0.00` | `0.00` | `0.00` |
///
/// The first three lift the contact and leave the pavement past it, which is the
/// gap the claim below is about; `disc`'s own window runs from 92 to 100 texels
/// on both adapters, so this count sits inside it as it sits inside the shipped
/// rung's. The third of them is the same rung and the same count read from
/// another pose, and it lands on the same side of both bounds (2026-09-05) —
/// which is what says the count is about the shadow map and not about the pixels
/// one camera resolves. The last does not — and **no** count does. The same
/// sweep at [`sun::NOON_TICK`] takes the contact and the pavement past it away
/// together:
///
/// | `r_shadow_bias` | contact, radv | beyond, radv | contact, lavapipe | beyond, lavapipe |
/// | --- | --- | --- | --- | --- |
/// | 50 | `176.00` | `173.33` | `175.93` | `173.33` |
/// | 52 | `91.65` | `96.31` | `91.97` | `96.67` |
/// | 54 | `0.21` | `0.39` | `0.23` | `0.37` |
/// | 56 | `0.00` | `0.00` | `0.00` | `0.00` |
///
/// **A sun at the top of its arc is why**, and it is the paragraph above turned
/// around: the ray from this contact to a sun that steep leaves the plinth
/// through the block's *top* face rather than through its far side, so what a
/// bias has to cross is the block's height, and that is the same depth for
/// every station in a shadow this short — the whole shadow is a fraction of
/// that height long. Contact and pavement lift as one,
/// there is no gap between them to read, and so there is no count to read it
/// at. `docs/backlog.md` carries what a peter-panning reading at the top of the
/// arc would want.
const PETER_PAN_BIAS: f32 = 96.0;

/// What `r_shadow_normal_offset` is pushed to for the other half, in the same
/// texels.
///
/// **Twenty times what ships, and the contact does not move at all** — which is
/// `docs/plan/45-shadows.md`'s seventh decision measured on a fixture rather
/// than argued: a move along the receiver's own normal leaves the depth it
/// compares alone, so it cannot lift a shadow off its caster the way the count
/// above does. What it costs instead is the shadow's *far* end, and this is the
/// last station before it costs both.
///
/// **Swept** with `r_shadow_bias` left where it ships, read exactly as
/// [`PETER_PAN_BIAS`]' table is:
///
/// | `r_shadow_normal_offset` | contact, radv | beyond, radv | contact, lavapipe | beyond, lavapipe |
/// | --- | --- | --- | --- | --- |
/// | 2 (ships) | `70.73` | `68.33` | `70.44` | `68.33` |
/// | 24 | `70.73` | `68.33` | `70.44` | `68.33` |
/// | 36 | `70.73` | `68.33` | `70.44` | `68.33` |
/// | 40 (this) | `70.73` | `66.59` | `70.44` | `66.53` |
/// | 44 | `31.05` | `0.92` | `30.89` | `1.01` |
///
/// At 44 the contact and the pavement beyond it go together, which is a shadow
/// that has gone rather than one that has come off its caster — so the claim
/// below is made at 40, where the contact reads what it reads with the offset at
/// two and the frame is nonetheless a different picture.
///
/// **This count on the other arms.** The `disc` rung reads what the shipped rung
/// reads to a hundredth: contact `70.73` and beyond `66.59` on radv, `70.44` and
/// `66.53` on lavapipe. From [`plaza::pavement_camera`] the shipped rung holds
/// its own contact exactly — `69.84` on radv and `69.59` on lavapipe, the same
/// digits its shipped arm reads there — while the pavement past it falls from
/// `68.08` to `65.75` and from `68.00` to `65.60` (2026-09-05), so both halves
/// of the pair hold at that pose as they do at the fixture one.
///
/// `box` reads the shipped arm's own numbers exactly —
/// `70.73`/`68.33` and `70.44`/`68.33` — which is a count that reached that
/// rung's frame nowhere these two readings can see, so that rung is pushed to
/// [`HELD_OFFSET_COVERED_RUNG`] instead. At the top of the arc the contact holds
/// at `176.00` and `175.93` while beyond falls to `24.27` and `24.33`, so this
/// half of the claim is one that arm could make; it is [`PETER_PAN_BIAS`]' half
/// it cannot.
const HELD_OFFSET: f32 = 40.0;

/// What it is pushed to on [`OFFSET_COVERED_RUNG`] instead, in the same texels.
///
/// **A texel and a half further out, and the narrowest kernel on the ladder is
/// why** — the same property [`OFFSET_COVERED_RUNG`] is about, seen from the
/// other end. `r_shadow_normal_offset` walks the lookup sideways and then the
/// kernel takes its taps around wherever the walk ended, so the two reaches add:
/// `tile_pcf` has left the far end of this strip at [`HELD_OFFSET`] and
/// `tile_box_pcf`, which reaches one texel, has not. Pushed there its arm draws
/// that rung's own shipped readings, contact *and* far end — what a knob wired
/// to nothing draws, and what the anti-vacuity clause refuses.
///
/// **Swept** on that rung at [`sun::GRAZING_TICK`] from [`plaza::fixed_camera`]
/// at [`CLAIM_EXTENT`], read exactly as [`HELD_OFFSET`]'s table is — three runs
/// per adapter and the same digits every time (2026-09-05):
///
/// | `r_shadow_normal_offset` | contact, radv | beyond, radv | contact, lavapipe | beyond, lavapipe |
/// | --- | --- | --- | --- | --- |
/// | 2 (ships) | `70.73` | `68.33` | `70.44` | `68.33` |
/// | 40 | `70.73` | `68.33` | `70.44` | `68.33` |
/// | 40.5 | `70.73` | `65.91` | `70.44` | `65.87` |
/// | 41 | `70.73` | `52.25` | `70.44` | `52.21` |
/// | 41.5 (this) | `70.73` | `39.49` | `70.44` | `39.49` |
/// | 42 | `70.73` | `28.35` | `70.44` | `28.40` |
/// | 42.5 | `69.48` | `18.44` | `69.19` | `18.48` |
/// | 43 | `56.77` | `9.37` | `56.48` | `9.47` |
/// | 43.5 | `43.09` | `1.57` | `42.88` | `1.59` |
/// | 44 | `31.29` | `0.00` | `31.13` | `0.00` |
///
/// **The middle of the window rather than either edge of it.** At 40 the far end
/// has not moved at all, so both of the arm's readings are that rung's shipped
/// readings and the anti-vacuity clause refuses it; at 40.5 it has moved by
/// about two of the 255, which is inside what a third driver could flatten. At
/// 42.5 the contact has started to go and by 43 it is going fast, which is the
/// claim the arm was meant to make running backwards. So the window is 41 to 42
/// — the contact identical to its own shipped arm to the hundredth on both
/// adapters while the pavement past it has lost most of its shadow — and this is
/// the station in the middle of it, with a working station on either side.
const HELD_OFFSET_COVERED_RUNG: f32 = 41.5;

/// How much shadow term a piece of pavement has to carry to count as shadowed,
/// in luma out of 255.
///
/// **Swept:** every reading either constant's table calls shadowed is between
/// `66.53` and `70.73`, and every one it calls lit is under `8.11`. Set between
/// them, nearer the shadowed end, so the two are separated by a wide gap rather
/// than by a threshold sitting on either.
const CONTACT_SHADOWED: f32 = 40.0;

/// How little term is left where the shadow has come off, out of the same 255.
///
/// The other side of the same gap: `8.11` is the largest reading either table
/// calls lit, `66.53` the smallest it calls shadowed.
const CONTACT_LIT: f32 = 20.0;

/// How far the contact's term may move and still be *where it was*, out of 255.
///
/// The tolerance on every "and the contact holds" clause. **Swept:** every arm
/// that is meant to leave it alone reads `70.73` on radv and `70.44` on
/// lavapipe, on every rung the claim is read on and at every station it is
/// pushed to — the same number to a hundredth, not a number inside a tolerance —
/// so this is a guard against readback noise rather than slack the claim needs.
const CONTACT_HELD: f32 = 2.0;

/// What share of the acne block is a self-shadowing dot once the normal offset
/// is gone, as a percentage.
///
/// **Swept:** `41.5329%` on radv and `41.5329%` on lavapipe with
/// `r_shadow_normal_offset` at zero, against `0.0000%` on both where it ships.
/// A floor at about half of what was seen, because it is a floor on an artefact
/// there is a great deal of rather than a second golden written in numbers.
///
/// The `disc` rung reads the same pair; `box` reads `42.6108%` on radv and
/// `42.7305%` on lavapipe. At [`sun::NOON_TICK`] the block counts `0.0000%` on
/// both adapters with this count at zero. What this count covers is how fast the
/// receiver climbs across one shadow texel — [`GRAZING_OVER_STEEP`]'s own
/// argument — and under a sun that steep it barely climbs, so there is nothing
/// there for zeroing it to buy back. The quantisation the constant bias covers
/// is still there, which is why the next constant's own noon reading is a rise
/// and this one's is not.
const ACNE_WITHOUT_OFFSET: f32 = 20.0;

/// The same once the constant bias is gone instead.
///
/// **Swept:** `3.2575%` on radv and `3.2335%` on lavapipe with `r_shadow_bias`
/// at zero. Two orders smaller than the offset's, which is the whole shape of
/// the seventh decision — over this block the offset covers a lost depth bias
/// nearly on its own — so the floor is set under it rather than at half of it.
///
/// The `disc` rung reads the same pair. `box` reads `0.0000%` on both adapters —
/// [`OFFSET_COVERED_RUNG`]'s own reading, and why *this* clause on that rung is
/// read against [`ACNE_WITHOUT_BIAS_REDUCED`] at another offset instead of
/// against this floor here; the clause above it, which zeroes the normal offset
/// outright, is read on that rung where the sample ships like every other. At
/// [`sun::NOON_TICK`] the shipped rung reads `2.9222%` on radv and `1.1737%` on
/// lavapipe.
const ACNE_WITHOUT_BIAS: f32 = 1.5;

/// What share of the block may be dots on an arm that is meant to be smooth.
///
/// **Swept:** `0.0000%` on both adapters at every count either table lists at or
/// above what ships, with a `0.0240%` seen at an offset of 32 — so this is set
/// an order over the largest reading a smooth arm produced and well under
/// [`ACNE_WITHOUT_BIAS`], which is the smallest rise it has to separate from.
const SMOOTH_PERCENT: f32 = 1.0;

/// What `r_shadow_normal_offset` is pulled back to to read the **constant
/// bias's** clause on [`OFFSET_COVERED_RUNG`], in the same texels.
///
/// **Under the narrowest kernel on the ladder the offset the sample ships covers
/// this block on its own** — [`OFFSET_COVERED_RUNG`] carries what that costs —
/// and this is the station where it stops covering it, so this is where that
/// rung's two counts trade again.
///
/// **Swept** on that rung at [`sun::GRAZING_TICK`] over [`acne_block`] at
/// [`CLAIM_EXTENT`], three runs per adapter and the same digits every time
/// (2026-09-05). The columns are the arm a setup at that offset calls `shipped`
/// and the arm it calls `no bias` — the same frames with `r_shadow_bias` at zero:
///
/// | `r_shadow_normal_offset` | shipped bias, radv | zero bias, radv | shipped bias, lavapipe | zero bias, lavapipe |
/// | --- | --- | --- | --- | --- |
/// | 1.00 | `43.3054%` | `44.0240%` | `43.3293%` | `44.0240%` |
/// | 1.25 | `27.9521%` | `43.6886%` | `27.8802%` | `43.6886%` |
/// | 1.50 (this) | `4.6467%` | `29.1737%` | `4.9581%` | `29.5090%` |
/// | 1.75 | `0.0000%` | `6.4192%` | `0.0000%` | `6.4192%` |
/// | 2.00 (ships) | `0.0000%` | `0.0000%` | `0.0000%` | `0.0000%` |
///
/// **The middle of the window rather than either edge of it.** At 1.00 the row's
/// own shipped arm is rougher than its `no offset` arm — `43.3054%` against that
/// arm's `42.6108%` — so [`ACNE_WITHOUT_OFFSET`]'s clause inverts on that row
/// and goes red. At 1.75 the two counts do trade, but the rise is `6.4192%` and
/// the next station, 2.00, reads `0.0000%` on both adapters: 1.75 is the last
/// station before the rise disappears, and a driver a fraction wider than radv
/// would stand on the wrong side of it. Here both clauses are wide, and there is
/// a working station on either side.
const REDUCED_OFFSET: f32 = 1.5;

/// What share of the block is a dot at [`REDUCED_OFFSET`] once the constant bias
/// is gone, as a percentage.
///
/// [`ACNE_WITHOUT_BIAS`]' floor for the one row read at that offset, and a
/// second constant rather than that one because the rise a constant bias is
/// worth is a function of how much of it the normal offset had already covered:
/// where the sample ships, zeroing the bias buys back about three points of this
/// block, and pulled back to [`REDUCED_OFFSET`] it buys back about twenty-nine.
///
/// **Swept:** [`REDUCED_OFFSET`]'s own table, whose middle row is this reading —
/// `29.1737%` on radv and `29.5090%` on lavapipe against the `4.6467%` and
/// `4.9581%` the same frames read with the bias where it ships (2026-09-05). Set
/// at about half the rise, on [`ACNE_WITHOUT_OFFSET`]'s terms, which also leaves
/// it three times clear of the arm it has to separate from.
const ACNE_WITHOUT_BIAS_REDUCED: f32 = 15.0;

/// The rung of the filter ladder whose own kernel the shipped normal offset
/// covers, and the two things the walk below does about it.
///
/// `box`. Its `tile_box_pcf` takes a three-by-three of taps a texel apart and
/// has no radius at all, where `disc` and the shipped rung both run `tile_pcf`
/// at `SHADOW_FILTER_TEXELS` — so it is the narrowest kernel on the ladder, and
/// a kernel reads its own quantised depth back only where it reaches further
/// *back* than the sideways walk `r_shadow_normal_offset` makes. At the two
/// texels that ship, this one does not.
///
/// **One clause of its acne half is read at [`REDUCED_OFFSET`] rather than where
/// the sample ships**, and it is the constant bias's. With `r_shadow_bias`
/// zeroed at the shipped offset this rung's block is `0.0000%` dots on both
/// adapters, against the `3.2575%` and `3.2335%` the shipped rung reads on the
/// same frames — [`ACNE_WITHOUT_BIAS`]' own sweep — so there is no rise there
/// for that clause to be about. Pulled back, the two counts trade on this rung
/// as they do on the others, and [`ACNE_WITHOUT_BIAS_REDUCED`] is the floor they
/// trade over. **One offset per rung for that clause and not both on every
/// rung**: at the offset that ships, every other rung's rise is the rise the
/// sample actually has, and reading those again at an offset nothing ships would
/// be a second claim about a configuration nobody runs.
///
/// The acne half's **other** clause — the normal offset zeroed outright — is
/// read on this rung where the sample ships, like every other rung: its block
/// counts `42.6108%` dots on radv and `42.7305%` on lavapipe against the
/// `0.0000%` its own shipped arm reads, with the contact unmoved at `70.73` and
/// `70.44`. Only the constant bias has nothing to be about at two texels.
///
/// **[`HELD_OFFSET`]'s two clauses are read on this rung at a station of its
/// own**, [`HELD_OFFSET_COVERED_RUNG`], and it is the paragraph above turned
/// around. Pushed to [`HELD_OFFSET`] the contact reads `70.73` on radv and
/// `70.44` on lavapipe and the pavement past it `68.33` on both, which is this
/// rung's own shipped arm to a hundredth — the very reading the anti-vacuity
/// clause refuses, because a contact that did not move is what a knob wired to
/// nothing draws. A narrower kernel carries the sideways walk less far, so the
/// station where this one loses the far end of the strip is further out; that
/// constant carries the sweep between the two.
///
/// **Every other clause is read on this rung at the count the sample ships**,
/// which is what makes the station above one kernel's own reach rather than a
/// rung with a defect in it: at [`PETER_PAN_BIAS`] its contact reads `0.00` on
/// both adapters and the pavement past it `68.08`.
///
/// Held against the ladder the engine declares rather than trusted: a rung
/// renamed out from under this fails the run, where an exclusion that silently
/// excluded nothing would leave the ladder short by one and say so nowhere.
const OFFSET_COVERED_RUNG: &str = "box";

/// The **shadow term** at a world point: the frame drawn without the shadow
/// passes, less the frame drawn with them.
///
/// `the_colonnades_shadow_crosses_the_cascade_split_without_a_step`'s quantity
/// and its reason: the pavement's own Lambert falloff, the occlusion pass and
/// the tonemap are in both frames and cancel, so what is left is what the sun's
/// shadow map did at that point. A brightness on its own could not say that —
/// pavement beside a block is darker than open pavement whether a shadow reaches
/// it or not.
///
/// **[`BLOCK`] unscaled**, where the readings at [`CLAIM_EXTENT`] elsewhere in
/// this file take [`block_for`]'s scaled one: the contact is five centimetres
/// from the plinth's face and the gap a lifted shadow opens is a few centimetres
/// of pavement, so a block scaled with the extent would average the two together
/// again — which is the very thing [`CLAIM_EXTENT`] exists to stop.
fn shadow_term(
    flat: &Image,
    shadowed: &Image,
    camera: &Camera,
    extent: (u32, u32),
    at: Vec3,
) -> f32 {
    let pixel = project(camera, extent, at);
    brightness(flat, pixel, BLOCK) - brightness(shadowed, pixel, BLOCK)
}

/// One arm's four readings, and whether it drew the shipped arm's own frame.
struct Reading {
    /// What the run's lines and the faults call this arm.
    name: &'static str,
    /// What share of [`acne_block`] is a self-shadowing dot.
    dots: f32,
    /// The block's mean, which [`LIT_PAVEMENT`] is read against.
    mean: f32,
    /// The shadow term at [`plaza::PLINTH_CONTACT`].
    contact: f32,
    /// The deepest shadow term over [`BEYOND_CONTACT`].
    beyond: f32,
    /// Whether this arm drew the shipped arm's frame byte for byte — the frame a
    /// count that never reached the shader draws, and the one every "holds"
    /// clause below is trivially true on. `false` on the shipped arm itself,
    /// which is the frame the others are compared against.
    unmoved: bool,
}

/// What one setup's arms read.
///
/// The readings rather than the frames they came off: every clause below holds
/// two of these against each other, and all of them come from one setup's own
/// pose, sun and filter.
struct Trade {
    /// Which paths drew this setup's frames.
    paths: String,
    /// The shipped arm, each count at **zero**, and each count **pushed**, in
    /// that order.
    arms: [Reading; 5],
}

/// Draws one setup's arms and reads the pair of artefacts off each.
///
/// The pose, the sun and the filter come off the [`Arm`] rather than from the
/// caller, so a setup on another rung is read against **its own** control: the
/// frame every shadow term here is a difference against is that setup drawn with
/// the shadow passes out, and [`acne_block`] is projected through that setup's
/// own camera.
///
/// `pushed_offset` is the count the last arm's `r_shadow_normal_offset` is
/// pushed to, off the [`Setup`] rather than a constant here: how far a sideways
/// walk has to go before it reaches this strip is a function of the kernel doing
/// the reading, which is what [`HELD_OFFSET_COVERED_RUNG`] is about.
fn bias_trade(extent: (u32, u32), name: &str, base: Arm, pushed_offset: f32) -> Trade {
    let camera = base.camera();
    let (centre, half) = acne_block(&camera, extent);
    let (flat, paths, _) = draw(extent, base.without_shadows());

    let mut shipped_pixels = Vec::new();
    let mut read = |arm_name: &'static str, arm: Arm| {
        let (image, _, _) = draw(extent, arm);
        let unmoved = if arm_name == "shipped" {
            shipped_pixels = image.pixels().to_vec();
            false
        } else {
            image.pixels() == shipped_pixels.as_slice()
        };
        let reading = Reading {
            name: arm_name,
            dots: speckle_percent(&image, centre, half),
            mean: brightness(&image, centre, half),
            contact: shadow_term(&flat, &image, &camera, extent, plaza::PLINTH_CONTACT),
            beyond: BEYOND_CONTACT
                .into_iter()
                .map(|out| {
                    let at = plaza::PLINTH_CONTACT + Vec3::new(0.0, 0.0, out);
                    shadow_term(&flat, &image, &camera, extent, at)
                })
                .fold(f32::MIN, f32::max),
            unmoved,
        };
        eprintln!(
            "sundial golden: {name}: the {arm_name} arm on {paths} — {dots:.4}% of the block is a \
             dot, mean {mean:.2}/255, shadow term {contact:.2} at the contact and {beyond:.2} \
             deepest beyond it",
            dots = reading.dots,
            mean = reading.mean,
            contact = reading.contact,
            beyond = reading.beyond,
        );
        reading
    };
    let arms = [
        read("shipped", base),
        read("no offset", base.offset(0.0)),
        read("no bias", base.biased(0.0)),
        read("pushed bias", base.biased(PETER_PAN_BIAS)),
        read("pushed offset", base.offset(pushed_offset)),
    ];
    Trade { paths, arms }
}

/// One row of the walk below: an [`Arm`]'s five readings, and what this row's
/// own normal offset lets them say.
///
/// A row rather than a rung, because [`OFFSET_COVERED_RUNG`] is two of them: the
/// one clause that rung cannot carry at the offset the sample ships is read on a
/// second row at [`REDUCED_OFFSET`], and everything else on its first.
struct Setup {
    /// What the run's lines and the faults call this row.
    name: String,
    /// Which rung of the ladder drew it, which is what holds the constant
    /// bias's clause to being read on every one of them.
    rung: &'static str,
    /// The arm every reading is taken off.
    base: Arm,
    /// The floor the **"zero the constant bias"** clause is held over here, or
    /// `None` where this row's own normal offset leaves that count nothing to be
    /// about — [`OFFSET_COVERED_RUNG`] at the offset that ships, whose reading
    /// is the row below it instead.
    ///
    /// **That one clause and no other.** The acne half's other clause zeroes the
    /// normal offset outright, which leaves a rise under every kernel on the
    /// ladder, so it is read on every row and this field says nothing about it.
    ///
    /// A floor per row rather than one constant, because the rise a constant
    /// bias is worth is a function of how much of it the normal offset had
    /// already covered: [`ACNE_WITHOUT_BIAS`] where the sample's own offset drew
    /// the frame, [`ACNE_WITHOUT_BIAS_REDUCED`] at [`REDUCED_OFFSET`].
    acne_without_bias: Option<f32>,
    /// What this row's pushed-offset arm pushes `r_shadow_normal_offset` to.
    ///
    /// [`HELD_OFFSET`] on every row but [`OFFSET_COVERED_RUNG`]'s, which reads
    /// the pair at [`HELD_OFFSET_COVERED_RUNG`]: a count of texels is a distance
    /// the lookup walks sideways, and how far it has to walk before it reaches
    /// the pavement this pair reads is a function of how far the kernel reading
    /// it already reaches. Per row rather than per rung only because a [`Setup`]
    /// is a row; both of [`OFFSET_COVERED_RUNG`]'s carry the same count.
    pushed_to: f32,
    /// Whether [`HELD_OFFSET`]'s pair of clauses is read on this row.
    ///
    /// `false` on [`OFFSET_COVERED_RUNG`]'s [`REDUCED_OFFSET`] row alone, whose
    /// shipped arm stands at an offset nothing ships — the pair is read on that
    /// rung's other row, against the frame the sample actually draws, and
    /// reading it twice would be a second claim about a configuration nobody
    /// runs. That row's pushed arm is still drawn and still held to the
    /// anti-vacuity clauses below; it is only what those two readings are asked
    /// to say about each other that is held back.
    held_offset: bool,
}

/// **The sun's two bias counts trade acne against the plinth's own contact, and
/// they do not trade it the same way.**
///
/// `docs/plan/sample/18-sundial.md`'s milestone 2: the pair of artefacts moving
/// against each other as the two counts change, on the fixture the plaza was
/// laid out for, where `docs/plan/45-shadows.md`'s seventh decision could only
/// measure one room's wall-foot strip and one patch's dots.
///
/// Five arms of one frame, all at [`sun::GRAZING_TICK`] — the most grazing sun
/// this clock reaches, and the worst case for both artefacts — and two readings
/// off each: [`speckle_percent`] over [`acne_block`]'s open pavement, and
/// [`shadow_term`] at [`plaza::PLINTH_CONTACT`] and at the [`BEYOND_CONTACT`]
/// stations past it. The arms are what ships, each count at **zero**, and each
/// count **pushed** — [`PETER_PAN_BIAS`] and [`HELD_OFFSET`].
///
/// What comes out is three claims, and the third is the one no still frame and
/// no golden could make:
///
/// * **Zero either count and the pavement roughens; the contact does not
///   move.** [`ACNE_WITHOUT_OFFSET`] and [`ACNE_WITHOUT_BIAS`] are the two
///   rises, two orders apart.
/// * **Push the constant bias and the shadow comes off the plinth** — the
///   contact lights while the pavement past it is still shadowed, which is
///   peter-panning rather than a shadow that has gone — and the acne block stays
///   smooth.
/// * **Push the normal offset far past what acne needs and the contact does not
///   move**, though the frame is a different picture. That is the seventh
///   decision's claim — a sideways move keeps a contact — measured rather than
///   argued.
///
/// # The setups
///
/// That set of arms, once per **setup**, and each setup read against its own
/// control: [`bias_trade`] takes the pose, the sun and the filter off the
/// [`Arm`] it is handed, so no arm is compared against a frame another rung
/// drew.
///
/// * **The shipped rung**, which is the setup every constant above was swept on.
/// * **Every other rung the engine declares**, out of
///   `filter::names(filter::FILTER)` rather than written down here — today that
///   is `disc` and `box`. A rung is a kernel over the same shadow map and both
///   of the artefacts this pair is about are things a kernel averages over, so a
///   count that covers a grazing receiver under one kernel need not cover it
///   under another.
/// * **[`OFFSET_COVERED_RUNG`] a second time, at [`REDUCED_OFFSET`]**, which is
///   where the **constant bias's** clause is read on it: under the ladder's
///   narrowest kernel the shipped normal offset covers this block on its own, so
///   at the offset that ships there is no rise for that count to be about. Its
///   other clause — the normal offset zeroed outright — is read on its first row
///   like every other rung's. That constant carries the sweep.
/// * **The shipped rung a second time, from [`plaza::pavement_camera`]**
///   (2026-09-05), which is the one row not framed from the fixture pose. Every
///   constant above was swept from [`plaza::fixed_camera`], and two of the three
///   claims are read through a screen-space statistic — [`speckle_percent`]
///   counts pixels, and [`shadow_term`] averages a block of them — so a count
///   that was right only about the pixels one pose resolves would pass all of
///   them. The ladder's rungs stay on the fixture pose: which kernel reads a
///   texel is not a function of where the camera stands, where how much of a
///   texel one pixel covers is.
///
/// So the acne half is read on **every** rung of the ladder — the normal
/// offset's clause at the offset the sample ships throughout, the constant
/// bias's at the one offset per rung where the two counts trade — and the
/// contact half on every rung as well: [`PETER_PAN_BIAS`] at the one count
/// throughout, and [`HELD_OFFSET`]'s pair at the station each kernel's own
/// sideways reach puts it at, which is that constant on every rung but
/// [`OFFSET_COVERED_RUNG`] and [`HELD_OFFSET_COVERED_RUNG`] on that one.
///
/// Every setup shares [`sun::GRAZING_TICK`], which is what lets
/// [`BEYOND_CONTACT`]'s stations run down `+z` for all of them, and every one
/// but the last is framed from [`plaza::fixed_camera`].
///
/// # What it is not read on
///
/// Two more setups were measured and are not here, and neither is a bound that
/// was loosened to fit it.
///
/// The first is **the top of the sun's arc**, [`sun::NOON_TICK`]. Two of the
/// three claims fail there and both for one reason — a sun that steep throws a
/// plinth shadow a fraction of the block's own height long. Zeroing the normal
/// offset draws `0.0000%` dots on both adapters, so there is no rise for the
/// first claim to be about; and no count of constant bias lifts the contact
/// while the pavement past it is still shadowed, because along a ray that steep
/// the depth to cross is the block's height for the contact and for every
/// station beyond it alike, so the whole shadow goes at once. [`PETER_PAN_BIAS`]
/// carries both sweeps. Reading the pair at the top of the arc wants a **second
/// caster**, a thin one whose noon shadow outruns the gap a bias opens, and not
/// another arm of this walk.
///
/// The second is [`plaza::counter_camera`], and the third pose above is what a
/// session spent looking for it closed (2026-09-05). [`plaza::PLINTH_CONTACT`]
/// and every one of [`BEYOND_CONTACT`]'s stations is **behind that pose's eye**:
/// it stands past the plinth's near face looking away down the plaza, so
/// [`on_screen`] refuses all of them and [`project`] would panic rather than
/// report. The acne half of the pair *is* framed from there — the block's four
/// corners all project — so what that pose is short of is the contact, and it is
/// still not a setup here. [`plaza::pavement_camera`] is the pose that frames
/// both, and its own doc carries what places it.
///
/// # Anti-vacuity
///
/// Five ways this could pass while measuring nothing, and a fault each. A
/// setup's arms could be **one picture**, where every "holds" clause is
/// trivially true and every "moves" clause would have failed — they are compared
/// as bytes against that setup's own shipped arm. The acne block could be **in
/// shadow**, where it counts no dots however the counts are set —
/// [`LIT_PAVEMENT`] is read off every arm of every setup. The contact could be
/// **lit to begin with**, where "it opened" means nothing — each setup's shipped
/// arm is held over [`CONTACT_SHADOWED`]. The pushed-offset arm could be a knob
/// that never reached the shader, where a contact that did not move is exactly
/// what a no-op draws — that arm's shadow beyond the contact is held to have
/// *fallen* against its own setup's shipped one, so the count is shown to have
/// done something before it is credited with not doing this, and that is the
/// clause [`HELD_OFFSET_COVERED_RUNG`] exists to satisfy. And the constant
/// bias's clause could **silently stop being read on a rung**, which is exactly
/// what [`REDUCED_OFFSET`] exists to stop happening again — every rung the
/// engine declares is held to appear on a setup that reads it, and
/// [`OFFSET_COVERED_RUNG`] is held against the ladder besides, so a rung renamed
/// out from under either fails the run rather than dropping out of it.
///
/// # How it was shown to fail
///
/// By making `crcbl_render::shadow::Cascades::params` hand the shader
/// `DEPTH_BIAS_TEXELS` and `NORMAL_OFFSET_TEXELS` again instead of reading
/// `r_shadow_bias` and `r_shadow_normal_offset` — a getter that ignores its own
/// console cell, which is the failure this whole pair of variables can hide
/// behind and the one every reading here would otherwise report as a clean
/// frame. **Every setup went red, on both adapters**, and the run reports them
/// together because the arms are read into one list of faults rather than one
/// assertion each. The rung that was added carries the same sentences under its
/// own name; on radv three of its lines were
///
/// > on the disc rung the pushed bias arm drew the shipped arm's frame byte for
/// > byte, so every reading taken off it is the shipped reading under another
/// > name
/// >
/// > on the disc rung with the constant bias at zero the block is 0.0000% dots
/// > against 0.0000% as the sample ships — short of the 1.5% this count is
/// > worth. …
/// >
/// > on the disc rung at 96 texels of constant bias the contact still carries
/// > 70.73 of shadow term, over the 20 this reading calls lit — the shadow has
/// > not come off its caster …
///
/// and on lavapipe the last of those read `70.44`.
///
/// **The rows added for [`OFFSET_COVERED_RUNG`] were reddened on their own
/// axes** (2026-09-05). The reduced row's offset put back to the two texels
/// that ship, so its constant bias has nothing to buy back:
///
/// > on the box rung at 2 texels of normal offset with the constant bias at
/// > zero the block is 0.0000% dots against 0.0000% as the sample ships — short
/// > of the 15% this count is worth. …
///
/// `r_shadow_bias` held at zero on both of that row's readings, so the `no bias`
/// arm is the shipped arm under another name and its rise is nothing:
///
/// > on the box rung at 1.5 texels of normal offset the no bias arm drew the
/// > shipped arm's frame byte for byte, …
/// >
/// > on the box rung at 1.5 texels of normal offset with the constant bias at
/// > zero the block is 29.1737% dots against 29.1737% as the sample ships …
///
/// The shipped-offset `box` row's own offset zeroed, so the normal offset's
/// clause — which is read on that row — has no rise either:
///
/// > on the box rung with the normal offset at zero the block is 42.6108% dots
/// > against 42.6108% as the sample ships — short of the 20% this count is
/// > worth. …
///
/// And the reduced row's floor taken away, which is the exclusion silently
/// excluding a rung again:
///
/// > the engine declares ["pcss", "disc", "box"] and no setup zeroes the
/// > constant bias on `box`, so that rung is one this pair no longer says
/// > anything about that count on, and nothing else would say so
///
/// **The row added for [`plaza::pavement_camera`] was reddened on two axes**
/// (2026-09-05). `r_shadow_bias` held at zero on both of that row's arms, so its
/// `no bias` arm is the shipped arm under another name and the rise it is
/// credited with is nothing — both halves, on both adapters:
///
/// > on the shipped pcss from the pavement pose the no bias arm drew the shipped
/// > arm's frame byte for byte, so every reading taken off it is the shipped
/// > reading under another name
/// >
/// > on the shipped pcss from the pavement pose with the constant bias at zero
/// > the block is 3.3629% dots against 3.3629% as the sample ships — short of
/// > the 1.5% this count is worth. …
///
/// and on lavapipe both numbers in the second line read `3.4974%`. Then the pose
/// itself pitched to the horizontal, so the pavement a metre past the contact
/// leaves the bottom of the frame — the row refuses the run rather than reading
/// a pixel that is not there:
///
/// > Vec3(-0.2, 0.0, 3.8) is behind a 1024x768 frame or outside it, so the claim
/// > about it would be about a pixel that is not there
///
/// [`plaza`]'s own
/// `the_pavement_pose_frames_the_shadows_the_fixed_one_stands_in_front_of`
/// refuses that pose first, with no GPU and naming the station it lost:
///
/// > the contact reading 1.0 m out at Vec3(-0.2, 0.0, 3.8) is behind the
/// > pavement pose's eye or outside its frame, so `tests/golden.rs`'s `project`
/// > would panic on it
///
/// **[`HELD_OFFSET_COVERED_RUNG`] was reddened from both sides** (2026-09-05),
/// which is what says the window it sits in has two edges rather than one. Moved
/// to the next station past that window, so the contact goes with the pavement:
///
/// > on the box rung at 44 texels of normal offset the contact's shadow term
/// > moved from 70.73 to 31.29. `docs/plan/45-shadows.md`'s seventh decision is
/// > that a move along the receiver's own normal leaves the depth it compares
/// > alone and therefore keeps a contact; this is the fixture that says so
///
/// and on lavapipe that line read `70.44` and `31.13`. Pulled back to the offset
/// the sample ships, so the pushed arm is that row's own shipped frame and the
/// clause it is credited with is the one a knob wired to nothing passes — both
/// halves of the anti-vacuity pair, identically on both adapters:
///
/// > on the box rung the pushed offset arm drew the shipped arm's frame byte for
/// > byte, so every reading taken off it is the shipped reading under another
/// > name
/// >
/// > on the box rung at 2 texels of normal offset the pavement past the contact
/// > carries 68.33 of shadow term at its deepest against the shipped arm's 68.33
/// > — the count reached the frame nowhere, so a contact that did not move is
/// > what a knob wired to nothing draws
///
/// # What was measured
///
/// The tables are on [`PETER_PAN_BIAS`], [`HELD_OFFSET`],
/// [`HELD_OFFSET_COVERED_RUNG`], [`REDUCED_OFFSET`] and [`OFFSET_COVERED_RUNG`],
/// and the run prints every arm's four readings again on whatever adapter it
/// opened.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sundial-golden.sh"]
fn the_two_bias_counts_trade_acne_against_the_plinths_own_contact() {
    let extent = CLAIM_EXTENT;
    let shipped = crcbl::render::shadow::shipped_filter().label();
    let ladder = filter::names(filter::FILTER);
    assert!(
        ladder.contains(&OFFSET_COVERED_RUNG),
        "the engine declares {ladder:?} and none of them is `{OFFSET_COVERED_RUNG}`, so the rung \
         this pair reads at its own offset is a rung nothing runs and the exclusion excludes \
         nothing"
    );
    let grazing = Arm::shipped().at_tick(sun::GRAZING_TICK);

    // One row per rung, at the offset the sample ships. The rung whose kernel
    // that offset already covers reads the constant bias's clause a row further
    // down instead, and pushes the normal offset a station further out for
    // `HELD_OFFSET`'s pair; the rule is applied here rather than to the rows
    // afterwards so it still holds if the engine ever ships that rung as its
    // default.
    let row = |name: String, rung: &'static str, base: Arm| {
        let covered = rung == OFFSET_COVERED_RUNG;
        Setup {
            name,
            rung,
            base,
            acne_without_bias: (!covered).then_some(ACNE_WITHOUT_BIAS),
            pushed_to: if covered {
                HELD_OFFSET_COVERED_RUNG
            } else {
                HELD_OFFSET
            },
            held_offset: true,
        }
    };
    let mut setups = vec![row(format!("the shipped {shipped}"), shipped, grazing)];
    for name in ladder {
        if *name != shipped {
            setups.push(row(format!("the {name} rung"), name, grazing.on(name)));
        }
    }
    assert!(
        setups.len() > 1,
        "the engine declares {ladder:?}, so there is no second rung to read this pair on and the \
         claim is a claim about one filter"
    );

    setups.push(Setup {
        name: format!("the {OFFSET_COVERED_RUNG} rung at {REDUCED_OFFSET} texels of normal offset"),
        rung: OFFSET_COVERED_RUNG,
        base: grazing.on(OFFSET_COVERED_RUNG).offset(REDUCED_OFFSET),
        acne_without_bias: Some(ACNE_WITHOUT_BIAS_REDUCED),
        pushed_to: HELD_OFFSET_COVERED_RUNG,
        held_offset: false,
    });

    // And the shipped rung a second time, from the other pose that frames the
    // plinth's contact. The ladder's rows stay on the fixture pose: what a
    // second pose is owed is a second place for a mis-set count to show, and
    // which kernel reads a texel is not a function of where the camera stands.
    setups.push(Setup {
        name: format!("the shipped {shipped} from the pavement pose"),
        rung: shipped,
        base: grazing.framed_on_the_pavement(),
        acne_without_bias: Some(ACNE_WITHOUT_BIAS),
        pushed_to: HELD_OFFSET,
        held_offset: true,
    });

    // A rung no row reads the constant bias's clause on is a rung that dropped
    // out of it rather than one read where the two counts trade — the very thing
    // the row above exists to stop, and the one an exclusion can hide.
    for rung in ladder {
        assert!(
            setups
                .iter()
                .any(|setup| setup.rung == *rung && setup.acne_without_bias.is_some()),
            "the engine declares {ladder:?} and no setup zeroes the constant bias on `{rung}`, so \
             that rung is one this pair no longer says anything about that count on, and nothing \
             else would say so"
        );
    }

    let mut faults = Vec::new();
    for Setup {
        name,
        rung: _,
        base,
        acne_without_bias,
        pushed_to,
        held_offset,
    } in setups
    {
        let Trade { paths, arms } = bias_trade(extent, &name, base, pushed_to);
        for arm in &arms {
            if arm.unmoved {
                faults.push(format!(
                    "on {name} the {arm} arm drew the shipped arm's frame byte for byte, so \
                     every reading taken off it is the shipped reading under another name",
                    arm = arm.name,
                ));
            }
            if arm.mean <= LIT_PAVEMENT {
                faults.push(format!(
                    "on {name} the block reads {mean:.2}/255 on the {arm} arm, under the \
                     {LIT_PAVEMENT} lit pavement stands at. This is a reading of a shadow, and a \
                     shadow counts no dots however the counts are set",
                    mean = arm.mean,
                    arm = arm.name,
                ));
            }
        }
        let [shipped_arm, no_offset, no_bias, pushed_bias, pushed_offset] = &arms;
        eprintln!(
            "sundial golden: the bias pair on {name}, {paths} — the contact reads {ships:.2}/255 \
             as the sample ships, {lifted:.2} at {PETER_PAN_BIAS} texels of constant bias and \
             {held:.2} at {pushed_to} of normal offset",
            ships = shipped_arm.contact,
            lifted = pushed_bias.contact,
            held = pushed_offset.contact,
        );

        // The contact has to be a shadow before "it opened" is a statement about
        // anything.
        if shipped_arm.contact <= CONTACT_SHADOWED {
            faults.push(format!(
                "on {name} the pavement at the plinth's contact carries {:.2} of shadow term as \
                 the sample ships, under the {CONTACT_SHADOWED} this reading calls shadowed — so \
                 there is no shadow here for a bias to take off",
                shipped_arm.contact,
            ));
        }

        // Zero either count: the pavement roughens, and the contact stays put.
        //
        // The normal offset on every row, because zeroing it outright leaves a
        // rise under every kernel on the ladder; the constant bias only where
        // this row's own offset left it something to be about, which is what
        // `acne_without_bias` carries.
        let zeroed = [
            Some(("normal offset", no_offset, ACNE_WITHOUT_OFFSET)),
            acne_without_bias.map(|least| ("constant bias", no_bias, least)),
        ];
        for (count, arm, least) in zeroed.into_iter().flatten() {
            if arm.dots <= least || arm.dots <= shipped_arm.dots {
                faults.push(format!(
                    "on {name} with the {count} at zero the block is {:.4}% dots against {:.4}% \
                     as the sample ships — short of the {least}% this count is worth. A count \
                     that buys no acne back when it is taken away is not what is covering the \
                     acne",
                    arm.dots, shipped_arm.dots,
                ));
            }
            if (arm.contact - shipped_arm.contact).abs() >= CONTACT_HELD {
                faults.push(format!(
                    "on {name} with the {count} at zero the contact's shadow term moved from \
                     {:.2} to {:.2}. Acne is what a count too small draws; a contact that moved \
                     as well says this reading is about the whole frame rather than about the \
                     pavement under the plinth",
                    shipped_arm.contact, arm.contact,
                ));
            }
        }

        // Push the constant bias: the shadow comes off the plinth and stays on
        // the pavement past it.
        if pushed_bias.contact >= CONTACT_LIT {
            faults.push(format!(
                "on {name} at {PETER_PAN_BIAS} texels of constant bias the contact still carries \
                 {:.2} of shadow term, over the {CONTACT_LIT} this reading calls lit — the \
                 shadow has not come off its caster and there is no peter-panning here to \
                 measure",
                pushed_bias.contact,
            ));
        }
        if pushed_bias.beyond <= CONTACT_SHADOWED {
            faults.push(format!(
                "on {name} at {PETER_PAN_BIAS} texels the pavement past the contact carries \
                 {:.2} of shadow term at its deepest, under the {CONTACT_SHADOWED} this reading \
                 calls shadowed. The shadow has gone rather than come off its caster, and \
                 peter-panning is the gap between the two",
                pushed_bias.beyond,
            ));
        }
        if pushed_bias.dots >= SMOOTH_PERCENT {
            faults.push(format!(
                "on {name} at {PETER_PAN_BIAS} texels the block is {:.4}% dots, past \
                 {SMOOTH_PERCENT}% — a count raised past what acne needs must not draw acne of \
                 its own",
                pushed_bias.dots,
            ));
        }

        // Push the normal offset the same way: the contact does not move. The
        // two clauses go together or not at all — a contact that held is what a
        // knob wired to nothing draws, so the reading that shows the count
        // reached the frame is the only thing that makes the other one a claim.
        if held_offset {
            if (pushed_offset.contact - shipped_arm.contact).abs() >= CONTACT_HELD {
                faults.push(format!(
                    "on {name} at {pushed_to} texels of normal offset the contact's shadow \
                     term moved from {:.2} to {:.2}. `docs/plan/45-shadows.md`'s seventh \
                     decision is that a move along the receiver's own normal leaves the depth it \
                     compares alone and therefore keeps a contact; this is the fixture that says \
                     so",
                    shipped_arm.contact, pushed_offset.contact,
                ));
            }
            if pushed_offset.beyond >= shipped_arm.beyond {
                faults.push(format!(
                    "on {name} at {pushed_to} texels of normal offset the pavement past the \
                     contact carries {:.2} of shadow term at its deepest against the shipped \
                     arm's {:.2} — the count reached the frame nowhere, so a contact that did \
                     not move is what a knob wired to nothing draws",
                    pushed_offset.beyond, shipped_arm.beyond,
                ));
            }
        }
        if pushed_offset.dots >= SMOOTH_PERCENT {
            faults.push(format!(
                "on {name} at {pushed_to} texels the block is {:.4}% dots, past \
                 {SMOOTH_PERCENT}%",
                pushed_offset.dots,
            ));
        }

        // And the two counts are not one knob: pushed the same way, one opens
        // the contact and the other leaves it alone.
        if pushed_bias.contact >= pushed_offset.contact {
            faults.push(format!(
                "on {name}, pushed past what acne needs, the constant bias leaves {:.2} of \
                 shadow term at the contact and the normal offset {:.2}. Two counts that did the \
                 same thing to a contact would be one quality knob, and this sample's pair of \
                 variables would be a distinction with nothing behind it",
                pushed_bias.contact, pushed_offset.contact,
            ));
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}

// ---------------------------------------------------------------------------
// The cascade cross-fade
// ---------------------------------------------------------------------------

/// How far apart the samples along a column's shadow stand, in metres of
/// pavement.
///
/// Under a pixel at [`CLAIM_EXTENT`] from [`plaza::fixed_camera`], for
/// [`SCAN_STEP`]'s reason: the walk is sampled at least as finely as the frame
/// resolves it, so a shell's mean is an average over the pixels it covers rather
/// than over a handful of them picked by rounding.
const CASCADE_WALK_STEP: f32 = 0.004;

/// How far from a column's own foot its walk starts, in metres along the shadow.
///
/// The shadow's axis begins *inside* the caster — a column is `COLUMN_HALF` on a
/// side in `plaza` and the axis runs out of its centre — so the first stretch
/// would read the column's own face rather than the pavement its shadow falls
/// on. [`plaza::hidden_from`] would refuse those samples anyway; starting past
/// them is what keeps the walk's own length honest.
const CASCADE_WALK_CLEARANCE: f32 = 0.45;

/// How far inside cascade 0 the walk reaches, in cross-fade bands.
const CASCADE_INNER_BANDS: f32 = 2.0;

/// How far past the split it reaches, in the same bands.
const CASCADE_OUTER_BANDS: f32 = 0.5;

/// How many shells the walk cuts one band into.
///
/// **The whole of what separates a band from an edge is here.** A switch with no
/// band puts its whole change into the *one* pair of neighbouring shells that
/// straddles the split, whatever this number is; a band spreads the same change
/// over all of them. So the finer the cut, the further apart the two cases
/// stand — and the fewer samples each shell holds, which is the other end of it.
///
/// **Swept** on radv, as the ratio the test bounds — the steepest step across
/// the split against the steepest the same walk shows clear of the band — and as
/// how many walks are left with both:
///
/// | shells per band | walks reading across the split | steepest ratio |
/// | --- | --- | --- |
/// | 8 | 2 | `3.77` over `2.07` |
/// | 12 | 3 | `2.83` over `1.55` |
/// | 16 | 3 | `2.24` over `1.43` |
/// | 24 | 0 | refused |
/// | 32 | 0 | refused |
///
/// Sixteen. Finer than that the shells stop holding [`CASCADE_SHELL_SAMPLES`],
/// every walk loses one end of its pair, and the run is refused rather than
/// passed.
const CASCADE_SHELLS_PER_BAND: f32 = 16.0;

/// How far off a shadow's own axis each sample stands, in metres.
///
/// **The offsets are what make this a reading of a shadow's edge rather than of
/// its middle.** A cascade switch is a switch of shadow map, and what differs
/// between the two maps at a vertical caster's shadow is the *width* of the
/// filter over it: `sun_penumbra_texels` in `shaders/mesh.slang` clamps its
/// estimate into two to eight texels **of the cascade the fragment landed in**,
/// and the outer cascade's texel is several times the near one's here. Deep in
/// the umbra both cascades answer the same nothing and out on the open pavement
/// both answer the same everything; the difference is at the edge, and these
/// straddle it — `plaza`'s `COLUMN_HALF` is where the geometric edge stands, and
/// there are offsets either side of it on both sides of the shadow.
///
/// The outermost is inside **half the lateral spacing of the colonnade's
/// shadows**, which the test asserts rather than assumes: the columns stand
/// [`plaza::COLONNADE_SPACING`] apart and their shadows are parallel strips, so
/// an offset past that would be a reading of the next column's shadow.
const CASCADE_LATERALS: [f32; 12] = [
    -0.26, -0.22, -0.18, -0.14, -0.10, -0.06, 0.06, 0.10, 0.14, 0.18, 0.22, 0.26,
];

/// The fewest samples a shell may hold before its mean is used.
///
/// A shell that collected fewer is one the colonnade or a lamp's reach hid most
/// of, and a mean over what is left is a mean over whichever end of the shell
/// happened to survive. Such a shell is dropped rather than averaged, and the
/// pairs either side of it with it.
///
/// **Swept, and it caught one.** At eight, column 3's walk at `-0.06` m read a
/// step of `13.88`/255 across the split against `2.05` clear of the band — and
/// the *same* `13.88` with the band collapsed to an edge, which is what says it
/// was not the switch at all but a shell the lamp's own reach had left a dozen
/// samples of. At sixteen that shell is dropped and the walks that remain are
/// the ones with a shell's worth of pavement behind every reading. At
/// twenty-four and thirty-two no walk has a pair either side of the split left, and
/// the test refuses the run rather than passing it.
const CASCADE_SHELL_SAMPLES: u32 = 16;

/// One walk: one column's shadow, read at one offset from its axis.
///
/// Every step below is taken **inside** one of these and never between two, so a
/// walk the colonnade hid half of contributes the pairs it has and nothing else.
/// That is what makes it sound to pool the readings of several columns: what is
/// pooled is the steps, not the levels, and two columns at different distances
/// from their own shadows legitimately darken the pavement by different amounts.
#[derive(Clone, Debug)]
struct Walk {
    /// Which column of the colonnade it follows.
    column: usize,
    /// How far off that column's shadow axis it stands, in metres.
    lateral: f32,
    /// The total darkening and the sample count in each shell, in order.
    shells: Vec<(f32, u32)>,
}

/// A shadow direction on the pavement, and the perpendicular the offsets step
/// along.
///
/// Both are the *colonnade's*: a vertical column's shadow is its own footprint
/// swept along the sun, so every column's strip runs along the first of these
/// and the strips stand apart along the second.
fn shadow_axes(sky: sun::Sky) -> (Vec3, Vec3) {
    let towards = sky.towards();
    let axis = Vec3::new(-towards.x, 0.0, -towards.z).normalize();
    (axis, Vec3::new(axis.z, 0.0, -axis.x))
}

/// Where each shell's middle stands, in metres from the eye.
fn cascade_shells(reach: f32, band: f32) -> Vec<f32> {
    let near_end = band.mul_add(-CASCADE_INNER_BANDS, reach);
    let width = band / CASCADE_SHELLS_PER_BAND;
    let spread = (CASCADE_INNER_BANDS + CASCADE_OUTER_BANDS) * CASCADE_SHELLS_PER_BAND;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a window of a few bands, a few shells to each"
    )]
    let count = spread.round() as usize;
    (0..count)
        .map(|index| {
            #[expect(clippy::cast_precision_loss, reason = "a shell index under a hundred")]
            let middle = index as f32 + 0.5;
            middle.mul_add(width, near_end)
        })
        .collect()
}

/// Walks every column of the colonnade's shadow across the cascade split and
/// bins what it reads into shells of distance from the eye.
///
/// `shadowed` and `flat` are one frame with the shadow passes on and off, and
/// what is binned is the **difference**: the pavement's own falloff is in both
/// and cancels, so what is left is the shadow term alone. A profile of raw luma
/// would carry the Lambert gradient [`SPECKLE_LUMA`]'s doc measures across this
/// stretch of plaza, which is larger than every step this reading is about.
///
/// A sample is taken only where the camera can **see** the pavement and no lamp
/// reaches it — [`plaza::hidden_from`] and [`plaza::lamplit`], which are the
/// plaza's own geometry rather than a copy of it here. Both refusals matter: a
/// column standing in front of its own neighbour's shadow puts that column's lit
/// face where the reading expects pavement, and a lamp puts a second shadow with
/// no cascades in it at all on top of the sun's.
fn cascade_walks(
    shadowed: &Image,
    flat: &Image,
    camera: &Camera,
    extent: (u32, u32),
    sky: sun::Sky,
    reach: f32,
    band: f32,
) -> Vec<Walk> {
    let (axis, perp) = shadow_axes(sky);
    let length = plaza::COLUMN_HEIGHT * sky.shadow_reach();
    let middles = cascade_shells(reach, band);
    let width = band / CASCADE_SHELLS_PER_BAND;
    let near_end = middles[0] - width / 2.0;
    let far_end = middles[middles.len() - 1] + width / 2.0;

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a shadow a few metres long walked in millimetres"
    )]
    let steps = ((length - CASCADE_WALK_CLEARANCE) / CASCADE_WALK_STEP) as u32;
    let mut walks = Vec::new();
    for column in 0..plaza::COLONNADE_COUNT {
        let foot = plaza::column_foot(column);
        for lateral in CASCADE_LATERALS {
            let mut shells = vec![(0.0f32, 0u32); middles.len()];
            for step in 0..=steps {
                let walked =
                    f32::from(u16::try_from(step).expect("the walk is a few thousand steps"));
                let along = walked.mul_add(CASCADE_WALK_STEP, CASCADE_WALK_CLEARANCE);
                let at = foot + axis * along + perp * lateral;
                let distance = at.distance(camera.eye);
                if distance < near_end || distance >= far_end {
                    continue;
                }
                if plaza::hidden_from(camera.eye, at) || plaza::lamplit(at) {
                    continue;
                }
                let Some(pixel) = on_screen(camera, extent, at) else {
                    continue;
                };
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "guarded into the window on the lines above"
                )]
                let shell = (((distance - near_end) / width) as usize).min(shells.len() - 1);
                shells[shell].0 +=
                    brightness(flat, pixel, (0, 0)) - brightness(shadowed, pixel, (0, 0));
                shells[shell].1 += 1;
            }
            walks.push(Walk {
                column,
                lateral,
                shells,
            });
        }
    }
    walks
}

/// How much darker than the frame with no shadow term the walks have to read on
/// each side of the split, in luma out of 255.
///
/// The anti-vacuity floor, and what says the profiles are readings of the
/// colonnade's shadow rather than of the open plaza beside it. Walks that had
/// drifted off the strips would find nothing to darken them, every shell would
/// read about zero, every step between them would be about zero, and the bound
/// below would hold on a frame with no shadow in it at all.
///
/// **Swept**, at [`CLAIM_EXTENT`] over the window these walks cover, on both
/// Vulkan adapters this workspace runs locally, on every arm the claim below is
/// read on:
///
/// | arm | cascade 0, radv | cascade 1, radv | cascade 0, lavapipe | cascade 1, lavapipe |
/// | --- | --- | --- | --- | --- |
/// | the shipped rung | `83.70` | `27.54` | `83.56` | `27.53` |
/// | the `disc` rung | `88.55` | `27.55` | `88.37` | `27.54` |
/// | the grazing sun | `43.60` | `47.73` | `43.46` | `47.64` |
///
/// The two sides differ on the first two because the outer cascade's filter is
/// several times wider — the same shadow, spread — which is the switch this
/// reading is about and not a fault. At the grazing sun they differ the other
/// way: a shadow several times longer puts a different stretch of itself in the
/// same window of distance from the eye, and the far shells land deeper in it.
/// Floored at half the thinnest of them, because what it bounds is a shadow
/// being present at all rather than how dark it is.
const CASCADE_SHADOWED_LEVELS: f32 = 14.0;

/// How much steeper than the steepest step the same walk shows clear of the band
/// its step across the split may be.
///
/// **A ratio and not a level**, because the walks are at different offsets from
/// their own shadow's edge and darken the pavement by very different amounts:
/// the question is whether the split is a discontinuity *in the profile it sits
/// in*, and a level would answer it differently for every offset.
///
/// **Swept, not guessed**, at [`CLAIM_EXTENT`] on both Vulkan adapters this
/// workspace runs locally, against the same frames drawn with
/// `CASCADE_FADE_FRACTION` in `shaders/mesh.slang` set to zero — the band
/// collapsed to an edge. The shipped arm, per walk:
///
/// | walk | radv | radv, no band | lavapipe | lavapipe, no band |
/// | --- | --- | --- | --- | --- |
/// | column 4 at `-0.26` m | `1.03` | `2.61` | `1.09` | `2.49` |
/// | column 4 at `-0.22` m | `1.57` | `8.48` | `2.01` | `7.51` |
/// | column 4 at `-0.18` m | `0.95` | `14.10` | `0.97` | `12.45` |
///
/// Five, which is the midpoint of the two worst readings — `2.01` with the band
/// and `12.45` without it — on a log scale, and so is two and a half times clear
/// of each. The outermost walk moves least because it stands furthest into the
/// lit gap between two shadows, where even the outer cascade's filter reaches
/// only part way.
///
/// **The same sweep on the other arms**, as the worst ratio each shows — the
/// `ratio` the run prints, which is the steepest step across the split over the
/// steepest the same walk has clear of the band:
///
/// | arm | radv | radv, no band | lavapipe | lavapipe, no band |
/// | --- | --- | --- | --- | --- |
/// | the `disc` rung | `2.08` | `9.76` | `2.68` | `9.24` |
/// | the grazing sun | `0.49` | `35.46` | `0.63` | `31.06` |
/// | the `box` rung | `13.17` | `10.20` | `38.50` | `9.13` |
/// | the pavement pose | `0.96` | `14.11` | `1.32` | `18.67` |
///
/// The first two and the fourth sit either side of five the way the shipped arm
/// does, and are the arms the claim is read on. The third does not, which is
/// what [`CASCADE_UNSEPARATED_RUNG`] is.
const CASCADE_STEP_OVER_NEIGHBOURS: f32 = 5.0;

/// The rung of the filter ladder the claim below is **not** read on.
///
/// `box`, and the reason is the denominator rather than the frame. Its walk is
/// flatter clear of the band than any other arm's: the steepest step column 4's
/// walk at `-0.18` m shows there is `0.12`/255 on radv and `0.04` on lavapipe,
/// against `2.18` and `1.96` for the shipped rung on the same walk. A ratio
/// taken against a denominator that small is a reading of the pavement's own
/// noise, and it comes out **higher with the band than without it** — the `box`
/// row of [`CASCADE_STEP_OVER_NEIGHBOURS`]' second table — so no bound on this
/// ratio separates the band from the edge here, and the bound is left where the
/// arms it does separate put it rather than loosened until this one fits.
///
/// **This is a rung the reading cannot measure and not a rung with a step in
/// it**: its step across the split is `1.55`/255 on radv against the shipped
/// rung's `2.24` on the same frames, so what changed between the two arms is
/// what the ratio is divided by. Reading `box` across the split wants a quantity
/// whose denominator does not collapse on a flat profile —
/// `docs/backlog.md` carries it.
///
/// Held against the ladder the engine declares rather than trusted: a rung
/// renamed out from under this fails the run, where an exclusion that silently
/// excluded nothing would leave the ladder short by one and say so nowhere.
const CASCADE_UNSEPARATED_RUNG: &str = "box";

/// What one arm's walks read across the split.
///
/// The pooled steps rather than the frames they came off: every step is taken
/// **inside** one walk and never between two, so what this carries is per walk
/// and is only ever pooled across walks as steps.
struct Crossing {
    /// Which paths drew this arm's two frames.
    paths: String,
    /// Every walk with a pair of shells either side of the split **and** a pair
    /// clear of the band: its column, its offset off that column's shadow axis,
    /// the steepest step across the split, and the steepest clear of the band.
    compared: Vec<(usize, f32, f32, f32)>,
    /// Every shell mean that landed inside cascade 0, and every one that landed
    /// outside the band, in that order.
    levels: [Vec<f32>; 2],
    /// Where cascade 0 ends for this arm's pose and sun, in metres from the eye.
    reach: f32,
}

/// Draws one arm with the shadow passes on and off and walks the colonnade's
/// shadow across that arm's own cascade split.
///
/// The pose and the sun come off the [`Arm`] rather than from the caller, so an
/// arm at another tick or from the other pose is read against **its** split and
/// **its** shadow direction — a cascade split is a function of the camera, and
/// the colonnade's strips run along the sun.
fn crossing(extent: (u32, u32), name: &str, arm: Arm) -> Crossing {
    let camera = arm.camera();
    let sky = arm.sky();
    let reach = plaza::cascade_split(&camera, sky);
    let band = reach * crcbl::shaders::mesh::CASCADE_FADE_FRACTION;

    // The colonnade's shadows are parallel strips, so an offset past half their
    // lateral spacing is a reading of the next column's shadow and not of this
    // one's edge. Read per arm, because the strips stand apart along the sun's
    // own perpendicular and this sample's sun moves.
    let (_, perp) = shadow_axes(sky);
    let spacing = (plaza::column_foot(0) - plaza::column_foot(1))
        .dot(perp)
        .abs();
    let outermost = CASCADE_LATERALS
        .iter()
        .fold(0.0f32, |widest, lateral| widest.max(lateral.abs()));
    assert!(
        outermost < spacing / 2.0,
        "on {name} the walks read {outermost:.3} m off a shadow's axis and the colonnade's \
         shadows stand {spacing:.3} m apart across it, so the outermost offset is inside the next \
         column's shadow rather than beside this one's"
    );

    let (shadowed, paths, _) = draw(extent, arm);
    let (flat, _, _) = draw(extent, arm.without_shadows());
    assert!(
        shadowed.pixels() != flat.pixels(),
        "on {name} the shadow passes drew the same frame off as on, so every darkening read off \
         it is zero by construction"
    );

    let middles = cascade_shells(reach, band);
    let walks = cascade_walks(&shadowed, &flat, &camera, extent, sky, reach, band);
    eprintln!(
        "sundial golden: the cascade walk on {name}, {paths} — split {reach:.3} m, band \
         {band:.3} m, {columns} columns at {laterals} offsets, {shells} shells from {near:.3} to \
         {far:.3} m",
        columns = plaza::COLONNADE_COUNT,
        laterals = CASCADE_LATERALS.len(),
        shells = middles.len(),
        near = middles[0],
        far = middles[middles.len() - 1],
    );

    // Every neighbouring pair of shells one walk has both of. A pair whose two
    // shells sit either side of the split is the one a switch with no band puts
    // its whole change into; a pair clear of the band altogether is what the
    // same walk shows where no switch is anywhere near it.
    let mean_of = |shell: &(f32, u32)| {
        (shell.1 >= CASCADE_SHELL_SAMPLES).then(|| {
            #[expect(clippy::cast_precision_loss, reason = "a shell holds a few hundred")]
            {
                shell.0 / shell.1 as f32
            }
        })
    };
    let (mut inside, mut outside, mut compared) = (Vec::new(), Vec::new(), Vec::new());
    for walk in &walks {
        for (index, middle) in middles.iter().enumerate() {
            if let Some(level) = mean_of(&walk.shells[index]) {
                if *middle < reach - band {
                    inside.push(level);
                } else if *middle > reach {
                    outside.push(level);
                }
            }
        }
        let (mut straddling, mut clear) = (f32::MIN, f32::MIN);
        for index in 0..middles.len() - 1 {
            let (Some(low), Some(high)) = (
                mean_of(&walk.shells[index]),
                mean_of(&walk.shells[index + 1]),
            ) else {
                continue;
            };
            let (near, far) = (middles[index], middles[index + 1]);
            let step = (high - low).abs();
            if near < reach && far >= reach {
                straddling = straddling.max(step);
            } else if far <= reach - band || near >= reach {
                clear = clear.max(step);
            }
        }
        if straddling > f32::MIN && clear > f32::MIN {
            compared.push((walk.column, walk.lateral, straddling, clear));
        }
    }
    for (column, lateral, straddling, clear) in &compared {
        eprintln!(
            "sundial golden:   {name}: column {column} at {lateral:+.2} m steps {straddling:.2}\
             /255 across the split and at most {clear:.2} clear of the band"
        );
    }
    Crossing {
        paths,
        compared,
        levels: [inside, outside],
        reach,
    }
}

/// **The colonnade's shadow crosses the cascade split without a step in it.**
///
/// `docs/plan/sample/18-sundial.md`'s milestone 3, and
/// `docs/plan/45-shadows.md`'s eighth decision from this sample's side: where two
/// cascades meet, both are sampled and the answers are mixed by distance, so the
/// switch is a **band** and not an edge. `crates/crcbl/tests/forward_e2e/
/// shadow.rs` holds the cascade *overlay* to that band — the two tints blend
/// across it — and what is added here is the thing the overlay is a picture of:
/// the shadow itself, on the fixture the colonnade was laid out for.
///
/// # What is read
///
/// Every column of the colonnade's shadow, walked from inside cascade 0 out past
/// the split, at [`CASCADE_LATERALS`]' offsets either side of its own edge, and
/// binned into shells of **distance from the eye** — the quantity a cascade is
/// selected by, so a shell is a set of pavement the switch treats alike. What
/// each shell holds is the **shadow term**: the pixel with the shadow passes
/// off, less the same pixel with them on, so the pavement's own falloff cancels
/// and what is left is what the sun's shadow map did there.
///
/// A cascade switch changes everything about that answer — the map, the texel
/// footprint both biases and the filter are denominated in, and the filter's
/// width with it — so the profile *does* change across the split, and is meant
/// to. The claim is about **how**: the steepest step between two neighbouring
/// shells that touches the band is held to the steepest step between two
/// neighbouring shells that does not, which is what the same walks show with no
/// switch anywhere near them.
///
/// # The arms
///
/// One reading each — the whole colonnade, walked at every offset — and each
/// drawn twice, passes on and off, against **its own** split: the pose picks
/// where cascade 0 ends and the sun picks the direction the strips run in, so an
/// arm at another tick or from another pose is read against its own and not the
/// fixture's.
///
/// * **The shipped rung**, at [`sun::FIXTURE_TICK`] from [`plaza::fixed_camera`],
///   which is the arm the constants above were swept on.
/// * **Every other rung the engine declares**, out of
///   `filter::names(filter::FILTER)` rather than written down here, less
///   [`CASCADE_UNSEPARATED_RUNG`] — today that is `disc`. A filter's width is
///   what differs between the two cascades at a vertical caster's shadow, so a
///   band held for one rung is not a band held for the ladder.
/// * **The grazing sun**, [`sun::GRAZING_TICK`], where the shadows are several
///   times longer and the stretch of shadow that lands in the same window of
///   distance is a different one — the walks that come back are on the *other*
///   side of the shadow's axis from the fixture arm's.
/// * **[`plaza::pavement_camera`]** (2026-09-05), at [`sun::FIXTURE_TICK`] on
///   the shipped rung, which is the arm that reads more than one column. From
///   the fixture pose [`plaza::hidden_from`] refuses about half of every sample
///   that lands in the shell window — the colonnade stands in front of the
///   pavement its own shadows fall on — and the walks that keep a pair of shells
///   either side of the split are one column's. From a pose set across the row
///   rather than along it, it refuses under a quarter and twelve walks over
///   three columns read across the split.
///
/// # What it is not read on
///
/// Two arms were measured and are not here, and neither is a bound that was
/// loosened to fit them.
///
/// [`CASCADE_UNSEPARATED_RUNG`] carries the first: on `box` the ratio reads
/// higher with the band than with it collapsed, so no bound on it separates the
/// two.
///
/// The second is [`plaza::counter_camera`], and the pavement arm above is what
/// closed it (2026-09-05). Every sample of every walk that lands in the shell
/// window is **outside that pose's frame** — the colonnade stands across the
/// plaza from the counters and the window is a shell of distance around the eye,
/// so the two do not meet on screen. Not one sample is refused by
/// [`plaza::hidden_from`] that the frame had not refused already, and no walk is
/// left with a pair of shells either side of the split. That is a refusal rather
/// than a pass: an arm with no such pair fails the run below, so putting the
/// counter pose in the list would red the suite rather than widen the claim.
/// Framing the colonnade from a second pose wanted a pose and not another arm,
/// and [`plaza::pavement_camera`] is it.
///
/// # Anti-vacuity
///
/// Five ways this could pass while measuring nothing. The walks could be off the
/// shadow, where every shell reads zero and every step with it —
/// [`CASCADE_SHADOWED_LEVELS`] is read off both sides of every arm. The two
/// frames could be one frame, where every darkening is zero by construction —
/// they are compared as bytes, per arm. The control could be zero, where the
/// bound is a bound against nothing — it is asserted positive. No pair could
/// straddle the split at all, where the reading is about two stretches of one
/// cascade — the pair that does is asserted to exist on every arm, which is what
/// refuses the counter pose above rather than passing it. And
/// [`CASCADE_UNSEPARATED_RUNG`] could name a rung the engine no longer declares,
/// where the exclusion silently excludes nothing — it is held against the
/// ladder.
///
/// # How it was shown to fail
///
/// **By collapsing the band to an edge** — `CASCADE_FADE_FRACTION` in
/// `shaders/mesh.slang` set to zero and every artifact regenerated — which is
/// the artefact this exists for and the thing `docs/plan/45-shadows.md`'s eighth
/// decision removed. **Every arm went red, on both adapters**, and the run
/// reports all three together because the arms are read into one list of faults
/// rather than one assertion each. On radv:
///
/// > on the shipped pcss column 4's shadow at -0.18 m off its axis steps
/// > 17.49/255 between the two shells either side of the split at 6.100 m,
/// > against 1.24 for the steepest pair of shells the same walk has clear of the
/// > band — past the 5x this holds it to. The cascade switch is an edge in the
/// > picture rather than the band `CASCADE_FADE_FRACTION` makes of it
/// >
/// > on the disc rung column 4's shadow at -0.18 m off its axis steps 39.24/255
/// > … against 4.02 …
/// >
/// > on the grazing sun column 4's shadow at +0.26 m off its axis steps 3.32/255
/// > … against 0.09 …
///
/// and on lavapipe the same three walks read `17.55` against `1.41`, `38.94`
/// against `4.22`, and `3.33` against `0.11`.
///
/// [`CASCADE_SHELL_SAMPLES`]' own doc carries the second run: at eight samples
/// a shell the near lamp's reach had all but emptied read a `13.88`/255 step
/// across the split, *and the same `13.88` with the band collapsed*, which is
/// how a step that was never the cascade's was told from one that is.
///
/// **The pavement arm's own control** (2026-09-05, run after the arm landed):
/// with the band it reads `0.90`/255 across the split against `0.94` clear of
/// it on radv and `1.03` against `0.78` on lavapipe; with the band collapsed the
/// same walk — column 3 at `-0.22` m — steps `13.23` against `0.94` on radv and
/// `13.25` against `0.71` on lavapipe. So the bound separates a band from an edge
/// at this pose the way it does on the fixture arms, and neither term is noise,
/// which is what [`CASCADE_UNSEPARATED_RUNG`] is about: `box`'s own denominator
/// on this walk is `0.12` and `0.04`.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sundial-golden.sh"]
fn the_colonnades_shadow_crosses_the_cascade_split_without_a_step() {
    let extent = CLAIM_EXTENT;
    let shipped = crcbl::render::shadow::shipped_filter().label();
    let ladder = filter::names(filter::FILTER);
    assert!(
        ladder.contains(&CASCADE_UNSEPARATED_RUNG),
        "the engine declares {ladder:?} and none of them is `{CASCADE_UNSEPARATED_RUNG}`, so the \
         rung this walk holds itself back from is a rung nothing runs and the exclusion excludes \
         nothing"
    );
    let mut arms = vec![(format!("the shipped {shipped}"), Arm::shipped())];
    for name in ladder {
        if *name != shipped && *name != CASCADE_UNSEPARATED_RUNG {
            arms.push((format!("the {name} rung"), Arm::shipped().on(name)));
        }
    }
    assert!(
        arms.len() > 1,
        "the engine declares {ladder:?}, so there is no second rung to read this band on and the \
         claim is a claim about one filter"
    );
    arms.push((
        "the grazing sun".to_string(),
        Arm::shipped().at_tick(sun::GRAZING_TICK),
    ));
    arms.push((
        "the pavement pose".to_string(),
        Arm::shipped().framed_on_the_pavement(),
    ));

    let mut faults = Vec::new();
    for (name, arm) in arms {
        let Crossing {
            paths,
            compared,
            levels: [inside, outside],
            reach,
        } = crossing(extent, &name, arm);

        // Both sides of the split read a shadow, which is what stops the bound
        // below holding on a frame that has none.
        for (side, read) in [("cascade 0", &inside), ("cascade 1", &outside)] {
            if read.is_empty() {
                faults.push(format!("on {name} no shell of any walk landed in {side}"));
                continue;
            }
            #[expect(clippy::cast_precision_loss, reason = "a few hundred shells")]
            let darkening = read.iter().sum::<f32>() / read.len() as f32;
            eprintln!(
                "sundial golden: {name}: the walks darken {side} by {darkening:.2}/255 over {} \
                 shells",
                read.len(),
            );
            if darkening <= CASCADE_SHADOWED_LEVELS {
                faults.push(format!(
                    "on {name} the walks darken {side} by {darkening:.2}/255, under the \
                     {CASCADE_SHADOWED_LEVELS} shadowed pavement stands at. This is a reading of \
                     the open plaza, and open plaza has no step in it however the cascades are \
                     switched"
                ));
            }
        }

        if compared.is_empty() {
            faults.push(format!(
                "on {name} no walk has both a pair of shells either side of the split and a pair \
                 clear of the band — so nothing here reads across the switch against what the \
                 same stretch of shadow shows without one"
            ));
            continue;
        }
        let worst = compared
            .iter()
            .copied()
            .fold((0, 0.0, f32::MIN, 1.0), |worst, walk| {
                if walk.2 * worst.3 > worst.2 * walk.3 {
                    walk
                } else {
                    worst
                }
            });
        let (column, lateral, straddling, clear) = worst;
        eprintln!(
            "sundial golden: the cascade split on {name}, {paths} — {n} walks read across it; \
             the steepest against its own is column {column} at {lateral:+.2} m, \
             {straddling:.2}/255 across the split against {clear:.2} clear of the band, ratio \
             {ratio:.2}",
            n = compared.len(),
            ratio = straddling / clear,
        );
        if clear <= 0.0 {
            faults.push(format!(
                "on {name} the walk this is worst on shows no step at all clear of the band, so \
                 the bound below is a bound against nothing"
            ));
            continue;
        }
        if straddling >= CASCADE_STEP_OVER_NEIGHBOURS * clear {
            faults.push(format!(
                "on {name} column {column}'s shadow at {lateral:+.2} m off its axis steps \
                 {straddling:.2}/255 between the two shells either side of the split at \
                 {reach:.3} m, against {clear:.2} for the steepest pair of shells the same walk \
                 has clear of the band — past the {CASCADE_STEP_OVER_NEIGHBOURS}x this holds it \
                 to. The cascade switch is an edge in the picture rather than the band \
                 `CASCADE_FADE_FRACTION` makes of it"
            ));
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}
