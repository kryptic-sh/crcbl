//! The round trip, driven from JavaScript, so that a browser can be asked
//! whether it works.
//!
//! Everything else in this crate is checked without a browser: the encoding
//! against a committed fixture, the transport against a synthetic
//! `WebAssembly.Memory`, the reply format against the same fixture read the
//! other way. **None of that can call `navigator.gpu`**, which is the one thing
//! this slice added and the one thing no node tool can reach. So there has to be
//! an entry point a page can drive end to end — encode a request, let the
//! replayer answer it, read what came back — and this module is it.
//!
//! # What it is, plainly
//!
//! An observation point, not a backend. It owns a [`StreamChannel`] because
//! nothing else does yet: `crcbl::backend`'s registry entry for
//! [`BackendKind::WebGpu`](crcbl_hal::BackendKind::WebGpu) still refuses, so no
//! engine code calls [`install`] and the seven transport
//! exports answer `0` on every frame of every demo. **When the backend arrives
//! and installs its own channel, this module has done its job and goes**, taking
//! its exports with it — and it refuses rather than fights on the way,
//! because [`install`] will not replace a live channel.
//!
//! # Exports
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_web_gpu_probe_adapters`](shim::__crcbl_web_gpu_probe_adapters) | `() -> i32` | Encode one enumeration and register its wait. `1`, or `0` if there was no room or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_state`](shim::__crcbl_web_gpu_probe_state) | `() -> i32` | Drain whatever JS has committed and answer one of the `PROBE_*` codes. |
//! | [`__crcbl_web_gpu_probe_text_ptr`](shim::__crcbl_web_gpu_probe_text_ptr) | `() -> i32` | Where the adapter's name, or the reason there is none, starts. |
//! | [`__crcbl_web_gpu_probe_text_len`](shim::__crcbl_web_gpu_probe_text_len) | `() -> i32` | How long it is, in UTF-8 bytes. |
//! | [`__crcbl_web_gpu_probe_features_lo`](shim::__crcbl_web_gpu_probe_features_lo) | `() -> i32` | Low 32 bits of the granted adapter's [`Features`]. |
//! | [`__crcbl_web_gpu_probe_features_hi`](shim::__crcbl_web_gpu_probe_features_hi) | `() -> i32` | High 32 bits of the same. |
//! | [`__crcbl_web_gpu_probe_max_image_2d`](shim::__crcbl_web_gpu_probe_max_image_2d) | `() -> i32` | The granted adapter's [`Limits::max_image_2d`](crcbl_hal::Limits::max_image_2d). |
//! | [`__crcbl_web_gpu_probe_device`](shim::__crcbl_web_gpu_probe_device) | `() -> i32` | Encode one device request for the adapter that was granted, and register its wait. `1`, or `0` if nothing has been granted yet, there was no room, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_device_state`](shim::__crcbl_web_gpu_probe_device_state) | `() -> i32` | Drain, and answer one of the `DEVICE_*` codes. |
//! | [`__crcbl_web_gpu_probe_device_reason_ptr`](shim::__crcbl_web_gpu_probe_device_reason_ptr) | `() -> i32` | Where the reason no device opened starts. Empty when one did. |
//! | [`__crcbl_web_gpu_probe_device_reason_len`](shim::__crcbl_web_gpu_probe_device_reason_len) | `() -> i32` | How long it is, in UTF-8 bytes. |
//! | [`__crcbl_web_gpu_probe_device_features_lo`](shim::__crcbl_web_gpu_probe_device_features_lo) | `() -> i32` | Low 32 bits of the **opened device's** [`Features`]. |
//! | [`__crcbl_web_gpu_probe_device_features_hi`](shim::__crcbl_web_gpu_probe_device_features_hi) | `() -> i32` | High 32 bits of the same. |
//! | [`__crcbl_web_gpu_probe_device_max_image_2d`](shim::__crcbl_web_gpu_probe_device_max_image_2d) | `() -> i32` | The opened device's [`Limits::max_image_2d`](crcbl_hal::Limits::max_image_2d). |
//! | [`__crcbl_web_gpu_probe_surface`](shim::__crcbl_web_gpu_probe_surface) | `(i32) -> i32` | Encode one [`CreateSurface`](crate::Command::CreateSurface) against [`PROBE_SURFACE`], naming the canvas that `canvas_id` is the page's registry key for. `1`, or `0` if the probe is re-entered or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_buffer`](shim::__crcbl_web_gpu_probe_buffer) | `(i32) -> i32` | Encode one [`CreateBuffer`](crate::Command::CreateBuffer) against [`PROBE_BUFFER`], of `size` bytes. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_image`](shim::__crcbl_web_gpu_probe_image) | `(i32, i32, i32) -> i32` | Encode one [`CreateImage`](crate::Command::CreateImage) against [`PROBE_IMAGE`], of `width` by `height` texels with `mip_levels` levels. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_image_view`](shim::__crcbl_web_gpu_probe_image_view) | `() -> i32` | Encode one [`CreateImageView`](crate::Command::CreateImageView) against [`PROBE_IMAGE_VIEW`], viewing [`PROBE_IMAGE`]. `1`, or `0` on the same three conditions. |
//! | [`__crcbl_web_gpu_probe_sampler`](shim::__crcbl_web_gpu_probe_sampler) | `() -> i32` | Encode one [`CreateSampler`](crate::Command::CreateSampler) against [`PROBE_SAMPLER`], with [`PROBE_SAMPLER_DESC`]. `1`, or `0` on the same three conditions. |
//! | [`__crcbl_web_gpu_probe_bind_group_layout`](shim::__crcbl_web_gpu_probe_bind_group_layout) | `() -> i32` | Encode one [`CreateBindGroupLayout`](crate::Command::CreateBindGroupLayout) against [`PROBE_BIND_GROUP_LAYOUT`], with [`PROBE_BIND_GROUP_LAYOUT_DESC`]. `1`, or `0` on the same three conditions. |
//! | [`__crcbl_web_gpu_probe_bind_group`](shim::__crcbl_web_gpu_probe_bind_group) | `() -> i32` | Encode one frame — a layout, its resources, and a [`CreateBindGroup`](crate::Command::CreateBindGroup) against [`PROBE_BIND_GROUP`] with [`PROBE_BIND_GROUP_DESC`]. `1`, or `0` on the same three conditions. |
//! | [`__crcbl_web_gpu_probe_shader_module`](shim::__crcbl_web_gpu_probe_shader_module) | `() -> i32` | Encode one [`CreateShaderModule`](crate::Command::CreateShaderModule) against [`PROBE_SHADER_MODULE`] with [`PROBE_SHADER_MODULE_DESC`]. `1`, or `0` on the same three conditions. |
//! | [`__crcbl_web_gpu_probe_surface_caps`](shim::__crcbl_web_gpu_probe_surface_caps) | `() -> i32` | Encode one [`SurfaceCaps`](crate::Command::SurfaceCaps) and register its wait. `1`, or `0` if there was no room or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_surface_caps_state`](shim::__crcbl_web_gpu_probe_surface_caps_state) | `() -> i32` | Drain, and answer one of the `CAPS_*` codes. |
//! | [`__crcbl_web_gpu_probe_surface_caps_reason_ptr`](shim::__crcbl_web_gpu_probe_surface_caps_reason_ptr) | `() -> i32` | Where the reason the query answered nothing starts. Empty when it answered. |
//! | [`__crcbl_web_gpu_probe_surface_caps_reason_len`](shim::__crcbl_web_gpu_probe_surface_caps_reason_len) | `() -> i32` | How long it is, in UTF-8 bytes. |
//! | [`__crcbl_web_gpu_probe_surface_caps_cause`](shim::__crcbl_web_gpu_probe_surface_caps_cause) | `() -> i32` | Which [`SurfaceCapsFailure`] refused it, as [`crate::tag::surface_caps_failure_code`] spells it. |
//! | [`__crcbl_web_gpu_probe_surface_caps_format`](shim::__crcbl_web_gpu_probe_surface_caps_format) | `() -> i32` | The surface's [`preferred_format`](SurfaceCaps::preferred_format), as [`crate::tag::format_code`] spells it. |
//! | [`__crcbl_web_gpu_probe_surface_caps_present_modes`](shim::__crcbl_web_gpu_probe_surface_caps_present_modes) | `() -> i32` | One bit per mode offered, at `1 <<` its [`crate::tag::present_mode_code`]. |
//! | [`__crcbl_web_gpu_probe_surface_caps_has_extent`](shim::__crcbl_web_gpu_probe_surface_caps_has_extent) | `() -> i32` | `1` if the surface reported a [`current_extent`](SurfaceCaps::current_extent), `0` if it reported none. |
//!
//! **`state` before `ptr`, always** — the log queue's rule and for its reason:
//! a `state` call decodes a buffer and clones a string out of it, so it
//! allocates, and an allocation may grow wasm memory and detach a `Uint8Array`
//! built before the call. The pointers, the lengths and the numbers allocate
//! nothing.
//!
//! **Any one `state` drains for all three probes.** There is one channel and one
//! committed reply buffer, so the first of the three calls in a frame decodes it
//! and hands each probe its own answer; the others find nothing left and report
//! what their probe now holds. The consequence worth stating: a buffer that will
//! not decode is reported by whichever was asked first, as that probe's
//! `*_UNDECODABLE`, and the others report the state they were already in.
//! Dropping the other probes' answers instead would leave a command waiting for
//! ever, which is the one thing this channel must never do.
//!
//! # The device this asks for, and why it asks for so little
//!
//! [`probe_device_desc`] requires [`Features::COMPUTE`](crcbl_hal::Features::COMPUTE)
//! — core WebGPU, so every browser can satisfy it — and asks for **nothing
//! optional**. That is not timidity: it is what makes the answer checkable. A
//! device opened with no optional features and no requested limits is the
//! specification's own default, so the page can open a second one for itself
//! and compare, and the result differs from the *adapter's* capabilities on any
//! machine whose adapter reports more than the floor. A request that asked for
//! everything the adapter had would produce a device whose capabilities equal
//! the adapter's, and a backend that reported the adapter's record for its
//! device would then pass.
//!
//! [`DeviceDesc::for_adapter`](crcbl_hal::DeviceDesc::for_adapter) is
//! deliberately *not* what this uses: it requires
//! [`TIMELINE_SEMAPHORE`](crcbl_hal::Features::TIMELINE_SEMAPHORE), which WebGPU
//! does not have, so it is the refusal case rather than the opening one. See
//! [`crate::device`].
//!
//! # `CreateSurface` is one export, and that is the command's shape
//!
//! [`__crcbl_web_gpu_probe_surface`](shim::__crcbl_web_gpu_probe_surface) has no
//! `state`, no codes of its own and nothing to absorb, because
//! [`CreateSurface`](crate::Command::CreateSurface) has **no entry on the reply
//! channel**: identity is positional, so wasm names the handle itself and moves
//! on, and there is nothing for a browser to send back. A state machine here
//! would have one state and a poll would have nothing to poll for, so the honest
//! shape is one call that encodes one command and answers whether it went.
//!
//! [`__crcbl_web_gpu_probe_buffer`](shim::__crcbl_web_gpu_probe_buffer) has the
//! same shape for the same reason — [`CreateBuffer`](crate::Command::CreateBuffer)
//! is answered by nothing either — with one difference that is the command's
//! rather than this module's: **it is a device method, so it refuses until a
//! device has opened.** That is [`__crcbl_web_gpu_probe_device`](shim::__crcbl_web_gpu_probe_device)'s
//! ordering rule seen once further along, and it is a rule about what the
//! *replayer* has rather than about what the descriptor names: nothing in a
//! [`BufferDesc`] comes from the device, but the
//! `createBuffer` call needs one, and a stream that asks before then is asking
//! the page for something it has not got. `web/engine/gpu-replay.js` records
//! exactly that as a `take_error`, which is where a buffer failure goes for
//! want of a reply channel.
//!
//! # The image pair is that shape twice, and the second one names the first
//!
//! [`__crcbl_web_gpu_probe_image`](shim::__crcbl_web_gpu_probe_image) is
//! [`__crcbl_web_gpu_probe_buffer`](shim::__crcbl_web_gpu_probe_buffer) in every
//! respect — a device method, so it refuses until a device has opened; no
//! `state`, because nothing answers a creation; the numbers passed in by the
//! page, so what it reads back off `GPUTexture` is something it chose.
//!
//! [`__crcbl_web_gpu_probe_image_view`](shim::__crcbl_web_gpu_probe_image_view)
//! is the one export here whose command **depends on another command having
//! worked**, and it cannot check that: the image lives in the page's replayer
//! and nothing in wasm holds one. So it refuses on the same condition its
//! neighbour does — no device — and an image handle that resolves to nothing is
//! the replayer's to report, through `Device::take_error`, exactly as a buffer
//! that could not be made is. That is a decision `web/engine/gpu-replay.js`
//! argues rather than an omission here: a view naming a missing image is a far
//! side that got its ordering wrong mid-frame, and taking the frame down over it
//! would abandon every command after it.
//!
//! The pair is also what puts [`ImageSubresourceRange::ALL`](crcbl_hal::ImageSubresourceRange::ALL)
//! in front of a real browser. Both counts in
//! [`PROBE_IMAGE_VIEW_DESC`] are the sentinel, they cross verbatim by the rule
//! `docs/plan/41-webgpu-stream.md` sets, and WebGPU spells "the rest" as an
//! **absent** descriptor member rather than as a number — so a replayer that
//! passed `4294967295` on builds a view the browser refuses, and only a browser
//! can say so.
//!
//! # The sampler is the same shape a third time, and it reports almost nothing
//!
//! [`__crcbl_web_gpu_probe_sampler`](shim::__crcbl_web_gpu_probe_sampler) is
//! [`__crcbl_web_gpu_probe_image_view`](shim::__crcbl_web_gpu_probe_image_view)
//! in every structural respect — a device method, so it refuses until a device
//! has opened; no `state`, because nothing answers a creation; no arguments,
//! because the descriptor is fixed.
//!
//! **What differs is what a browser can be asked about it, and the answer is:
//! its label.** A `GPUSampler` has no other readable member at all — no filters,
//! no address modes, no clamps — so the "pass the numbers in, read them back
//! off the object" argument the image and buffer probes are built on has nothing
//! to work with here. What is left is exactly what a stub cannot fake: that the
//! object is an instance of this browser's own `GPUSampler`, and that the device
//! reported nothing about the descriptor afterwards.
//!
//! That second half is what makes [`PROBE_SAMPLER_DESC`] worth choosing rather
//! than defaulting. Its `lod_max` is [`f32::MAX`] — the "no limit" sentinel
//! [`SamplerDesc::default`] carries — which crosses the wire verbatim by the
//! same rule the view's range does, and which the replayer has to hand WebGPU as
//! an explicit `lodMaxClamp` rather than by omitting the member: WebGPU's own
//! default for that member is a *number* rather than "the rest", so an omission
//! silently substitutes it. Only a browser can say the value this seam sends is
//! one `createSampler` accepts, and it says so by reporting nothing on the
//! device's error channel.
//!
//! # The bind-group layout is that shape a fourth time, and it is a *list*
//!
//! [`__crcbl_web_gpu_probe_bind_group_layout`](shim::__crcbl_web_gpu_probe_bind_group_layout)
//! is [`__crcbl_web_gpu_probe_sampler`](shim::__crcbl_web_gpu_probe_sampler) in
//! every structural respect, down to what a browser can be asked about the
//! result: **a `GPUBindGroupLayout` reports its `label` and nothing else**, so
//! the evidence is again the object's class and the device's silence afterwards.
//!
//! What is new is the *body*. Every command before this one is a fixed set of
//! fields; this one is a counted list of structs, and each struct carries an
//! enum whose variants have different-length payloads. A stride wrong by a byte
//! therefore does not truncate — it decodes the next entry out of the middle of
//! this one, and produces a layout that is well-formed and describes different
//! resources. [`PROBE_BIND_GROUP_LAYOUT_ENTRIES`] is four entries for that
//! reason, and every one of them is a kind WebGPU can express, so what a browser
//! is being asked is whether the *whole list* survived.
//!
//! Its neighbour [`__crcbl_web_gpu_probe_surface_caps`](shim::__crcbl_web_gpu_probe_surface_caps)
//! is the opposite case and has the full awaited shape, because
//! [`SurfaceCaps`](crate::Command::SurfaceCaps) **is** answered — with the
//! capabilities or with a refusal, and both name the sequence it was assigned.
//!
//! That also decides where a failure surfaces. A canvas id the page has not
//! registered, or a canvas that will not give up a `webgpu` context, is a
//! `SurfaceError` **thrown out of the replayer in JS** — the far side cannot be
//! told, so the near side is. `web/engine/gpu-replay.js` argues that choice
//! where it is made.
//!
//! # Why three numbers and not the whole of `AdapterInfo`
//!
//! These exist for the browser gate, and a gate check is only worth its line if a
//! browser can **corroborate** it — the adapter-name check compares what wasm
//! received against what `navigator.gpu` tells the same page, which is what makes
//! it evidence rather than a constant. Two of the seven fields on the wire have
//! that property: the feature set, which the page can rebuild from
//! `adapter.features`, and the limits, which the page can read off
//! `adapter.limits`. Both vary per machine and per browser.
//!
//! The other five do not. `vendor_id`, `device_id`, `device_type` and `driver`
//! are the documented absences — a browser has nothing to disagree with — and
//! `id` is `0` by construction. Exporting them would add checks that can only
//! restate a constant, so they are held by `cargo test` and by
//! `web/tools/gpu-replay.mjs` instead, where the whole record is compared field
//! for field.
//!
//! `max_image_2d` is one limit of nineteen for the same reason: it is
//! `maxTextureDimension2D`, which differs between a phone and a desktop, so it
//! catches a limits block that crossed as zeros. What holds the other eighteen is
//! the mapping check the gate runs in-page against the live adapter, plus the
//! committed fixture.
//!
//! The device's three numbers are the same three for the same reasons, and one
//! more: **they are what says the device's capabilities are not a copy of the
//! adapter's.** A page can open its own default device and read `device.features`
//! and `device.limits.maxTextureDimension2D` off it, and both differ from the
//! adapter's whenever the adapter reports anything above the specification's
//! floor.
//!
//! `i32` pairs rather than one `i64` because the whole of this ABI is
//! `(i32, …) -> i32`, which `docs/plan/41-webgpu-stream.md` sets as the
//! convention and which needs no `BigInt` on the JS side.
//!
//! # Why the preferred format, and not the whole of `SurfaceCaps`
//!
//! The same argument, landing on exactly one field. [`SurfaceCaps`] has six, and
//! five of them are **this seam's decisions rather than the browser's**:
//! `web/engine/gpu-replay.js` fills [`present_modes`](SurfaceCaps::present_modes)
//! with `Fifo` alone because WebGPU has no present mode at all,
//! [`composite_alpha`](SurfaceCaps::composite_alpha) with the two
//! `GPUCanvasConfiguration.alphaMode` spellings, the two image counts with the
//! statement that a canvas has one configuration, and
//! [`current_extent`](SurfaceCaps::current_extent) with nothing. A browser has
//! no opinion to disagree with about any of them, so an export for each would
//! buy checks that can only read back what that file wrote.
//!
//! [`formats`](SurfaceCaps::formats) is the field the browser fills, and its
//! first entry — which is what [`preferred_format`](SurfaceCaps::preferred_format)
//! answers, neither canvas format being sRGB — is `getPreferredCanvasFormat()`.
//! **That varies by browser and by machine and the page can ask for it
//! independently**, so it is the one value here a gate check can corroborate
//! instead of restate, and it is exported as its wire code.
//!
//! Two of the other five are exported anyway, doing a different job. The format
//! is one code out of a record of three lists, two counts and an optional pair,
//! and it decodes correctly whatever happens to the rest — so
//! [`present_modes`](SurfaceCaps::present_modes), which the seam promises always
//! contains [`Fifo`](crcbl_hal::PresentMode::Fifo), and the presence of
//! [`current_extent`](SurfaceCaps::current_extent), which a browser never has, are
//! the two cheapest facts that say the rest of the record survived the crossing:
//! an empty mode list or an extent that appeared from nowhere is a reader that
//! lost its place after the first list. Neither is a count written into a check,
//! which would only assert what the replayer chose; both are invariants that hold
//! however many entries there turn out to be.
//!
//! # The capability query takes nothing, so there is one answer to observe
//!
//! Its neighbours take what they can: the enumeration takes nothing, and the
//! device request takes its adapter from the enumeration that was answered and
//! refuses to encode until there is one. This one takes nothing either, because
//! [`Command::SurfaceCaps`](crate::Command::SurfaceCaps) carries nothing —
//! `getPreferredCanvasFormat()` is a method on `GPU` and the rest of the record
//! is fixed for a canvas, so the surface and the adapter the HAL call names are
//! validated by an `impl Instance` against its own tables and never travel.
//!
//! What that costs is the refusals: a stale surface handle and an adapter index
//! nothing enumerated were the two causes this export could drive, and neither
//! is a question the wire asks any more. The only cause left is
//! [`Backend`](SurfaceCapsFailure::Backend), which is the replayer meeting
//! something it did not anticipate — a canvas format this seam has no
//! [`Format`] for — and which nothing here can provoke on
//! demand. So what a browser gate can observe of this command is the *answer*,
//! and [`CAPS_REFUSED`] is a path `cargo test` drives rather than a browser.
//!
//! `web/engine/gpu-probe.js` is the page's half, and
//! `web/tools/browser-e2e.mjs` is what drives it in a real browser.

