//! The HAL impls against an injected reply channel.
//!
//! No browser and no shim: a test builds the replies the browser would send with
//! [`ReplyWriter`], hands them to the channel through
//! [`StreamChannel::commit_replies`](crate::web::StreamChannel::commit_replies),
//! and the impl under test drains them exactly as it would a shim-committed
//! buffer. The command side is checked by decoding the stream the impl left.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

use crcbl_core::Handle;
use crcbl_core::SurfaceTarget;
use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, BindGroupDesc, BindGroupEntry, BindGroupLayoutDesc,
    BindGroupLayoutEntry, BindingFlags, BindingKind, BindingResource, BufferDesc, BufferHandle,
    BufferUsage, CommandEncoderDesc, CompositeAlpha, ComputePipelineDesc, Device, DeviceCaps,
    DeviceDesc, DeviceType, Extent3d, Features, Format, HalError, ImageAspect, ImageDesc,
    ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc, ImageViewType, Instance, Limits,
    MemoryLocation, PendingDevice, PresentMode, QueueKind, ReadbackDesc, ReadbackState,
    SamplerDesc, ShaderEntry, ShaderModuleDesc, ShaderStages, SubmitInfo, SurfaceCaps,
    SurfaceHandle, SwapchainDesc,
};

use crate::ReplyWriter;
use crate::decode_stream;

use super::channel::{HandlePool, SharedChannel};
use super::device::WebGpuDevice;
use super::instance::WebGpuInstance;
use super::open::WebGpuInstanceOpen;

// ── fixtures ───────────────────────────────────────────────────────────────

/// An adapter shaped the way the browser's replayer builds one.
fn granted_adapter() -> AdapterInfo {
    AdapterInfo {
        id: AdapterId(0),
        name: "llvmpipe".into(),
        vendor_id: 0,
        device_id: 0,
        device_type: DeviceType::Other,
        driver: String::new(),
        backend: BackendKind::WebGpu,
        caps: device_caps(),
    }
}

/// What the browser's replayer answers `Command::SurfaceCaps` with where
/// `getPreferredCanvasFormat()` is `"rgba8unorm"` — `web/engine/gpu-replay.js`'s
/// `surfaceCapsFor`, field for field: the sRGB counterpart of the preference
/// first, then the other counterpart, then the two formats a canvas can actually
/// be configured with, the preference still ahead of the other.
fn browser_canvas_caps() -> SurfaceCaps {
    SurfaceCaps {
        formats: vec![
            Format::Rgba8UnormSrgb,
            Format::Bgra8UnormSrgb,
            Format::Rgba8Unorm,
            Format::Bgra8Unorm,
        ],
        present_modes: vec![PresentMode::Fifo],
        composite_alpha: vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied],
        min_image_count: 2,
        max_image_count: 2,
        current_extent: None,
    }
}

fn device_caps() -> DeviceCaps {
    DeviceCaps {
        features: Features::COMPUTE,
        limits: Limits::minimum(),
    }
}

fn buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("test"),
        size: 256,
        usage: BufferUsage::STORAGE,
        memory: MemoryLocation::DeviceLocal,
    }
}

fn noop_context() -> Context<'static> {
    Context::from_waker(Waker::noop())
}

/// Commit a reply stream to `channel`, the way a shim would.
fn feed(channel: &SharedChannel, build: impl FnOnce(&mut ReplyWriter)) {
    let mut writer = ReplyWriter::new();
    build(&mut writer);
    let accepted = channel.with(|c| c.commit_replies(writer.bytes()));
    assert!(accepted, "the channel must accept the injected replies");
}

/// Drive an open future to its instance on the two replies the open waits for.
///
/// `enumerate_adapters` is the first awaited command on a fresh channel and the
/// canvas capability query the second, so their sequences are 0 and 1.
fn opened_instance() -> WebGpuInstance {
    let mut open = WebGpuInstanceOpen::start();
    let channel = open.channel();
    feed(&channel, |w| {
        w.adapter(0, &granted_adapter());
        w.surface_caps(1, &browser_canvas_caps());
    });
    match Pin::new(&mut open).poll(&mut noop_context()) {
        Poll::Ready(Ok(instance)) => instance,
        other => panic!("the instance must open on the browser's two replies, got {other:?}"),
    }
}

/// A device on a fresh channel, without the open/request dance — for tests that
/// only care what the device encodes.
fn device_on_fresh_channel() -> (SharedChannel, WebGpuDevice) {
    let channel = SharedChannel::new();
    let device = WebGpuDevice::new(channel.clone(), device_caps(), HandlePool::new());
    (channel, device)
}

// ── (a) instance open ──────────────────────────────────────────────────────

#[test]
fn instance_open_settles_to_an_instance_on_the_browsers_two_replies() {
    let mut open = WebGpuInstanceOpen::start();
    let channel = open.channel();

    // Nothing fed yet: both questions are in flight, which is `Pending`.
    assert!(
        matches!(Pin::new(&mut open).poll(&mut noop_context()), Poll::Pending),
        "an unanswered enumeration polls Pending"
    );

    // The adapter alone is not enough: `surface_caps` is synchronous once the
    // instance exists, so an instance built before the canvas answer arrived
    // would have nothing to answer a canvas query with but a guess.
    let info = granted_adapter();
    feed(&channel, |w| w.adapter(0, &info));
    assert!(
        matches!(Pin::new(&mut open).poll(&mut noop_context()), Poll::Pending),
        "an unanswered canvas capability query polls Pending"
    );

    feed(&channel, |w| w.surface_caps(1, &browser_canvas_caps()));
    let Poll::Ready(Ok(instance)) = Pin::new(&mut open).poll(&mut noop_context()) else {
        panic!("the second reply must settle the open future");
    };
    assert_eq!(instance.adapters(), vec![info]);
    assert_eq!(instance.backend(), BackendKind::WebGpu);
}

/// A canvas query the browser refused is an `Err` from `surface_caps`, not a
/// failed open: the reason belongs to the call whose `Err` half is ordinary, and
/// an offscreen surface on the same instance does not depend on it.
#[test]
fn a_refused_canvas_query_opens_the_instance_and_fails_the_canvas_call() {
    let mut open = WebGpuInstanceOpen::start();
    let channel = open.channel();
    feed(&channel, |w| {
        w.adapter(0, &granted_adapter());
        w.surface_caps_failed(
            1,
            "getPreferredCanvasFormat() answered \"rgba16float\"",
            crate::SurfaceCapsFailure::Backend,
        );
    });
    let Poll::Ready(Ok(instance)) = Pin::new(&mut open).poll(&mut noop_context()) else {
        panic!("a refused canvas query must still open the instance");
    };

    let canvas = unsafe { instance.create_surface(&SurfaceTarget::Web { canvas_id: 7 }) }
        .expect("a Web canvas surface is reachable");
    let Err(HalError::Backend(reason)) = instance.surface_caps(canvas, AdapterId(0)) else {
        panic!("the canvas query answers the refusal it was given");
    };
    assert!(reason.contains("rgba16float"), "{reason}");

    let offscreen = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
        .expect("an offscreen surface is reachable");
    assert!(
        instance.surface_caps(offscreen, AdapterId(0)).is_ok(),
        "a ring of GPUTextures is not described by the canvas answer",
    );
}

// ── (b) surface caps ───────────────────────────────────────────────────────

#[test]
fn surface_caps_answers_the_fetched_canvas_caps_synchronously() {
    let instance = opened_instance();
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Web { canvas_id: 7 }) }
        .expect("a Web canvas surface is reachable");

    // No reply is fed *here*: the answer crossed during the open, and the call
    // itself must not need a frame.
    let caps = instance
        .surface_caps(surface, AdapterId(0))
        .expect("the caps fetched during the open");
    assert_eq!(
        caps.formats,
        vec![
            Format::Rgba8UnormSrgb,
            Format::Bgra8UnormSrgb,
            Format::Rgba8Unorm,
            Format::Bgra8Unorm,
        ]
    );
    assert_eq!(caps.present_modes, vec![PresentMode::Fifo]);
    assert_eq!(
        caps.composite_alpha,
        vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied]
    );
    assert_eq!((caps.min_image_count, caps.max_image_count), (2, 2));
    assert_eq!(caps.current_extent, None);
}

/// **The canvas caps lead with the sRGB counterpart of the format the browser
/// said it prefers**, and the browser's own answer still leads the pair a canvas
/// can be configured with.
///
/// Two separate claims, and both are load-bearing.
/// [`SurfaceCaps::preferred_format`](crcbl_hal::SurfaceCaps::preferred_format)
/// takes the first sRGB entry, so what the engine asks for is that counterpart —
/// every pass above the seam writes display-referred values and leaves the encode
/// to the hardware, so a linear target presents a transfer function too dark. And
/// a `GPUCanvasContext` configured with anything but
/// `navigator.gpu.getPreferredCanvasFormat()` makes the browser insert a
/// full-canvas copy on every present, which it warns about and every visitor pays
/// for the life of the page — so the base format the page configures has to stay
/// the browser's. The two are not in tension: `viewFormats` is what lets one
/// canvas be configured `rgba8unorm` and viewed `rgba8unorm-srgb`.
///
/// Red while the canvas branch answered a constant: the constant led with
/// `Bgra8Unorm`, and `getPreferredCanvasFormat()` is `"rgba8unorm"` on Chromium
/// under Linux and on every Apple device. Red again while it offered nothing
/// sRGB at all, which is the frame the demo site shipped.
#[test]
fn canvas_caps_lead_with_the_format_the_browser_prefers() {
    let mut open = WebGpuInstanceOpen::start();
    let channel = open.channel();
    // Sequence 0 is the enumeration and 1 the capability query: `start` encodes
    // them in that order on a fresh channel.
    feed(&channel, |w| {
        w.adapter(0, &granted_adapter());
        w.surface_caps(1, &browser_canvas_caps());
    });
    let Poll::Ready(Ok(instance)) = Pin::new(&mut open).poll(&mut noop_context()) else {
        panic!("both replies are fed, so the open future must settle to an instance");
    };

    let canvas = unsafe { instance.create_surface(&SurfaceTarget::Web { canvas_id: 7 }) }
        .expect("a Web canvas surface is reachable");
    let caps = instance
        .surface_caps(canvas, AdapterId(0))
        .expect("the browser answered the capability query");
    assert_eq!(
        caps.formats,
        vec![
            Format::Rgba8UnormSrgb,
            Format::Bgra8UnormSrgb,
            Format::Rgba8Unorm,
            Format::Bgra8Unorm,
        ],
        "the canvas reports the sRGB counterparts first and the two configurable \
         formats behind them, the browser's preference leading each pair",
    );
    assert_eq!(
        caps.preferred_format(),
        Some(Format::Rgba8UnormSrgb),
        "what the engine asks for is the sRGB view of what the browser prefers",
    );
    assert_eq!(
        caps.formats
            .iter()
            .find(|format| !format.is_srgb())
            .copied(),
        Some(Format::Rgba8Unorm),
        "and the first format a canvas can actually be CONFIGURED with is still \
         the browser's own, which is what costs no copy per present",
    );
}

/// **Both surfaces offer an sRGB format, and they are still two different
/// answers.**
///
/// Every pass above the seam writes display-referred values and leaves the encode
/// to the hardware, so both branches have to hand
/// [`SurfaceCaps::preferred_format`](crcbl_hal::SurfaceCaps::preferred_format)
/// something sRGB or the frame is never encoded — the ring by *being* allocated
/// in it, the canvas by being configured in the base format and viewed through
/// the counterpart. That leaves the two format lists agreeing whenever the
/// browser prefers `rgba8unorm`, which is Chromium under Linux and every Apple
/// device, so **the format list is no longer what tells the branches apart** and
/// this test keys on `composite_alpha` instead: only a canvas has a
/// `GPUCanvasConfiguration.alphaMode` to offer `PreMultiplied` from.
///
/// Red the moment either branch takes the other's caps whole.
#[test]
fn both_surfaces_are_offered_an_srgb_format_and_still_answer_differently() {
    let instance = opened_instance();
    let offscreen = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
        .expect("an offscreen surface is reachable");
    let canvas = unsafe { instance.create_surface(&SurfaceTarget::Web { canvas_id: 7 }) }
        .expect("a Web canvas surface is reachable");

    let offscreen_caps = instance
        .surface_caps(offscreen, AdapterId(0))
        .expect("the offscreen caps");
    assert_eq!(
        offscreen_caps.formats,
        vec![
            Format::Rgba8UnormSrgb,
            Format::Bgra8UnormSrgb,
            Format::Rgba8Unorm,
            Format::Bgra8Unorm,
        ],
        "the offscreen ring lists its sRGB formats first",
    );
    assert_eq!(
        offscreen_caps.preferred_format(),
        Some(Format::Rgba8UnormSrgb),
        "the engine asks for what preferred_format picks, and a golden image \
         wants a display-referred frame",
    );

    let canvas_caps = instance
        .surface_caps(canvas, AdapterId(0))
        .expect("the canvas caps");
    assert_eq!(
        canvas_caps.formats,
        vec![
            Format::Rgba8UnormSrgb,
            Format::Bgra8UnormSrgb,
            Format::Rgba8Unorm,
            Format::Bgra8Unorm,
        ],
        "a canvas reports the sRGB views it can be given ahead of the two \
         formats GPUCanvasContext.configure actually takes",
    );
    assert_eq!(
        canvas_caps.composite_alpha,
        vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied],
        "a canvas has an alphaMode and offers both spellings of it",
    );
    assert_eq!(
        offscreen_caps.composite_alpha,
        vec![CompositeAlpha::Opaque],
        "an offscreen ring has no alphaMode at all — its textures are opaque \
         render targets read straight back",
    );
    assert_ne!(
        canvas_caps, offscreen_caps,
        "the two answers must not collapse into one, whatever the format lists \
         happen to agree on",
    );
}

