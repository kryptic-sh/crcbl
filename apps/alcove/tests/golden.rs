//! The court off a real device, from the fixed camera, against checked-in
//! goldens — and the claims about the occlusion in front of them.
//!
//! # A golden alone cannot make a claim about occlusion
//!
//! A wrong grey image is a plausible grey image. An occlusion pass that never
//! ran leaves a white channel; one whose intensity is stuck at zero leaves the
//! same; a technique selector wired to one pipeline draws two identical halves
//! either side of a seam; and every one of those produces a frame somebody would
//! bless. So the goldens are the *last* of the assertions here, and the ones
//! before them are about **where** the frame is dark and by how much, in the
//! shape `apps/lantern/tests/golden.rs` uses.
//!
//! Each of them is a ratio between two readings rather than an absolute value,
//! because an absolute one is a second golden written in numbers: it moves when
//! the tonemap moves, and it says nothing a reviewer can act on.
//!
//! # Feature-gated *and* ignored
//!
//! The pair `crcbl`'s `render-e2e` uses. A plain `cargo test --workspace
//! --all-features` on a machine with no GPU must stay green, and
//! `tests/run-alcove-golden.sh` is the only thing that turns both off — and it
//! fails when the suite reports zero tests run.

#![cfg(feature = "golden-e2e")]

use crcbl::hal::{AdapterInfo, Format};
use crcbl::math::Vec3;
use crcbl::render::{Camera, EffectOverride, EffectRequest, ForwardRenderer, RenderEffects};
use crcbl::screenshot::{ForwardScene, OffscreenSetup};
use crcbl_alcove::{court, occlusion};
use crcbl_golden::{ChannelOrder, Golden, Image};

/// The extent the checked-in goldens are blessed at.
const EXTENT: (u32, u32) = (256, 192);

/// The extent the review frames are written at.
const REVIEW_EXTENT: (u32, u32) = (1280, 960);

/// Where a review-size frame is written, relative to the workspace root.
const REVIEW_DIR: &str = "target/alcove";

/// Half-extents, in pixels, of the block each claim averages over at [`EXTENT`].
const BLOCK: (u32, u32) = (2, 2);

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// One arm of a comparison: which effects, which gather, which seam, and which
/// picture.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Arm {
    /// Which of the render effects this arm draws.
    effects: RenderEffects,
    /// Which gather the near side runs, or `None` for what ships.
    technique: Option<&'static str>,
    /// Where the comparison seam stands, or `None` for a frame comparing
    /// nothing.
    split: Option<f32>,
    /// Whether the frame draws the occlusion channel instead of shading it.
    occlusion_view: bool,
    /// Whether the frame draws the channel's **bent direction** instead of
    /// shading it.
    ///
    /// A field of its own beside [`occlusion_view`](Self::occlusion_view) rather
    /// than one enum, because [`ForwardRenderer`] is what this arm drives and it
    /// carries a switch per view: the precedence between them is
    /// `ForwardRenderer::debug_view`'s and not this fixture's to restate.
    bent_normal_view: bool,
    /// The occlusion radius, or `None` for the shipped one.
    radius: Option<f32>,
    /// Whether the sun is switched off, leaving the ambient term alone.
    sunless: bool,
    /// Whether the frame is taken from [`court::rim_camera`] rather than
    /// [`court::fixed_camera`].
    rim: bool,
}

impl Arm {
    /// **The court as the sample ships it**: the default effect stack, the
    /// shipped gather, no seam, shaded.
    ///
    /// [`RenderEffects::DEFAULT_STACK`] rather than [`RenderEffects::all`], and
    /// the crease claim is why: `all()` turns on auto exposure and bloom, and
    /// both of them make a reading depend on the rest of the frame. An arm that
    /// switches the occlusion pass off is a slightly brighter frame, so auto
    /// exposure answers by lowering the exposure, and the difference the claim
    /// measures at one point stops being about that point.
    const fn shipped() -> Self {
        Self {
            effects: RenderEffects::DEFAULT_STACK,
            technique: None,
            split: None,
            occlusion_view: false,
            bent_normal_view: false,
            radius: None,
            sunless: false,
            rim: false,
        }
    }

    /// The same arm at a named occlusion radius.
    const fn with_radius(self, radius: f32) -> Self {
        Self {
            radius: Some(radius),
            ..self
        }
    }

    /// The same arm with the occlusion pass out — every claim's control.
    const fn without_occlusion(self) -> Self {
        Self {
            effects: RenderEffects::DEFAULT_STACK.difference(RenderEffects::AMBIENT_OCCLUSION),
            ..self
        }
    }

    /// The same arm drawing the occlusion channel as grey.
    const fn as_channel(self) -> Self {
        Self {
            occlusion_view: true,
            ..self
        }
    }

    /// The same arm drawing the channel's bent direction — what `N` and the
    /// pause panel's `BENT VIEW` row put up.
    const fn as_bent_direction(self) -> Self {
        Self {
            bent_normal_view: true,
            ..self
        }
    }

    /// The same arm on a named gather.
    const fn on(self, technique: &'static str) -> Self {
        Self {
            technique: Some(technique),
            ..self
        }
    }

    /// **The same arm with the sun switched off**, so the ambient term is the
    /// whole of what lights the court.
    ///
    /// The control half of the crease claim. Occlusion is defined to scale the
    /// ambient term and to leave direct light alone, and the way to see that is
    /// to subtract the direct light rather than to reason about it: whatever the
    /// occlusion pass takes off a directly lit surface, it must take exactly the
    /// same amount off when the sun is not there at all.
    const fn sunless(self) -> Self {
        Self {
            sunless: true,
            ..self
        }
    }

    /// The same arm framed by [`court::rim_camera`].
    const fn framed_on_the_rim(self) -> Self {
        Self { rim: true, ..self }
    }

    /// Which camera this arm is drawn from.
    fn camera(self) -> Camera {
        if self.rim {
            court::rim_camera()
        } else {
            court::fixed_camera()
        }
    }