use std::cell::RefCell;
use std::rc::Rc;

use crcbl_hal::{
    AdapterId, BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc,
    BindGroupLayoutEntry, BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource,
    BufferDesc, BufferHandle, BufferUsage, CompareOp, DeviceDesc, Extent3d, Features, FilterMode,
    Format, ImageDesc, ImageHandle, ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc,
    ImageViewHandle, ImageViewType, MemoryLocation, SampleType, SamplerAddressMode, SamplerDesc,
    SamplerHandle, ShaderModuleDesc, ShaderModuleHandle, ShaderStages, SurfaceCaps, SurfaceHandle,
};

use crate::device::DeviceProbe;
use crate::instance::AdapterProbe;
use crate::reply::{Reply, SurfaceCapsFailure};
use crate::web::{StreamChannel, install};
use crate::writer::StreamWriter;

/// [`AdapterProbe::Unasked`], or no channel to ask through.
pub const PROBE_UNASKED: u32 = 0;
/// [`AdapterProbe::Waiting`] — the request is out and unanswered.
pub const PROBE_WAITING: u32 = 1;
/// [`AdapterProbe::Granted`]; the text is the adapter's name.
pub const PROBE_GRANTED: u32 = 2;
/// [`AdapterProbe::Refused`]; the text is the reason.
pub const PROBE_REFUSED: u32 = 3;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the text is the [`DecodeError`](crate::DecodeError).
///
/// Distinct from [`PROBE_REFUSED`] because the two blame opposite halves: a
/// refusal is a browser with no GPU, and this is the format's two hand-written
/// sides having drifted.
pub const PROBE_UNDECODABLE: u32 = 4;

/// [`DeviceProbe::Unasked`], or no adapter to open.
pub const DEVICE_UNASKED: u32 = 0;
/// [`DeviceProbe::Waiting`] — `requestDevice` has not settled.
///
/// [`DeviceRequestState::Pending`](crcbl_hal::DeviceRequestState::Pending) seen
/// through the ABI, and the ordinary answer on every frame between the ask and
/// the answer.
pub const DEVICE_WAITING: u32 = 1;
/// [`DeviceProbe::Opened`]; the three numeric exports carry the device's own
/// capabilities and the reason is empty.
pub const DEVICE_OPENED: u32 = 2;
/// [`DeviceProbe::Failed`]; the reason says what the browser refused, or what
/// this backend refused to ask it for.
pub const DEVICE_FAILED: u32 = 3;
/// The committed reply buffer would not decode; the reason is the
/// [`DecodeError`](crate::DecodeError). [`PROBE_UNDECODABLE`]'s twin, and
/// distinct from [`DEVICE_FAILED`] for its reason: a refusal is a browser, and
/// this is the format's two hand-written sides having drifted.
pub const DEVICE_UNDECODABLE: u32 = 4;

/// Nothing has been asked, or there is no channel to ask through.
pub const CAPS_UNASKED: u32 = 0;
/// The query is out and unanswered.
///
/// Real but brief: `web/engine/gpu-replay.js` answers this command *within* the
/// replay rather than out of a promise, so it settles on the frame the demo's
/// loop replays it — one frame later than the call that asked, and no more.
pub const CAPS_WAITING: u32 = 1;
/// The surface answered; the format, the mode bits and the extent flag carry
/// what it will accept, and the reason is empty.
pub const CAPS_ANSWERED: u32 = 2;
/// The query answered nothing; the reason says what happened and
/// [`shim::__crcbl_web_gpu_probe_surface_caps_cause`] says which
/// [`SurfaceCapsFailure`] it was.
///
/// **Not an error on this seam.** `surface_caps` is how adapter selection is
/// done, so a query that answers nothing is a step of it — and it is still a
/// reply rather than a thrown frame, which is what this code exists to observe.
pub const CAPS_REFUSED: u32 = 3;
/// The committed reply buffer would not decode; the reason is the
/// [`DecodeError`](crate::DecodeError). [`PROBE_UNDECODABLE`]'s twin, and
/// distinct from [`CAPS_REFUSED`] for its reason: a refusal is an answer this
/// seam asked for, and this is the format's two hand-written sides having
/// drifted.
pub const CAPS_UNDECODABLE: u32 = 4;

/// The descriptor [`shim::__crcbl_web_gpu_probe_device`] asks with.
///
/// Requires only [`Features::COMPUTE`], which core WebGPU grants with no
/// `GPUFeatureName` behind it, and asks for nothing optional — see the [module
/// docs](self#the-device-this-asks-for-and-why-it-asks-for-so-little) for why
/// the emptiness is the point rather than a placeholder.
#[must_use]
pub const fn probe_device_desc(adapter: AdapterId) -> DeviceDesc<'static> {
    DeviceDesc {
        label: Some("crcbl-webgpu probe"),
        adapter,
        required_features: Features::COMPUTE,
        optional_features: Features::empty(),
        compatible_surface: None,
    }
}

/// The surface [`shim::__crcbl_web_gpu_probe_surface`] creates, every time.
///
/// One fixed handle rather than one drawn from a pool, because the probe has no
/// pool to draw from: it is an observation point, and identity on this stream is
/// positional — wasm picks the id, JS files the context under it. Index `0` and
/// generation `1`, the smallest
/// [`Handle::from_bits`](crcbl_core::Handle::from_bits) accepts and all this
/// module needs: nothing here destroys a surface, so the index is never reissued
/// and a generation has nothing to distinguish.
///
/// Asking twice therefore names this same surface twice, and the replayer's
/// table takes the second context in the first's place rather than growing.
pub const PROBE_SURFACE: SurfaceHandle = match SurfaceHandle::from_bits(1 << 32) {
    Some(surface) => surface,
    // Generation `1`, written into the high half above, so this arm is the
    // expression being wrong rather than a case anything can reach.
    None => panic!("generation 1 is not zero"),
};

/// The buffer [`shim::__crcbl_web_gpu_probe_buffer`] creates, every time.
///
/// [`PROBE_SURFACE`]'s twin, on its terms and for its reasons — one fixed
/// handle, index `0` and generation `1`, because identity on this stream is
/// positional and this module has no pool to draw from. **Its bits are that
/// surface's bits, deliberately.** A handle carries no kind, so a buffer and a
/// surface may hold the same eight bytes; the opcode is what says which table an
/// id indexes, and a page that files both under the same key would be a replayer
/// with one table where the crate docs require two.
pub const PROBE_BUFFER: BufferHandle = match BufferHandle::from_bits(1 << 32) {
    Some(buffer) => buffer,
    // Generation `1`, written into the high half above, so this arm is the
    // expression being wrong rather than a case anything can reach.
    None => panic!("generation 1 is not zero"),
};

/// The descriptor [`shim::__crcbl_web_gpu_probe_buffer`] asks with.
///
/// **Every field is one a browser can be held to**, which is what an
/// observation point is for. `size` is the caller's, so a page passes a number
/// and reads `GPUBuffer.size` back; the label reaches `GPUBuffer.label`; and the
/// usage is two flags that map onto two different `GPUBufferUsage` bits, so a
/// translation that dropped one or or-ed the wrong constant produces a number
/// the page can see is wrong.
///
/// [`MemoryLocation::DeviceLocal`] because it is the location WebGPU expresses
/// by adding *nothing* — the other two add a mapping usage — so what
/// `GPUBuffer.usage` reports is the usage word alone, and the check on it stays
/// about the flags rather than about the location. The locations are held by
/// `web/tools/gpu-replay.mjs`, which drives all three.
#[must_use]
pub const fn probe_buffer_desc(size: u64) -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu probe buffer"),
        size,
        usage: BufferUsage::STORAGE.union(BufferUsage::TRANSFER_DST),
        memory: MemoryLocation::DeviceLocal,
    }
}

/// The image [`shim::__crcbl_web_gpu_probe_image`] creates, every time.
///
/// [`PROBE_BUFFER`]'s twin on its terms, and its bits are that buffer's and that
/// surface's — deliberately, for the reason stated there: a handle carries no
/// kind, the opcode is what says which table an id indexes, and a page filing
/// three kinds under one key would be a replayer with one table where the crate
/// docs require three.
pub const PROBE_IMAGE: ImageHandle = match ImageHandle::from_bits(1 << 32) {
    Some(image) => image,
    // Generation `1`, written into the high half above, so this arm is the
    // expression being wrong rather than a case anything can reach.
    None => panic!("generation 1 is not zero"),
};

/// The view [`shim::__crcbl_web_gpu_probe_image_view`] creates, every time.
///
/// The same bits again, and here the sharing is the *point* rather than an
/// economy: a view and the image it views are separate objects in separate
/// tables, and these two carry identical eight bytes, so a replayer that filed
/// them together would overwrite the image with its own view.
pub const PROBE_IMAGE_VIEW: ImageViewHandle = match ImageViewHandle::from_bits(1 << 32) {
    Some(view) => view,
    // Generation `1`, as above.
    None => panic!("generation 1 is not zero"),
};

/// The descriptor [`shim::__crcbl_web_gpu_probe_image`] asks with.
///
/// **Every field is one a browser can be held to**, which is
/// [`probe_buffer_desc`]'s standard and is easier to meet here: a `GPUTexture`
/// reports its `width`, `height`, `depthOrArrayLayers`, `mipLevelCount`,
/// `sampleCount`, `dimension`, `format`, `usage` and `label`, where a
/// `GPUBuffer` reports three things. The extent and the mip count are the
/// caller's for that reason — a page passes numbers and reads them back off the
/// object the device made, rather than comparing a constant against itself.
///
/// [`Format::Rgba8Unorm`] because it is core WebGPU, which is what makes the
/// check runnable on the software adapter the browser gate uses: the seam's BC
/// formats are gated behind `texture-compression-bc` and its depth-stencil pair
/// behind `depth32float-stencil8`, and a probe that asked for one would be
/// testing whether *this* machine has the feature. Those paths are held by
/// `web/tools/gpu-replay.mjs`, which drives the table format by format.
///
/// The usage is two flags that map onto two different `GPUTextureUsage` bits, so
/// a translation that dropped one or or-ed the wrong constant produces a number
/// the page can see is wrong — [`probe_buffer_desc`]'s argument, unchanged.
#[must_use]
pub const fn probe_image_desc(width: u32, height: u32, mip_levels: u32) -> ImageDesc<'static> {
    ImageDesc {
        label: Some("crcbl-webgpu probe image"),
        image_type: ImageType::D2,
        extent: Extent3d::d2(width, height),
        format: Format::Rgba8Unorm,
        mip_levels,
        samples: 1,
        usage: ImageUsage::SAMPLED.union(ImageUsage::TRANSFER_DST),
    }
}