/// The kind is per surface, so destroying the offscreen one must not leave a
/// later canvas surface answering the ring's caps — the handle pool never
/// repeats a value, but the list would still match on a stale entry if nothing
/// removed it.
///
/// Keyed on `composite_alpha` for the reason
/// [`both_surfaces_are_offered_an_srgb_format_and_still_answer_differently`] is:
/// the two format lists agree under a browser that prefers `rgba8unorm`, so a
/// check on `formats` here would pass whether the handle was forgotten or not.
#[test]
fn destroying_an_offscreen_surface_forgets_that_it_was_offscreen() {
    let instance = opened_instance();
    let offscreen = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
        .expect("an offscreen surface is reachable");
    assert_eq!(
        instance
            .surface_caps(offscreen, AdapterId(0))
            .expect("the offscreen caps")
            .composite_alpha,
        vec![CompositeAlpha::Opaque],
    );

    instance.destroy_surface(offscreen);
    assert_eq!(
        instance
            .surface_caps(offscreen, AdapterId(0))
            .expect("caps for a destroyed handle")
            .composite_alpha,
        vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied],
        "a handle this instance no longer holds falls back to the canvas answer",
    );
}

#[test]
fn create_surface_refuses_a_pointer_target() {
    let instance = opened_instance();
    // A pointer-carrying target — its `NonNull` never crosses the wasm boundary,
    // so it is the one shape this seam refuses outright. `dangling` is never
    // dereferenced; the refusal is decided from the variant alone.
    let target = SurfaceTarget::AppKit {
        layer: core::ptr::NonNull::<core::ffi::c_void>::dangling(),
    };
    let refused = unsafe { instance.create_surface(&target) };
    assert!(
        matches!(refused, Err(HalError::Unsupported { .. })),
        "only a Web canvas or an Offscreen surface is reachable on the stream"
    );
}

#[test]
fn create_surface_encodes_an_offscreen_surface_command() {
    let instance = opened_instance();
    let channel = instance.channel();
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Offscreen) }
        .expect("an offscreen surface is reachable");

    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    assert_eq!(
        commands.last().map(crate::Command::name),
        Some("CreateOffscreenSurface"),
        "the offscreen target encodes a CreateOffscreenSurface command",
    );
    let Some(crate::Command::CreateOffscreenSurface { surface: encoded }) = commands.last() else {
        panic!("the last command is a CreateOffscreenSurface");
    };
    assert_eq!(*encoded, surface, "the command names the allocated surface");
}

// ── (c) device request ─────────────────────────────────────────────────────

#[test]
fn a_device_request_polls_ready_on_a_device_reply() {
    let instance = opened_instance();
    let channel = instance.channel();
    let desc = DeviceDesc {
        label: Some("engine"),
        adapter: AdapterId(0),
        required_features: Features::COMPUTE,
        optional_features: Features::empty(),
        compatible_surface: None,
    };

    let mut pending = instance.open_device(&desc).expect("the request encodes");
    assert_eq!(pending.backend(), BackendKind::WebGpu);
    let sequence = pending.sequence().expect("a fresh request waits");

    // Before the reply, the request is Pending.
    assert!(
        matches!(pending.poll(), Ok(crcbl_hal::DeviceRequestState::Pending)),
        "an unanswered request polls Pending"
    );

    feed(&channel, |w| w.device(sequence, &device_caps()));
    let state = pending.poll().expect("the poll succeeds");
    assert!(state.is_ready(), "the device opens on the device reply");
    let device = state.into_device().expect("the device is there");
    assert_eq!(device.caps(), device_caps());

    // Polling again after Ready is a caller bug, reported not repeated.
    assert!(
        matches!(pending.poll(), Err(HalError::InvalidDescriptor(_))),
        "a poll after completion is refused"
    );
}

#[test]
fn a_device_request_for_an_unknown_adapter_is_refused_up_front() {
    let instance = opened_instance();
    let desc = DeviceDesc::for_adapter(AdapterId(9));
    assert!(
        matches!(instance.open_device(&desc), Err(HalError::NoSuchAdapter(9))),
        "an adapter this instance never enumerated is an Err from the request"
    );
}

/// A zero-size buffer is refused here, and nothing is encoded for it.
///
/// **The seam's rule that this backend alone did not keep.**
/// `BufferDesc::size` is documented "must be non-zero"; the null, Vulkan, Metal
/// and D3D12 backends each answer [`HalError::InvalidDescriptor`], and this one
/// allocated a handle, encoded a `CreateBuffer` and answered `Ok`. Nothing
/// downstream would have caught it — the stream refuses malformed streams, not
/// invalid descriptors.
///
/// The four native backends are held to the same rule by
/// `a_zero_size_buffer_is_refused_by_every_backend` in
/// `crates/crcbl/tests/hal_seam_e2e.rs`, which cannot reach this one: that suite
/// is a native binary and `CRCBL_GPU` names no browser. This is the WebGPU half,
/// and it is the half that was wrong.
///
/// **Both halves are the check.** A refusal alone is satisfiable by a backend
/// that refuses everything, so the encoder is asserted silent: a rejected
/// descriptor must not have put a command on the wire for a handle the caller
/// never received. A legal size beside it keeps the check a floor rather than a
/// refusal of small buffers.
#[test]
fn a_zero_size_buffer_is_refused_without_encoding_anything() {
    let (channel, device) = device_on_fresh_channel();

    for memory in [
        MemoryLocation::DeviceLocal,
        MemoryLocation::HostUpload,
        MemoryLocation::HostReadback,
    ] {
        let error = device
            .create_buffer(&BufferDesc {
                size: 0,
                memory,
                ..buffer_desc()
            })
            .err()
            .unwrap_or_else(|| {
                panic!("a zero-size buffer in {memory:?} must be refused, not served")
            });
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "{memory:?}: a zero size is a caller bug named in the descriptor, not an \
             unsupported feature: {error}"
        );
    }
    let names = |channel: &SharedChannel| -> Vec<&'static str> {
        channel
            .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode")
            .iter()
            .map(crate::Command::name)
            .collect()
    };
    assert!(
        names(&channel).is_empty(),
        "a refused descriptor must not reach the wire: the replayer would make a \
         buffer for a handle this call never handed back, and got {:?}",
        names(&channel)
    );

    // The smallest legal size still works, and does encode.
    device
        .create_buffer(&BufferDesc {
            size: 1,
            ..buffer_desc()
        })
        .expect("one byte is a legal buffer");
    assert_eq!(
        names(&channel),
        vec!["CreateBuffer"],
        "one byte must still reach the replayer, or the check above is refusing \
         every buffer and this test would not know"
    );
}

/// A view of mips or layers the image does not have is refused, and nothing is
/// encoded for it.
///
/// **The seam's subresource rule, which this backend tracked no state to ask.**
/// `Device::create_image_view` documents an out-of-range subresource as
/// [`HalError::InvalidDescriptor`]; `crcbl-mtl` and `crcbl-dx12` refuse one
/// from their own image tables, and this device kept no image table at all, so
/// the range was encoded exactly as handed over. The browser does refuse a bad
/// `createView` — but a frame later, in its own words about a texture, through
/// `take_error`, rather than as this call's answer.
///
/// The volume case is the one a wrong reading of `Extent3d::depth_or_layers`
/// inverts: it is a 3D image's *depth*, so a 64-deep image has **one** array
/// layer, and filing the raw field would serve a view of its 64th.
///
/// **Both halves are the check.** Each refusal is paired with the encoder
/// falling silent — a rejected descriptor must not put a command on the wire
/// for a handle the caller never received — and with a legal view beside it,
/// which is what stops a `check` that refused everything passing this test.
#[test]
fn a_view_of_mips_or_layers_the_image_lacks_is_refused_without_encoding_anything() {
    let (channel, device) = device_on_fresh_channel();
    let names = |channel: &SharedChannel| -> Vec<&'static str> {
        channel
            .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode")
            .iter()
            .map(crate::Command::name)
            .collect()
    };
    let format = Format::Rgba8Unorm;
    let image = |image_type, depth_or_layers, mip_levels| {
        device
            .create_image(&ImageDesc {
                label: Some("view subject"),
                image_type,
                extent: Extent3d {
                    width: 8,
                    height: 8,
                    depth_or_layers,
                },
                format,
                mip_levels,
                samples: 1,
                usage: ImageUsage::SAMPLED,
            })
            .expect("the descriptor is one this device serves")
    };
    let view = |image, base_mip, mip_count, base_layer, layer_count| {
        device.create_image_view(&ImageViewDesc {
            label: Some("subrange"),
            image,
            view_type: ImageViewType::D2Array,
            format,
            range: ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip,
                mip_count,
                base_layer,
                layer_count,
            },
        })
    };
    let all = ImageSubresourceRange::ALL;

    // Four mips over three array layers — no two of the image's numbers agree
    // — and a 64-deep volume, which has one array layer.
    let array = image(ImageType::D2, 3, 4);
    let volume = image(ImageType::D3, 64, 1);
    assert_eq!(
        names(&channel),
        vec!["CreateImage", "CreateImage"],
        "both images must reach the replayer before anything below is judged"
    );

    for (handle, base_mip, mip_count, base_layer, layer_count, what) in [
        (array, 4, all, 0, all, "a view starting past the last mip"),
        (array, 0, all, 3, all, "a view starting past the last layer"),
        (array, 0, 0, 0, all, "a view of no mip levels"),
        (array, 0, all, 0, 0, "a view of no layers"),
        (array, 2, 3, 0, all, "a mip range running past the last mip"),
        (array, 0, all, 1, 3, "a layer range past the last layer"),
        (
            volume,
            0,
            all,
            1,
            all,
            "the second array layer of a 64-deep volume, which has one",
        ),
    ] {
        let error = view(handle, base_mip, mip_count, base_layer, layer_count)
            .expect_err(&format!("{what} must be refused"));
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "{what}: {error:?}"
        );
        assert_eq!(
            names(&channel),
            vec!["CreateImage", "CreateImage"],
            "{what}: a refused descriptor must not reach the wire, and got {:?}",
            names(&channel)
        );
    }

    // An image this device never created is a handle error rather than a
    // descriptor one, and is also not encoded.
    let stranger = view(
        Handle::from_bits(1 << 32).expect("a non-zero generation"),
        0,
        all,
        0,
        all,
    )
    .expect_err("an image handle this device never issued is not viewable");
    assert!(
        matches!(stranger, HalError::InvalidHandle { .. }),
        "{stranger:?}"
    );

    // …and the legal views still go through, or the refusals above are a
    // device that refuses every view and this test would not know.
    for (handle, base_mip, mip_count, base_layer, layer_count, what) in [
        (
            array,
            0,
            all,
            0,
            all,
            "the whole image through both sentinels",
        ),
        (array, 3, all, 2, all, "the last mip of the last layer"),
        (array, 1, 2, 1, 2, "a subrange ending exactly at the end"),
        (volume, 0, all, 0, all, "the whole volume"),
    ] {
        view(handle, base_mip, mip_count, base_layer, layer_count)
            .unwrap_or_else(|error| panic!("{what} is a legal view: {error:?}"));
    }
    assert_eq!(
        names(&channel),
        vec![
            "CreateImage",
            "CreateImage",
            "CreateImageView",
            "CreateImageView",
            "CreateImageView",
            "CreateImageView",
        ],
        "every legal view must still reach the replayer"
    );

    // **A frame the swapchain hands out is an image too**, and one
    // `acquire_next_frame` mints rather than `create_image`. Unrecorded it
    // would be an unknown handle above, so a view of an acquired frame — which
    // `crcbl-vk` serves, its WSI images sitting in the very pool
    // `create_image_view` looks in — would be refused on this backend alone.
    // A canvas texture is one mip of one array layer, which is the shape a view
    // of it is held to.
    let handles = HandlePool::new();
    let swapchain = device
        .create_swapchain(&SwapchainDesc {
            label: Some("canvas"),
            surface: handles.alloc(),
            format: Format::Rgba8UnormSrgb,
            extent: (64, 48),
            image_count: 2,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        })
        .expect("a 64x48 swapchain is a size a canvas can have");
    let frame = device
        .acquire_next_frame(swapchain)
        .expect("WebGPU's acquire answers in the call");
    // The swapchain's own format, not this test's image format: a view may not
    // reinterpret, so a view of the acquired texture asks for what the canvas
    // was configured with. Asking for `Rgba8Unorm` here is refused, which is
    // the rule and not an accident of this fixture.
    let frame_view = |base_mip, mip_count, base_layer, layer_count| {
        device.create_image_view(&ImageViewDesc {
            label: Some("frame subrange"),
            image: frame.image,
            view_type: ImageViewType::D2Array,
            format: Format::Rgba8UnormSrgb,
            range: ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip,
                mip_count,
                base_layer,
                layer_count,
            },
        })
    };
    frame_view(0, all, 0, all)
        .expect("the whole of an acquired frame is a view a caller may ask for");
    frame_view(1, all, 0, all)
        .expect_err("a canvas texture has one mip, so there is no second one to view");
    frame_view(0, all, 1, all)
        .expect_err("a canvas texture has one array layer, so there is no second one to view");
    view(frame.image, 0, all, 0, all)
        .expect_err("a view may not reinterpret the canvas's format as another");
}