    /// The same arm with the comparison seam at `at`.
    const fn split_at(self, at: f32) -> Self {
        Self {
            split: Some(at),
            ..self
        }
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
    occlusion::reset();
    if let Some(technique) = arm.technique {
        occlusion::var(occlusion::TECHNIQUE)
            .set(&crcbl::console::Value::Enum(technique))
            .expect("the engine declares that technique");
    }
    if let Some(radius) = arm.radius {
        occlusion::var(occlusion::RADIUS)
            .set(&crcbl::console::Value::Float(radius))
            .expect("the radius is inside its own range");
    }
    if let Some(at) = arm.split {
        occlusion::var(occlusion::SPLIT)
            .set(&crcbl::console::Value::Float(at))
            .expect("the seam is inside its own range");
    }

    let mut setup = OffscreenSetup::open_forward_with(
        extent.0,
        extent.1,
        OffscreenSetup::OPTIONAL_FEATURES,
        |device, queue, format| {
            Ok(ForwardScene {
                camera: arm.camera(),
                sun: if arm.sunless {
                    crcbl::render::DirectionalLight {
                        color: Vec3::ZERO,
                        ..court::sun()
                    }
                } else {
                    court::sun()
                },
                renderer: Box::new(build(device, queue, format, arm)?),
            })
        },
    )
    .unwrap_or_else(|why| panic!("a GPU backend opens for alcove's court: {why}"));

    let backend = setup.backend();
    let caps = setup.caps();
    let adapter = setup.adapter().clone();
    // Printed unconditionally and read with `--success-output immediate`: on a
    // green run — the run where the selected path is worth knowing — nextest
    // captures this and it is otherwise invisible.
    eprintln!(
        "alcove golden: device on adapter {id} {name:?} type={kind:?}",
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
        "alcove golden: {paths} at {}x{}, arm {arm:?}",
        extent.0, extent.1,
    );

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    // Before any assertion: `finish` waits the device idle, and a device lost
    // during the frame surfaces there and nowhere else.
    setup.finish().expect("the device reaches idle");
    occlusion::reset();

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

/// The court, made resident and placed, on a device the caller opened.
fn build(
    device: &dyn crcbl::hal::Device,
    queue: crcbl::hal::QueueHandle,
    format: Format,
    arm: Arm,
) -> Result<ForwardRenderer, crcbl::screenshot::OffscreenError> {
    let scene = court::court();
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    // The **programmatic** layer of topic 39's resolution order, which is the
    // one a test has any business driving.
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::all().difference(arm.effects), Some(false)),
        ..EffectRequest::default()
    });
    renderer.set_occlusion_view(arm.occlusion_view);
    renderer.set_bent_normal_view(arm.bent_normal_view);
    if let Err(error) = court::place(&mut renderer) {
        renderer.destroy(device);
        return Err(crcbl::screenshot::OffscreenError::Hal(
            crcbl::hal::HalError::InvalidDescriptor(format!(
                "alcove's court does not fit its own instance pool: {error}"
            )),
        ));
    }
    // The court has one light and it does not move, so the list is empty and the
    // sun is the whole of what lights it.
    renderer.set_lights(&[]);
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

/// Mean **linear** luminance of a block of pixels, in `0.0..=1.0`.
///
/// The unit the crease claim is stated in, and the reason it is not stated in
/// the 0–255 one above: the frame is stored sRGB-encoded, so a fixed change in
/// radiance is a large number of codes down near the ambient term and a small
/// number of codes up near a sunlit surface. Comparing the two in codes compares
/// the transfer function.
///
/// The decode is the sRGB EOTF as IEC 61966-2-1 states it, per channel and
/// before the average — averaging codes and decoding once is a different number.
fn linear_brightness(image: &Image, centre: (u32, u32), half: (u32, u32)) -> f32 {
    fn eotf(code: u8) -> f32 {
        let c = f32::from(code) / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let (mut total, mut count) = (0.0f32, 0u32);
    let x0 = centre.0.saturating_sub(half.0);
    let y0 = centre.1.saturating_sub(half.1);
    let x1 = (centre.0 + half.0).min(image.width().saturating_sub(1));
    let y1 = (centre.1 + half.1).min(image.height().saturating_sub(1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let pixel = image.pixel(x, y).expect("inside the frame");
            total += (eotf(pixel[0]) + eotf(pixel[1]) + eotf(pixel[2])) / 3.0;
            count += 1;
        }
    }
    assert!(count > 0, "an empty block at {centre:?} measures nothing");
    #[allow(clippy::cast_precision_loss)]
    {
        total / count as f32
    }
}

/// Mean of each colour channel over a block of pixels, out of 255.
///
/// The unit the bent-direction claims are stated in, and the reason
/// [`brightness`] cannot state them: the bent view draws a **direction** as
/// `n * 0.5 + 0.5`, so `+Y` and `+X` have the same luminance and differ only in
/// which channel carries it. A mean over the three would call them the same
/// picture.
fn channels(image: &Image, centre: (u32, u32), half: (u32, u32)) -> [f32; 3] {
    let (mut total, mut count) = ([0.0f32; 3], 0u32);
    let x0 = centre.0.saturating_sub(half.0);
    let y0 = centre.1.saturating_sub(half.1);
    let x1 = (centre.0 + half.0).min(image.width().saturating_sub(1));
    let y1 = (centre.1 + half.1).min(image.height().saturating_sub(1));
    for y in y0..=y1 {
        for x in x0..=x1 {
            let pixel = image.pixel(x, y).expect("inside the frame");
            for (channel, sum) in total.iter_mut().enumerate() {
                *sum += f32::from(pixel[channel]);
            }
            count += 1;
        }
    }
    assert!(count > 0, "an empty block at {centre:?} measures nothing");
    #[allow(clippy::cast_precision_loss)]
    total.map(|sum| sum / count as f32)
}

/// `value` in linear light, as the swapchain's sRGB encode writes it, out of
/// 255.
///
/// IEC 61966-2-1's transfer function, which the Vulkan specification's sRGB
/// conversion is. It is here because the bent-direction claim compares a colour
/// **derived from the court's own geometry** against a readback byte, and the
/// value `mesh.slang` returns is not the value that lands in the buffer: the
/// bent view writes the encoded direction straight into the `Rgba16Float` scene
/// target, the tonemap resolves that as the identity on `[0, 1]` at the default
/// exposure, and the swapchain encodes on the way out. Every other claim in this
/// file compares two readbacks with each other and needed no such thing.
///
/// A transcription of `crcbl`'s own `forward_e2e::depth_probe::srgb_encode`,
/// which is `pub(crate)` to one test binary and cannot be reached from another.
fn srgb_encode(value: f32) -> f32 {
    let encoded = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    encoded * 255.0
}

/// [`BLOCK`] scaled to `extent`.
fn block_for(extent: (u32, u32)) -> (u32, u32) {
    let scale = (extent.0 / EXTENT.0).max(1);
    (BLOCK.0 * scale, BLOCK.1 * scale)
}

/// Writes a frame where a reviewer can open it, and hands it back.
fn save(image: &Image, name: &str, extent: (u32, u32)) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(REVIEW_DIR);
    std::fs::create_dir_all(&dir).expect("target/ is writable");
    let path = dir.join(format!("{name}-{}x{}.png", extent.0, extent.1));
    image.save_png(&path).expect("the review frame is writable");
    eprintln!("alcove golden: {name} frame at {}", path.display());
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

/// The darkest pixel anywhere in the frame, out of 255.
fn darkest(image: &Image) -> f32 {
    let mut lowest = 255.0f32;
    for y in 0..image.height() {
        for x in 0..image.width() {
            let pixel = image.pixel(x, y).expect("inside the frame");
            let mean = (f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2])) / 3.0;
            lowest = lowest.min(mean);
        }
    }
    lowest
}

