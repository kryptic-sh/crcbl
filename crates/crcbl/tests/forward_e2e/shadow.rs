//! Topic 18's shadow maps — the sun's cascades, a shadowed spot and a shadowed
//! point light — on whichever backend `CRCBL_GPU` names.
//!
//! # What this module exists to rule out
//!
//! **A shadow pass that renders nothing produces a frame that looks entirely
//! plausible.** Every surface is lit, no artifact appears anywhere, and a golden
//! blessed from that frame passes for ever. So does a pass that renders into a
//! map nothing ever samples. Neither is visible in a picture, which means a
//! golden image is exactly the wrong instrument here and the assertions below
//! are the right one:
//!
//! * [`the_shadow_atlas_is_written_rather_than_left_at_its_clear_value`] copies
//!   the atlas back and looks at the depth in it. The reversed-Z clear is `0.0`;
//!   a caster writes something above it. This is the assertion that separates
//!   "the cascades rendered" from "the cascades exist".
//! * [`a_wall_darkens_the_floor_it_stands_on_and_the_sun_decides_which_half`]
//!   is the other half — that the map is *sampled*, and that what it darkens
//!   moves when the light does. A shadow map can be perfectly written and read
//!   through a matrix that disagrees with the one it was rendered with; the
//!   result is a shadow in the wrong place, which only a comparison between two
//!   light directions catches.
//!
//! # The scene, and why it needs no geometry of its own
//!
//! The **cube** is the caster and the **open box's floor** is the receiver, both
//! meshes the engine already has. The cube hangs above and behind the box, the
//! sun comes in over one shoulder, and its shadow falls across one half of the
//! floor; reverse the sun and it falls across the other.
//!
//! The box's *walls* are deliberately not the caster, and the reason is worth
//! writing down because it is the first thing that was tried. Its five faces
//! point **inward**, so a sun outside the box sees every one of them from
//! behind — and the shadow pass rasterises with
//! [`CullMode::Back`](crcbl::hal::CullMode::Back) and, on a device with an
//! amplification stage, rejects clusters whose normal cone faces away from the
//! light. Both are correct for a closed caster and both discard a single-sided
//! wall, so the box casts nothing at all and the floor stayed uniformly lit.
//! That is a real property of this shadow pass rather than a defect in the
//! scene, and `docs/backlog.md` is where a two-sided caster belongs.
//!
//! The spot's scene is separate and its own section below says why: a light that
//! is not the sun needs a caster the *light* can see past and the *camera*
//! cannot, which the sun's overhead arrangement does not give.

use crate::harness::{Headless, poisoned};
use crate::mesh_scene::MESH_EXTENT;
use crcbl::hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, Capability, CommandEncoderDesc, Extent3d,
    Features, Format, HalError, ImageAspect, ImageBarrier, ImageSubresourceLayers,
    ImageSubresourceRange, MemoryLocation, PresentInfo, ResourceState, SubmitInfo, Support,
};

/// Where the open box sits: at the origin, so the camera below looks straight
/// down into it.
const BOX_AT: crcbl::math::Vec3 = crcbl::math::Vec3::ZERO;

/// Where the cube hangs: above the box and behind it in `+Z`.
///
/// Out of the box's own footprint, so a camera looking straight down into the
/// box sees the floor rather than the cube — and high enough that the sun below
/// throws its shadow far enough sideways to land on that floor rather than on
/// the cube's own feet.
const CUBE_AT: crcbl::math::Vec3 = crcbl::math::Vec3::new(0.0, 1.2, 1.4);

/// The sun's elevation, as the `y` of a direction whose horizontal part is `1`.
///
/// **Chosen so the shadow covers about half the floor and not all of it.** A
/// caster drops `d` onto the floor and its shadow moves `d / SUN_ELEVATION`
/// sideways: too low and the whole floor is dark, so the comparison below has
/// nothing to compare; too high and the shadow lands under the cube and never
/// reaches the box.
const SUN_ELEVATION: f32 = 2.0;

/// How far the sun is towards `+Z`, on [`SUN_ELEVATION`]'s terms.
///
/// The cube is behind the box in `+Z`, so a sun with no `z` at all would throw
/// its shadow along `x` and leave it `+Z` of the floor entirely. This is what
/// walks it back over the box.
const SUN_TOWARDS_Z: f32 = 1.0;

/// A frame drawn under one sun, with its shadow atlas read back beside it.
struct ShadowFrame {
    image: crcbl_golden::Image,
    /// The swapchain format this frame was drawn into.
    ///
    /// Carried because the atlas viewer's check compares a **derived** grey
    /// against a readback byte, and what a fragment wrote is not what lands in
    /// the buffer where the surface preferred an sRGB format — see
    /// [`crate::depth_probe::srgb_encode`]. The fixture takes whatever the
    /// surface offers, so which of the two happened is a fact about the run
    /// rather than a constant.
    format: Format,
    /// The atlas as depth, row-major, `atlas_extent()` of them — or the reason
    /// this backend produced none, which is a declared answer rather than a
    /// failure. See [`ShadowFrame::atlas`].
    atlas: Result<Vec<f32>, &'static str>,
    /// Where each atlas slot's map went, in the shape the uniform blocks carry:
    /// a scale into the atlas in `xy` and an offset in `zw`, one row per slot.
    ///
    /// Read off the renderer rather than derived a second time, and that is the
    /// point: `ForwardRenderer::begin_frame` hands this very array to both the
    /// frame block and the atlas viewer's, so a reading placed from it is placed
    /// where the shader was told the map is. See
    /// [`the_atlas_view_borders_a_subdivided_slot_at_its_own_size`].
    rects: [[f32; 4]; crcbl::render::shadow::TILES],
}

impl ShadowFrame {
    /// The whole atlas as depths, or `None` on a backend that declares
    /// [`Capability::DepthImageCopy`] absent.
    ///
    /// **`None` is a claim, not a skip.** Reading a shadow atlas means copying
    /// a *depth* image into a buffer, which is not a copy every backend has: a
    /// depth format is two planes, a sampled one may be stored typeless, and
    /// `crcbl_hal::DIVERGENCES` lists D3D12 and the browser replayer with the
    /// reason each gives. [`render_scene`] records that copy either way — where
    /// the capability is declared present the depths come back and every tile
    /// assertion below runs against them, and where it is declared absent
    /// `render_scene` has already asserted that the recorded copy was *refused*,
    /// with [`HalError::Unsupported`] carrying the same reason
    /// `Device::supports` gave. A backend that declared the gap and then
    /// performed the copy, or that refused it as a malformed descriptor, fails
    /// there rather than arriving here.
    fn atlas(&self) -> Option<&[f32]> {
        match &self.atlas {
            Ok(atlas) => Some(atlas),
            Err(why) => {
                eprintln!(
                    "{suite}: shadow — this backend declares no depth-image copy and refused the \
                     atlas readback as it says it does, so the tile assertions have nothing to \
                     read: {why}",
                    suite = crate::SUITE
                );
                None
            }
        }
    }
}

/// Tile `tile` of `atlas`, as depths.
fn tile(atlas: &[f32], tile: usize) -> Vec<f32> {
    let (width, _) = crcbl::render::shadow::atlas_extent();
    let (origin_x, origin_y) = crcbl::render::shadow::tile_origin(tile);
    let side = crcbl::render::shadow::TILE;
    (0..side)
        .flat_map(|row| {
            let start = ((origin_y + row) * width + origin_x) as usize;
            atlas[start..start + side as usize].to_vec()
        })
        .collect()
}

/// What one frame of this module draws.
///
/// A struct rather than four arguments, because there are two scenes here now —
/// the sun's wall and the spot's caster — and every line of the plumbing below
/// is the same for both. What differs is exactly these four things.
struct ShadowScene<'a> {
    /// What goes in the frame besides the cube: the box for the sun's scene, the
    /// pyramid for the spot's.
    prepare: &'a dyn Fn(&mut crcbl::render::ForwardRenderer),
    camera: crcbl::render::Camera,
    sun: crcbl::render::DirectionalLight,
    /// Where the caller puts the cube — the wall's hanging caster in the sun's
    /// scene, the floor in the spot's.
    model: crcbl::math::Mat4,
}

/// Opens a device, draws `scene`, and reads back both the tonemapped frame and
/// the shadow atlas.
fn render_scene(scene: &ShadowScene<'_>) -> ShadowFrame {
    // The mesh suite's own harness, so this frame is drawn at the extent and on
    // the device every other mesh assertion is made against.
    let headless = Headless::open_for_mesh_with(
        Features::GPU_DRIVEN | Features::MESH_SHADER | Features::TASK_SHADER,
    );
    let device = headless.device.as_ref();
    let (width, height) = MESH_EXTENT;
    let mut pool = crcbl::render::TransientPool::new();
    let mut renderer = crcbl::render::ForwardRenderer::new(device, headless.queue, headless.format)
        .expect("the forward renderer builds");
    // The cube first and whatever else the scene wants above it, which is the
    // order the pool has always been filled in.
    crate::mesh_scene::place_cube_at(&mut renderer, scene.model);
    (scene.prepare)(&mut renderer);

    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, MESH_EXTENT);

    let (atlas_width, atlas_height) = crcbl::render::shadow::atlas_extent();
    let color_bytes = u64::from(width) * u64::from(height) * 4;
    // `D32Float`: one channel of four bytes.
    let atlas_bytes = u64::from(atlas_width) * u64::from(atlas_height) * 4;
    let staging = |label, size| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer")
    };
    let color_staging = staging("shadow readback", color_bytes);
    let atlas_staging = staging("shadow atlas readback", atlas_bytes);

    renderer
        .begin_frame(device, &scene.camera, &scene.sun, MESH_EXTENT)
        .expect("the uniform buffer is writable");
    // The layout that call just spent, taken before the renderer is released:
    // these are the rectangles this frame's blocks carry, and every slot's map
    // is rendered into the one that names it.
    let rects = renderer.shadow_lights().atlas_rects();

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("shadow frame"),
        queue: headless.queue,
    });
    let compiled = {
        let mut graph = crcbl::render::RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl::render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: MESH_EXTENT,
                initial: ResourceState::Undefined,
                claim: crcbl::render::InitialClaim::Acquired,
                final_state: ResourceState::TransferSrc,
            },
        );
        let _ = renderer.add_passes(&mut graph, &pool, target, MESH_EXTENT);
        graph.compile(&pool).expect("a legal frame")
    };
    compiled
        .execute(device, &mut pool, encoder.as_mut(), None)
        .expect("the graph executed");

    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: color_staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: acquired.image,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: crcbl::hal::Offset3d::default(),
        image_extent: Extent3d::d2(width, height),
    });

    // **Reading the atlas is a declared capability, not a given.** The atlas is
    // `D32Float`, and copying a depth image into a buffer is
    // `Capability::DepthImageCopy` — which D3D12 and the browser replayer both
    // declare absent, each for a reason `crcbl_hal::DIVERGENCES` carries. So the
    // copy is recorded where the backend says it works and *probed on an
    // encoder of its own* where it says it does not: recording it into this one
    // would fail `finish` and take the colour readback down with it, leaving
    // every assertion in this module — including the several that never look at
    // a depth at all — unable to run.
    let atlas_copy = BufferImageCopy {
        buffer: atlas_staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: renderer.shadow_atlas(),
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::DEPTH,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: crcbl::hal::Offset3d::default(),
        image_extent: Extent3d::d2(atlas_width, atlas_height),
    };
    let depth_copy = device.supports(Capability::DepthImageCopy);
    if depth_copy.is_yes() {
        // **The one hand-written barrier in this module.** The atlas belongs to
        // the renderer, not to the graph, and the graph left it in `ShaderRead`
        // because that is what the colour pass wanted — so a reader outside the
        // frame has to ask for it. Every barrier *inside* the frame is still
        // the graph's.
        let range = ImageSubresourceRange::all(Format::D32Float);
        let to_source = [ImageBarrier::new(
            renderer.shadow_atlas(),
            range,
            ResourceState::ShaderRead,
            ResourceState::TransferSrc,
        )];
        encoder.pipeline_barrier(&Barriers {
            images: &to_source,
            ..Barriers::default()
        });
        encoder.copy_image_to_buffer(&atlas_copy);
    }

    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device
        .present(
            headless.queue,
            &PresentInfo {
                swapchain: headless.swapchain,
                waits: acquired.present_semaphore.as_slice(),
                present_id: None,
            },
        )
        .expect("present");
    device.wait_idle().expect("idle");

    let mut color = poisoned(color_bytes as usize);
    headless.readback(color_staging, color_bytes, &mut color);
    let atlas = match depth_copy {
        Support::Yes => {
            let mut atlas_raw = poisoned(atlas_bytes as usize);
            headless.readback(atlas_staging, atlas_bytes, &mut atlas_raw);
            Ok(atlas_raw
                .chunks_exact(4)
                .map(|word| f32::from_le_bytes(word.try_into().expect("a four-byte chunk")))
                .collect())
        }
        // Either refusal means the same thing here — no atlas to read. The
        // second arm is unreachable in practice, because `DepthImageCopy` has no
        // `gating_feature` and `Support::granted` is the only source of it; it
        // is written out rather than wildcarded so a gate arriving later is a
        // decision somebody makes instead of one this `_` makes for them.
        Support::No(why) | Support::NotOnThisDevice(why) => {
            Err(refused_atlas_copy(&headless, &atlas_copy, why))
        }
    };

    // Through the ring's own channel order rather than as raw RGBA. The Vulkan
    // original could assume the latter because its fixture only ever opened one
    // format; this fixture takes whatever the surface prefers, and wgpu prefers
    // BGRA. Every measurement below is a mean over the three colour channels, so
    // the order does not change a number — it is here so the pixels a future
    // assertion reads are the pixels it names.
    let order = match headless.format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    let image = crcbl_golden::Image::from_readback(width, height, &color, order)
        .expect("the readback is exactly one image");

    let format = headless.format;
    device.destroy_command_buffer(commands);
    device.destroy_buffer(color_staging);
    device.destroy_buffer(atlas_staging);
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    ShadowFrame {
        image,
        format,
        atlas,
        rects,
    }
}

/// **Holds a backend to a declared missing depth-image copy**, and hands back
/// the reason it declared so [`ShadowFrame::atlas`] can carry it.
///
/// The assertion that stands in for the tile checks on such a backend, and the
/// reason this module does not simply stop looking at the atlas when
/// [`Capability::DepthImageCopy`] is absent: a declaration nothing exercises is
/// a sentence in a match arm. Three things have to hold, and each is a distinct
/// way the declaration could be a lie:
///
/// * the copy is **refused** — a backend that declared the gap and then recorded
///   the copy anyway is claiming to be worse than it is, and the atlas readback
///   should come back;
/// * it is refused with [`HalError::Unsupported`], which is the variant the seam
///   documents for "this backend cannot" and the one a caller branches on to
///   pick a fallback. `InvalidDescriptor` here would send that caller looking
///   for a field to correct, and this whole slice exists because `crcbl-dx12`
///   answered exactly that; and
/// * the message carries the reason `Device::supports` gave, so the declaration
///   and the error a caller actually reads cannot drift apart.
///
/// The probe gets its own encoder because the frame's has already been finished
/// and submitted; nothing is submitted from this one, and `finish` failing is
/// the whole result.
fn refused_atlas_copy(
    headless: &Headless,
    copy: &BufferImageCopy,
    why: &'static str,
) -> &'static str {
    let mut probe = headless.device.create_command_encoder(&CommandEncoderDesc {
        label: Some("shadow atlas depth-copy probe"),
        queue: headless.queue,
    });
    probe.copy_image_to_buffer(copy);
    let error = probe.finish().err().unwrap_or_else(|| {
        panic!(
            "this backend declares Capability::DepthImageCopy absent ({why}) and then recorded \
             the copy without complaint. Either the declaration is stale — in which case delete \
             its crcbl_hal::DIVERGENCES entry and let this module read the atlas — or the \
             encoder dropped the copy silently, which is a shadow atlas nobody can check and a \
             frame that looks correct."
        )
    });
    assert!(
        matches!(error, HalError::Unsupported { .. }),
        "this backend refused the atlas copy with {error}, which is not the \
         HalError::Unsupported the seam documents for \"this backend cannot\". A caller branching \
         on that variant to pick a fallback would miss the refusal entirely."
    );
    let text = error.to_string();
    assert!(
        text.contains(why),
        "this backend declares Capability::DepthImageCopy absent because {why:?}, and refused the \
         copy saying {text:?} instead. The declaration a caller reads before recording and the \
         error it reads afterwards have to be the same sentence, or one of them is stale."
    );
    eprintln!(
        "{suite}: shadow — the atlas is not readable on this backend, as declared: {text}",
        suite = crate::SUITE
    );
    why
}

