//! Offscreen render → readback → golden image — the `crcbl screenshot` path.
//!
//! Opens a GPU backend, creates an offscreen surface and swapchain, renders
//! one frame of the [`Scene`] the caller named, and reads the pixels back.
//!
//! # Why there is more than one scene
//!
//! `docs/plan/02-vulkan-backend.md`'s shader-portability rule 5: a shader can
//! compile cleanly to SPIR-V, WGSL, MSL and DXIL and *mean something different*
//! on each, and no lint can see it — `SV_InstanceID` lowers to
//! `InstanceIndex - BaseInstance` on SPIR-V and to a bare
//! `@builtin(instance_index)` on WGSL, which is why every sprite batch after the
//! first drew the first batch's instances while every gate stayed green. The
//! only detector is rendering the same scene through two backends and comparing
//! pixels, which is what `tests/run-cross-backend-e2e.sh` does — and it can only
//! catch what it draws. [`Scene::Cube`] exercises `mesh.slang` and
//! `tonemap.slang`; [`Scene::Sprite`] and [`Scene::Ui`] are here so
//! `sprite.slang` and `ui.slang` — the two with an actual history of divergence
//! — are covered too.
//!
//! `SV_VertexID` turned out to be the same story, and it is why [`Scene::Cube`]
//! draws the mesh pool's *second* resident beside the cube: a base vertex only
//! means something for a mesh that is not at base 0, and this is the only frame
//! in the tree that renders one through both backends.
//!
//! What comes back is the swapchain image's bytes *as they are in memory*, in
//! [`OffscreenSetup::format`]'s channel order — which on an ordinary desktop
//! surface is BGRA, not RGBA. Turning them into a `crcbl_golden::Image` is
//! the caller's job and needs `Image::from_readback` with the matching
//! `ChannelOrder`, not `Image::from_rgba8`; this module deliberately does not
//! guess, because the format is the thing it knows and the image type is not
//! its dependency to reach for.
//!
//! This module is the render half of the CLI subcommand; the CLI module
//! owns the argument parsing and I/O.
//!
//! # Native only
//!
//! The whole path blocks: [`crate::backend::open`], a blocking device creation,
//! and a `std::thread::sleep` poll loop waiting for the readback copy to land.
//! The browser's main thread may not block, so this module is
//! `#[cfg(not(target_arch = "wasm32"))]` in `lib.rs` and a wasm build that
//! reaches for it fails to *compile* rather than hanging on the first
//! screenshot — the rule [`crate::backend`] states and
//! [`Instance::create_device`] established.
//!
//! Nothing here is wanted in a browser at P5: it is a command-line tool's back
//! end, not something a game calls per frame. If it ever is, the polled shapes
//! it would have to be rewritten onto already exist —
//! [`Instance::request_device`] and
//! [`Device::poll_readback`].
//!
//! # Open gap: a screenshot does not say which adapter drew it
//!
//! This module picks `adapters().first()` and never reports which one that was,
//! and `crcbl screenshot` installs no logger, so the backends' own adapter lines
//! go nowhere. `run-vk-e2e.sh` and `run-wgpu-e2e.sh` both read that line out of
//! their suites precisely so a pinned ICD the loader ignored is caught; the
//! cross-backend harness (`tests/run-cross-backend-e2e.sh`) cannot, and pins by
//! manifest path alone. Recorded here rather than worked around: closing it
//! means either a `--json` field naming the adapter or a logger in the CLI, and
//! both are `crcbl-cli`'s call to make.

use std::time::Duration;

use crate::hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Device, DeviceDesc,
    Extent3d, Features, Format, ImageAspect, ImageBarrier, ImageSubresourceLayers,
    ImageSubresourceRange, Instance, MemoryLocation, Offset3d, PresentInfo, PresentMode,
    QueueHandle, QueueKind, ReadbackDesc, ReadbackState, ResourceState, SubmitInfo, SurfaceError,
    SurfaceHandle, SurfaceTarget, SwapchainDesc, SwapchainHandle,
};
use crate::render::{
    Camera, DirectionalLight, FontAtlas, ForwardRenderer, GraphError, Projection, RenderGraph,
    SampleMode, SheetDesc, SheetId, Sprite, SpriteRenderer, TransientPool, UiRenderer,
};
use crate::ui::draw_list::DrawList;

// ---------------------------------------------------------------------------
// Scenes
// ---------------------------------------------------------------------------

/// What a screenshot draws.
///
/// One variant per engine shader pair that has pixels of its own, because the
/// cross-backend comparison this feeds can only catch divergence in a shader it
/// actually ran — see the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scene {
    /// The sandbox's lit cube through [`ForwardRenderer`]: `mesh.slang` into an
    /// HDR target and `tonemap.slang` back out of it.
    ///
    /// The default, because it is the frame every caller of this module drew
    /// before there was anything to choose between.
    ///
    /// The pyramid beside it is the mesh pool's second resident, and it is here
    /// for the reason the module docs give: it is the only geometry in the tree
    /// at a non-zero base vertex, so it is the only thing this comparison can
    /// use to tell the four targets' `SV_VertexID` lowerings apart.
    #[default]
    Cube,
    /// Four sprites over three [`SpriteRenderer`] batches: `sprite.slang`.
    Sprite,
    /// Rectangles, an outline and glyph-atlas text through [`UiRenderer`]:
    /// `ui.slang`.
    Ui,
}