// ---------------------------------------------------------------------------
// What the occlusion pass is allowed to touch
// ---------------------------------------------------------------------------

/// How far apart the two absolute drops in
/// [`occlusion_scales_the_ambient_term_and_leaves_direct_light_alone`] may be,
/// as a fraction of the larger.
///
/// **Measured, and quantisation is what sets it.** On lavapipe at [`EXTENT`] the
/// sunlit drop is a little over one 8-bit code at a code value where one code is
/// worth about 0.0075 in linear light, and the whole signal being measured is
/// about that size — so the two readings cannot agree more closely than the
/// encoding resolves. The recorded pair is in the test's own doc.
///
/// It is wide and it still has teeth: occlusion applied to the direct term as
/// well would make the sunlit drop about four times the sunless one, because the
/// crease's sunlit radiance is about four times its ambient radiance. The
/// red-check that does exactly that is in the same doc.
const CREASE_AGREEMENT: f32 = 0.45;

/// How much brighter the crease must be with the sun than without it.
///
/// The anti-vacuity half: "the drop is the same with and without the sun" is
/// trivially true at a point the sun never reached, and a sun that missed the
/// crease — a shadow the slot walls cast, a sun azimuth that drifted off
/// [`court::slot_axis`] — is exactly the failure this fixture exists to catch.
const CREASE_SUNLIT_FACTOR: f32 = 2.5;

/// **The occlusion term multiplies the ambient light and leaves direct light
/// alone.**
///
/// `docs/plan/sample/19-alcove.md`'s first acceptance claim, and the one the
/// court's geometry was laid out for: [`court::crease_lit`] is floor at the
/// bottom of a slot narrow enough to be almost closed to the sky, and in **full
/// sun**, because the sun's azimuth and the slot's axis are one line.
///
/// Four frames, because the claim is a difference of differences. Switching the
/// pass off gives the drop the occlusion term is responsible for; switching the
/// sun off as well gives the same drop with the direct term subtracted out.
/// Occlusion that scales only the ambient term takes the same **absolute**
/// radiance off both — while the **relative** drop differs greatly, which is why
/// the tempting version of this test, a ratio, says nothing here.
///
/// # What was measured
///
/// At [`EXTENT`] the occlusion pass takes 0.00779 of linear radiance off the
/// sunlit crease and 0.00628 off the same point with the sun removed on lavapipe
/// (llvmpipe, LLVM 22.1.8) — a disagreement of 0.194 — and 0.00768 against
/// 0.00642 on radv (AMD Radeon RX 7900 XTX, RADV NAVI31), a disagreement of
/// 0.165. The widest seen at either extent is 0.265, at [`REVIEW_EXTENT`] on
/// lavapipe. The run prints every one of them, so a reader does not have to
/// trust this paragraph.
///
/// # How it was shown to fail
///
/// By drawing the sunless pair with the sun on: that is what "the occlusion term
/// scales direct light as well" looks like from outside the shader, because the
/// drop the claim compares against is then taken off the whole radiance instead
/// of off the ambient part of it. The drop with the sun went to 0.02566 against
/// the same 0.00628 without it — a disagreement of 0.755, past the bound.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-alcove-golden.sh"]
fn occlusion_scales_the_ambient_term_and_leaves_direct_light_alone() {
    let block = block_for(EXTENT);
    let at = project(&court::fixed_camera(), EXTENT, court::crease_lit());

    let (sun_on, paths, _) = draw(EXTENT, Arm::shipped());
    let (sun_on_flat, _, _) = draw(EXTENT, Arm::shipped().without_occlusion());
    let (sun_off, _, _) = draw(EXTENT, Arm::shipped().sunless());
    let (sun_off_flat, _, _) = draw(EXTENT, Arm::shipped().sunless().without_occlusion());

    let lit = linear_brightness(&sun_on_flat, at, block);
    let ambient = linear_brightness(&sun_off_flat, at, block);
    assert!(
        lit > ambient * CREASE_SUNLIT_FACTOR,
        "the crease reads {lit:.4} in linear light with the sun and {ambient:.4} without it, so \
         the sun barely reaches it and this test is about a shaded floor"
    );

    let with_sun = lit - linear_brightness(&sun_on, at, block);
    let without_sun = ambient - linear_brightness(&sun_off, at, block);
    let disagreement = (with_sun - without_sun).abs() / with_sun.max(without_sun).max(1e-6);
    eprintln!(
        "alcove golden: crease on {paths} — sunlit {lit:.4} ambient {ambient:.4}, occlusion \
         takes {with_sun:.5} with the sun and {without_sun:.5} without it, disagreement \
         {disagreement:.3}"
    );
    assert!(
        without_sun > 0.0,
        "the occlusion pass took nothing off the crease at all, so there is no drop to compare"
    );
    assert!(
        disagreement < CREASE_AGREEMENT,
        "occlusion takes {with_sun:.5} off the sunlit crease and {without_sun:.5} off the same \
         point with the sun removed — a disagreement of {disagreement:.3}, past \
         {CREASE_AGREEMENT}. It scales the ambient term alone, so the two must be the same \
         amount of light"
    );
}

// ---------------------------------------------------------------------------
// Where the occlusion pass darkens the court
// ---------------------------------------------------------------------------

/// The least darkening [`the_court_darkens_where_it_is_enclosed`] accepts at the
/// alcove's back corner, as a fraction of the unoccluded reading.
///
/// Measured at [`EXTENT`]: 0.0529 on lavapipe and 0.0534 on radv, and about
/// 0.097 on both at [`REVIEW_EXTENT`], where the block covers less of the jamb
/// beside it. Set well under the lower of those, because it is a floor on a real
/// effect rather than a second golden written in numbers.
const CORNER_DARKENING: f32 = 0.04;

/// The same, at the contact band beside the box.
///
/// Lower than [`CORNER_DARKENING`], and it should be: a floor beside one box is
/// enclosed by two surfaces where the alcove's corner is enclosed by four.
/// Measured at 0.0349 on both adapters at [`EXTENT`] and 0.039 at
/// [`REVIEW_EXTENT`].
const CONTACT_DARKENING: f32 = 0.015;

/// How far the open floor may move, in 0–255 codes, when the pass is switched
/// off.
///
/// The control. Nothing is within the shipped occlusion radius of
/// [`court::OPEN_FLOOR`], so the occlusion channel there is one and the two
/// frames must agree exactly — a tolerance below one code is what makes this a
/// statement about that point rather than about the frame's average.
const OPEN_FLOOR_TOLERANCE: f32 = 0.5;