/// The descriptor [`shim::__crcbl_web_gpu_probe_image_view`] asks with.
///
/// A `const` rather than a function because it takes nothing: the image is
/// [`PROBE_IMAGE`] and every other field is fixed, and the one field worth
/// choosing is chosen already.
///
/// **THAT FIELD IS THE RANGE, AND IT IS [`ImageSubresourceRange::all`].** Both
/// counts are therefore [`ImageSubresourceRange::ALL`] — `u32::MAX`, which
/// crosses the wire verbatim by the sentinel rule in
/// `docs/plan/41-webgpu-stream.md` and which **only the replayer can resolve**.
/// WebGPU spells "the rest" as an absent descriptor member and refuses
/// `4294967295` outright, so a replayer that passed the number on produces a
/// view the browser rejects — and this probe is what puts that path in front of
/// a real browser rather than a stub. The format is the image's own, so the view
/// reinterprets nothing: `ImageDesc` has no `view_formats` for WebGPU's
/// `GPUTextureDescriptor.viewFormats`, so a view that changed format would be
/// refused by the browser for a reason that is the seam's rather than this
/// probe's.
pub const PROBE_IMAGE_VIEW_DESC: ImageViewDesc<'static> = ImageViewDesc {
    label: Some("crcbl-webgpu probe view"),
    image: PROBE_IMAGE,
    view_type: ImageViewType::D2,
    format: Format::Rgba8Unorm,
    range: ImageSubresourceRange::all(Format::Rgba8Unorm),
};

/// The sampler [`shim::__crcbl_web_gpu_probe_sampler`] creates, every time.
///
/// The same bits a fourth time, on [`PROBE_IMAGE_VIEW`]'s terms: a handle
/// carries no kind, so a page filing four kinds under one key would be a
/// replayer with one table where the crate docs require four.
pub const PROBE_SAMPLER: SamplerHandle = match SamplerHandle::from_bits(1 << 32) {
    Some(sampler) => sampler,
    // Generation `1`, as above.
    None => panic!("generation 1 is not zero"),
};

/// The descriptor [`shim::__crcbl_web_gpu_probe_sampler`] asks with.
///
/// A `const` rather than a function because there is nothing for a caller to
/// pass: **a `GPUSampler` reports its `label` and nothing else**, so unlike
/// [`probe_image_desc`] there is no number a page could hand in and read back
/// off the object the device made. Every field is chosen here instead, and each
/// is chosen for what a browser can *refuse*.
///
/// **`lod_max` is [`f32::MAX`], which is the sentinel.** It is
/// [`SamplerDesc::default`]'s "no limit"; it crosses the wire verbatim by the
/// rule `docs/plan/41-webgpu-stream.md` sets, and the replayer has to hand it to
/// WebGPU as an explicit `lodMaxClamp` — omitting the member substitutes
/// WebGPU's own default, which is a number rather than "the rest". This probe is
/// what puts that in front of a real `createSampler`.
///
/// **The three address modes are three different ones**, so all three
/// `GPUAddressMode` spellings the replayer knows are sent at once and a
/// translation that wrote a string WebGPU does not have is refused by the
/// browser. [`SamplerAddressMode::ClampToBorder`] is deliberately absent: WebGPU
/// has no border colour, so the replayer refuses it, and a probe that asked for
/// it would be testing the refusal rather than the creation.
/// `web/tools/gpu-replay.mjs` drives that path against a stub, through a command
/// the corpus really carries.
///
/// **The filters are all [`FilterMode::Linear`] and the anisotropy is `1.0`.**
/// WebGPU ties a `maxAnisotropy` above `1` to all three filters being `'linear'`
/// — so the pairing here is the one combination that is valid whichever way the
/// replayer's rule for anisotropy falls, which keeps this probe about the
/// sentinel rather than about that rule.
///
/// **`compare` is [`CompareOp::Greater`]**, which makes this a comparison
/// sampler — hardware PCF, and the op the engine's reversed-Z shadow test
/// actually wants. `None` would exercise an absent member; a present one
/// exercises the presence byte, the code table, and the `GPUCompareFunction`
/// spelling all at once, and it is the value a wrong table would silently turn
/// into its opposite.
pub const PROBE_SAMPLER_DESC: SamplerDesc<'static> = SamplerDesc {
    label: Some("crcbl-webgpu probe sampler"),
    mag_filter: FilterMode::Linear,
    min_filter: FilterMode::Linear,
    mip_filter: FilterMode::Linear,
    address_mode: [
        SamplerAddressMode::ClampToEdge,
        SamplerAddressMode::Repeat,
        SamplerAddressMode::MirrorRepeat,
    ],
    lod_min: 0.0,
    lod_max: f32::MAX,
    anisotropy: 1.0,
    compare: Some(CompareOp::Greater),
};

/// The layout [`shim::__crcbl_web_gpu_probe_bind_group_layout`] creates, every
/// time.
///
/// The same bits a fifth time, on [`PROBE_SAMPLER`]'s terms: a handle carries no
/// kind, so a page filing five kinds under one key would be a replayer with one
/// table where the crate docs require five.
pub const PROBE_BIND_GROUP_LAYOUT: BindGroupLayoutHandle =
    match BindGroupLayoutHandle::from_bits(1 << 32) {
        Some(layout) => layout,
        // Generation `1`, as above.
        None => panic!("generation 1 is not zero"),
    };

/// The binding slots [`PROBE_BIND_GROUP_LAYOUT_DESC`] declares.
///
/// **Four entries, because one proves nothing about a counted list.** This is
/// the first command on the stream carrying a counted list of *structs*, and a
/// single-entry layout decodes identically whether the reader advances by an
/// entry or by a guess — every field after the first would simply be the end of
/// the command. Four entries means a stride that is wrong by a byte lands inside
/// the next entry and produces a layout the browser refuses.
///
/// **Every one of them is a kind WebGPU can express, and between them they cover
/// four of `GPUBindGroupLayoutEntry`'s five members.** `buffer` twice, with both
/// of its `type`s that this seam can reach and `hasDynamicOffset` both ways;
/// `texture`, whose `sampleType` and `viewDimension` are two more tables;
/// `sampler`, whose `type` is the comparison flag. [`BindingKind::StorageImage`]
/// is deliberately absent — `GPUStorageTextureBindingLayout.format` is a
/// required member and the seam's variant carries no format, so a probe naming
/// one would be testing the refusal rather than the creation.
/// `web/tools/gpu-replay.mjs` drives that path against a stub, through a command
/// the corpus really carries.
///
/// **Every `count` is `1` and no entry sets a [`BindingFlags`]**, for the same
/// reason turned around: WebGPU has no binding arrays at all — a
/// `GPUBindGroupLayoutEntry` has no `count` member — so anything else here is a
/// layout the replayer must refuse rather than one a browser can accept. The
/// bindless declaration is in the corpus instead, where the refusal is what is
/// being observed.
///
/// **Three different [`ShaderStages`], and one entry naming two at once**, so
/// the `GPUShaderStage` mapping is exercised bit by bit rather than through one
/// value that could stand for any of them.
pub const PROBE_BIND_GROUP_LAYOUT_ENTRIES: [BindGroupLayoutEntry; 4] = [
    // The engine's own geometry binding: vertex pulling reads its streams out of
    // a read-only storage buffer, which is why this is the first slot of the
    // first layout this seam ever builds.
    BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::VERTEX,
        kind: BindingKind::StorageBuffer {
            read_only: true,
            dynamic: false,
        },
        count: 1,
        flags: BindingFlags::empty(),
    },
    // The substitute for push constants, which WebGPU has none of: per-draw data
    // as a dynamic offset into one uniform buffer. `dynamic: true` is therefore
    // the interesting value rather than an arbitrary one.
    BindGroupLayoutEntry {
        binding: 1,
        visibility: ShaderStages::VERTEX.union(ShaderStages::FRAGMENT),
        kind: BindingKind::UniformBuffer { dynamic: true },
        count: 1,
        flags: BindingFlags::empty(),
    },
    BindGroupLayoutEntry {
        binding: 2,
        visibility: ShaderStages::FRAGMENT,
        kind: BindingKind::SampledImage {
            view_type: ImageViewType::D2,
            sample_type: SampleType::Float,
        },
        count: 1,
        flags: BindingFlags::empty(),
    },
    BindGroupLayoutEntry {
        binding: 3,
        visibility: ShaderStages::COMPUTE,
        kind: BindingKind::Sampler { comparison: false },
        count: 1,
        flags: BindingFlags::empty(),
    },
];

/// The descriptor [`shim::__crcbl_web_gpu_probe_bind_group_layout`] asks with.
///
/// A `const` rather than a function for [`PROBE_IMAGE_VIEW_DESC`]'s reason:
/// there is nothing for a caller to pass. **A `GPUBindGroupLayout` reports its
/// `label` and nothing else** — no entries, no bindings, no visibility — so this
/// is [`PROBE_SAMPLER_DESC`]'s situation exactly, and what a browser can be asked
/// is the same two things: that the object is an instance of this browser's own
/// `GPUBindGroupLayout`, and that the device reported nothing about the
/// descriptor afterwards.
///
/// That second half is what the entries are chosen for; see
/// [`PROBE_BIND_GROUP_LAYOUT_ENTRIES`].
pub const PROBE_BIND_GROUP_LAYOUT_DESC: BindGroupLayoutDesc<'static> = BindGroupLayoutDesc {
    label: Some("crcbl-webgpu probe layout"),
    entries: &PROBE_BIND_GROUP_LAYOUT_ENTRIES,
};

/// The bind group [`shim::__crcbl_web_gpu_probe_bind_group`] creates, every time.
///
/// The same bits a sixth time, on [`PROBE_BIND_GROUP_LAYOUT`]'s terms: a handle
/// carries no kind, so a page filing six kinds under one key would be a replayer
/// with one table where the crate docs require six — and this is the kind whose
/// entries make the point concrete, since each of them *also* names a handle that
/// only its discriminant says the table for.
pub const PROBE_BIND_GROUP: BindGroupHandle = match BindGroupHandle::from_bits(1 << 32) {
    Some(group) => group,
    // Generation `1`, as above.
    None => panic!("generation 1 is not zero"),
};

/// The binding slots of the layout the probe's bind group is built against.
///
/// **Not [`PROBE_BIND_GROUP_LAYOUT_ENTRIES`]**, and the difference is what the
/// two commands are for. Those four are chosen to exercise the *layout* command's
/// members, and a bind group filling them would need a dynamic-offset uniform
/// buffer and a *filtering* sampler this observation point has no resources for.
/// These three match the three resources the group binds — one of each
/// [`BindingResource`] shape — so the group is a valid one a browser can build
/// rather than one it must refuse. The sampler slot is a *comparison* one because
/// [`PROBE_SAMPLER`] is a comparison sampler ([`PROBE_SAMPLER_DESC`]'s
/// `compare` is [`CompareOp::Greater`]); binding a comparison `GPUSampler` to a
/// comparison slot is what a browser accepts, and the two bindings are validated
/// independently of the float texture beside them.
pub const PROBE_GROUP_LAYOUT_ENTRIES: [BindGroupLayoutEntry; 3] = [
    // binding 0 <- the whole of PROBE_BUFFER, a read-only storage buffer.
    BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::COMPUTE,
        kind: BindingKind::StorageBuffer {
            read_only: true,
            dynamic: false,
        },
        count: 1,
        flags: BindingFlags::empty(),
    },
    // binding 1 <- PROBE_IMAGE_VIEW, a float 2D sampled image.
    BindGroupLayoutEntry {
        binding: 1,
        visibility: ShaderStages::FRAGMENT,
        kind: BindingKind::SampledImage {
            view_type: ImageViewType::D2,
            sample_type: SampleType::Float,
        },
        count: 1,
        flags: BindingFlags::empty(),
    },
    // binding 2 <- PROBE_SAMPLER, a comparison sampler.
    BindGroupLayoutEntry {
        binding: 2,
        visibility: ShaderStages::FRAGMENT,
        kind: BindingKind::Sampler { comparison: true },
        count: 1,
        flags: BindingFlags::empty(),
    },
];

/// The layout the probe's bind group conforms to.
///
/// Built at [`PROBE_BIND_GROUP_LAYOUT`] in the same frame as the group, just
/// before it — a bind group names a live layout, so the layout has to exist first,
/// which is why this probe records several commands where every other records
/// one. See [`shim::__crcbl_web_gpu_probe_bind_group`].
pub const PROBE_GROUP_LAYOUT_DESC: BindGroupLayoutDesc<'static> = BindGroupLayoutDesc {
    label: Some("crcbl-webgpu probe group layout"),
    entries: &PROBE_GROUP_LAYOUT_ENTRIES,
};

/// The assignments [`PROBE_BIND_GROUP_DESC`] carries — one per resource shape.
///
/// **This is what puts all three resource tables in front of a browser at once.**
/// binding 0 is a [`BindingResource::Buffer`] into the buffer table, binding 1 an
/// [`BindingResource::ImageView`] into the image-view table, binding 2 a
/// [`BindingResource::Sampler`] into the sampler table — and every one of the
/// three handles is the same eight bytes as the others, so a replayer that
/// resolved them against one table would bind the wrong kind of object.
///
/// binding 0's `size` is [`BindingResource::WHOLE_BUFFER`], the sentinel: it
/// crosses verbatim and the replayer has to hand WebGPU an *absent*
/// `GPUBufferBinding.size`, which means "to the end". This probe is what puts
/// that resolution in front of a real `createBindGroup`.
pub const PROBE_BIND_GROUP_ENTRIES: [BindGroupEntry; 3] = [
    BindGroupEntry {
        binding: 0,
        array_index: 0,
        resource: BindingResource::whole_buffer(PROBE_BUFFER),
    },
    BindGroupEntry {
        binding: 1,
        array_index: 0,
        resource: BindingResource::ImageView(PROBE_IMAGE_VIEW),
    },
    BindGroupEntry {
        binding: 2,
        array_index: 0,
        resource: BindingResource::Sampler(PROBE_SAMPLER),
    },
];

/// The descriptor [`shim::__crcbl_web_gpu_probe_bind_group`] asks with.
///
/// A `const` rather than a function for [`PROBE_IMAGE_VIEW_DESC`]'s reason: there
/// is nothing for a caller to pass. **A `GPUBindGroup` reports its `label` and
/// nothing else** — not its layout, not its entries — so this is
/// [`PROBE_BIND_GROUP_LAYOUT_DESC`]'s situation exactly, and what a browser can be
/// asked is the same two things: that the object is an instance of this browser's
/// own `GPUBindGroup`, and that the device reported nothing about the descriptor
/// afterwards.
///
/// `variable_count` is `None`: a `Some` could only pair with a layout carrying a
/// `VARIABLE_COUNT` binding, which the replayer refuses at layout creation, so it
/// is the refusal case the corpus drives rather than the creation this observes.
pub const PROBE_BIND_GROUP_DESC: BindGroupDesc<'static> = BindGroupDesc {
    label: Some("crcbl-webgpu probe bind group"),
    layout: PROBE_BIND_GROUP_LAYOUT,
    entries: &PROBE_BIND_GROUP_ENTRIES,
    variable_count: None,
};