/// Where [`Scene::Cube`] puts the pyramid, in world units.
///
/// Left of the cube and clear of it: the camera is two units back with a 60°
/// vertical field of view, so at `z = 0` the frame is about 2.3 units tall and
/// 3.1 wide at 4:3. The cube spans `±0.5` and the pyramid `±0.4`, so this
/// leaves both fully inside the frame with a gap between them — and a gap is
/// what makes "the pyramid drew the cube's vertices" a visibly different
/// picture rather than an overlap.
const PYRAMID_AT: glam::Vec3 = glam::Vec3::new(-1.05, 0.0, 0.0);

/// The colour [`Scene::Sprite`] and [`Scene::Ui`] composite onto, in **linear**
/// light — which is what a clear value on an sRGB attachment means.
///
/// Neither pass clears: both load what is already in the target, because
/// compositing onto it is what they are for. So the scene clears first, and
/// deliberately not to black — alpha blending onto black is the one background
/// where `src * a + dst * (1 - a)` and `src * a` agree, so a premultiplication
/// mistake would be invisible in exactly the frame meant to reveal it. Same
/// value, and the same reasoning, as `crcbl-vk`'s sprite suite.
const SCENE_CLEAR: [f32; 4] = [0.10, 0.20, 0.35, 1.0];

/// Half the world height [`Scene::Sprite`]'s orthographic camera shows.
///
/// The frame size is the caller's (`--size`), so the scene is laid out in world
/// units and the projection scales it: every rectangle below sits inside
/// `|x| <= 95`, which is on screen for any frame at least as wide as it is tall.
const SPRITE_HALF_HEIGHT: f32 = 100.0;

/// [`Scene::Sprite`]'s first sheet: 4×2 texels, two 2×2 frames side by side,
/// and no two texels alike.
///
/// ```text
///   frame A          frame B
///   red    green  |  cyan   magenta
///   blue   yellow |  white  black
/// ```
///
/// Asymmetric for the reason `crcbl-vk`'s sprite suite records: a flipped V
/// swaps red with blue, a flipped U swaps red with green, and a symmetric test
/// image passes through both while looking entirely plausible.
const SPRITE_SHEET_A: [u8; 32] = [
    255, 0, 0, 255, // A top-left: red
    0, 255, 0, 255, // A top-right: green
    0, 255, 255, 255, // B top-left: cyan
    255, 0, 255, 255, // B top-right: magenta
    0, 0, 255, 255, // A bottom-left: blue
    255, 255, 0, 255, // A bottom-right: yellow
    255, 255, 255, 255, // B bottom-left: white
    0, 0, 0, 255, // B bottom-right: black
];

/// [`Scene::Sprite`]'s second sheet: 2×2 texels in four colours that appear
/// nowhere in [`SPRITE_SHEET_A`], so "which sheet was bound" is readable
/// straight off the picture.
const SPRITE_SHEET_B: [u8; 16] = [
    255, 128, 0, 255, // orange
    128, 0, 255, 255, // violet
    0, 128, 128, 255, // teal
    128, 128, 128, 255, // grey
];

/// The tint on the fourth sprite.
///
/// A different factor per channel, because a tint that left any channel alone
/// would let the tinted rectangle share colours with the untinted one it is
/// there to be told apart from.
const SPRITE_TINT: [f32; 4] = [0.5, 0.7, 0.9, 1.0];

/// Frame A of [`SPRITE_SHEET_A`], as normalised UVs.
const SPRITE_FRAME_A: [f32; 4] = [0.0, 0.0, 0.5, 1.0];
/// Frame B of [`SPRITE_SHEET_A`].
const SPRITE_FRAME_B: [f32; 4] = [0.5, 0.0, 1.0, 1.0];

/// [`Scene::Sprite`]'s four rectangles, in world units: `[x, y, w, h]`, minimum
/// corner first, Y up. Ten units apart, so no two of them touch.
const SPRITE_RECTS: [[f32; 4]; 4] = [
    [-95.0, -20.0, 40.0, 40.0],
    [-45.0, -20.0, 40.0, 40.0],
    [5.0, -20.0, 40.0, 40.0],
    [55.0, -20.0, 40.0, 40.0],
];

/// Which sheet each of [`SPRITE_RECTS`] samples: **A A B A**, not A A B B.
///
/// The submission order of a `&[Sprite]` is the batching, so this is three
/// batches over two sheets and the third batch starts at instance 3. That
/// arrangement is the whole point of the scene: one batch is exactly the case
/// that hid the `SV_InstanceID` divergence, because with a single draw the
/// SPIR-V and WGSL lowerings of the instance index agree. A backend that reads
/// the wrong one draws the last rectangle in the *first* rectangle's place,
/// leaving its own slot at the clear colour.
const SPRITE_ORDER: [usize; 4] = [0, 0, 1, 0];

/// The camera [`Scene::Sprite`] is drawn with: orthographic, looking down −Z at
/// the plane the sprites live on.
fn sprite_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, 0.0, 1.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Y,
        projection: Projection::Orthographic {
            half_height: SPRITE_HALF_HEIGHT,
            near: 0.1,
            far: 10.0,
        },
    }
}

/// [`Scene::Sprite`]'s four sprites, over the two registered sheets.
fn sprite_scene(sheets: [SheetId; 2]) -> [Sprite; 4] {
    let uv = |index: usize| match index {
        0 => SPRITE_FRAME_A,
        1 => SPRITE_FRAME_B,
        // The second sheet has one frame, which is the whole image.
        2 => [0.0, 0.0, 1.0, 1.0],
        _ => SPRITE_FRAME_A,
    };
    std::array::from_fn(|index| Sprite {
        sheet: sheets[SPRITE_ORDER[index]],
        rect: SPRITE_RECTS[index],
        rotation: 0.0,
        uv: uv(index),
        // Only the last one is tinted, so the two rectangles that share frame A
        // are the same picture in different colours.
        tint: if index == 3 { SPRITE_TINT } else { [1.0; 4] },
    })
}