/// **The court is darker where it is enclosed, and unchanged where it is not.**
///
/// The charter's contact claim and its alcove claim, together with the control
/// that separates them from a frame that merely got darker. A pass whose
/// intensity ran away, a tonemap that lost a stop, an exposure that moved: each
/// darkens the corner *and* the open floor, and only the third assertion here
/// notices.
///
/// Both measured points carry **no direct light at all** — the alcove's recess
/// faces `+z` and the sun's `z` is negative; the contact band is in the box's own
/// shadow — so what is read at them is the ambient term times the occlusion
/// channel, which is the thing being claimed about.
///
/// # What was measured
///
/// The figures are on [`CORNER_DARKENING`] and [`CONTACT_DARKENING`], and the
/// run prints them again on whatever adapter it opened. The open floor read
/// 229.67/255 with the pass and 229.67 without it, on both adapters — the same
/// number, not a number within a tolerance.
///
/// # How it was shown to fail
///
/// By pointing the two occluded blocks at [`court::OPEN_FLOOR`]: a point with
/// nothing near it darkens by nothing, the darkening came out at 0.0000, and the
/// first assertion failed.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-alcove-golden.sh"]
fn the_court_darkens_where_it_is_enclosed() {
    let block = block_for(EXTENT);
    let camera = court::fixed_camera();
    let (occluded, paths, _) = draw(EXTENT, Arm::shipped());
    let (flat, _, _) = draw(EXTENT, Arm::shipped().without_occlusion());

    for (name, point, least) in [
        (
            "the alcove's back corner",
            court::ALCOVE_CORNER,
            CORNER_DARKENING,
        ),
        ("the contact band", court::CONTACT_BAND, CONTACT_DARKENING),
    ] {
        let at = project(&camera, EXTENT, point);
        let (dark, plain) = (
            brightness(&occluded, at, block),
            brightness(&flat, at, block),
        );
        let darkening = (plain - dark) / plain.max(1e-3);
        eprintln!(
            "alcove golden: {name} on {paths} — {plain:.2} without occlusion, {dark:.2} with it, \
             darkening {darkening:.4}"
        );
        assert!(
            darkening > least,
            "{name} reads {plain:.2}/255 without the occlusion pass and {dark:.2} with it — a \
             darkening of {darkening:.4}, short of {least}. The pass is meant to close that \
             corner up"
        );
    }

    let at = project(&camera, EXTENT, court::OPEN_FLOOR);
    let (dark, plain) = (
        brightness(&occluded, at, block),
        brightness(&flat, at, block),
    );
    eprintln!("alcove golden: the open floor — {plain:.2} without occlusion, {dark:.2} with it");
    assert!(
        (plain - dark).abs() < OPEN_FLOOR_TOLERANCE,
        "the open floor moved from {plain:.2}/255 to {dark:.2} when the occlusion pass was \
         switched off. Nothing is within the occlusion radius of it, so what moved was the \
         whole frame and the darkening above is not about the corner"
    );
}

// ---------------------------------------------------------------------------
// The technique selector
// ---------------------------------------------------------------------------

/// How far apart the two gathers' occlusion channels must be, in mean 0–255
/// codes over the whole frame.
///
/// The anti-vacuity bound for the selector: a `r_ssao_technique` wired to one
/// pipeline draws two identical frames, and a frame is not evidence of a choice
/// unless choosing differently produces a different one. Measured at 6.681 on
/// lavapipe and 6.711 on radv, so this is a sixth of what was seen.
const GATHERS_DIFFER_BY: f32 = 1.0;

/// **The two gathers draw different occlusion, and both of them darken the same
/// corners.**
///
/// Two halves, and each is useless without the other. A selector that quietly
/// runs one pipeline for both names passes any claim about darkening; a second
/// pipeline that returns noise fails no difference test. So: the frames must
/// differ, and each of them on its own must be dark where the court is enclosed
/// and white where it is open.
///
/// Drawn through `set_occlusion_view`, so what is compared is the occlusion
/// channel itself rather than a shaded frame in which a difference of a few per
/// cent of the ambient term is a difference of a fraction of a code.
///
/// # How it was shown to fail
///
/// By naming the shipped gather in both arms. The difference came out at exactly
/// 0.000 and the assertion failed; the per-gather halves still passed, which is
/// the point of having both.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-alcove-golden.sh"]
fn the_two_gathers_differ_and_both_darken_the_court() {
    let block = block_for(EXTENT);
    let camera = court::fixed_camera();
    let names = occlusion::names(occlusion::TECHNIQUE);
    assert!(
        names.len() > 1,
        "the engine declares {names:?}, so there is no selector to make a claim about"
    );

    let mut frames = Vec::new();
    for technique in names {
        let (channel, paths, _) = draw(EXTENT, Arm::shipped().as_channel().on(technique));
        let open = brightness(&channel, project(&camera, EXTENT, court::OPEN_FLOOR), block);
        for (name, point) in [
            ("the alcove's back corner", court::ALCOVE_CORNER),
            ("the contact band", court::CONTACT_BAND),
        ] {
            let here = brightness(&channel, project(&camera, EXTENT, point), block);
            eprintln!(
                "alcove golden: {technique} on {paths} — {name} {here:.2}/255, open floor \
                 {open:.2}"
            );
            assert!(
                here < open * (1.0 - CORNER_DARKENING.min(CONTACT_DARKENING)),
                "the {technique} gather reads {here:.2}/255 at {name} and {open:.2} on open \
                 floor, so it is not darkening the enclosed part of the court"
            );
        }
        frames.push((*technique, channel));
    }

    let mut total = 0.0f32;
    for x in 0..EXTENT.0 {
        total += column_difference(&frames[0].1, &frames[1].1, x);
    }
    #[allow(clippy::cast_precision_loss)]
    let difference = total / EXTENT.0 as f32;
    eprintln!(
        "alcove golden: {} and {} differ by {difference:.3}/255 over the frame",
        frames[0].0, frames[1].0
    );
    assert!(
        difference > GATHERS_DIFFER_BY,
        "the {} and {} occlusion channels differ by {difference:.3}/255 over the whole frame, \
         under {GATHERS_DIFFER_BY}. r_ssao_technique is not reaching a second pipeline",
        frames[0].0,
        frames[1].0
    );
}

// ---------------------------------------------------------------------------
// The comparison seam
// ---------------------------------------------------------------------------

/// How many pixels either side of the seam the column-exact comparison skips.
///
/// **Not slack: the blur's footprint.** `crcbl_render::split` divides the
/// *gather* alone — the blur and the depth-aware upsample that follow it run
/// over the whole target — so for a few texels each way a pixel is a mixture of
/// the two gathers and belongs to neither reference frame. The measured band on
/// lavapipe at [`REVIEW_EXTENT`] runs from nine pixels left of the seam to four
/// right of it, and it is a **texel** count rather than a fraction of the frame,
/// so the same constant holds at every extent.
const SEAM_BLEED: u32 = 12;