/// The open box under a sun at `to_light`, with the cube hanging over it.
fn render_shadowed(to_light: crcbl::math::Vec3) -> ShadowFrame {
    render_scene(&ShadowScene {
        prepare: &|renderer| {
            crate::mesh_scene::place(
                renderer,
                crcbl::render::scene::DEMO_OPEN_BOX,
                crcbl::render::scene::DEMO_UNTINTED,
                crcbl::math::Mat4::from_translation(BOX_AT),
            );
        },
        camera: overhead_camera(),
        sun: crcbl::render::DirectionalLight {
            direction: to_light,
            ..crcbl::render::DirectionalLight::default()
        },
        model: crcbl::math::Mat4::from_translation(CUBE_AT),
    })
}

/// A camera above the box, looking down into it.
///
/// Nearly on the box's axis, so the floor is a square in the middle of the frame
/// with none of it hidden behind a wall — the floor is where every measurement
/// below is taken. Far enough up that the cube, which hangs `+Z` of the box, is
/// outside the cone that reaches the floor and hides none of it.
fn overhead_camera() -> crcbl::render::Camera {
    crcbl::render::Camera {
        eye: BOX_AT + crcbl::math::Vec3::new(0.0, 3.0, 0.15),
        target: BOX_AT + crcbl::math::Vec3::new(0.0, -0.5, 0.0),
        up: crcbl::math::Vec3::Y,
        projection: crcbl::render::Projection::default(),
    }
}

/// The mean brightness of a horizontal band of the floor, over `x` in
/// `columns`.
///
/// A band rather than one pixel: PCF makes a shadow edge a gradient a few texels
/// wide, and a single sample lands wherever that gradient happens to be.
fn band_brightness(image: &crcbl_golden::Image, columns: std::ops::Range<u32>) -> f32 {
    let rows = (MESH_EXTENT.1 * 7 / 16)..(MESH_EXTENT.1 * 9 / 16);
    let mut total = 0.0f32;
    let mut count = 0u32;
    for y in rows {
        for x in columns.clone() {
            let pixel = image.pixel(x, y).expect("inside the frame");
            total += f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2]);
            count += 1;
        }
    }
    assert!(count > 0, "an empty band measures nothing");
    total / (count as f32 * 3.0)
}

/// A sun coming from `+X` or `-X` at [`SUN_ELEVATION`].
fn sun(sign: f32) -> crcbl::math::Vec3 {
    crcbl::math::Vec3::new(sign, SUN_ELEVATION, SUN_TOWARDS_Z).normalize()
}

/// Takes `r_shadow_filter` and `r_shadow_split` for the run and puts both back
/// at their defaults when it drops.
///
/// A guard rather than two `set` calls per test, because the variables are
/// process-wide and this suite's runner may share a process between tests: a
/// filter one test left moved is a frame the next one did not ask for, and
/// every golden in this workspace was blessed at the defaults.
struct FilterSwitch;

impl FilterSwitch {
    /// Takes the two and puts them at what ships.
    fn take() -> Self {
        let switch = Self;
        switch.reset();
        switch
    }

    /// Asks for `filter` on the near side of the seam.
    fn filter(&self, filter: crcbl::render::shadow::Filter) {
        crcbl::render::shadow::r_shadow_filter
            .set(&crcbl::console::Value::Enum(filter.label()))
            .expect("`r_shadow_filter` is a writable enum in the set");
    }

    /// Asks for a seam at `at`, or for none at zero.
    fn seam(&self, at: f32) {
        crcbl::render::shadow::r_shadow_split
            .set(&crcbl::console::Value::Float(at))
            .expect("`r_shadow_split` is a writable float in range");
    }

    /// Back to the frame every golden was blessed as.
    fn reset(&self) {
        self.filter(crcbl::render::shadow::shipped_filter());
        self.seam(0.0);
    }
}

impl Drop for FilterSwitch {
    fn drop(&mut self) {
        self.reset();
    }
}

/// How many pixels a filter change has to move before the difference is the
/// filter rather than the driver, and by how much.
///
/// **Swept before they were fixed**, on both Vulkan adapters this workspace can
/// run locally — radv on an RX 7900 XTX and lavapipe — over the `sun(1.0)` frame
/// [`a_penumbra_resolves_differently_under_every_rung_of_the_filter_ladder`]
/// draws:
///
/// | Against the shipped `pcss` | radv        | lavapipe    |
/// | -------------------------- | ----------- | ----------- |
/// | `disc`                     | 326 px / 75 | 323 px / 75 |
/// | `box`                      | 690 px / 74 | 693 px / 74 |
///
/// The two adapters agree to within three pixels and to the level on the worst
/// channel, and the *narrow* rung is `disc` — the middle of the ladder, which
/// differs from `pcss` only where a blocker stands high enough for the search to
/// widen the disc. So the counts are floored under that column rather than under
/// the box's, at roughly two thirds of it, and the level under half of what
/// either rung moves: enough margin for a driver that rounds a penumbra
/// differently, and nowhere near enough for an arm that fell through to the
/// filter that ships, which moves exactly zero pixels.
const PENUMBRA_PIXELS: usize = 200;

/// How far the worst-moved channel has to travel. See [`PENUMBRA_PIXELS`].
const PENUMBRA_LEVELS: u8 = 32;

/// How many pixels of `a` and `b` differ at all, and by how much the worst
/// channel of any of them does.
///
/// The pair rather than one number, because they fail differently: a filter
/// that reached nothing moves *no* pixels, and one that reached the frame
/// through a lane that is almost always zero moves many pixels by one level —
/// which is the shape of a rounding difference between two drivers rather than
/// of a kernel change.
fn difference(a: &crcbl_golden::Image, b: &crcbl_golden::Image) -> (usize, u8) {
    difference_over(a, b, 0..MESH_EXTENT.0)
}

/// [`difference`] over `columns` alone, which is how the seam's two sides are
/// asked about separately.
///
/// A count and a worst case rather than the two pixel vectors compared whole:
/// an `assert_eq!` on a quarter of a frame prints a quarter of a frame, and a
/// failure nobody can read is one nobody diagnoses.
fn difference_over(
    a: &crcbl_golden::Image,
    b: &crcbl_golden::Image,
    columns: std::ops::Range<u32>,
) -> (usize, u8) {
    let mut moved = 0usize;
    let mut worst = 0u8;
    assert!(!columns.is_empty(), "an empty range compares nothing");
    for y in 0..MESH_EXTENT.1 {
        for x in columns.clone() {
            let (one, two) = (
                a.pixel(x, y).expect("inside the frame"),
                b.pixel(x, y).expect("inside the frame"),
            );
            let apart = one
                .iter()
                .zip(&two)
                .map(|(l, r)| l.abs_diff(*r))
                .max()
                .unwrap_or(0);
            if apart > 0 {
                moved += 1;
                worst = worst.max(apart);
            }
        }
    }
    (moved, worst)
}

/// **The filter the console selects reaches the picture**, measured across the
/// one part of the frame where two filters can disagree.
///
/// `docs/plan/45-shadows.md`'s ladder has three rungs and until
/// `r_shadow_filter` existed only the top one was compiled. A selector that
/// wrote its mode into a lane nothing read, or a shader arm that fell through
/// to the filter that ships, would leave every frame identical — and no golden
/// could see it, because the frame it draws is the one that was blessed.
///
/// **The penumbra is where the claim lives.** A 3×3 box reaches one texel and
/// the shipping filter reaches two to eight, so a fully lit or fully shadowed
/// fragment answers the same under both and only the gradient between them
/// carries the difference. That is why this counts *pixels that moved* rather
/// than averaging a band: the penumbra is a few dozen pixels of a frame the
/// tonemap has taken to near white almost everywhere, and a mean over any band
/// wide enough to be robust dilutes it to under a level — measured at 0.03 of
/// 255 over the two central eighths, which is why the first spelling of this
/// test was thrown away. Both thresholds are swept rather than guessed; see
/// [`PENUMBRA_PIXELS`].
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_penumbra_resolves_differently_under_every_rung_of_the_filter_ladder() {
    let switch = FilterSwitch::take();
    let ships = crcbl::render::shadow::shipped_filter();
    let shipped = render_shadowed(sun(1.0));

    // Every rung but the one that ships, so the middle of the ladder is held to
    // the same claim as its ends: a `disc` arm that fell through to `pcss` is
    // exactly as invisible as a `box` one that did, and a test naming only the
    // box would pass over it.
    for filter in [
        crcbl::render::shadow::Filter::Disc,
        crcbl::render::shadow::Filter::Box,
    ] {
        assert_ne!(filter, ships, "this rung is the one that ships");
        switch.filter(filter);
        let frame = render_shadowed(sun(1.0));
        let (moved, worst) = difference(&shipped.image, &frame.image);
        eprintln!(
            "{suite}: shadow — `{rung}` against the shipped `{shipped_rung}`: {moved} pixels \
             moved, worst channel {worst}",
            rung = filter.label(),
            shipped_rung = ships.label(),
            suite = crate::SUITE
        );
        assert!(
            moved >= PENUMBRA_PIXELS && worst >= PENUMBRA_LEVELS,
            "`r_shadow_filter {rung}` moved {moved} pixels by at most {worst} levels, which is \
             under the {PENUMBRA_PIXELS} and {PENUMBRA_LEVELS} both adapters clear — so that \
             rung is not reaching the shader",
            rung = filter.label()
        );
    }
}

/// How far the seam's own column bleeds sideways, in pixels, and therefore how
/// many columns either side of it
/// [`the_seam_puts_the_console_s_filter_on_one_side_and_the_shipped_one_on_the_other`]
/// leaves out of its comparison.
///
/// **The antialiasing filter's own horizontal reach**, which is what carries a
/// per-pixel seam sideways. `crcbl_render::split`'s header is where the shape is
/// argued: the fragment stage decides its own filter per pixel, and the passes
/// *after* it read their neighbours, so the picture either side of the column is
/// only the unsplit frame's outside that footprint.
///
/// `shaders/smaa_weights.slang`'s `MAX_SEARCH_STEPS` bounds the edge walk and
/// each of its steps covers two texels, so this is twice that count. The number
/// is taken from the pass rather than from the measurement, because what the
/// measurement finds is what *this frame's* edges happened to need: probed
/// column by column on radv over the `sun(1.0)` frame at a seam of 0.5 — the
/// seam falls on column 128 of 256 — the near side differed from the unsplit
/// `box` frame only at column 127, in one pixel by one level, and the far side
/// differed from the unsplit `pcss` frame at six columns between 128 and 140, in
/// one pixel each. Widening a constant until a test passes is how a threshold
/// ends up describing one frame; the walk's own bound describes the pass.
///
/// What it costs is the columns nearest the seam, and the assertion either side
/// of it is then exact rather than tolerant — the anti-vacuity check above is
/// what says the columns that are left still carry a difference to see.
const SEAM_BLEED: u32 = 32;

/// **The seam runs down the column `crcbl_render::split` counted**: the
/// console's filter left of it, the shipped one right of it, and the whole
/// frame the console's when there is no seam.
///
/// Both halves are asserted against *unsplit* frames rather than against each
/// other, which is what makes this a claim about the seam rather than about the
/// two filters: a shader that ignored the column and ran the near mode
/// everywhere would satisfy the left half and fail the right, and one that ran
/// the far mode everywhere the other way round.
///
/// **Compared pixel for pixel and not by a mean.** Everything but the filter is
/// the same frame — the same scene, the same camera, the same block except for
/// the row that selects — so a side of the seam is bit-identical to the unsplit
/// frame it belongs to. A mean would pass on two frames that differed
/// everywhere and averaged the same.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_seam_puts_the_console_s_filter_on_one_side_and_the_shipped_one_on_the_other() {
    let switch = FilterSwitch::take();
    let internal = MESH_EXTENT;
    let column = internal.0 / 2;
    assert!(
        column > 0 && column < internal.0,
        "a seam at 0.5 has to leave columns on both sides of a {internal:?} frame"
    );

    let shipped = render_shadowed(sun(1.0));
    switch.filter(crcbl::render::shadow::Filter::Box);
    let boxed = render_shadowed(sun(1.0));

    // Anti-vacuity: the two unsplit frames have to actually differ on **both**
    // sides of the column, or an equality below holds of a half where the two
    // filters agree anyway.
    for (side, range) in [
        ("left", 0..column - SEAM_BLEED),
        ("right", column + SEAM_BLEED..internal.0),
    ] {
        let (moved, _) = difference_over(&boxed.image, &shipped.image, range);
        assert!(
            moved > 0,
            "the two filters drew the same {side} half, so the assertion about that side of \
             the seam separates nothing"
        );
    }

    // A seam at zero is no seam: the console's filter over the whole target.
    switch.seam(0.0);
    let unsplit = render_shadowed(sun(1.0));
    assert_eq!(
        difference(&unsplit.image, &boxed.image),
        (0, 0),
        "`r_shadow_split 0` has to draw `r_shadow_filter`'s own filter everywhere, which is \
         what makes the default a frame with no comparison in it"
    );

    switch.seam(0.5);
    let compared = render_shadowed(sun(1.0));
    assert_eq!(
        difference_over(&compared.image, &boxed.image, 0..column - SEAM_BLEED),
        (0, 0),
        "left of the seam's column the frame has to be the one `r_shadow_filter box` draws \
         on its own"
    );
    assert_eq!(
        difference_over(
            &compared.image,
            &shipped.image,
            column + SEAM_BLEED..internal.0
        ),
        (0, 0),
        "and right of it the one the shipped filter draws, which is the far side following \
         the variable's *default* rather than the console"
    );
}