/// The shader module [`shim::__crcbl_web_gpu_probe_shader_module`] creates, every
/// time.
///
/// The same bits a seventh time, on [`PROBE_BIND_GROUP`]'s terms: a handle
/// carries no kind, so a page filing seven kinds under one key would be a
/// replayer with one table where the crate docs require seven.
pub const PROBE_SHADER_MODULE: ShaderModuleHandle = match ShaderModuleHandle::from_bits(1 << 32) {
    Some(module) => module,
    // Generation `1`, as above.
    None => panic!("generation 1 is not zero"),
};

/// A trivial vertex entry point — valid WGSL a software adapter compiles.
///
/// The probe is what proves a real `GPUShaderModule` comes back from a real WGSL
/// string, so the source has to be one `createShaderModule` accepts *and*
/// `getCompilationInfo()` reports no error for. A bare vertex entry that writes a
/// clip-space position is the smallest such thing SwiftShader will take —
/// smaller than a fragment shader, which would need a colour target to be worth
/// anything, and unambiguously non-empty, unlike `""` (which is also valid but
/// says nothing about whether a compiler ran).
pub const PROBE_SHADER_MODULE_WGSL: &str =
    "@vertex fn main() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }";

/// The descriptor [`shim::__crcbl_web_gpu_probe_shader_module`] asks with.
///
/// A `const` rather than a function for [`PROBE_IMAGE_VIEW_DESC`]'s reason: there
/// is nothing for a caller to pass. **Only [`wgsl`](ShaderModuleDesc::wgsl) is
/// filled**, because a browser consumes only WGSL — the other three artifacts are
/// absent (`spirv` empty, `msl` `None`, `dxil` empty), which the seam's four
/// distinct absence conventions each spell differently and each cross verbatim.
/// A `GPUShaderModule` reports its `label`, so that is one piece of evidence; the
/// stronger piece is that compilation happened here, so the gate reads
/// `getCompilationInfo()` and holds it to no errors — see
/// `web/tools/browser-e2e.mjs`.
pub const PROBE_SHADER_MODULE_DESC: ShaderModuleDesc<'static> = ShaderModuleDesc {
    label: Some("crcbl-webgpu probe shader"),
    spirv: &[],
    wgsl: Some(PROBE_SHADER_MODULE_WGSL),
    msl: None,
    dxil: &[],
};

/// One surface-capability query, from the frame that asked to the frame that
/// was answered.
///
/// [`AdapterProbe`] and [`DeviceProbe`] for the third answered command, and
/// private for the difference between them and it: those two are the seam's own
/// state machines and live beside the trait methods they will implement, while
/// nothing outside this module holds one of these. The `impl Instance` that
/// eventually needs one is where [`AdapterProbe`] is — this module is an
/// observation point and goes when that arrives.
///
/// **Not [`Eq`]**, because [`Answered`](Self::Answered) holds a
/// [`SurfaceCaps`] and that is not.
#[derive(Clone, Debug, Default, PartialEq)]
enum SurfaceCapsProbe {
    /// Nothing has been asked, or the channel had no room for it.
    #[default]
    Unasked,
    /// The command is on the stream and its answer has not arrived.
    Waiting {
        /// Sequence of the [`SurfaceCaps`](crate::Command::SurfaceCaps)
        /// command, which is what the reply will name.
        sequence: u64,
    },
    /// The surface said what it will accept.
    Answered {
        /// The whole record, as it crossed.
        caps: SurfaceCaps,
    },
    /// The query answered nothing, which on this seam is an answer.
    Refused {
        /// What the browser said, or what the replayer refused to ask. For a
        /// log or a banner; never a code to branch on.
        reason: String,
        /// Which failure, and the half of this that *is* for branching on.
        cause: SurfaceCapsFailure,
    },
}

impl SurfaceCapsProbe {
    /// Ask what a canvas surface will accept, on this frame's stream.
    ///
    /// `None` when the channel would not take the query — a full waiting set, or
    /// a buffer already borrowed — and nothing is encoded then, for
    /// [`AdapterProbe::request`]'s reason: a command whose sequence nothing
    /// waits on turns the frame's *entire* reply buffer into a
    /// [`DecodeError::UnexpectedSequence`](crate::DecodeError::UnexpectedSequence).
    fn request(channel: &StreamChannel) -> Option<Self> {
        let sequence = channel.encode_awaited(StreamWriter::surface_caps)?;
        Some(Self::Waiting { sequence })
    }

    /// The sequence this is waiting on, or `None` if it is not waiting.
    const fn sequence(&self) -> Option<u64> {
        match self {
            Self::Waiting { sequence } => Some(*sequence),
            _ => None,
        }
    }

    /// Take this probe's answer out of a drained frame's replies, if it is
    /// there.
    ///
    /// `true` when this call settled the probe. Everything not naming this
    /// probe's sequence is left alone rather than consumed —
    /// [`AdapterProbe::absorb`]'s rule, and what lets all three probes read the
    /// same drained buffer.
    fn absorb(&mut self, replies: &[(u64, Reply)]) -> bool {
        let Some(waiting) = self.sequence() else {
            return false;
        };
        let Some((_, reply)) = replies.iter().find(|(sequence, _)| *sequence == waiting) else {
            return false;
        };
        *self = match reply {
            Reply::SurfaceCaps { caps } => Self::Answered { caps: caps.clone() },
            Reply::SurfaceCapsFailed { reason, cause } => Self::Refused {
                reason: reason.clone(),
                cause: *cause,
            },
            // A reply of another shape naming this sequence is a bug in the
            // replayer, and it settles rather than waits: the sequence has been
            // answered and a second answer to it is refused, so nothing else is
            // coming. `Backend` is the cause that promises nothing, which is the
            // only honest thing to say about a failure nobody wrote a branch
            // for — the same judgement `gpu-replay.js` makes at the same seam.
            other => Self::Refused {
                reason: format!(
                    "the replayer answered the capability query with {}",
                    other.name()
                ),
                cause: SurfaceCapsFailure::Backend,
            },
        };
        true
    }
}

thread_local! {
    /// The probe's own channel and its state. Thread-local for
    /// [`crate::web`]'s reason: whichever thread the engine runs on is the one
    /// the shim calls.
    static PROBE: RefCell<Probe> = const { RefCell::new(Probe::new()) };
}

/// The channel the probe installed, the two calls it is waiting on, and the
/// last thing each has to say.
#[derive(Debug)]
struct Probe {
    /// Held for as long as the probe exists, because
    /// [`install`] keeps only a [`Weak`](std::rc::Weak):
    /// dropping this is what puts every transport export back to `0`.
    channel: Option<Rc<StreamChannel>>,
    state: AdapterProbe,
    /// The adapter's name, the reason there is none, or a decode error.
    text: String,
    device: DeviceProbe,
    /// Why no device opened, or a decode error. Its own string rather than a
    /// share of [`text`](Self::text): the probes settle at different times and
    /// each export reads the text belonging to its own `state` call, so one
    /// buffer would mean whichever ran last.
    reason: String,
    caps: SurfaceCapsProbe,
    /// Why the capability query answered nothing, or a decode error. Its own
    /// string for [`reason`](Self::reason)'s reason.
    caps_reason: String,
}

impl Probe {
    const fn new() -> Self {
        Self {
            channel: None,
            state: AdapterProbe::Unasked,
            text: String::new(),
            device: DeviceProbe::Unasked,
            reason: String::new(),
            caps: SurfaceCapsProbe::Unasked,
            caps_reason: String::new(),
        }
    }

    /// Install a channel if this probe has none, and hand it back.
    ///
    /// `None` when a channel is already installed by something that is not this
    /// probe — which is the engine having grown a real backend, and the point at
    /// which this module should be deleted rather than made to share.
    fn channel(&mut self) -> Option<&Rc<StreamChannel>> {
        if self.channel.is_none() {
            let channel = Rc::new(StreamChannel::new());
            if !install(&channel) {
                return None;
            }
            self.channel = Some(channel);
        }
        self.channel.as_ref()
    }

    /// Encode one enumeration and register its wait.
    fn request(&mut self) -> bool {
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(state) = AdapterProbe::request(channel) else {
            return false;
        };
        self.state = state;
        self.text.clear();
        true
    }

    /// Encode one device request for the adapter that was granted.
    ///
    /// `false` when nothing has been granted yet, which is an ordering rule
    /// rather than a failure: [`DeviceDesc::adapter`](crcbl_hal::DeviceDesc)
    /// names an adapter from an enumeration, so there has to have been one.
    fn request_device(&mut self) -> bool {
        let Some(adapter) = self.granted().map(|info| info.id) else {
            return false;
        };
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(state) = DeviceProbe::request(channel, &probe_device_desc(adapter)) else {
            return false;
        };
        self.device = state;
        self.reason.clear();
        true
    }

    /// Encode one [`CreateSurface`](crate::Command::CreateSurface) against
    /// [`PROBE_SURFACE`], naming the canvas `canvas_id` is the page's key for.
    ///
    /// [`encode`](StreamChannel::encode) and never
    /// [`encode_awaited`](StreamChannel::encode_awaited): nothing answers this
    /// command, so a registered wait would hold a slot in a bounded set for a
    /// reply that is never coming.
    fn request_surface(&mut self, canvas_id: u32) -> bool {
        let Some(channel) = self.channel() else {
            return false;
        };
        channel
            .encode(|stream| stream.create_surface(PROBE_SURFACE, canvas_id))
            .is_some()
    }

    /// Encode one [`CreateBuffer`](crate::Command::CreateBuffer) against
    /// [`PROBE_BUFFER`], of `size` bytes.
    ///
    /// [`encode`](StreamChannel::encode) and never
    /// [`encode_awaited`](StreamChannel::encode_awaited), for
    /// [`request_surface`](Self::request_surface)'s reason: nothing answers this
    /// command either.
    ///
    /// `false` until a device has opened, which is
    /// [`request_device`](Self::request_device)'s ordering rule one step
    /// further along — `create_buffer` is a device method, and the page has no
    /// device to call it on until the request this probe made has come back.
    fn request_buffer(&mut self, size: u32) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        channel
            .encode(|stream| {
                stream.create_buffer(PROBE_BUFFER, &probe_buffer_desc(u64::from(size)))
            })
            .is_some()
    }

    /// Encode one [`CreateImage`](crate::Command::CreateImage) against
    /// [`PROBE_IMAGE`], of `width` by `height` texels with `mip_levels` levels.
    ///
    /// [`request_buffer`](Self::request_buffer)'s twin in every respect,
    /// including the ordering rule: `create_image` is a device method, so this
    /// refuses until the device request this probe made has come back.
    fn request_image(&mut self, width: u32, height: u32, mip_levels: u32) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        channel
            .encode(|stream| {
                stream.create_image(PROBE_IMAGE, &probe_image_desc(width, height, mip_levels))
            })
            .is_some()
    }

    /// Encode one [`CreateImageView`](crate::Command::CreateImageView) against
    /// [`PROBE_IMAGE_VIEW`], viewing [`PROBE_IMAGE`].
    ///
    /// **It cannot check that the image is there**, and does not pretend to: the
    /// image lives in the page's replayer and nothing on this side of the seam
    /// holds one. What this can check is the same thing its neighbour checks —
    /// that a device has opened — and the rest is the replayer's, which reports
    /// an unresolvable image handle through `Device::take_error` rather than by
    /// refusing to encode. `web/engine/gpu-replay.js` argues that where it is
    /// made.
    fn request_image_view(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        channel
            .encode(|stream| stream.create_image_view(PROBE_IMAGE_VIEW, &PROBE_IMAGE_VIEW_DESC))
            .is_some()
    }

    /// Encode one [`CreateSampler`](crate::Command::CreateSampler) against
    /// [`PROBE_SAMPLER`], with [`PROBE_SAMPLER_DESC`].
    ///
    /// [`request_image_view`](Self::request_image_view)'s twin, minus the one
    /// thing that one cannot check: a sampler names no other resource, so there
    /// is nothing here that has to already exist. The ordering rule is the same
    /// — `create_sampler` is a device method — and so is the reason a wait is
    /// not registered: nothing answers a creation.
    fn request_sampler(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        channel
            .encode(|stream| stream.create_sampler(PROBE_SAMPLER, &PROBE_SAMPLER_DESC))
            .is_some()
    }

    /// Encode one [`CreateBindGroupLayout`](crate::Command::CreateBindGroupLayout)
    /// against [`PROBE_BIND_GROUP_LAYOUT`], with
    /// [`PROBE_BIND_GROUP_LAYOUT_DESC`].
    ///
    /// [`request_sampler`](Self::request_sampler)'s twin in every structural
    /// respect — a device method, so it refuses until a device has opened; no
    /// wait registered, because nothing answers a creation; no arguments,
    /// because the descriptor is fixed and a `GPUBindGroupLayout` reports
    /// nothing a page could have chosen.
    ///
    /// **It cannot run [`BindGroupLayoutDesc::check_entries`] and does not
    /// pretend to**: that check needs a [`DeviceCaps`](crcbl_hal::DeviceCaps),
    /// which lives on the far side of this seam. The descriptor above is one it
    /// would pass on any device — no flags, every `count` one, no mesh or task
    /// visibility — so what this probe puts in front of a browser is the
    /// creation rather than the refusal.
    fn request_bind_group_layout(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        channel
            .encode(|stream| {
                stream.create_bind_group_layout(
                    PROBE_BIND_GROUP_LAYOUT,
                    &PROBE_BIND_GROUP_LAYOUT_DESC,
                )
            })
            .is_some()
    }

    /// Encode one [`CreateBindGroup`](crate::Command::CreateBindGroup) against
    /// [`PROBE_BIND_GROUP`], with [`PROBE_BIND_GROUP_DESC`].
    ///
    /// **Several commands in one frame, unlike every probe before it.** A bind
    /// group names a live layout and live resources, so this encodes the layout
    /// ([`PROBE_GROUP_LAYOUT_DESC`]), the buffer, the image, its view and the
    /// sampler the group binds, and then the group — six commands, still one
    /// export, because a creation is answered by nothing and there is no reply to
    /// poll for at any step. Reusing [`PROBE_BUFFER`], [`PROBE_IMAGE`],
    /// [`PROBE_IMAGE_VIEW`] and [`PROBE_SAMPLER`] means the frame files each
    /// resource in its own table just before the group resolves it.
    ///
    /// [`encode`](StreamChannel::encode) and never
    /// [`encode_awaited`](StreamChannel::encode_awaited), for
    /// [`request_sampler`](Self::request_sampler)'s reason: nothing answers a
    /// creation. The ordering rule is the same too — every command in the frame is
    /// a device method — so it refuses until a device has opened.
    ///
    /// **It cannot check that its resources will resolve, and does not pretend
    /// to**: they live in the page's replayer and nothing here holds one. A
    /// descriptor whose handles the browser cannot resolve, or whose entries it
    /// refuses, is reported through `Device::take_error`, exactly as an
    /// image view naming a missing image is. `web/engine/gpu-replay.js` argues
    /// that where it is made.
    fn request_bind_group(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        channel
            .encode(|stream| {
                stream.create_bind_group_layout(PROBE_BIND_GROUP_LAYOUT, &PROBE_GROUP_LAYOUT_DESC);
                stream.create_buffer(PROBE_BUFFER, &probe_buffer_desc(256));
                stream.create_image(PROBE_IMAGE, &probe_image_desc(4, 4, 1));
                stream.create_image_view(PROBE_IMAGE_VIEW, &PROBE_IMAGE_VIEW_DESC);
                stream.create_sampler(PROBE_SAMPLER, &PROBE_SAMPLER_DESC);
                stream.create_bind_group(PROBE_BIND_GROUP, &PROBE_BIND_GROUP_DESC)
            })
            .is_some()
    }

    /// Encode one [`CreateShaderModule`](crate::Command::CreateShaderModule)
    /// against [`PROBE_SHADER_MODULE`], with [`PROBE_SHADER_MODULE_DESC`].
    ///
    /// [`request_sampler`](Self::request_sampler)'s twin in every structural
    /// respect — a device method, so it refuses until a device has opened; no wait
    /// registered, because nothing answers a creation; no arguments, because the
    /// descriptor is fixed. What differs is what a browser can be asked about the
    /// result: a `GPUShaderModule` is where *compilation* happens, so the gate
    /// reads `getCompilationInfo()` off it — evidence a stub cannot fake and a
    /// clamp or a filter cannot stand in for. `crcbl.gpu.replayer.shaderModules`
    /// is the table the module lands in.
    fn request_shader_module(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        channel
            .encode(|stream| {
                stream.create_shader_module(PROBE_SHADER_MODULE, &PROBE_SHADER_MODULE_DESC)
            })
            .is_some()
    }

    /// Encode one [`SurfaceCaps`](crate::Command::SurfaceCaps).
    ///
    /// **Nothing is taken from what has already happened**, which is the
    /// difference from [`request_device`](Self::request_device): that one names
    /// an adapter and so has to wait for one, and this one names nothing at all.
    /// See the [module docs](self#the-capability-query-takes-nothing-so-there-is-one-answer-to-observe).
    fn request_surface_caps(&mut self) -> bool {
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(state) = SurfaceCapsProbe::request(channel) else {
            return false;
        };
        self.caps = state;
        self.caps_reason.clear();
        true
    }

    /// Drain what JS has committed and hand **every** probe its answer.
    ///
    /// The error, if the buffer would not decode, for the caller to report as
    /// its own probe's `*_UNDECODABLE`. One drain for all of them because there
    /// is one buffer: absorbing into only the probe that asked would drop the
    /// others' answers, and a dropped reply is a command that waits for ever.
    fn drain(&mut self) -> Option<crate::DecodeError> {
        let channel = self.channel.as_ref()?;
        // `None` is the inbox being borrowed, which nothing here can cause; it
        // reads as "no replies this frame", which is also what an ordinary
        // frame answers.
        match channel.drain_replies() {
            Some(Ok(replies)) => {
                self.state.absorb(&replies);
                self.device.absorb(&replies);
                self.caps.absorb(&replies);
                None
            }
            Some(Err(error)) => Some(error),
            None => None,
        }
    }

    /// Drain, absorb, and report where the enumeration has got to.
    fn state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.text = error.to_string();
            return PROBE_UNDECODABLE;
        }
        match &self.state {
            AdapterProbe::Unasked => PROBE_UNASKED,
            AdapterProbe::Waiting { .. } => PROBE_WAITING,
            AdapterProbe::Granted { info } => {
                self.text.clone_from(&info.name);
                PROBE_GRANTED
            }
            AdapterProbe::Refused { reason } => {
                self.text.clone_from(reason);
                PROBE_REFUSED
            }
        }
    }

    /// Drain, absorb, and report where the device request has got to.
    fn device_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.reason = error.to_string();
            return DEVICE_UNDECODABLE;
        }
        match &self.device {
            DeviceProbe::Unasked => DEVICE_UNASKED,
            DeviceProbe::Waiting { .. } => DEVICE_WAITING,
            DeviceProbe::Opened { .. } => {
                self.reason.clear();
                DEVICE_OPENED
            }
            DeviceProbe::Failed { reason, .. } => {
                self.reason.clone_from(reason);
                DEVICE_FAILED
            }
        }
    }

    /// Drain, absorb, and report where the capability query has got to.
    fn caps_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.caps_reason = error.to_string();
            return CAPS_UNDECODABLE;
        }
        match &self.caps {
            SurfaceCapsProbe::Unasked => CAPS_UNASKED,
            SurfaceCapsProbe::Waiting { .. } => CAPS_WAITING,
            SurfaceCapsProbe::Answered { .. } => {
                self.caps_reason.clear();
                CAPS_ANSWERED
            }
            SurfaceCapsProbe::Refused { reason, .. } => {
                self.caps_reason.clone_from(reason);
                CAPS_REFUSED
            }
        }
    }

    /// What the granted adapter said about itself, or `None` if none was.
    ///
    /// The numeric exports read through this rather than each reaching into the
    /// enum, so "not granted" is answered in one place instead of three.
    const fn granted(&self) -> Option<&crcbl_hal::AdapterInfo> {
        match &self.state {
            AdapterProbe::Granted { info } => Some(info),
            _ => None,
        }
    }

    /// What the opened device said about itself, or `None` if none opened.
    ///
    /// **Not [`granted`](Self::granted)'s `caps`**, and the whole reason the
    /// device has numeric exports of its own: WebGPU grants a device what was
    /// asked for, which is less than the adapter has.
    const fn opened(&self) -> Option<crcbl_hal::DeviceCaps> {
        self.device.caps()
    }

    /// What the surface said it will accept, or `None` if it has not.
    const fn accepted(&self) -> Option<&SurfaceCaps> {
        match &self.caps {
            SurfaceCapsProbe::Answered { caps } => Some(caps),
            _ => None,
        }
    }

    /// Which failure refused the capability query, or `None` if none did.
    const fn caps_failure(&self) -> Option<SurfaceCapsFailure> {
        match &self.caps {
            SurfaceCapsProbe::Refused { cause, .. } => Some(*cause),
            _ => None,
        }
    }
}

