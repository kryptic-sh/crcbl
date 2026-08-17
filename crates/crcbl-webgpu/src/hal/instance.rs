//! `impl Instance for WebGpuInstance` — adapters, surfaces, the device request.

use std::sync::Mutex;

use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, CompositeAlpha, DeviceDesc, Format, HalError, Instance,
    PendingDevice, PresentMode, SurfaceCaps, SurfaceHandle,
};

use crate::device::DeviceProbe;

use super::channel::{HandlePool, SharedChannel};
use super::device::WebGpuPendingDevice;

/// The opened instance: the adapters the browser granted, and the channel every
/// object it makes encodes through.
///
/// Built by [`WebGpuInstanceOpen`](super::open::WebGpuInstanceOpen) once the
/// enumeration is answered, so [`adapters`](Instance::adapters) is a settled
/// `Vec`, not a query — WebGPU grants one adapter or none, and this is that
/// list.
#[derive(Debug)]
pub struct WebGpuInstance {
    channel: SharedChannel,
    adapters: Vec<AdapterInfo>,
    pool: HandlePool,
    /// The surfaces created from [`SurfaceTarget::Offscreen`], so
    /// [`surface_caps`](Instance::surface_caps) can tell a ring of textures from
    /// a canvas.
    ///
    /// A [`SurfaceHandle`] carries no kind and the stream answers no query about
    /// one, so the only place the distinction exists is the `create_surface`
    /// call that made it — which is why it is recorded there and nowhere else.
    /// The same slot table `crcbl-wgpu`'s instance keeps, minus the platform
    /// object a browser has none of.
    ///
    /// A `Vec` rather than a set because a run holds one surface or two, and
    /// [`HandlePool`] never mints the same handle twice, so a removed entry can
    /// never be re-matched by a later one.
    offscreen: Mutex<Vec<SurfaceHandle>>,
    /// What the browser said a canvas accepts, fetched during the open — or the
    /// reason it said nothing, which becomes the
    /// [`HalError::Backend`] a canvas query answers with.
    ///
    /// One value for every canvas surface, because
    /// [`Command::SurfaceCaps`](crate::Command::SurfaceCaps) names no surface
    /// and no adapter: `navigator.gpu.getPreferredCanvasFormat()` is a fact
    /// about the browser, not about a canvas. The offscreen surfaces are the
    /// ones this does not describe — see [`offscreen_surface_caps`].
    canvas: Result<SurfaceCaps, String>,
}

impl WebGpuInstance {
    /// Assemble an instance from a settled enumeration and a settled canvas
    /// capability query. Called by the open future; not a public entry point,
    /// since both must already be the browser's answer.
    pub(crate) fn new(
        channel: SharedChannel,
        adapters: Vec<AdapterInfo>,
        pool: HandlePool,
        canvas: Result<SurfaceCaps, String>,
    ) -> Self {
        Self {
            channel,
            adapters,
            pool,
            offscreen: Mutex::new(Vec::new()),
            canvas,
        }
    }

    /// A clone of the channel this instance encodes through, for feeding replies
    /// in a test.
    #[must_use]
    pub fn channel(&self) -> SharedChannel {
        self.channel.clone()
    }

    /// [`request_device`](Instance::request_device), keeping the concrete type
    /// so a caller (and a test) can read the request's waiting sequence off the
    /// [`WebGpuPendingDevice`] it returns.
    ///
    /// # Errors
    ///
    /// [`HalError::NoSuchAdapter`] for an adapter this instance never
    /// enumerated — decided now, per the seam — or [`HalError::Backend`] if the
    /// channel would not take the request.
    pub(crate) fn open_device(
        &self,
        desc: &DeviceDesc<'_>,
    ) -> Result<WebGpuPendingDevice, HalError> {
        // Everything decidable without the browser is decided here: an unknown
        // adapter is an `Err` from this call, not a deferred failure. The
        // features cross unfiltered — the replayer holds the WebGPU vocabulary
        // and is where a required bit it cannot satisfy is refused.
        self.adapters
            .iter()
            .find(|adapter| adapter.id == desc.adapter)
            .ok_or(HalError::NoSuchAdapter(desc.adapter.0))?;

        let probe = self
            .channel
            .with(|channel| DeviceProbe::request(channel, desc))
            .ok_or_else(|| {
                HalError::Backend(
                    "the WebGPU stream channel would not accept the device request".to_string(),
                )
            })?;

        Ok(WebGpuPendingDevice::new(
            self.channel.clone(),
            probe,
            self.pool.clone(),
        ))
    }