/// The seam's checks that this backend used to run nowhere at all.
///
/// Each is a rule `crcbl-hal` states and provides the check for, that every
/// other backend calls and this one did not — so a descriptor breaking it was
/// refused on four backends and encoded here. None of the three can be caught
/// downstream: `web/engine/gpu-replay.js` re-checks only what WebGPU itself has
/// a rule about, and WebGPU has no rule about any of these. A zero `count` and
/// a duplicate binding are not WebGPU errors; `workgroupSize` is dropped on the
/// wire, because `GPUComputePipelineDescriptor` has no member for it; and a
/// module carrying only SPIR-V becomes a `createShaderModule` with empty
/// `code`, which fails later naming neither the module nor the format shipped.
///
/// Every case pairs its refusal with the nearest legal descriptor, which must
/// still encode — a check that refused everything would satisfy the first half
/// alone, and this suite would not know.
#[test]
fn the_seam_checks_this_backend_skipped_now_run_before_anything_is_encoded() {
    let names = |channel: &SharedChannel| -> Vec<&'static str> {
        channel
            .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode")
            .iter()
            .map(crate::Command::name)
            .collect()
    };
    let entry = |binding, count, flags| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        kind: BindingKind::StorageBuffer {
            read_only: true,
            dynamic: false,
        },
        count,
        flags,
    };

    // `check_entries`: a zero count, and a binding declared twice.
    for (case, entries) in [
        ("a zero count", vec![entry(0, 0, BindingFlags::empty())]),
        (
            "a binding declared twice",
            vec![
                entry(0, 1, BindingFlags::empty()),
                entry(0, 1, BindingFlags::empty()),
            ],
        ),
    ] {
        let (channel, device) = device_on_fresh_channel();
        let error = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("layout"),
                entries: &entries,
            })
            .err()
            .unwrap_or_else(|| panic!("{case} is a caller bug the seam states"));
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "{case}: {error}"
        );
        assert!(
            names(&channel).is_empty(),
            "{case} reached the wire: {:?}",
            names(&channel)
        );
    }
    // …and a layout that breaks neither still encodes.
    let (channel, device) = device_on_fresh_channel();
    device
        .create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("layout"),
            entries: &[entry(0, 1, BindingFlags::empty())],
        })
        .expect("one storage binding breaks no rule");
    assert_eq!(names(&channel), vec!["CreateBindGroupLayout"]);

    // `check_workgroup_size`: zero in an axis, and past this device's ceiling.
    // The two handles are never resolved — the check runs before the descriptor
    // is encoded, which is the property under test — but they must be handles a
    // pool actually issued, since `from_bits` refuses a made-up bit pattern.
    let handles = HandlePool::new();
    let ceiling = Limits::minimum().max_compute_workgroup_size;
    for (case, size) in [
        ("zero in an axis", [1, 0, 1]),
        ("past the device's ceiling", [ceiling[0] + 1, 1, 1]),
    ] {
        let (channel, device) = device_on_fresh_channel();
        let error = device
            .create_compute_pipeline(&ComputePipelineDesc {
                label: Some("compute"),
                layout: handles.alloc(),
                compute: ShaderEntry {
                    module: handles.alloc(),
                    entry_point: "main",
                },
                workgroup_size: size,
            })
            .err()
            .unwrap_or_else(|| panic!("{case} launches nothing this device can run"));
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "{case}: {error}"
        );
        assert!(
            names(&channel).is_empty(),
            "{case} reached the wire: {:?}",
            names(&channel)
        );
    }
    // …and the largest size the device does allow still encodes.
    let (channel, device) = device_on_fresh_channel();
    device
        .create_compute_pipeline(&ComputePipelineDesc {
            label: Some("compute"),
            layout: handles.alloc(),
            compute: ShaderEntry {
                module: handles.alloc(),
                entry_point: "main",
            },
            workgroup_size: [ceiling[0], 1, 1],
        })
        .expect("the device's own ceiling is a size it can launch");
    assert_eq!(names(&channel), vec!["CreateComputePipeline"]);

    // A module carrying every format but the one this backend compiles.
    let (channel, device) = device_on_fresh_channel();
    let error = device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("spirv only"),
            spirv: &[0x0723_0203],
            wgsl: None,
            msl: Some("kernel void main() {}"),
            dxil: &[],
        })
        .expect_err("WebGPU compiles WGSL and nothing else");
    let HalError::ShaderCompilation(message) = &error else {
        panic!("an unusable module is a compilation failure, not {error}");
    };
    // Both sides, which is what `ShaderModuleDesc::unusable` exists to say.
    assert!(message.contains("spirv only"), "{message}");
    assert!(message.contains("WGSL"), "{message}");
    assert!(
        names(&channel).is_empty(),
        "an uncompilable module reached the wire: {:?}",
        names(&channel)
    );
    // …and the same module with WGSL on it encodes.
    device
        .create_shader_module(&ShaderModuleDesc {
            label: Some("wgsl"),
            spirv: &[],
            wgsl: Some("@compute @workgroup_size(1) fn main() {}"),
            msl: None,
            dxil: &[],
        })
        .expect("WGSL is what this backend compiles");
    assert_eq!(names(&channel), vec!["CreateShaderModule"]);
}

/// `ImageDesc::check` and the swapchain extent rule, neither of which this
/// backend used to run.
///
/// The image rules are the seam's, moved out of the null backend so a second
/// caller could have them rather than a second copy of them; the extent rule is
/// the one sentence every other backend already answers and
/// `crates/crcbl/tests/hal_seam_e2e.rs` asserts — a suite that is a native
/// binary and so cannot reach this backend, which is why the rule went
/// unenforced here and why the test has to live in this file.
///
/// The replayer stands in for neither: `createTexture` and `context.configure`
/// throw, and what comes back is the browser's sentence about a texture or a
/// canvas, a frame late through `take_error`, rather than the seam's about a
/// descriptor.
#[test]
fn image_and_swapchain_descriptors_are_checked_before_anything_is_encoded() {
    let names = |channel: &SharedChannel| -> Vec<&'static str> {
        channel
            .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode")
            .iter()
            .map(crate::Command::name)
            .collect()
    };
    let image = |mip_levels, samples, extent| ImageDesc {
        label: Some("image"),
        image_type: ImageType::D2,
        extent,
        format: Format::Rgba8UnormSrgb,
        mip_levels,
        samples,
        usage: ImageUsage::SAMPLED,
    };
    let one = Extent3d {
        width: 64,
        height: 64,
        depth_or_layers: 1,
    };

    for (case, desc) in [
        ("a zero extent", image(1, 1, Extent3d { width: 0, ..one })),
        ("no mip levels at all", image(0, 1, one)),
        ("more mips than the extent holds", image(99, 1, one)),
        (
            "a sample count that is not a power of two",
            image(1, 3, one),
        ),
    ] {
        let (channel, device) = device_on_fresh_channel();
        let error = device
            .create_image(&desc)
            .expect_err("{case} describes no image any device could make");
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "{case}: {error}"
        );
        assert!(
            names(&channel).is_empty(),
            "{case} reached the wire: {:?}",
            names(&channel)
        );
    }
    // …and an ordinary image still encodes.
    let (channel, device) = device_on_fresh_channel();
    device
        .create_image(&image(1, 1, one))
        .expect("a 64x64 single-sampled image breaks no rule");
    assert_eq!(names(&channel), vec!["CreateImage"]);

    // The swapchain extent. The surface handle is never resolved — the check
    // runs before the descriptor is encoded, which is the property under test —
    // but it must be one a pool issued, since `from_bits` refuses a made-up
    // pattern.
    let handles = HandlePool::new();
    let surface: SurfaceHandle = handles.alloc();
    let swapchain = |extent| SwapchainDesc {
        label: Some("swapchain"),
        surface,
        format: Format::Rgba8UnormSrgb,
        extent,
        image_count: 2,
        present_mode: PresentMode::Fifo,
        composite_alpha: CompositeAlpha::Opaque,
    };
    for extent in [(0, 48), (64, 0), (0, 0)] {
        let (channel, device) = device_on_fresh_channel();
        let error = device
            .create_swapchain(&swapchain(extent))
            .expect_err("a zero extent means 'not yet', not 'guess a size'");
        // The sentence every backend answers, which the agnostic suite matches
        // on and this backend is not in.
        assert!(
            format!("{error}").contains("do not create one yet"),
            "{extent:?}: {error}"
        );
        assert!(
            names(&channel).is_empty(),
            "{extent:?} reached the wire: {:?}",
            names(&channel)
        );

        // And the reconfigure path, which is the one the rule is really about:
        // a window minimised mid-run resizes its swapchain to zero.
        let error = device
            .reconfigure_swapchain(handles.alloc(), &swapchain(extent))
            .expect_err("a minimised window means 'not yet' on this path too");
        assert!(
            format!("{error}").contains("do not create one yet"),
            "{extent:?} on reconfigure: {error}"
        );
        assert!(
            names(&channel).is_empty(),
            "{extent:?} reached the wire on reconfigure: {:?}",
            names(&channel)
        );
    }

    // …and a swapchain with a real extent still encodes, on both paths.
    let (channel, device) = device_on_fresh_channel();
    let handle = device
        .create_swapchain(&swapchain((64, 48)))
        .expect("a 64x48 swapchain is a size a surface can have");
    device
        .reconfigure_swapchain(handle, &swapchain((32, 24)))
        .expect("so is 32x24");
    assert_eq!(
        names(&channel),
        vec!["CreateSwapchain", "ReconfigureSwapchain"]
    );
}

/// The anisotropy **floor**, which this backend ran no check for at all — and
/// the ceiling, which it deliberately does not run.
///
/// `create_sampler` used to allocate a handle and encode the descriptor
/// whatever was in it, so a `0.5` or a `NAN` crossed to the browser and came
/// back — if at all — as `createSampler`'s sentence a frame later through
/// `take_error`. The floor is device-independent, so it is the seam's answer
/// here as on every other backend.
///
/// **The accepting arm above `1.0` is the load-bearing one.** This device
/// reports `max_sampler_anisotropy: 1.0`, meaning "no ceiling this backend can
/// guarantee" rather than "more than one is refused" — WebGPU has no query for
/// a device's maximum. `16.0` being *accepted* against a reported limit of
/// `1.0` is what pins that decision down: the day someone folds the floor and
/// the ceiling into one call and wires it here, this is the assertion that goes
/// red instead of anisotropic filtering going quietly unreachable. See
/// `docs/backlog.md`, "Anisotropy: the limit says one, the replayer passes more
/// through".
#[test]
fn a_samplers_anisotropy_is_held_to_the_floor_and_not_to_the_reported_ceiling() {
    let names = |channel: &SharedChannel| -> Vec<&'static str> {
        channel
            .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode")
            .iter()
            .map(crate::Command::name)
            .collect()
    };
    let sampler = |anisotropy| SamplerDesc {
        label: Some("anisotropy"),
        anisotropy,
        ..SamplerDesc::default()
    };

    for (case, anisotropy) in [
        ("below the floor", 0.5),
        // Neither comparison catches this on its own: `NAN < 1.0` is `false`
        // and so is `NAN > max`, which is how a NaN reached a driver on every
        // backend before the floor was written to take it.
        ("a NaN", f32::NAN),
    ] {
        let (channel, device) = device_on_fresh_channel();
        let error = device
            .create_sampler(&sampler(anisotropy))
            .err()
            .unwrap_or_else(|| panic!("{case} is below the value that disables anisotropy"));
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "{case}: {error}"
        );
        assert!(
            names(&channel).is_empty(),
            "{case} reached the wire: {:?}",
            names(&channel)
        );
    }

    let ceiling = device_caps().limits.max_sampler_anisotropy;
    assert_eq!(
        ceiling, 1.0,
        "this fixture is the reported-limit-of-one case the arm below is about"
    );
    for (case, anisotropy) in [
        ("the floor exactly", 1.0),
        ("well above the reported limit", 16.0),
    ] {
        let (channel, device) = device_on_fresh_channel();
        device
            .create_sampler(&sampler(anisotropy))
            .unwrap_or_else(|error| panic!("{case} must reach the replayer: {error}"));
        assert_eq!(names(&channel), vec!["CreateSampler"], "{case}");
    }
}

