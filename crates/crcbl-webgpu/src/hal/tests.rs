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
    AdapterId, AdapterInfo, BackendKind, BufferDesc, BufferHandle, BufferUsage, CommandEncoderDesc,
    CompositeAlpha, Device, DeviceCaps, DeviceDesc, DeviceType, Features, Format, HalError,
    Instance, Limits, MemoryLocation, PendingDevice, PresentMode, QuerySetHandle, QueueKind,
    ReadbackDesc, ReadbackState, SubmitInfo, SurfaceCaps,
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

    // A fabricated source buffer handle: request_readback needs one but never
    // creates it, so the readback poll stays the second command (sequence 1).
    let source: BufferHandle = Handle::from_bits((1 << 32) | 1).expect("a real handle");
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

    // request_readback is sequence 0 (not awaited), the poll is sequence 1.
    feed(&channel, |w| w.readback_ready(1, readback, &[1, 2, 3, 4]));
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

    let source: BufferHandle = Handle::from_bits((1 << 32) | 1).expect("a real handle");
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

    // request_readback is sequence 0 (not awaited), the poll is sequence 1.
    feed(&channel, |w| {
        w.readback_failed(1, readback, "mapAsync rejected: device was lost");
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

    // `write_timestamp` has no stream command yet; recording it must make finish
    // refuse rather than replay a command buffer missing the write.
    let set = QuerySetHandle::from_bits(1 << 32).expect("generation 1 is not zero");
    encoder.write_timestamp(set, 0);
    let Err(HalError::Unsupported { what, .. }) = encoder.finish() else {
        panic!("finish must refuse a recorded unwired op");
    };
    assert!(
        what.contains("write_timestamp"),
        "the error names the op: {what}"
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

    encoder.begin_compute_pass(&crcbl_hal::ComputePassDesc { label: None });
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

// ── refusals of needed-but-unwired Device methods ──────────────────────────

#[test]
fn needed_but_unwired_device_methods_refuse_loudly() {
    let (_channel, device) = device_on_fresh_channel();
    assert!(matches!(
        device.query_results(
            Handle::from_bits((1 << 32) | 2).expect("a real handle"),
            0,
            &mut [0u64; 1],
        ),
        Err(HalError::Unsupported { .. })
    ));
    assert!(matches!(
        device.create_query_set(&crcbl_hal::QuerySetDesc {
            label: None,
            kind: crcbl_hal::QueryKind::Timestamp,
            count: 1,
        }),
        Err(HalError::Unsupported { .. })
    ));
    assert!(
        matches!(
            device.create_mesh_pipeline(&mesh_pipeline_desc()),
            Err(HalError::Unsupported { .. })
        ),
        "a mesh pipeline is legitimately refused: WebGPU has no mesh stage"
    );
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
        mesh: crcbl_hal::ShaderEntry {
            module: Handle::from_bits((1 << 32) | 4).expect("a real handle"),
            entry_point: "main",
        },
        fragment: None,
        primitive: crcbl_hal::PrimitiveState::default(),
        depth_stencil: None,
        multisample: crcbl_hal::MultisampleState::default(),
        color_targets: &[],
    }
}