/// [`Scene::Ui`]'s draw list, in the pass's Y-down screen pixels.
///
/// Laid out as fractions of `extent` so the same picture arrives at every
/// `--size`, and built from all three command kinds: a filled rectangle, a
/// translucent one straddling its edge, an outline, and two lines of text
/// through the glyph atlas. Text alone would leave a broken atlas binding
/// looking like an empty frame; a rectangle alone would never sample the atlas
/// at all.
fn ui_draw_list(extent: (u32, u32)) -> DrawList {
    use glam::Vec2;

    let (width, height) = (extent.0 as f32, extent.1 as f32);
    let at = |x: f32, y: f32| Vec2::new(width * x, height * y);

    let mut list = DrawList::new();
    // The panel, then the translucent bar half on it and half off it: the two
    // colours it blends to are two more distinct colours in the frame, and they
    // are the only evidence the pass blends rather than replaces.
    list.rect(at(0.08, 0.10), at(0.92, 0.62), [0.15, 0.20, 0.55, 1.0]);
    list.rect(at(0.30, 0.45), at(0.70, 0.85), [1.0, 0.45, 0.10, 0.5]);
    list.rect_outline(
        at(0.03, 0.04),
        at(0.97, 0.96),
        (height * 0.02).max(1.0),
        [1.0, 0.85, 0.0, 1.0],
    );
    list.text(at(0.12, 0.16), "CRCBL", [1.0, 1.0, 1.0, 1.0], height * 0.18);
    list.text(
        at(0.12, 0.66),
        "ui scene",
        [0.2, 1.0, 0.4, 1.0],
        height * 0.10,
    );
    list
}

/// The renderer, and the content, for the scene being drawn.
///
/// One variant per [`Scene`]; the frame's per-scene work is the two arms of
/// [`OffscreenSetup::draw_and_readback`] and nothing else keys off it.
enum SceneState {
    Cube {
        camera: Camera,
        light: DirectionalLight,
        /// Boxed because it is much the largest of the three: it carries the
        /// geometry pools and the instance ring, and an unboxed variant would
        /// make every `SceneState` — including the two small ones — that size.
        renderer: Box<ForwardRenderer>,
    },
    Sprite {
        renderer: SpriteRenderer,
        sheets: [SheetId; 2],
    },
    Ui {
        renderer: UiRenderer,
        atlas: FontAtlas,
    },
}