/// **The seam runs the console's gather on the left and the shipped one on the
/// right, to the column.**
///
/// The comparison the whole feature exists for, and the end-to-end check
/// `docs/backlog.md` asked for: `crcbl-render`'s own tests show two gather passes
/// with two scissor rectangles recorded, which is not the same as showing that
/// the pixels either side of the line came from different pipelines.
///
/// Three frames — the moved gather everywhere, the shipped gather everywhere,
/// and the seamed one — and then every column of the seamed frame is held
/// against **both**. Outside [`SEAM_BLEED`] the agreement is exact, byte for
/// byte, and the disagreement with the other reference is what stops the whole
/// thing being vacuous: two identical references would satisfy the equality half
/// perfectly.
///
/// # How it was shown to fail
///
/// By swapping the two references, so each half is compared against the gather
/// the other side ran. Column 0 disagreed by 5.651/255 and the assertion failed
/// there.
///
/// # What was measured
///
/// 233 of the 256 columns are compared — the rest are the bleed band — and every
/// one of them is exact on both adapters. The thinnest disagreement with the
/// other gather anywhere outside the band is 2.688/255 on lavapipe and 2.677 on
/// radv, so the equality is not an equality of two identical pictures.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-alcove-golden.sh"]
fn the_seam_runs_the_console_gather_on_the_left_and_the_shipped_one_on_the_right() {
    let shipped = occlusion::shipped_technique();
    let moved = occlusion::names(occlusion::TECHNIQUE)
        .iter()
        .copied()
        .find(|name| *name != shipped)
        .expect("the engine declares a gather other than the shipped one");

    let (whole_moved, paths, _) = draw(EXTENT, Arm::shipped().as_channel().on(moved));
    let (whole_shipped, _, _) = draw(EXTENT, Arm::shipped().as_channel().on(shipped));
    let (seamed, _, _) = draw(
        EXTENT,
        Arm::shipped()
            .as_channel()
            .on(moved)
            .split_at(occlusion::SEAM_CENTRE),
    );

    let seam = EXTENT.0 / 2;
    let mut thinnest = f32::MAX;
    let mut columns = 0u32;
    for x in 0..EXTENT.0 {
        if x.abs_diff(seam) < SEAM_BLEED {
            continue;
        }
        let near_side = x < seam;
        let (mine, theirs) = if near_side {
            (&whole_moved, &whole_shipped)
        } else {
            (&whole_shipped, &whole_moved)
        };
        let side = if near_side { moved } else { shipped };
        let agreement = column_difference(&seamed, mine, x);
        assert!(
            agreement == 0.0,
            "column {x} of the seamed frame differs from the whole-frame {side} run by \
             {agreement:.3}/255. With the seam at {} the {} of the frame is meant to be that \
             gather and nothing else",
            occlusion::SEAM_CENTRE,
            if near_side { "left" } else { "right" },
        );
        thinnest = thinnest.min(column_difference(&seamed, theirs, x));
        columns += 1;
    }
    assert!(
        columns > 0,
        "the bleed band swallowed the whole frame, so nothing was compared"
    );
    eprintln!(
        "alcove golden: the seam on {paths} — {columns} columns exact, thinnest disagreement \
         with the other gather {thinnest:.3}/255"
    );
    assert!(
        thinnest > 0.0,
        "some column outside the bleed band is identical under both gathers, so the seam's two \
         sides cannot be told apart there and the equality above proves nothing"
    );
}

/// The radius the near side of the seam is moved to in
/// [`the_seam_reads_the_console_block_on_the_left`].
///
/// Twice the shipped one, which is a difference the gather cannot express any
/// other way: it is a number in the uniform block rather than a choice of
/// pipeline.
const SEAM_MOVED_RADIUS: f32 = 1.0;

/// How far the far side of a radius-moved seam may sit from the whole-frame run
/// at the shipped radius, in mean 0–255 codes down a column.
///
/// **Measured, and it is not slack.** Moving the technique leaves the far side
/// exact — that is what the test above asserts — while moving the *radius*
/// leaves a residue of a fraction of a code on the far side and none at all on
/// the near one. Why the two sides are not symmetric was not chased down; it is
/// in `docs/backlog.md`. The worst column seen is 0.4479 on lavapipe and 0.4167
/// on radv, and the bound is a loose ceiling over both — the assertion with
/// teeth is the per-column one beside it, which holds every far-side column
/// closer to the shipped block than to the moved one.
const SEAM_BLOCK_RESIDUE: f32 = 1.0;

/// **The seam reads the console's uniform block on the left and the shipped one
/// on the right.**
///
/// The sibling of the test above and not a duplicate of it. `docs/backlog.md`
/// records that `crcbl-render`'s own coverage can see which *pipeline* each
/// march ran — the reference backend records a pipeline as itself — and cannot
/// see which *buffer* it bound, because a bind group is recorded as a handle and
/// a layout. Moving the technique, as the test above does, moves the pipeline.
/// This one holds the technique still and moves [`SEAM_MOVED_RADIUS`], which
/// lives only in the block, so the two sides can differ for exactly one reason.
///
/// The far side is held to a bound rather than to equality, and the near side to
/// equality; [`SEAM_BLOCK_RESIDUE`] says why and what was measured. Neither half
/// is the load-bearing one on its own: every column outside the bleed band is
/// also asserted to be **closer to the block that side is meant to have read
/// than to the other one**, which is what a wrongly bound buffer would break.
///
/// # How it was shown to fail
///
/// By swapping the two references, so each half is compared against the radius
/// the other side ran. Column 0 came out 3.2292/255 from the block the near side
/// was supposed to have read and 0.0000 from the other one, and the assertion
/// failed there.
///
/// # What was measured
///
/// The worst far-side residue is 0.4479/255 on lavapipe and 0.4167 on radv; the
/// thinnest disagreement with the block that side did not read is 0.6406 and
/// 0.6615. The run prints both again on whatever adapter it opened.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-alcove-golden.sh"]
fn the_seam_reads_the_console_block_on_the_left() {
    let (whole_moved, paths, _) = draw(
        EXTENT,
        Arm::shipped().as_channel().with_radius(SEAM_MOVED_RADIUS),
    );
    let (whole_shipped, _, _) = draw(EXTENT, Arm::shipped().as_channel());
    let (seamed, _, _) = draw(
        EXTENT,
        Arm::shipped()
            .as_channel()
            .with_radius(SEAM_MOVED_RADIUS)
            .split_at(occlusion::SEAM_CENTRE),
    );

    let seam = EXTENT.0 / 2;
    let (mut worst, mut thinnest) = (0.0f32, f32::MAX);
    for x in 0..EXTENT.0 {
        if x.abs_diff(seam) < SEAM_BLEED {
            continue;
        }
        let near_side = x < seam;
        let (mine, theirs) = if near_side {
            (
                column_difference(&seamed, &whole_moved, x),
                column_difference(&seamed, &whole_shipped, x),
            )
        } else {
            (
                column_difference(&seamed, &whole_shipped, x),
                column_difference(&seamed, &whole_moved, x),
            )
        };
        let side = if near_side { "near" } else { "far" };
        assert!(
            mine < theirs,
            "column {x} is {mine:.4}/255 from the whole-frame run at the radius the {side} side \
             is meant to be reading and {theirs:.4} from the other one, so it did not read the \
             block that side was given"
        );
        if near_side {
            assert!(
                mine == 0.0,
                "column {x} of the seamed frame differs from the whole-frame run at radius \
                 {SEAM_MOVED_RADIUS} by {mine:.4}/255. The near side reads the console's block \
                 and nothing else"
            );
        } else {
            assert!(
                mine < SEAM_BLOCK_RESIDUE,
                "column {x} of the seamed frame sits {mine:.4}/255 from the whole-frame run at \
                 the shipped radius, past {SEAM_BLOCK_RESIDUE}"
            );
            worst = worst.max(mine);
        }
        thinnest = thinnest.min(theirs);
    }
    eprintln!(
        "alcove golden: the seam's blocks on {paths} — worst far-side residue {worst:.4}/255, \
         thinnest disagreement with the other block {thinnest:.4}"
    );
}