/// A write or a readback past the end of the buffer it names, and a handle this
/// device never issued.
///
/// Both ranges are decidable here and were decided nowhere: `write_buffer`
/// checked only that its end address fits a `u64`, and `request_readback`
/// checked nothing at all. `queue.writeBuffer` past the end *is* a WebGPU
/// validation error, so the refusal did arrive — a frame later, in the
/// browser's words, through `take_error`, which is the same "arrives, but not
/// from the call" exception `create_pipeline_layout` documents. The seam says
/// `InvalidDescriptor` from the call, as the other four backends answer.
///
/// The stale-handle arm is the half a size table makes possible at all: with no
/// record of the buffer there is nothing to compare against, so a handle from a
/// destroyed or foreign buffer used to encode a command naming an id the
/// replayer would fail to resolve, a frame away.
#[test]
fn a_write_or_readback_past_the_end_of_its_buffer_is_refused() {
    let (channel, device) = device_on_fresh_channel();
    let names = |channel: &SharedChannel| -> Vec<&'static str> {
        channel
            .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode")
            .iter()
            .map(crate::Command::name)
            .collect()
    };
    // `buffer_desc` is 256 bytes.
    let buffer = device.create_buffer(&buffer_desc()).expect("a buffer");
    assert_eq!(names(&channel), vec!["CreateBuffer"]);

    let readback = |offset, size| {
        device.request_readback(&ReadbackDesc {
            label: None,
            buffer,
            offset,
            size,
            after: None,
        })
    };

    // One byte past the end, from both directions, on both calls.
    for (case, error) in [
        (
            "a write past the end",
            device.write_buffer(buffer, 250, &[0; 7]),
        ),
        (
            "a write starting past the end",
            device.write_buffer(buffer, 256, &[0; 1]),
        ),
    ] {
        let error = error.expect_err("{case} runs off the end of a 256-byte buffer");
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "{case}: {error}"
        );
        assert!(format!("{error}").contains("256-byte"), "{case}: {error}");
    }
    for (case, offset, size) in [
        ("a readback past the end", 250, 7),
        ("a readback starting past the end", 256, 1),
    ] {
        let error = readback(offset, size).expect_err("{case} reads off the end");
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "{case}: {error}"
        );
        assert!(format!("{error}").contains("256-byte"), "{case}: {error}");
    }
    assert_eq!(
        names(&channel),
        vec!["CreateBuffer"],
        "a refused range must not reach the wire"
    );

    // Exactly to the end is legal on both, which is what keeps the two checks
    // from being off-by-one in the direction that refuses valid work.
    device
        .write_buffer(buffer, 250, &[0; 6])
        .expect("a write ending exactly at the end is in range");
    readback(250, 6).expect("so is a readback");

    // And a destroyed buffer is stale rather than silently encoded — the arm
    // the table makes possible at all, since with no record there is nothing to
    // compare a range against. (A buffer from *another* device is not tested
    // here: `device_on_fresh_channel` gives each device its own `HandlePool`
    // starting at the same place, so two devices' first buffers share their
    // bits. Cross-device handles are the agnostic suite's
    // `a_handle_from_one_device_is_foreign_to_another`, on backends whose
    // handles carry a device tag.)
    device.destroy_buffer(buffer);
    let error = device
        .write_buffer(buffer, 0, &[0; 4])
        .expect_err("a destroyed buffer is not one to write into");
    assert!(
        matches!(error, HalError::InvalidHandle { .. }),
        "a stale handle is stale, not a bad descriptor: {error}"
    );
}

/// The two binding rules that need the layout's `BindingKind` and the buffer's
/// size and location together, neither of which this backend used to keep.
///
/// A range over the slot's ceiling is what WebGPU calls
/// `maxUniformBufferBindingSize` and validates itself, so that one did arrive —
/// through `take_error`, a frame late, rather than from the call. The
/// device-local rule has no WebGPU equivalent at all: the browser is happy to
/// bind a `COPY_DST | STORAGE` buffer to a writable slot, so a caller who got
/// it wrong found out on D3D12 or not at all.
///
/// `WHOLE_BUFFER` is the arm that decides whether the size table was worth
/// keeping: it is the commoner spelling and carries no number, so before there
/// was a size to resolve it against, checking explicit sizes alone would have
/// been a guard that misses most bindings.
#[test]
fn a_buffer_binding_is_held_to_its_slots_ceiling_and_memory() {
    let (channel, device) = device_on_fresh_channel();
    let names = |channel: &SharedChannel| -> Vec<&'static str> {
        channel
            .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode")
            .iter()
            .map(crate::Command::name)
            .collect()
    };
    // `Limits::minimum` puts the uniform ceiling at 64 KiB and the storage one
    // far above it, which is why the ceiling arms use a uniform slot.
    let ceiling = Limits::minimum().max_uniform_buffer_range;
    let layout_of = |kind| {
        device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("one buffer"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    kind,
                    count: 1,
                    flags: BindingFlags::empty(),
                }],
            })
            .expect("one buffer binding")
    };
    let buffer_of = |size, memory| {
        device
            .create_buffer(&BufferDesc {
                size,
                memory,
                ..buffer_desc()
            })
            .expect("a buffer")
    };
    let group_of = |layout, buffer, offset, size| {
        device.create_bind_group(&BindGroupDesc {
            label: None,
            layout,
            entries: &[BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: BindingResource::Buffer {
                    buffer,
                    offset,
                    size,
                },
            }],
            variable_count: None,
        })
    };

    let uniform = layout_of(BindingKind::UniformBuffer { dynamic: false });
    let writable = layout_of(BindingKind::StorageBuffer {
        read_only: false,
        dynamic: false,
    });
    let read_only = layout_of(BindingKind::StorageBuffer {
        read_only: true,
        dynamic: false,
    });
    let small = buffer_of(4096, MemoryLocation::DeviceLocal);
    let over = buffer_of(ceiling + 4096, MemoryLocation::DeviceLocal);
    let upload = buffer_of(4096, MemoryLocation::HostUpload);
    let readback = buffer_of(4096, MemoryLocation::HostReadback);
    let encoded = names(&channel).len();

    // An explicit range one byte over the uniform ceiling…
    let error = group_of(uniform, small, 0, ceiling + 1)
        .expect_err("a range over the ceiling is not a binding this device can make");
    assert!(
        format!("{error}").contains(&ceiling.to_string()),
        "the refusal names the limit: {error}"
    );
    // …and the same ceiling reached through `WHOLE_BUFFER`, which is the arm a
    // size table exists for.
    let error = group_of(uniform, over, 0, BindingResource::WHOLE_BUFFER)
        .expect_err("a whole buffer over the ceiling is over it too");
    assert!(
        format!("{error}").contains(&ceiling.to_string()),
        "the whole-buffer arm names the limit: {error}"
    );

    for (case, buffer) in [("HostUpload", upload), ("HostReadback", readback)] {
        let error = group_of(writable, buffer, 0, BindingResource::WHOLE_BUFFER)
            .expect_err("a shader cannot write host-visible memory");
        let text = format!("{error}");
        assert!(text.contains(case), "{case}: {text}");
        assert!(text.contains("DeviceLocal"), "{case}: {text}");

        // The same buffer in a read-only slot is untouched by the rule.
        group_of(read_only, buffer, 0, BindingResource::WHOLE_BUFFER)
            .expect("a read-only storage binding of host memory is fine");
    }

    // A binding the layout does not declare is a caller bug, not a pass.
    let error = device
        .create_bind_group(&BindGroupDesc {
            label: None,
            layout: uniform,
            entries: &[BindGroupEntry {
                binding: 7,
                array_index: 0,
                resource: BindingResource::Buffer {
                    buffer: small,
                    offset: 0,
                    size: BindingResource::WHOLE_BUFFER,
                },
            }],
            variable_count: None,
        })
        .expect_err("binding 7 is not in a layout that declares only binding 0");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error}");

    // Nothing refused reached the wire — only the four accepted bind groups
    // (two read-only arms) and the creates before them.
    let after = names(&channel);
    assert_eq!(
        after.iter().filter(|n| **n == "CreateBindGroup").count(),
        2,
        "only the two read-only arms may have encoded: {after:?}"
    );
    assert_eq!(
        after.len(),
        encoded + 2,
        "a refused binding must not reach the wire: {after:?}"
    );

    // …and an ordinary in-range device-local binding still encodes.
    group_of(uniform, small, 0, BindingResource::WHOLE_BUFFER)
        .expect("4096 bytes of device-local memory is an ordinary uniform binding");
    assert_eq!(names(&channel).len(), encoded + 3);
}

/// A buffer binding's offset is held to the alignment its slot requires, which
/// this backend used to send to the wire unmeasured.
///
/// [`crcbl_hal::check_binding_offset_alignment`] is the rule, and it needs only
/// the slot's [`BindingKind`] and the offset in the descriptor — no buffer
/// record at all — so it is the cheapest of the three rules
/// `check_buffer_bindings` answers and was the one missing. A browser reports
/// the violation as a `GPUValidationError` on the queue: asynchronous, so it
/// arrives after `create_bind_group` has already returned a handle the caller
/// went on using.
///
/// The device here is opened on [`Limits::desktop`] rather than the suite's
/// usual [`Limits::minimum`], because the two alignments are equal on the
/// minimum (256 apiece) and an offset legal in one slot and refused in the
/// other is what shows the check reads the *slot* rather than one shared
/// number.
///
/// **The accepting arm runs first**, and the wire is counted at the end: a
/// check that refused every binding would satisfy both refusals below and
/// break every frame.
#[test]
fn a_binding_offset_is_held_to_its_slots_alignment() {
    let channel = SharedChannel::new();
    let device = WebGpuDevice::new(
        channel.clone(),
        DeviceCaps {
            features: Features::COMPUTE,
            limits: Limits::desktop(),
        },
        HandlePool::new(),
    );
    let names = |channel: &SharedChannel| -> Vec<&'static str> {
        channel
            .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode")
            .iter()
            .map(crate::Command::name)
            .collect()
    };
    assert_eq!(Limits::desktop().min_uniform_buffer_offset_alignment, 64);
    assert_eq!(Limits::desktop().min_storage_buffer_offset_alignment, 16);

    let layout_of = |kind| {
        device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("one buffer"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    kind,
                    count: 1,
                    flags: BindingFlags::empty(),
                }],
            })
            .expect("one buffer binding")
    };
    let uniform = layout_of(BindingKind::UniformBuffer { dynamic: false });
    let storage = layout_of(BindingKind::StorageBuffer {
        read_only: true,
        dynamic: false,
    });
    let buffer = device
        .create_buffer(&BufferDesc {
            size: 4096,
            memory: MemoryLocation::DeviceLocal,
            ..buffer_desc()
        })
        .expect("a buffer");
    let group_of = |layout, offset| {
        device.create_bind_group(&BindGroupDesc {
            label: None,
            layout,
            entries: &[BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: BindingResource::Buffer {
                    buffer,
                    offset,
                    size: 256,
                },
            }],
            variable_count: None,
        })
    };
    let encoded = names(&channel).len();

    group_of(uniform, 128).expect("128 is a multiple of 64");
    group_of(storage, 32).expect("32 is a multiple of 16");

    // 32 is a legal *storage* offset and an illegal uniform one.
    let error = group_of(uniform, 32).expect_err("32 is not a multiple of 64");
    let text = format!("{error}");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{text}");
    assert!(text.contains("binding 0"), "{text}");
    assert!(text.contains("64-byte"), "{text}");

    let error = group_of(storage, 8).expect_err("8 is not a multiple of 16");
    let text = format!("{error}");
    assert!(matches!(error, HalError::InvalidDescriptor(_)), "{text}");
    assert!(text.contains("16-byte"), "{text}");

    let after = names(&channel);
    assert_eq!(
        after.len(),
        encoded + 2,
        "the two aligned bindings encoded and neither refusal did: {after:?}"
    );
}