/// The JS→wasm ABI. See the [module docs](self) for the whole contract.
///
/// `#[unsafe(no_mangle)]` only on `wasm32`. None of these is `unsafe`: none
/// dereferences a pointer the caller supplied.
pub mod shim {
    use super::{CAPS_UNASKED, DEVICE_UNASKED, PROBE, PROBE_UNASKED};

    /// Ask the browser what it will grant.
    ///
    /// `1` when one enumeration is on the stream with its wait registered; `0`
    /// when there was no room, when the probe is re-entered, or when another
    /// channel is already installed.
    ///
    /// Calling it twice asks twice: each request is its own sequence, and the
    /// second one replaces the first probe's state, so the first answer arrives
    /// naming a sequence nothing is waiting for any more.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_adapters() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request()),
            Err(_) => 0,
        })
    }

    /// Drain the committed replies and report where the enumeration has got to.
    ///
    /// One of the `PROBE_*` codes. **May allocate**, so any view onto wasm
    /// memory is built after it rather than before.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.state(),
            Err(_) => PROBE_UNASKED,
        })
    }

    /// Where the text belonging to the last
    /// [`__crcbl_web_gpu_probe_state`] starts. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_text_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.text.as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How long that text is, in UTF-8 bytes. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_text_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.text.len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Ask the browser to open the adapter it granted.
    ///
    /// `1` when one device request is on the stream with its wait registered;
    /// `0` when no adapter has been granted yet, when there was no room, when
    /// the probe is re-entered, or when another channel is already installed.
    ///
    /// **The enumeration has to have been answered first.** The descriptor names
    /// an [`AdapterId`](crcbl_hal::AdapterId) from an enumeration, so there is
    /// nothing to name until one has come back — and `0` here while
    /// [`__crcbl_web_gpu_probe_state`] still answers
    /// [`PROBE_WAITING`](super::PROBE_WAITING) is that ordering, not a failure.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_device()),
            Err(_) => 0,
        })
    }

    /// Drain the committed replies and report where the device request has got
    /// to.
    ///
    /// One of the `DEVICE_*` codes. **May allocate**, on
    /// [`__crcbl_web_gpu_probe_state`]'s terms and for its reason — and, like
    /// it, this is the call that drains for *both* probes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.device_state(),
            Err(_) => DEVICE_UNASKED,
        })
    }

    /// Where the reason belonging to the last
    /// [`__crcbl_web_gpu_probe_device_state`] starts. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device_reason_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.reason.as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How long that reason is, in UTF-8 bytes. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device_reason_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.reason.len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Reads one number off the granted adapter, or `0`.
    ///
    /// **`0` is a legal value for every one of them** — an adapter may
    /// genuinely have no optional features — so it is not a failure code, and
    /// these are only meaningful once [`__crcbl_web_gpu_probe_state`] has
    /// answered [`PROBE_GRANTED`](super::PROBE_GRANTED). Allocates nothing.
    fn granted_u32(read: impl FnOnce(&crcbl_hal::AdapterInfo) -> u32) -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.granted().map_or(0, read),
            Err(_) => 0,
        })
    }

    /// Low 32 bits of the granted adapter's
    /// [`Features`](crcbl_hal::Features). `0` when nothing has been granted,
    /// which is also a legal value for it — read it only once
    /// [`__crcbl_web_gpu_probe_state`] has answered
    /// [`PROBE_GRANTED`](super::PROBE_GRANTED).
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_features_lo() -> u32 {
        granted_u32(|info| info.caps.features.bits() as u32)
    }

    /// High 32 bits of the same word, on the same terms. Split because the whole
    /// of this ABI is `(i32, …) -> i32`; see the [module docs](super).
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_features_hi() -> u32 {
        granted_u32(|info| (info.caps.features.bits() >> 32) as u32)
    }

    /// The granted adapter's
    /// [`Limits::max_image_2d`](crcbl_hal::Limits::max_image_2d) —
    /// `maxTextureDimension2D` as the browser reported it. `0` when nothing has
    /// been granted, on the terms [`__crcbl_web_gpu_probe_features_lo`] states.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_max_image_2d() -> u32 {
        granted_u32(|info| info.caps.limits.max_image_2d)
    }

    /// Ask the page to make a surface out of one of its canvases.
    ///
    /// `1` when one [`CreateSurface`](crate::Command::CreateSurface) is on the
    /// stream; `0` when the probe is re-entered, or when another channel is
    /// already installed.
    ///
    /// **THE ONE EXPORT HERE WITH NO `state` BESIDE IT.** Its two neighbours ask
    /// a question and poll for the answer; this one only tells. `create_surface`
    /// makes no round trip — see the [module
    /// docs](super#createsurface-is-one-export-and-that-is-the-commands-shape)
    /// — so `1` says the command was encoded and reached the shim's buffer, and
    /// nothing more. **Whether the page could resolve the canvas is the page's
    /// to report**, and it reports it by throwing out of the replay.
    ///
    /// `canvas_id` is a parameter rather than a constant of this module's
    /// because the value is the page's fact and not wasm's:
    /// [`SurfaceTarget::Web`](crcbl_core::SurfaceTarget) is an integer key into
    /// the shell's JS-side canvas registry, and nothing here knows what the
    /// shell registered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_surface(canvas_id: u32) -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_surface(canvas_id)),
            Err(_) => 0,
        })
    }

    /// Ask the page to make a buffer of `size` bytes on the device it opened.
    ///
    /// `1` when one [`CreateBuffer`](crate::Command::CreateBuffer) is on the
    /// stream; `0` when no device has opened yet, when the probe is re-entered,
    /// or when another channel is already installed.
    ///
    /// **No `state` beside it**, on
    /// [`__crcbl_web_gpu_probe_surface`]'s terms and for its reason: nothing
    /// answers a creation, because wasm named the handle itself. What the page
    /// got is the page's to report — `crcbl.gpu.replayer.buffers` is the table
    /// the `GPUBuffer` lands in — and what it could *not* do arrives out of band
    /// through `Device::take_error`, which is what
    /// `web/engine/gpu-replay.js` queues it into.
    ///
    /// `size` is a parameter for [`__crcbl_web_gpu_probe_surface`]'s reason
    /// turned around: the canvas id is a number only the page knows, and this is
    /// a number only the page can *check* — a browser reports `GPUBuffer.size`,
    /// so a size chosen here rather than there would be a check comparing a
    /// constant against itself.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_buffer(size: u32) -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_buffer(size)),
            Err(_) => 0,
        })
    }

    /// Ask the page to make a texture of `width` by `height` texels with
    /// `mip_levels` mip levels, on the device it opened.
    ///
    /// `1` when one [`CreateImage`](crate::Command::CreateImage) is on the
    /// stream; `0` when no device has opened yet, when the probe is re-entered,
    /// or when another channel is already installed.
    ///
    /// **No `state` beside it**, on
    /// [`__crcbl_web_gpu_probe_buffer`]'s terms and for its reason: nothing
    /// answers a creation. What the page got is the page's to report —
    /// `crcbl.gpu.replayer.images` is the table the `GPUTexture` lands in — and
    /// what it could *not* do arrives out of band through `Device::take_error`.
    ///
    /// The three numbers are parameters for that export's reason: a browser
    /// reports `GPUTexture.width`, `.height` and `.mipLevelCount` off the object
    /// it made, so numbers chosen here rather than by the page would be a check
    /// comparing a constant against itself.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_image(width: u32, height: u32, mip_levels: u32) -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_image(width, height, mip_levels)),
            Err(_) => 0,
        })
    }

    /// Ask the page to make a view of the texture
    /// [`__crcbl_web_gpu_probe_image`] created.
    ///
    /// `1` when one [`CreateImageView`](crate::Command::CreateImageView) is on
    /// the stream; `0` on that export's three conditions.
    ///
    /// **No arguments, and the descriptor is fixed**, because the one field
    /// worth varying is already the interesting one:
    /// [`PROBE_IMAGE_VIEW_DESC`](super::PROBE_IMAGE_VIEW_DESC)'s range is
    /// [`ImageSubresourceRange::all`](crcbl_hal::ImageSubresourceRange::all), so
    /// both counts cross as the `u32::MAX` sentinel and the replayer is what has
    /// to turn them into WebGPU's absent member.
    /// `crcbl.gpu.replayer.imageViews` is the table the `GPUTextureView` lands
    /// in.
    ///
    /// **The image has to have been created first** and this cannot check it:
    /// the table is the page's, so a view naming an image that is not there is
    /// reported by the replayer through `Device::take_error` rather than
    /// refused here.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_image_view() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_image_view()),
            Err(_) => 0,
        })
    }

    /// Ask the page to make a sampler on the device it opened.
    ///
    /// `1` when one [`CreateSampler`](crate::Command::CreateSampler) is on the
    /// stream; `0` on [`__crcbl_web_gpu_probe_image_view`]'s three conditions.
    ///
    /// **No `state` beside it and no arguments either**, and the second of those
    /// is not the first one's reason. There is nothing to poll for because
    /// nothing answers a creation; there is nothing to pass in because **a
    /// `GPUSampler` reports nothing but its `label`** — no filters, no address
    /// modes, no clamps — so a number chosen by the page could not be read back
    /// off the object anyway. [`PROBE_SAMPLER_DESC`](super::PROBE_SAMPLER_DESC)
    /// is chosen for what a browser can refuse instead, and its `lod_max` is the
    /// [`f32::MAX`] sentinel the replayer has to resolve.
    /// `crcbl.gpu.replayer.samplers` is the table the `GPUSampler` lands in, and
    /// a descriptor the browser would not have is reported through
    /// `Device::take_error`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_sampler() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_sampler()),
            Err(_) => 0,
        })
    }

    /// Ask the page to make a bind-group layout on the device it opened.
    ///
    /// `1` when one [`CreateBindGroupLayout`](crate::Command::CreateBindGroupLayout)
    /// is on the stream; `0` on [`__crcbl_web_gpu_probe_sampler`]'s three
    /// conditions.
    ///
    /// **No `state` beside it and no arguments either**, exactly as its
    /// neighbour has none and for the same two separate reasons: nothing answers
    /// a creation, and **a `GPUBindGroupLayout` reports its `label` and nothing
    /// else** — not its entries, not their bindings, not their visibility — so a
    /// number chosen by the page could not be read back off the object.
    /// [`PROBE_BIND_GROUP_LAYOUT_DESC`](super::PROBE_BIND_GROUP_LAYOUT_DESC) is
    /// chosen for what a browser can *refuse* instead, and it carries four
    /// entries rather than one because this is the stream's first counted list
    /// of structs. `crcbl.gpu.replayer.bindGroupLayouts` is the table the
    /// `GPUBindGroupLayout` lands in, and a layout the browser would not have is
    /// reported through `Device::take_error`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_bind_group_layout() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_bind_group_layout()),
            Err(_) => 0,
        })
    }

    /// Ask the page to make a bind group on the device it opened.
    ///
    /// `1` when the frame — a layout, the resources it names, and the
    /// [`CreateBindGroup`](crate::Command::CreateBindGroup) itself — is on the
    /// stream; `0` on [`__crcbl_web_gpu_probe_bind_group_layout`]'s three
    /// conditions.
    ///
    /// **No `state` beside it and no arguments either**, exactly as its neighbours
    /// have none and for the same two reasons: nothing answers a creation, and a
    /// `GPUBindGroup` reports its `label` and nothing else — not its layout, not
    /// its entries — so a number chosen by the page could not be read back off the
    /// object. The descriptor is fixed in `crates/crcbl-webgpu/src/probe.rs`.
    ///
    /// **What is new is that it encodes a whole *frame*.** A bind group binds a
    /// layout and resources that have to exist first, so this records six commands
    /// where its neighbours record one — and the last of them binds one handle
    /// into each of three resource tables, which is what puts the
    /// [`BindingResource`](crate::Command::CreateBindGroup) discriminant in front
    /// of a real device. `crcbl.gpu.replayer.bindGroups` is the table the
    /// `GPUBindGroup` lands in, and a descriptor the browser would not have is
    /// reported through `Device::take_error`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_bind_group() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_bind_group()),
            Err(_) => 0,
        })
    }

    /// Ask the page to make a shader module on the device it opened.
    ///
    /// `1` when one [`CreateShaderModule`](crate::Command::CreateShaderModule) is
    /// on the stream; `0` on [`__crcbl_web_gpu_probe_bind_group`]'s three
    /// conditions.
    ///
    /// **No `state` beside it and no arguments either**, exactly as its neighbours
    /// have none — nothing answers a creation, and the descriptor is fixed in
    /// `crates/crcbl-webgpu/src/probe.rs`. What is new is *why* this module is
    /// worth a group of its own: a shader module is where compilation happens, so
    /// beyond `instanceof GPUShaderModule` the gate reads `getCompilationInfo()`
    /// off the object and holds it to no errors for the known-good WGSL
    /// [`PROBE_SHADER_MODULE_DESC`](super::PROBE_SHADER_MODULE_DESC) carries.
    /// `crcbl.gpu.replayer.shaderModules` is the table the `GPUShaderModule` lands
    /// in.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_shader_module() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_shader_module()),
            Err(_) => 0,
        })
    }

    /// [`granted_u32`] for the device that opened, on the same terms: `0` is a
    /// legal value for each of these, so they are read only once
    /// [`__crcbl_web_gpu_probe_device_state`] has answered
    /// [`DEVICE_OPENED`](super::DEVICE_OPENED). Allocates nothing.
    fn opened_u32(read: impl FnOnce(crcbl_hal::DeviceCaps) -> u32) -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.opened().map_or(0, read),
            Err(_) => 0,
        })
    }

    /// Low 32 bits of the **opened device's**
    /// [`Features`](crcbl_hal::Features) — what WebGPU granted, which is what
    /// was asked for and not everything the adapter had.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device_features_lo() -> u32 {
        opened_u32(|caps| caps.features.bits() as u32)
    }

    /// High 32 bits of the same word.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device_features_hi() -> u32 {
        opened_u32(|caps| (caps.features.bits() >> 32) as u32)
    }

    /// The opened device's
    /// [`Limits::max_image_2d`](crcbl_hal::Limits::max_image_2d) — the limit the
    /// *device* was created with, which is the specification's default unless
    /// something asked for more, and therefore not the adapter's ceiling.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_device_max_image_2d() -> u32 {
        opened_u32(|caps| caps.limits.max_image_2d)
    }

    /// Ask what a canvas surface will accept.
    ///
    /// `1` when one [`SurfaceCaps`](crate::Command::SurfaceCaps) is on the
    /// stream with its wait registered; `0` when there was no room, when the
    /// probe is re-entered, or when another channel is already installed.
    ///
    /// **No arguments, because the command has no body**: the surface and the
    /// adapter the HAL call names are an `impl Instance`'s to validate, and the
    /// record depends on neither. See the [module
    /// docs](super#the-capability-query-takes-nothing-so-there-is-one-answer-to-observe).
    ///
    /// Unlike [`__crcbl_web_gpu_probe_device`] this needs no granted adapter to
    /// have arrived first, so it is legal on any frame — including one where
    /// nothing has been enumerated at all.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_surface_caps() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_surface_caps()),
            Err(_) => 0,
        })
    }

    /// Drain the committed replies and report where the capability query has got
    /// to.
    ///
    /// One of the `CAPS_*` codes. **May allocate**, on
    /// [`__crcbl_web_gpu_probe_state`]'s terms and for its reason — and, like
    /// it, this is the call that drains for *every* probe.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_surface_caps_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.caps_state(),
            Err(_) => CAPS_UNASKED,
        })
    }

    /// Where the reason belonging to the last
    /// [`__crcbl_web_gpu_probe_surface_caps_state`] starts. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_surface_caps_reason_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.caps_reason.as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How long that reason is, in UTF-8 bytes. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_surface_caps_reason_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.caps_reason.len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Which [`SurfaceCapsFailure`](crate::SurfaceCapsFailure) refused the
    /// query, as
    /// [`surface_caps_failure_code`](crate::tag::surface_caps_failure_code)
    /// spells it.
    ///
    /// **`0` is a real code** —
    /// [`Backend`](crate::SurfaceCapsFailure::Backend), the only one there is —
    /// and is also what this answers when nothing was refused, on the terms
    /// [`__crcbl_web_gpu_probe_features_lo`] states: read it only once
    /// [`__crcbl_web_gpu_probe_surface_caps_state`] has answered
    /// [`CAPS_REFUSED`](super::CAPS_REFUSED). Allocates nothing.
    ///
    /// **One cause is not one code**, which is why this export stays: the byte
    /// it reports is the wire's, so a JavaScript half still writing a retired
    /// cause reaches wasm as a decode error rather than as this answering the
    /// one value it knows.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_surface_caps_cause() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.caps_failure().map_or(0, |cause| {
                u32::from(crate::tag::surface_caps_failure_code(cause))
            }),
            Err(_) => 0,
        })
    }

    /// Reads one number off the capability record the surface answered, or `0`.
    ///
    /// [`granted_u32`]'s terms: `0` is a legal value for each of these, so it is
    /// not a failure code, and they are read only once
    /// [`__crcbl_web_gpu_probe_surface_caps_state`] has answered
    /// [`CAPS_ANSWERED`](super::CAPS_ANSWERED). Allocates nothing.
    fn accepted_u32(read: impl FnOnce(&crcbl_hal::SurfaceCaps) -> u32) -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.accepted().map_or(0, read),
            Err(_) => 0,
        })
    }

    /// The surface's
    /// [`preferred_format`](crcbl_hal::SurfaceCaps::preferred_format), as
    /// [`format_code`](crate::tag::format_code) spells it — the browser's
    /// `getPreferredCanvasFormat()`, having crossed the wire and come back.
    ///
    /// **The one number here a page can corroborate**, which is why it is the
    /// one exported; see the [module
    /// docs](super#why-the-preferred-format-and-not-the-whole-of-surfacecaps).
    /// `0` is [`FORMAT_R8_UNORM`](crate::tag::FORMAT_R8_UNORM) as well as
    /// "nothing answered", so read it only once
    /// [`__crcbl_web_gpu_probe_surface_caps_state`] has answered
    /// [`CAPS_ANSWERED`](super::CAPS_ANSWERED).
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_surface_caps_format() -> u32 {
        accepted_u32(|caps| {
            caps.preferred_format()
                .map_or(0, |format| u32::from(crate::tag::format_code(format)))
        })
    }

    /// One bit per [`PresentMode`](crcbl_hal::PresentMode) the surface offers,
    /// at `1 <<` its [`present_mode_code`](crate::tag::present_mode_code).
    ///
    /// A word rather than the list itself because what there is to check is a
    /// membership — [`SurfaceCaps`](crcbl_hal::SurfaceCaps) promises
    /// [`Fifo`](crcbl_hal::PresentMode::Fifo) is always there — and a bit per
    /// code answers that without an export per entry. Order is lost, which
    /// costs nothing: `present_modes` carries no ordering promise, unlike
    /// [`formats`](crcbl_hal::SurfaceCaps::formats).
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_surface_caps_present_modes() -> u32 {
        accepted_u32(|caps| {
            caps.present_modes.iter().fold(0, |bits, mode| {
                bits | (1u32 << crate::tag::present_mode_code(*mode))
            })
        })
    }

    /// `1` if the surface reported a
    /// [`current_extent`](crcbl_hal::SurfaceCaps::current_extent), `0` if it
    /// reported none.
    ///
    /// The presence and not the pair, because from a browser the presence is
    /// the whole fact: WebGPU has no `currentExtent` query and a canvas's own
    /// size is the page's number rather than the surface's, so `gpu-replay.js`
    /// answers `None` — and an extent appearing here is either a reader that
    /// lost its place or a replayer that started handing the shell its own
    /// request back as confirmation.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_surface_caps_has_extent() -> u32 {
        accepted_u32(|caps| u32::from(caps.current_extent.is_some()))
    }
}