/// **The cascades were rendered into, not merely allocated.**
///
/// The single assertion this whole slice can otherwise hide behind. Under
/// reversed-Z the atlas is cleared to [`crcbl::hal::depth::CLEAR`] = `0.0`, and a
/// caster writes a depth above it — so a tile that is entirely zero is a cascade
/// whose cull rejected everything, whose draws were never recorded, or whose
/// viewport landed somewhere else. Every one of those renders a frame in which
/// nothing is shadowed, which is a picture no golden can tell from a correct one.
///
/// Asserted **per cascade**, not over the atlas as a whole: one cascade drawing
/// and the rest empty is exactly what a viewport that ignored
/// [`tile_origin`](crcbl::render::shadow::tile_origin) would produce, and a
/// whole-atlas check passes it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_shadow_atlas_is_written_rather_than_left_at_its_clear_value() {
    let frame = render_shadowed(sun(1.0));
    // On a backend without `Capability::DepthImageCopy` there is no atlas to
    // look at, and what `render_scene` asserted in its place is that the copy
    // was refused for the reason the backend declares — see
    // [`ShadowFrame::atlas`].
    let Some(atlas) = frame.atlas() else {
        return;
    };
    let side = crcbl::render::shadow::TILE as usize;
    assert_eq!(
        atlas.len(),
        side * side * crcbl::render::shadow::TILES,
        "the readback is the whole atlas"
    );
    // **The light tiles are untouched in a scene with no shadowed light**, which
    // is the other half of the same claim: a free tile that got drawn into would
    // be a viewport landing where it does not belong, and a cascade's map
    // written over a light's is a picture no golden can see.
    for light_tile in 0..crcbl::render::shadow::LIGHT_TILES {
        let free = tile(atlas, crcbl::render::shadow::light_tile(light_tile));
        assert!(
            free.iter().all(|depth| *depth == crcbl::hal::depth::CLEAR),
            "light tile {light_tile} holds depths in a frame with no shadowed light in it"
        );
    }
    for cascade in 0..crcbl::render::shadow::CASCADES {
        let tile = tile(atlas, cascade);
        let written = tile.iter().filter(|depth| **depth > 0.0).count();
        assert!(
            written > 0,
            "cascade {cascade}'s tile is entirely at the reversed-Z clear, so nothing was \
             rendered into it — which is a shadow map that shadows nothing and a frame that \
             looks correct"
        );
        // Not a stray texel either: the box is a solid object a few units
        // across, and a cascade covering it writes a real area of the tile. One
        // per mille is far below what the geometry occupies and far above what
        // a mis-transformed sliver would.
        let fraction = written as f64 / tile.len() as f64;
        assert!(
            fraction > 0.001,
            "cascade {cascade} wrote only {written} of {} texels ({fraction:.5}), which is a \
             sliver rather than a caster",
            tile.len()
        );
        // And every depth is a legal reversed-Z one, which is what says the
        // projection's range is the one the comparison sampler is testing
        // against rather than an arbitrary scale that happens to be non-zero.
        assert!(
            tile.iter().all(|depth| (0.0..=1.0).contains(depth)),
            "cascade {cascade} holds a depth outside 0..1"
        );
    }
}

/// **The map is sampled, and the sun decides where the shadow lands.**
///
/// The box's `+X` wall stands between a sun in `+X` and the floor, so the floor's
/// `+X` half is dark and its `-X` half is lit. Move the sun to `-X` and the two
/// swap. Both halves of that are asserted, and the second is what makes it
/// evidence: a renderer that darkened one side of everything — a lighting bug, a
/// wrong normal, a vignette — would satisfy the first on its own and cannot
/// satisfy a pair that swaps.
///
/// Measured as a ratio between two bands of the same frame rather than as an
/// absolute colour, for the reason the mesh suite's own light test gives: what
/// survives Lambert, the GGX lobe and the tonemap is which side leads.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_wall_darkens_the_floor_it_stands_on_and_the_sun_decides_which_half() {
    // The outer sixths of the frame are the walls seen edge-on; the inner
    // quarters either side of centre are floor under both suns.
    let left = (MESH_EXTENT.0 * 6 / 16)..(MESH_EXTENT.0 * 8 / 16);
    let right = (MESH_EXTENT.0 * 8 / 16)..(MESH_EXTENT.0 * 10 / 16);

    let from_plus_x = render_shadowed(sun(1.0));
    let from_minus_x = render_shadowed(sun(-1.0));

    let plus = (
        band_brightness(&from_plus_x.image, left.clone()),
        band_brightness(&from_plus_x.image, right.clone()),
    );
    let minus = (
        band_brightness(&from_minus_x.image, left),
        band_brightness(&from_minus_x.image, right),
    );
    eprintln!(
        "crcbl forward e2e: shadow — sun in +X: left {:.1} right {:.1}; sun in -X: left {:.1} right {:.1}",
        plus.0, plus.1, minus.0, minus.1
    );

    // A margin rather than "any difference at all": the two bands are lit by
    // the same light at slightly different angles, so a small difference is
    // ordinary shading. A shadow is not small.
    const MARGIN: f32 = 8.0;
    assert!(
        plus.0 + MARGIN < plus.1,
        "with the sun in +X the cube's shadow must fall across the -X half of the floor, \
         but that half measured {:.1} against {:.1}",
        plus.0,
        plus.1
    );
    assert!(
        minus.1 + MARGIN < minus.0,
        "and the sun in -X must darken the other half, but it measured {:.1} against {:.1}",
        minus.1,
        minus.0
    );
}

// ---------------------------------------------------------------------------
// The cascade debug overlay
// ---------------------------------------------------------------------------
//
// `docs/plan/45-shadows.md`'s eighth decision made the cascade switch a **band**
// rather than a step, and until `DebugView::Cascades` existed nothing in this
// workspace could show where that band falls. The overlay multiplies the shaded
// picture by `crcbl::shaders::mesh::CASCADE_TINTS` of the cascade each sun-lit
// fragment read, mixing the two tints across the band by the same weight the
// visibility itself was mixed with.
//
// The scene is one flat pavement seen obliquely, and nothing else: no caster, so
// the whole of it is lit and the only thing that varies along it is the cascade
// its shadow lookup selected. That is what makes a pixel's colour readable as a
// cascade rather than as a shadow.

/// How much the cube is scaled by to get the cascade overlay's pavement, and how
/// far it is dropped so its `+Y` face is the plane `y = 0`.
///
/// `SPOT_FLOOR_SCALE`'s shape and larger, because this camera looks *along* the
/// surface rather than down at it: the far reading below stands past the last
/// cascade's inner edge, and the pavement has to reach beyond that or the
/// reading is of the clear colour.
const PAVEMENT_SCALE: f32 = 30.0;

/// How far above the pavement the eye stands, in world units.
///
/// The pavement is the plane `y = 0`, so this is also the shortest eye distance
/// any pixel of it can have — [`pavement_pixel`] inverts that relation.
const PAVEMENT_EYE_UP: f32 = 3.0;

/// The camera the cascade overlay is read from: over the pavement, pitched down
/// 45° so the surface fills the frame from close to far.
///
/// **The pitch is what puts a cascade boundary on screen.** Straight down and
/// every pixel is at much the same eye distance, so one cascade covers the frame
/// and there is no boundary to look at; along the surface and the near rows are
/// metres closer than the far ones, which is the range the splits divide.
fn pavement_camera() -> crcbl::render::Camera {
    crcbl::render::Camera {
        eye: crcbl::math::Vec3::new(0.0, PAVEMENT_EYE_UP, 0.0),
        target: crcbl::math::Vec3::new(0.0, 0.0, -PAVEMENT_EYE_UP),
        up: crcbl::math::Vec3::Y,
        projection: crcbl::render::Projection::default(),
    }
}

/// The sun the pavement is lit by: high and a little off both axes.
///
/// High, so the pavement's `+Y` face has a positive `N·L` everywhere and every
/// pixel of it is a fragment `sun_visibility` actually selects a cascade for — a
/// grazing sun would leave the far rows with the `CASCADE_NONE` a surface facing
/// away gets, and the overlay would be white there for a reason that has nothing
/// to do with the splits.
fn pavement_sun() -> crcbl::render::DirectionalLight {
    crcbl::render::DirectionalLight {
        direction: crcbl::math::Vec3::new(0.35, 1.0, 0.2),
        ..crcbl::render::DirectionalLight::default()
    }
}

/// The pavement under [`pavement_sun`], with the cascade view `on` or off.
fn render_pavement(on: bool) -> ShadowFrame {
    let prepare = move |renderer: &mut crcbl::render::ForwardRenderer| {
        renderer.set_cascade_view(on);
    };
    render_scene(&ShadowScene {
        prepare: &prepare,
        camera: pavement_camera(),
        sun: pavement_sun(),
        model: crcbl::math::Mat4::from_translation(crcbl::math::Vec3::new(
            0.0,
            -0.5 * PAVEMENT_SCALE,
            0.0,
        )) * crcbl::math::Mat4::from_scale(crcbl::math::Vec3::splat(PAVEMENT_SCALE)),
    })
}

/// Which pixel the pavement point at eye distance `distance` lands on.
///
/// The pavement is the plane `y = 0` and the camera stands [`PAVEMENT_EYE_UP`]
/// above it on the axis, so a point straight ahead at eye distance `d` is at
/// `z = -sqrt(d² - up²)` — and the pixel is that point through the very matrix
/// the frame was drawn with, which is what lets the readings below be *placed*
/// from `Cascades::far` instead of found by looking at the picture.
///
/// # Panics
///
/// If `distance` is inside the eye's own height, where no pavement point has it,
/// or if the point lands outside the frame.
fn pavement_pixel(distance: f32) -> (u32, u32) {
    let camera = pavement_camera();
    assert!(
        distance > PAVEMENT_EYE_UP,
        "no point of the pavement is {distance} from an eye {PAVEMENT_EYE_UP} above it"
    );
    let point = crcbl::math::Vec3::new(
        0.0,
        0.0,
        -distance
            .mul_add(distance, -(PAVEMENT_EYE_UP * PAVEMENT_EYE_UP))
            .sqrt(),
    );
    #[expect(
        clippy::cast_precision_loss,
        reason = "a frame extent is a few hundred pixels"
    )]
    let (width, height) = (MESH_EXTENT.0 as f32, MESH_EXTENT.1 as f32);
    let clip = camera.view_projection(width / height) * point.extend(1.0);
    assert!(
        clip.w > 0.0,
        "the pavement point at {distance} is behind the eye"
    );
    // Y-up NDC — `Projection::matrix` says so and `crcbl-vk` submits the
    // negative-height viewport that keeps it true — and row zero is the top.
    let ndc = (clip.x / clip.w, clip.y / clip.w);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the assertion below is what says the result is a pixel of this frame"
    )]
    let pixel = (
        (ndc.0.mul_add(0.5, 0.5) * width) as u32,
        ((0.5 - ndc.1 * 0.5) * height) as u32,
    );
    assert!(
        pixel.0 + 1 < MESH_EXTENT.0 && pixel.1 + 1 < MESH_EXTENT.1,
        "the pavement point at {distance} projects to {pixel:?}, which is not a pixel of a \
         {MESH_EXTENT:?} frame with a patch around it"
    );
    pixel
}

/// How far red leads blue over the 3×3 patch centred on `at`, in levels.
///
/// **The reading the tints were chosen for.** `CASCADE_TINTS`' near colour is
/// red-dominant and its far one blue-dominant, so this is positive under the
/// near cascade, negative under the far one, and between the two inside the
/// band — a difference of channels rather than of brightness, which is what
/// survives the tonemap and a pavement that is not white.
///
/// A patch rather than one pixel because the pavement is seen obliquely: one row
/// either side is a few centimetres of eye distance, far less than the band is
/// wide, and averaging over them is what stops a single sample landing on a
/// dithered edge.
fn red_over_blue(image: &crcbl_golden::Image, at: (u32, u32)) -> f32 {
    let mut total = 0.0f32;
    let mut count = 0u32;
    for y in at.1 - 1..=at.1 + 1 {
        for x in at.0 - 1..=at.0 + 1 {
            let pixel = image.pixel(x, y).expect("inside the frame");
            total += f32::from(pixel[0]) - f32::from(pixel[2]);
            count += 1;
        }
    }
    assert!(count > 0, "an empty patch measures nothing");
    #[expect(clippy::cast_precision_loss, reason = "a patch is nine pixels")]
    let mean = total / count as f32;
    mean
}

/// How far apart two neighbouring readings have to be before the difference is
/// the cascade rather than the pavement's own shading, in levels of red over
/// blue.
///
/// **Swept before it was fixed**, on both Vulkan adapters this workspace runs
/// locally — radv on an RX 7900 XTX and lavapipe — over the frame this test
/// draws. The three readings, with the view on:
///
/// | reading | eye distance | radv    | lavapipe |
/// | ------- | ------------ | ------- | -------- |
/// | near    | 3.760 m      | `+62.0` | `+62.0`  |
/// | band    | 4.464 m      | `-7.1`  | `-7.0`   |
/// | far     | 6.109 m      | `-72.0` | `-72.0`  |
///
/// so the two gaps the assertions are made on are 69.1 and 64.9 on radv and
/// 69.0 and 65.0 on lavapipe. The two adapters agree to a tenth of a level.
/// Floored at roughly half the *narrower* of them: margin for a driver that
/// tonemaps a shade differently, and nowhere near enough for a band that stepped
/// to one cascade's flat tint, which moves that gap to zero.
const CASCADE_TINT_LEVELS: f32 = 32.0;

/// How flat the same three readings have to be with the view **off**, in the
/// same levels.
///
/// The anti-vacuity constant, and it is separate from
/// [`CASCADE_TINT_LEVELS`] because it is a different measurement rather than the
/// same one loosened: what it bounds is the pavement's *own* spread across the
/// three places, which the sweep above measured at `-7.0`, `-7.0` and `-7.0` on
/// both adapters — a spread of a tenth of a level, because the surface is one
/// flat unshadowed material and nothing about it varies with distance. Four
/// levels is forty times that and still eight times under the gap the tint
/// opens, so a frame in which the *shading* ordered the three places would fail
/// here rather than quietly satisfying the assertions above.
const PAVEMENT_FLATNESS_LEVELS: f32 = 4.0;

/// **The cascade view tints each pixel by the cascade its sun shadow came from,
/// and the band between two cascades is the blend of their two tints.**
///
/// The claim `docs/plan/45-shadows.md`'s eighth decision has had no observer
/// for: the switch is a band, and a band is only a band if the picture across it
/// is a mixture rather than a step. Three readings, placed from
/// `Cascades::far[0]` rather than found by looking — well inside the near
/// cascade, half way through the band, and past it in the far cascade — have to
/// come out in that order, and the middle one strictly between its neighbours.
///
/// **Anti-vacuity, three ways.** The two tints have to differ, or every reading
/// is the same colour and the ordering is noise; the middle reading has to be
/// strictly between rather than merely not-equal, which is what a step would
/// fail; and the same three readings under the view *off* must not order
/// themselves that way, or the test is measuring the pavement's own shading and
/// would pass with the overlay deleted.
///
/// **And the view off is the frame that was always drawn.** A renderer switched
/// on and back off has to produce the frame byte for byte, which is the
/// property every golden in this workspace depends on — the lane is negative
/// precisely so that no other view's threshold sees it, and a sentinel that
/// leaked would move goldens nobody re-blessed.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_cascade_view_tints_a_pixel_by_the_cascade_its_shadow_came_from() {
    let tints = crcbl::shaders::mesh::CASCADE_TINTS;
    assert_ne!(
        tints[0], tints[1],
        "the two cascades wear one colour, so no arrangement of readings below could tell them \
         apart"
    );

    let cascades =
        crcbl::render::shadow::Cascades::new(&pavement_camera(), pavement_sun().direction);
    let reach = cascades.far[0];
    let band = reach * crcbl::shaders::mesh::CASCADE_FADE_FRACTION;
    // Two bands inside the near cascade's own edge, half way through the band,
    // and three bands past it — all three off `reach`, so a change to the split
    // scheme moves the readings with it instead of leaving them measuring
    // whatever now falls there.
    let places = [
        ("near", reach - 2.0 * band),
        ("band", reach - 0.5 * band),
        ("far", reach + 3.0 * band),
    ];

    let tinted = render_pavement(true);
    let shaded = render_pavement(false);
    let mut readings = Vec::new();
    for (name, distance) in places {
        let at = pavement_pixel(distance);
        let (on, off) = (
            red_over_blue(&tinted.image, at),
            red_over_blue(&shaded.image, at),
        );
        eprintln!(
            "{suite}: shadow — the {name} reading at {distance:.3} m (cascade reach {reach:.3}, \
             band {band:.3}) is pixel {at:?}: red over blue {on:.1} with the view on, {off:.1} \
             with it off",
            suite = crate::SUITE
        );
        readings.push((name, on, off));
    }

    let [
        (_, near_on, near_off),
        (_, band_on, band_off),
        (_, far_on, far_off),
    ] = <[(&str, f32, f32); 3]>::try_from(readings.as_slice()).expect("three readings");

    assert!(
        near_on - band_on > CASCADE_TINT_LEVELS,
        "the near cascade's reading ({near_on:.1}) is not {CASCADE_TINT_LEVELS} levels redder \
         than the band's ({band_on:.1}), so the tint is not reaching the picture"
    );
    assert!(
        band_on - far_on > CASCADE_TINT_LEVELS,
        "the band's reading ({band_on:.1}) is not {CASCADE_TINT_LEVELS} levels redder than the \
         far cascade's ({far_on:.1}), so the band is a step rather than a blend — or the far \
         cascade wears the near one's colour"
    );

    // The same three places with the view off do not order themselves that way,
    // which is what says the ordering above is the overlay and not the scene.
    assert!(
        (near_off - band_off).abs() < PAVEMENT_FLATNESS_LEVELS
            && (band_off - far_off).abs() < PAVEMENT_FLATNESS_LEVELS,
        "the shaded pavement already separates these three places — {near_off:.1}, {band_off:.1}, \
         {far_off:.1} — so the ordering above is a picture of the shading and would hold with the \
         overlay deleted"
    );

    // And switching the view off puts the frame back exactly, not nearly.
    let toggled = {
        let prepare = |renderer: &mut crcbl::render::ForwardRenderer| {
            renderer.set_cascade_view(true);
            renderer.set_cascade_view(false);
        };
        render_scene(&ShadowScene {
            prepare: &prepare,
            camera: pavement_camera(),
            sun: pavement_sun(),
            model: crcbl::math::Mat4::from_translation(crcbl::math::Vec3::new(
                0.0,
                -0.5 * PAVEMENT_SCALE,
                0.0,
            )) * crcbl::math::Mat4::from_scale(crcbl::math::Vec3::splat(PAVEMENT_SCALE)),
        })
    };
    assert_eq!(
        difference(&shaded.image, &toggled.image),
        (0, 0),
        "a renderer the cascade view was switched on and off again does not draw the frame it \
         drew before, so every golden blessed without the overlay is at risk of moving"
    );
}