    /// The offscreen-surface list, locked.
    fn offscreen_surfaces(&self) -> std::sync::MutexGuard<'_, Vec<SurfaceHandle>> {
        self.offscreen
            .lock()
            .expect("the WebGPU offscreen-surface list mutex was poisoned")
    }

    /// Whether `surface` came from [`SurfaceTarget::Offscreen`].
    ///
    /// `false` for a handle this instance never made, which is the same answer a
    /// canvas gets: the seam has no "no such surface" error on this call, and a
    /// stray handle is a caller bug that the swapchain command will surface
    /// against the replayer's own surface table.
    fn is_offscreen(&self, surface: SurfaceHandle) -> bool {
        self.offscreen_surfaces().contains(&surface)
    }
}

/// What an offscreen ring accepts, which is not what a canvas accepts.
///
/// **THE RING ANSWERS TO NO CANVAS.** `GPUCanvasContext.configure` takes only
/// `rgba8unorm` and `bgra8unorm` and rejects an `-srgb` one, which is why
/// [`WebGpuInstance::surface_caps`] reports those for a canvas surface — but an
/// offscreen swapchain is a ring of `GPUTexture`s the replayer creates with
/// `createTexture`, where every one of these is a core, renderable, copyable
/// format. Nothing about the canvas restriction applies to it.
///
/// **sRGB FIRST, BECAUSE THAT IS THE FRAME THE GOLDENS HOLD.**
/// [`SurfaceCaps::preferred_format`] takes the first sRGB entry, and the engine
/// asks for exactly that. Every pass above the seam writes display-referred
/// values and leaves the encode to the hardware, so a linear ring skips the
/// encode entirely and the readback is a whole transfer function away from the
/// image it is compared against.
///
/// **Deliberately the same list `crcbl-vk` and `crcbl-wgpu` state for their own
/// offscreen surfaces**, minus `Rgba16Float`, so the parity gate compares a
/// frame drawn into the same format the native suites drew into — which is the
/// only reason the gate exists. `Rgba16Float` is absent because
/// [`crate::tag`] can name it but nothing has ever asked this backend for a
/// half-float ring, and a `formats` entry is a promise that a swapchain can be
/// created with it.
fn offscreen_surface_caps() -> SurfaceCaps {
    SurfaceCaps {
        formats: vec![
            Format::Rgba8UnormSrgb,
            Format::Bgra8UnormSrgb,
            Format::Rgba8Unorm,
            Format::Bgra8Unorm,
        ],
        // `fifo` is what the ring rotates at — there is no display to pace
        // against, and the seam promises `Fifo` is always offered.
        present_modes: vec![PresentMode::Fifo],
        // The replayer's offscreen branch has no `alphaMode` to set at all; the
        // textures are opaque render targets read straight back.
        composite_alpha: vec![CompositeAlpha::Opaque],
        // The replayer builds `imageCount` textures from the descriptor and
        // refuses only a count of zero, so this is a statement about the caller's
        // clamp rather than a measurement: two is the ring the canvas branch
        // reports and the one the engine has always asked this backend for.
        min_image_count: 2,
        max_image_count: 2,
        // Nothing here has an opinion about the size; the replayer sizes the
        // ring from the swapchain descriptor.
        current_extent: None,
    }
}

impl Instance for WebGpuInstance {
    fn backend(&self) -> BackendKind {
        BackendKind::WebGpu
    }

    fn adapters(&self) -> Vec<AdapterInfo> {
        self.adapters.clone()
    }