#[cfg(test)]
mod tests {
    use crcbl_hal::{
        AdapterInfo, BackendKind, CompositeAlpha, DeviceCaps, DeviceType, Format, ImageAspect,
        Limits, PresentMode,
    };

    use super::shim::{
        __crcbl_web_gpu_probe_adapters, __crcbl_web_gpu_probe_bind_group,
        __crcbl_web_gpu_probe_buffer, __crcbl_web_gpu_probe_device,
        __crcbl_web_gpu_probe_device_features_hi, __crcbl_web_gpu_probe_device_features_lo,
        __crcbl_web_gpu_probe_device_max_image_2d, __crcbl_web_gpu_probe_device_reason_len,
        __crcbl_web_gpu_probe_device_reason_ptr, __crcbl_web_gpu_probe_device_state,
        __crcbl_web_gpu_probe_features_hi, __crcbl_web_gpu_probe_features_lo,
        __crcbl_web_gpu_probe_image, __crcbl_web_gpu_probe_image_view,
        __crcbl_web_gpu_probe_max_image_2d, __crcbl_web_gpu_probe_sampler,
        __crcbl_web_gpu_probe_shader_module, __crcbl_web_gpu_probe_state,
        __crcbl_web_gpu_probe_surface, __crcbl_web_gpu_probe_surface_caps,
        __crcbl_web_gpu_probe_surface_caps_cause, __crcbl_web_gpu_probe_surface_caps_format,
        __crcbl_web_gpu_probe_surface_caps_has_extent,
        __crcbl_web_gpu_probe_surface_caps_present_modes,
        __crcbl_web_gpu_probe_surface_caps_reason_len,
        __crcbl_web_gpu_probe_surface_caps_reason_ptr, __crcbl_web_gpu_probe_surface_caps_state,
        __crcbl_web_gpu_probe_text_len, __crcbl_web_gpu_probe_text_ptr,
    };
    use super::*;
    use crate::web::shim::{
        __crcbl_web_gpu_reply_buffer, __crcbl_web_gpu_reply_commit, __crcbl_web_gpu_stream_len,
        __crcbl_web_gpu_stream_ptr, __crcbl_web_gpu_stream_release,
    };
    use crate::{Command, ReplyWriter, decode_stream, tag};

    /// The text the last `state` call left, read the way JS reads it.
    fn text() -> String {
        let len = __crcbl_web_gpu_probe_text_len() as usize;
        let ptr = __crcbl_web_gpu_probe_text_ptr();
        assert!(
            !ptr.is_null(),
            "the probe answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::text`, which nothing
        // between the two calls above can have moved — neither export allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        String::from_utf8(bytes.to_vec()).expect("the probe's text is a Rust String")
    }

    /// The reason the last `device_state` call left, read the way JS reads it.
    fn device_reason() -> String {
        let len = __crcbl_web_gpu_probe_device_reason_len() as usize;
        let ptr = __crcbl_web_gpu_probe_device_reason_ptr();
        assert!(
            !ptr.is_null(),
            "the probe answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::reason`, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        String::from_utf8(bytes.to_vec()).expect("the probe's reason is a Rust String")
    }

    /// The reason the last `caps_state` call left, read the way JS reads it.
    fn caps_reason() -> String {
        let len = __crcbl_web_gpu_probe_surface_caps_reason_len() as usize;
        let ptr = __crcbl_web_gpu_probe_surface_caps_reason_ptr();
        assert!(
            !ptr.is_null(),
            "the probe answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::caps_reason`, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        String::from_utf8(bytes.to_vec()).expect("the probe's reason is a Rust String")
    }

    /// Reads the frame out through the transport exports, as the shim does, and
    /// returns what it decoded.
    fn take_frame() -> Vec<Command> {
        let len = __crcbl_web_gpu_stream_len() as usize;
        assert!(len >= tag::HEADER_BYTES, "no channel is installed");
        let ptr = __crcbl_web_gpu_stream_ptr();
        // SAFETY: the pair the two calls above just handed out, and nothing
        // encodes between them.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        let commands = decode_stream(bytes).expect("the writer's own bytes decode");
        assert_eq!(__crcbl_web_gpu_stream_release(), 1);
        commands
    }

    /// Hands `bytes` to wasm the way `putReplyStream` does.
    fn deliver(bytes: &[u8]) {
        let len = u32::try_from(bytes.len()).expect("a test buffer fits");
        let ptr = __crcbl_web_gpu_reply_buffer(len);
        assert!(!ptr.is_null(), "wasm would not take the replies");
        // SAFETY: `ptr` and `len` are what the call above just returned, and
        // nothing has called back into wasm since.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len as usize) };
        assert_eq!(__crcbl_web_gpu_reply_commit(len), 1);
    }

    /// An adapter with a feature set spanning both halves of the `u64` and a
    /// `max_image_2d` no default would produce, so the three numeric exports
    /// cannot pass by reading zero or by swapping their halves.
    fn granted(name: &str) -> AdapterInfo {
        AdapterInfo {
            id: AdapterId(0),
            name: name.into(),
            vendor_id: 0,
            device_id: 0,
            device_type: DeviceType::Other,
            driver: String::new(),
            backend: BackendKind::WebGpu,
            caps: DeviceCaps {
                // `COMPUTE` is bit 8, `RAY_QUERY` bit 24, `ACCELERATION_STRUCTURE`
                // bit 26 — all in the low word, which is why the corpus in
                // `tests/replies` carries the high-word case the enum cannot
                // reach yet.
                features: Features::COMPUTE | Features::RAY_QUERY,
                limits: Limits {
                    max_image_2d: 16384,
                    ..Limits::minimum()
                },
            },
        }
    }

    /// The whole exchange through the exports alone, which is what the browser
    /// gate does — with the replayer replaced by a `ReplyWriter`, because a
    /// `cargo test` has no `navigator.gpu` and that is the entire reason the
    /// browser gate exists.
    #[test]
    fn the_exports_carry_a_request_out_and_an_adapter_back() {
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_UNASKED);
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_WAITING);

        // The sequence is not on the wire; it is the base plus the position,
        // and this frame holds exactly one command.
        let commands = take_frame();
        assert_eq!(commands, vec![Command::EnumerateAdapters]);

        let info = granted("Cherry MX Blue GPU");
        let mut replies = ReplyWriter::new();
        replies.adapter(0, &info);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_GRANTED);
        assert_eq!(text(), "Cherry MX Blue GPU");
    }