// ---------------------------------------------------------------------------
// The atlas viewer
// ---------------------------------------------------------------------------
//
// `docs/plan/sample/18-sundial.md`'s milestone 1 owed one diagnostic after the
// cascade overlay: the atlas *itself* on screen. Everything above this line
// reads the atlas back on the CPU, which no reviewer can do while looking at a
// live frame — so a tile that was never rendered into, or one a light was
// refused, has only ever been visible to a test.
//
// `DebugView::ShadowAtlas` is that picture, and it is the one debug view that is
// a pass rather than a branch in `mesh.slang`. The checks below therefore have
// to hold two things a lane-reading assertion cannot: that the pass reaches the
// frame at all, and that the grey it draws is a function of the depth the CPU
// readback finds at the very texel the shader sampled — not of itself.

/// The sun's scene, with the atlas viewer on or off.
///
/// The same open box and hanging cube every check above draws, because what is
/// being looked at is the atlas rather than the scene: the cascades are filled
/// by a caster and the light region is empty, which is exactly the pair of
/// states the picture has to tell apart.
fn render_atlas_view(on: bool) -> ShadowFrame {
    let prepare = move |renderer: &mut crcbl::render::ForwardRenderer| {
        crate::mesh_scene::place(
            renderer,
            crcbl::render::scene::DEMO_OPEN_BOX,
            crcbl::render::scene::DEMO_UNTINTED,
            crcbl::math::Mat4::from_translation(BOX_AT),
        );
        renderer.set_atlas_view(on);
    };
    render_scene(&ShadowScene {
        prepare: &prepare,
        camera: overhead_camera(),
        sun: crcbl::render::DirectionalLight {
            direction: sun(1.0),
            ..crcbl::render::DirectionalLight::default()
        },
        model: crcbl::math::Mat4::from_translation(CUBE_AT),
    })
}

/// Where the atlas is drawn in this frame, in pixels: `xy` the corner and `zw`
/// the size.
///
/// The renderer's own letterbox rather than a second one — `begin_frame` writes
/// the block with this function — so a change to how the atlas is fitted moves
/// the readings below with it instead of leaving them measuring whatever now
/// falls there. The rectangles are irrelevant to it and are passed empty.
fn atlas_on_screen() -> [f32; 4] {
    crcbl::shaders::atlas_view::AtlasViewParams::letterboxed(
        MESH_EXTENT,
        crcbl::render::shadow::atlas_extent(),
        [[0.0; 4]; crcbl::shaders::mesh::SHADOW_ATLAS_TILES],
    )
    .view
}

/// Where atlas root cell `cell` is drawn in the frame: `(x, y, width, height)`
/// in pixels, right and bottom exclusive.
///
/// **A choice of where to look, not a claim about the allocator.** What every
/// grey below is asserted against is the depth the readback holds at
/// [`atlas_texel_under`] of the pixel actually read, so a cell the allocator
/// subdivided changes which texel that is and not whether the reading means
/// anything. The one place the arrangement *is* assumed is the border check,
/// which asks whether the edge of this rectangle is drawn — and that is the
/// assumption [`tile`] above already makes.
fn cell_on_screen(cell: usize) -> (u32, u32, u32, u32) {
    let view = atlas_on_screen();
    let (origin_x, origin_y) = crcbl::render::shadow::tile_origin(cell);
    let (atlas_width, atlas_height) = crcbl::render::shadow::atlas_extent();
    let side = crcbl::render::shadow::TILE;
    #[expect(
        clippy::cast_precision_loss,
        reason = "an atlas extent is a few thousand texels, and a tile's origin \
                  and side are inside it"
    )]
    let (across, down) = (atlas_width as f32, atlas_height as f32);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a tile origin and side are inside that extent"
    )]
    let (origin, span) = (
        (origin_x as f32 / across, origin_y as f32 / down),
        (side as f32 / across, side as f32 / down),
    );
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the assertion below is what says the result is a rectangle of this frame"
    )]
    let rect = (
        origin.0.mul_add(view[2], view[0]) as u32,
        origin.1.mul_add(view[3], view[1]) as u32,
        (span.0 * view[2]) as u32,
        (span.1 * view[3]) as u32,
    );
    assert!(
        rect.2 > 0
            && rect.3 > 0
            && rect.0 + rect.2 <= MESH_EXTENT.0
            && rect.1 + rect.3 <= MESH_EXTENT.1,
        "root cell {cell} lands at {rect:?}, which is not a rectangle of a {MESH_EXTENT:?} frame"
    );
    rect
}

/// The middle pixel of [`cell_on_screen`]'s rectangle.
fn cell_centre(cell: usize) -> (u32, u32) {
    let (x, y, width, height) = cell_on_screen(cell);
    (x + width / 2, y + height / 2)
}

/// The pixels of `cell`'s rectangle whose texel the readback says something was
/// drawn into, in scan order.
///
/// **A cascade is mostly empty and that is not a defect.** Its map covers the
/// whole of the camera's near range, and the scene standing in it is one box and
/// one cube — so the middle of the cell is the clear value, and a check that
/// read there would be comparing two empty tiles and passing. This is what picks
/// a pixel that is over a caster: the rule is stated, the readback decides, and
/// the count is what says the cascade drew anything at all.
fn drawn_pixels_of(atlas: &[f32], cell: usize) -> Vec<(u32, u32)> {
    let (x, y, width, height) = cell_on_screen(cell);
    let (atlas_width, _) = crcbl::render::shadow::atlas_extent();
    let mut found = Vec::new();
    for row in y..y + height {
        for column in x..x + width {
            let (tx, ty) = atlas_texel_under((column, row));
            if atlas[(ty * atlas_width + tx) as usize] > crcbl::shaders::atlas_view::DEPTH_CLEAR {
                found.push((column, row));
            }
        }
    }
    found
}

/// Which atlas texel `atlas_view.slang` reads for the pixel at `at`.
///
/// The shader's own arithmetic — a pixel *centre* mapped through the letterbox
/// and floored — so the depth compared below is the depth the fragment that drew
/// this pixel loaded, rather than the depth of a texel nearby.
fn atlas_texel_under(at: (u32, u32)) -> (u32, u32) {
    let view = atlas_on_screen();
    let (width, height) = crcbl::render::shadow::atlas_extent();
    #[expect(
        clippy::cast_precision_loss,
        reason = "a frame extent is a few hundred pixels and an atlas a few thousand texels"
    )]
    let inside = ((at.0 as f32 + 0.5) - view[0], (at.1 as f32 + 0.5) - view[1]);
    assert!(
        inside.0 >= 0.0 && inside.0 < view[2] && inside.1 >= 0.0 && inside.1 < view[3],
        "pixel {at:?} is outside the atlas's rectangle {view:?}"
    );
    #[expect(
        clippy::cast_precision_loss,
        reason = "an atlas extent is a few thousand texels"
    )]
    let extent = (width as f32, height as f32);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the product is inside the extent by the bound asserted above"
    )]
    let texel = (
        (inside.0 / view[2] * extent.0) as u32,
        (inside.1 / view[3] * extent.1) as u32,
    );
    (texel.0.min(width - 1), texel.1.min(height - 1))
}

/// The grey the viewer draws for a texel holding `depth`, as a readback level.
///
/// `atlas_view.slang`'s mapping, then the swapchain's own encode — see
/// [`crate::depth_probe::srgb_encode`], and the field on [`ShadowFrame`] that
/// says which of the two happened.
fn expected_level(depth: f32, format: Format) -> f32 {
    use crcbl::shaders::atlas_view::{DEPTH_CLEAR, EMPTY_GREY, OCCUPIED_FLOOR};
    let grey = if depth > DEPTH_CLEAR {
        OCCUPIED_FLOOR + depth * (1.0 - OCCUPIED_FLOOR)
    } else {
        EMPTY_GREY
    };
    let encoded = match format {
        Format::Rgba8UnormSrgb | Format::Bgra8UnormSrgb => crate::depth_probe::srgb_encode(grey),
        _ => grey,
    };
    encoded * 255.0
}

/// The red channel at `at`, which is the grey where the viewer drew one.
fn level_at(image: &crcbl_golden::Image, at: (u32, u32)) -> f32 {
    f32::from(image.pixel(at.0, at.1).expect("inside the frame")[0])
}

/// How far red leads blue at `at`, in levels — the reading that separates the
/// tile borders from every grey in the picture.
fn tint_at(image: &crcbl_golden::Image, at: (u32, u32)) -> f32 {
    let pixel = image.pixel(at.0, at.1).expect("inside the frame");
    f32::from(pixel[0]) - f32::from(pixel[2])
}

/// Which root cell the sun's near cascade is drawn into, and which one nothing
/// is.
///
/// The sun's scene lights no punctual light at all, so every slot of the atlas's
/// light region is free — [`EMPTY_CELL`] is one of them, chosen away from the
/// top row so a reading there cannot be a cascade the allocator moved.
const CASCADE_CELL: usize = 0;

/// See [`CASCADE_CELL`].
const EMPTY_CELL: usize = 5;

/// How many of the near cascade's pixels have to be over a texel a caster wrote.
///
/// **Swept before it was fixed**, with the rest of this check's constants; the
/// table is beside [`ATLAS_GREY_LEVELS`]. It is the anti-vacuity floor under
/// [`drawn_pixels_of`]'s pick: one such pixel would let a cascade that drew a
/// single stray texel satisfy every reading below, and the box and the cube
/// standing in this cascade cover far more of it than that. Floored at roughly
/// half the measured count.
const CASCADE_DRAWN_PIXELS: usize = 32;

/// How far apart an occupied texel's grey and an empty one's have to be, in
/// levels.
///
/// **Swept before it was fixed**, on both Vulkan adapters this workspace runs
/// locally — radv on an RX 7900 XTX and lavapipe — over the frame this test
/// draws, into an `Rgba8UnormSrgb` swapchain:
///
/// | reading                            | radv        | lavapipe    |
/// | ---------------------------------- | ----------- | ----------- |
/// | pixels of cell 0 over a caster     | `77`        | `77`        |
/// | depth at the pixel picked          | `0.0413146` | `0.0413142` |
/// | grey drawn there / depth predicts  | `155.0` / `155.2` | `155.0` / `155.2` |
/// | grey over the empty slot / predicts | `69.0` / `69.3` | `69.0` / `69.3` |
/// | border's red over blue             | `147.0`     | `147.0`     |
/// | either grey's red over blue        | `0.0`       | `0.0`       |
/// | the surround                       | `0.0`       | `0.0`       |
///
/// so the gap this constant floors is `86.0` on both, and the two adapters agree
/// to the level everywhere. Floored at roughly half of it: margin for a driver
/// that rounds an encode differently, and nowhere near enough for a viewer that
/// drew every texel at one grey, which moves the gap to zero.
const ATLAS_GREY_LEVELS: f32 = 40.0;

/// How close the grey the picture drew has to be to the grey the readback's own
/// depth predicts, in levels.
///
/// The tolerance on the *cross-check* rather than on the comparison above, and
/// it is what holds the mapping to the real depth: a viewer whose grey ignored
/// the depth entirely would still open a gap against an empty tile and would
/// land nowhere near the value this predicts. The sweep above measured `0.2` and
/// `0.3`; two levels is the rounding of one 8-bit quantisation and one transfer
/// function, and a tenth of the gap the mapping actually spans.
const ATLAS_LEVEL_TOLERANCE: f32 = 2.0;

/// How far red must lead blue for a pixel to be one of the tile borders.
///
/// `BORDER_TINT` is amber and every other texel of the picture is a grey, so the
/// two are not on one axis at all — the sweep above measured `147.0` on the
/// border and `0.0` on both greys. Floored at roughly half the lead, which is
/// still far above anything a grey can produce.
///
/// [`the_atlas_view_borders_a_subdivided_slot_at_its_own_size`] reads a
/// quarter-cell's edges through this same constant and measured the same pair on
/// both adapters — `147.0` on the border it asserts and `0.0` at every pixel it
/// asserts clear of one — so the two frames put the same distance between the
/// tint and the greys whatever size the tile is.
const ATLAS_BORDER_LEVELS: f32 = 70.0;