// ---------------------------------------------------------------------------
// The radius knob
// ---------------------------------------------------------------------------

/// **A wider radius closes the alcove's corner up further.**
///
/// `r_ssao_radius` is on the pause panel and on `[` / `]`, and a knob whose value
/// changes while the frame does not is the failure a sample like this exists to
/// catch. Monotone across three radii rather than a single pair, so a knob that
/// happens to be read once at startup fails as well as one that is ignored.
///
/// # How it was shown to fail
///
/// By asking for the shipped radius three times. The second reading came back
/// equal to the first rather than under it, and the assertion failed.
///
/// # What was measured
///
/// The alcove's corner reads 232.20, 211.16 and 184.48 out of 255 at the three
/// radii on lavapipe, and 232.16, 210.80 and 184.40 on radv.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-alcove-golden.sh"]
fn a_wider_radius_deepens_the_occlusion() {
    let block = block_for(EXTENT);
    let at = project(&court::fixed_camera(), EXTENT, court::ALCOVE_CORNER);
    let mut previous = f32::MAX;
    for radius in [0.25f32, 0.5, 1.0] {
        let (channel, paths, _) = draw(EXTENT, Arm::shipped().as_channel().with_radius(radius));
        let here = brightness(&channel, at, block);
        eprintln!(
            "alcove golden: radius {radius:.2} on {paths} — the alcove's corner reads \
             {here:.2}/255"
        );
        assert!(
            here < previous,
            "at radius {radius:.2} the alcove's corner reads {here:.2}/255, no darker than the \
             {previous:.2} the narrower radius gave. r_ssao_radius is not reaching the gather"
        );
        previous = here;
    }
}

// ---------------------------------------------------------------------------
// The silhouette
// ---------------------------------------------------------------------------

/// How much darker than the wall above it the wall beside the silhouette may
/// read.
///
/// **The bound the rim golden exists to hold.** Occlusion is a local effect: the
/// sphere stands two metres in front of the far wall, four times the shipped
/// radius, so nothing on it may darken the wall behind it. What produces a halo
/// there anyway is a normal reconstructed from depth, which is exact on a plane
/// and wrong on the one pixel of wall next to a silhouette.
///
/// Measured: the shipped gather leaves both blocks at 255.00 on both adapters —
/// no halo at all — while the hemisphere gather reads 237.88 against 246.20 on
/// lavapipe and 237.76 against 246.00 on radv, a halo of 0.0338 and 0.0335. The
/// bound is set above the second so that this is a claim about both gathers
/// rather than about the one that happens to be clean.
const RIM_HALO: f32 = 0.06;

/// How dark the silhouette pose's occlusion channel must get **somewhere**.
///
/// The other half of the anti-vacuity, and the one the shipped gather makes
/// necessary: it leaves the wall either side of the limb at 255.00, so the halo
/// bound above is satisfied by a gather that wrote white over the entire frame.
/// The sphere sits on a pedestal and the pedestal sits on the floor, both of
/// them in this pose, so a gather that ran has closed those creases up.
/// Measured: 172/255 under the shipped gather on lavapipe and 171 on radv, 159
/// under the other one on both.
const RIM_POSE_OCCLUDED: f32 = 220.0;

/// **A silhouette does not print onto the wall two metres behind it.**
///
/// The charter's rim claim, framed by [`court::rim_camera`] because at the fixed
/// camera the sphere is a few dozen pixels across and a one-pixel halo is not
/// something a person or a block average can see.
///
/// Three blocks. [`court::rim_outside`] is wall a few pixels clear of the limb,
/// [`court::rim_far`] is the same wall a long way above it, and
/// [`court::rim_inside`] is a pixel the sphere is drawn at. The claim is that the
/// first two agree; the third is what makes that mean anything, because two
/// blocks of flat wall with no silhouette between them agree perfectly.
///
/// The straddle is shown on the **shaded** frame rather than the occlusion
/// channel, and deliberately: the shipped gather leaves that whole region white,
/// so an anti-vacuity check read off the channel would be comparing 255 against
/// 255.
///
/// # How it was shown to fail
///
/// By moving [`court::rim_outside`]'s block onto the sphere, which is what a real
/// halo would look like from here and points the assertion the way it would
/// point. The halo came out at 0.1082 under the hemisphere gather, past the
/// bound.
///
/// # What was measured
///
/// The straddle: 122.00/255 on the wall against 118.47 on the sphere, the same
/// on both adapters. The halos are on [`RIM_HALO`].
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-alcove-golden.sh"]
fn the_silhouette_does_not_print_on_the_wall_behind_it() {
    let block = block_for(EXTENT);
    let camera = court::rim_camera();
    let (outside, far, inside) = (
        project(&camera, EXTENT, court::rim_outside()),
        project(&camera, EXTENT, court::rim_far()),
        project(&camera, EXTENT, court::rim_inside()),
    );

    let (shaded, paths, _) = draw(EXTENT, Arm::shipped().framed_on_the_rim());
    let (on_wall, on_sphere) = (
        brightness(&shaded, outside, block),
        brightness(&shaded, inside, block),
    );
    eprintln!(
        "alcove golden: the rim on {paths} — wall {on_wall:.2}/255, sphere {on_sphere:.2} at \
         {outside:?} and {inside:?}"
    );
    assert!(
        (on_wall - on_sphere).abs() > 1.0,
        "the block outside the limb reads {on_wall:.2}/255 and the one inside it {on_sphere:.2}, \
         so the pair does not straddle the silhouette and the bound below is a bound on two \
         patches of the same wall"
    );

    for technique in occlusion::names(occlusion::TECHNIQUE) {
        let (channel, _, _) = draw(
            EXTENT,
            Arm::shipped()
                .framed_on_the_rim()
                .as_channel()
                .on(technique),
        );
        let (beside, above) = (
            brightness(&channel, outside, block),
            brightness(&channel, far, block),
        );
        let halo = (above - beside) / above.max(1e-3);
        eprintln!(
            "alcove golden: {technique} at the rim — beside the limb {beside:.2}/255, well above \
             it {above:.2}, halo {halo:.4}"
        );
        let deepest = darkest(&channel);
        eprintln!("alcove golden: {technique} at the rim — darkest {deepest:.2}/255");
        assert!(
            deepest < RIM_POSE_OCCLUDED,
            "the {technique} gather leaves the whole silhouette pose at {deepest:.2}/255 or \
             brighter, so it produced no occlusion anywhere in this frame and the bound below \
             is a bound on a pass that did nothing"
        );
        assert!(
            above > 200.0,
            "the wall well above the silhouette reads {above:.2}/255 under {technique}, so the \
             comparand is itself occluded and the halo below is a ratio of two darkenings"
        );
        assert!(
            halo < RIM_HALO,
            "under {technique} the wall beside the silhouette reads {beside:.2}/255 against \
             {above:.2} for the same wall away from it — a halo of {halo:.4}, past {RIM_HALO}. \
             The sphere is four occlusion radii in front of that wall"
        );
    }
}