/// A present with no frame to present, which three backends refuse and this one
/// answered `Ok` to.
///
/// The second arm is the one that matters: `reconfigure_swapchain` clears the
/// acquired pair *and destroys the image behind it*, so a present after one is
/// a use-after-free that reached the replayer as an ordinary command. The seam
/// entry that recorded this hole believed `crcbl-vk` had it too; it does not —
/// its reconfigure replaces the whole swapchain entry, whose `acquired` starts
/// `None`, and its present already refused.
#[test]
fn a_present_with_no_acquired_frame_is_refused() {
    let (channel, device) = device_on_fresh_channel();
    let names = |channel: &SharedChannel| -> Vec<&'static str> {
        channel
            .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode")
            .iter()
            .map(crate::Command::name)
            .collect()
    };
    let handles = HandlePool::new();
    let queue: crcbl_hal::QueueHandle = handles.alloc();
    let desc = |extent| SwapchainDesc {
        label: Some("swapchain"),
        surface: handles.alloc(),
        format: Format::Rgba8UnormSrgb,
        extent,
        image_count: 2,
        present_mode: PresentMode::Fifo,
        composite_alpha: CompositeAlpha::Opaque,
    };
    let swapchain = device
        .create_swapchain(&desc((64, 48)))
        .expect("a swapchain");
    let present_now = || {
        device.present(
            queue,
            &crcbl_hal::PresentInfo {
                swapchain,
                waits: &[],
                present_id: None,
            },
        )
    };

    // Nothing acquired yet.
    let error = present_now().expect_err("there is no frame to present");
    assert!(
        format!("{error}").contains("without a matching acquire_next_frame"),
        "the wording every other backend answers: {error}"
    );
    let before = names(&channel);
    assert!(
        !before.contains(&"Present"),
        "a refused present must not reach the wire: {before:?}"
    );

    // Acquired: the present goes through.
    device
        .acquire_next_frame(swapchain)
        .expect("an acquire on a live swapchain");
    present_now().expect("an acquired frame is one to present");
    assert!(names(&channel).contains(&"Present"));

    // …and only once. The pair stays filed until the next acquire retires it,
    // so `SwapchainState::presented` is what makes a second present a refusal
    // here, where the other backends simply take their slot.
    let error = present_now().expect_err("that frame has already been presented");
    assert!(
        format!("{error}").contains("without a matching acquire_next_frame"),
        "{error}"
    );

    // A fresh acquire clears it again.
    device
        .acquire_next_frame(swapchain)
        .expect("a second acquire");
    present_now().expect("a newly acquired frame is one to present");

    // …and a reconfigure clears it again, which is the use-after-free arm: the
    // image that pair named has been destroyed by the reconfigure.
    device
        .reconfigure_swapchain(swapchain, &desc((32, 24)))
        .expect("a reconfigure");
    let error = present_now().expect_err("the reconfigure destroyed the frame that was acquired");
    assert!(
        format!("{error}").contains("without a matching acquire_next_frame"),
        "{error}"
    );
}

// ── (d) a recorded frame's command stream ──────────────────────────────────

#[test]
fn a_buffer_encoder_draw_finish_submit_encodes_the_expected_commands() {
    let (channel, device) = device_on_fresh_channel();

    let buffer = device
        .create_buffer(&buffer_desc())
        .expect("a buffer handle");
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("the graphics queue");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
    encoder.draw(0..3, 0..1);
    let command_buffer = encoder.finish().expect("finish with only wired ops");
    device
        .submit(
            queue,
            &SubmitInfo {
                command_buffers: &[command_buffer],
                waits: &[],
                signals: &[],
            },
        )
        .expect("submit");

    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    let names: Vec<_> = commands.iter().map(crate::Command::name).collect();
    assert_eq!(
        names,
        vec![
            "CreateBuffer",
            "CreateCommandEncoder",
            "Draw",
            "Finish",
            "Submit",
        ],
        "the recorded frame is exactly these commands in order"
    );
    let _ = buffer;
}

// ── write_buffer: the host→buffer upload ───────────────────────────────────

#[test]
fn write_buffer_encodes_a_write_buffer_command() {
    let (channel, device) = device_on_fresh_channel();

    let buffer = device
        .create_buffer(&buffer_desc())
        .expect("a buffer handle");
    device
        .write_buffer(buffer, 8, &[0xCA, 0xFE, 0xBA, 0xBE])
        .expect("write_buffer is wired");

    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    assert_eq!(
        commands.last().map(crate::Command::name),
        Some("WriteBuffer"),
        "the upload encodes a WriteBuffer command",
    );
    let Some(crate::Command::WriteBuffer {
        buffer: encoded,
        offset,
        data,
    }) = commands.last()
    else {
        panic!("the last command is a WriteBuffer");
    };
    assert_eq!(*encoded, buffer, "the write names the created buffer");
    assert_eq!(*offset, 8, "the byte offset crosses");
    assert_eq!(
        data.as_slice(),
        &[0xCA, 0xFE, 0xBA, 0xBE],
        "the bytes cross"
    );
}

// ── (e) readback ───────────────────────────────────────────────────────────

#[test]
fn a_readback_polls_ready_on_a_readback_reply() {
    let (channel, device) = device_on_fresh_channel();

    // A real source buffer, because `request_readback` holds its range against
    // the size this device recorded and refuses a handle it never issued. That
    // makes `CreateBuffer` sequence 0, the readback 1 and the poll 2.
    let source = device
        .create_buffer(&buffer_desc())
        .expect("a source buffer");
    let desc = ReadbackDesc {
        label: None,
        buffer: source,
        offset: 0,
        size: 4,
        after: None,
    };
    let readback = device.request_readback(&desc).expect("a readback handle");

    let mut out = [0u8; 4];
    // First poll issues the poll command and reports Pending.
    assert_eq!(
        device.poll_readback(readback, &mut out).expect("poll"),
        ReadbackState::Pending,
    );

    // CreateBuffer is sequence 0, request_readback 1 (not awaited), the poll 2.
    feed(&channel, |w| w.readback_ready(2, readback, &[1, 2, 3, 4]));
    assert_eq!(
        device.poll_readback(readback, &mut out).expect("poll"),
        ReadbackState::Ready,
    );
    assert_eq!(out, [1, 2, 3, 4], "the bytes reach the caller's slice");
}

/// **The check that would have caught the hang.** Before
/// [`Reply::ReadbackFailed`](crate::Reply::ReadbackFailed) existed, a rejected
/// `mapAsync` left the replayer's request filed as still mapping, so every poll
/// from here to the end of the process answered
/// [`Pending`](ReadbackState::Pending) — a caller with no deadline spins for
/// ever on bytes that are never coming. What is asserted is the stop: the poll
/// returns an `Err`, the reason the browser gave is inside it, and it stays an
/// `Err` rather than dropping back into the poll loop.
#[test]
fn a_failed_readback_reports_the_reason_rather_than_polling_for_ever() {
    let (channel, device) = device_on_fresh_channel();

    // A real source, for `a_readback_polls_ready_on_a_readback_reply`'s reason:
    // the range check needs a size this device recorded.
    let source = device
        .create_buffer(&buffer_desc())
        .expect("a source buffer");
    let desc = ReadbackDesc {
        label: None,
        buffer: source,
        offset: 0,
        size: 4,
        after: None,
    };
    let readback = device.request_readback(&desc).expect("a readback handle");

    let mut out = [0u8; 4];
    assert_eq!(
        device.poll_readback(readback, &mut out).expect("poll"),
        ReadbackState::Pending,
    );

    // CreateBuffer is sequence 0, request_readback 1 (not awaited), the poll 2.
    feed(&channel, |w| {
        w.readback_failed(2, readback, "mapAsync rejected: device was lost");
    });
    let error = device
        .poll_readback(readback, &mut out)
        .expect_err("a readback whose map rejected is an error, not another Pending");
    let HalError::DeviceLost(reason) = &error else {
        panic!("a failed readback is DeviceLost, the arm poll_readback documents: {error:?}");
    };
    assert!(
        reason.contains("device was lost"),
        "the browser's own words reach the caller: {reason}"
    );
    assert_eq!(
        out, [0u8; 4],
        "a failed poll leaves the caller's slice alone"
    );

    // **And it stays failed.** A second poll re-issuing the command would put
    // the caller straight back in the loop this reply exists to end — and would
    // ask the replayer about a request it has already answered.
    let again = device
        .poll_readback(readback, &mut out)
        .expect_err("polling a failed readback reports the same failure");
    assert!(matches!(again, HalError::DeviceLost(_)), "{again:?}");
    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    assert_eq!(
        commands
            .iter()
            .filter(|command| command.name() == "PollReadback")
            .count(),
        1,
        "the failure is terminal, so no further poll goes on the stream",
    );
}

// ── take_error: the browser's out-of-band failures ─────────────────────────

/// **The check the stub could not fail.** `take_error` answered `None`
/// unconditionally, so a browser reporting an out-of-memory cascade — the real
/// failure this exists for — reached the engine as a healthy device drawing
/// nothing. Every assertion below is about a message *arriving*: the ask on the
/// stream, the text out of the reply, one message per call, and the next ask
/// once the queue is dry.
#[test]
fn take_error_hands_back_what_the_browser_reported() {
    let (channel, device) = device_on_fresh_channel();

    // Nothing has been reported and nothing has been asked, so the first call is
    // `None` — and it is what puts the ask on the stream. On a fresh channel it
    // is the first command, so its sequence is 0.
    assert_eq!(device.take_error(), None, "nothing has been reported yet");
    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    assert_eq!(
        commands
            .iter()
            .map(crate::Command::name)
            .collect::<Vec<_>>(),
        vec!["TakeError"],
        "the first call asks, exactly once",
    );

    // A second call while that ask is unanswered must not ask again: the reply
    // carries the replayer's whole queue, so a second ask would bring the same
    // messages twice.
    assert_eq!(device.take_error(), None, "still nothing to report");
    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    assert_eq!(commands.len(), 1, "one ask is outstanding, not two");

    // What the browser said, answering that ask.
    let reported = [
        "Buffer usage (MapRead|Storage) is invalid.".to_string(),
        "vkAllocateMemory failed with VK_ERROR_OUT_OF_DEVICE_MEMORY".to_string(),
    ];
    feed(&channel, |w| {
        assert_eq!(
            w.device_errors(0, &reported),
            reported.len(),
            "both messages fit one reply",
        );
    });

    assert_eq!(
        device.take_error().as_deref(),
        Some(reported[0].as_str()),
        "the first message reaches the caller, text and all",
    );
    assert_eq!(
        device.take_error().as_deref(),
        Some(reported[1].as_str()),
        "and the second, in the order the browser reported them",
    );
    assert_eq!(
        device.take_error(),
        None,
        "each error is reported once, which is what take_error promises",
    );

    // The queue ran dry, so the drained call asked again — with the sequence the
    // command after the answered one has.
    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    assert_eq!(
        commands
            .iter()
            .map(crate::Command::name)
            .collect::<Vec<_>>(),
        vec!["TakeError", "TakeError"],
        "an emptied queue asks for the next batch",
    );
    feed(&channel, |w| {
        w.device_errors(1, &["the device was lost".to_string()]);
    });
    assert_eq!(
        device.take_error().as_deref(),
        Some("the device was lost"),
        "and the second ask is answered on its own sequence",
    );
}

/// A reply of the wrong shape naming the ask is a replayer bug, and the failure
/// mode to avoid is silence for ever: the ask is answered exactly once, so a
/// device that kept waiting on it would never ask again and would report nothing
/// no matter what the browser said afterwards.
#[test]
fn take_error_asks_again_after_a_reply_of_the_wrong_shape() {
    let (channel, device) = device_on_fresh_channel();
    assert_eq!(device.take_error(), None);

    // `NoAdapter` is not an answer to a `TakeError`, but it names its sequence,
    // so the channel accepts it and the queue must treat the ask as spent.
    feed(&channel, |w| w.no_adapter(0, "not an answer to this"));
    assert_eq!(device.take_error(), None, "nothing arrived to report");

    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    assert_eq!(
        commands.len(),
        2,
        "the device asks again rather than hanging"
    );
    feed(&channel, |w| {
        w.device_errors(1, &["a real message".to_string()]);
    });
    assert_eq!(device.take_error().as_deref(), Some("a real message"));
}

// ── (f) an unwired op fails loudly at finish ───────────────────────────────

#[test]
fn finish_fails_loudly_after_recording_an_unwired_op() {
    let (_channel, device) = device_on_fresh_channel();
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("the graphics queue");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });

    // `draw_mesh_tasks` has no stream command and never will — WebGPU has no
    // mesh stage — so recording it must make finish refuse rather than replay a
    // command buffer missing the draw.
    encoder.draw_mesh_tasks(1, 1, 1);
    let Err(HalError::Unsupported { what, .. }) = encoder.finish() else {
        panic!("finish must refuse a recorded unwired op");
    };
    assert!(
        what.contains("mesh"),
        "the error names what is missing: {what}"
    );
}

// ── (g) the debug-marker ops the device advertises ─────────────────────────