    unsafe fn create_surface(&self, target: &SurfaceTarget) -> Result<SurfaceHandle, HalError> {
        // Two targets are reachable in a browser, and each has its own command.
        // `Web { canvas_id }` resolves a canvas out of the shim's registry;
        // `Offscreen` names no canvas and needs a ring of textures nothing
        // presents. The four *pointer* variants carry `NonNull`s to platform
        // objects, and a pointer sent as an integer is the one thing this seam
        // refuses outright — the encoders take a `u32` or nothing, never a
        // `SurfaceTarget`, so the refusal has to land here, where the target is
        // still whole.
        let surface: SurfaceHandle = self.pool.alloc();
        match *target {
            SurfaceTarget::Web { canvas_id } => {
                self.channel.with(|channel| {
                    channel.encode(|stream| stream.create_surface(surface, canvas_id))
                });
            }
            SurfaceTarget::Offscreen => {
                // Recorded before the encode, so `surface_caps` on the very next
                // line — which is where `crcbl::engine`'s open calls it — can
                // already tell this handle from a canvas one.
                self.offscreen_surfaces().push(surface);
                self.channel.with(|channel| {
                    channel.encode(|stream| stream.create_offscreen_surface(surface))
                });
            }
            _ => {
                return Err(HalError::Unsupported {
                    backend: BackendKind::WebGpu,
                    what: "only a Web canvas or an Offscreen surface is reachable on the \
                           WebGPU stream; the pointer-carrying targets never cross the \
                           wasm boundary",
                });
            }
        }
        Ok(surface)
    }

    fn destroy_surface(&self, surface: SurfaceHandle) {
        self.offscreen_surfaces().retain(|held| *held != surface);
        self.channel
            .with(|channel| channel.encode(|stream| stream.destroy_surface(surface)));
    }

    fn surface_caps(
        &self,
        surface: SurfaceHandle,
        _adapter: AdapterId,
    ) -> Result<SurfaceCaps, HalError> {
        // Answered synchronously, with no round trip of its own.
        // `crcbl::engine`'s open calls `create_surface` and then this on the
        // next line with no frame between them, so a query issued here could not
        // answer in time, and an `Err` meaning "not yet" would send a caller
        // doing adapter selection to an adapter a browser does not have. The
        // canvas answer is the browser's all the same: it was fetched during the
        // instance open, alongside the enumeration, and is held in `canvas`.
        //
        // WHICH OF THE TWO ANSWERS DEPENDS ON WHAT THE SURFACE IS, and that is
        // the whole reason `offscreen` exists. A canvas is bound by
        // `GPUCanvasContext.configure`, which takes only the linear 8-bit
        // formats; an offscreen ring is a set of `GPUTexture`s the replayer
        // creates and no canvas constrains. Answering the canvas's list for both
        // handed `SurfaceCaps::preferred_format` nothing sRGB to find, so it
        // fell through to the first entry and the ring was allocated linear —
        // every pass above the seam writes display-referred values and leaves
        // the encode to the hardware, so the frame was never encoded and came
        // back a whole transfer function away from the golden it is compared to.
        if self.is_offscreen(surface) {
            return Ok(offscreen_surface_caps());
        }
        // THE BROWSER'S OWN ANSWER, LED BY `getPreferredCanvasFormat()`. Both
        // canvas formats are valid `GPUCanvasConfiguration.format` values, so a
        // list in the other order is not refused — it makes the browser insert a
        // full-canvas copy on every present, and say so. Neither is sRGB, so
        // `SurfaceCaps::preferred_format` falls through to the first entry,
        // which is what makes the browser's ordering the whole of the answer.
        //
        // The `Err` half is `Reply::SurfaceCapsFailed`'s reason, and this is the
        // call it belongs to: `surface_caps` is the only one whose docs oblige a
        // caller to treat an `Err` as "try the next adapter", so a query the
        // browser refused is reported here rather than having failed the open
        // that the offscreen surfaces do not need it for.
        self.canvas.clone().map_err(HalError::Backend)
    }

    fn request_device(&self, desc: &DeviceDesc<'_>) -> Result<Box<dyn PendingDevice>, HalError> {
        self.open_device(desc)
            .map(|pending| Box::new(pending) as Box<dyn PendingDevice>)
    }
}