// ---------------------------------------------------------------------------
// The bent direction
// ---------------------------------------------------------------------------

/// How far each channel on open floor may sit from the geometric normal's own
/// colour, in 0-255 codes.
///
/// **A ceiling on quantisation and nothing else.** `court::OPEN_FLOOR` has
/// nothing inside the shipped occlusion radius of it, so the average unblocked
/// direction there is the floor's own normal and the frame draws `+Y` exactly:
/// measured at 0.48 codes on both adapters, which is the distance from
/// `srgb_encode(0.5)` to the byte it rounds to. It is set an order under the
/// thinnest lean below, so it is a bound that separates "the direction is the
/// normal" from "the direction leans" rather than one that admits both.
const OPEN_FLOOR_BENT_TOLERANCE: f32 = 2.5;

/// How far the crease's bent direction must lean out of the slot, in 0-255
/// codes on the `+z` channel.
///
/// **Measured, and floored at about half of it.** The figures are in the test's
/// own doc, and this is the thinnest of the three because it is the shallowest
/// geometry: two walls a quarter of a metre apart open the whole sky strip above
/// the slot, where the alcove's recess is closed on five sides.
const CREASE_BENT_LEAN: f32 = 3.75;

/// The same, at the alcove's back corner.
///
/// The deepest enclosure in the court, and the lean is an order above the
/// crease's.
const CORNER_BENT_LEAN: f32 = 28.0;

/// The same, at the contact band beside the box.
const CONTACT_BENT_LEAN: f32 = 18.0;