/// **`Features::DEBUG_MARKERS` is a promise, and this is what keeps it.**
///
/// `web/engine/gpu-replay.js` grants the bit unconditionally in `CORE_FEATURES`,
/// so every device this backend opens reports it. A caller that branches on it
/// records a label region and a marker; if either were unwired, `finish` would
/// refuse and take the whole command buffer down a frame away from the call that
/// caused it. So all three ops must encode, and the frame must finish.
#[test]
fn the_debug_marker_ops_encode_rather_than_refusing_at_finish() {
    let (channel, device) = device_on_fresh_channel();
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("the graphics queue");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });

    encoder.begin_debug_label("gbuffer");
    encoder.insert_debug_marker("cull done");
    encoder.end_debug_label();
    encoder
        .finish()
        .expect("the debug ops are wired, so finish succeeds");

    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    let names: Vec<_> = commands.iter().map(crate::Command::name).collect();
    assert_eq!(
        names,
        vec![
            "CreateCommandEncoder",
            "BeginDebugLabel",
            "InsertDebugMarker",
            "EndDebugLabel",
            "Finish",
        ],
        "the three debug ops each encode a command of their own, in order"
    );
    let Some(crate::Command::InsertDebugMarker { label }) = commands.get(2) else {
        panic!("the third command is the marker: {commands:?}");
    };
    assert_eq!(label, "cull done", "the marker carries the caller's label");
}

/// **`set_stencil_reference` reaches the stream with its value intact.**
///
/// The declaration side of `Capability::StencilReference`, which this backend
/// answers `Support::Yes` for. It used to record an unwired op, so a frame that
/// set a stencil reference lost its whole command buffer at `finish` — this
/// pins both halves of the fix: the frame finishes, and the value the caller
/// gave is the value on the wire.
///
/// The reference is deliberately neither `0` nor small: `0` is WebGPU's own
/// initial value for a fresh pass, so a writer that encoded the tag and dropped
/// the field would still produce the reading a correct one does.
#[test]
fn set_stencil_reference_encodes_rather_than_refusing_at_finish() {
    let (channel, device) = device_on_fresh_channel();
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("the graphics queue");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });

    encoder.set_stencil_reference(0x00BE_EF2A);
    encoder
        .finish()
        .expect("set_stencil_reference is wired, so finish succeeds");

    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    let names: Vec<_> = commands.iter().map(crate::Command::name).collect();
    assert_eq!(
        names,
        vec!["CreateCommandEncoder", "SetStencilReference", "Finish"],
        "the stencil reference encodes a command of its own"
    );
    let Some(crate::Command::SetStencilReference { reference }) = commands.get(1) else {
        panic!("the second command is the stencil reference: {commands:?}");
    };
    assert_eq!(*reference, 0x00BE_EF2A, "the whole u32 crosses");
}

// ── (h) the indirect dispatch ──────────────────────────────────────────────

/// **`dispatch_indirect` reaches the stream with its buffer and offset intact.**
///
/// WebGPU's `dispatchWorkgroupsIndirect(buffer, offset)` maps one to one, so the
/// op has no reason to fail `finish`. The offset is deliberately not zero: a
/// writer that dropped the field would still encode a command, and only a
/// non-zero value tells the two apart.
#[test]
fn dispatch_indirect_encodes_rather_than_refusing_at_finish() {
    let (channel, device) = device_on_fresh_channel();
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("the graphics queue");
    let args: BufferHandle = Handle::from_bits((7 << 32) | 5).expect("a real handle");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });

    encoder.begin_compute_pass(&crcbl_hal::ComputePassDesc {
        label: None,
        timestamp_writes: None,
    });
    encoder.dispatch_indirect(args, 256);
    encoder.end_compute_pass();
    encoder
        .finish()
        .expect("dispatch_indirect is wired, so finish succeeds");

    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    let names: Vec<_> = commands.iter().map(crate::Command::name).collect();
    assert_eq!(
        names,
        vec![
            "CreateCommandEncoder",
            "BeginComputePass",
            "DispatchIndirect",
            "EndComputePass",
            "Finish",
        ],
        "the indirect dispatch encodes its own command inside the pass"
    );
    let Some(crate::Command::DispatchIndirect { buffer, offset }) = commands.get(2) else {
        panic!("the third command is the indirect dispatch: {commands:?}");
    };
    assert_eq!(*buffer, args, "the argument buffer crosses");
    assert_eq!(*offset, 256, "the byte offset crosses");
}

/// **An indirect draw's offset and stride are checked before anything is
/// encoded; its bound is left to the browser, deliberately.**
///
/// `crcbl_hal::indirect` states three rules for stepping an array of argument
/// structures. This encoder holds a channel and a handle pool and cannot reach
/// a buffer's length, so it calls `check_layout` — the offset and the stride —
/// and the browser answers the third against the buffer itself. Those first two
/// are the ones no API reports: a misaligned indirect offset is
/// `VUID-vkCmdDrawIndirect-offset-02710`, which has no error code at all.
///
/// **Both halves are the check.** The refusals alone would be satisfied by an
/// encoder that refused every indirect draw, so a legal one — including one
/// whose offset is far past any buffer this test could name — must still reach
/// the stream with its four fields intact. That last row is the honest half of
/// the bound: it is not checked here, and a test claiming otherwise would be
/// claiming a check that does not exist.
///
/// **What turns it red.** Encoding before the check, so a refused draw still
/// reaches the stream; checking the indexed form against the four-word
/// structure, which accepts a 16-byte stride that reads a word short every
/// structure after the first; or reaching for the bound, which would refuse the
/// far-offset row this backend has no length to judge.
#[test]
fn an_indirect_draws_offset_and_stride_are_checked_before_it_is_encoded() {
    use crcbl_hal::DrawIndirect;

    let (channel, device) = device_on_fresh_channel();
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("the graphics queue");
    let args: BufferHandle = Handle::from_bits((9 << 32) | 3).expect("a real handle");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
    // One draw never strides, so `stride: 0` is legal here.
    encoder.draw_indirect(&DrawIndirect {
        args,
        offset: 16,
        draw_count: 1,
        stride: 0,
    });
    // Two five-word structures 32 bytes apart: a padded stride, honoured.
    encoder.draw_indexed_indirect(&DrawIndirect {
        args,
        offset: 64,
        draw_count: 2,
        stride: 32,
    });
    // Past the end of any buffer a browser would hand back — and encoded, because
    // the bound is the browser's rule to enforce and this encoder has no length
    // to enforce it with.
    encoder.draw_indirect(&DrawIndirect {
        args,
        offset: 1 << 40,
        draw_count: 1,
        stride: 0,
    });
    encoder
        .finish()
        .expect("three legal argument layouts, so finish succeeds");

    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    let names: Vec<_> = commands.iter().map(crate::Command::name).collect();
    assert_eq!(
        names,
        vec![
            "CreateCommandEncoder",
            "DrawIndirect",
            "DrawIndexedIndirect",
            "DrawIndirect",
            "Finish",
        ],
        "a legal indirect draw encodes a command of its own"
    );
    let Some(crate::Command::DrawIndexedIndirect {
        buffer,
        offset,
        draw_count,
        stride,
    }) = commands.get(2)
    else {
        panic!("the third command is the indexed indirect draw: {commands:?}");
    };
    assert_eq!(
        (*buffer, *offset, *draw_count, *stride),
        (args, 64, 2, 32),
        "all four fields cross unchanged"
    );

    for (what, indexed, draw) in [
        (
            "an offset that is not a multiple of four",
            false,
            DrawIndirect {
                args,
                offset: 2,
                draw_count: 1,
                stride: 0,
            },
        ),
        (
            "a stride below one 16-byte structure",
            false,
            DrawIndirect {
                args,
                offset: 0,
                draw_count: 2,
                stride: 12,
            },
        ),
        (
            "a stride below one 20-byte indexed structure",
            true,
            DrawIndirect {
                args,
                offset: 0,
                draw_count: 2,
                stride: 16,
            },
        ),
        (
            "a stride that is not a multiple of four",
            false,
            DrawIndirect {
                args,
                offset: 0,
                draw_count: 2,
                stride: 18,
            },
        ),
    ] {
        // A fresh channel each time: the refusal is asserted by what the stream
        // does *not* hold, which a shared one would carry over.
        let (channel, device) = device_on_fresh_channel();
        let queue = device
            .queue(QueueKind::Graphics)
            .expect("the graphics queue");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        if indexed {
            encoder.draw_indexed_indirect(&draw);
        } else {
            encoder.draw_indirect(&draw);
        }
        let finished = encoder.finish();
        assert!(
            matches!(finished, Err(HalError::InvalidDescriptor(_))),
            "{what} must be refused at finish, and finish answered {finished:?}"
        );

        let commands = channel
            .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode");
        let names: Vec<_> = commands.iter().map(crate::Command::name).collect();
        assert_eq!(
            names,
            vec!["CreateCommandEncoder"],
            "{what}: nothing is encoded for a draw that was refused"
        );
    }
}

// ── refusals of needed-but-unwired Device methods ──────────────────────────

#[test]
fn needed_but_unwired_device_methods_refuse_loudly() {
    let (_channel, device) = device_on_fresh_channel();
    assert!(
        matches!(
            device.query_results(
                Handle::from_bits((1 << 32) | 2).expect("a real handle"),
                0,
                &mut [0u64; 1],
            ),
            Err(HalError::InvalidHandle { .. })
        ),
        "a set this device never created names no pool, whatever the browser would say"
    );
    assert!(
        matches!(
            device.create_mesh_pipeline(&mesh_pipeline_desc()),
            Err(HalError::Unsupported { .. })
        ),
        "a mesh pipeline is legitimately refused: WebGPU has no mesh stage"
    );
}

// ── the query spine ────────────────────────────────────────────────────────

/// **The timestamp kind is gated on the browser's feature, and this is the
/// guard on the whole demo site.**
///
/// `crcbl-render`'s `PassTimers::new` decides whether to time a frame from
/// [`Features::TIMESTAMP_QUERY`] on the device's caps and from
/// `create_query_set` succeeding — nothing else. Both must answer the same
/// question: a set handed out on a device that opened without
/// `'timestamp-query'` would put a `timestampWrites` into every frame that the
/// browser then refuses pass by pass, and one refused on a device that *has* the
/// feature would leave the profiler blank for no reason.
///
/// **What turns it red.** `create_query_set` refusing `Timestamp` on the
/// device that carries the flag, accepting it on the one that does not, or
/// either arrangement disagreeing with what `supports` declares. The statistics
/// kind is refused on both, because `GPUQueryType` has no such member.
#[test]
fn a_timestamp_set_follows_the_feature_the_device_opened_with() {
    use crcbl_hal::{Capability, QueryKind, QuerySetDesc, Support};

    for features in [
        Features::COMPUTE | Features::TIMESTAMP_QUERY,
        Features::COMPUTE,
    ] {
        let timestamps = features.contains(Features::TIMESTAMP_QUERY);
        let channel = SharedChannel::new();
        let device = WebGpuDevice::new(
            channel.clone(),
            DeviceCaps {
                features,
                limits: crcbl_hal::Limits::minimum(),
            },
            HandlePool::new(),
        );

        let created = device.create_query_set(&QuerySetDesc {
            label: Some("graph pass timers"),
            kind: QueryKind::Timestamp,
            count: 2,
        });
        assert_eq!(
            created.is_ok(),
            timestamps,
            "a timestamp set follows the feature and nothing else: {created:?}"
        );
        assert_eq!(
            matches!(device.supports(Capability::TimestampQuery), Support::Yes),
            timestamps,
            "the declaration and the refusal are one answer"
        );
        assert!(
            matches!(
                device.create_query_set(&QuerySetDesc {
                    label: None,
                    kind: QueryKind::PipelineStatistics,
                    count: 2,
                }),
                Err(HalError::Unsupported { .. })
            ),
            "GPUQueryType has no statistics member"
        );

        // Nothing reached the stream for the refusals: a command encoded for a
        // set the caller never got back would create a pool nothing releases.
        // The accepted one encodes exactly its own creation.
        let commands = channel
            .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
            .expect("the channel is not borrowed")
            .expect("the writer's own bytes decode");
        assert_eq!(
            commands.len(),
            usize::from(timestamps),
            "only the accepted set encodes: {commands:?}"
        );
    }
}