impl SceneState {
    /// Builds the renderer this scene needs, and uploads whatever it draws.
    fn open(
        scene: Scene,
        device: &dyn Device,
        queue: QueueHandle,
        format: Format,
    ) -> Result<Self, OffscreenError> {
        Ok(match scene {
            Scene::Cube => {
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                renderer.set_pyramid(Some(glam::Mat4::from_translation(PYRAMID_AT)));
                Self::Cube {
                    camera: Camera::default().with_projection(Projection::Perspective {
                        fov_y: std::f32::consts::FRAC_PI_3,
                        near: 0.01,
                    }),
                    light: DirectionalLight::default(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Sprite => {
                let mut renderer = SpriteRenderer::new(device, queue, format)?;
                let mut register = |label, width, height, pixels| {
                    renderer.register_sheet(
                        device,
                        &SheetDesc {
                            label,
                            width,
                            height,
                            // Pixel art's sampler, and the branch of
                            // `sprite.slang` a game actually ships on.
                            sample: SampleMode::Pixel,
                            pixels,
                        },
                    )
                };
                let sheets = match (
                    register("screenshot sheet A", 4, 2, &SPRITE_SHEET_A),
                    register("screenshot sheet B", 2, 2, &SPRITE_SHEET_B),
                ) {
                    (Ok(a), Ok(b)) => [a, b],
                    // Whichever failed, the renderer owns everything that did
                    // upload and gives it back here rather than at drop.
                    (Err(error), _) | (Ok(_), Err(error)) => {
                        renderer.destroy(device);
                        return Err(OffscreenError::Hal(error));
                    }
                };
                Self::Sprite { renderer, sheets }
            }
            Scene::Ui => Self::Ui {
                renderer: UiRenderer::new(device, queue, format)?,
                atlas: FontAtlas::built_in(),
            },
        })
    }

    /// Releases the scene's GPU resources. The device must be idle.
    fn destroy(self, device: &dyn Device) {
        match self {
            Self::Cube { renderer, .. } => renderer.destroy(device),
            Self::Sprite { renderer, .. } => renderer.destroy(device),
            Self::Ui { renderer, .. } => renderer.destroy(device),
        }
    }
}

// ---------------------------------------------------------------------------
// OffscreenSetup
// ---------------------------------------------------------------------------

/// The largest edge, in pixels, an offscreen frame may have.
///
/// 16384 is `maxImageDimension2D` on every implementation the engine targets,
/// so anything past it would be refused by swapchain creation anyway — but
/// only *after* this module had already tried to allocate a host-visible
/// staging buffer for it. `16384x16384` RGBA8 is one gibibyte, which is the
/// most this path will ever ask an allocator for.
///
/// The CLI checks `--size` against this at parse time so an absurd request is
/// a bad *invocation* (exit 2) rather than a failed command; [`OffscreenSetup::open`]
/// checks it again because a library may not have gone through the CLI.
pub const MAX_DIMENSION: u32 = 16_384;

/// Row-pitch alignment, in bytes, for the readback staging buffer.
///
/// **Not a performance hint — a portability requirement.** wgpu refuses a
/// multi-row buffer↔image copy whose row pitch is not a multiple of
/// `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT` (256), so a tightly packed readback —
/// which is legal on Vulkan and is what this module used to record — fails on
/// `crcbl-wgpu` for every width that is not a multiple of 64:
///
/// ```text
/// $ CRCBL_GPU=wgpu crcbl screenshot --size 32x32 --output /tmp/x.png
/// crcbl: render/readback failed: HAL: invalid descriptor: a buffer↔image copy
///   of 32 texel(s) per row is 128 bytes, which wgpu requires to be a multiple
///   of 256
/// ```
///
/// Found by the cross-backend harness at P5.12, which compares this path's
/// output between the two backends at more than one frame size — `256x192`, the
/// only size anything had ever asked for, happens to be a multiple of 64 and hid
/// it. Vulkan imposes no such rule and pads harmlessly, so the padded pitch is
/// unconditional rather than a backend-specific branch: nothing above
/// `crcbl-hal` may key off which backend is behind the seam.
///
/// The padding never reaches the caller — [`OffscreenSetup::draw_and_readback`]
/// compacts the rows before returning.
pub const READBACK_ROW_ALIGNMENT: u32 = 256;

/// How long [`OffscreenSetup::draw_and_readback`] waits for the copy to land.
///
/// Generous because an offscreen ring on a software rasteriser can take
/// hundreds of milliseconds for a single frame. Public because the two error
/// paths that mention it are public, and a deadline a caller can be hit by is
/// one it should be able to read.
pub const READBACK_DEADLINE: Duration = Duration::from_secs(10);

/// Holds everything needed to render one frame offscreen: a GPU instance,
/// device, offscreen swapchain ring, and the chosen scene's renderer.
///
/// The caller drives one frame via [`Self::draw_and_readback`], then tears
/// down with [`Self::finish`].
#[allow(missing_debug_implementations)]
pub struct OffscreenSetup {
    instance: Box<dyn Instance>,
    device: Box<dyn Device>,
    surface: SurfaceHandle,
    swapchain: SwapchainHandle,
    queue: QueueHandle,
    format: Format,
    /// The renderer for the [`Scene`] this setup was opened with, and whatever
    /// that scene draws.
    scene: SceneState,
    pool: TransientPool,
}

/// Reasons an offscreen render might fail before a pixel is written.
#[derive(Debug, thiserror::Error)]
pub enum OffscreenError {
    /// No GPU backend could be opened.
    #[error("GPU backend: {0}")]
    Backend(#[from] crate::backend::GpuError),

    /// No adapter, no queue, no format, or a surface-cap query failed.
    #[error("device not usable: {0}")]
    Unusable(&'static str),

    /// A HAL call failed.
    #[error("HAL: {0}")]
    Hal(#[from] crate::hal::HalError),

    /// A surface operation failed.
    #[error("surface: {0}")]
    Surface(#[from] crate::hal::SurfaceError),

    /// A graph compile or execute failed.
    #[error("graph: {0}")]
    Graph(#[from] GraphError),

    /// The swapchain went out of date before the first frame completed.
    #[error("offscreen swapchain is out of date")]
    OutOfDate,

    /// The requested frame is larger than [`MAX_DIMENSION`] on an edge, or its
    /// byte count does not fit this machine's address space.
    #[error("{width}x{height} is larger than the {MAX_DIMENSION}x{MAX_DIMENSION} offscreen limit")]
    TooLarge {
        /// Requested width, in pixels.
        width: u32,
        /// Requested height, in pixels.
        height: u32,
    },

    /// The readback did not land within [`READBACK_DEADLINE`].
    ///
    /// A `Result` rather than a panic because the CLI's contract is exit 1 with
    /// a `--json`-shaped message, and a `panic!` here aborted with exit 101
    /// past `report::emit` entirely.
    #[error("readback did not complete within {0:?}")]
    ReadbackTimeout(Duration),
}

impl OffscreenSetup {
    /// Opens the auto-selected GPU backend, creates an offscreen surface,
    /// adapter, device, swapchain, and `scene`'s renderer for a frame of
    /// `(width, height)` pixels.
    ///
    /// [`Scene::default`] is [`Scene::Cube`], the frame this module drew before
    /// there was anything to choose between.
    ///
    /// Returns `Err` if the frame is not between `1x1` and
    /// [`MAX_DIMENSION`]`x`[`MAX_DIMENSION`], if no GPU is available (lavapipe,
    /// swiftshader, or a real card), if the device is unusable, or if any HAL
    /// call fails.
    pub fn open(width: u32, height: u32, scene: Scene) -> Result<Self, OffscreenError> {
        // Checked before the backend is opened, so an absurd `--size` costs a
        // comparison rather than a device.
        if width == 0 || height == 0 {
            return Err(OffscreenError::Unusable("a frame must be at least 1x1"));
        }
        if width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(OffscreenError::TooLarge { width, height });
        }

        let extent = (width, height);
        let instance = crate::backend::open()?;

        let target = SurfaceTarget::Offscreen;
        // SAFETY: `Offscreen` names no platform object, so nothing can dangle.
        let surface = unsafe {
            instance
                .create_surface(&target)
                .map_err(OffscreenError::Hal)?
        };

        let adapters = instance.adapters();
        let adapter = adapters
            .first()
            .ok_or(OffscreenError::Unusable("no adapter"))?;

        let caps = instance
            .surface_caps(surface, adapter.id)
            .map_err(OffscreenError::Hal)?;

        let format = caps
            .preferred_format()
            .ok_or(OffscreenError::Unusable("no surface format"))?;

        let device = instance
            .create_device(&DeviceDesc {
                label: Some("crcbl screenshot"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::GPU_DRIVEN | Features::DEBUG_MARKERS,
                compatible_surface: Some(surface),
            })
            .map_err(OffscreenError::Hal)?;

        let queue = device
            .queue(QueueKind::Graphics)
            .ok_or(OffscreenError::Unusable("no graphics queue"))?;

        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("screenshot ring"),
                surface,
                format,
                extent,
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: crate::hal::CompositeAlpha::Opaque,
            })
            .map_err(OffscreenError::Surface)?;

        let scene = SceneState::open(scene, device.as_ref(), queue, format)?;

        Ok(Self {
            instance,
            device,
            surface,
            swapchain,
            queue,
            format,
            scene,
            pool: TransientPool::new(),
        })
    }

    /// The surface format the readback bytes are in.
    ///
    /// The swapchain's preferred format is `Bgra8UnormSrgb` on most surfaces,
    /// and [`Self::draw_and_readback`] copies the swapchain image *raw*. A
    /// caller turning those bytes into an image therefore has to know whether
    /// to swizzle red and blue, and this is how it knows. Feeding them to an
    /// RGBA constructor unconditionally produces a channel-swapped PNG on
    /// every ordinary desktop surface.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    /// Which backend [`crate::backend::open`] selected for this frame.
    ///
    /// The frame itself cannot say: every backend renders the same scene
    /// through the same [`ForwardRenderer`], so a caller comparing pixels has
    /// no way to tell which one produced them. A test that pinned
    /// [`BACKEND_ENV_VAR`](crate::backend::BACKEND_ENV_VAR) and never checked
    /// would therefore pass identically on the backend it asked for and on the
    /// one it fell back to — "not supported here" arriving as a green run.
    #[must_use]
    pub fn backend(&self) -> crate::hal::BackendKind {
        self.device.backend()
    }

    /// What the opened device reported it can do.
    ///
    /// The selector this exists for is
    /// [`geometry_path`](crate::hal::DeviceCaps::geometry_path): the forward
    /// pass's indirect tail is chosen from it once, at build, and is otherwise
    /// invisible from outside — so this is how a caller learns which arm a
    /// frame was actually drawn through rather than assuming the one its
    /// developer's GPU happens to select.
    #[must_use]
    pub fn caps(&self) -> crate::hal::DeviceCaps {
        self.device.caps()
    }

    /// Records, submits, and reads back one frame.
    ///
    /// Returns the swapchain image's bytes as `((width, height), Vec<u8>)`,
    /// four bytes per pixel, row-major, top row first, in [`Self::format`]'s
    /// channel order — **not** necessarily RGBA.
    ///
    /// The pose is fixed at `t = 0`: a screenshot is a golden-image input, and
    /// a deterministic frame is the only kind worth comparing against a
    /// reference. (There was an `advance`/`elapsed` pair here to move the
    /// clock; nothing ever called it, so every screenshot rendered `t = 0`
    /// anyway and the state only made the frame look configurable.)
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Hal`] if recording, submission, or readback fail,
    /// [`OffscreenError::OutOfDate`] if the swapchain is stale,
    /// [`OffscreenError::TooLarge`] if the acquired extent's byte count does
    /// not fit in a `usize`, or [`OffscreenError::ReadbackTimeout`] if the copy
    /// has not landed after [`READBACK_DEADLINE`].
    pub fn draw_and_readback(&mut self) -> Result<((u32, u32), Vec<u8>), OffscreenError> {
        let device = self.device.as_ref();
        let acquired = device
            .acquire_next_frame(self.swapchain)
            .map_err(|error| match error {
                SurfaceError::OutOfDate => OffscreenError::OutOfDate,
                other => OffscreenError::Surface(other),
            })?;

        let extent = acquired.extent;
        // The extent comes back from the swapchain rather than from `open`, so
        // it is checked here too: `u32 * u32 * 4` overflows a `u32` and the
        // product has to survive narrowing to a `usize` before it can size a
        // staging buffer or a `Vec`.
        let too_large = || OffscreenError::TooLarge {
            width: extent.0,
            height: extent.1,
        };
        // The *staged* row pitch, padded to `READBACK_ROW_ALIGNMENT`; the
        // tightly packed pitch is what the caller gets back.
        let packed_pitch = u64::from(extent.0).checked_mul(4).ok_or_else(too_large)?;
        let staged_pitch = packed_pitch
            .checked_next_multiple_of(u64::from(READBACK_ROW_ALIGNMENT))
            .ok_or_else(too_large)?;
        let byte_count = staged_pitch
            .checked_mul(u64::from(extent.1))
            .ok_or_else(too_large)?;
        let packed_bytes = packed_pitch
            .checked_mul(u64::from(extent.1))
            .ok_or_else(too_large)?;
        let staged_capacity = usize::try_from(byte_count).map_err(|_| too_large())?;
        let host_capacity = usize::try_from(packed_bytes).map_err(|_| too_large())?;
        // `buffer_row_length` is in *texels*, and the padded pitch is a multiple
        // of 4 for every 4-byte format, so this division is exact.
        let staged_row_texels = u32::try_from(staged_pitch / 4).map_err(|_| too_large())?;

        // ---- render the frame through the graph ----

        let compiled = {
            let mut graph = RenderGraph::new(self.queue);
            // The same swapchain import for every scene: what differs between
            // them is which passes are hung off it, not how it is presented.
            let target = graph.import_image(
                "swapchain",
                ForwardRenderer::present_target(acquired.image, acquired.view, self.format, extent),
            );
            match &mut self.scene {
                SceneState::Cube {
                    camera,
                    light,
                    renderer,
                } => {
                    renderer.begin_frame(
                        device,
                        camera,
                        light,
                        ForwardRenderer::spin(0.0),
                        extent,
                    )?;
                    let _hdr = renderer.add_passes(&mut graph, target, extent);
                }
                SceneState::Sprite { renderer, sheets } => {
                    let sprites = sprite_scene(*sheets);
                    let aspect = extent.0 as f32 / extent.1 as f32;
                    renderer.begin_frame(
                        device,
                        &sprites,
                        sprite_camera().view_projection(aspect),
                        extent,
                    )?;
                    // Both of the passes below load rather than clear, so the
                    // scene supplies the background they composite onto.
                    graph
                        .add_render_pass("scene background")
                        .clear_color(target, SCENE_CLEAR)
                        .execute(|_| {});
                    renderer.add_pass(&mut graph, target);
                }
                SceneState::Ui { renderer, atlas } => {
                    // `scale` is 1.0 because every size in the draw list is
                    // already a fraction of this frame's extent; a second
                    // multiplier is a second thing that can disagree with it.
                    renderer.begin_frame(device, &ui_draw_list(extent), atlas, 1.0)?;
                    graph
                        .add_render_pass("scene background")
                        .clear_color(target, SCENE_CLEAR)
                        .execute(|_| {});
                    renderer.add_pass(&mut graph, target, extent);
                }
            }
            graph.compile(&self.pool)?
        };

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("screenshot frame"),
            queue: self.queue,
        });
        compiled.execute(device, &mut self.pool, encoder.as_mut(), None)?;

        // ---- readback: barrier to TransferSrc, copy, barrier back, submit ----

        let staging = device.create_buffer(&BufferDesc {
            label: Some("screenshot readback"),
            size: byte_count,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })?;

        let range = ImageSubresourceRange::all(self.format);
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                acquired.image,
                range,
                ResourceState::ColorAttachment,
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });

        encoder.copy_image_to_buffer(&BufferImageCopy {
            buffer: staging,
            buffer_offset: 0,
            buffer_row_length: staged_row_texels,
            buffer_image_height: 0,
            image: acquired.image,
            image_subresource: ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 0,
                base_layer: 0,
                layer_count: 1,
            },
            image_offset: Offset3d::default(),
            image_extent: Extent3d::d2(extent.0, extent.1),
        });

        let commands = encoder.finish()?;
        device.submit(self.queue, &SubmitInfo::new(&[commands]))?;
        device.present(
            self.queue,
            &PresentInfo {
                swapchain: self.swapchain,
                waits: acquired.present_semaphore.as_slice(),
                present_id: None,
            },
        )?;

        let readback = device.request_readback(&ReadbackDesc {
            label: Some("screenshot readback"),
            buffer: staging,
            offset: 0,
            size: byte_count,
            after: None,
        })?;

        let mut staged = vec![0u8; staged_capacity];

        let deadline = std::time::Instant::now() + READBACK_DEADLINE;
        loop {
            let state = device.poll_readback(readback, &mut staged)?;
            if let ReadbackState::Ready = state {
                break;
            }
            if std::time::Instant::now() > deadline {
                // The command buffer, staging buffer and readback are left
                // alone deliberately: the GPU may still be reading them, and
                // destroying them now would be worse than leaking them until
                // `finish` waits the device idle and drops it.
                return Err(OffscreenError::ReadbackTimeout(READBACK_DEADLINE));
            }
            std::thread::sleep(Duration::from_micros(100));
        }

        device.destroy_command_buffer(commands);
        device.destroy_buffer(staging);
        device.destroy_readback(readback);

        // Drop the row padding. Done here rather than left to the caller because
        // the pitch is this module's decision, and a caller that forgot it would
        // get a sheared image — the one failure a structural comparison sees and
        // a per-pixel one does not describe usefully.
        let pixels = compact_rows(&staged, staged_pitch, packed_pitch, host_capacity);

        Ok((extent, pixels))
    }

    /// Tears down in correct order: wait idle, destroy the scene and the
    /// transient pool, then swapchain → surface → device.
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Hal`] if the device could not be brought to idle — a
    /// device lost during the frame surfaces here and nowhere else, and a
    /// caller about to save the pixels as a golden image needs to be told
    /// before it trusts them. The teardown still runs either way; the failure
    /// is reported after it.
    pub fn finish(mut self) -> Result<(), OffscreenError> {
        let idle = self.device.wait_idle();
        // After the wait, because both of these hand handles back to a device
        // that may still be reading them. `SpriteRenderer` and `UiRenderer` warn
        // on a drop that skipped this; `ForwardRenderer` and `TransientPool`
        // leak silently, which is why the screenshot path used to.
        self.scene.destroy(self.device.as_ref());
        self.pool.destroy(self.device.as_ref());
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        drop(self.device);
        drop(self.instance);
        idle.map_err(OffscreenError::Hal)
    }
}

/// Copies `packed_pitch` bytes out of every `staged_pitch`-byte row.
///
/// A free function so the arithmetic is testable with no GPU: this is the step
/// that turns a padded readback back into an image, and getting it wrong shears
/// the frame by a few pixels per row — which looks like a rendering bug and is
/// not one.
///
/// A short final row is copied as far as it goes rather than dropped: a backend
/// that wrote less than it promised should produce a visibly truncated image,
/// not a panic in the middle of a screenshot.
fn compact_rows(staged: &[u8], staged_pitch: u64, packed_pitch: u64, packed_len: usize) -> Vec<u8> {
    if staged_pitch == packed_pitch {
        let end = packed_len.min(staged.len());
        return staged[..end].to_vec();
    }
    // Both pitches sized a `Vec` above, so both fit a `usize`.
    let staged_pitch = staged_pitch as usize;
    let packed_pitch = packed_pitch as usize;
    let mut packed = Vec::with_capacity(packed_len);
    let mut offset = 0usize;
    while offset < staged.len() && packed.len() < packed_len {
        let row_end = (offset + packed_pitch).min(staged.len());
        packed.extend_from_slice(&staged[offset..row_end]);
        offset += staged_pitch;
    }
    packed.truncate(packed_len);
    packed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The padding is only correct if it is dropped again, and this is the
    /// arithmetic that drops it. A GPU is not needed to check it, so it is
    /// checked in the plain suite that runs everywhere rather than only in the
    /// e2e run that needs two backends.
    #[test]
    fn a_padded_readback_compacts_back_to_a_tight_image() {
        // Three rows of two RGBA pixels each, staged at a 16-byte pitch.
        let staged = vec![
            1, 2, 3, 4, 5, 6, 7, 8, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, //
            9, 10, 11, 12, 13, 14, 15, 16, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, //
            17, 18, 19, 20, 21, 22, 23, 24, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
        ];
        let packed = compact_rows(&staged, 16, 8, 24);
        assert_eq!(
            packed,
            (1u8..=24).collect::<Vec<u8>>(),
            "the padding bytes must not survive"
        );
    }

    /// The unpadded case is the one every existing caller was already on, and
    /// it must stay a straight copy.
    #[test]
    fn an_unpadded_readback_is_copied_verbatim() {
        let staged: Vec<u8> = (0u8..32).collect();
        assert_eq!(compact_rows(&staged, 8, 8, 32), staged);
        // A staging buffer larger than the image is truncated, never read past.
        assert_eq!(compact_rows(&staged, 8, 8, 16), staged[..16].to_vec());
    }

    /// A backend that returned a short buffer must produce a short image, not
    /// an out-of-range slice.
    #[test]
    fn a_truncated_readback_does_not_panic() {
        let staged: Vec<u8> = (0u8..20).collect();
        let packed = compact_rows(&staged, 16, 8, 24);
        assert_eq!(packed, vec![0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19]);
    }

    /// The pitch rule the wgpu backend enforces, stated as arithmetic: every
    /// width has to land on a multiple of 256 bytes.
    #[test]
    fn the_staged_pitch_is_always_a_legal_wgpu_row_pitch() {
        for width in [1u32, 3, 32, 63, 64, 97, 256, 1920, 4096] {
            let packed = u64::from(width) * 4;
            let staged = packed
                .checked_next_multiple_of(u64::from(READBACK_ROW_ALIGNMENT))
                .expect("no overflow at these widths");
            assert!(staged >= packed, "{width}: padding may not shrink a row");
            assert_eq!(
                staged % u64::from(READBACK_ROW_ALIGNMENT),
                0,
                "{width}: wgpu refuses this pitch",
            );
            assert_eq!(staged % 4, 0, "{width}: texel count must be exact");
        }
    }

    /// `--size 4000000000x4000000000` used to reach an unchecked
    /// `width * height * 4`, and `--size 100000x100000` a 40 GB allocation.
    /// Both are now refused, and refused *before* a GPU is opened, which is
    /// what makes this testable without one.
    #[test]
    fn an_absurd_frame_is_refused_before_a_backend_is_opened() {
        for (width, height) in [
            (u32::MAX, u32::MAX),
            (4_000_000_000, 4_000_000_000),
            (100_000, 100_000),
            (MAX_DIMENSION + 1, 1),
            (1, MAX_DIMENSION + 1),
        ] {
            let error = OffscreenSetup::open(width, height, Scene::default())
                .err()
                .unwrap_or_else(|| panic!("{width}x{height} is not a frame"));
            assert!(
                matches!(error, OffscreenError::TooLarge { .. }),
                "{width}x{height}: {error}"
            );
        }
    }

    /// A zero edge would make a swapchain nothing can present, and the error
    /// says so rather than being a validation-layer message later.
    #[test]
    fn a_zero_sized_frame_is_refused() {
        for (width, height) in [(0, 16), (16, 0), (0, 0)] {
            assert!(
                matches!(
                    OffscreenSetup::open(width, height, Scene::default()),
                    Err(OffscreenError::Unusable(_))
                ),
                "{width}x{height} should be unusable"
            );
        }
    }

    /// The sprite scene is only diagnostic if it **batches**, and batching is
    /// the submission order: `A A B A` is three batches, `A A B B` is two and
    /// `A A A A` is one. One batch is exactly the case that hid the
    /// `SV_InstanceID` divergence, because with a single draw the SPIR-V and
    /// WGSL lowerings of the instance index are the same number.
    #[test]
    fn the_sprite_scene_submits_three_batches_and_returns_to_the_first_sheet() {
        let batches: Vec<usize> = SPRITE_ORDER.iter().fold(Vec::new(), |mut runs, sheet| {
            if runs.last() != Some(sheet) {
                runs.push(*sheet);
            }
            runs
        });
        assert_eq!(
            batches,
            vec![0, 1, 0],
            "the scene must interleave its sheets, not group them"
        );
        // The last batch is what the bug got wrong: its instances start at 3,
        // and a backend reading the wrong index draws instance 0 instead.
        assert!(
            SPRITE_ORDER.len() > batches.len(),
            "at least one batch must carry more than one instance"
        );
    }

    /// Every rectangle has to be inside the frame at every size the harness
    /// renders, or the scene silently loses the batch the size cropped — and a
    /// missing batch is the very thing it is there to catch.
    #[test]
    fn every_sprite_rectangle_is_on_screen_at_the_harness_sizes() {
        for (width, height) in [(256u32, 192u32), (97, 61), (1920, 1080), (192, 192)] {
            let half_width = SPRITE_HALF_HEIGHT * (width as f32 / height as f32);
            for rect in SPRITE_RECTS {
                let (left, right) = (rect[0], rect[0] + rect[2]);
                let (bottom, top) = (rect[1], rect[1] + rect[3]);
                assert!(
                    left > -half_width && right < half_width,
                    "{width}x{height}: {rect:?} runs off the side of a ±{half_width} view"
                );
                assert!(
                    bottom > -SPRITE_HALF_HEIGHT && top < SPRITE_HALF_HEIGHT,
                    "{width}x{height}: {rect:?} runs off the top or bottom"
                );
            }
        }
    }

    /// The UI scene has to exercise all three command kinds — a frame of
    /// rectangles never samples the glyph atlas, so a broken atlas binding
    /// would draw an identical picture on both backends.
    #[test]
    fn the_ui_scene_draws_text_a_rect_and_an_outline_inside_the_frame() {
        use crate::ui::draw_list::DrawCommand;

        for extent in [(256u32, 192u32), (97, 61), (1920, 1080)] {
            let list = ui_draw_list(extent);
            let (mut rects, mut outlines, mut texts) = (0, 0, 0);
            for command in list.commands() {
                match command {
                    DrawCommand::Rect { min, max, .. } => {
                        rects += 1;
                        assert!(min.x >= 0.0 && min.y >= 0.0, "{extent:?}: {command:?}");
                        assert!(
                            max.x <= extent.0 as f32 && max.y <= extent.1 as f32,
                            "{extent:?}: {command:?}"
                        );
                    }
                    DrawCommand::RectOutline { .. } => outlines += 1,
                    // The glyphs' extent is the atlas's business, so only the
                    // anchor is checked here.
                    DrawCommand::Text { pos, .. } => {
                        texts += 1;
                        assert!(
                            pos.x >= 0.0 && pos.y >= 0.0 && pos.y < extent.1 as f32,
                            "{extent:?}: {command:?}"
                        );
                    }
                }
            }
            assert!(
                rects >= 2 && outlines == 1 && texts >= 1,
                "{extent:?}: {rects} rect(s), {outlines} outline(s), {texts} text(s)"
            );
        }
    }

    /// The null backend exercises the same code paths but renders nothing.
    /// The test exists to prove the module compiles and the HAL seam is
    /// wired correctly; pixel content is tested by the Vulkan e2e suite.
    #[test]
    fn offscreen_setup_opens_with_null_backend() {
        // Force the null backend to avoid needing a real GPU.
        let instance = crate::backend::open_backend(crate::backend::GpuBackend::Null)
            .expect("null backend is always compiled in");

        let surface = unsafe {
            instance
                .create_surface(&SurfaceTarget::Offscreen)
                .expect("null accepts every surface")
        };
        let adapters = instance.adapters();
        let adapter = adapters
            .first()
            .expect("null backend always has an adapter");

        let caps = instance
            .surface_caps(surface, adapter.id)
            .expect("null reports surface caps");
        let format = caps.preferred_format().expect("null has a format");

        let device = instance
            .create_device(&DeviceDesc {
                label: Some("screenshot test"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features: Features::GPU_DRIVEN,
                compatible_surface: Some(surface),
            })
            .expect("null device");

        let queue = device
            .queue(QueueKind::Graphics)
            .expect("null has a graphics queue");

        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("test ring"),
                surface,
                format,
                extent: (16, 16),
                image_count: 2,
                present_mode: PresentMode::Fifo,
                composite_alpha: crate::hal::CompositeAlpha::Opaque,
            })
            .expect("null creates a swapchain");

        let mut renderer = ForwardRenderer::new(device.as_ref(), queue, format)
            .expect("null builds the forward renderer");

        // Acquire, record, submit — no readback on the null backend.
        let acquired = device
            .acquire_next_frame(swapchain)
            .expect("null ring has an image");

        let mut graph = RenderGraph::new(queue);
        let target = graph.import_image(
            "swapchain",
            ForwardRenderer::present_target(acquired.image, acquired.view, format, (16, 16)),
        );
        let pool = TransientPool::new();
        let _hdr = renderer.add_passes(&mut graph, target, (16, 16));

        let compiled = graph.compile(&pool).expect("graph compiles");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("test frame"),
            queue,
        });
        compiled
            .execute(
                device.as_ref(),
                &mut TransientPool::new(),
                encoder.as_mut(),
                None,
            )
            .expect("graph executes");
        let commands = encoder.finish().expect("encoding succeeds");
        device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");

        device.destroy_command_buffer(commands);
        device.destroy_swapchain(swapchain);
        instance.destroy_surface(surface);
        drop(device);
        drop(instance);
    }
}
