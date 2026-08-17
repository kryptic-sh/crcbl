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
    Instance, Limits, MemoryLocation, PendingDevice, PresentMode, QueueKind, ReadbackDesc,
    ReadbackState, SubmitInfo,
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

/// Drive an open future to its instance on a fed adapter reply.
fn opened_instance() -> WebGpuInstance {
    let mut open = WebGpuInstanceOpen::start();
    let channel = open.channel();
    // `enumerate_adapters` is the first awaited command on a fresh channel, so
    // its sequence is 0.
    feed(&channel, |w| w.adapter(0, &granted_adapter()));
    match Pin::new(&mut open).poll(&mut noop_context()) {
        Poll::Ready(Ok(instance)) => instance,
        other => panic!("the instance must open on the adapter reply, got {other:?}"),
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
fn instance_open_settles_to_an_instance_on_an_adapter_reply() {
    let mut open = WebGpuInstanceOpen::start();
    let channel = open.channel();

    // Nothing fed yet: the enumeration is still in flight, which is `Pending`.
    assert!(
        matches!(Pin::new(&mut open).poll(&mut noop_context()), Poll::Pending),
        "an unanswered enumeration polls Pending"
    );

    let info = granted_adapter();
    feed(&channel, |w| w.adapter(0, &info));
    let Poll::Ready(Ok(instance)) = Pin::new(&mut open).poll(&mut noop_context()) else {
        panic!("the adapter reply must settle the open future");
    };
    assert_eq!(instance.adapters(), vec![info]);
    assert_eq!(instance.backend(), BackendKind::WebGpu);
}

// ── (b) surface caps ───────────────────────────────────────────────────────

#[test]
fn surface_caps_answers_the_constant_canvas_caps_synchronously() {
    let instance = opened_instance();
    let surface = unsafe { instance.create_surface(&SurfaceTarget::Web { canvas_id: 7 }) }
        .expect("a Web canvas surface is reachable");

    // No reply is fed: the answer must be synchronous.
    let caps = instance
        .surface_caps(surface, AdapterId(0))
        .expect("the constant caps");
    assert_eq!(caps.formats, vec![Format::Bgra8Unorm, Format::Rgba8Unorm]);
    assert_eq!(caps.present_modes, vec![PresentMode::Fifo]);
    assert_eq!(
        caps.composite_alpha,
        vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied]
    );
    assert_eq!((caps.min_image_count, caps.max_image_count), (2, 2));
    assert_eq!(caps.current_extent, None);
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

// ── (f) an unwired op fails loudly at finish ───────────────────────────────

#[test]
fn finish_fails_loudly_after_recording_an_unwired_op() {
    let (_channel, device) = device_on_fresh_channel();
    let queue = device
        .queue(QueueKind::Graphics)
        .expect("the graphics queue");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc { label: None, queue });

    // `set_stencil_reference` has no stream command yet; recording it must make
    // finish refuse rather than replay a command buffer missing the state.
    encoder.set_stencil_reference(0);
    let Err(HalError::Unsupported { what, .. }) = encoder.finish() else {
        panic!("finish must refuse a recorded unwired op");
    };
    assert!(
        what.contains("set_stencil_reference"),
        "the error names the op: {what}"
    );
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