/// **An occlusion set is created, recorded against, read and released — the
/// whole spine, in the order a caller drives it.**
///
/// The declaration this holds is `Capability::OcclusionQuery`, and what it
/// claims is exactly what the capability defines: a set of the size asked for
/// exists, and the seam's verbs reach the stream naming it. The browser gate's
/// group AE is what holds the *values* to it.
///
/// **What turns it red.** A verb going back to `record_unsupported`, which
/// `finish` would then refuse over; a command encoding under the wrong tag; or
/// the range arriving with its halves swapped.
/// **Every capability this backend declares unsupported is refused by the call
/// that names it** — the half of the parity contract the native seam suite
/// cannot reach here.
///
/// `crcbl-webgpu` is the one backend `crates/crcbl/tests/hal_seam_e2e.rs` cannot
/// open: `crcbl::backend::open` answers "the crcbl-webgpu backend is not active
/// in this build — it reaches a device only on wasm32". So the driver that holds
/// every other backend to `(Support::No, Exercise::Refused)` has never run
/// against this one, and its unconditional `Support::No` rows have never been
/// held to a refusal by it.
///
/// It needs no browser, which is the point. `WebGpuDevice` records commands to a
/// stream rather than executing them, so a refusal is a decision this crate
/// makes in Rust and is observable in an ordinary unit test.
///
/// # What this covers, and what it does not
///
/// The rows whose refusal is a single device call: the four timeline rows,
/// through `create_semaphore`, and `PipelineStatisticsQuery` through
/// `create_query_set`. The rest — `PushConstants`, `MeshShading`,
/// `DrawIndirectCount`, `UpdateBindGroup`, `BindlessDescriptorArray`,
/// `PolygonModeLine` — need a pipeline or a layout built first, which is the
/// seam suite's `exercise_*` machinery and is not worth a second copy here.
/// Those stay uncovered on this backend and the backlog says so.
///
/// `IndirectArgumentPaddedStride` was on that list and is not a refusal any
/// more: the stride crosses the stream whole and `web/engine/gpu-replay.js`
/// unrolls the draw at `offset + i * stride`, so the declaration is
/// `Support::Yes` and the browser gate's indirect group is what holds it to a
/// value.
#[test]
fn a_capability_declared_unsupported_is_refused_by_its_own_call() {
    use crcbl_hal::{Capability, QueryKind, QuerySetDesc, SemaphoreDesc, SemaphoreKind, Support};

    let (_channel, device) = device_on_fresh_channel();

    // The declarations under test, so a row that changed side would fail here
    // rather than silently stop being checked.
    for capability in [
        Capability::TimelineSemaphore,
        Capability::CpuTimelineWait,
        Capability::CpuTimelineSignal,
        Capability::TimelineWaitBeforeSignal,
        Capability::PipelineStatisticsQuery,
    ] {
        assert!(
            matches!(device.supports(capability), Support::No(_)),
            "{capability:?} is no longer declared unsupported on this backend, so this test is \
             asserting a refusal the seam no longer promises"
        );
    }

    let timeline = device.create_semaphore(&SemaphoreDesc {
        label: Some("refused timeline"),
        kind: SemaphoreKind::Timeline { initial_value: 0 },
    });
    assert!(
        matches!(timeline, Err(HalError::Unsupported { .. })),
        "a timeline semaphore is declared unsupported and create_semaphore answered {timeline:?}. \
         The seam documents Unsupported for this, and a caller branching on that variant to pick \
         a fallback would miss any other error"
    );

    let statistics = device.create_query_set(&QuerySetDesc {
        label: Some("refused statistics"),
        kind: QueryKind::PipelineStatistics,
        count: 1,
    });
    assert!(
        matches!(statistics, Err(HalError::Unsupported { .. })),
        "a pipeline-statistics query set is declared unsupported and create_query_set answered \
         {statistics:?}"
    );

    // `UpdateBindGroup`, which needs no bind group to reach: this backend
    // refuses unconditionally and reads neither argument, because a
    // `GPUBindGroup` exposes a label and nothing else. Recorded here as a
    // *third* row that "needs a layout or a bind group built first" turned out
    // not to need one — the claim was made about a category rather than checked
    // per member.
    let updated =
        device.update_bind_group(Handle::from_bits((7 << 32) | 1).expect("a handle"), &[]);
    assert!(
        matches!(updated, Err(HalError::Unsupported { .. })),
        "update_bind_group is declared unsupported and answered {updated:?}"
    );
    assert!(
        matches!(device.supports(Capability::UpdateBindGroup), Support::No(_)),
        "UpdateBindGroup is no longer declared unsupported"
    );

    // **The accepting side, so this is not a test that passes by refusing
    // everything.** A binary semaphore and an occlusion set are both declared
    // supported, and both must still be served.
    device
        .create_semaphore(&SemaphoreDesc {
            label: Some("served binary"),
            kind: SemaphoreKind::Binary,
        })
        .expect("a binary semaphore is declared supported");
    device
        .create_query_set(&QuerySetDesc {
            label: Some("served occlusion"),
            kind: QueryKind::Occlusion,
            count: 1,
        })
        .expect("an occlusion set is declared supported");
}

/// **The encoder's refusals too**, for the three capabilities whose call is
/// recorded rather than returned.
///
/// A sibling of
/// [`a_capability_declared_unsupported_is_refused_by_its_own_call`], and split
/// from it because the channel differs: `CommandEncoder`'s verbs return nothing,
/// so a refusal is recorded and surfaces at `finish`. A caller who never called
/// `finish` would never see it, which is worth asserting in its own right.
///
/// **These three were recorded as needing "a pipeline or a layout built first"
/// and do not.** `record_unsupported` sets a field; it reads no pass state and
/// needs no pipeline, so an encoder and one call reach all three. That claim was
/// made about all seven uncovered rows at once without checking them
/// individually — the four that genuinely do need a pipeline or a bind group are
/// `PushConstants`, `UpdateBindGroup`, `BindlessDescriptorArray` and
/// `PolygonModeLine`.
#[test]
fn an_encoder_verb_declared_unsupported_is_refused_at_finish() {
    use crcbl_hal::{Capability, DrawIndirectCount, Support};

    let (_channel, device) = device_on_fresh_channel();
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("the graphics queue");

    for capability in [
        Capability::DrawIndirectCount,
        Capability::MeshShading,
        Capability::TaskShaderStage,
    ] {
        assert!(
            matches!(device.supports(capability), Support::No(_)),
            "{capability:?} is no longer declared unsupported, so this test asserts a refusal the \
             seam no longer promises"
        );
    }

    let count = DrawIndirectCount {
        args: Handle::from_bits((3 << 32) | 1).expect("a real handle"),
        args_offset: 0,
        count_buffer: Handle::from_bits((3 << 32) | 2).expect("a real handle"),
        count_offset: 0,
        max_draw_count: 1,
        stride: 16,
    };

    // Each verb on its own encoder, so one refusal cannot mask another: the
    // recorder keeps the *first* it is given and drops the rest.
    for (what, record) in [
        ("draw_indirect_count", 0),
        ("draw_indexed_indirect_count", 1),
        ("draw_mesh_tasks", 2),
    ] {
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
        match record {
            0 => encoder.draw_indirect_count(&count),
            1 => encoder.draw_indexed_indirect_count(&count),
            _ => encoder.draw_mesh_tasks(1, 1, 1),
        }
        let finished = encoder.finish();
        assert!(
            matches!(finished, Err(HalError::Unsupported { .. })),
            "{what} is declared unsupported and finish answered {finished:?}. The seam documents \
             Unsupported, and a caller branching on that variant to pick a fallback would miss \
             any other error"
        );
    }

    // **An encoder that recorded nothing must still finish**, or the assertions
    // above would pass on a backend that refused every command buffer.
    let clean = device
        .create_command_encoder(&CommandEncoderDesc { label: None, queue })
        .finish();
    assert!(
        clean.is_ok(),
        "an encoder with no unsupported verb in it failed to finish: {clean:?}"
    );
}

#[test]
fn the_occlusion_query_spine_reaches_the_stream() {
    use crcbl_hal::{BufferHandle, QueryKind, QuerySetDesc};

    let (channel, device) = device_on_fresh_channel();
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("the graphics queue");
    let set = device
        .create_query_set(&QuerySetDesc {
            label: Some("visibility"),
            kind: QueryKind::Occlusion,
            count: 8,
        })
        .expect("an occlusion set is core WebGPU");
    let dst: BufferHandle = Handle::from_bits((3 << 32) | 4).expect("a real handle");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });
    encoder.reset_query_set(set, 0..8);
    encoder.resolve_query_set(set, 2..5, dst, 256);
    encoder
        .finish()
        .expect("every query verb is wired, so finish succeeds");
    device.destroy_query_set(set);

    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    let names: Vec<_> = commands.iter().map(crate::Command::name).collect();
    assert_eq!(
        names,
        vec![
            "CreateQuerySet",
            "CreateCommandEncoder",
            "ResetQuerySet",
            "ResolveQuerySet",
            "Finish",
            "DestroyQuerySet",
        ],
    );
    let Some(crate::Command::CreateQuerySet { kind, count, .. }) = commands.first() else {
        panic!("the first command creates the set: {commands:?}");
    };
    assert_eq!(*kind, QueryKind::Occlusion);
    assert_eq!(*count, 8);
    let Some(crate::Command::ResolveQuerySet {
        first_query,
        query_count,
        dst_offset,
        ..
    }) = commands.get(3)
    else {
        panic!("the fourth command is the resolve: {commands:?}");
    };
    assert_eq!(
        (*first_query, *query_count),
        (2, 3),
        "the range crosses as a first index and a count, not as its two ends"
    );
    assert_eq!(*dst_offset, 256);
}

/// **`query_results` answers the bounds question itself and defers the rest.**
///
/// Two halves, and the pair is the check: a read inside the set encodes an ask
/// and reports that the answer is not here yet, and a read one query past the
/// end is refused with [`HalError::InvalidDescriptor`] without touching the
/// stream. `Ok` alone is what an implementation doing nothing answers to
/// everything, and it is the refusal beside it that makes the deferral mean
/// something — the seam's own `exercise_query_set_creation` makes the same pair
/// the check for the four native backends.
///
/// **What turns it red.** An over-long read encoding a command or answering
/// anything but `InvalidDescriptor`; an in-range read failing to put an ask on
/// the stream, which would leave a caller polling for an answer nobody asked
/// for.
#[test]
fn a_read_past_the_end_of_a_query_set_is_refused_without_asking_the_browser() {
    use crcbl_hal::{QueryKind, QuerySetDesc};

    let (channel, device) = device_on_fresh_channel();
    let set = device
        .create_query_set(&QuerySetDesc {
            label: None,
            kind: QueryKind::Occlusion,
            count: 4,
        })
        .expect("an occlusion set is core WebGPU");

    let mut past_the_end = [0u64; 5];
    assert!(
        matches!(
            device.query_results(set, 0, &mut past_the_end),
            Err(HalError::InvalidDescriptor(_))
        ),
        "the seam documents InvalidDescriptor for a range that exceeds the set"
    );
    let mut inside = [0u64; 4];
    assert!(
        matches!(
            device.query_results(set, 0, &mut inside),
            Err(HalError::Backend(_))
        ),
        "an in-range read this browser has not answered yet is a deferral, not a refusal"
    );

    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    let names: Vec<_> = commands.iter().map(crate::Command::name).collect();
    assert_eq!(
        names,
        vec!["CreateQuerySet", "QueryResults"],
        "the over-long read encodes nothing and the in-range one encodes exactly one ask"
    );
}

/// **The values the browser answers with are the values the caller gets.**
///
/// [`take_error`](crcbl_hal::Device::take_error)'s protocol on a query set: the
/// first call encodes the ask and defers, the reply is fed in, and the next call
/// hands the values over. The values are neither zero nor sequential, so a path
/// that answered a fresh buffer — or the query indices — is a different reading.
///
/// **What turns it red.** The second call still deferring (the reply was never
/// absorbed), or `out` coming back holding anything but what the reply carried.
#[test]
fn an_answered_query_read_hands_back_the_browsers_own_values() {
    use crcbl_hal::{QueryKind, QuerySetDesc};

    let (channel, device) = device_on_fresh_channel();
    let set = device
        .create_query_set(&QuerySetDesc {
            label: None,
            kind: QueryKind::Occlusion,
            count: 4,
        })
        .expect("an occlusion set is core WebGPU");

    let mut out = [0u64; 2];
    assert!(
        device.query_results(set, 1, &mut out).is_err(),
        "the first call is the ask"
    );
    // The channel is fresh, so `create_query_set` holds sequence 0 and the ask
    // above holds 1 — the same arithmetic `opened_instance` relies on.
    let values = vec![0x0102_0304_0506_0708, 0x1112_1314_1516_1718];
    feed(&channel, |w| w.query_results(1, set, 1, &values));

    device
        .query_results(set, 1, &mut out)
        .expect("the answer has arrived, so the read succeeds");
    assert_eq!(out.to_vec(), values, "the browser's own values reach out");
}

// ── semaphores: the timeline half refuses, the binary half does not ────────