    /// **The capabilities reach the page too, not only the name.** This is the
    /// half the browser gate corroborates against `navigator.gpu`, so the three
    /// exports have to answer the adapter that was granted rather than zeros.
    #[test]
    fn the_numeric_exports_answer_the_granted_adapters_capabilities() {
        // Nothing granted yet: the documented `0`, which is also why these are
        // read only after `state` said `GRANTED`.
        assert_eq!(__crcbl_web_gpu_probe_features_lo(), 0);
        assert_eq!(__crcbl_web_gpu_probe_features_hi(), 0);
        assert_eq!(__crcbl_web_gpu_probe_max_image_2d(), 0);

        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);
        let info = granted("capable");
        let mut replies = ReplyWriter::new();
        replies.adapter(0, &info);
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_GRANTED);

        let bits = info.caps.features.bits();
        assert_eq!(
            u64::from(__crcbl_web_gpu_probe_features_lo()),
            bits & 0xFFFF_FFFF
        );
        assert_eq!(u64::from(__crcbl_web_gpu_probe_features_hi()), bits >> 32);
        assert_eq!(
            __crcbl_web_gpu_probe_max_image_2d(),
            info.caps.limits.max_image_2d
        );
    }

    /// A refusal has no adapter, so the numbers must stay at their "nothing
    /// granted" value rather than keeping whatever a previous probe left.
    #[test]
    fn a_refusal_leaves_the_numeric_exports_at_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);
        let mut replies = ReplyWriter::new();
        replies.no_adapter(0, "no GPU here");
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_REFUSED);
        assert_eq!(__crcbl_web_gpu_probe_features_lo(), 0);
        assert_eq!(__crcbl_web_gpu_probe_features_hi(), 0);
        assert_eq!(__crcbl_web_gpu_probe_max_image_2d(), 0);
    }

    #[test]
    fn a_browser_that_grants_nothing_comes_back_as_a_refusal_with_its_reason() {
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);

        let mut replies = ReplyWriter::new();
        replies.no_adapter(0, "requestAdapter() resolved null");
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_REFUSED);
        assert_eq!(text(), "requestAdapter() resolved null");
    }

    /// The state a drifted format lands in, and it must not read as a browser
    /// without a GPU: those blame opposite halves of the build.
    #[test]
    fn a_reply_answering_a_command_nobody_asked_is_undecodable_rather_than_refused() {
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);

        let mut replies = ReplyWriter::new();
        replies.adapter(9_999, &granted("an answer to nothing"));
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_UNDECODABLE);
        assert!(text().contains("9999"), "{}", text());
    }

    /// The capabilities an opened device answers with — deliberately *less*
    /// than [`granted`]'s adapter, which is what a WebGPU device is: the
    /// features that were asked for, and the specification's default limits.
    fn device_caps() -> DeviceCaps {
        DeviceCaps {
            features: Features::COMPUTE,
            limits: Limits {
                max_image_2d: 8192,
                ..Limits::minimum()
            },
        }
    }

    /// Enumerates, grants `info`, and leaves the probe with an adapter.
    fn grant(info: &AdapterInfo) {
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);
        let mut replies = ReplyWriter::new();
        replies.adapter(0, info);
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_GRANTED);
    }

    /// The device half of the exchange, through the exports alone — the second
    /// round trip the browser gate drives.
    #[test]
    fn the_exports_carry_a_device_request_out_and_the_devices_own_capabilities_back() {
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_UNASKED);
        grant(&granted("Cherry MX Blue GPU"));

        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::RequestDevice {
                adapter: AdapterId(0),
                label: Some("crcbl-webgpu probe".into()),
                required_features: Features::COMPUTE,
                optional_features: Features::empty(),
                compatible_surface: None,
            }]
        );

        // Sequence 1: the enumeration spent 0.
        let mut replies = ReplyWriter::new();
        replies.device(1, &device_caps());
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_OPENED);
        assert_eq!(device_reason(), "");
        let bits = device_caps().features.bits();
        assert_eq!(
            u64::from(__crcbl_web_gpu_probe_device_features_lo()),
            bits & 0xFFFF_FFFF
        );
        assert_eq!(
            u64::from(__crcbl_web_gpu_probe_device_features_hi()),
            bits >> 32
        );
        assert_eq!(
            __crcbl_web_gpu_probe_device_max_image_2d(),
            device_caps().limits.max_image_2d
        );
    }

    /// **The device's numbers are not the adapter's**, and the two sets of
    /// exports must not read the same store. The corpus here is built so that
    /// every one of the three differs.
    #[test]
    fn the_device_exports_do_not_answer_with_the_adapters_capabilities() {
        let adapter = granted("capable");
        grant(&adapter);
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(take_frame().len(), 1);
        let mut replies = ReplyWriter::new();
        replies.device(1, &device_caps());
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_OPENED);

        assert_ne!(
            adapter.caps.features,
            device_caps().features,
            "the corpus would not notice a copy otherwise"
        );
        assert_ne!(
            adapter.caps.limits.max_image_2d,
            device_caps().limits.max_image_2d
        );
        assert_eq!(
            __crcbl_web_gpu_probe_features_lo(),
            adapter.caps.features.bits() as u32
        );
        assert_eq!(
            __crcbl_web_gpu_probe_device_features_lo(),
            device_caps().features.bits() as u32
        );
        assert_eq!(
            __crcbl_web_gpu_probe_max_image_2d(),
            adapter.caps.limits.max_image_2d
        );
        assert_eq!(
            __crcbl_web_gpu_probe_device_max_image_2d(),
            device_caps().limits.max_image_2d
        );
    }

    /// The descriptor names an adapter, so there has to be one. Nothing may be
    /// encoded before there is.
    #[test]
    fn a_device_request_before_an_adapter_is_granted_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_device(), 0);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_UNASKED);
        // Not even a channel: refusing before installing one is what keeps the
        // "another channel is installed" answer meaningful.
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);

        // …and it is still refused while the enumeration is in flight.
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_device(), 0);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);
    }

    /// A refusal carries what the browser said and leaves the numbers at their
    /// "nothing opened" value rather than at whatever a previous answer left.
    #[test]
    fn a_refused_device_request_reports_its_reason_and_no_capabilities() {
        grant(&granted("has an adapter, will not open it"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(take_frame().len(), 1);

        let mut replies = ReplyWriter::new();
        replies.device_failed(
            1,
            "no WebGPU feature satisfies Features(TIMELINE_SEMAPHORE)",
            Features::TIMELINE_SEMAPHORE,
        );
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_FAILED);
        assert!(
            device_reason().contains("TIMELINE_SEMAPHORE"),
            "{}",
            device_reason()
        );
        assert_eq!(__crcbl_web_gpu_probe_device_features_lo(), 0);
        assert_eq!(__crcbl_web_gpu_probe_device_features_hi(), 0);
        assert_eq!(__crcbl_web_gpu_probe_device_max_image_2d(), 0);
    }

    /// **One buffer, two probes.** Both answers arrive in the same frame and
    /// whichever export is asked first is what decodes it — so the other's
    /// answer has to have been absorbed by then rather than dropped with the
    /// buffer.
    #[test]
    fn one_drain_hands_both_probes_their_own_answer() {
        // Ask for both before either is answered, so the two replies land
        // together.
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);
        let mut replies = ReplyWriter::new();
        replies.adapter(0, &granted("both at once"));
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_GRANTED);

        // The device request first: a second enumeration puts the adapter probe
        // back to `Waiting`, and the descriptor has no adapter to name then.
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(take_frame().len(), 2);

        let mut replies = ReplyWriter::new();
        replies.device(1, &device_caps());
        replies.adapter(2, &granted("answered second"));
        deliver(replies.bytes());

        // The device is asked first, so it is the call that drains. The
        // adapter's answer must survive that.
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_OPENED);
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_GRANTED);
        assert_eq!(text(), "answered second");
    }

    /// A drifted format lands on whichever probe asked first, and must not read
    /// as a browser that refused a device.
    #[test]
    fn a_reply_answering_a_device_request_nobody_made_is_undecodable_rather_than_failed() {
        grant(&granted("adapter"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(take_frame().len(), 1);

        let mut replies = ReplyWriter::new();
        replies.device(9_999, &device_caps());
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_UNDECODABLE);
        assert!(device_reason().contains("9999"), "{}", device_reason());
    }

    /// How many sequences the probe's own channel is still waiting on.
    ///
    /// Reached through the thread-local rather than through an export, because
    /// there is no export for it: what it is here to observe is a *negative* —
    /// see the test below.
    fn waiting_replies() -> usize {
        PROBE.with(|probe| {
            probe
                .borrow()
                .channel
                .as_ref()
                .map_or(0, |channel| channel.waiting_replies())
        })
    }

    /// The surface half, which is one export and one command: the page's canvas
    /// id goes out and the handle wasm named goes with it.
    #[test]
    fn the_surface_export_encodes_one_create_surface_naming_the_canvas() {
        assert_eq!(__crcbl_web_gpu_probe_surface(7), 1);
        assert_eq!(
            take_frame(),
            vec![Command::CreateSurface {
                surface: PROBE_SURFACE,
                canvas_id: 7,
            }]
        );
    }

    /// **Nothing waits on it**, and that is the difference from its two
    /// neighbours rather than an omission: `create_surface` has no reply, so a
    /// registered wait would hold a slot in a bounded set for ever.
    #[test]
    fn the_surface_request_registers_no_wait_because_nothing_answers_it() {
        assert_eq!(__crcbl_web_gpu_probe_surface(7), 1);
        assert_eq!(waiting_replies(), 0);
        assert_eq!(take_frame().len(), 1);

        // The same channel, one command later, does register one — so the zero
        // above is this command's shape and not a counter that never moves.
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 1);
        assert_eq!(waiting_replies(), 1);
        assert_eq!(take_frame(), vec![Command::EnumerateAdapters]);
    }

    /// It needs no adapter and no device, which is what lets the browser gate
    /// drive it as its own group: `Instance::create_surface` is an instance
    /// method, and the seam lets a caller make a surface before any device
    /// exists.
    #[test]
    fn a_surface_request_needs_neither_an_adapter_nor_a_device() {
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_UNASKED);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_UNASKED);
        assert_eq!(__crcbl_web_gpu_probe_surface(3), 1);
        assert_eq!(
            take_frame(),
            vec![Command::CreateSurface {
                surface: PROBE_SURFACE,
                canvas_id: 3,
            }]
        );
    }

    /// Grants an adapter, opens a device, and leaves the probe with both.
    ///
    /// What a buffer command needs on this side: `create_buffer` is a device
    /// method, so the export refuses until the device request it made has been
    /// answered.
    fn open_device() {
        grant(&granted("has a device"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(take_frame().len(), 1);
        let mut replies = ReplyWriter::new();
        replies.device(1, &device_caps());
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_OPENED);
    }

    /// The buffer half: one export, one command, and the descriptor this module
    /// fixed — with the size the caller passed, which is the one field a browser
    /// can be held to.
    #[test]
    fn the_buffer_export_encodes_one_create_buffer_with_the_size_it_was_given() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_buffer(4096), 1);
        assert_eq!(
            take_frame(),
            vec![Command::CreateBuffer {
                buffer: PROBE_BUFFER,
                label: Some("crcbl-webgpu probe buffer".into()),
                size: 4096,
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            }]
        );
    }

    /// The size is the caller's rather than this module's, so a second call with
    /// a different one has to move it — otherwise the browser gate is comparing
    /// `GPUBuffer.size` against a constant.
    #[test]
    fn the_buffer_size_is_the_one_the_caller_asked_for() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_buffer(64), 1);
        let commands = take_frame();
        let [Command::CreateBuffer { size, .. }] = commands.as_slice() else {
            panic!("the frame carries one CreateBuffer: {commands:?}");
        };
        assert_eq!(*size, 64);
    }

    /// **Nothing waits on it**, which is the difference from the two answered
    /// commands and the same shape `create_surface` has: a creation is answered
    /// by nothing, so a registered wait would hold a slot for a reply that is
    /// never coming.
    #[test]
    fn the_buffer_request_registers_no_wait_because_nothing_answers_it() {
        open_device();
        let before = waiting_replies();
        assert_eq!(__crcbl_web_gpu_probe_buffer(64), 1);
        assert_eq!(waiting_replies(), before);
        assert_eq!(take_frame().len(), 1);
    }

    /// **A device has to have opened first**, and nothing may be encoded before
    /// one has: `Device::create_buffer` is a device method, and a page with no
    /// device has nothing to call `createBuffer` on.
    #[test]
    fn a_buffer_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_buffer(64), 0);
        // Not even a channel: refusing before installing one is what keeps the
        // "another channel is installed" answer meaningful.
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);

        // …and it is still refused while the device request is in flight.
        grant(&granted("no device yet"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_buffer(64), 0);
        assert_eq!(take_frame().len(), 1);
    }

    /// A buffer handle and a surface handle may carry identical bits, and the
    /// probe's two do — the opcode is what says which table an id indexes, so a
    /// replayer with one table for both would file the second over the first.
    #[test]
    fn the_probes_buffer_and_surface_name_the_same_handle_bits() {
        assert_eq!(PROBE_BUFFER.to_bits(), PROBE_SURFACE.to_bits());
    }

    /// And so do the image and its view, which is the pair where the sharing
    /// costs the most: a view and the image it views are alive at the same time,
    /// so a replayer with one table would overwrite the image with its own view.
    #[test]
    fn the_probes_image_and_view_name_the_same_handle_bits_as_everything_else() {
        assert_eq!(PROBE_IMAGE.to_bits(), PROBE_BUFFER.to_bits());
        assert_eq!(PROBE_IMAGE_VIEW.to_bits(), PROBE_IMAGE.to_bits());
    }

    /// The image half: one export, one command, and the descriptor this module
    /// fixed — with the three numbers the caller passed, which are the ones a
    /// browser reports back off the `GPUTexture` it made.
    #[test]
    fn the_image_export_encodes_one_create_image_with_the_extent_it_was_given() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_image(256, 128, 4), 1);
        assert_eq!(
            take_frame(),
            vec![Command::CreateImage {
                image: PROBE_IMAGE,
                label: Some("crcbl-webgpu probe image".into()),
                image_type: ImageType::D2,
                extent: Extent3d::d2(256, 128),
                format: Format::Rgba8Unorm,
                mip_levels: 4,
                samples: 1,
                usage: ImageUsage::SAMPLED | ImageUsage::TRANSFER_DST,
            }]
        );
    }

    /// The extent and the mip count are the caller's rather than this module's,
    /// so a second call with different ones has to move them — otherwise the
    /// browser gate is comparing `GPUTexture.width` against a constant.
    #[test]
    fn the_image_extent_and_mip_count_are_the_ones_the_caller_asked_for() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_image(64, 32, 2), 1);
        let commands = take_frame();
        let [
            Command::CreateImage {
                extent, mip_levels, ..
            },
        ] = commands.as_slice()
        else {
            panic!("the frame carries one CreateImage: {commands:?}");
        };
        assert_eq!(*extent, Extent3d::d2(64, 32));
        assert_eq!(*mip_levels, 2);
    }

    /// The view half, and the field this whole pair exists to put on the wire:
    /// both counts are [`ImageSubresourceRange::ALL`], which crosses verbatim
    /// and which only the replayer can resolve.
    #[test]
    fn the_view_export_encodes_one_create_image_view_carrying_the_sentinel() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_image_view(), 1);
        assert_eq!(
            take_frame(),
            vec![Command::CreateImageView {
                view: PROBE_IMAGE_VIEW,
                label: Some("crcbl-webgpu probe view".into()),
                image: PROBE_IMAGE,
                view_type: ImageViewType::D2,
                format: Format::Rgba8Unorm,
                range: ImageSubresourceRange {
                    aspect: ImageAspect::COLOR,
                    base_mip: 0,
                    mip_count: ImageSubresourceRange::ALL,
                    base_layer: 0,
                    layer_count: ImageSubresourceRange::ALL,
                },
            }]
        );
    }

    /// **Nothing waits on either**, which is the shape both creations share with
    /// the buffer and the surface: a creation is answered by nothing, so a
    /// registered wait would hold a slot for a reply that never comes.
    #[test]
    fn the_image_requests_register_no_wait_because_nothing_answers_them() {
        open_device();
        let before = waiting_replies();
        assert_eq!(__crcbl_web_gpu_probe_image(8, 8, 1), 1);
        assert_eq!(__crcbl_web_gpu_probe_image_view(), 1);
        assert_eq!(waiting_replies(), before);
        assert_eq!(take_frame().len(), 2);
    }

    /// **A device has to have opened first**, for the buffer export's reason:
    /// `Device::create_image` and `Device::create_image_view` are device
    /// methods, and a page with no device has nothing to call them on.
    #[test]
    fn an_image_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_image(8, 8, 1), 0);
        assert_eq!(__crcbl_web_gpu_probe_image_view(), 0);
        // Not even a channel: refusing before installing one is what keeps the
        // "another channel is installed" answer meaningful.
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);

        // …and both are still refused while the device request is in flight.
        grant(&granted("no device yet"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_image(8, 8, 1), 0);
        assert_eq!(__crcbl_web_gpu_probe_image_view(), 0);
        assert_eq!(take_frame().len(), 1);
    }

    /// And so does the sampler, which makes four kinds on one set of bits.
    #[test]
    fn the_probes_sampler_names_the_same_handle_bits_as_everything_else() {
        assert_eq!(PROBE_SAMPLER.to_bits(), PROBE_IMAGE_VIEW.to_bits());
    }

    /// The sampler half: one export, one command, and the descriptor this module
    /// fixed — sentinel and comparison included, because those are the two
    /// fields a browser can refuse and the two a wrong translation would change
    /// without saying so.
    #[test]
    fn the_sampler_export_encodes_one_create_sampler_carrying_the_no_limit_sentinel() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_sampler(), 1);
        assert_eq!(
            take_frame(),
            vec![Command::CreateSampler {
                sampler: PROBE_SAMPLER,
                label: Some("crcbl-webgpu probe sampler".into()),
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                mip_filter: FilterMode::Linear,
                address_mode: [
                    SamplerAddressMode::ClampToEdge,
                    SamplerAddressMode::Repeat,
                    SamplerAddressMode::MirrorRepeat,
                ],
                lod_min: 0.0,
                lod_max: f32::MAX,
                anisotropy: 1.0,
                compare: Some(CompareOp::Greater),
            }]
        );
    }

    /// The sentinel is the seam's own, not a large number this module picked:
    /// the probe's `lod_max` has to be the one `SamplerDesc::default` carries,
    /// or the gate is putting some other value in front of the browser.
    #[test]
    fn the_probe_samplers_lod_max_is_the_default_descriptors_no_limit() {
        assert_eq!(
            PROBE_SAMPLER_DESC.lod_max.to_bits(),
            SamplerDesc::default().lod_max.to_bits()
        );
        // …and the three address modes are three different ones, so all three
        // spellings the replayer knows are exercised at once.
        let [u, v, w] = PROBE_SAMPLER_DESC.address_mode;
        assert!(u != v && v != w && u != w);
    }

    /// **Nothing waits on it**, for the image pair's reason.
    #[test]
    fn the_sampler_request_registers_no_wait_because_nothing_answers_it() {
        open_device();
        let before = waiting_replies();
        assert_eq!(__crcbl_web_gpu_probe_sampler(), 1);
        assert_eq!(waiting_replies(), before);
        assert_eq!(take_frame().len(), 1);
    }

    /// **A device has to have opened first**, for the image pair's reason:
    /// `Device::create_sampler` is a device method too.
    #[test]
    fn a_sampler_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_sampler(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);

        grant(&granted("no device yet"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_sampler(), 0);
        assert_eq!(take_frame().len(), 1);
    }

    /// The shader module shares its bits with everything else, so seven kinds now
    /// stand on one index and one generation, distinguished only by the opcode.
    #[test]
    fn the_probes_shader_module_names_the_same_handle_bits_as_everything_else() {
        assert_eq!(PROBE_SHADER_MODULE.to_bits(), PROBE_BIND_GROUP.to_bits());
    }

    /// The shader-module half: one export, one command, and the descriptor this
    /// module fixed — WGSL alone, with the other three artifacts spelled absent by
    /// their own conventions, because a browser consumes only WGSL and that is the
    /// one the gate builds a real `GPUShaderModule` from.
    #[test]
    fn the_shader_module_export_encodes_one_create_shader_module_carrying_only_wgsl() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_shader_module(), 1);
        assert_eq!(
            take_frame(),
            vec![Command::CreateShaderModule {
                module: PROBE_SHADER_MODULE,
                label: Some("crcbl-webgpu probe shader".into()),
                spirv: Vec::new(),
                wgsl: Some(PROBE_SHADER_MODULE_WGSL.into()),
                msl: None,
                dxil: Vec::new(),
            }]
        );
    }

    /// **Nothing waits on it**, for the image pair's reason.
    #[test]
    fn the_shader_module_request_registers_no_wait_because_nothing_answers_it() {
        open_device();
        let before = waiting_replies();
        assert_eq!(__crcbl_web_gpu_probe_shader_module(), 1);
        assert_eq!(waiting_replies(), before);
        assert_eq!(take_frame().len(), 1);
    }

    /// **A device has to have opened first**, for the sampler export's reason:
    /// `Device::create_shader_module` is a device method too.
    #[test]
    fn a_shader_module_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_shader_module(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);

        grant(&granted("no device yet"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_shader_module(), 0);
        assert_eq!(take_frame().len(), 1);
    }

    /// The capabilities a browser answers with, as `gpu-replay.js` builds them:
    /// the preferred canvas format first, one present mode, no extent.
    fn canvas_caps(preferred: Format) -> crcbl_hal::SurfaceCaps {
        crcbl_hal::SurfaceCaps {
            formats: vec![preferred, Format::Rgba8Unorm],
            present_modes: vec![PresentMode::Fifo],
            composite_alpha: vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied],
            min_image_count: 2,
            max_image_count: 2,
            current_extent: None,
        }
    }

    /// The whole capability exchange through the exports alone, which is what
    /// the browser gate drives — and the numbers it reads are the record's, not
    /// zeros.
    #[test]
    fn the_exports_carry_a_capability_query_out_and_the_surfaces_answer_back() {
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_state(), CAPS_UNASKED);
        // Nothing answered yet: the documented `0`, which is also why these are
        // read only after `state` said `ANSWERED`.
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_format(), 0);
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_present_modes(), 0);
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_has_extent(), 0);

        assert_eq!(__crcbl_web_gpu_probe_surface_caps(), 1);
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_state(), CAPS_WAITING);
        // One command and one wait, both counted rather than merely present: a
        // second copy of either would be a reply the channel has no home for,
        // and `CAPS_WAITING` above cannot tell one query from two.
        assert_eq!(waiting_replies(), 1);
        assert_eq!(take_frame(), vec![Command::SurfaceCaps]);

        let mut replies = ReplyWriter::new();
        replies.surface_caps(0, &canvas_caps(Format::Bgra8Unorm));
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_surface_caps_state(), CAPS_ANSWERED);
        assert_eq!(caps_reason(), "");
        // `Bgra8Unorm` rather than the list's first entry by accident: neither
        // canvas format is sRGB, so `preferred_format` falls through to the
        // first, which is what the browser preferred.
        assert_eq!(
            __crcbl_web_gpu_probe_surface_caps_format(),
            u32::from(tag::format_code(Format::Bgra8Unorm))
        );
        assert_eq!(
            __crcbl_web_gpu_probe_surface_caps_present_modes(),
            1 << tag::PRESENT_MODE_FIFO
        );
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_has_extent(), 0);
    }

    /// **The format export is the record's, not a constant.** A second run with
    /// the other canvas format has to move it, or the browser gate's round-trip
    /// check is comparing something fixed against something live.
    #[test]
    fn the_format_export_follows_the_format_the_reply_carried() {
        assert_eq!(__crcbl_web_gpu_probe_surface_caps(), 1);
        assert_eq!(take_frame().len(), 1);
        let mut replies = ReplyWriter::new();
        replies.surface_caps(0, &canvas_caps(Format::Rgba8Unorm));
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_state(), CAPS_ANSWERED);
        assert_eq!(
            __crcbl_web_gpu_probe_surface_caps_format(),
            u32::from(tag::format_code(Format::Rgba8Unorm))
        );
        assert_ne!(
            tag::format_code(Format::Rgba8Unorm),
            tag::format_code(Format::Bgra8Unorm),
            "the two canvas formats must differ for this to notice anything"
        );
    }

    /// An extent that crossed is reported as present, so the gate's "a browser
    /// reports none" check is reading a field rather than a hard-wired zero.
    #[test]
    fn an_extent_that_crossed_is_reported_as_present() {
        assert_eq!(__crcbl_web_gpu_probe_surface_caps(), 1);
        assert_eq!(take_frame().len(), 1);
        let mut caps = canvas_caps(Format::Bgra8Unorm);
        caps.current_extent = Some((1280, 800));
        let mut replies = ReplyWriter::new();
        replies.surface_caps(0, &caps);
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_state(), CAPS_ANSWERED);
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_has_extent(), 1);
    }

    /// Every present mode has its own bit, so a word carrying one mode cannot
    /// read as carrying another.
    #[test]
    fn each_present_mode_lands_on_its_own_bit() {
        assert_eq!(__crcbl_web_gpu_probe_surface_caps(), 1);
        assert_eq!(take_frame().len(), 1);
        let mut caps = canvas_caps(Format::Bgra8Unorm);
        caps.present_modes = vec![PresentMode::Fifo, PresentMode::Mailbox];
        let mut replies = ReplyWriter::new();
        replies.surface_caps(0, &caps);
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_state(), CAPS_ANSWERED);
        assert_eq!(
            __crcbl_web_gpu_probe_surface_caps_present_modes(),
            (1 << tag::PRESENT_MODE_FIFO) | (1 << tag::PRESENT_MODE_MAILBOX)
        );
    }

    /// **A refusal is an answer.** It carries what the browser said and the
    /// cause a caller branches on, and it leaves the capability numbers at their
    /// "nothing answered" value rather than at whatever a previous query left.
    ///
    /// [`SurfaceCapsFailure::Backend`] is the only cause an argument-less query
    /// has, and the replayer raises it when the browser names a canvas format
    /// this seam has no [`Format`] for.
    #[test]
    fn a_refused_query_reports_its_cause_and_no_capabilities() {
        assert_eq!(__crcbl_web_gpu_probe_surface_caps(), 1);
        assert_eq!(take_frame(), vec![Command::SurfaceCaps]);

        let mut replies = ReplyWriter::new();
        replies.surface_caps_failed(
            0,
            "getPreferredCanvasFormat() answered \"rgba32float\"",
            SurfaceCapsFailure::Backend,
        );
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_surface_caps_state(), CAPS_REFUSED);
        assert!(caps_reason().contains("rgba32float"), "{}", caps_reason());
        assert_eq!(
            __crcbl_web_gpu_probe_surface_caps_cause(),
            u32::from(tag::SURFACE_CAPS_FAILURE_BACKEND)
        );
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_format(), 0);
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_present_modes(), 0);
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_has_extent(), 0);
    }

    /// The query needs neither an adapter nor a device to have been asked for,
    /// which is the difference from the device request: that one names an
    /// adapter, so it refuses until one is granted, and this one names nothing.
    #[test]
    fn a_capability_query_needs_no_granted_adapter_the_way_a_device_request_does() {
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_UNASKED);
        assert_eq!(__crcbl_web_gpu_probe_device(), 0);
        assert_eq!(__crcbl_web_gpu_probe_surface_caps(), 1);
        assert_eq!(take_frame().len(), 1);
    }

    /// A drifted format lands on whichever probe asked first, and must not read
    /// as a surface that refused.
    #[test]
    fn a_reply_answering_a_query_nobody_made_is_undecodable_rather_than_refused() {
        assert_eq!(__crcbl_web_gpu_probe_surface_caps(), 1);
        assert_eq!(take_frame().len(), 1);

        let mut replies = ReplyWriter::new();
        replies.surface_caps(9_999, &canvas_caps(Format::Bgra8Unorm));
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_surface_caps_state(), CAPS_UNDECODABLE);
        assert!(caps_reason().contains("9999"), "{}", caps_reason());
    }

    /// **One buffer, three probes.** The capability query is the third, and its
    /// answer must survive a drain another probe's `state` call performed.
    #[test]
    fn one_drain_hands_the_capability_probe_its_answer_too() {
        grant(&granted("three at once"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_surface_caps(), 1);
        assert_eq!(take_frame().len(), 2);

        // Sequence 0 was the enumeration; the device took 1 and the query 2.
        let mut replies = ReplyWriter::new();
        replies.device(1, &device_caps());
        replies.surface_caps(2, &canvas_caps(Format::Bgra8Unorm));
        deliver(replies.bytes());

        // The device is asked first, so it is the call that drains. The
        // capability answer must survive that.
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_OPENED);
        assert_eq!(__crcbl_web_gpu_probe_surface_caps_state(), CAPS_ANSWERED);
        assert_eq!(
            __crcbl_web_gpu_probe_surface_caps_format(),
            u32::from(tag::format_code(Format::Bgra8Unorm))
        );
    }

    /// And so does the bind group, which makes six kinds on one set of bits.
    #[test]
    fn the_probes_bind_group_names_the_same_handle_bits_as_everything_else() {
        assert_eq!(PROBE_BIND_GROUP.to_bits(), PROBE_SAMPLER.to_bits());
    }

    /// The bind-group half: **one export, a whole frame.** A bind group names a
    /// live layout and live resources, so the export encodes the layout, the
    /// buffer, the image, its view and the sampler before the group — and the
    /// group itself carries one handle into each of three resource tables.
    #[test]
    fn the_bind_group_export_encodes_the_layout_its_resources_and_the_group() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_bind_group(), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateBindGroupLayout",
                "CreateBuffer",
                "CreateImage",
                "CreateImageView",
                "CreateSampler",
                "CreateBindGroup",
            ],
            "the frame builds the layout and resources before the group"
        );
        assert_eq!(
            commands.last(),
            Some(&Command::CreateBindGroup {
                group: PROBE_BIND_GROUP,
                label: Some("crcbl-webgpu probe bind group".into()),
                layout: PROBE_BIND_GROUP_LAYOUT,
                entries: PROBE_BIND_GROUP_ENTRIES.to_vec(),
                variable_count: None,
            })
        );
    }

    /// **Nothing waits on the frame**, for the image pair's reason: every command
    /// in it is a creation, and a creation is answered by nothing.
    #[test]
    fn the_bind_group_request_registers_no_wait_because_nothing_answers_it() {
        open_device();
        let before = waiting_replies();
        assert_eq!(__crcbl_web_gpu_probe_bind_group(), 1);
        assert_eq!(waiting_replies(), before);
        assert_eq!(take_frame().len(), 6);
    }

    /// **A device has to have opened first**, for the sampler export's reason:
    /// every command the frame carries is a device method.
    #[test]
    fn a_bind_group_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_bind_group(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);

        grant(&granted("no device yet"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_bind_group(), 0);
        assert_eq!(take_frame().len(), 1);
    }

    /// The probe must not take a channel from an engine that has one, because
    /// replacing it would strand the frame the shim is part-way through.
    #[test]
    fn the_probe_refuses_when_another_channel_is_already_installed() {
        let engine = Rc::new(StreamChannel::new());
        assert!(install(&engine));
        assert_eq!(__crcbl_web_gpu_probe_adapters(), 0);
        assert_eq!(__crcbl_web_gpu_probe_state(), PROBE_UNASKED);
    }
}