/// **The atlas viewer draws each atlas texel's stored depth, borders the slots
/// that hold a map, and leaves the frame alone when it is off.**
///
/// The claim `docs/plan/sample/18-sundial.md`'s milestone 1 has had no observer
/// for. Four readings, each placed from the atlas's own geometry rather than
/// found by looking:
///
/// * the middle of the near cascade's cell, whose texel the readback says a
///   caster wrote — a grey the mapping predicts from that very depth;
/// * the middle of a cell in the light region, which this scene lights nothing
///   into, so its texels are still the reversed-Z clear;
/// * a pixel on the cascade cell's own edge, which has to be the border tint and
///   therefore off the grey axis entirely;
/// * a pixel outside the letterbox, which is the surround.
///
/// **Anti-vacuity.** The two greys are compared against each other *and* against
/// what the readback's depth predicts, so a viewer that drew one flat grey fails
/// the first and one that drew a plausible ramp of its own fails the second; the
/// border is asserted on a channel difference no grey can produce; and the same
/// frame with the view off must come back byte for byte, or the pass is reaching
/// frames nobody asked it to.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_atlas_view_draws_the_stored_depth_and_borders_the_slots_that_hold_a_map() {
    let viewed = render_atlas_view(true);
    let Some(atlas) = viewed.atlas() else {
        return;
    };
    let (atlas_width, _) = crcbl::render::shadow::atlas_extent();

    let depth_under = |at: (u32, u32)| {
        let (x, y) = atlas_texel_under(at);
        atlas[(y * atlas_width + x) as usize]
    };

    let drawn = drawn_pixels_of(atlas, CASCADE_CELL);
    let occupied_at = *drawn.first().unwrap_or_else(|| {
        panic!(
            "no pixel of root cell {CASCADE_CELL} is over a texel a caster wrote, so the near \
             cascade drew nothing this frame and there is no occupied tile to look at"
        )
    });
    assert!(
        drawn.len() >= CASCADE_DRAWN_PIXELS,
        "only {} of root cell {CASCADE_CELL}'s pixels are over a caster, which is fewer than \
         the {CASCADE_DRAWN_PIXELS} this scene draws — the reading below would be a single \
         texel's accident",
        drawn.len()
    );
    let empty_at = cell_centre(EMPTY_CELL);
    assert!(
        drawn_pixels_of(atlas, EMPTY_CELL).is_empty(),
        "root cell {EMPTY_CELL} holds a map, so this scene is not the two-state fixture this \
         check needs"
    );
    let (occupied_depth, empty_depth) = (depth_under(occupied_at), depth_under(empty_at));
    eprintln!(
        "{suite}: shadow — the atlas is drawn at {view:?}; {drawn} of root cell {CASCADE_CELL}'s \
         pixels are over a caster, the first at {occupied_at:?} over texel {occupied_texel:?} \
         holding depth {occupied_depth}; cell {EMPTY_CELL} is pixel {empty_at:?} over texel \
         {empty_texel:?} holding depth {empty_depth}",
        suite = crate::SUITE,
        view = atlas_on_screen(),
        drawn = drawn.len(),
        occupied_texel = atlas_texel_under(occupied_at),
        empty_texel = atlas_texel_under(empty_at),
    );
    assert_eq!(
        empty_depth,
        crcbl::shaders::atlas_view::DEPTH_CLEAR,
        "a slot in the light region holds a depth, so this scene is not the two-state fixture \
         this check needs"
    );

    let (occupied, empty) = (
        level_at(&viewed.image, occupied_at),
        level_at(&viewed.image, empty_at),
    );
    let (cascade_x, cascade_y, cascade_width, _) = cell_on_screen(CASCADE_CELL);
    let border_at = (cascade_x + cascade_width / 2, cascade_y);
    let surround_at = (0, MESH_EXTENT.1 / 2);
    eprintln!(
        "{suite}: shadow — the viewer drew {occupied:.1} over the caster (the depth predicts \
         {want_occupied:.1}) and {empty:.1} over the empty slot (predicting {want_empty:.1}); \
         the border at {border_at:?} leads red over blue by {border:.1}, the two greys by \
         {occupied_tint:.1} and {empty_tint:.1}, and the surround at {surround_at:?} is \
         {surround:.1}",
        suite = crate::SUITE,
        want_occupied = expected_level(occupied_depth, viewed.format),
        want_empty = expected_level(empty_depth, viewed.format),
        border = tint_at(&viewed.image, border_at),
        occupied_tint = tint_at(&viewed.image, occupied_at),
        empty_tint = tint_at(&viewed.image, empty_at),
        surround = level_at(&viewed.image, surround_at),
    );

    assert!(
        occupied - empty > ATLAS_GREY_LEVELS,
        "the caster's texel drew {occupied:.1} and the empty slot's {empty:.1}, which is not \
         {ATLAS_GREY_LEVELS} levels apart — so the picture cannot tell a tile that holds a map \
         from one that does not"
    );
    // And the grey is the one the depth in the atlas predicts, which is what
    // holds the mapping to the evidence rather than to itself.
    for (name, at, depth, drawn) in [
        ("the caster's", occupied_at, occupied_depth, occupied),
        ("the empty slot's", empty_at, empty_depth, empty),
    ] {
        let want = expected_level(depth, viewed.format);
        assert!(
            (drawn - want).abs() <= ATLAS_LEVEL_TOLERANCE,
            "{name} texel at {at:?} holds depth {depth}, which the viewer's mapping draws as \
             {want:.1}, and the picture has {drawn:.1}"
        );
    }

    // The tile grid, on the edge of the cell the cascade was drawn into.
    assert!(
        tint_at(&viewed.image, border_at) > ATLAS_BORDER_LEVELS,
        "the edge of the occupied cell is not the border tint, so the picture has no tile grid \
         and slot assignment is as unreadable as it was"
    );
    for (name, at) in [
        ("the caster's", occupied_at),
        ("the empty slot's", empty_at),
    ] {
        assert!(
            tint_at(&viewed.image, at).abs() < ATLAS_BORDER_LEVELS,
            "{name} texel is on the border's own colour axis, so the reading above cannot say \
             which of the two it found"
        );
    }
    assert_eq!(
        level_at(&viewed.image, surround_at),
        0.0,
        "the frame outside the atlas's rectangle is not the surround, so the letterbox is not \
         where this check thinks it is"
    );

    // And switching the view off puts the frame back exactly, not nearly.
    let shaded = render_atlas_view(false);
    let toggled = {
        let prepare = |renderer: &mut crcbl::render::ForwardRenderer| {
            crate::mesh_scene::place(
                renderer,
                crcbl::render::scene::DEMO_OPEN_BOX,
                crcbl::render::scene::DEMO_UNTINTED,
                crcbl::math::Mat4::from_translation(BOX_AT),
            );
            renderer.set_atlas_view(true);
            renderer.set_atlas_view(false);
        };
        render_scene(&ShadowScene {
            prepare: &prepare,
            camera: overhead_camera(),
            sun: crcbl::render::DirectionalLight {
                direction: sun(1.0),
                ..crcbl::render::DirectionalLight::default()
            },
            model: crcbl::math::Mat4::from_translation(CUBE_AT),
        })
    };
    assert_eq!(
        difference(&shaded.image, &toggled.image),
        (0, 0),
        "a renderer the atlas view was switched on and off again does not draw the frame it \
         drew before, so every golden blessed without the viewer is at risk of moving"
    );
}

// ---------------------------------------------------------------------------
// The atlas viewer over a cell the allocator subdivided
// ---------------------------------------------------------------------------
//
// Every map in the frame above takes a whole root cell, so every border in that
// picture sits on `tile_origin`'s grid — and a viewer that read no rectangle at
// all, drawing the grid it can derive from `atlas_extent` and `TILE` alone,
// would satisfy each of its readings unchanged.
//
// `docs/plan/45-shadows.md`'s atlas rung is what makes that a distinction: since
// it, a map takes a quarter of a cell or a sixteenth whenever
// `shadow::tile_level` says its coverage does not earn a whole one, and
// `atlas_view.slang`'s border loop reads each slot's own
// `FrameUniforms::shadow_atlas_rect` so the border lands on that map's real
// edges. The scene below is the first in this module to lay one out.

/// Where the spot that earns a subdivided tile hangs: over the box, aimed
/// straight down at its floor.
///
/// Inside the scene rather than out beside the camera, so the map in the
/// quarter cell is a map of this box. What demotes it is the **camera's**
/// distance, which is the term `shadow::coverage` divides by.
const SUBDIVIDED_SPOT_AT: crcbl::math::Vec3 = crcbl::math::Vec3::new(0.0, 1.2, 0.0);

/// How far above the box that scene's camera stands.
///
/// **Swept before it was fixed.** A light's coverage is its map's extent over
/// its distance from the eye, so with the cone below fixed this height alone
/// decides which rung of `shadow::tile_level`'s ladder the spot lands on. The
/// level is host arithmetic — no adapter is in it — and this is what
/// `Selection::atlas_rect` answered for this scene's light at each height:
///
/// | height  | the tile the spot is given |
/// | ------- | -------------------------- |
/// | `10.64` | a whole root cell          |
/// | `10.66` | a quarter of one           |
/// | `14.5`  | a quarter of one           |
/// | `20.08` | a quarter of one           |
/// | `20.10` | a sixteenth                |
///
/// so the band that halves the tile exactly once runs from just under `10.66`
/// to just over `20.08`. This stands at the geometric middle of the two
/// distances, which is the middle of the band: the ladder's rungs are a
/// geometric series, so that is where the margin either way is equal.
const SUBDIVIDED_CAMERA_UP: f32 = 14.5;

/// The cone [`SUBDIVIDED_CAMERA_UP`] demotes.
///
/// Its own numbers rather than [`spot_light`]'s, and deliberately: what this
/// fixture needs is a map whose extent puts it on a known rung of the ladder,
/// and a light shared with the spot checks below would move this one's tile size
/// whenever those were re-aimed.
fn subdivided_spot() -> crcbl::render::Light {
    crcbl::render::Light::Spot(crcbl::render::SpotLight {
        position: SUBDIVIDED_SPOT_AT,
        radius: 3.4,
        color: crcbl::math::Vec3::new(1.0, 0.95, 0.85) * 5.0,
        direction: -crcbl::math::Vec3::Y,
        inner_angle: 0.18,
        outer_angle: 0.28,
        fill: false,
    })
}

/// A camera straight down over the box from [`SUBDIVIDED_CAMERA_UP`].
///
/// Not [`overhead_camera`] lifted: that one keeps `up` at `+Y` while looking
/// very nearly down `-Y`, which is a basis two parallel vectors cannot span once
/// the eye is this far off the floor. `+Z` is the up every straight-down camera
/// in this module takes — see [`spot_camera`].
fn subdivided_camera() -> crcbl::render::Camera {
    crcbl::render::Camera {
        eye: BOX_AT + crcbl::math::Vec3::new(0.0, SUBDIVIDED_CAMERA_UP, 0.0),
        target: BOX_AT,
        up: crcbl::math::Vec3::Z,
        projection: crcbl::render::Projection::default(),
    }
}

/// The atlas viewer over a frame whose one shadowed light was given a quarter
/// of a root cell.
fn render_subdivided_atlas_view() -> ShadowFrame {
    let prepare = |renderer: &mut crcbl::render::ForwardRenderer| {
        crate::mesh_scene::place(
            renderer,
            crcbl::render::scene::DEMO_OPEN_BOX,
            crcbl::render::scene::DEMO_UNTINTED,
            crcbl::math::Mat4::from_translation(BOX_AT),
        );
        renderer.set_lights(&[subdivided_spot()]);
        renderer.set_atlas_view(true);
    };
    render_scene(&ShadowScene {
        prepare: &prepare,
        camera: subdivided_camera(),
        sun: crcbl::render::DirectionalLight {
            direction: sun(1.0),
            ..crcbl::render::DirectionalLight::default()
        },
        model: crcbl::math::Mat4::from_translation(CUBE_AT),
    })
}

/// A whole root cell as the uniform block spells a rectangle: `TileRect::to_uv`
/// of a tile of `TILE` texels, on each axis of the atlas.
///
/// What the anti-vacuity assertion below compares against, and the reason it is
/// derived here rather than written down: the block carries fractions of the
/// atlas, so the size of a cell in it moves with both `TILE` and
/// `atlas_extent`.
fn whole_cell_rect() -> (f32, f32) {
    let (width, height) = crcbl::render::shadow::atlas_extent();
    #[expect(
        clippy::cast_precision_loss,
        reason = "an atlas extent is a few thousand texels and a tile's side is inside it"
    )]
    let cell = (
        crcbl::render::shadow::TILE as f32 / width as f32,
        crcbl::render::shadow::TILE as f32 / height as f32,
    );
    cell
}

/// Where the map whose rectangle is `rect` is drawn in the frame:
/// `(x, y, width, height)` in pixels, right and bottom exclusive.
///
/// `atlas_view.slang`'s own arithmetic on the rectangle the uniform block
/// carries — `rect.zw * view.zw` for the corner and `rect.xy * view.zw` for the
/// size — so a reading placed from this is placed where the shader was told the
/// map is. That is the whole difference from [`cell_on_screen`], which takes the
/// grid's own cell and knows nothing about what the allocator did inside it.
fn slot_on_screen(rect: [f32; 4]) -> (u32, u32, u32, u32) {
    let view = atlas_on_screen();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the assertion below is what says the result is a rectangle of this frame"
    )]
    let at = (
        rect[2].mul_add(view[2], view[0]) as u32,
        rect[3].mul_add(view[3], view[1]) as u32,
        (rect[0] * view[2]) as u32,
        (rect[1] * view[3]) as u32,
    );
    assert!(
        at.2 > 0 && at.3 > 0 && at.0 + at.2 <= MESH_EXTENT.0 && at.1 + at.3 <= MESH_EXTENT.1,
        "the map at {rect:?} lands at {at:?}, which is not a rectangle of a {MESH_EXTENT:?} frame"
    );
    at
}

/// Which slot's rectangle covers the frame pixel at `at`, if any.
///
/// `atlas_view.slang`'s own containment test, run over the whole block — the
/// same one whose `continue` decides which slot's border a pixel may be. What it
/// is for is the readings that assert **no** border: a pixel some other map was
/// laid out under would draw that map's border, and "no border here" would then
/// be a claim about the fixture rather than about the viewer.
fn slot_covering(
    rects: &[[f32; 4]; crcbl::render::shadow::TILES],
    at: (u32, u32),
) -> Option<usize> {
    let view = atlas_on_screen();
    #[expect(
        clippy::cast_precision_loss,
        reason = "a frame extent is a few hundred pixels"
    )]
    let inside = ((at.0 as f32 + 0.5) - view[0], (at.1 as f32 + 0.5) - view[1]);
    rects.iter().position(|rect| {
        if rect[0] <= 0.0 || rect[1] <= 0.0 {
            return false;
        }
        let min = (rect[2] * view[2], rect[3] * view[3]);
        let max = (min.0 + rect[0] * view[2], min.1 + rect[1] * view[3]);
        inside.0 >= min.0 && inside.0 < max.0 && inside.1 >= min.1 && inside.1 < max.1
    })
}