/// **The bent direction is the geometric normal where nothing occludes, and
/// leans out towards the opening where something does.**
///
/// `docs/plan/sample/19-alcove.md`'s milestone 3, and the reason the charter
/// asked for a picture at all: the occlusion channel's scalar can only *dim* the
/// ambient term, and the bent direction is what decides which part of the room
/// the surviving ambient is sampled from — so a term steering that cannot be
/// reviewed as a grey image, and until this view there was nothing to review.
///
/// Four readings off one frame, every one of them placed from the court's own
/// geometry rather than found by looking for a colour.
///
/// * **Open floor**, which is the claim with an absolute answer. Nothing is
///   within the shipped radius of [`court::OPEN_FLOOR`], so the average
///   unblocked direction there is the floor's own normal — `+Y` — and the view
///   draws a direction as `n * 0.5 + 0.5`, so the pixel is
///   `(0.5, 1.0, 0.5)` in linear light. [`srgb_encode`] is what turns that into
///   the byte the swapchain writes; nothing else in this file needed it, because
///   every other claim compares two readbacks with each other.
/// * **Three enclosed points**, each of which must lean **out of** its own
///   enclosure. All three open towards `+z`, and not by coincidence: the alcove's
///   mouth is cut in its `+z` face, the slot's near end is the end the fixed
///   camera stands off, and the contact band is the floor on the `+z` side of
///   the box — the fixed camera can only see a surface that faces it. So the
///   claim on each is that the `b` channel, which carries `+z`, stands **above**
///   the open floor's.
///
/// **Anti-vacuity, and it is the first reading that supplies it.** A view wired
/// to a constant, or a gather that reported the sentinel everywhere, draws the
/// mid grey `0x80` in all three channels — which is `(128, 128, 128)`, nowhere
/// near the `+Y` the first assertion demands. And a view that drew the shading
/// normal rather than the bent one passes the first assertion and fails all
/// three of the others, because the floor's shading normal is `+Y` at every one
/// of these points.
///
/// # What was measured
///
/// Open floor draws `(188.0, 255.0, 188.0)` on lavapipe (llvmpipe, LLVM 22.1.8)
/// and on radv (AMD Radeon RX 7900 XTX, RADV NAVI31) against a geometric normal
/// of `(187.5, 255.0, 187.5)` — 0.48 codes apart on both, which is the rounding.
/// The `+z` lean is 7.68 codes at the crease, 57.48 at the alcove's back corner
/// and 37.16 at the contact band on lavapipe, and 7.68 / 57.52 / 37.24 on radv.
/// Each floor above is about half the lower of its pair. The run prints every
/// one of them again on whatever adapter it opened.
///
/// # How it was shown to fail
///
/// Three runs, one per thing this check says.
///
/// * **The view never ran**, by drawing the arm without
///   `Arm::as_bent_direction` — the shaded court, which is what a switch wired
///   to nothing leaves. Open floor came out `(232.0, 230.0, 227.0)`, 44.48 codes
///   from the geometric normal and past [`OPEN_FLOOR_BENT_TOLERANCE`].
/// * **The open-floor reading taken at an occluded point**, `court::ALCOVE_CORNER`,
///   which is what a reference block that had slipped onto geometry would do:
///   `(214.16, 217.24, 245.48)`, 57.96 codes away, and the same assertion failed.
/// * **The golden compared against a frame drawn at another radius**, which
///   moves the direction everywhere the court encloses and nowhere it does not.
///   All four readings above still passed — a wider radius leans the crease
///   further, not less — and the comparison failed at 1.1230% grossly wrong
///   against a 0.1% budget. That is the half that says the picture is held to
///   *this* frame rather than to any frame with a green floor in it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-alcove-golden.sh"]
fn the_bent_direction_is_the_normal_on_open_floor_and_leans_out_of_an_enclosure() {
    let block = block_for(EXTENT);
    let camera = court::fixed_camera();
    let (bent, paths, _) = draw(EXTENT, Arm::shipped().as_bent_direction());

    let read = |point| channels(&bent, project(&camera, EXTENT, point), block);
    let open = read(court::OPEN_FLOOR);
    // `+Y` — the floor's own normal — encoded `n * 0.5 + 0.5` and put through the
    // swapchain's sRGB encode, which is the only place in this file a claim is
    // made against a derived colour rather than against a second readback.
    let up = [srgb_encode(0.5), srgb_encode(1.0), srgb_encode(0.5)];
    let drift = (0..3)
        .map(|at| (open[at] - up[at]).abs())
        .fold(0.0f32, f32::max);
    eprintln!(
        "alcove golden: the bent direction on {paths} — open floor {open:?} against the \
         geometric normal {up:?}, {drift:.2} codes apart"
    );
    assert!(
        drift < OPEN_FLOOR_BENT_TOLERANCE,
        "open floor draws {open:?} and the floor's own normal encodes to {up:?} — {drift:.2} \
         codes apart, past {OPEN_FLOOR_BENT_TOLERANCE}. Nothing is within the occlusion radius \
         of that point, so the average unblocked direction there is the normal itself"
    );

    // The `+z` channel: every enclosure in this court opens towards the camera,
    // which is the only half-space a surface it can see faces into.
    const OUT: usize = 2;
    for (name, point, least) in [
        ("the crease", court::crease_lit(), CREASE_BENT_LEAN),
        (
            "the alcove's back corner",
            court::ALCOVE_CORNER,
            CORNER_BENT_LEAN,
        ),
        ("the contact band", court::CONTACT_BAND, CONTACT_BENT_LEAN),
    ] {
        let here = read(point);
        let lean = here[OUT] - open[OUT];
        eprintln!(
            "alcove golden: the bent direction at {name} — {here:?} against open floor \
             {open:?}, leaning out by {lean:.2}"
        );
        assert!(
            lean > least,
            "{name} draws {here:?} and open floor {open:?}, so the direction leans out towards \
             the opening by {lean:.2} codes, short of {least}. The gather reports where what is \
             left of the sky lies, and at that point it is not overhead"
        );
    }

    // And last, the picture itself, on the four goldens' terms.
    match check_golden(&bent, "bent-normal", &paths) {
        Ok(line) => eprintln!("alcove golden: {line}"),
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
/// first reference would need four runs to produce four references — and the
/// three it had not reached yet would each be blessed against a different
/// process's state.
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
/// which is worth having and is not evidence that it was ever right. One shaded
/// court per gather, the occlusion channel on its own, and the silhouette pose.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-alcove-golden.sh"]
fn the_court_matches_its_goldens() {
    let mut faults = Vec::new();
    for (name, arm) in [
        ("court-gtao", Arm::shipped().on("gtao")),
        ("court-hemisphere", Arm::shipped().on("hemisphere")),
        ("occlusion", Arm::shipped().as_channel()),
        ("rim", Arm::shipped().framed_on_the_rim()),
    ] {
        let (image, paths, _) = draw(EXTENT, arm);
        match check_golden(&image, name, &paths) {
            Ok(line) => eprintln!("alcove golden: {line}"),
            Err(fault) => faults.push(fault),
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}

// ---------------------------------------------------------------------------
// The same claims at twenty-five times the pixels
// ---------------------------------------------------------------------------

/// **The court reads the same at presentation size**, and the frames are written
/// where a person can look at them.
///
/// Two jobs, and the first is what makes this a test. Every reading above is a
/// block a few pixels across, and at [`EXTENT`] a block is a large fraction of
/// the surface it sits on — so a claim that held because of where one triangle
/// edge landed would hold at that extent and nowhere else. The same world points
/// at [`REVIEW_EXTENT`] cover twenty-five times the area.
///
/// The second is that occlusion is a thing people look at, and 256×192 is not a
/// size anybody can judge a contact shadow at. Nothing here is blessed — there is
/// no reference at this extent — so the pictures are artefacts of the run.
///
/// The pair worth opening is `shaded` against `no-occlusion`: the alcove and the
/// stair are the same albedo as the wall behind them and carry no direct light,
/// so with the pass switched off they are **invisible**, and the occlusion term
/// is the whole of what makes that geometry legible.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-alcove-golden.sh"]
fn the_court_reads_the_same_at_presentation_size() {
    let extent = REVIEW_EXTENT;
    let block = block_for(extent);
    let camera = court::fixed_camera();

    let (occluded, paths, _) = draw(extent, Arm::shipped());
    let (flat, _, _) = draw(extent, Arm::shipped().without_occlusion());
    let (sunless, _, _) = draw(extent, Arm::shipped().sunless());
    let (sunless_flat, _, _) = draw(extent, Arm::shipped().sunless().without_occlusion());
    let (channel, _, _) = draw(extent, Arm::shipped().as_channel());
    save(&occluded, "shaded", extent);
    save(&flat, "no-occlusion", extent);
    save(&sunless, "sunless", extent);
    save(&channel, "occlusion", extent);

    for (name, point, least) in [
        (
            "the alcove's back corner",
            court::ALCOVE_CORNER,
            CORNER_DARKENING,
        ),
        ("the contact band", court::CONTACT_BAND, CONTACT_DARKENING),
    ] {
        let at = project(&camera, extent, point);
        let (dark, plain) = (
            brightness(&occluded, at, block),
            brightness(&flat, at, block),
        );
        let darkening = (plain - dark) / plain.max(1e-3);
        eprintln!(
            "alcove golden: {name} at {}x{} — {plain:.2} without occlusion, {dark:.2} with it, \
             darkening {darkening:.4}",
            extent.0, extent.1
        );
        assert!(
            darkening > least,
            "at {}x{} {name} darkens by {darkening:.4}, short of the {least} the same claim \
             holds to at {}x{}. The claim is about the court, not about the sampling",
            extent.0,
            extent.1,
            EXTENT.0,
            EXTENT.1,
        );
    }

    let at = project(&camera, extent, court::OPEN_FLOOR);
    let moved = brightness(&occluded, at, block) - brightness(&flat, at, block);
    assert!(
        moved.abs() < OPEN_FLOOR_TOLERANCE,
        "at {}x{} the open floor moved by {moved:.2}/255 when the occlusion pass was switched \
         off",
        extent.0,
        extent.1,
    );

    let crease = project(&camera, extent, court::crease_lit());
    let lit = linear_brightness(&flat, crease, block);
    let ambient = linear_brightness(&sunless_flat, crease, block);
    let with_sun = lit - linear_brightness(&occluded, crease, block);
    let without_sun = ambient - linear_brightness(&sunless, crease, block);
    let disagreement = (with_sun - without_sun).abs() / with_sun.max(without_sun).max(1e-6);
    eprintln!(
        "alcove golden: the crease at {}x{} on {paths} — sunlit {lit:.4} ambient {ambient:.4}, \
         occlusion takes {with_sun:.5} with the sun and {without_sun:.5} without it, \
         disagreement {disagreement:.3}",
        extent.0, extent.1
    );
    assert!(
        disagreement < CREASE_AGREEMENT,
        "at {}x{} the two drops disagree by {disagreement:.3}, past {CREASE_AGREEMENT}",
        extent.0,
        extent.1,
    );
}