/// **A semaphore call that cannot do anything must not report that it did.**
///
/// The three timeline entry points used to answer `Ok`: `create_semaphore`
/// handed out a pool slot, `semaphore_value` answered `0` for ever, and
/// `wait_semaphores` answered `Ok(true)` for a wait it never evaluated. A caller
/// polling for progress therefore saw success and no movement, with nothing in
/// any return value to say why — which is the one failure a caller cannot
/// detect, and the reason
/// [`Capability::TimelineSemaphore`](crcbl_hal::Capability::TimelineSemaphore)
/// declares the *behaviour* rather than the return code.
///
/// **What turns it red.** Any of the three going back to `Ok`. Refusing the
/// binary kind as well — WSI acquire is where binary semaphores come from and
/// `crcbl_hal::sync` requires every device to hand one out, so a backend that
/// refused both would satisfy every assertion about the timeline half and still
/// be wrong. And the declarations drifting from the calls: `supports` is checked
/// against the same three here, because a declaration nobody compares to
/// behaviour is what this whole enum exists to stop.
#[test]
fn the_timeline_semaphore_calls_refuse_instead_of_succeeding_at_nothing() {
    use crcbl_hal::{Capability, SemaphoreDesc, SemaphoreKind, SemaphoreWait, Support};

    let (_channel, device) = device_on_fresh_channel();

    let timeline = device.create_semaphore(&SemaphoreDesc {
        label: Some("frames in flight"),
        kind: SemaphoreKind::Timeline { initial_value: 0 },
    });
    assert!(
        matches!(
            timeline,
            Err(HalError::Unsupported {
                backend: BackendKind::WebGpu,
                ..
            })
        ),
        "a timeline this backend cannot advance must be refused, not handed out: {timeline:?}"
    );

    // The binary kind still works, and is what the seam requires every device to
    // give. It is also what makes the assertion above about the *kind* rather
    // than about `create_semaphore` refusing everything.
    let binary = device
        .create_semaphore(&SemaphoreDesc {
            label: Some("acquire"),
            kind: SemaphoreKind::Binary,
        })
        .expect("WSI acquire needs a binary semaphore on every device");

    let value = device.semaphore_value(binary);
    assert!(
        matches!(value, Err(HalError::Unsupported { .. })),
        "every semaphore here is binary and has no counter; 0 forever is the answer this \
         refusal replaced: {value:?}"
    );
    let waited = device.wait_semaphores(
        &[SemaphoreWait {
            semaphore: binary,
            value: 1,
        }],
        0,
    );
    assert!(
        matches!(waited, Err(HalError::Unsupported { .. })),
        "there is no timeline to block on, so Ok(true) would be claiming a wait was satisfied \
         that was never evaluated: {waited:?}"
    );
    let signalled = device.signal_semaphore(binary, 1);
    assert!(
        matches!(signalled, Err(HalError::Unsupported { .. })),
        "there is no timeline to advance, so Ok(()) would be claiming a signal landed somewhere \
         nothing can observe: {signalled:?}"
    );
    device.destroy_semaphore(binary);

    // And the declarations say the same thing the calls do. The fixture's device
    // reports no TIMELINE_SEMAPHORE, exactly as a browser's does.
    assert!(
        !device
            .caps()
            .features
            .contains(Features::TIMELINE_SEMAPHORE)
    );
    for capability in [
        Capability::TimelineSemaphore,
        Capability::CpuTimelineWait,
        Capability::CpuTimelineSignal,
        Capability::TimelineWaitBeforeSignal,
    ] {
        assert!(
            matches!(device.supports(capability), Support::No(_)),
            "{capability} is refused at the call site and must be declared refused too"
        );
    }
    assert_eq!(device.supports(Capability::BinarySemaphore), Support::Yes);
}

/// A mesh-pipeline descriptor shaped only enough to be refused before anything
/// reads its stages.
fn mesh_pipeline_desc() -> crcbl_hal::MeshPipelineDesc<'static> {
    crcbl_hal::MeshPipelineDesc {
        label: None,
        layout: Handle::from_bits((1 << 32) | 3).expect("a real handle"),
        task: None,
        task_workgroup_size: [1, 1, 1],
        mesh: crcbl_hal::ShaderEntry {
            module: Handle::from_bits((1 << 32) | 4).expect("a real handle"),
            entry_point: "main",
        },
        mesh_workgroup_size: [1, 1, 1],
        fragment: None,
        primitive: crcbl_hal::PrimitiveState::default(),
        depth_stencil: None,
        multisample: crcbl_hal::MultisampleState::default(),
        color_targets: &[],
    }
}

// ── the extent an acquired frame carries ───────────────────────────────────

/// `web/engine/shell.js` — the shell's half of the browser, pulled in whole so
/// the one thing `WebGpuDevice::acquire_next_frame` claims about the canvas can
/// be checked without one.
///
/// `include_str!` resolves against this file, so moving the shim stops this
/// crate compiling — a failure nobody can read as a pass, which is why
/// [`crate::js_mirror`] pulls the page's other halves in the same way.
const SHELL_JS: &str = include_str!("../../../../web/engine/shell.js");

/// The three statements that make the extent this backend reports and the size
/// of the texture the browser hands back one number, in the order that keeps
/// them one: **write the backing store, then tell the engine what was written.**
const SIZED_THEN_REPORTED: &str = "canvas.width = width; canvas.height = height; \
     exports.__crcbl_web_resize(canvasId, width, height, scale);";

/// A canvas swapchain descriptor at `extent`, shaped the way the engine's is —
/// the browser's preferred format's sRGB counterpart, the ring a canvas reports,
/// and the only present mode a browser has.
fn canvas_swapchain_desc(surface: SurfaceHandle, extent: (u32, u32)) -> SwapchainDesc<'static> {
    SwapchainDesc {
        label: Some("test canvas swapchain"),
        surface,
        format: Format::Rgba8UnormSrgb,
        extent,
        image_count: 2,
        present_mode: PresentMode::Fifo,
        composite_alpha: CompositeAlpha::Opaque,
    }
}

/// **The shim sizes the canvas backing store to the number it then reports, in
/// that order, and it is the only thing that sizes it.**
///
/// `acquire_next_frame` reports the extent the last configure carried and
/// declares the frame never suboptimal, while the texture the browser hands back
/// is sized by `canvas.width`/`canvas.height` — WebGPU's *Canvas Context sizing*,
/// which no `configure()` has a say in. The two are the same number only because
/// `syncSize` writes the backing store and then hands the engine those very
/// locals. A shim that reported CSS pixels while sizing the store in device
/// pixels, that told the engine first and resized afterwards, or that grew a
/// second place to resize the canvas from, would each make that `false` a lie
/// with nothing anywhere to catch it.
///
/// Read off the whitespace-folded source, so prettier decides where these
/// statements wrap and not what this guard covers, and pinned by **count** as
/// well as by text — one writer of each half of the backing store, one report,
/// and the three of them adjacent.
#[test]
fn the_shim_reports_the_canvas_size_it_just_wrote() {
    let shim = crate::js_mirror::collapsed(SHELL_JS);
    assert_eq!(
        shim.matches("canvas.width =").count(),
        1,
        "web/engine/shell.js must have exactly one writer of the canvas backing store's width"
    );
    assert_eq!(
        shim.matches("canvas.height =").count(),
        1,
        "web/engine/shell.js must have exactly one writer of the canvas backing store's height"
    );
    assert_eq!(
        shim.matches("__crcbl_web_resize").count(),
        1,
        "web/engine/shell.js must report a size to the engine from exactly one place"
    );
    assert_eq!(
        shim.matches(SIZED_THEN_REPORTED).count(),
        1,
        "web/engine/shell.js must size the canvas and then report those same numbers, as \
         `{SIZED_THEN_REPORTED}` — the extent `acquire_next_frame` hands back is only the \
         texture's size because of this"
    );
}

/// **A session's acquires leave one image and one view filed, not one pair per
/// frame it has ever drawn.**
///
/// `getCurrentTexture` hands back a texture the canvas expires by itself, so
/// nothing here was holding GPU memory — but the replayer files it by the index
/// *this* side mints, in a `Map` only a destroy removes from, and this side
/// minted a fresh index every frame. Measured in a browser before the fix, the
/// image and image-view tables climbed by exactly one per rendered frame, in
/// lock step with the replayed-command count, for as long as a demo ran.
///
/// The assertion is the arithmetic a table does: what an acquire files, minus
/// what a destroy removes, over three frames.
#[test]
fn an_acquire_retires_the_pair_the_frame_before_it_took() {
    let (channel, device) = device_on_fresh_channel();
    let surface: SurfaceHandle = Handle::from_bits(1 << 32).expect("a real handle");
    let swapchain = device
        .create_swapchain(&canvas_swapchain_desc(surface, (640, 360)))
        .expect("configuring a canvas needs no reply");

    let frames: Vec<_> = (0..3)
        .map(|_| {
            device
                .acquire_next_frame(swapchain)
                .expect("an acquire needs no reply")
        })
        .collect();

    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");

    let destroyed_images: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            crate::Command::DestroyImage { image } => Some(*image),
            _ => None,
        })
        .collect();
    let destroyed_views: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            crate::Command::DestroyImageView { view } => Some(*view),
            _ => None,
        })
        .collect();

    assert_eq!(
        destroyed_images,
        frames[..2]
            .iter()
            .map(|frame| frame.image)
            .collect::<Vec<_>>(),
        "each acquire retires the image the one before it took, in order — and never the one \
         it is handing to the caller"
    );
    assert_eq!(
        destroyed_views,
        frames[..2]
            .iter()
            .map(|frame| frame.view)
            .collect::<Vec<_>>(),
        "and its view, which is a second table with the same problem"
    );

    let acquires = commands
        .iter()
        .filter(|command| command.name() == "AcquireNextFrame")
        .count();
    assert_eq!(acquires, frames.len(), "one acquire per frame, all encoded");
    assert_eq!(
        acquires - destroyed_images.len(),
        1,
        "however many frames a session draws, the replayer is left holding one"
    );
}

/// **Destroying the swapchain retires the frame it last handed out.**
///
/// The pair lives in tables of the replayer's own, which a `DestroySwapchain`
/// does not touch: without this the last frame of every session stays filed for
/// as long as the page does, and a page that opens and closes a canvas
/// repeatedly accumulates one pair per open.
#[test]
fn destroying_a_swapchain_retires_its_last_acquired_pair() {
    let (channel, device) = device_on_fresh_channel();
    let surface: SurfaceHandle = Handle::from_bits(1 << 32).expect("a real handle");
    let swapchain = device
        .create_swapchain(&canvas_swapchain_desc(surface, (640, 360)))
        .expect("configuring a canvas needs no reply");
    let frame = device
        .acquire_next_frame(swapchain)
        .expect("an acquire needs no reply");

    device.destroy_swapchain(swapchain);

    let commands = channel
        .with(|c| c.encode(|stream| decode_stream(stream.bytes())))
        .expect("the channel is not borrowed")
        .expect("the writer's own bytes decode");
    let tail: Vec<_> = commands
        .iter()
        .rev()
        .take(3)
        .map(crate::Command::name)
        .collect();
    assert_eq!(
        tail,
        ["DestroySwapchain", "DestroyImage", "DestroyImageView"],
        "the frame's two handles are retired, and before the swapchain they came from"
    );
    assert!(
        commands.iter().any(|command| matches!(
            command,
            crate::Command::DestroyImage { image } if *image == frame.image
        )),
        "the image retired is the one the last acquire handed out"
    );
    assert!(
        commands.iter().any(|command| matches!(
            command,
            crate::Command::DestroyImageView { view } if *view == frame.view
        )),
        "and its view"
    );

    // A second acquire on a handle this device no longer knows still encodes,
    // and has nothing to retire — the replayer decides what an unknown
    // swapchain is, and a panic here would make a shutdown race fatal.
    let after = device
        .acquire_next_frame(swapchain)
        .expect("an acquire needs no reply");
    assert_eq!(
        after.extent,
        (0, 0),
        "a swapchain this device has forgotten"
    );
}

/// **The extent an acquired frame carries is the one the *last* configure
/// carried**, never the one the swapchain was created at.
///
/// `crcbl::engine` renders at `AcquiredFrame::extent` rather than at the size it
/// asked for — the swapchain seam's obligation 3 — and a resize reaches this
/// backend only as a `reconfigure_swapchain`. A reconfigure that left the map
/// alone would go on reporting the size the canvas had before the resize, and
/// `suboptimal: false` would insist nothing was wrong while every pass rendered
/// to the wrong viewport.
#[test]
fn the_acquired_extent_is_the_one_last_configured() {
    let (_channel, device) = device_on_fresh_channel();
    let surface: SurfaceHandle = Handle::from_bits(1 << 32).expect("a real handle");

    let swapchain = device
        .create_swapchain(&canvas_swapchain_desc(surface, (640, 360)))
        .expect("configuring a canvas needs no reply");
    assert_eq!(
        device
            .acquire_next_frame(swapchain)
            .expect("an acquire needs no reply")
            .extent,
        (640, 360),
        "a fresh swapchain reports the extent it was created at"
    );

    device
        .reconfigure_swapchain(swapchain, &canvas_swapchain_desc(surface, (800, 450)))
        .expect("reconfiguring a canvas needs no reply");
    let acquired = device
        .acquire_next_frame(swapchain)
        .expect("an acquire needs no reply");
    assert_eq!(
        acquired.extent,
        (800, 450),
        "the reconfigure is what moves the extent every later frame is told"
    );
    assert!(
        !acquired.suboptimal,
        "a canvas is sized by its own width and height, so a configure can never fall behind \
         it and there is nothing to report as suboptimal"
    );
}