/// **The viewer borders a map the allocator subdivided at the map's own size,
/// not at its root cell's.**
///
/// [`the_atlas_view_draws_the_stored_depth_and_borders_the_slots_that_hold_a_map`]
/// reads two whole root cells, so every border it looks at falls on
/// [`tile_origin`]'s grid and a viewer that never read
/// `FrameUniforms::shadow_atlas_rect` would pass it. This frame is the one that
/// tells the two apart: its spot stands [`SUBDIVIDED_CAMERA_UP`] from the eye,
/// which is the middle of the band `shadow::tile_level` gives a quarter of a
/// cell, so the map is bordered inside a cell rather than around one.
///
/// Every reading is placed off the rectangle [`ShadowFrame::rects`] carries:
///
/// * the quarter's far edges, which lie in the *middle* of its root cell, must
///   be the border tint;
/// * its own centre must not be, or a viewer that filled the tile amber would
///   answer the first reading too;
/// * a pixel past its far edge must not be, which is what says the border stops
///   where the rectangle does;
/// * and the root cell's own far edges must not be — that is the reading a
///   viewer bordering whole cells draws, and this one may not.
///
/// **Anti-vacuity.** The rectangle is asserted smaller than a whole root cell
/// before anything is read through it, so a frame whose maps all took whole
/// cells fails here rather than passing by reading a cell's edge under another
/// name; the tile is asserted wider than the border it draws on both sides, so
/// the centre reading is a pixel the border does not reach; and each pixel
/// asserted clear of the tint outside the map is asserted to lie in no slot's
/// rectangle at all, so it cannot be a neighbouring map's border read as this
/// one's absence.
///
/// **No atlas readback.** What this check reads is the rectangle the block
/// carries and the pixels the pass drew, neither of which is a depth — so unlike
/// every other check in this section it also runs on a backend that declares
/// [`Capability::DepthImageCopy`] absent.
///
/// # How it was shown to fail
///
/// **By the viewer itself**, which is the only thing that can red the last
/// reading: `atlas_view.slang`'s border loop was given a second test that tints
/// the edges of the root cell a rectangle sits in, the artifacts regenerated,
/// and the run read on radv
///
/// > the root cell's right edge at (175, 24) carries the border tint, and no
/// > map was laid out there — so the viewer is drawing the grid's own cells
/// > rather than the rectangles the block carries
///
/// with every reading before it still green, since that viewer borders the
/// map's own edges too. The rest went red from the test's side: the camera
/// lowered until the spot earned a whole cell, the border read at the root
/// cell's rectangle, the centre and the outside readings moved onto and off the
/// map, and a second shadowed light in the scene, each refused by the guard
/// written for it.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn the_atlas_view_borders_a_subdivided_slot_at_its_own_size() {
    let viewed = render_subdivided_atlas_view();

    // The light region's occupied slots. This scene lights one shadowable light,
    // so there is one — and finding it here rather than naming a slot is what
    // keeps the check on the allocator's answer instead of on a guess at it.
    let held: Vec<usize> = (crcbl::render::shadow::CASCADES..crcbl::render::shadow::TILES)
        .filter(|slot| viewed.rects[*slot][0] > 0.0)
        .collect();
    assert_eq!(
        held.len(),
        1,
        "{count} slots of the light region hold a map, and this check is written for the one \
         this scene's spot earns: {rects:?}",
        count = held.len(),
        rects = viewed.rects,
    );
    let slot = held[0];
    let rect = viewed.rects[slot];
    let cell = whole_cell_rect();
    assert!(
        rect[0] < cell.0 && rect[1] < cell.1,
        "slot {slot}'s rectangle {rect:?} is a whole root cell of the atlas, which is {cell:?} \
         in these units — so the ladder gave this scene's spot no halving and every reading \
         below would be reading the grid again"
    );

    let (x, y, width, height) = slot_on_screen(rect);
    // The cell the allocator subdivided: the one `tile_origin` puts this
    // rectangle's corner in.
    let root = {
        let (atlas_width, atlas_height) = crcbl::render::shadow::atlas_extent();
        #[expect(
            clippy::cast_precision_loss,
            reason = "an atlas extent is a few thousand texels"
        )]
        let extent = (atlas_width as f32, atlas_height as f32);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the rectangle is inside the atlas, so its corner is inside the grid"
        )]
        let (column, row) = {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a tile's side is a few hundred texels"
            )]
            let side = crcbl::render::shadow::TILE as f32;
            (
                (rect[2] * extent.0 / side) as usize,
                (rect[3] * extent.1 / side) as usize,
            )
        };
        row * crcbl::render::shadow::ATLAS_COLUMNS as usize + column
    };
    let (cell_x, cell_y, cell_width, cell_height) = cell_on_screen(root);

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the border is a couple of pixels wide"
    )]
    let border = crcbl::shaders::atlas_view::BORDER_PIXELS as u32;
    assert!(
        width > 2 * border && height > 2 * border,
        "the subdivided map is {width}x{height} pixels of the frame, which the border drawn on \
         both of its sides covers entirely — the centre reading below would be a border pixel \
         and could not say the tile is not simply filled"
    );

    let bordered = [
        ("the map's right edge", (x + width - 1, y + height / 2)),
        ("the map's bottom edge", (x + width / 2, y + height - 1)),
    ];
    let centre = (x + width / 2, y + height / 2);
    let outside = [
        (
            "just past the map's right edge",
            (x + width + border, y + height / 2),
        ),
        (
            "the root cell's right edge",
            (cell_x + cell_width - 1, cell_y + cell_height / 2),
        ),
        (
            "the root cell's bottom edge",
            (cell_x + cell_width / 2, cell_y + cell_height - 1),
        ),
    ];
    eprintln!(
        "{suite}: shadow — the atlas is drawn at {view:?}; slot {slot} holds {rect:?}, which is \
         {width}x{height} pixels at ({x}, {y}) inside root cell {root}'s {cell:?}. The map's \
         edges lead red over blue by {right:.1} at {right_at:?} and {bottom:.1} at \
         {bottom_at:?}; its centre {centre:?} by {middle:.1}, and {outside:?}",
        suite = crate::SUITE,
        view = atlas_on_screen(),
        cell = (cell_x, cell_y, cell_width, cell_height),
        right = tint_at(&viewed.image, bordered[0].1),
        right_at = bordered[0].1,
        bottom = tint_at(&viewed.image, bordered[1].1),
        bottom_at = bordered[1].1,
        middle = tint_at(&viewed.image, centre),
        outside = outside.map(|(name, at)| (name, at, tint_at(&viewed.image, at))),
    );

    for (name, at) in bordered {
        assert!(
            tint_at(&viewed.image, at) > ATLAS_BORDER_LEVELS,
            "{name} at {at:?} is not the border tint, so the viewer is not bordering the \
             rectangle the block carries — this pixel is in the middle of root cell {root}, \
             which a grid drawn on whole cells leaves grey"
        );
    }
    assert_eq!(
        slot_covering(&viewed.rects, centre),
        Some(slot),
        "the centre reading at {centre:?} is not inside slot {slot}'s own rectangle, so it \
         cannot say whether the viewer filled the tile"
    );
    assert!(
        tint_at(&viewed.image, centre).abs() < ATLAS_BORDER_LEVELS,
        "the middle of the subdivided map at {centre:?} carries the border tint, so the viewer \
         filled the tile rather than bordering it and the edge readings above say nothing"
    );
    for (name, at) in outside {
        assert_eq!(
            slot_covering(&viewed.rects, at),
            None,
            "{name} at {at:?} is inside a slot's rectangle, so this scene is not the fixture \
             the reading below needs: {rects:?}",
            rects = viewed.rects,
        );
        assert!(
            tint_at(&viewed.image, at).abs() < ATLAS_BORDER_LEVELS,
            "{name} at {at:?} carries the border tint, and no map was laid out there — so the \
             viewer is drawing the grid's own cells rather than the rectangles the block \
             carries, which is exactly the picture a subdivided cell must not produce"
        );
    }
}

// ---------------------------------------------------------------------------
// The shadowed spot
// ---------------------------------------------------------------------------
//
// `docs/plan/18-render-features.md`'s 2026-08-13 slice: a light other than the
// sun that occludes. The scene is `crcbl::screenshot`'s `Scene::SpotShadow` in
// every number that matters — the same floor, the same light, the same camera —
// so a failure here and a moved golden there are the same failure seen twice
// rather than two scenes to keep in step.
//
// What this module adds that the golden cannot is **motion**: the golden is one
// frame, and a shadow painted at a fixed place would match it for ever. Moving
// the caster and watching the dark region follow is the only assertion that
// separates a shadow map from a decal.

/// How far above the floor the spot hangs, and how far towards `+Z`.
///
/// 45° from vertical. `crcbl::screenshot`'s `SPOT_SHADOW_LIGHT_AT` carries the
/// argument for the angle: with the camera overhead and the light overhead too,
/// a shadow falls under the object that casts it and the object's own image
/// covers it.
const SPOT_LIGHT_AT: crcbl::math::Vec3 = crcbl::math::Vec3::new(0.0, 1.2, 1.2);

/// How far above the floor the camera stands, looking straight down.
const SPOT_CAMERA_UP: f32 = 1.3;

/// How much the pyramid is scaled by to get the caster, and how far its base is
/// lifted so it stands on the floor.
const SPOT_CASTER_SCALE: f32 = 0.5;

/// How far the caster is moved off the frame's axis, in world units.
///
/// Far enough that its shadow lands wholly inside one of the two bands measured
/// below, and inside the cone's pool at that distance from the axis — see
/// `SPOT_BAND_COLUMNS`.
const SPOT_CASTER_OFFSET: f32 = 0.30;

/// How much the cube is scaled by to get the floor, and how far it is dropped so
/// its `+Y` face is the plane `y = 0`.
///
/// `crcbl::screenshot`'s `SPOT_FLOOR_SCALE`, and large enough here for the same
/// reason: the floor runs past every edge of the frame, so there is no
/// silhouette anywhere for a band to measure instead of the floor.
const SPOT_FLOOR_SCALE: f32 = 8.0;

/// The rows the bands below are measured over: level with the shadow, a third of
/// the way down from the frame's centre.
///
/// The light comes from `+Z` and `+Z` is the top of the frame, so the shadow
/// falls *down* it. The caster's own image ends about 26 rows below centre and
/// its shadow reaches about 92; this band sits between them.
const SPOT_BAND_ROWS: std::ops::Range<u32> = (MESH_EXTENT.1 / 2 + 36)..(MESH_EXTENT.1 / 2 + 48);

/// How far either side of the frame's axis each band sits, in columns.
///
/// The caster at `SPOT_CASTER_OFFSET` throws its shadow over roughly columns
/// 153 to 191 in this band's rows, and the cone's pool reaches to about column
/// 192 — so a band inside 162..174 is inside the shadow and inside the pool at
/// once, and its mirror is lit floor at the same distance from the cone's axis.
const SPOT_BAND_COLUMNS: (u32, u32) = (34, 46);

/// The spot this suite lights its floor with, on `Scene::SpotShadow`'s terms.
fn spot_light() -> crcbl::render::Light {
    crcbl::render::Light::Spot(crcbl::render::SpotLight {
        position: SPOT_LIGHT_AT,
        radius: 3.4,
        color: crcbl::math::Vec3::new(1.0, 0.95, 0.85) * 5.0,
        // Along the cone, away from the light: from the light to the floor's
        // centre.
        direction: -SPOT_LIGHT_AT,
        inner_angle: 0.18,
        outer_angle: 0.28,
        fill: false,
    })
}

/// A camera straight down over the floor's centre.
fn spot_camera() -> crcbl::render::Camera {
    crcbl::render::Camera {
        eye: crcbl::math::Vec3::new(0.0, SPOT_CAMERA_UP, 0.0),
        target: crcbl::math::Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the
        // direction the light comes from at the top of the frame.
        up: crcbl::math::Vec3::Z,
        projection: crcbl::render::Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// Draws the floor under the spot, with the caster at `caster` on the `x` axis —
/// or with no caster at all.
fn render_spot(caster: Option<f32>) -> ShadowFrame {
    render_scene(&ShadowScene {
        prepare: &move |renderer| {
            renderer.set_lights(&[spot_light()]);
            if let Some(x) = caster {
                // The pyramid's base is at `-0.4` in its own space, so lifting
                // it by that much of the scale stands it on `y = 0`. A caster
                // floating above the floor would hide a shadow detached from it,
                // which is what too much bias looks like.
                crate::mesh_scene::place(
                    renderer,
                    crcbl::render::scene::DEMO_PYRAMID,
                    crcbl::render::scene::DEMO_UNTINTED,
                    crcbl::math::Mat4::from_translation(crcbl::math::Vec3::new(
                        x,
                        0.4 * SPOT_CASTER_SCALE,
                        0.0,
                    )) * crcbl::math::Mat4::from_scale(crcbl::math::Vec3::splat(SPOT_CASTER_SCALE)),
                );
            }
        },
        camera: spot_camera(),
        // Dim, so the pool and the shadow in it are the spot's work: a bright
        // sun would light the shadowed floor from a direction the spot's map
        // knows nothing about, and the ratio below would be measuring the sun.
        sun: crcbl::render::DirectionalLight {
            color: crcbl::render::DirectionalLight::default().color * 0.03,
            ambient: crcbl::render::DirectionalLight::default().ambient * 0.09,
            ..crcbl::render::DirectionalLight::default()
        },
        // The cube, scaled into a floor whose `+Y` face is the plane `y = 0`.
        model: crcbl::math::Mat4::from_translation(crcbl::math::Vec3::new(
            0.0,
            -0.5 * SPOT_FLOOR_SCALE,
            0.0,
        )) * crcbl::math::Mat4::from_scale(crcbl::math::Vec3::splat(SPOT_FLOOR_SCALE)),
    })
}

/// The mean brightness of the band `SPOT_BAND_COLUMNS` from the frame's axis, on
/// the **world** `+X` side when `sign` is positive.
///
/// **World `+X` is the frame's left.** The camera looks down `-Y` with `+Z` up,
/// and a right-handed basis built from those two puts screen-right at `-X`:
/// `cross((0,-1,0), (0,0,1))` is `(-1,0,0)`. So the flip is here rather than in
/// every caller, and the callers can say "the band the caster is over" and mean
/// it.
fn spot_band(frame: &ShadowFrame, sign: i32) -> f32 {
    let centre = i32::try_from(MESH_EXTENT.0 / 2).expect("a frame edge fits in an i32");
    let near = i32::try_from(SPOT_BAND_COLUMNS.0).expect("a band offset fits in an i32");
    let far = i32::try_from(SPOT_BAND_COLUMNS.1).expect("a band offset fits in an i32");
    let columns = if sign >= 0 {
        (centre - far)..(centre - near)
    } else {
        (centre + near)..(centre + far)
    };
    let columns = u32::try_from(columns.start).expect("inside the frame")
        ..u32::try_from(columns.end).expect("inside the frame");
    let mut total = 0.0f32;
    let mut count = 0u32;
    for y in SPOT_BAND_ROWS {
        for x in columns.clone() {
            let pixel = frame.image.pixel(x, y).expect("inside the frame");
            total += f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2]);
            count += 1;
        }
    }
    assert!(count > 0, "an empty band measures nothing");
    total / (count as f32 * 3.0)
}

/// How much brighter the lit band must be than the shadowed one.
///
/// A ratio rather than a difference, on `a_wall_darkens_the_floor_it_stands_on`'s
/// terms: what survives Lambert, the falloff, the cone and the tonemap is which
/// side leads and by how much in proportion. The two bands are the same distance
/// from the cone's axis, so every term but the shadow has the same value in
/// both.
const SPOT_SHADOW_RATIO: f32 = 1.5;

/// **A spot's shadow map is written, and it is the map the slot says it is.**
///
/// The tile a shadowed light was given must hold real depths, and the *other*
/// light tile must not: a viewport that ignored `shadow::tile_origin` would
/// write the cascades' tiles or the free one, and every one of those renders a
/// frame that looks entirely plausible.
///
/// The pass is one `LoadOp::Clear` over the whole atlas, so a tile nothing drew
/// into is exactly `depth::CLEAR` — which is what makes "is it at the clear" a
/// question with a crisp answer rather than a threshold.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_shadowed_spot_fills_the_tile_it_was_given_and_no_other() {
    let frame = render_spot(Some(0.0));
    // As `the_shadow_atlas_is_written_rather_than_left_at_its_clear_value`: a
    // backend that declares no depth-image copy was held to that refusal in
    // `render_scene`, and has no atlas for the tiles below.
    let Some(atlas) = frame.atlas() else {
        return;
    };
    let held = tile(atlas, crcbl::render::shadow::light_tile(0));
    let written = held.iter().filter(|depth| **depth > 0.0).count();
    let fraction = written as f64 / held.len() as f64;
    eprintln!(
        "crcbl forward e2e: spot shadow — slot 0's tile holds {written} written texel(s) ({fraction:.4})"
    );
    assert!(
        written > 0,
        "the one shadowed light's tile is entirely at the reversed-Z clear, so its map was \
         never rendered — which is a spot that shadows nothing and a frame that looks correct"
    );
    // **A real area, not a stray texel.** The caster alone covers about a sixth
    // of this tile — the cone's frustum is barely wider than the pyramid at the
    // depth it stands at — so a sliver is a mis-transformed caster and this
    // floor is well under what a correct one writes.
    //
    // It is deliberately not "most of the tile", even though the floor runs past
    // every edge of the cone and geometrically covers all of it. On a device
    // with an amplification stage the floor's clusters are rejected before they
    // are drawn, because `cluster_survives` transforms a cluster's bounding
    // *centre* by the instance transform and leaves its **radius alone** — which
    // is correct for the rigid transform `GpuInstance::transform` documents and
    // eight times too small for a cube scaled into a floor. Nothing in the
    // picture changes: a receiver missing from its own shadow map is a receiver
    // that is not self-shadowed, which is what a correct bias produces anyway,
    // and `crcbl`'s `the_spot_shadow_scene_draws_the_same_frame_on_every_geometry_path`
    // is what says the two paths agree pixel for pixel. It is in
    // `docs/backlog.md` rather than fixed here.
    assert!(
        fraction > 0.05,
        "slot 0's tile wrote only {fraction:.4} of its texels, which is a sliver rather than \
         a caster"
    );
    assert!(
        held.iter().all(|depth| (0.0..=1.0).contains(depth)),
        "slot 0's tile holds a depth outside 0..1, so its reversed-Z range is not the one \
         the comparison sampler tests against"
    );
    // And every other light tile is untouched, which is what says the viewport
    // went where `tile_origin` put it rather than one tile along — and, since a
    // spot is one tile of a region six wide, that a spot did not render the five
    // faces it does not have.
    for light_tile in 1..crcbl::render::shadow::LIGHT_TILES {
        let free = tile(atlas, crcbl::render::shadow::light_tile(light_tile));
        assert!(
            free.iter().all(|depth| *depth == crcbl::hal::depth::CLEAR),
            "light tile {light_tile} holds no map and was written anyway"
        );
    }
}

/// **The caster darkens the floor, and removing it lights that floor back up.**
///
/// The half a golden cannot make: a frame with a dark patch in it is a frame
/// with a dark patch in it, and a `spot_visibility` hard-wired to zero over the
/// caster's own footprint would draw one. What separates a shadow from a decal is
/// that it is *absent* when the caster is, and this is that comparison — the same
/// pixels, the same light, the same floor, differing in one instance.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn removing_a_spots_caster_lights_the_floor_it_darkened() {
    let with = render_spot(Some(SPOT_CASTER_OFFSET));
    let without = render_spot(None);
    let shadowed = spot_band(&with, 1);
    let lit = spot_band(&without, 1);
    eprintln!(
        "crcbl forward e2e: spot shadow — the band measures {shadowed:.1} with the caster and {lit:.1} \
         without it"
    );
    assert!(
        shadowed * SPOT_SHADOW_RATIO < lit,
        "the band under the caster's shadow measures {shadowed:.1}, and the same band with no \
         caster at all measures {lit:.1} — a shadow is the difference between those two, and \
         there is not one here"
    );
}

/// **The dark region follows the caster**, which is the whole claim.
///
/// A shadow map read through a matrix that disagrees with the one it was
/// rendered with puts a shadow somewhere — often somewhere plausible — and a
/// single frame cannot tell that from a correct one. Two frames can: the caster
/// moves from `-X` to `+X` and the dark band has to move with it, which no fixed
/// patch and no lighting bug can do.
///
/// Both halves are asserted, and the second is what makes it evidence: a
/// renderer that darkened one side of everything satisfies the first on its own.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_spots_shadow_follows_its_caster() {
    let from_plus_x = render_spot(Some(SPOT_CASTER_OFFSET));
    let from_minus_x = render_spot(Some(-SPOT_CASTER_OFFSET));

    // `.0` is the band on the world `-X` side, `.1` the one on `+X`.
    let plus = (spot_band(&from_plus_x, -1), spot_band(&from_plus_x, 1));
    let minus = (spot_band(&from_minus_x, -1), spot_band(&from_minus_x, 1));
    eprintln!(
        "crcbl forward e2e: spot shadow — caster in +X: over it {:.1} across from it {:.1}; caster in -X: \
         over +X {:.1} over -X {:.1}",
        plus.1, plus.0, minus.1, minus.0
    );

    assert!(
        plus.1 * SPOT_SHADOW_RATIO < plus.0,
        "a caster in +X must darken the +X band, but it measured {:.1} against {:.1} on the \
         other side",
        plus.1,
        plus.0
    );
    assert!(
        minus.0 * SPOT_SHADOW_RATIO < minus.1,
        "and moving it to -X must darken the other band, but it measured {:.1} against {:.1}",
        minus.0,
        minus.1
    );
}

// ---------------------------------------------------------------------------
// The shadowed point light
// ---------------------------------------------------------------------------
//
// `docs/plan/18-render-features.md`'s six-tiles decision: one light, six atlas
// tiles, and a face picked per fragment out of the direction to the light. The
// scene is `crcbl::screenshot`'s `Scene::PointShadow` in every number that
// matters, so a failure here and a moved golden there are the same failure seen
// twice.
//
// What this module adds that the golden cannot is the **atlas**: which of the six
// tiles were rendered into and which was not. A frame drawn through five correct
// faces and one wrong one is a picture; a tile that holds depths where its face
// looks at empty sky is not.

/// How far above the floor the point light hangs, on `Scene::PointShadow`'s
/// terms: low enough that the floor past `|x| = POINT_LIGHT_UP` is on the side
/// faces rather than under the light on the `-Y` one.
const POINT_LIGHT_UP: f32 = 0.5;

/// How far its reach is, in world units.
const POINT_REACH: f32 = 3.0;

/// How far from the light's axis a caster stands.
const POINT_CASTER_AT: f32 = 0.6;

/// How much the pyramid is scaled by to get a caster.
const POINT_CASTER_SCALE: f32 = 0.3;

/// How far above the floor the camera stands, looking straight down.
const POINT_CAMERA_UP: f32 = 2.2;

/// How far out along a shadow's own axis the bands below are measured.
///
/// Past the caster's own image and past `POINT_LIGHT_UP`, which is what puts the
/// band on a side face of the light's map — `crcbl`'s `POINT_SHADOW_AT` carries
/// the full argument.
const POINT_BAND_AT: f32 = 1.0;

/// The half-extent of each band, in pixels.
const POINT_BAND: u32 = 6;

/// The point light this suite lights its floor with.
fn point_light() -> crcbl::render::Light {
    crcbl::render::Light::Point(crcbl::render::PointLight {
        position: crcbl::math::Vec3::new(0.0, POINT_LIGHT_UP, 0.0),
        radius: POINT_REACH,
        color: crcbl::math::Vec3::new(1.0, 0.95, 0.85) * 5.0,
        fill: false,
    })
}

/// A camera straight down over the floor's centre.
fn point_camera() -> crcbl::render::Camera {
    crcbl::render::Camera {
        eye: crcbl::math::Vec3::new(0.0, POINT_CAMERA_UP, 0.0),
        target: crcbl::math::Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`.
        up: crcbl::math::Vec3::Z,
        projection: crcbl::render::Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// A caster standing on the floor at `at`.
///
/// The pyramid's base is at `-0.4` in its own space, so lifting it by that much
/// of the scale stands it on `y = 0`. A caster floating above the floor would
/// hide a shadow detached from it, which is what too much bias looks like.
fn point_caster(at: crcbl::math::Vec3) -> crcbl::math::Mat4 {
    crcbl::math::Mat4::from_translation(
        at + crcbl::math::Vec3::new(0.0, 0.4 * POINT_CASTER_SCALE, 0.0),
    ) * crcbl::math::Mat4::from_scale(crcbl::math::Vec3::splat(POINT_CASTER_SCALE))
}

/// Draws the floor under the point light with one caster at `caster`, and a
/// second one at `second` — or with neither.
fn render_point(
    caster: Option<crcbl::math::Vec3>,
    second: Option<crcbl::math::Vec3>,
) -> ShadowFrame {
    render_scene(&ShadowScene {
        prepare: &move |renderer| {
            renderer.set_lights(&[point_light()]);
            // Two rows rather than one, so the pair is two objects in the frame
            // rather than one drawn twice — the untinted row first, on the order
            // the pool has always been filled in.
            for (at, material) in [
                (caster, crcbl::render::scene::DEMO_UNTINTED),
                (second, crcbl::render::scene::DEMO_TINTED),
            ] {
                if let Some(at) = at {
                    crate::mesh_scene::place(
                        renderer,
                        crcbl::render::scene::DEMO_PYRAMID,
                        material,
                        point_caster(at),
                    );
                }
            }
        },
        camera: point_camera(),
        // Dim, on `render_spot`'s terms: a bright sun would light the shadowed
        // floor from a direction this light's map knows nothing about, and the
        // ratios below would be measuring the sun.
        sun: crcbl::render::DirectionalLight {
            color: crcbl::render::DirectionalLight::default().color * 0.03,
            ambient: crcbl::render::DirectionalLight::default().ambient * 0.09,
            ..crcbl::render::DirectionalLight::default()
        },
        // The cube, scaled into a floor whose `+Y` face is the plane `y = 0`.
        model: crcbl::math::Mat4::from_translation(crcbl::math::Vec3::new(
            0.0,
            -0.5 * SPOT_FLOOR_SCALE,
            0.0,
        )) * crcbl::math::Mat4::from_scale(crcbl::math::Vec3::splat(SPOT_FLOOR_SCALE)),
    })
}

/// The mean brightness of the band of floor at `(x, z)` in **world** units.
///
/// **World `+X` is the frame's left and `+Z` its top**, on `spot_band`'s terms
/// exactly: the camera looks down `-Y` with `+Z` up, and a right-handed basis
/// built from those two puts screen-right at `-X`.
fn point_band(frame: &ShadowFrame, x: f32, z: f32) -> f32 {
    // The frame's short half-axis covers `POINT_CAMERA_UP * tan(30°)` of floor.
    let pixels_per_unit = (f64::from(MESH_EXTENT.1) / 2.0)
        / (f64::from(POINT_CAMERA_UP) * (30.0f64).to_radians().tan());
    let centre = |extent: u32, world: f32| {
        let half = f64::from(extent) / 2.0;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "every band this module measures is inside the frame"
        )]
        let pixel = (half - f64::from(world) * pixels_per_unit) as u32;
        pixel
    };
    let column = centre(MESH_EXTENT.0, x);
    let row = centre(MESH_EXTENT.1, z);
    let mut total = 0.0f32;
    let mut count = 0u32;
    for y in row.saturating_sub(POINT_BAND)..(row + POINT_BAND).min(MESH_EXTENT.1) {
        for x in column.saturating_sub(POINT_BAND)..(column + POINT_BAND).min(MESH_EXTENT.0) {
            let pixel = frame.image.pixel(x, y).expect("inside the frame");
            total += f32::from(pixel[0]) + f32::from(pixel[1]) + f32::from(pixel[2]);
            count += 1;
        }
    }
    assert!(count > 0, "an empty band measures nothing");
    total / (count as f32 * 3.0)
}

/// **A point light's tiles are its six faces, in `shadow::face_axis`' order.**
///
/// The assertion that pins the face convention at the *atlas* rather than in the
/// picture, and it is deliberately about which tile is **empty**: the light hangs
/// over a floor with nothing above it, so the `+Y` face — index 2, and the only
/// index this scene can name from outside — looks at empty sky and its tile has
/// to come back at the clear. A renderer that rendered one face six times, or
/// built the six in another order, writes depths there.
///
/// The two casters are what make the other half checkable without depending on
/// which geometry path ran: on a device with an amplification stage the floor's
/// own clusters are rejected before they reach this map — `cluster_survives`
/// leaves a cluster's bounding radius unscaled and the floor is a scaled cube,
/// which `a_shadowed_spot_fills_the_tile_it_was_given_and_no_other` records in
/// full — so the only thing certain to be in *any* face is a caster. One stands
/// out along `+X` and one out along `-Z`, so faces 0 and 5 hold geometry on every
/// path.
///
/// The pass is one `LoadOp::Clear` over the whole atlas, so a tile nothing drew
/// into is exactly `depth::CLEAR` — which makes "is it at the clear" a question
/// with a crisp answer rather than a threshold.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_shadowed_point_lights_faces_are_the_six_the_host_built() {
    let frame = render_point(
        Some(crcbl::math::Vec3::new(POINT_CASTER_AT, 0.0, 0.0)),
        Some(crcbl::math::Vec3::new(0.0, 0.0, -POINT_CASTER_AT)),
    );
    // The one light is the only candidate and the region is empty, so `Selection`
    // gives it the whole of it from tile 0 — which is the base `mesh.slang` reads
    // off the row and adds a face to.
    // As `the_shadow_atlas_is_written_rather_than_left_at_its_clear_value`: a
    // backend that declares no depth-image copy was held to that refusal in
    // `render_scene`, and has no atlas for the faces below.
    let Some(atlas) = frame.atlas() else {
        return;
    };
    let mut written = [0usize; crcbl::render::shadow::POINT_FACES];
    for (face, count) in written.iter_mut().enumerate() {
        let tile = tile(atlas, crcbl::render::shadow::light_tile(face));
        *count = tile.iter().filter(|depth| **depth > 0.0).count();
        assert!(
            tile.iter().all(|depth| (0.0..=1.0).contains(depth)),
            "face {face} holds a depth outside 0..1, so its reversed-Z range is not the one the \
             comparison sampler tests against"
        );
    }
    eprintln!("crcbl forward e2e: point shadow — written texels per face {written:?}");

    // `+Y`, which `shadow::face_axis` puts at index 2.
    assert_eq!(
        written[2], 0,
        "face 2 looks straight up from a light with nothing above it and holds {} written \
         texel(s), so the six matrices are not the six faces `shadow::face_axis` names",
        written[2]
    );
    // `+X` and `-Z`, which each hold a caster. A real area rather than a stray
    // texel: a caster covers a few per cent of a face's tile — the light is close
    // to it and a face is a 90° frustum — and a mis-transformed sliver covers far
    // less.
    let texels = f64::from(crcbl::render::shadow::TILE).powi(2);
    for face in [0, 5] {
        assert!(
            written[face] > 0,
            "face {face} has a caster standing in it and is entirely at the reversed-Z clear, so \
             its map was never rendered — a face that shadows nothing and a frame that looks \
             correct"
        );
        let fraction = written[face] as f64 / texels;
        assert!(
            fraction > 0.01,
            "face {face} wrote only {fraction:.4} of its texels, which is a sliver rather than a \
             caster"
        );
    }
}

/// **The dark region follows the caster from one face of the map to another**,
/// which is the claim six tiles exist for.
///
/// A caster out along `+X` darkens the floor `+X` of the light and leaves the
/// floor `-Z` of it lit; move the same caster to `-Z` and the two swap. Neither
/// frame can be told from a working one on its own — the first is exactly what a
/// face lookup stuck on `+X` draws — and no lighting bug, no fixed patch and no
/// single working face can produce both.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_point_lights_shadow_follows_its_caster_from_one_face_to_another() {
    let along_x = render_point(
        Some(crcbl::math::Vec3::new(POINT_CASTER_AT, 0.0, 0.0)),
        None,
    );
    let along_z = render_point(
        Some(crcbl::math::Vec3::new(0.0, 0.0, -POINT_CASTER_AT)),
        None,
    );

    // `.0` is the band out along `+X`, `.1` the one out along `-Z`.
    let with_x = (
        point_band(&along_x, POINT_BAND_AT, 0.0),
        point_band(&along_x, 0.0, -POINT_BAND_AT),
    );
    let with_z = (
        point_band(&along_z, POINT_BAND_AT, 0.0),
        point_band(&along_z, 0.0, -POINT_BAND_AT),
    );
    eprintln!(
        "crcbl forward e2e: point shadow — caster in +X: +X band {:.1} -Z band {:.1}; caster in -Z: \
         +X band {:.1} -Z band {:.1}",
        with_x.0, with_x.1, with_z.0, with_z.1
    );

    assert!(
        with_x.0 * SPOT_SHADOW_RATIO < with_x.1,
        "a caster in +X must darken the +X band and leave the -Z one lit, but they measured \
         {:.1} and {:.1}",
        with_x.0,
        with_x.1
    );
    assert!(
        with_z.1 * SPOT_SHADOW_RATIO < with_z.0,
        "and moving it to -Z must darken the -Z band instead, but they measured {:.1} and \
         {:.1} — a face lookup stuck on one face draws the first frame and not this one",
        with_z.1,
        with_z.0
    );
}

// ---------------------------------------------------------------------------
// A point light and a spot in one atlas
// ---------------------------------------------------------------------------
//
// `docs/plan/18-render-features.md`'s 2026-08-23 slice: the light region grew
// from a point light's cube to one tile past it, so a frame can shadow a point
// light and a spot at the same time. `crcbl_render::shadow`'s own tests cover
// the arithmetic that hands the runs out; the two below are what says the pair
// survives a device — both maps rasterised into one atlas, and both of them
// sampled in one shaded frame.
//
// **Each light stands over its own floor.** `punctual_falloff` reaches exactly
// zero at a light's radius and the spot's cone is a narrow one, so the point's
// reach ends before the spot's pool begins and the cone never covers the
// point's. That is what makes the two bands separable, and it is the property
// the second test turns on: a floor lit by both lights everywhere darkens under
// either caster, and a pair of assertions that cannot tell one shadow from the
// other is a pair one working map passes.

/// Where the point light hangs.
///
/// **Outboard of its own caster**, which is what keeps its shadow clear of that
/// caster's own image. The camera looks straight down, so an object standing off
/// the frame's axis leans *outward* in the picture while its shadow falls *away*
/// from the light: put the light between the axis and the caster and the two
/// land on top of each other, and put the caster between the axis and the light
/// — as [`PAIR_POINT_CASTER_AT`] is — and the shadow falls inward, past a band
/// the caster's image cannot reach.
const PAIR_POINT_AT: crcbl::math::Vec3 = crcbl::math::Vec3::new(1.3, 0.6, 0.0);

/// How far the point light reaches, in world units.
///
/// Two things at once. It is short enough that this light is **exactly** dark at
/// [`PAIR_SPOT_BAND_AT`] — `punctual_falloff` windows to zero at the radius, and
/// `light_cluster.slang` culls against the same number — which is what leaves
/// the spot's band lit by the spot alone. And it is what ranks this light above
/// the spot in [`Selection`](crcbl::render::shadow::Selection), whose metric is
/// radius over distance to the eye: the two lights sit at all but the same
/// distance from this camera, so the wider radius is what decides the order, and
/// the point light takes the first run of tiles with the spot behind it.
const PAIR_POINT_REACH: f32 = 1.6;

/// Where the spot hangs: 45° from vertical, tilted along `-Z`, so its cone's
/// axis lands on the floor at the frame's `z = 0` and the light itself stands
/// `+Z` of that pool.
///
/// Outboard of its own caster on the same terms [`PAIR_POINT_AT`] is, in `z`
/// rather than in `x`.
const PAIR_SPOT_AT: crcbl::math::Vec3 = crcbl::math::Vec3::new(-1.2, 0.7, 0.7);

/// How far the spot reaches, in world units.
///
/// Past its own band and not much further — a cone that reached the point
/// light's floor would light the far side of the frame through a map that
/// belongs to this side of it.
const PAIR_SPOT_REACH: f32 = 1.3;

/// Where the point light's caster stands: inboard of its light, on the `x` axis.
const PAIR_POINT_CASTER_AT: crcbl::math::Vec3 = crcbl::math::Vec3::new(0.85, 0.0, 0.0);

/// Where the spot's caster stands: inboard of its light, under the cone.
const PAIR_SPOT_CASTER_AT: crcbl::math::Vec3 = crcbl::math::Vec3::new(-1.2, 0.0, 0.15);

/// How tall the point light's caster is, in world units.
///
/// A cube rather than the pyramid the spot's and the point light's scenes above
/// cast with, because what these bands need is a *wide* shadow: a pyramid's is a
/// triangle that narrows to nothing at its tip, so a band placed far enough from
/// the caster to be clear of the caster's own image sits where that shadow is a
/// few texels across. A box throws one as wide at its far edge as at its near
/// one.
const PAIR_POINT_CASTER_SIDE: f32 = 0.3;

/// How tall the spot's caster is, on the same terms.
///
/// Shorter than the point light's, because this one has a cone to stay inside.
/// The spot's pool is a few tenths of a unit across at this throw, and the
/// caster, its shadow and a band between the two all have to fit in it — a
/// taller caster throws its shadow past the edge, where there is no light for
/// the shadow to be the absence of.
const PAIR_SPOT_CASTER_SIDE: f32 = 0.24;

/// Where the point light's shadow is measured, in world `x` and `z`.
///
/// Inside the caster's shadow and inboard of the caster's own image. The two
/// lean opposite ways from the frame's axis — see [`PAIR_POINT_AT`] — so there
/// is floor between them that the shadow covers and the picture of the caster
/// does not.
const PAIR_POINT_BAND_AT: (f32, f32) = (0.35, 0.0);

/// Where the spot's shadow is measured, on the same terms.
///
/// Straight down-light of its caster, which stands directly below the spot in
/// `x`, so the band's `x` is the light's own.
const PAIR_SPOT_BAND_AT: (f32, f32) = (PAIR_SPOT_AT.x, -0.15);

/// A cube of side `side` standing on the floor at `at`.
fn pair_caster(at: crcbl::math::Vec3, side: f32) -> crcbl::math::Mat4 {
    crcbl::math::Mat4::from_translation(at + crcbl::math::Vec3::new(0.0, 0.5 * side, 0.0))
        * crcbl::math::Mat4::from_scale(crcbl::math::Vec3::splat(side))
}

/// The point light of the combined scene.
fn pair_point_light() -> crcbl::render::Light {
    crcbl::render::Light::Point(crcbl::render::PointLight {
        position: PAIR_POINT_AT,
        radius: PAIR_POINT_REACH,
        color: crcbl::math::Vec3::new(1.0, 0.95, 0.85) * 5.0,
        fill: false,
    })
}

/// The spot of the combined scene, at [`spot_light`]'s cone angles.
fn pair_spot_light() -> crcbl::render::Light {
    crcbl::render::Light::Spot(crcbl::render::SpotLight {
        position: PAIR_SPOT_AT,
        radius: PAIR_SPOT_REACH,
        color: crcbl::math::Vec3::new(1.0, 0.95, 0.85) * 5.0,
        // Along the cone, away from the light: straight down at the floor below
        // it, so its pool sits under the light rather than beside it.
        direction: crcbl::math::Vec3::new(0.0, -PAIR_SPOT_AT.y, -PAIR_SPOT_AT.z),
        inner_angle: 0.18,
        outer_angle: 0.28,
        fill: false,
    })
}

/// Which of the combined scene's two casters are in the frame.
///
/// A struct rather than two `bool` arguments, because every assertion below
/// turns on *which* caster was left out and `render_pair(true, false)` does not
/// say which.
#[derive(Clone, Copy)]
struct Casters {
    point: bool,
    spot: bool,
}

/// Draws the floor under both lights, with whichever of the two casters
/// `casters` names.
fn render_pair(casters: Casters) -> ShadowFrame {
    render_scene(&ShadowScene {
        prepare: &move |renderer| {
            // The point light first, so a tie in the selection's ranking breaks
            // towards it by index — the order the assertions below are written
            // against.
            renderer.set_lights(&[pair_point_light(), pair_spot_light()]);
            for (wanted, at, side) in [
                (casters.point, PAIR_POINT_CASTER_AT, PAIR_POINT_CASTER_SIDE),
                (casters.spot, PAIR_SPOT_CASTER_AT, PAIR_SPOT_CASTER_SIDE),
            ] {
                if wanted {
                    crate::mesh_scene::place(
                        renderer,
                        crcbl::render::scene::DEMO_CUBE,
                        crcbl::render::scene::DEMO_UNTINTED,
                        pair_caster(at, side),
                    );
                }
            }
        },
        camera: point_camera(),
        // Dim, on `render_spot`'s terms: a bright sun would light both shadowed
        // bands from a direction neither map knows anything about, and the
        // ratios below would be measuring the sun.
        sun: crcbl::render::DirectionalLight {
            color: crcbl::render::DirectionalLight::default().color * 0.03,
            ambient: crcbl::render::DirectionalLight::default().ambient * 0.09,
            ..crcbl::render::DirectionalLight::default()
        },
        // The cube, scaled into a floor whose `+Y` face is the plane `y = 0`.
        model: crcbl::math::Mat4::from_translation(crcbl::math::Vec3::new(
            0.0,
            -0.5 * SPOT_FLOOR_SCALE,
            0.0,
        )) * crcbl::math::Mat4::from_scale(crcbl::math::Vec3::splat(SPOT_FLOOR_SCALE)),
    })
}

/// **A point light's cube and a spot's map are in the atlas at once.**
///
/// The claim the seventh light tile was added for, made against the image
/// rather than against the selection: `crcbl_render::shadow`'s unit tests say
/// `Selection` hands out a run of [`POINT_FACES`](crcbl::render::shadow::POINT_FACES)
/// and a run of one, and nothing until this said the shadow pass then rasterised
/// into both of them. A viewport that stopped at the cube, a second cull that
/// was never dispatched, or a region still sized for one point light all leave a
/// frame in which the spot lights without occluding — which is a picture that
/// looks entirely plausible.
///
/// Three things are asserted and they are three different failures:
///
/// * the point light's `+Y` face is at the clear. It looks straight up from a
///   light with nothing above it, so it is empty in a correct frame — and it is
///   empty only if the point light's run starts at light tile 0, which is what
///   pins the rest of the arithmetic below.
/// * the faces its caster stands in hold depths, so the cube is a map rather
///   than six cleared tiles.
/// * the tile **past** the cube holds the spot's map. That tile exists at all
///   only because the light region is a point light's cube plus one.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_point_light_and_a_spot_hold_the_cube_and_the_tile_past_it() {
    // The tile the spot is expected in: the one after the point light's run,
    // which is at light tile 0 because the point light outranks the spot — see
    // `PAIR_POINT_REACH`. Named rather than written as a literal, so a light
    // region shrunk back to a point light's six faces fails on the sentence
    // below instead of on an out-of-range tile.
    let spot_tile = crcbl::render::shadow::POINT_FACES;
    assert!(
        spot_tile < crcbl::render::shadow::LIGHT_TILES,
        "the atlas's light region is {} tiles, so a point light's cube fills the whole of it and \
         a spot in the same frame is refused a map — there is no tile here for a second shadowed \
         light to be rendered into",
        crcbl::render::shadow::LIGHT_TILES
    );

    let frame = render_pair(Casters {
        point: true,
        spot: true,
    });
    // As `the_shadow_atlas_is_written_rather_than_left_at_its_clear_value`: a
    // backend that declares no depth-image copy was held to that refusal in
    // `render_scene`, and has no atlas for the tiles below.
    let Some(atlas) = frame.atlas() else {
        return;
    };
    let written = |light_tile: usize| {
        let tile = tile(atlas, crcbl::render::shadow::light_tile(light_tile));
        assert!(
            tile.iter().all(|depth| (0.0..=1.0).contains(depth)),
            "light tile {light_tile} holds a depth outside 0..1, so its reversed-Z range is not \
             the one the comparison sampler tests against"
        );
        tile.iter().filter(|depth| **depth > 0.0).count()
    };
    let counts: Vec<usize> = (0..crcbl::render::shadow::LIGHT_TILES)
        .map(written)
        .collect();
    eprintln!(
        "{suite}: point and spot — written texels per light tile {counts:?}",
        suite = crate::SUITE
    );

    // `+Y`, which `shadow::face_axis` puts at index 2 of the point light's run.
    assert_eq!(
        counts[2], 0,
        "the point light's `+Y` face looks straight up from a light with nothing above it and \
         holds {} written texel(s). Either the six matrices are not the six `shadow::face_axis` \
         names, or this light's run does not start where the assertions below read it",
        counts[2]
    );
    // `-X` and `-Y`. The caster stands inboard of its light and below it by very
    // nearly as much as it stands inboard, so it straddles the boundary between
    // those two faces and each of their frusta holds a part of it — and `-X` is
    // the face the band in the next test reads its shadow through. A real area
    // rather than a stray texel, on
    // `a_shadowed_point_lights_faces_are_the_six_the_host_built`'s terms: a
    // caster this close to the light covers a few per cent of a 90° face, and a
    // mis-transformed sliver covers far less.
    let texels = f64::from(crcbl::render::shadow::TILE).powi(2);
    for face in [1, 3] {
        let fraction = counts[face] as f64 / texels;
        assert!(
            fraction > 0.01,
            "the point light's face {face} has its caster standing in it and wrote {fraction:.4} \
             of its texels, which is a sliver rather than a caster — a face that shadows nothing \
             and a frame that looks correct"
        );
    }
    // And the spot's own tile, past the whole of that cube.
    let fraction = counts[spot_tile] as f64 / texels;
    assert!(
        fraction > 0.05,
        "light tile {spot_tile} is the spot's, and it wrote {fraction:.4} of its texels. At the \
         reversed-Z clear it is a spot that was given a tile and never rendered into it; a sliver \
         is a map that was rendered through the wrong matrix. Both light the floor and occlude \
         nothing"
    );
}

/// **Both maps are sampled, and each caster darkens only its own light's
/// floor.**
///
/// Writing a tile is not occluding with it, and the atlas assertion above cannot
/// tell the two apart: a second map rendered perfectly and never read produces
/// exactly the frame a light with no map at all does. So each band here is
/// measured against the same band of a frame that differs in **one instance**,
/// on `removing_a_spots_caster_lights_the_floor_it_darkened`'s terms.
///
/// What makes it a test of *two* shadows rather than one is a third reading of
/// each band, in the frame that keeps its own caster and drops the **other**
/// light's — and it has to stay dark. Without that a scene lit by both lights
/// everywhere would satisfy the first claim twice over with a single working
/// map, since either caster removed would lift either band. Here the point
/// light's falloff is exactly zero at the spot's band and the cone never
/// reaches the point light's, so a band that brightens when the far caster
/// leaves is not measuring the map this test names.
///
/// Those two are asserted **first**, before the claim they underwrite: a band
/// the far caster also darkens makes the numbers in the claim meaningless, and
/// a run that reports which of the two it is beats one that reports "no shadow
/// here" about a band two shadows fall on.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-forward-e2e.sh"]
fn a_point_light_and_a_spot_each_darken_the_floor_their_own_caster_blocks() {
    let both = render_pair(Casters {
        point: true,
        spot: true,
    });
    let no_point = render_pair(Casters {
        point: false,
        spot: true,
    });
    let no_spot = render_pair(Casters {
        point: true,
        spot: false,
    });

    let band = |frame: &ShadowFrame, at: (f32, f32)| point_band(frame, at.0, at.1);
    let point = (
        band(&both, PAIR_POINT_BAND_AT),
        band(&no_point, PAIR_POINT_BAND_AT),
        band(&no_spot, PAIR_POINT_BAND_AT),
    );
    let spot = (
        band(&both, PAIR_SPOT_BAND_AT),
        band(&no_spot, PAIR_SPOT_BAND_AT),
        band(&no_point, PAIR_SPOT_BAND_AT),
    );
    eprintln!(
        "{suite}: point and spot — the point light's band measures {:.1} with both casters, \
         {:.1} without its own and {:.1} without the spot's; the spot's band measures {:.1}, \
         {:.1} and {:.1}",
        point.0,
        point.1,
        point.2,
        spot.0,
        spot.1,
        spot.2,
        suite = crate::SUITE
    );

    // **Each band belongs to one light, and that is asserted before anything is
    // read from it.** A band the far caster also darkens makes the pair below
    // meaningless — it would go dark and light again with either instance, and
    // one working map would satisfy both.
    assert!(
        point.2 < point.0 * SPOT_SHADOW_RATIO,
        "the point light's band lifted from {:.1} to {:.1} when the *spot's* caster left the \
         frame. Nothing the spot occludes can reach this band — it is outside the cone — so what \
         darkened it is not the map this test says it is",
        point.0,
        point.2
    );
    assert!(
        spot.2 < spot.0 * SPOT_SHADOW_RATIO,
        "and the spot's band lifted from {:.1} to {:.1} when the *point light's* caster left, \
         which is the mirror of the same defect: the point light's falloff is exactly zero here",
        spot.0,
        spot.2
    );
    assert!(
        point.0 * SPOT_SHADOW_RATIO < point.1,
        "the point light's caster must darken the floor inboard of it, but that band measures \
         {:.1} with the caster and {:.1} without it — a shadow is the difference between those \
         two and there is not one here",
        point.0,
        point.1
    );
    assert!(
        spot.0 * SPOT_SHADOW_RATIO < spot.1,
        "and the spot's caster must darken the floor under its cone, but that band measures \
         {:.1} with the caster and {:.1} without it. A tile in the atlas is not an occluder \
         until something samples it",
        spot.0,
        spot.1
    );
}
