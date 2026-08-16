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
//! | [`__crcbl_web_gpu_probe_pipeline_layout`](shim::__crcbl_web_gpu_probe_pipeline_layout) | `() -> i32` | Encode one frame — a bind-group layout and a [`CreatePipelineLayout`](crate::Command::CreatePipelineLayout) against [`PROBE_PIPELINE_LAYOUT`] built from it. `1`, or `0` on the same three conditions. |
//! | [`__crcbl_web_gpu_probe_compute_pipeline`](shim::__crcbl_web_gpu_probe_compute_pipeline) | `() -> i32` | Encode one frame — a compute shader module, a pipeline layout, and a [`CreateComputePipeline`](crate::Command::CreateComputePipeline) against [`PROBE_COMPUTE_PIPELINE`] built from both. `1`, or `0` on the same three conditions. |
//! | [`__crcbl_web_gpu_probe_graphics_pipeline`](shim::__crcbl_web_gpu_probe_graphics_pipeline) | `() -> i32` | Encode one frame — a vertex-plus-fragment shader module, an empty pipeline layout, and a [`CreateGraphicsPipeline`](crate::Command::CreateGraphicsPipeline) against [`PROBE_GRAPHICS_PIPELINE`] built from both. `1`, or `0` on the same three conditions. |
//! | [`__crcbl_web_gpu_probe_surface_caps`](shim::__crcbl_web_gpu_probe_surface_caps) | `() -> i32` | Encode one [`SurfaceCaps`](crate::Command::SurfaceCaps) and register its wait. `1`, or `0` if there was no room or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_surface_caps_state`](shim::__crcbl_web_gpu_probe_surface_caps_state) | `() -> i32` | Drain, and answer one of the `CAPS_*` codes. |
//! | [`__crcbl_web_gpu_probe_surface_caps_reason_ptr`](shim::__crcbl_web_gpu_probe_surface_caps_reason_ptr) | `() -> i32` | Where the reason the query answered nothing starts. Empty when it answered. |
//! | [`__crcbl_web_gpu_probe_surface_caps_reason_len`](shim::__crcbl_web_gpu_probe_surface_caps_reason_len) | `() -> i32` | How long it is, in UTF-8 bytes. |
//! | [`__crcbl_web_gpu_probe_surface_caps_cause`](shim::__crcbl_web_gpu_probe_surface_caps_cause) | `() -> i32` | Which [`SurfaceCapsFailure`] refused it, as [`crate::tag::surface_caps_failure_code`] spells it. |
//! | [`__crcbl_web_gpu_probe_surface_caps_format`](shim::__crcbl_web_gpu_probe_surface_caps_format) | `() -> i32` | The surface's [`preferred_format`](SurfaceCaps::preferred_format), as [`crate::tag::format_code`] spells it. |
//! | [`__crcbl_web_gpu_probe_surface_caps_present_modes`](shim::__crcbl_web_gpu_probe_surface_caps_present_modes) | `() -> i32` | One bit per mode offered, at `1 <<` its [`crate::tag::present_mode_code`]. |
//! | [`__crcbl_web_gpu_probe_surface_caps_has_extent`](shim::__crcbl_web_gpu_probe_surface_caps_has_extent) | `() -> i32` | `1` if the surface reported a [`current_extent`](SurfaceCaps::current_extent), `0` if it reported none. |
//! | [`__crcbl_web_gpu_probe_draw`](shim::__crcbl_web_gpu_probe_draw) | `() -> i32` | Encode one frame — a red-triangle pipeline, a pass that clears to [`PROBE_DRAW_CLEAR`] then binds and draws it, the copy, and a `request_readback` against [`PROBE_DRAW_READBACK`]. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_draw_poll`](shim::__crcbl_web_gpu_probe_draw_poll) | `() -> i32` | Poll the draw's readback once. `1` when a poll is on the stream, `0` when there is nothing to poll for. |
//! | [`__crcbl_web_gpu_probe_draw_state`](shim::__crcbl_web_gpu_probe_draw_state) | `() -> i32` | Drain, and answer one of the `DRAW_*` codes. |
//! | [`__crcbl_web_gpu_probe_draw_bytes_ptr`](shim::__crcbl_web_gpu_probe_draw_bytes_ptr) | `() -> i32` | Where the drawn pixels start, once [`__crcbl_web_gpu_probe_draw_state`](shim::__crcbl_web_gpu_probe_draw_state) answers [`DRAW_READY`]. |
//! | [`__crcbl_web_gpu_probe_draw_bytes_len`](shim::__crcbl_web_gpu_probe_draw_bytes_len) | `() -> i32` | How many bytes there are, or `0` if the draw has not answered. |
//! | [`__crcbl_web_gpu_probe_compute`](shim::__crcbl_web_gpu_probe_compute) | `() -> i32` | Encode one frame — a compute pipeline that writes [`PROBE_DISPATCH_PATTERN`] into a storage buffer, a pass that binds and dispatches it, the copy to a host buffer, and a `request_readback` against [`PROBE_DISPATCH_READBACK`]. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_compute_poll`](shim::__crcbl_web_gpu_probe_compute_poll) | `() -> i32` | Poll the dispatch's readback once. `1` when a poll is on the stream, `0` when there is nothing to poll for. |
//! | [`__crcbl_web_gpu_probe_compute_state`](shim::__crcbl_web_gpu_probe_compute_state) | `() -> i32` | Drain, and answer one of the `COMPUTE_*` codes. |
//! | [`__crcbl_web_gpu_probe_compute_bytes_ptr`](shim::__crcbl_web_gpu_probe_compute_bytes_ptr) | `() -> i32` | Where the dispatched bytes start, once [`__crcbl_web_gpu_probe_compute_state`](shim::__crcbl_web_gpu_probe_compute_state) answers [`COMPUTE_READY`]. |
//! | [`__crcbl_web_gpu_probe_compute_bytes_len`](shim::__crcbl_web_gpu_probe_compute_bytes_len) | `() -> i32` | How many bytes there are, or `0` if the dispatch has not answered. |
//! | [`__crcbl_web_gpu_probe_copychain`](shim::__crcbl_web_gpu_probe_copychain) | `() -> i32` | Encode one frame — a dispatch that fills a storage buffer with [`PROBE_COPYCHAIN_PATTERN`], a buffer→image copy into a texture, an image→image copy to a second texture, an image→buffer copy out to a host buffer, and a `request_readback` against [`PROBE_COPYCHAIN_READBACK`]. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_copychain_poll`](shim::__crcbl_web_gpu_probe_copychain_poll) | `() -> i32` | Poll the copy chain's readback once. `1` when a poll is on the stream, `0` when there is nothing to poll for. |
//! | [`__crcbl_web_gpu_probe_copychain_state`](shim::__crcbl_web_gpu_probe_copychain_state) | `() -> i32` | Drain, and answer one of the `COPYCHAIN_*` codes. |
//! | [`__crcbl_web_gpu_probe_copychain_bytes_ptr`](shim::__crcbl_web_gpu_probe_copychain_bytes_ptr) | `() -> i32` | Where the copied bytes start, once [`__crcbl_web_gpu_probe_copychain_state`](shim::__crcbl_web_gpu_probe_copychain_state) answers [`COPYCHAIN_READY`]. |
//! | [`__crcbl_web_gpu_probe_copychain_bytes_len`](shim::__crcbl_web_gpu_probe_copychain_bytes_len) | `() -> i32` | How many bytes there are, or `0` if the copy chain has not answered. |
//! | [`__crcbl_web_gpu_probe_fill`](shim::__crcbl_web_gpu_probe_fill) | `() -> i32` | Encode one frame — a dispatch that fills a storage buffer with [`PROBE_FILL_PATTERN`], a zero `fill_buffer` over its first half, the copy to a host buffer, and a `request_readback` against [`PROBE_FILL_READBACK`]. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_fill_poll`](shim::__crcbl_web_gpu_probe_fill_poll) | `() -> i32` | Poll the fill probe's readback once. `1` when a poll is on the stream, `0` when there is nothing to poll for. |
//! | [`__crcbl_web_gpu_probe_fill_state`](shim::__crcbl_web_gpu_probe_fill_state) | `() -> i32` | Drain, and answer one of the `FILL_*` codes. |
//! | [`__crcbl_web_gpu_probe_fill_bytes_ptr`](shim::__crcbl_web_gpu_probe_fill_bytes_ptr) | `() -> i32` | Where the filled bytes start, once [`__crcbl_web_gpu_probe_fill_state`](shim::__crcbl_web_gpu_probe_fill_state) answers [`FILL_READY`]. |
//! | [`__crcbl_web_gpu_probe_fill_bytes_len`](shim::__crcbl_web_gpu_probe_fill_bytes_len) | `() -> i32` | How many bytes there are, or `0` if the fill probe has not answered. |
//! | [`__crcbl_web_gpu_probe_present`](shim::__crcbl_web_gpu_probe_present) | `(i32) -> i32` | Encode one frame — a surface on the canvas `canvas_id` names, a swapchain configured on it, the acquired frame, a pass that clears the acquired view to [`PROBE_PRESENT_COLOR`], the copy, submit, present, and a `request_readback` against [`PROBE_PRESENT_READBACK`]. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_present_poll`](shim::__crcbl_web_gpu_probe_present_poll) | `() -> i32` | Poll the present probe's readback once. `1` when a poll is on the stream, `0` when there is nothing to poll for. |
//! | [`__crcbl_web_gpu_probe_present_state`](shim::__crcbl_web_gpu_probe_present_state) | `() -> i32` | Drain, and answer one of the `PRESENT_*` codes. |
//! | [`__crcbl_web_gpu_probe_present_bytes_ptr`](shim::__crcbl_web_gpu_probe_present_bytes_ptr) | `() -> i32` | Where the presented bytes start, once [`__crcbl_web_gpu_probe_present_state`](shim::__crcbl_web_gpu_probe_present_state) answers [`PRESENT_READY`]. |
//! | [`__crcbl_web_gpu_probe_present_bytes_len`](shim::__crcbl_web_gpu_probe_present_bytes_len) | `() -> i32` | How many bytes there are, or `0` if the present probe has not answered. |
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
    AdapterId, Barriers, BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc,
    BindGroupLayoutEntry, BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource,
    BlendState, BufferBarrier, BufferCopy, BufferDesc, BufferHandle, BufferImageCopy, BufferUsage,
    ClearValue, ColorAttachment, ColorTargetState, ColorWrites, CommandBufferHandle,
    CommandEncoderDesc, CompareOp, CompositeAlpha, ComputePassDesc, ComputePipelineDesc,
    ComputePipelineHandle, CullMode, DepthBias, DepthStencilState, DeviceDesc, Extent3d, Features,
    FilterMode, Format, FrontFace, GraphicsPipelineDesc, GraphicsPipelineHandle, ImageAspect,
    ImageCopy, ImageDesc, ImageHandle, ImageSubresourceLayers, ImageSubresourceRange, ImageType,
    ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType, LoadOp, MemoryLocation,
    MultisampleState, Offset3d, PipelineLayoutDesc, PipelineLayoutHandle, PolygonMode, PresentInfo,
    PresentMode, PrimitiveState, PrimitiveTopology, QueueHandle, ReadbackDesc, ReadbackHandle,
    Rect2d, RenderPassDesc, ResourceState, SampleType, SamplerAddressMode, SamplerDesc,
    SamplerHandle, ShaderEntry, ShaderModuleDesc, ShaderModuleHandle, ShaderStages, StoreOp,
    SubmitInfo, SurfaceCaps, SurfaceHandle, SwapchainDesc, SwapchainHandle,
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

/// Nothing has been asked, or there is no channel to ask through.
pub const READBACK_UNASKED: u32 = 0;
/// The setup frame — clear, copy, submit, request — is on the stream, and no
/// poll has been issued yet.
pub const READBACK_REQUESTED: u32 = 1;
/// A [`poll_readback`](crate::StreamWriter::poll_readback) is out and its reply
/// has not arrived.
pub const READBACK_WAITING: u32 = 2;
/// The last poll was answered [`Pending`](crcbl_hal::ReadbackState::Pending):
/// the map has not resolved yet, so the next frame polls again.
pub const READBACK_PENDING: u32 = 3;
/// The bytes are in. [`shim::__crcbl_web_gpu_probe_readback_bytes_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_readback_bytes_len`] carry them.
pub const READBACK_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`PROBE_UNDECODABLE`]'s twin.
pub const READBACK_UNDECODABLE: u32 = 5;

/// Nothing has been asked, or there is no channel to ask through.
pub const DRAW_UNASKED: u32 = 0;
/// The setup frame — the pipeline, a clear, a bound pipeline, a draw, the copy,
/// the submit and the request — is on the stream, and no poll has been issued.
pub const DRAW_REQUESTED: u32 = 1;
/// A [`poll_readback`](crate::StreamWriter::poll_readback) is out and its reply
/// has not arrived.
pub const DRAW_WAITING: u32 = 2;
/// The last poll was answered [`Pending`](crcbl_hal::ReadbackState::Pending):
/// the map has not resolved yet, so the next frame polls again.
pub const DRAW_PENDING: u32 = 3;
/// The bytes are in. [`shim::__crcbl_web_gpu_probe_draw_bytes_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_draw_bytes_len`] carry them — one drawn texel
/// per four, which the gate checks is the draw colour and not the clear.
pub const DRAW_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`READBACK_UNDECODABLE`]'s twin.
pub const DRAW_UNDECODABLE: u32 = 5;

/// Nothing has been asked, or there is no channel to ask through.
pub const COMPUTE_UNASKED: u32 = 0;
/// The setup frame — the storage buffer, the host buffer, the pipeline's four
/// resources, a compute pass that binds and dispatches, the copy, the submit and
/// the request — is on the stream, and no poll has been issued.
pub const COMPUTE_REQUESTED: u32 = 1;
/// A [`poll_readback`](crate::StreamWriter::poll_readback) is out and its reply
/// has not arrived.
pub const COMPUTE_WAITING: u32 = 2;
/// The last poll was answered [`Pending`](crcbl_hal::ReadbackState::Pending):
/// the map has not resolved yet, so the next frame polls again.
pub const COMPUTE_PENDING: u32 = 3;
/// The bytes are in. [`shim::__crcbl_web_gpu_probe_compute_bytes_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_compute_bytes_len`] carry them — 64 `u32`s the
/// dispatch wrote, which the gate checks are all [`PROBE_DISPATCH_PATTERN`] and
/// so proves the dispatch ran.
pub const COMPUTE_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`READBACK_UNDECODABLE`]'s twin.
pub const COMPUTE_UNDECODABLE: u32 = 5;

/// Nothing has been asked, or there is no channel to ask through.
pub const COPYCHAIN_UNASKED: u32 = 0;
/// The setup frame — a dispatch that fills a storage buffer with the red
/// pattern, the buffer→image, image→image and image→buffer copies, the submit
/// and the request — is on the stream, and no poll has been issued.
pub const COPYCHAIN_REQUESTED: u32 = 1;
/// A [`poll_readback`](crate::StreamWriter::poll_readback) is out and its reply
/// has not arrived.
pub const COPYCHAIN_WAITING: u32 = 2;
/// The last poll was answered [`Pending`](crcbl_hal::ReadbackState::Pending):
/// the map has not resolved yet, so the next frame polls again.
pub const COPYCHAIN_PENDING: u32 = 3;
/// The bytes are in. [`shim::__crcbl_web_gpu_probe_copychain_bytes_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_copychain_bytes_len`] carry them — 64×64
/// `Rgba8Unorm` texels, every one [`PROBE_COPYCHAIN_PATTERN`] if both copies
/// ran, which is what proves `copyBufferToTexture` and `copyTextureToTexture`.
pub const COPYCHAIN_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`COMPUTE_UNDECODABLE`]'s twin.
pub const COPYCHAIN_UNDECODABLE: u32 = 5;

/// Nothing has been asked, or there is no channel to ask through.
pub const FILL_UNASKED: u32 = 0;
/// The setup frame — a dispatch that fills a storage buffer with the pattern, a
/// zero [`fill_buffer`](crate::StreamWriter::fill_buffer) over its first half,
/// the copy to a host buffer, the submit and the request — is on the stream,
/// and no poll has been issued.
pub const FILL_REQUESTED: u32 = 1;
/// A [`poll_readback`](crate::StreamWriter::poll_readback) is out and its reply
/// has not arrived.
pub const FILL_WAITING: u32 = 2;
/// The last poll was answered [`Pending`](crcbl_hal::ReadbackState::Pending):
/// the map has not resolved yet, so the next frame polls again.
pub const FILL_PENDING: u32 = 3;
/// The bytes are in. [`shim::__crcbl_web_gpu_probe_fill_bytes_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_fill_bytes_len`] carry them — the gate checks
/// the first half is zero (the fill ran) and the second half is still
/// [`PROBE_FILL_PATTERN`] (the fill zeroed exactly its sub-range).
pub const FILL_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`COMPUTE_UNDECODABLE`]'s twin.
pub const FILL_UNDECODABLE: u32 = 5;

/// Nothing has been asked, or there is no channel to ask through.
pub const PRESENT_UNASKED: u32 = 0;
/// The setup frame — a surface, a configured swapchain, the acquired frame, the
/// host buffer, an encoder, a render pass that clears the acquired view to red,
/// the copy, the submit, the present, and the request — is on the stream, and no
/// poll has been issued.
pub const PRESENT_REQUESTED: u32 = 1;
/// A [`poll_readback`](crate::StreamWriter::poll_readback) is out and its reply
/// has not arrived.
pub const PRESENT_WAITING: u32 = 2;
/// The last poll was answered [`Pending`](crcbl_hal::ReadbackState::Pending):
/// the map has not resolved yet, so the next frame polls again.
pub const PRESENT_PENDING: u32 = 3;
/// The bytes are in. [`shim::__crcbl_web_gpu_probe_present_bytes_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_present_bytes_len`] carry them — 64×64
/// `Rgba8Unorm` texels, every one [`PROBE_PRESENT_COLOR_BYTES`] if the real
/// canvas context path acquired, rendered and copied a frame end to end.
pub const PRESENT_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`COMPUTE_UNDECODABLE`]'s twin.
pub const PRESENT_UNDECODABLE: u32 = 5;

/// The side of the square texture the readback probe clears and reads back.
///
/// **64, chosen so the row is exactly 256 bytes** — the copy's tight rows are
/// `64 × 4` bytes for [`Format::Rgba8Unorm`], which is
/// [`COPY_BYTES_PER_ROW_ALIGNMENT`](https://www.w3.org/TR/webgpu/) aligned
/// already, so the happy path needs no padding. See
/// [`shim::__crcbl_web_gpu_probe_readback`].
pub const PROBE_READBACK_SIZE: u32 = 64;

/// The distinctive clear colour, in linear-to-8-bit-exact channels.
///
/// **Every channel is exact in 8 bits**: `0.25 → 64`, `0.5 → 128`, `0.75 → 191`,
/// `1.0 → 255`, so the bytes the gate asserts — `[64, 128, 191, 255]` — are what
/// a correct clear-and-copy produces with no rounding to argue about. A stub
/// cannot produce them: only a real clear writes the right pixels.
pub const PROBE_READBACK_CLEAR: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// The clear colour as the bytes a `Rgba8Unorm` texel holds — what the gate
/// checks every pixel against.
pub const PROBE_READBACK_CLEAR_BYTES: [u8; 4] = [64, 128, 191, 255];

/// The queue [`shim::__crcbl_web_gpu_probe_readback`] names in its command
/// encoder. The same bits as every other probe handle — a handle carries no
/// kind — and carried, not used to pick a queue: WebGPU has one implicit queue.
pub const PROBE_QUEUE: QueueHandle = match QueueHandle::from_bits(1 << 32) {
    Some(queue) => queue,
    None => panic!("generation 1 is not zero"),
};

/// The command buffer [`shim::__crcbl_web_gpu_probe_readback`] finishes its
/// encoder into. The same bits as every other probe handle.
pub const PROBE_COMMAND_BUFFER: CommandBufferHandle = match CommandBufferHandle::from_bits(1 << 32)
{
    Some(command_buffer) => command_buffer,
    None => panic!("generation 1 is not zero"),
};

/// The in-flight readback [`shim::__crcbl_web_gpu_probe_readback`] requests and
/// [`shim::__crcbl_web_gpu_probe_readback_poll`] polls. The same bits again.
pub const PROBE_READBACK: ReadbackHandle = match ReadbackHandle::from_bits(1 << 32) {
    Some(readback) => readback,
    None => panic!("generation 1 is not zero"),
};

/// The image the readback probe clears — a 64×64 [`Format::Rgba8Unorm`] colour
/// target that is also a copy source.
///
/// **Not [`probe_image_desc`]'s image**: that one is `SAMPLED | TRANSFER_DST`
/// for a texture upload, and this one is `COLOR_ATTACHMENT | TRANSFER_SRC` — it
/// is rendered into and then copied *out of*. Reusing [`PROBE_IMAGE`]'s handle
/// is fine: identity is positional, so the replayer files the latest at that id.
#[must_use]
pub const fn probe_readback_image_desc() -> ImageDesc<'static> {
    ImageDesc {
        label: Some("crcbl-webgpu readback image"),
        image_type: ImageType::D2,
        extent: Extent3d::d2(PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
        format: Format::Rgba8Unorm,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::COLOR_ATTACHMENT.union(ImageUsage::TRANSFER_SRC),
    }
}

/// The view of [`probe_readback_image_desc`]'s image the render pass clears.
pub const PROBE_READBACK_VIEW_DESC: ImageViewDesc<'static> = ImageViewDesc {
    label: Some("crcbl-webgpu readback view"),
    image: PROBE_IMAGE,
    view_type: ImageViewType::D2,
    format: Format::Rgba8Unorm,
    range: ImageSubresourceRange::all(Format::Rgba8Unorm),
};

/// The buffer the cleared pixels are copied into and read back from.
///
/// `64 * 64 * 4` bytes, [`MemoryLocation::HostReadback`] — which the replayer
/// turns into WebGPU's `MAP_READ` — and [`BufferUsage::TRANSFER_DST`] for the
/// copy. `MAP_READ | COPY_DST` is the one combination WebGPU allows a mappable
/// readback buffer, and this is it.
#[must_use]
pub const fn probe_readback_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu readback buffer"),
        size: (PROBE_READBACK_SIZE as u64) * (PROBE_READBACK_SIZE as u64) * 4,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    }
}

/// The image→buffer copy that moves the cleared pixels into the readback buffer.
///
/// **Tightly packed** (`buffer_row_length` / `buffer_image_height` both `0`), so
/// the replayer computes `64 × 4 = 256` bytes per row — already 256-aligned. The
/// whole 64×64 mip-0 slice, from the origin.
#[must_use]
pub const fn probe_readback_copy() -> BufferImageCopy {
    BufferImageCopy {
        buffer: PROBE_BUFFER,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: PROBE_IMAGE,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: Extent3d::d2(PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
    }
}

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

/// The pipeline layout [`shim::__crcbl_web_gpu_probe_pipeline_layout`] creates,
/// every time.
///
/// The same bits an eighth time, on [`PROBE_SHADER_MODULE`]'s terms: a handle
/// carries no kind, so a page filing eight kinds under one key would be a
/// replayer with one table where the crate docs require eight. Its bits are
/// [`PROBE_BIND_GROUP_LAYOUT`]'s too — and here the sharing is the *point*, as it
/// is for the image and its view: a pipeline layout and a bind-group layout are
/// alive at the same time and the pipeline layout **names** the bind-group
/// layout, so a replayer with one table would resolve the pipeline layout's own
/// id as the set it is built from.
pub const PROBE_PIPELINE_LAYOUT: PipelineLayoutHandle =
    match PipelineLayoutHandle::from_bits(1 << 32) {
        Some(layout) => layout,
        // Generation `1`, as above.
        None => panic!("generation 1 is not zero"),
    };

/// The bind-group layouts [`PROBE_PIPELINE_LAYOUT_DESC`] is built from.
///
/// One entry, [`PROBE_BIND_GROUP_LAYOUT`], which the pipeline-layout probe's
/// frame creates just before the pipeline layout — a pipeline layout names live
/// bind-group layouts, so they have to exist first. One is enough here: the
/// *order* of a longer list is pinned by the corpus, which carries a two-layout
/// pipeline layout; what this probe puts in front of a browser is a real
/// `createPipelineLayout` accepting a set it can resolve.
pub const PROBE_PIPELINE_LAYOUT_BIND_GROUP_LAYOUTS: [BindGroupLayoutHandle; 1] =
    [PROBE_BIND_GROUP_LAYOUT];

/// The descriptor [`shim::__crcbl_web_gpu_probe_pipeline_layout`] asks with.
///
/// A `const` rather than a function for [`PROBE_IMAGE_VIEW_DESC`]'s reason: there
/// is nothing for a caller to pass. **A `GPUPipelineLayout` reports its `label`
/// and nothing else** — not its bind-group layouts, not its push-constant ranges
/// (it has none) — so this is [`PROBE_BIND_GROUP_LAYOUT_DESC`]'s situation
/// exactly, and what a browser can be asked is the same two things: that the
/// object is an instance of this browser's own `GPUPipelineLayout`, and that the
/// device reported nothing about the descriptor afterwards.
///
/// **`push_constants` is `None`.** A `Some` is the "WebGPU cannot express it"
/// case the replayer refuses — WebGPU has no push constants at all — so a probe
/// naming one would be testing the refusal rather than the creation. The corpus
/// drives that refusal instead, through a pipeline layout the writer really
/// carries a `Some` on.
pub const PROBE_PIPELINE_LAYOUT_DESC: PipelineLayoutDesc<'static> = PipelineLayoutDesc {
    label: Some("crcbl-webgpu probe pipeline layout"),
    bind_group_layouts: &PROBE_PIPELINE_LAYOUT_BIND_GROUP_LAYOUTS,
    push_constants: None,
};

/// The compute pipeline [`shim::__crcbl_web_gpu_probe_compute_pipeline`] creates,
/// every time.
///
/// The same bits a ninth time, on [`PROBE_PIPELINE_LAYOUT`]'s terms: a handle
/// carries no kind, so a page filing nine kinds under one key would be a replayer
/// with one table where the crate docs require nine. Its bits are
/// [`PROBE_SHADER_MODULE`]'s and [`PROBE_PIPELINE_LAYOUT`]'s too — and here the
/// sharing is the *point*: the pipeline resolves those two ids into two
/// *different* tables in the same frame, so a replayer with one table would
/// resolve the pipeline's own id as its layout or its module.
pub const PROBE_COMPUTE_PIPELINE: ComputePipelineHandle =
    match ComputePipelineHandle::from_bits(1 << 32) {
        Some(pipeline) => pipeline,
        // Generation `1`, as above.
        None => panic!("generation 1 is not zero"),
    };

/// A trivial compute entry point — valid WGSL a software adapter compiles into a
/// real compute pipeline.
///
/// **`@workgroup_size(1)`, and that is where the real workgroup size lives.**
/// WebGPU reads the size from this attribute, not from the descriptor —
/// `GPUComputePipelineDescriptor` has no member for it — so a pipeline built from
/// this module launches `1×1×1` whatever
/// [`ComputePipelineDesc::workgroup_size`](crcbl_hal::ComputePipelineDesc::workgroup_size)
/// says. The empty body is the smallest thing SwiftShader accepts as a compute
/// stage; `fn main` is the entry point [`PROBE_COMPUTE_PIPELINE_DESC`] names.
pub const PROBE_COMPUTE_PIPELINE_WGSL: &str = "@compute @workgroup_size(1) fn main() {}";

/// The compute shader module the pipeline probe's frame creates just before the
/// pipeline.
///
/// **Its own descriptor rather than [`PROBE_SHADER_MODULE_DESC`]**, because that
/// one carries a *vertex* entry point and a compute pipeline needs a compute one.
/// WGSL alone, on [`PROBE_SHADER_MODULE_DESC`]'s terms: a browser consumes only
/// WGSL, so the other three artifacts are absent. It is filed at
/// [`PROBE_SHADER_MODULE`] — the shader-module table, one of the two the pipeline
/// then resolves against.
pub const PROBE_COMPUTE_SHADER_MODULE_DESC: ShaderModuleDesc<'static> = ShaderModuleDesc {
    label: Some("crcbl-webgpu probe compute shader"),
    spirv: &[],
    wgsl: Some(PROBE_COMPUTE_PIPELINE_WGSL),
    msl: None,
    dxil: &[],
};

/// The pipeline layout the compute pipeline probe's frame creates just before the
/// pipeline.
///
/// **Empty, unlike [`PROBE_PIPELINE_LAYOUT_DESC`]** — no bind-group layouts, no
/// push constants — so the frame need not build a bind-group layout first: the
/// compute shader binds nothing, and an empty pipeline layout is what a shader
/// with no `@group` declarations wants. It is filed at [`PROBE_PIPELINE_LAYOUT`],
/// the other of the two tables the pipeline resolves against.
pub const PROBE_COMPUTE_PIPELINE_LAYOUT_DESC: PipelineLayoutDesc<'static> = PipelineLayoutDesc {
    label: Some("crcbl-webgpu probe compute pipeline layout"),
    bind_group_layouts: &[],
    push_constants: None,
};

/// The descriptor [`shim::__crcbl_web_gpu_probe_compute_pipeline`] asks with.
///
/// A `const` rather than a function for [`PROBE_IMAGE_VIEW_DESC`]'s reason: there
/// is nothing for a caller to pass. **A `GPUComputePipeline` reports its `label`,
/// and — unlike a sampler or a layout — one thing more the gate can reach:
/// `getBindGroupLayout(0)`**, which only a genuinely-built pipeline answers,
/// because it is where the shader and the layout are bound together. So beyond
/// `instanceof GPUComputePipeline` the gate has that call and the device's silent
/// error queue.
///
/// **`layout` is [`PROBE_PIPELINE_LAYOUT`] and `compute.module` is
/// [`PROBE_SHADER_MODULE`]** — the two ids the pipeline resolves into two
/// different tables, filed by the two creations the frame records first.
/// `entry_point` is `"main"`, the entry [`PROBE_COMPUTE_PIPELINE_WGSL`] declares.
///
/// **`workgroup_size` is `[1, 1, 1]`, matching the module's
/// `@workgroup_size(1)`.** The replayer drops the field — WebGPU reads the real
/// value from the module — so the corpus carries the non-uniform case where a
/// transposition is visible; here it is chosen to agree with the shader so the
/// pipeline the browser builds is the one the descriptor describes.
pub const PROBE_COMPUTE_PIPELINE_DESC: ComputePipelineDesc<'static> = ComputePipelineDesc {
    label: Some("crcbl-webgpu probe compute pipeline"),
    layout: PROBE_PIPELINE_LAYOUT,
    compute: ShaderEntry {
        module: PROBE_SHADER_MODULE,
        entry_point: "main",
    },
    workgroup_size: [1, 1, 1],
};

/// The graphics pipeline [`shim::__crcbl_web_gpu_probe_graphics_pipeline`]
/// creates, every time.
///
/// The same bits a tenth time, on [`PROBE_COMPUTE_PIPELINE`]'s terms: a handle
/// carries no kind, so a page filing ten kinds under one key would be a replayer
/// with one table where the crate docs require ten. Its bits are
/// [`PROBE_SHADER_MODULE`]'s and [`PROBE_PIPELINE_LAYOUT`]'s too — and, as for the
/// compute pipeline, the sharing is the *point*: the pipeline resolves those two
/// into two different tables in the same frame, and the module twice more (once
/// for the vertex stage, once for the fragment), so a replayer with one table
/// would resolve the pipeline's own id as its layout or one of its stages.
pub const PROBE_GRAPHICS_PIPELINE: GraphicsPipelineHandle =
    match GraphicsPipelineHandle::from_bits(1 << 32) {
        Some(pipeline) => pipeline,
        // Generation `1`, as above.
        None => panic!("generation 1 is not zero"),
    };

/// A trivial vertex-plus-fragment WGSL module — valid WGSL a software adapter
/// compiles into a real render pipeline.
///
/// **One module with both entry points**, rather than two: it is the smaller
/// thing SwiftShader accepts and still exercises the two-lookup path the replayer
/// takes, since the pipeline names this same module for its vertex stage and its
/// fragment stage. The vertex entry writes a fixed clip-space position and the
/// fragment entry a fixed colour at location 0 — the smallest pair that builds a
/// pipeline with a colour target, and unambiguously non-empty. `vsMain` and
/// `fsMain` are the entry points [`PROBE_GRAPHICS_PIPELINE_DESC`] names.
pub const PROBE_GRAPHICS_PIPELINE_WGSL: &str = concat!(
    "@vertex fn vsMain() -> @builtin(position) vec4<f32> ",
    "{ return vec4<f32>(0.0, 0.0, 0.0, 1.0); } ",
    "@fragment fn fsMain() -> @location(0) vec4<f32> ",
    "{ return vec4<f32>(1.0, 1.0, 1.0, 1.0); }"
);

/// The shader module the graphics-pipeline probe's frame creates just before the
/// pipeline.
///
/// **Its own descriptor rather than [`PROBE_SHADER_MODULE_DESC`]**, because that
/// one carries a vertex entry alone and a raster pipeline needs a fragment entry
/// too. WGSL alone, on [`PROBE_SHADER_MODULE_DESC`]'s terms: a browser consumes
/// only WGSL, so the other three artifacts are absent. It is filed at
/// [`PROBE_SHADER_MODULE`] — the shader-module table, which the pipeline then
/// resolves against for both stages.
pub const PROBE_GRAPHICS_SHADER_MODULE_DESC: ShaderModuleDesc<'static> = ShaderModuleDesc {
    label: Some("crcbl-webgpu probe raster shader"),
    spirv: &[],
    wgsl: Some(PROBE_GRAPHICS_PIPELINE_WGSL),
    msl: None,
    dxil: &[],
};

/// The pipeline layout the graphics-pipeline probe's frame creates just before
/// the pipeline.
///
/// **Empty**, like [`PROBE_COMPUTE_PIPELINE_LAYOUT_DESC`] — no bind-group
/// layouts, no push constants — because the shaders bind nothing, and an empty
/// pipeline layout is what a shader with no `@group` declarations wants. It is
/// filed at [`PROBE_PIPELINE_LAYOUT`], one of the two tables the pipeline resolves
/// against.
pub const PROBE_GRAPHICS_PIPELINE_LAYOUT_DESC: PipelineLayoutDesc<'static> = PipelineLayoutDesc {
    label: Some("crcbl-webgpu probe raster pipeline layout"),
    bind_group_layouts: &[],
    push_constants: None,
};

/// The one colour target [`PROBE_GRAPHICS_PIPELINE_DESC`] writes to.
///
/// [`Format::Rgba8Unorm`] because it is core WebGPU, which is what makes the
/// pipeline buildable on the software adapter the browser gate uses. A `Some`
/// blend rather than opaque, so the `GPUBlendState` mapping — six factors and two
/// ops across two components — is exercised at a real `createRenderPipeline`
/// rather than only in the corpus. [`BlendState::alpha`] is a mode `Rgba8Unorm`
/// can blend.
pub const PROBE_GRAPHICS_COLOR_TARGETS: [ColorTargetState; 1] = [ColorTargetState {
    format: Format::Rgba8Unorm,
    blend: Some(BlendState::alpha()),
    write_mask: ColorWrites::ALL,
}];

/// The descriptor [`shim::__crcbl_web_gpu_probe_graphics_pipeline`] asks with.
///
/// A `const` rather than a function for [`PROBE_IMAGE_VIEW_DESC`]'s reason: there
/// is nothing for a caller to pass. **A `GPURenderPipeline` reports its `label`,
/// and — like a compute pipeline — `getBindGroupLayout(0)`**, which only a
/// genuinely-built pipeline answers, because it is where the shaders and the
/// layout are bound together. So beyond `instanceof GPURenderPipeline` the gate
/// has that call and the device's silent error queue.
///
/// **Genuinely rich, not the minimum.** `layout` is the empty
/// [`PROBE_PIPELINE_LAYOUT`]; `vertex` and `fragment` both name
/// [`PROBE_SHADER_MODULE`] with the two entry points its WGSL declares — the
/// fragment present, so this is not a depth-only pass. The primitive is a plain
/// [`PrimitiveTopology::TriangleList`] with counter-clockwise winding and no
/// culling so it builds; the depth-stencil is the engine's own reversed-Z default
/// ([`Format::D32Float`], [`CompareOp::Greater`], writes on, no stencil), which
/// exercises `depthCompare`, `depthWriteEnabled` and the depth-format mapping;
/// and the one colour target carries a `Some` blend. The multisample is
/// single-sampled so no attachment needs to be multisampled for it to build.
///
/// **`depth_clamp` is `false`.** `true` maps to `unclippedDepth`, which is
/// feature-gated and would be refused on a device that did not enable
/// `depth-clip-control`; keeping it `false` builds, and the refusal is driven
/// through `web/tools/gpu-replay.mjs` instead.
pub const PROBE_GRAPHICS_PIPELINE_DESC: GraphicsPipelineDesc<'static> = GraphicsPipelineDesc {
    label: Some("crcbl-webgpu probe raster pipeline"),
    layout: PROBE_PIPELINE_LAYOUT,
    vertex: ShaderEntry {
        module: PROBE_SHADER_MODULE,
        entry_point: "vsMain",
    },
    fragment: Some(ShaderEntry {
        module: PROBE_SHADER_MODULE,
        entry_point: "fsMain",
    }),
    primitive: PrimitiveState {
        topology: PrimitiveTopology::TriangleList,
        front_face: FrontFace::Ccw,
        cull_mode: CullMode::None,
        polygon_mode: PolygonMode::Fill,
        depth_clamp: false,
    },
    depth_stencil: Some(DepthStencilState {
        format: Format::D32Float,
        depth_write: true,
        depth_compare: CompareOp::Greater,
        stencil: None,
        bias: DepthBias {
            constant: 0.0,
            slope_scale: 0.0,
            clamp: 0.0,
        },
    }),
    multisample: MultisampleState {
        samples: 1,
        mask: !0,
        alpha_to_coverage: false,
    },
    color_targets: &PROBE_GRAPHICS_COLOR_TARGETS,
};

// The draw probe (group T): the readback probe's frame with a real pipeline
// bound and a triangle drawn between the clear and the copy, so the pixels read
// back are the fragment's colour rather than the clear's. Every handle it names
// is `2 << 32` — a generation past every other probe's `1 << 32` — so its nine
// live resources never land in another probe's slot in the shared page, the way
// the readback and graphics-pipeline probes each reuse `1 << 32` across their own.

/// The clear the draw pass loads with — the colour the draw must overwrite.
///
/// The same channels as [`PROBE_READBACK_CLEAR`], and exact in 8 bits for its
/// reason. It is decisive that this is **not** [`PROBE_DRAW_COLOR_BYTES`]: a stub
/// that binds no pipeline and draws nothing leaves these bytes in the buffer, so
/// the gate reading back the draw colour instead is what proves the draw ran.
pub const PROBE_DRAW_CLEAR: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// The colour the fragment shader writes, as the bytes a `Rgba8Unorm` texel
/// holds — opaque red, `vec4<f32>(1.0, 0.0, 0.0, 1.0)`. What the gate checks
/// every pixel against, and what only a real `setPipeline` + `draw` produces.
pub const PROBE_DRAW_COLOR_BYTES: [u8; 4] = [255, 0, 0, 255];

/// The queue the draw probe names in its command encoder. `2 << 32` — carried,
/// not used to pick a queue: WebGPU has one implicit queue.
pub const PROBE_DRAW_QUEUE: QueueHandle = match QueueHandle::from_bits(2 << 32) {
    Some(queue) => queue,
    None => panic!("generation 2 is not zero"),
};

/// The command buffer the draw probe finishes its encoder into. `2 << 32`.
pub const PROBE_DRAW_COMMAND_BUFFER: CommandBufferHandle =
    match CommandBufferHandle::from_bits(2 << 32) {
        Some(command_buffer) => command_buffer,
        None => panic!("generation 2 is not zero"),
    };

/// The in-flight readback the draw probe requests and polls. `2 << 32`.
pub const PROBE_DRAW_READBACK: ReadbackHandle = match ReadbackHandle::from_bits(2 << 32) {
    Some(readback) => readback,
    None => panic!("generation 2 is not zero"),
};

/// The image the draw probe renders into and copies out of — a 64×64
/// [`Format::Rgba8Unorm`] colour target and copy source, [`PROBE_DRAW_IMAGE`]'s
/// own descriptor so it never shares a slot with the readback probe's image.
#[must_use]
pub const fn probe_draw_image_desc() -> ImageDesc<'static> {
    ImageDesc {
        label: Some("crcbl-webgpu draw image"),
        image_type: ImageType::D2,
        extent: Extent3d::d2(PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
        format: Format::Rgba8Unorm,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::COLOR_ATTACHMENT.union(ImageUsage::TRANSFER_SRC),
    }
}

/// The image handle the draw probe renders into. `2 << 32`.
pub const PROBE_DRAW_IMAGE: ImageHandle = match ImageHandle::from_bits(2 << 32) {
    Some(image) => image,
    None => panic!("generation 2 is not zero"),
};

/// The image-view handle the draw probe's pass clears and draws into. `2 << 32`.
pub const PROBE_DRAW_IMAGE_VIEW: ImageViewHandle = match ImageViewHandle::from_bits(2 << 32) {
    Some(view) => view,
    None => panic!("generation 2 is not zero"),
};

/// The view of [`probe_draw_image_desc`]'s image the draw pass renders into.
pub const PROBE_DRAW_VIEW_DESC: ImageViewDesc<'static> = ImageViewDesc {
    label: Some("crcbl-webgpu draw view"),
    image: PROBE_DRAW_IMAGE,
    view_type: ImageViewType::D2,
    format: Format::Rgba8Unorm,
    range: ImageSubresourceRange::all(Format::Rgba8Unorm),
};

/// The buffer handle the drawn pixels are copied into and read back from.
/// `2 << 32`.
pub const PROBE_DRAW_BUFFER: BufferHandle = match BufferHandle::from_bits(2 << 32) {
    Some(buffer) => buffer,
    None => panic!("generation 2 is not zero"),
};

/// The buffer the drawn pixels are copied into and read back from — the readback
/// buffer's shape (`64 * 64 * 4` bytes, [`MemoryLocation::HostReadback`],
/// [`BufferUsage::TRANSFER_DST`]) under [`PROBE_DRAW_BUFFER`].
#[must_use]
pub const fn probe_draw_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu draw buffer"),
        size: (PROBE_READBACK_SIZE as u64) * (PROBE_READBACK_SIZE as u64) * 4,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    }
}

/// The image→buffer copy that moves the drawn pixels into the readback buffer —
/// tightly packed (`64 × 4 = 256` bytes per row, already 256-aligned), the whole
/// 64×64 mip-0 slice, under the draw probe's own image and buffer handles.
#[must_use]
pub const fn probe_draw_copy() -> BufferImageCopy {
    BufferImageCopy {
        buffer: PROBE_DRAW_BUFFER,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: PROBE_DRAW_IMAGE,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: Extent3d::d2(PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
    }
}

/// A fullscreen-triangle WGSL module that paints the target one flat colour.
///
/// **No vertex buffers.** `vsMain` positions three vertices from
/// `@builtin(vertex_index)` alone — `(-1,-1)`, `(3,-1)`, `(-1,3)`, the oversized
/// triangle that covers the whole viewport — so the draw needs no geometry bound,
/// which is what lets the probe draw with an empty pipeline layout. `fsMain`
/// returns a constant opaque red, [`PROBE_DRAW_COLOR_BYTES`]'s colour, so every
/// covered texel is the same known value. `vsMain`/`fsMain` are the entry points
/// [`PROBE_DRAW_PIPELINE_DESC`] names.
pub const PROBE_DRAW_WGSL: &str = concat!(
    "@vertex fn vsMain(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4<f32> { ",
    "var positions = array<vec2<f32>, 3>(",
    "vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0)); ",
    "return vec4<f32>(positions[vertex], 0.0, 1.0); } ",
    "@fragment fn fsMain() -> @location(0) vec4<f32> ",
    "{ return vec4<f32>(1.0, 0.0, 0.0, 1.0); }"
);

/// The shader module the draw probe's frame creates for its pipeline. WGSL only,
/// on [`PROBE_GRAPHICS_SHADER_MODULE_DESC`]'s terms, filed at
/// [`PROBE_DRAW_SHADER_MODULE`].
pub const PROBE_DRAW_SHADER_MODULE_DESC: ShaderModuleDesc<'static> = ShaderModuleDesc {
    label: Some("crcbl-webgpu draw shader"),
    spirv: &[],
    wgsl: Some(PROBE_DRAW_WGSL),
    msl: None,
    dxil: &[],
};

/// The shader-module handle the draw probe's pipeline names. `2 << 32`.
pub const PROBE_DRAW_SHADER_MODULE: ShaderModuleHandle =
    match ShaderModuleHandle::from_bits(2 << 32) {
        Some(module) => module,
        None => panic!("generation 2 is not zero"),
    };

/// The pipeline-layout handle the draw probe's pipeline is built against.
/// `2 << 32`.
pub const PROBE_DRAW_PIPELINE_LAYOUT: PipelineLayoutHandle =
    match PipelineLayoutHandle::from_bits(2 << 32) {
        Some(layout) => layout,
        None => panic!("generation 2 is not zero"),
    };

/// The pipeline layout the draw probe's frame creates. **Empty** — the shaders
/// bind nothing, so there are no bind-group layouts and no push constants.
pub const PROBE_DRAW_PIPELINE_LAYOUT_DESC: PipelineLayoutDesc<'static> = PipelineLayoutDesc {
    label: Some("crcbl-webgpu draw pipeline layout"),
    bind_group_layouts: &[],
    push_constants: None,
};

/// The graphics-pipeline handle the draw probe binds and draws with. `2 << 32`.
pub const PROBE_DRAW_PIPELINE: GraphicsPipelineHandle =
    match GraphicsPipelineHandle::from_bits(2 << 32) {
        Some(pipeline) => pipeline,
        None => panic!("generation 2 is not zero"),
    };

/// The one colour target [`PROBE_DRAW_PIPELINE_DESC`] writes.
///
/// [`Format::Rgba8Unorm`] to match the render target, and **`blend: None`** —
/// opaque, so the fragment's colour is written to the texel exactly with no
/// blend against the clear underneath it. That exactness is what lets the gate
/// assert the read-back bytes are [`PROBE_DRAW_COLOR_BYTES`] and not a mix.
pub const PROBE_DRAW_COLOR_TARGETS: [ColorTargetState; 1] = [ColorTargetState {
    format: Format::Rgba8Unorm,
    blend: None,
    write_mask: ColorWrites::ALL,
}];

/// The pipeline the draw probe binds before its draw.
///
/// **Colour only** — `depth_stencil: None`, not the graphics-probe default's
/// reversed-Z depth — because the pass has no depth attachment and the draw only
/// needs to write colour. A plain [`PrimitiveTopology::TriangleList`] with no
/// culling so the fullscreen triangle always rasterises whichever way it winds;
/// single-sampled so no attachment must be multisampled; and the one opaque
/// colour target above. `vertex` and `fragment` both name
/// [`PROBE_DRAW_SHADER_MODULE`] with the two entry points its WGSL declares.
pub const PROBE_DRAW_PIPELINE_DESC: GraphicsPipelineDesc<'static> = GraphicsPipelineDesc {
    label: Some("crcbl-webgpu draw pipeline"),
    layout: PROBE_DRAW_PIPELINE_LAYOUT,
    vertex: ShaderEntry {
        module: PROBE_DRAW_SHADER_MODULE,
        entry_point: "vsMain",
    },
    fragment: Some(ShaderEntry {
        module: PROBE_DRAW_SHADER_MODULE,
        entry_point: "fsMain",
    }),
    primitive: PrimitiveState {
        topology: PrimitiveTopology::TriangleList,
        front_face: FrontFace::Ccw,
        cull_mode: CullMode::None,
        polygon_mode: PolygonMode::Fill,
        depth_clamp: false,
    },
    depth_stencil: None,
    multisample: MultisampleState {
        samples: 1,
        mask: !0,
        alpha_to_coverage: false,
    },
    color_targets: &PROBE_DRAW_COLOR_TARGETS,
};

// The dispatch probe (group U): a compute shader that writes a known 32-bit
// pattern into a storage buffer, copied to a host buffer and read back, so the
// bytes prove a `dispatchWorkgroups` actually ran. Every handle it names is
// `3 << 32` — a generation past the draw probe's `2 << 32` and the readback
// probe's `1 << 32` — so its live resources never land in another probe's slot
// in the shared page. It creates *two* buffers, which the one type that carries
// two here (storage and host) distinguishes by index; every other resource is a
// different type, so a shared `3 << 32` is distinct by kind.
//
// **Named `PROBE_DISPATCH_*`, not `PROBE_COMPUTE_*`.** The compute-*pipeline*
// probe (group Q) already owns the `PROBE_COMPUTE_PIPELINE*` names for the
// pipeline it builds without ever dispatching; this probe is that one's draw-probe
// analogue — its own pipeline and its own frame that runs the dispatch — so it
// takes a distinct prefix the way [`PROBE_DRAW_PIPELINE`] is distinct from
// [`PROBE_GRAPHICS_PIPELINE`].

/// The 32-bit pattern the compute shader writes into every slot of the storage
/// buffer — `0xDEADBEEF`, a value a zero-initialised buffer cannot hold, so a
/// readback of it is proof the dispatch ran.
pub const PROBE_DISPATCH_PATTERN: u32 = 0xDEAD_BEEF;

/// [`PROBE_DISPATCH_PATTERN`] as the four little-endian bytes the storage buffer
/// holds it as — what the gate checks every 4-byte word against.
pub const PROBE_DISPATCH_PATTERN_BYTES: [u8; 4] = PROBE_DISPATCH_PATTERN.to_le_bytes();

/// The number of `u32` slots the storage buffer holds and the dispatch fills.
///
/// **64, matching the shader's `@workgroup_size(64)`**: one `dispatch(1, 1, 1)`
/// launches a single 64-invocation workgroup, and `out[gid.x]` fills slots 0..64,
/// so the whole buffer is written by the one dispatch. `64 * 4 = 256` bytes, which
/// is also a tightly-packed copy the buffer→buffer path needs no alignment for.
pub const PROBE_DISPATCH_SLOTS: u32 = 64;

/// The storage buffer the dispatch writes and the copy reads. `3 << 32`,
/// index `0`.
pub const PROBE_DISPATCH_STORAGE_BUFFER: BufferHandle = match BufferHandle::from_bits(3 << 32) {
    Some(buffer) => buffer,
    None => panic!("generation 3 is not zero"),
};

/// The storage buffer's descriptor: [`PROBE_DISPATCH_SLOTS`] `u32`s,
/// [`BufferUsage::STORAGE`] (the dispatch's `read_write` binding) and
/// [`BufferUsage::TRANSFER_SRC`] (it is copied from), device-local.
#[must_use]
pub const fn probe_dispatch_storage_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu dispatch storage buffer"),
        size: (PROBE_DISPATCH_SLOTS as u64) * 4,
        usage: BufferUsage::STORAGE.union(BufferUsage::TRANSFER_SRC),
        memory: MemoryLocation::DeviceLocal,
    }
}

/// The host buffer the storage buffer is copied into and read back from. A
/// distinct handle from [`PROBE_DISPATCH_STORAGE_BUFFER`] — both are
/// [`BufferHandle`]s, so they cannot share bits the way two different kinds can —
/// at `3 << 32` with index `1`.
pub const PROBE_DISPATCH_HOST_BUFFER: BufferHandle = match BufferHandle::from_bits((3 << 32) | 1) {
    Some(buffer) => buffer,
    None => panic!("generation 3 is not zero"),
};

/// The host buffer's descriptor — the readback buffer's shape
/// ([`MemoryLocation::HostReadback`], [`BufferUsage::TRANSFER_DST`]) at
/// [`PROBE_DISPATCH_SLOTS`] `u32`s, mirroring [`probe_draw_buffer_desc`].
#[must_use]
pub const fn probe_dispatch_host_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu dispatch host buffer"),
        size: (PROBE_DISPATCH_SLOTS as u64) * 4,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    }
}

/// The compute WGSL: a `read_write` storage buffer whose every slot the shader
/// sets to [`PROBE_DISPATCH_PATTERN`].
///
/// **`@workgroup_size(64)` and `out[gid.x]`**: a single `dispatch(1, 1, 1)` runs
/// one 64-invocation workgroup, and each invocation writes its own slot, so the
/// 64-`u32` buffer is filled exactly. `main` is the entry point
/// [`PROBE_DISPATCH_PIPELINE_DESC`] names.
pub const PROBE_DISPATCH_WGSL: &str = concat!(
    "@group(0) @binding(0) var<storage, read_write> out: array<u32>; ",
    "@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) ",
    "{ out[gid.x] = 0xDEADBEEFu; }"
);

/// The shader-module handle the dispatch probe's pipeline names. `3 << 32`.
pub const PROBE_DISPATCH_SHADER_MODULE: ShaderModuleHandle =
    match ShaderModuleHandle::from_bits(3 << 32) {
        Some(module) => module,
        None => panic!("generation 3 is not zero"),
    };

/// The shader module the dispatch probe's frame creates. WGSL only, on
/// [`PROBE_GRAPHICS_SHADER_MODULE_DESC`]'s terms, filed at
/// [`PROBE_DISPATCH_SHADER_MODULE`].
pub const PROBE_DISPATCH_SHADER_MODULE_DESC: ShaderModuleDesc<'static> = ShaderModuleDesc {
    label: Some("crcbl-webgpu dispatch shader"),
    spirv: &[],
    wgsl: Some(PROBE_DISPATCH_WGSL),
    msl: None,
    dxil: &[],
};

/// The bind-group-layout handle the dispatch pipeline is built against. `3 << 32`.
pub const PROBE_DISPATCH_BIND_GROUP_LAYOUT: BindGroupLayoutHandle =
    match BindGroupLayoutHandle::from_bits(3 << 32) {
        Some(layout) => layout,
        None => panic!("generation 3 is not zero"),
    };

/// The one binding the dispatch's shader declares: a `read_write` storage buffer
/// at `@group(0) @binding(0)`, visible to compute. `read_only: false` is the
/// interesting value — it is what makes the buffer writable by the dispatch.
pub const PROBE_DISPATCH_BIND_GROUP_LAYOUT_ENTRIES: [BindGroupLayoutEntry; 1] =
    [BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::COMPUTE,
        kind: BindingKind::StorageBuffer {
            read_only: false,
            dynamic: false,
        },
        count: 1,
        flags: BindingFlags::empty(),
    }];

/// The bind-group layout the dispatch's frame creates just before the group.
pub const PROBE_DISPATCH_BIND_GROUP_LAYOUT_DESC: BindGroupLayoutDesc<'static> =
    BindGroupLayoutDesc {
        label: Some("crcbl-webgpu dispatch layout"),
        entries: &PROBE_DISPATCH_BIND_GROUP_LAYOUT_ENTRIES,
    };

/// The bind-group handle the dispatch binds at slot 0. `3 << 32`.
pub const PROBE_DISPATCH_BIND_GROUP: BindGroupHandle = match BindGroupHandle::from_bits(3 << 32) {
    Some(group) => group,
    None => panic!("generation 3 is not zero"),
};

/// The one assignment the dispatch's bind group carries: the whole of
/// [`PROBE_DISPATCH_STORAGE_BUFFER`] at binding 0.
pub const PROBE_DISPATCH_BIND_GROUP_ENTRIES: [BindGroupEntry; 1] = [BindGroupEntry {
    binding: 0,
    array_index: 0,
    resource: BindingResource::whole_buffer(PROBE_DISPATCH_STORAGE_BUFFER),
}];

/// The descriptor the dispatch's `create_bind_group` asks with: the layout above
/// and the storage buffer bound whole.
pub const PROBE_DISPATCH_BIND_GROUP_DESC: BindGroupDesc<'static> = BindGroupDesc {
    label: Some("crcbl-webgpu dispatch bind group"),
    layout: PROBE_DISPATCH_BIND_GROUP_LAYOUT,
    entries: &PROBE_DISPATCH_BIND_GROUP_ENTRIES,
    variable_count: None,
};

/// The pipeline-layout handle the dispatch pipeline is built against. `3 << 32`.
pub const PROBE_DISPATCH_PIPELINE_LAYOUT: PipelineLayoutHandle =
    match PipelineLayoutHandle::from_bits(3 << 32) {
        Some(layout) => layout,
        None => panic!("generation 3 is not zero"),
    };

/// The one bind-group layout [`PROBE_DISPATCH_PIPELINE_LAYOUT_DESC`] names.
pub const PROBE_DISPATCH_PIPELINE_LAYOUT_BIND_GROUP_LAYOUTS: [BindGroupLayoutHandle; 1] =
    [PROBE_DISPATCH_BIND_GROUP_LAYOUT];

/// The pipeline layout the dispatch's frame creates — the one bind-group layout
/// above, no push constants.
pub const PROBE_DISPATCH_PIPELINE_LAYOUT_DESC: PipelineLayoutDesc<'static> = PipelineLayoutDesc {
    label: Some("crcbl-webgpu dispatch pipeline layout"),
    bind_group_layouts: &PROBE_DISPATCH_PIPELINE_LAYOUT_BIND_GROUP_LAYOUTS,
    push_constants: None,
};

/// The compute-pipeline handle the dispatch binds. `3 << 32`.
pub const PROBE_DISPATCH_PIPELINE: ComputePipelineHandle =
    match ComputePipelineHandle::from_bits(3 << 32) {
        Some(pipeline) => pipeline,
        None => panic!("generation 3 is not zero"),
    };

/// The pipeline the dispatch binds and runs.
///
/// **`workgroup_size` is `[64, 1, 1]`, matching the module's
/// `@workgroup_size(64)`.** The replayer drops the field — WebGPU reads the real
/// value from the module — so it is chosen to agree with the shader, exactly as
/// [`PROBE_COMPUTE_PIPELINE_DESC`] does. `compute.module` is
/// [`PROBE_DISPATCH_SHADER_MODULE`] and `layout` is
/// [`PROBE_DISPATCH_PIPELINE_LAYOUT`], the two ids resolved into two tables.
pub const PROBE_DISPATCH_PIPELINE_DESC: ComputePipelineDesc<'static> = ComputePipelineDesc {
    label: Some("crcbl-webgpu dispatch pipeline"),
    layout: PROBE_DISPATCH_PIPELINE_LAYOUT,
    compute: ShaderEntry {
        module: PROBE_DISPATCH_SHADER_MODULE,
        entry_point: "main",
    },
    workgroup_size: [PROBE_DISPATCH_SLOTS, 1, 1],
};

/// The queue the dispatch probe names in its command encoder. `3 << 32` —
/// carried, not used to pick a queue: WebGPU has one implicit queue.
pub const PROBE_DISPATCH_QUEUE: QueueHandle = match QueueHandle::from_bits(3 << 32) {
    Some(queue) => queue,
    None => panic!("generation 3 is not zero"),
};

/// The command buffer the dispatch probe finishes its encoder into. `3 << 32`.
pub const PROBE_DISPATCH_COMMAND_BUFFER: CommandBufferHandle =
    match CommandBufferHandle::from_bits(3 << 32) {
        Some(command_buffer) => command_buffer,
        None => panic!("generation 3 is not zero"),
    };

/// The in-flight readback the dispatch probe requests and polls. `3 << 32`.
pub const PROBE_DISPATCH_READBACK: ReadbackHandle = match ReadbackHandle::from_bits(3 << 32) {
    Some(readback) => readback,
    None => panic!("generation 3 is not zero"),
};

/// The buffer→buffer copy that moves the dispatch's storage output into the host
/// buffer — the whole 256 bytes from offset 0 to offset 0.
#[must_use]
pub const fn probe_dispatch_copy() -> BufferCopy {
    BufferCopy {
        src: PROBE_DISPATCH_STORAGE_BUFFER,
        src_offset: 0,
        dst: PROBE_DISPATCH_HOST_BUFFER,
        dst_offset: 0,
        size: (PROBE_DISPATCH_SLOTS as u64) * 4,
    }
}

// The copy-chain probe (group V): a compute dispatch fills a storage buffer with
// a red `rgba8` pattern, that buffer is copied INTO a texture
// (`copy_buffer_to_image`), that texture is copied to a SECOND texture
// (`copy_image_to_image`), and the second is copied back OUT to a host buffer
// (`copy_image_to_buffer`) that is read back. The read-back texels are red only
// if both new copies ran, so one chain observes them both. Every handle is
// `4 << 32` — a generation past the dispatch probe's `3 << 32` — so its live
// resources never land in another probe's slot in the shared page; the handle
// kinds it shares with the fill probe (which is also `4 << 32`) are given
// distinct indices below.

/// The 32-bit pattern the dispatch writes into every slot of the storage buffer
/// — opaque red as `Rgba8Unorm` little-endian bytes `[255, 0, 0, 255]`, a value
/// a zero-initialised buffer or texture cannot hold, so a red read-back is proof
/// the whole copy chain ran.
pub const PROBE_COPYCHAIN_PATTERN: u32 = 0xFF00_00FF;

/// [`PROBE_COPYCHAIN_PATTERN`] as the four little-endian bytes each texel holds
/// it as — `[255, 0, 0, 255]`, what the gate checks every 4-byte texel against.
pub const PROBE_COPYCHAIN_PATTERN_BYTES: [u8; 4] = PROBE_COPYCHAIN_PATTERN.to_le_bytes();

/// The side of the square textures the copy chain moves through.
///
/// **64, so a tightly-packed `Rgba8Unorm` row is `64 × 4 = 256` bytes** — already
/// the copy alignment WebGPU wants, exactly as [`PROBE_READBACK_SIZE`] is chosen.
pub const PROBE_COPYCHAIN_SIZE: u32 = 64;

/// The number of `u32` slots the storage buffer holds and the dispatch fills —
/// one per texel of a [`PROBE_COPYCHAIN_SIZE`]² texture, `64 × 64 = 4096`.
pub const PROBE_COPYCHAIN_SLOTS: u32 = PROBE_COPYCHAIN_SIZE * PROBE_COPYCHAIN_SIZE;

/// The storage buffer the dispatch writes and the buffer→image copy reads.
/// `4 << 32`, index `0`.
pub const PROBE_COPYCHAIN_STORAGE_BUFFER: BufferHandle = match BufferHandle::from_bits(4 << 32) {
    Some(buffer) => buffer,
    None => panic!("generation 4 is not zero"),
};

/// The storage buffer's descriptor: [`PROBE_COPYCHAIN_SLOTS`] `u32`s (16 KiB),
/// [`BufferUsage::STORAGE`] (the dispatch's `read_write` binding) and
/// [`BufferUsage::TRANSFER_SRC`] (it is copied from), device-local.
#[must_use]
pub const fn probe_copychain_storage_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu copychain storage buffer"),
        size: (PROBE_COPYCHAIN_SLOTS as u64) * 4,
        usage: BufferUsage::STORAGE.union(BufferUsage::TRANSFER_SRC),
        memory: MemoryLocation::DeviceLocal,
    }
}

/// The host buffer the second texture is copied into and read back from. A
/// distinct [`BufferHandle`] from [`PROBE_COPYCHAIN_STORAGE_BUFFER`] at `4 << 32`
/// with index `1`.
pub const PROBE_COPYCHAIN_HOST_BUFFER: BufferHandle = match BufferHandle::from_bits((4 << 32) | 1) {
    Some(buffer) => buffer,
    None => panic!("generation 4 is not zero"),
};

/// The host buffer's descriptor — the readback buffer's shape
/// ([`MemoryLocation::HostReadback`], [`BufferUsage::TRANSFER_DST`]) at
/// [`PROBE_COPYCHAIN_SLOTS`] `u32`s.
#[must_use]
pub const fn probe_copychain_host_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu copychain host buffer"),
        size: (PROBE_COPYCHAIN_SLOTS as u64) * 4,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    }
}

/// The first texture — the buffer→image copy's destination and the image→image
/// copy's source. `4 << 32`, index `0`.
pub const PROBE_COPYCHAIN_IMAGE_A: ImageHandle = match ImageHandle::from_bits(4 << 32) {
    Some(image) => image,
    None => panic!("generation 4 is not zero"),
};

/// The second texture — the image→image copy's destination and the image→buffer
/// copy's source. A distinct [`ImageHandle`] from [`PROBE_COPYCHAIN_IMAGE_A`] at
/// `4 << 32` with index `1`.
pub const PROBE_COPYCHAIN_IMAGE_B: ImageHandle = match ImageHandle::from_bits((4 << 32) | 1) {
    Some(image) => image,
    None => panic!("generation 4 is not zero"),
};

/// The two textures' shared descriptor: [`PROBE_COPYCHAIN_SIZE`]² `Rgba8Unorm`,
/// both [`ImageUsage::TRANSFER_DST`] (each is copied into) and
/// [`ImageUsage::TRANSFER_SRC`] (each is copied from).
#[must_use]
pub const fn probe_copychain_image_desc() -> ImageDesc<'static> {
    ImageDesc {
        label: Some("crcbl-webgpu copychain image"),
        image_type: ImageType::D2,
        extent: Extent3d::d2(PROBE_COPYCHAIN_SIZE, PROBE_COPYCHAIN_SIZE),
        format: Format::Rgba8Unorm,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::TRANSFER_DST.union(ImageUsage::TRANSFER_SRC),
    }
}

/// The compute WGSL: a `read_write` storage buffer whose every slot the shader
/// sets to [`PROBE_COPYCHAIN_PATTERN`].
///
/// **`@workgroup_size(64)` and `out[gid.x]`**: `dispatch(64, 1, 1)` runs 64
/// workgroups of 64 invocations, `4096` in all, and each writes its own slot, so
/// the 4096-`u32` buffer is filled exactly.
pub const PROBE_COPYCHAIN_WGSL: &str = concat!(
    "@group(0) @binding(0) var<storage, read_write> out: array<u32>; ",
    "@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) ",
    "{ out[gid.x] = 0xFF0000FFu; }"
);

/// The shader-module handle the copy chain's pipeline names. `4 << 32`, index
/// `0`.
pub const PROBE_COPYCHAIN_SHADER_MODULE: ShaderModuleHandle =
    match ShaderModuleHandle::from_bits(4 << 32) {
        Some(module) => module,
        None => panic!("generation 4 is not zero"),
    };

/// The shader module the copy chain's frame creates. WGSL only, filed at
/// [`PROBE_COPYCHAIN_SHADER_MODULE`].
pub const PROBE_COPYCHAIN_SHADER_MODULE_DESC: ShaderModuleDesc<'static> = ShaderModuleDesc {
    label: Some("crcbl-webgpu copychain shader"),
    spirv: &[],
    wgsl: Some(PROBE_COPYCHAIN_WGSL),
    msl: None,
    dxil: &[],
};

/// The bind-group-layout handle the copy chain's pipeline is built against.
/// `4 << 32`, index `0`.
pub const PROBE_COPYCHAIN_BIND_GROUP_LAYOUT: BindGroupLayoutHandle =
    match BindGroupLayoutHandle::from_bits(4 << 32) {
        Some(layout) => layout,
        None => panic!("generation 4 is not zero"),
    };

/// The one binding the copy chain's shader declares: a `read_write` storage
/// buffer at `@group(0) @binding(0)`, visible to compute.
pub const PROBE_COPYCHAIN_BIND_GROUP_LAYOUT_ENTRIES: [BindGroupLayoutEntry; 1] =
    [BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::COMPUTE,
        kind: BindingKind::StorageBuffer {
            read_only: false,
            dynamic: false,
        },
        count: 1,
        flags: BindingFlags::empty(),
    }];

/// The bind-group layout the copy chain's frame creates just before the group.
pub const PROBE_COPYCHAIN_BIND_GROUP_LAYOUT_DESC: BindGroupLayoutDesc<'static> =
    BindGroupLayoutDesc {
        label: Some("crcbl-webgpu copychain layout"),
        entries: &PROBE_COPYCHAIN_BIND_GROUP_LAYOUT_ENTRIES,
    };

/// The bind-group handle the copy chain binds at slot 0. `4 << 32`, index `0`.
pub const PROBE_COPYCHAIN_BIND_GROUP: BindGroupHandle = match BindGroupHandle::from_bits(4 << 32) {
    Some(group) => group,
    None => panic!("generation 4 is not zero"),
};

/// The one assignment the copy chain's bind group carries: the whole of
/// [`PROBE_COPYCHAIN_STORAGE_BUFFER`] at binding 0.
pub const PROBE_COPYCHAIN_BIND_GROUP_ENTRIES: [BindGroupEntry; 1] = [BindGroupEntry {
    binding: 0,
    array_index: 0,
    resource: BindingResource::whole_buffer(PROBE_COPYCHAIN_STORAGE_BUFFER),
}];

/// The descriptor the copy chain's `create_bind_group` asks with.
pub const PROBE_COPYCHAIN_BIND_GROUP_DESC: BindGroupDesc<'static> = BindGroupDesc {
    label: Some("crcbl-webgpu copychain bind group"),
    layout: PROBE_COPYCHAIN_BIND_GROUP_LAYOUT,
    entries: &PROBE_COPYCHAIN_BIND_GROUP_ENTRIES,
    variable_count: None,
};

/// The pipeline-layout handle the copy chain's pipeline is built against.
/// `4 << 32`, index `0`.
pub const PROBE_COPYCHAIN_PIPELINE_LAYOUT: PipelineLayoutHandle =
    match PipelineLayoutHandle::from_bits(4 << 32) {
        Some(layout) => layout,
        None => panic!("generation 4 is not zero"),
    };

/// The one bind-group layout [`PROBE_COPYCHAIN_PIPELINE_LAYOUT_DESC`] names.
pub const PROBE_COPYCHAIN_PIPELINE_LAYOUT_BIND_GROUP_LAYOUTS: [BindGroupLayoutHandle; 1] =
    [PROBE_COPYCHAIN_BIND_GROUP_LAYOUT];

/// The pipeline layout the copy chain's frame creates — the one bind-group
/// layout above, no push constants.
pub const PROBE_COPYCHAIN_PIPELINE_LAYOUT_DESC: PipelineLayoutDesc<'static> = PipelineLayoutDesc {
    label: Some("crcbl-webgpu copychain pipeline layout"),
    bind_group_layouts: &PROBE_COPYCHAIN_PIPELINE_LAYOUT_BIND_GROUP_LAYOUTS,
    push_constants: None,
};

/// The compute-pipeline handle the copy chain binds. `4 << 32`, index `0`.
pub const PROBE_COPYCHAIN_PIPELINE: ComputePipelineHandle =
    match ComputePipelineHandle::from_bits(4 << 32) {
        Some(pipeline) => pipeline,
        None => panic!("generation 4 is not zero"),
    };

/// The pipeline the copy chain binds and runs — `workgroup_size` `[64, 1, 1]`
/// matching the module's `@workgroup_size(64)`.
pub const PROBE_COPYCHAIN_PIPELINE_DESC: ComputePipelineDesc<'static> = ComputePipelineDesc {
    label: Some("crcbl-webgpu copychain pipeline"),
    layout: PROBE_COPYCHAIN_PIPELINE_LAYOUT,
    compute: ShaderEntry {
        module: PROBE_COPYCHAIN_SHADER_MODULE,
        entry_point: "main",
    },
    workgroup_size: [64, 1, 1],
};

/// The queue the copy chain names in its command encoder. `4 << 32`, index `0`.
pub const PROBE_COPYCHAIN_QUEUE: QueueHandle = match QueueHandle::from_bits(4 << 32) {
    Some(queue) => queue,
    None => panic!("generation 4 is not zero"),
};

/// The command buffer the copy chain finishes its encoder into. `4 << 32`, index
/// `0`.
pub const PROBE_COPYCHAIN_COMMAND_BUFFER: CommandBufferHandle =
    match CommandBufferHandle::from_bits(4 << 32) {
        Some(command_buffer) => command_buffer,
        None => panic!("generation 4 is not zero"),
    };

/// The in-flight readback the copy chain requests and polls. `4 << 32`, index
/// `0`.
pub const PROBE_COPYCHAIN_READBACK: ReadbackHandle = match ReadbackHandle::from_bits(4 << 32) {
    Some(readback) => readback,
    None => panic!("generation 4 is not zero"),
};

/// The buffer→image copy that uploads the dispatch's red storage buffer into the
/// first texture — tightly packed (`0` row length and image height), the whole
/// mip-0 slice.
#[must_use]
pub const fn probe_copychain_buffer_to_image() -> BufferImageCopy {
    BufferImageCopy {
        buffer: PROBE_COPYCHAIN_STORAGE_BUFFER,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: PROBE_COPYCHAIN_IMAGE_A,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: Extent3d::d2(PROBE_COPYCHAIN_SIZE, PROBE_COPYCHAIN_SIZE),
    }
}

/// The image→image copy that moves the first texture into the second — mip 0 to
/// mip 0, origin to origin, the whole slice.
#[must_use]
pub const fn probe_copychain_image_to_image() -> ImageCopy {
    ImageCopy {
        src: PROBE_COPYCHAIN_IMAGE_A,
        src_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        src_offset: Offset3d { x: 0, y: 0, z: 0 },
        dst: PROBE_COPYCHAIN_IMAGE_B,
        dst_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        dst_offset: Offset3d { x: 0, y: 0, z: 0 },
        extent: Extent3d::d2(PROBE_COPYCHAIN_SIZE, PROBE_COPYCHAIN_SIZE),
    }
}

/// The image→buffer copy that reads the second texture out into the host buffer
/// — tightly packed, the whole mip-0 slice.
#[must_use]
pub const fn probe_copychain_image_to_buffer() -> BufferImageCopy {
    BufferImageCopy {
        buffer: PROBE_COPYCHAIN_HOST_BUFFER,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: PROBE_COPYCHAIN_IMAGE_B,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: Extent3d::d2(PROBE_COPYCHAIN_SIZE, PROBE_COPYCHAIN_SIZE),
    }
}

// The fill probe (group W): a compute dispatch fills a 256-byte storage buffer
// with a pattern, `fill_buffer(offset 0, size 128, value 0)` zeroes its first
// half, and the whole buffer is copied to a host buffer and read back. The read
// back proves `clearBuffer` zeroed exactly its sub-range — the first half zero,
// the second half still the pattern. `4 << 32`, with the handle kinds it shares
// with the copy chain given distinct indices.

/// The 32-bit pattern the dispatch writes into every slot before the fill zeroes
/// half — `0xDEADBEEF`, a value a zero-initialised buffer cannot hold, so the
/// second half reading it back is proof the fill left it alone.
pub const PROBE_FILL_PATTERN: u32 = 0xDEAD_BEEF;

/// [`PROBE_FILL_PATTERN`] as the four little-endian bytes the buffer holds it as
/// — `[0xEF, 0xBE, 0xAD, 0xDE]`, what the gate checks the untouched half against.
pub const PROBE_FILL_PATTERN_BYTES: [u8; 4] = PROBE_FILL_PATTERN.to_le_bytes();

/// The number of `u32` slots the storage buffer holds and the dispatch fills —
/// `64`, so the buffer is `256` bytes, matching the shader's `@workgroup_size`.
pub const PROBE_FILL_SLOTS: u32 = 64;

/// The bytes the fill zeroes, from offset `0` — `128`, exactly half the
/// `256`-byte buffer, so the read-back proves the fill touched its range and no
/// more.
pub const PROBE_FILL_ZEROED_BYTES: u64 = 128;

/// The storage buffer the dispatch writes, the fill zeroes half of, and the copy
/// reads. `4 << 32`, index `2` — distinct from the copy chain's two buffers.
pub const PROBE_FILL_STORAGE_BUFFER: BufferHandle = match BufferHandle::from_bits((4 << 32) | 2) {
    Some(buffer) => buffer,
    None => panic!("generation 4 is not zero"),
};

/// The storage buffer's descriptor: [`PROBE_FILL_SLOTS`] `u32`s (256 bytes),
/// [`BufferUsage::STORAGE`] (the dispatch's binding), [`BufferUsage::TRANSFER_DST`]
/// (the fill writes it) and [`BufferUsage::TRANSFER_SRC`] (it is copied from),
/// device-local.
#[must_use]
pub const fn probe_fill_storage_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu fill storage buffer"),
        size: (PROBE_FILL_SLOTS as u64) * 4,
        usage: BufferUsage::STORAGE
            .union(BufferUsage::TRANSFER_DST)
            .union(BufferUsage::TRANSFER_SRC),
        memory: MemoryLocation::DeviceLocal,
    }
}

/// The host buffer the storage buffer is copied into and read back from. A
/// distinct [`BufferHandle`] at `4 << 32` with index `3`.
pub const PROBE_FILL_HOST_BUFFER: BufferHandle = match BufferHandle::from_bits((4 << 32) | 3) {
    Some(buffer) => buffer,
    None => panic!("generation 4 is not zero"),
};

/// The host buffer's descriptor — the readback buffer's shape at
/// [`PROBE_FILL_SLOTS`] `u32`s.
#[must_use]
pub const fn probe_fill_host_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu fill host buffer"),
        size: (PROBE_FILL_SLOTS as u64) * 4,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    }
}

/// The compute WGSL: a `read_write` storage buffer whose every slot the shader
/// sets to [`PROBE_FILL_PATTERN`]. `@workgroup_size(64)` filled by
/// `dispatch(1, 1, 1)` — one 64-invocation workgroup for the 64 slots.
pub const PROBE_FILL_WGSL: &str = concat!(
    "@group(0) @binding(0) var<storage, read_write> out: array<u32>; ",
    "@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) ",
    "{ out[gid.x] = 0xDEADBEEFu; }"
);

/// The shader-module handle the fill probe's pipeline names. `4 << 32`, index
/// `1`.
pub const PROBE_FILL_SHADER_MODULE: ShaderModuleHandle =
    match ShaderModuleHandle::from_bits((4 << 32) | 1) {
        Some(module) => module,
        None => panic!("generation 4 is not zero"),
    };

/// The shader module the fill probe's frame creates. WGSL only, filed at
/// [`PROBE_FILL_SHADER_MODULE`].
pub const PROBE_FILL_SHADER_MODULE_DESC: ShaderModuleDesc<'static> = ShaderModuleDesc {
    label: Some("crcbl-webgpu fill shader"),
    spirv: &[],
    wgsl: Some(PROBE_FILL_WGSL),
    msl: None,
    dxil: &[],
};

/// The bind-group-layout handle the fill probe's pipeline is built against.
/// `4 << 32`, index `1`.
pub const PROBE_FILL_BIND_GROUP_LAYOUT: BindGroupLayoutHandle =
    match BindGroupLayoutHandle::from_bits((4 << 32) | 1) {
        Some(layout) => layout,
        None => panic!("generation 4 is not zero"),
    };

/// The one binding the fill probe's shader declares: a `read_write` storage
/// buffer at `@group(0) @binding(0)`, visible to compute.
pub const PROBE_FILL_BIND_GROUP_LAYOUT_ENTRIES: [BindGroupLayoutEntry; 1] =
    [BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStages::COMPUTE,
        kind: BindingKind::StorageBuffer {
            read_only: false,
            dynamic: false,
        },
        count: 1,
        flags: BindingFlags::empty(),
    }];

/// The bind-group layout the fill probe's frame creates just before the group.
pub const PROBE_FILL_BIND_GROUP_LAYOUT_DESC: BindGroupLayoutDesc<'static> = BindGroupLayoutDesc {
    label: Some("crcbl-webgpu fill layout"),
    entries: &PROBE_FILL_BIND_GROUP_LAYOUT_ENTRIES,
};

/// The bind-group handle the fill probe binds at slot 0. `4 << 32`, index `1`.
pub const PROBE_FILL_BIND_GROUP: BindGroupHandle = match BindGroupHandle::from_bits((4 << 32) | 1) {
    Some(group) => group,
    None => panic!("generation 4 is not zero"),
};

/// The one assignment the fill probe's bind group carries: the whole of
/// [`PROBE_FILL_STORAGE_BUFFER`] at binding 0.
pub const PROBE_FILL_BIND_GROUP_ENTRIES: [BindGroupEntry; 1] = [BindGroupEntry {
    binding: 0,
    array_index: 0,
    resource: BindingResource::whole_buffer(PROBE_FILL_STORAGE_BUFFER),
}];

/// The descriptor the fill probe's `create_bind_group` asks with.
pub const PROBE_FILL_BIND_GROUP_DESC: BindGroupDesc<'static> = BindGroupDesc {
    label: Some("crcbl-webgpu fill bind group"),
    layout: PROBE_FILL_BIND_GROUP_LAYOUT,
    entries: &PROBE_FILL_BIND_GROUP_ENTRIES,
    variable_count: None,
};

/// The pipeline-layout handle the fill probe's pipeline is built against.
/// `4 << 32`, index `1`.
pub const PROBE_FILL_PIPELINE_LAYOUT: PipelineLayoutHandle =
    match PipelineLayoutHandle::from_bits((4 << 32) | 1) {
        Some(layout) => layout,
        None => panic!("generation 4 is not zero"),
    };

/// The one bind-group layout [`PROBE_FILL_PIPELINE_LAYOUT_DESC`] names.
pub const PROBE_FILL_PIPELINE_LAYOUT_BIND_GROUP_LAYOUTS: [BindGroupLayoutHandle; 1] =
    [PROBE_FILL_BIND_GROUP_LAYOUT];

/// The pipeline layout the fill probe's frame creates — the one bind-group
/// layout above, no push constants.
pub const PROBE_FILL_PIPELINE_LAYOUT_DESC: PipelineLayoutDesc<'static> = PipelineLayoutDesc {
    label: Some("crcbl-webgpu fill pipeline layout"),
    bind_group_layouts: &PROBE_FILL_PIPELINE_LAYOUT_BIND_GROUP_LAYOUTS,
    push_constants: None,
};

/// The compute-pipeline handle the fill probe binds. `4 << 32`, index `1`.
pub const PROBE_FILL_PIPELINE: ComputePipelineHandle =
    match ComputePipelineHandle::from_bits((4 << 32) | 1) {
        Some(pipeline) => pipeline,
        None => panic!("generation 4 is not zero"),
    };

/// The pipeline the fill probe binds and runs — `workgroup_size` `[64, 1, 1]`
/// matching the module's `@workgroup_size(64)`.
pub const PROBE_FILL_PIPELINE_DESC: ComputePipelineDesc<'static> = ComputePipelineDesc {
    label: Some("crcbl-webgpu fill pipeline"),
    layout: PROBE_FILL_PIPELINE_LAYOUT,
    compute: ShaderEntry {
        module: PROBE_FILL_SHADER_MODULE,
        entry_point: "main",
    },
    workgroup_size: [PROBE_FILL_SLOTS, 1, 1],
};

/// The queue the fill probe names in its command encoder. `4 << 32`, index `1`.
pub const PROBE_FILL_QUEUE: QueueHandle = match QueueHandle::from_bits((4 << 32) | 1) {
    Some(queue) => queue,
    None => panic!("generation 4 is not zero"),
};

/// The command buffer the fill probe finishes its encoder into. `4 << 32`, index
/// `1`.
pub const PROBE_FILL_COMMAND_BUFFER: CommandBufferHandle =
    match CommandBufferHandle::from_bits((4 << 32) | 1) {
        Some(command_buffer) => command_buffer,
        None => panic!("generation 4 is not zero"),
    };

/// The in-flight readback the fill probe requests and polls. `4 << 32`, index
/// `1`.
pub const PROBE_FILL_READBACK: ReadbackHandle = match ReadbackHandle::from_bits((4 << 32) | 1) {
    Some(readback) => readback,
    None => panic!("generation 4 is not zero"),
};

/// The buffer→buffer copy that moves the filled storage buffer into the host
/// buffer — the whole 256 bytes from offset 0 to offset 0.
#[must_use]
pub const fn probe_fill_copy() -> BufferCopy {
    BufferCopy {
        src: PROBE_FILL_STORAGE_BUFFER,
        src_offset: 0,
        dst: PROBE_FILL_HOST_BUFFER,
        dst_offset: 0,
        size: (PROBE_FILL_SLOTS as u64) * 4,
    }
}

// The present probe (group X): the first probe to drive a *real canvas context*.
// It creates a surface on the page's canvas, configures a swapchain on it,
// acquires the frame, clears that acquired image to red, copies it out to a host
// buffer and reads it back — so the bytes prove the whole canvas-context path
// (configure, getCurrentTexture, render, copy) ran. Every handle it names is
// `5 << 32` — a generation past the copy-chain and fill probes' `4 << 32` — so
// its live resources never land in another probe's slot in the shared page.

/// The colour the present probe clears the acquired frame to — opaque red,
/// `vec4<f32>(1.0, 0.0, 0.0, 1.0)`. Distinctive so a stub that acquired nothing
/// and cleared nothing leaves a black/zero canvas rather than this.
pub const PROBE_PRESENT_COLOR: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

/// The clear colour as the bytes a `Rgba8Unorm` texel holds — what the gate
/// checks every pixel against. Only a real acquire-render-copy produces them.
pub const PROBE_PRESENT_COLOR_BYTES: [u8; 4] = [255, 0, 0, 255];

/// The surface the present probe creates on the page's canvas. `5 << 32`.
pub const PROBE_PRESENT_SURFACE: SurfaceHandle = match SurfaceHandle::from_bits(5 << 32) {
    Some(surface) => surface,
    None => panic!("generation 5 is not zero"),
};

/// The swapchain the present probe configures on its surface. `5 << 32`.
pub const PROBE_PRESENT_SWAPCHAIN: SwapchainHandle = match SwapchainHandle::from_bits(5 << 32) {
    Some(swapchain) => swapchain,
    None => panic!("generation 5 is not zero"),
};

/// The descriptor the present probe configures its swapchain with — a 64×64
/// [`Format::Rgba8Unorm`] surface, `Fifo` and `Opaque` (the two a browser
/// canvas offers), on [`PROBE_PRESENT_SURFACE`].
#[must_use]
pub const fn probe_present_swapchain_desc() -> SwapchainDesc<'static> {
    SwapchainDesc {
        label: Some("crcbl-webgpu present swapchain"),
        surface: PROBE_PRESENT_SURFACE,
        format: Format::Rgba8Unorm,
        extent: (PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
        image_count: 2,
        present_mode: PresentMode::Fifo,
        composite_alpha: CompositeAlpha::Opaque,
    }
}

/// The image handle the acquired frame is filed under. `5 << 32`.
pub const PROBE_PRESENT_IMAGE: ImageHandle = match ImageHandle::from_bits(5 << 32) {
    Some(image) => image,
    None => panic!("generation 5 is not zero"),
};

/// The image-view handle the acquired frame's view is filed under, and the pass
/// clears. `5 << 32`.
pub const PROBE_PRESENT_VIEW: ImageViewHandle = match ImageViewHandle::from_bits(5 << 32) {
    Some(view) => view,
    None => panic!("generation 5 is not zero"),
};

/// The buffer handle the presented pixels are copied into and read back from.
/// `5 << 32`.
pub const PROBE_PRESENT_BUFFER: BufferHandle = match BufferHandle::from_bits(5 << 32) {
    Some(buffer) => buffer,
    None => panic!("generation 5 is not zero"),
};

/// The buffer the presented pixels are copied into and read back from — the
/// readback buffer's shape (`64 * 64 * 4` bytes, [`MemoryLocation::HostReadback`],
/// [`BufferUsage::TRANSFER_DST`]) under [`PROBE_PRESENT_BUFFER`].
#[must_use]
pub const fn probe_present_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu present buffer"),
        size: (PROBE_READBACK_SIZE as u64) * (PROBE_READBACK_SIZE as u64) * 4,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    }
}

/// The queue the present probe names in its command encoder. `5 << 32`.
pub const PROBE_PRESENT_QUEUE: QueueHandle = match QueueHandle::from_bits(5 << 32) {
    Some(queue) => queue,
    None => panic!("generation 5 is not zero"),
};

/// The command buffer the present probe finishes its encoder into. `5 << 32`.
pub const PROBE_PRESENT_COMMAND_BUFFER: CommandBufferHandle =
    match CommandBufferHandle::from_bits(5 << 32) {
        Some(command_buffer) => command_buffer,
        None => panic!("generation 5 is not zero"),
    };

/// The in-flight readback the present probe requests and polls. `5 << 32`.
pub const PROBE_PRESENT_READBACK: ReadbackHandle = match ReadbackHandle::from_bits(5 << 32) {
    Some(readback) => readback,
    None => panic!("generation 5 is not zero"),
};

/// The image→buffer copy that moves the acquired-and-cleared pixels into the
/// readback buffer — tightly packed (`64 × 4 = 256` bytes per row), the whole
/// 64×64 mip-0 slice, under the present probe's own image and buffer handles.
///
/// **Recorded before the present**, which is legal because the present is a
/// no-op here: the copy reads the acquired texture the configured canvas handed
/// back, and the `COPY_SRC` usage
/// [`create_swapchain`](crate::StreamWriter::create_swapchain) configures the
/// context with is what lets that copy exist. See
/// [`shim::__crcbl_web_gpu_probe_present`].
#[must_use]
pub const fn probe_present_copy() -> BufferImageCopy {
    BufferImageCopy {
        buffer: PROBE_PRESENT_BUFFER,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: PROBE_PRESENT_IMAGE,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: Extent3d::d2(PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
    }
}

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

/// One readback, from the frame that cleared and copied to the bytes read back.
///
/// The first probe whose answer is *data* rather than a handle or a
/// capability — the decisive proof that the WebGPU backend puts the right pixels
/// in memory. Unlike [`SurfaceCapsProbe`], whose one query has one answer, a
/// readback is polled across frames: the setup frame requests, and each frame
/// after it polls until the browser's `mapAsync` has resolved.
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the bytes.
#[derive(Clone, Debug, Default, PartialEq)]
enum ReadbackProbe {
    /// Nothing has been asked, or the channel had no room.
    #[default]
    Unasked,
    /// The setup frame is on the stream; no poll is out yet.
    Requested,
    /// A poll is on the stream and its answer has not arrived.
    Waiting {
        /// Sequence of the [`PollReadback`](crate::Command::PollReadback), which
        /// the reply will name.
        sequence: u64,
    },
    /// The last poll answered pending; the map has not resolved, so the next
    /// frame polls again.
    Pending,
    /// The bytes are in.
    Ready {
        /// The bytes read back — one `Rgba8Unorm` texel per four.
        bytes: Vec<u8>,
    },
}

impl ReadbackProbe {
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
    /// `true` when this call settled or advanced the probe. A
    /// [`Reply::ReadbackReady`](crate::Reply::ReadbackReady) makes it
    /// [`Ready`](Self::Ready); a [`Reply::ReadbackPending`](crate::Reply::ReadbackPending)
    /// drops it back to [`Pending`](Self::Pending) so the next frame re-polls.
    /// Everything not naming this probe's sequence is left alone, exactly as
    /// [`SurfaceCapsProbe::absorb`] leaves the other probes' answers.
    fn absorb(&mut self, replies: &[(u64, Reply)]) -> bool {
        let Some(waiting) = self.sequence() else {
            return false;
        };
        let Some((_, reply)) = replies.iter().find(|(sequence, _)| *sequence == waiting) else {
            return false;
        };
        *self = match reply {
            Reply::ReadbackReady { data, .. } => Self::Ready {
                bytes: data.clone(),
            },
            Reply::ReadbackPending { .. } => Self::Pending,
            // A reply of another shape naming this sequence is a replayer bug,
            // and it settles rather than waits: the sequence is answered and a
            // second answer is refused, so nothing else is coming. Reported as a
            // pending that will never advance is worse than an honest stop, so
            // drop to `Requested` to re-issue — but that would loop. Instead
            // leave it `Pending`, which the gate's deadline catches.
            _ => Self::Pending,
        };
        true
    }
}

/// One draw-and-read-back, from the frame that drew to the bytes read back.
///
/// [`ReadbackProbe`]'s state machine over again, and deliberately its own rather
/// than a share of it: the two probes are drawing distinct frames and settling on
/// their own schedules, so each holds its own place in the poll protocol. A draw
/// probe *is* a readback at heart — its setup frame ends in the same
/// `request_readback`, and it is answered by the same
/// [`Reply::ReadbackReady`](crate::Reply::ReadbackReady) /
/// [`Reply::ReadbackPending`](crate::Reply::ReadbackPending) — so the transitions
/// mirror the readback's exactly.
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the bytes.
#[derive(Clone, Debug, Default, PartialEq)]
enum DrawProbe {
    /// Nothing has been asked, or the channel had no room.
    #[default]
    Unasked,
    /// The setup frame is on the stream; no poll is out yet.
    Requested,
    /// A poll is on the stream and its answer has not arrived.
    Waiting {
        /// Sequence of the [`PollReadback`](crate::Command::PollReadback), which
        /// the reply will name.
        sequence: u64,
    },
    /// The last poll answered pending; the map has not resolved, so the next
    /// frame polls again.
    Pending,
    /// The bytes are in.
    Ready {
        /// The bytes read back — one `Rgba8Unorm` texel per four, every one the
        /// drawn colour if the draw ran.
        bytes: Vec<u8>,
    },
}

impl DrawProbe {
    /// The sequence this is waiting on, or `None` if it is not waiting.
    const fn sequence(&self) -> Option<u64> {
        match self {
            Self::Waiting { sequence } => Some(*sequence),
            _ => None,
        }
    }

    /// Take this probe's answer out of a drained frame's replies, if it is
    /// there — [`ReadbackProbe::absorb`]'s logic, on this probe's sequence.
    fn absorb(&mut self, replies: &[(u64, Reply)]) -> bool {
        let Some(waiting) = self.sequence() else {
            return false;
        };
        let Some((_, reply)) = replies.iter().find(|(sequence, _)| *sequence == waiting) else {
            return false;
        };
        *self = match reply {
            Reply::ReadbackReady { data, .. } => Self::Ready {
                bytes: data.clone(),
            },
            Reply::ReadbackPending { .. } => Self::Pending,
            // A reply of another shape naming this sequence settles rather than
            // waits, exactly as [`ReadbackProbe::absorb`] argues: leave it
            // `Pending` for the gate's deadline to catch.
            _ => Self::Pending,
        };
        true
    }
}

/// One dispatch-and-read-back, from the frame that dispatched to the bytes read
/// back — [`DrawProbe`]'s state machine again, on the compute path.
///
/// The two probes differ only in the frame they encode: a draw rasterises a
/// triangle, a dispatch runs a compute shader that writes a storage buffer. Both
/// end in the same `request_readback` and are answered by the same
/// [`Reply::ReadbackReady`](crate::Reply::ReadbackReady) /
/// [`Reply::ReadbackPending`](crate::Reply::ReadbackPending), so the transitions
/// mirror [`DrawProbe`]'s exactly.
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the bytes.
#[derive(Clone, Debug, Default, PartialEq)]
enum ComputeProbe {
    /// Nothing has been asked, or the channel had no room.
    #[default]
    Unasked,
    /// The setup frame is on the stream; no poll is out yet.
    Requested,
    /// A poll is on the stream and its answer has not arrived.
    Waiting {
        /// Sequence of the [`PollReadback`](crate::Command::PollReadback), which
        /// the reply will name.
        sequence: u64,
    },
    /// The last poll answered pending; the map has not resolved, so the next
    /// frame polls again.
    Pending,
    /// The bytes are in.
    Ready {
        /// The bytes read back — 64 `u32`s, every one [`PROBE_DISPATCH_PATTERN`]
        /// if the dispatch ran.
        bytes: Vec<u8>,
    },
}

impl ComputeProbe {
    /// The sequence this is waiting on, or `None` if it is not waiting.
    const fn sequence(&self) -> Option<u64> {
        match self {
            Self::Waiting { sequence } => Some(*sequence),
            _ => None,
        }
    }

    /// Take this probe's answer out of a drained frame's replies, if it is
    /// there — [`DrawProbe::absorb`]'s logic, on this probe's sequence.
    fn absorb(&mut self, replies: &[(u64, Reply)]) -> bool {
        let Some(waiting) = self.sequence() else {
            return false;
        };
        let Some((_, reply)) = replies.iter().find(|(sequence, _)| *sequence == waiting) else {
            return false;
        };
        *self = match reply {
            Reply::ReadbackReady { data, .. } => Self::Ready {
                bytes: data.clone(),
            },
            Reply::ReadbackPending { .. } => Self::Pending,
            // A reply of another shape naming this sequence settles rather than
            // waits, exactly as [`DrawProbe::absorb`] argues.
            _ => Self::Pending,
        };
        true
    }
}

/// One copy-chain-and-read-back, from the frame that dispatched-and-copied to the
/// bytes read back — [`ComputeProbe`]'s state machine again, on the copy path.
///
/// The frame it encodes differs only in what it records between the dispatch and
/// the readback: a buffer→image, an image→image and an image→buffer copy rather
/// than one buffer→buffer. Both end in the same `request_readback` and are
/// answered by the same replies, so the transitions mirror [`ComputeProbe`]'s
/// exactly.
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the bytes.
#[derive(Clone, Debug, Default, PartialEq)]
enum CopyChainProbe {
    /// Nothing has been asked, or the channel had no room.
    #[default]
    Unasked,
    /// The setup frame is on the stream; no poll is out yet.
    Requested,
    /// A poll is on the stream and its answer has not arrived.
    Waiting {
        /// Sequence of the [`PollReadback`](crate::Command::PollReadback), which
        /// the reply will name.
        sequence: u64,
    },
    /// The last poll answered pending; the map has not resolved, so the next
    /// frame polls again.
    Pending,
    /// The bytes are in.
    Ready {
        /// The bytes read back — 64×64 `Rgba8Unorm` texels, every one
        /// [`PROBE_COPYCHAIN_PATTERN`] if both copies ran.
        bytes: Vec<u8>,
    },
}

impl CopyChainProbe {
    /// The sequence this is waiting on, or `None` if it is not waiting.
    const fn sequence(&self) -> Option<u64> {
        match self {
            Self::Waiting { sequence } => Some(*sequence),
            _ => None,
        }
    }

    /// Take this probe's answer out of a drained frame's replies, if it is
    /// there — [`ComputeProbe::absorb`]'s logic, on this probe's sequence.
    fn absorb(&mut self, replies: &[(u64, Reply)]) -> bool {
        let Some(waiting) = self.sequence() else {
            return false;
        };
        let Some((_, reply)) = replies.iter().find(|(sequence, _)| *sequence == waiting) else {
            return false;
        };
        *self = match reply {
            Reply::ReadbackReady { data, .. } => Self::Ready {
                bytes: data.clone(),
            },
            Reply::ReadbackPending { .. } => Self::Pending,
            _ => Self::Pending,
        };
        true
    }
}

/// One fill-and-read-back, from the frame that dispatched-filled-and-copied to
/// the bytes read back — [`ComputeProbe`]'s state machine again, on the fill
/// path.
///
/// The frame it encodes fills a storage buffer by dispatch, zeroes its first
/// half with a [`fill_buffer`](crate::StreamWriter::fill_buffer), and copies the
/// whole thing to a host buffer. Both end in the same `request_readback`, so the
/// transitions mirror [`ComputeProbe`]'s exactly.
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the bytes.
#[derive(Clone, Debug, Default, PartialEq)]
enum FillProbe {
    /// Nothing has been asked, or the channel had no room.
    #[default]
    Unasked,
    /// The setup frame is on the stream; no poll is out yet.
    Requested,
    /// A poll is on the stream and its answer has not arrived.
    Waiting {
        /// Sequence of the [`PollReadback`](crate::Command::PollReadback), which
        /// the reply will name.
        sequence: u64,
    },
    /// The last poll answered pending; the map has not resolved, so the next
    /// frame polls again.
    Pending,
    /// The bytes are in.
    Ready {
        /// The bytes read back — 64 `u32`s, the first half zeroed by the fill and
        /// the second half still [`PROBE_FILL_PATTERN`].
        bytes: Vec<u8>,
    },
}

impl FillProbe {
    /// The sequence this is waiting on, or `None` if it is not waiting.
    const fn sequence(&self) -> Option<u64> {
        match self {
            Self::Waiting { sequence } => Some(*sequence),
            _ => None,
        }
    }

    /// Take this probe's answer out of a drained frame's replies, if it is
    /// there — [`ComputeProbe::absorb`]'s logic, on this probe's sequence.
    fn absorb(&mut self, replies: &[(u64, Reply)]) -> bool {
        let Some(waiting) = self.sequence() else {
            return false;
        };
        let Some((_, reply)) = replies.iter().find(|(sequence, _)| *sequence == waiting) else {
            return false;
        };
        *self = match reply {
            Reply::ReadbackReady { data, .. } => Self::Ready {
                bytes: data.clone(),
            },
            Reply::ReadbackPending { .. } => Self::Pending,
            _ => Self::Pending,
        };
        true
    }
}

/// One present-and-read-back, from the frame that acquired-cleared-and-copied to
/// the bytes read back — [`DrawProbe`]'s state machine again, on the present
/// path.
///
/// The frame it encodes differs in what it records before the readback: a
/// surface, a configured swapchain, an acquire, a clear of the *acquired* view,
/// the copy, a submit and a no-op present rather than a create-image-and-clear.
/// It ends in the same `request_readback` and is answered by the same
/// [`Reply::ReadbackReady`](crate::Reply::ReadbackReady) /
/// [`Reply::ReadbackPending`](crate::Reply::ReadbackPending), so the transitions
/// mirror [`DrawProbe`]'s exactly.
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the bytes.
#[derive(Clone, Debug, Default, PartialEq)]
enum PresentProbe {
    /// Nothing has been asked, or the channel had no room.
    #[default]
    Unasked,
    /// The setup frame is on the stream; no poll is out yet.
    Requested,
    /// A poll is on the stream and its answer has not arrived.
    Waiting {
        /// Sequence of the [`PollReadback`](crate::Command::PollReadback), which
        /// the reply will name.
        sequence: u64,
    },
    /// The last poll answered pending; the map has not resolved, so the next
    /// frame polls again.
    Pending,
    /// The bytes are in.
    Ready {
        /// The bytes read back — 64×64 `Rgba8Unorm` texels, every one
        /// [`PROBE_PRESENT_COLOR_BYTES`] if the canvas-context path ran.
        bytes: Vec<u8>,
    },
}

impl PresentProbe {
    /// The sequence this is waiting on, or `None` if it is not waiting.
    const fn sequence(&self) -> Option<u64> {
        match self {
            Self::Waiting { sequence } => Some(*sequence),
            _ => None,
        }
    }

    /// Take this probe's answer out of a drained frame's replies, if it is
    /// there — [`DrawProbe::absorb`]'s logic, on this probe's sequence.
    fn absorb(&mut self, replies: &[(u64, Reply)]) -> bool {
        let Some(waiting) = self.sequence() else {
            return false;
        };
        let Some((_, reply)) = replies.iter().find(|(sequence, _)| *sequence == waiting) else {
            return false;
        };
        *self = match reply {
            Reply::ReadbackReady { data, .. } => Self::Ready {
                bytes: data.clone(),
            },
            Reply::ReadbackPending { .. } => Self::Pending,
            _ => Self::Pending,
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
    readback: ReadbackProbe,
    /// A decode error the readback drain hit, for
    /// [`READBACK_UNDECODABLE`]. Its own string for [`reason`](Self::reason)'s
    /// reason.
    readback_reason: String,
    draw: DrawProbe,
    /// A decode error the draw drain hit, for [`DRAW_UNDECODABLE`]. Its own
    /// string for [`reason`](Self::reason)'s reason.
    draw_reason: String,
    compute: ComputeProbe,
    /// A decode error the compute drain hit, for [`COMPUTE_UNDECODABLE`]. Its own
    /// string for [`reason`](Self::reason)'s reason.
    compute_reason: String,
    copychain: CopyChainProbe,
    /// A decode error the copy-chain drain hit, for [`COPYCHAIN_UNDECODABLE`]. Its
    /// own string for [`reason`](Self::reason)'s reason.
    copychain_reason: String,
    fill: FillProbe,
    /// A decode error the fill drain hit, for [`FILL_UNDECODABLE`]. Its own string
    /// for [`reason`](Self::reason)'s reason.
    fill_reason: String,
    present: PresentProbe,
    /// A decode error the present drain hit, for [`PRESENT_UNDECODABLE`]. Its own
    /// string for [`reason`](Self::reason)'s reason.
    present_reason: String,
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
            readback: ReadbackProbe::Unasked,
            readback_reason: String::new(),
            draw: DrawProbe::Unasked,
            draw_reason: String::new(),
            compute: ComputeProbe::Unasked,
            compute_reason: String::new(),
            copychain: CopyChainProbe::Unasked,
            copychain_reason: String::new(),
            fill: FillProbe::Unasked,
            fill_reason: String::new(),
            present: PresentProbe::Unasked,
            present_reason: String::new(),
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

    /// Encode one [`CreatePipelineLayout`](crate::Command::CreatePipelineLayout)
    /// against [`PROBE_PIPELINE_LAYOUT`], built from a bind-group layout the same
    /// frame creates.
    ///
    /// **Two commands in one frame**, like the bind group: a pipeline layout
    /// names a live bind-group layout, so this records the layout
    /// ([`PROBE_BIND_GROUP_LAYOUT_DESC`]) and then the pipeline layout — one
    /// export, because a creation is answered by nothing and there is no reply to
    /// poll for at either step.
    ///
    /// [`request_sampler`](Self::request_sampler)'s ordering rule and wait rule
    /// both apply — `create_pipeline_layout` is a device method, so it refuses
    /// until a device has opened, and nothing answers a creation.
    ///
    /// **It cannot check that its bind-group layout will resolve, and does not
    /// pretend to**: the layout lives in the page's replayer and nothing here
    /// holds one. A pipeline layout naming a set the browser cannot resolve, or
    /// carrying push constants, is reported through `Device::take_error`, exactly
    /// as a bind group naming a missing resource is. `web/engine/gpu-replay.js`
    /// argues that where it is made.
    fn request_pipeline_layout(&mut self) -> bool {
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
                );
                stream.create_pipeline_layout(PROBE_PIPELINE_LAYOUT, &PROBE_PIPELINE_LAYOUT_DESC)
            })
            .is_some()
    }

    /// Encode one [`CreateComputePipeline`](crate::Command::CreateComputePipeline)
    /// against [`PROBE_COMPUTE_PIPELINE`], built from a shader module and a
    /// pipeline layout the same frame creates.
    ///
    /// **Three commands in one frame.** A compute pipeline names a live shader
    /// module and a live pipeline layout, so this records the compute shader
    /// ([`PROBE_COMPUTE_SHADER_MODULE_DESC`]) and the empty pipeline layout
    /// ([`PROBE_COMPUTE_PIPELINE_LAYOUT_DESC`]) before the pipeline — one export,
    /// because a creation is answered by nothing and there is no reply to poll for
    /// at any step.
    ///
    /// [`request_sampler`](Self::request_sampler)'s ordering rule and wait rule
    /// both apply — `create_compute_pipeline` is a device method, so it refuses
    /// until a device has opened, and nothing answers a creation.
    ///
    /// **It cannot check that its two handles will resolve, and does not pretend
    /// to**: the module and the layout live in the page's replayer and nothing
    /// here holds one. A pipeline naming a stale layout or module is reported
    /// through `Device::take_error`, exactly as a pipeline layout naming a stale
    /// set is. `web/engine/gpu-replay.js` argues that where it is made.
    fn request_compute_pipeline(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        channel
            .encode(|stream| {
                stream.create_shader_module(PROBE_SHADER_MODULE, &PROBE_COMPUTE_SHADER_MODULE_DESC);
                stream.create_pipeline_layout(
                    PROBE_PIPELINE_LAYOUT,
                    &PROBE_COMPUTE_PIPELINE_LAYOUT_DESC,
                );
                stream.create_compute_pipeline(PROBE_COMPUTE_PIPELINE, &PROBE_COMPUTE_PIPELINE_DESC)
            })
            .is_some()
    }

    /// Encode one [`CreateGraphicsPipeline`](crate::Command::CreateGraphicsPipeline)
    /// against [`PROBE_GRAPHICS_PIPELINE`], built from a shader module and a
    /// pipeline layout the same frame creates.
    ///
    /// **Three commands in one frame**, like the compute pipeline. A raster
    /// pipeline names a live shader module and a live pipeline layout, so this
    /// records the shader ([`PROBE_GRAPHICS_SHADER_MODULE_DESC`], both entry
    /// points) and the empty pipeline layout
    /// ([`PROBE_GRAPHICS_PIPELINE_LAYOUT_DESC`]) before the pipeline — one export,
    /// because a creation is answered by nothing and there is no reply to poll for
    /// at any step.
    ///
    /// [`request_sampler`](Self::request_sampler)'s ordering rule and wait rule
    /// both apply — `create_graphics_pipeline` is a device method, so it refuses
    /// until a device has opened, and nothing answers a creation.
    ///
    /// **It cannot check that its handles will resolve, and does not pretend
    /// to**: the module and the layout live in the page's replayer and nothing
    /// here holds one. A pipeline naming a stale one, or a descriptor field WebGPU
    /// cannot express, is reported through `Device::take_error`, exactly as the
    /// compute pipeline's is. `web/engine/gpu-replay.js` argues that where it is
    /// made.
    fn request_graphics_pipeline(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        channel
            .encode(|stream| {
                stream
                    .create_shader_module(PROBE_SHADER_MODULE, &PROBE_GRAPHICS_SHADER_MODULE_DESC);
                stream.create_pipeline_layout(
                    PROBE_PIPELINE_LAYOUT,
                    &PROBE_GRAPHICS_PIPELINE_LAYOUT_DESC,
                );
                stream.create_graphics_pipeline(
                    PROBE_GRAPHICS_PIPELINE,
                    &PROBE_GRAPHICS_PIPELINE_DESC,
                )
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
                self.readback.absorb(&replies);
                self.draw.absorb(&replies);
                self.compute.absorb(&replies);
                self.copychain.absorb(&replies);
                self.fill.absorb(&replies);
                self.present.absorb(&replies);
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

    /// Encode the readback setup frame: a cleared image, its copy to a host
    /// buffer, and the request that will be polled.
    ///
    /// **One frame, many commands, no reply** — the whole of the readback path
    /// up to the poll. It records the image and its view, the host-readback
    /// buffer, a command encoder, a render pass that clears the view to
    /// [`PROBE_READBACK_CLEAR`], the copy of the image into the buffer, the
    /// finish, the submit, and finally the `request_readback` that files the
    /// in-flight map under [`PROBE_READBACK`]. None of these is answered — every
    /// handle is caller-allocated — so it is [`encode`](StreamChannel::encode),
    /// not [`encode_awaited`](StreamChannel::encode_awaited); the poll is what is
    /// awaited.
    ///
    /// `false` until a device has opened — every command here is a device method
    /// — which is [`request_buffer`](Self::request_buffer)'s ordering rule.
    fn request_readback(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_image(PROBE_IMAGE, &probe_readback_image_desc());
                stream.create_image_view(PROBE_IMAGE_VIEW, &PROBE_READBACK_VIEW_DESC);
                stream.create_buffer(PROBE_BUFFER, &probe_readback_buffer_desc());
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu readback encoder"),
                    queue: PROBE_QUEUE,
                });
                let attachments = [ColorAttachment {
                    view: PROBE_IMAGE_VIEW,
                    resolve: None,
                    load: LoadOp::Clear,
                    store: StoreOp::Store,
                    clear: ClearValue::color(PROBE_READBACK_CLEAR),
                }];
                stream.begin_render_pass(&RenderPassDesc {
                    label: Some("crcbl-webgpu readback clear"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    render_area: Rect2d::from_size(PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
                });
                stream.end_render_pass();
                stream.copy_image_to_buffer(&probe_readback_copy());
                stream.finish(PROBE_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_COMMAND_BUFFER]));
                stream.request_readback(
                    PROBE_READBACK,
                    &ReadbackDesc {
                        label: Some("crcbl-webgpu readback"),
                        buffer: PROBE_BUFFER,
                        offset: 0,
                        size: probe_readback_buffer_desc().size,
                        after: None,
                    },
                )
            })
            .is_some();
        if encoded {
            self.readback = ReadbackProbe::Requested;
            self.readback_reason.clear();
        }
        encoded
    }

    /// Encode one [`poll_readback`](crate::StreamWriter::poll_readback) and
    /// register its wait, unless the readback is already waiting or ready.
    ///
    /// **Only polls when there is something to poll for**, which is what keeps
    /// the poll protocol honest: a second poll while one is unanswered would
    /// register a second sequence, and the first reply — naming a sequence
    /// nothing waits on any more — would turn the whole frame's reply buffer into
    /// a [`DecodeError::UnexpectedSequence`](crate::DecodeError::UnexpectedSequence).
    /// So it encodes only from [`Requested`](ReadbackProbe::Requested) or
    /// [`Pending`](ReadbackProbe::Pending), and is a no-op while
    /// [`Waiting`](ReadbackProbe::Waiting) or [`Ready`](ReadbackProbe::Ready).
    ///
    /// Answered by a [`Reply::ReadbackReady`](crate::Reply::ReadbackReady) or
    /// [`Reply::ReadbackPending`](crate::Reply::ReadbackPending), so it goes
    /// through [`encode_awaited`](StreamChannel::encode_awaited) — the reply
    /// names the sequence it returns.
    fn poll_readback(&mut self) -> bool {
        if !matches!(
            self.readback,
            ReadbackProbe::Requested | ReadbackProbe::Pending
        ) {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(sequence) = channel.encode_awaited(|stream| stream.poll_readback(PROBE_READBACK))
        else {
            return false;
        };
        self.readback = ReadbackProbe::Waiting { sequence };
        true
    }

    /// Drain, absorb, and report where the readback has got to.
    fn readback_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.readback_reason = error.to_string();
            return READBACK_UNDECODABLE;
        }
        match &self.readback {
            ReadbackProbe::Unasked => READBACK_UNASKED,
            ReadbackProbe::Requested => READBACK_REQUESTED,
            ReadbackProbe::Waiting { .. } => READBACK_WAITING,
            ReadbackProbe::Pending => READBACK_PENDING,
            ReadbackProbe::Ready { .. } => READBACK_READY,
        }
    }

    /// The bytes the readback came back with, or an empty slice if it has not.
    fn readback_bytes(&self) -> &[u8] {
        match &self.readback {
            ReadbackProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// Encode the draw setup frame: a red-triangle pipeline, a pass that clears
    /// to [`PROBE_DRAW_CLEAR`] and then binds it and draws, and the copy to a host
    /// buffer that is read back.
    ///
    /// **One frame, many commands, no reply** — [`request_readback`](Self::request_readback)'s
    /// frame with three creations prepended (the shader module, the empty pipeline
    /// layout, the pipeline) and two commands added inside the pass (the bind and
    /// the draw). It records the image, its view, the host buffer, the pipeline's
    /// three resources, a command encoder, a render pass that clears the view,
    /// **binds [`PROBE_DRAW_PIPELINE`] and draws three vertices**, the copy, the
    /// finish, the submit, and the `request_readback` under [`PROBE_DRAW_READBACK`].
    /// None is answered — every handle is caller-allocated — so it is
    /// [`encode`](StreamChannel::encode); the poll is what is awaited.
    ///
    /// `false` until a device has opened, [`request_readback`](Self::request_readback)'s
    /// ordering rule — every command here is a device method.
    fn request_draw(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_image(PROBE_DRAW_IMAGE, &probe_draw_image_desc());
                stream.create_image_view(PROBE_DRAW_IMAGE_VIEW, &PROBE_DRAW_VIEW_DESC);
                stream.create_buffer(PROBE_DRAW_BUFFER, &probe_draw_buffer_desc());
                stream
                    .create_shader_module(PROBE_DRAW_SHADER_MODULE, &PROBE_DRAW_SHADER_MODULE_DESC);
                stream.create_pipeline_layout(
                    PROBE_DRAW_PIPELINE_LAYOUT,
                    &PROBE_DRAW_PIPELINE_LAYOUT_DESC,
                );
                stream.create_graphics_pipeline(PROBE_DRAW_PIPELINE, &PROBE_DRAW_PIPELINE_DESC);
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu draw encoder"),
                    queue: PROBE_DRAW_QUEUE,
                });
                let attachments = [ColorAttachment {
                    view: PROBE_DRAW_IMAGE_VIEW,
                    resolve: None,
                    load: LoadOp::Clear,
                    store: StoreOp::Store,
                    clear: ClearValue::color(PROBE_DRAW_CLEAR),
                }];
                stream.begin_render_pass(&RenderPassDesc {
                    label: Some("crcbl-webgpu draw pass"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    render_area: Rect2d::from_size(PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
                });
                stream.bind_graphics_pipeline(PROBE_DRAW_PIPELINE);
                stream.draw(0..3, 0..1);
                stream.end_render_pass();
                stream.copy_image_to_buffer(&probe_draw_copy());
                stream.finish(PROBE_DRAW_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_DRAW_COMMAND_BUFFER]));
                stream.request_readback(
                    PROBE_DRAW_READBACK,
                    &ReadbackDesc {
                        label: Some("crcbl-webgpu draw readback"),
                        buffer: PROBE_DRAW_BUFFER,
                        offset: 0,
                        size: probe_draw_buffer_desc().size,
                        after: None,
                    },
                )
            })
            .is_some();
        if encoded {
            self.draw = DrawProbe::Requested;
            self.draw_reason.clear();
        }
        encoded
    }

    /// Encode one [`poll_readback`](crate::StreamWriter::poll_readback) for the
    /// draw's readback and register its wait, unless it is already waiting or
    /// ready — [`poll_readback`](Self::poll_readback)'s protocol on the draw's
    /// handle.
    fn poll_draw(&mut self) -> bool {
        if !matches!(self.draw, DrawProbe::Requested | DrawProbe::Pending) {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(sequence) =
            channel.encode_awaited(|stream| stream.poll_readback(PROBE_DRAW_READBACK))
        else {
            return false;
        };
        self.draw = DrawProbe::Waiting { sequence };
        true
    }

    /// Drain, absorb, and report where the draw readback has got to.
    fn draw_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.draw_reason = error.to_string();
            return DRAW_UNDECODABLE;
        }
        match &self.draw {
            DrawProbe::Unasked => DRAW_UNASKED,
            DrawProbe::Requested => DRAW_REQUESTED,
            DrawProbe::Waiting { .. } => DRAW_WAITING,
            DrawProbe::Pending => DRAW_PENDING,
            DrawProbe::Ready { .. } => DRAW_READY,
        }
    }

    /// The bytes the draw readback came back with, or an empty slice if it has
    /// not.
    fn draw_bytes(&self) -> &[u8] {
        match &self.draw {
            DrawProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// Encode the present setup frame: a surface on the page's canvas, a
    /// swapchain configured on it, the acquired frame, a pass that clears the
    /// acquired view to [`PROBE_PRESENT_COLOR`], the copy to a host buffer, a
    /// submit, a no-op present, and the request that is read back.
    ///
    /// **One frame, many commands, no reply** — [`request_draw`](Self::request_draw)'s
    /// shape on the present path, and the first probe to drive a *real canvas
    /// context*. It records the surface (naming the canvas `canvas_id` is the
    /// page's key for), the swapchain (a `configure`), the acquire (a
    /// `getCurrentTexture` that binds [`PROBE_PRESENT_IMAGE`] and its view), the
    /// host buffer, an encoder, a render pass that clears
    /// [`PROBE_PRESENT_VIEW`] to red, the copy out of the acquired image, the
    /// finish, the submit, the present (a no-op) and the `request_readback` under
    /// [`PROBE_PRESENT_READBACK`]. None is answered — every handle is
    /// caller-allocated — so it is [`encode`](StreamChannel::encode); the poll is
    /// what is awaited.
    ///
    /// **The copy is recorded before the present, which is deliberate**: the
    /// present is a no-op, so it changes nothing, and reading the acquired texture
    /// first is what makes the presented frame observable. That copy can exist at
    /// all only because [`create_swapchain`](crate::StreamWriter::create_swapchain)
    /// configures the canvas context with `COPY_SRC` beside the render-target
    /// usage — see `web/engine/gpu-replay.js`.
    ///
    /// `false` until a device has opened, [`request_draw`](Self::request_draw)'s
    /// ordering rule — every command after the surface is a device method.
    fn request_present(&mut self, canvas_id: u32) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_surface(PROBE_PRESENT_SURFACE, canvas_id);
                stream.create_swapchain(PROBE_PRESENT_SWAPCHAIN, &probe_present_swapchain_desc());
                stream.acquire_next_frame(
                    PROBE_PRESENT_SWAPCHAIN,
                    PROBE_PRESENT_IMAGE,
                    PROBE_PRESENT_VIEW,
                );
                stream.create_buffer(PROBE_PRESENT_BUFFER, &probe_present_buffer_desc());
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu present encoder"),
                    queue: PROBE_PRESENT_QUEUE,
                });
                let attachments = [ColorAttachment {
                    view: PROBE_PRESENT_VIEW,
                    resolve: None,
                    load: LoadOp::Clear,
                    store: StoreOp::Store,
                    clear: ClearValue::color(PROBE_PRESENT_COLOR),
                }];
                stream.begin_render_pass(&RenderPassDesc {
                    label: Some("crcbl-webgpu present clear"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    render_area: Rect2d::from_size(PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
                });
                stream.end_render_pass();
                stream.copy_image_to_buffer(&probe_present_copy());
                stream.finish(PROBE_PRESENT_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_PRESENT_COMMAND_BUFFER]));
                stream.present(&PresentInfo {
                    swapchain: PROBE_PRESENT_SWAPCHAIN,
                    waits: &[],
                    present_id: None,
                });
                stream.request_readback(
                    PROBE_PRESENT_READBACK,
                    &ReadbackDesc {
                        label: Some("crcbl-webgpu present readback"),
                        buffer: PROBE_PRESENT_BUFFER,
                        offset: 0,
                        size: probe_present_buffer_desc().size,
                        after: None,
                    },
                )
            })
            .is_some();
        if encoded {
            self.present = PresentProbe::Requested;
            self.present_reason.clear();
        }
        encoded
    }

    /// Encode one [`poll_readback`](crate::StreamWriter::poll_readback) for the
    /// present's readback and register its wait, unless it is already waiting or
    /// ready — [`poll_readback`](Self::poll_readback)'s protocol on the present's
    /// handle.
    fn poll_present(&mut self) -> bool {
        if !matches!(
            self.present,
            PresentProbe::Requested | PresentProbe::Pending
        ) {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(sequence) =
            channel.encode_awaited(|stream| stream.poll_readback(PROBE_PRESENT_READBACK))
        else {
            return false;
        };
        self.present = PresentProbe::Waiting { sequence };
        true
    }

    /// Drain, absorb, and report where the present readback has got to.
    fn present_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.present_reason = error.to_string();
            return PRESENT_UNDECODABLE;
        }
        match &self.present {
            PresentProbe::Unasked => PRESENT_UNASKED,
            PresentProbe::Requested => PRESENT_REQUESTED,
            PresentProbe::Waiting { .. } => PRESENT_WAITING,
            PresentProbe::Pending => PRESENT_PENDING,
            PresentProbe::Ready { .. } => PRESENT_READY,
        }
    }

    /// The bytes the present readback came back with, or an empty slice if it has
    /// not.
    fn present_bytes(&self) -> &[u8] {
        match &self.present {
            PresentProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// Encode the dispatch setup frame: a compute pipeline that writes a storage
    /// buffer, a pass that binds and dispatches it, and the copy to a host buffer
    /// that is read back.
    ///
    /// **One frame, many commands, no reply** — [`request_draw`](Self::request_draw)'s
    /// shape on the compute path. It records the two buffers, the pipeline's four
    /// resources (shader, bind-group layout, bind group, pipeline layout, and the
    /// pipeline itself), a command encoder, a compute pass that **binds
    /// [`PROBE_DISPATCH_PIPELINE`], binds the storage buffer's group and
    /// dispatches `1×1×1`** (one 64-invocation workgroup filling the 64 slots), the
    /// buffer→buffer copy, the finish, the submit, and the `request_readback` under
    /// [`PROBE_DISPATCH_READBACK`]. None is answered — every handle is
    /// caller-allocated — so it is [`encode`](StreamChannel::encode); the poll is
    /// what is awaited.
    ///
    /// `false` until a device has opened, [`request_draw`](Self::request_draw)'s
    /// ordering rule — every command here is a device method.
    fn request_compute(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_buffer(
                    PROBE_DISPATCH_STORAGE_BUFFER,
                    &probe_dispatch_storage_buffer_desc(),
                );
                stream.create_buffer(
                    PROBE_DISPATCH_HOST_BUFFER,
                    &probe_dispatch_host_buffer_desc(),
                );
                stream.create_shader_module(
                    PROBE_DISPATCH_SHADER_MODULE,
                    &PROBE_DISPATCH_SHADER_MODULE_DESC,
                );
                stream.create_bind_group_layout(
                    PROBE_DISPATCH_BIND_GROUP_LAYOUT,
                    &PROBE_DISPATCH_BIND_GROUP_LAYOUT_DESC,
                );
                stream
                    .create_bind_group(PROBE_DISPATCH_BIND_GROUP, &PROBE_DISPATCH_BIND_GROUP_DESC);
                stream.create_pipeline_layout(
                    PROBE_DISPATCH_PIPELINE_LAYOUT,
                    &PROBE_DISPATCH_PIPELINE_LAYOUT_DESC,
                );
                stream.create_compute_pipeline(
                    PROBE_DISPATCH_PIPELINE,
                    &PROBE_DISPATCH_PIPELINE_DESC,
                );
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu dispatch encoder"),
                    queue: PROBE_DISPATCH_QUEUE,
                });
                stream.begin_compute_pass(&ComputePassDesc {
                    label: Some("crcbl-webgpu dispatch pass"),
                });
                stream.bind_compute_pipeline(PROBE_DISPATCH_PIPELINE);
                stream.bind_group(
                    0,
                    PROBE_DISPATCH_BIND_GROUP,
                    &[],
                    PROBE_DISPATCH_PIPELINE_LAYOUT,
                );
                stream.dispatch(1, 1, 1);
                stream.end_compute_pass();
                stream.copy_buffer_to_buffer(&probe_dispatch_copy());
                stream.finish(PROBE_DISPATCH_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_DISPATCH_COMMAND_BUFFER]));
                stream.request_readback(
                    PROBE_DISPATCH_READBACK,
                    &ReadbackDesc {
                        label: Some("crcbl-webgpu dispatch readback"),
                        buffer: PROBE_DISPATCH_HOST_BUFFER,
                        offset: 0,
                        size: probe_dispatch_host_buffer_desc().size,
                        after: None,
                    },
                )
            })
            .is_some();
        if encoded {
            self.compute = ComputeProbe::Requested;
            self.compute_reason.clear();
        }
        encoded
    }

    /// Encode one [`poll_readback`](crate::StreamWriter::poll_readback) for the
    /// dispatch's readback and register its wait, unless it is already waiting or
    /// ready — [`poll_draw`](Self::poll_draw)'s protocol on the dispatch's handle.
    fn poll_compute(&mut self) -> bool {
        if !matches!(
            self.compute,
            ComputeProbe::Requested | ComputeProbe::Pending
        ) {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(sequence) =
            channel.encode_awaited(|stream| stream.poll_readback(PROBE_DISPATCH_READBACK))
        else {
            return false;
        };
        self.compute = ComputeProbe::Waiting { sequence };
        true
    }

    /// Drain, absorb, and report where the dispatch readback has got to.
    fn compute_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.compute_reason = error.to_string();
            return COMPUTE_UNDECODABLE;
        }
        match &self.compute {
            ComputeProbe::Unasked => COMPUTE_UNASKED,
            ComputeProbe::Requested => COMPUTE_REQUESTED,
            ComputeProbe::Waiting { .. } => COMPUTE_WAITING,
            ComputeProbe::Pending => COMPUTE_PENDING,
            ComputeProbe::Ready { .. } => COMPUTE_READY,
        }
    }

    /// The bytes the dispatch readback came back with, or an empty slice if it
    /// has not.
    fn compute_bytes(&self) -> &[u8] {
        match &self.compute {
            ComputeProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// Encode the copy-chain setup frame: a dispatch that fills a storage buffer
    /// with the red pattern, then the buffer→image, image→image and image→buffer
    /// copies that carry it through two textures to a host buffer, read back.
    ///
    /// **One frame, many commands, no reply** — [`request_compute`](Self::request_compute)'s
    /// shape with three copies where that one has one. It records the two buffers,
    /// the two textures, the pipeline's four resources, a command encoder, a
    /// compute pass that binds, binds the storage group and dispatches
    /// `64×1×1` (4096 invocations for the 4096 slots), a `pipeline_barrier`
    /// (the documented no-op) between the dispatch and the first copy, the three
    /// copies, the finish, the submit, and the `request_readback` under
    /// [`PROBE_COPYCHAIN_READBACK`]. None is answered, so it is
    /// [`encode`](StreamChannel::encode); the poll is what is awaited.
    ///
    /// `false` until a device has opened, [`request_compute`](Self::request_compute)'s
    /// ordering rule — every command here is a device method.
    fn request_copychain(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_buffer(
                    PROBE_COPYCHAIN_STORAGE_BUFFER,
                    &probe_copychain_storage_buffer_desc(),
                );
                stream.create_buffer(
                    PROBE_COPYCHAIN_HOST_BUFFER,
                    &probe_copychain_host_buffer_desc(),
                );
                stream.create_image(PROBE_COPYCHAIN_IMAGE_A, &probe_copychain_image_desc());
                stream.create_image(PROBE_COPYCHAIN_IMAGE_B, &probe_copychain_image_desc());
                stream.create_shader_module(
                    PROBE_COPYCHAIN_SHADER_MODULE,
                    &PROBE_COPYCHAIN_SHADER_MODULE_DESC,
                );
                stream.create_bind_group_layout(
                    PROBE_COPYCHAIN_BIND_GROUP_LAYOUT,
                    &PROBE_COPYCHAIN_BIND_GROUP_LAYOUT_DESC,
                );
                stream.create_bind_group(
                    PROBE_COPYCHAIN_BIND_GROUP,
                    &PROBE_COPYCHAIN_BIND_GROUP_DESC,
                );
                stream.create_pipeline_layout(
                    PROBE_COPYCHAIN_PIPELINE_LAYOUT,
                    &PROBE_COPYCHAIN_PIPELINE_LAYOUT_DESC,
                );
                stream.create_compute_pipeline(
                    PROBE_COPYCHAIN_PIPELINE,
                    &PROBE_COPYCHAIN_PIPELINE_DESC,
                );
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu copychain encoder"),
                    queue: PROBE_COPYCHAIN_QUEUE,
                });
                stream.begin_compute_pass(&ComputePassDesc {
                    label: Some("crcbl-webgpu copychain pass"),
                });
                stream.bind_compute_pipeline(PROBE_COPYCHAIN_PIPELINE);
                stream.bind_group(
                    0,
                    PROBE_COPYCHAIN_BIND_GROUP,
                    &[],
                    PROBE_COPYCHAIN_PIPELINE_LAYOUT,
                );
                stream.dispatch(PROBE_COPYCHAIN_SIZE, 1, 1);
                stream.end_compute_pass();
                // The documented no-op, sitting at the natural seam: the storage
                // buffer moves from the dispatch's `ShaderWrite` to the copy's
                // `TransferSrc`. The replayer records nothing (WebGPU tracks state
                // itself), so the readback still comes back red — which is what
                // proves a barrier mid-frame does not disturb replay.
                stream.pipeline_barrier(&Barriers {
                    buffers: &[BufferBarrier {
                        buffer: PROBE_COPYCHAIN_STORAGE_BUFFER,
                        from: ResourceState::ShaderWrite,
                        to: ResourceState::TransferSrc,
                        queue_transfer: None,
                    }],
                    images: &[],
                    global: false,
                });
                stream.copy_buffer_to_image(&probe_copychain_buffer_to_image());
                stream.copy_image_to_image(&probe_copychain_image_to_image());
                stream.copy_image_to_buffer(&probe_copychain_image_to_buffer());
                stream.finish(PROBE_COPYCHAIN_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_COPYCHAIN_COMMAND_BUFFER]));
                stream.request_readback(
                    PROBE_COPYCHAIN_READBACK,
                    &ReadbackDesc {
                        label: Some("crcbl-webgpu copychain readback"),
                        buffer: PROBE_COPYCHAIN_HOST_BUFFER,
                        offset: 0,
                        size: probe_copychain_host_buffer_desc().size,
                        after: None,
                    },
                )
            })
            .is_some();
        if encoded {
            self.copychain = CopyChainProbe::Requested;
            self.copychain_reason.clear();
        }
        encoded
    }

    /// Encode one [`poll_readback`](crate::StreamWriter::poll_readback) for the
    /// copy chain's readback and register its wait, unless it is already waiting
    /// or ready — [`poll_compute`](Self::poll_compute)'s protocol on the copy
    /// chain's handle.
    fn poll_copychain(&mut self) -> bool {
        if !matches!(
            self.copychain,
            CopyChainProbe::Requested | CopyChainProbe::Pending
        ) {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(sequence) =
            channel.encode_awaited(|stream| stream.poll_readback(PROBE_COPYCHAIN_READBACK))
        else {
            return false;
        };
        self.copychain = CopyChainProbe::Waiting { sequence };
        true
    }

    /// Drain, absorb, and report where the copy chain's readback has got to.
    fn copychain_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.copychain_reason = error.to_string();
            return COPYCHAIN_UNDECODABLE;
        }
        match &self.copychain {
            CopyChainProbe::Unasked => COPYCHAIN_UNASKED,
            CopyChainProbe::Requested => COPYCHAIN_REQUESTED,
            CopyChainProbe::Waiting { .. } => COPYCHAIN_WAITING,
            CopyChainProbe::Pending => COPYCHAIN_PENDING,
            CopyChainProbe::Ready { .. } => COPYCHAIN_READY,
        }
    }

    /// The bytes the copy chain's readback came back with, or an empty slice if
    /// it has not.
    fn copychain_bytes(&self) -> &[u8] {
        match &self.copychain {
            CopyChainProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// Encode the fill setup frame: a dispatch that fills a storage buffer with
    /// the pattern, a zero [`fill_buffer`](crate::StreamWriter::fill_buffer) over
    /// its first half, and the copy to a host buffer that is read back.
    ///
    /// **One frame, many commands, no reply** — [`request_compute`](Self::request_compute)'s
    /// shape with a `fill_buffer` recorded on the encoder after the compute pass
    /// closes and before the buffer→buffer copy. `false` until a device has
    /// opened, that method's ordering rule.
    fn request_fill(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_buffer(PROBE_FILL_STORAGE_BUFFER, &probe_fill_storage_buffer_desc());
                stream.create_buffer(PROBE_FILL_HOST_BUFFER, &probe_fill_host_buffer_desc());
                stream
                    .create_shader_module(PROBE_FILL_SHADER_MODULE, &PROBE_FILL_SHADER_MODULE_DESC);
                stream.create_bind_group_layout(
                    PROBE_FILL_BIND_GROUP_LAYOUT,
                    &PROBE_FILL_BIND_GROUP_LAYOUT_DESC,
                );
                stream.create_bind_group(PROBE_FILL_BIND_GROUP, &PROBE_FILL_BIND_GROUP_DESC);
                stream.create_pipeline_layout(
                    PROBE_FILL_PIPELINE_LAYOUT,
                    &PROBE_FILL_PIPELINE_LAYOUT_DESC,
                );
                stream.create_compute_pipeline(PROBE_FILL_PIPELINE, &PROBE_FILL_PIPELINE_DESC);
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu fill encoder"),
                    queue: PROBE_FILL_QUEUE,
                });
                stream.begin_compute_pass(&ComputePassDesc {
                    label: Some("crcbl-webgpu fill pass"),
                });
                stream.bind_compute_pipeline(PROBE_FILL_PIPELINE);
                stream.bind_group(0, PROBE_FILL_BIND_GROUP, &[], PROBE_FILL_PIPELINE_LAYOUT);
                stream.dispatch(1, 1, 1);
                stream.end_compute_pass();
                stream.fill_buffer(PROBE_FILL_STORAGE_BUFFER, 0, PROBE_FILL_ZEROED_BYTES, 0);
                stream.copy_buffer_to_buffer(&probe_fill_copy());
                stream.finish(PROBE_FILL_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_FILL_COMMAND_BUFFER]));
                stream.request_readback(
                    PROBE_FILL_READBACK,
                    &ReadbackDesc {
                        label: Some("crcbl-webgpu fill readback"),
                        buffer: PROBE_FILL_HOST_BUFFER,
                        offset: 0,
                        size: probe_fill_host_buffer_desc().size,
                        after: None,
                    },
                )
            })
            .is_some();
        if encoded {
            self.fill = FillProbe::Requested;
            self.fill_reason.clear();
        }
        encoded
    }

    /// Encode one [`poll_readback`](crate::StreamWriter::poll_readback) for the
    /// fill probe's readback and register its wait, unless it is already waiting
    /// or ready — [`poll_compute`](Self::poll_compute)'s protocol on the fill
    /// probe's handle.
    fn poll_fill(&mut self) -> bool {
        if !matches!(self.fill, FillProbe::Requested | FillProbe::Pending) {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(sequence) =
            channel.encode_awaited(|stream| stream.poll_readback(PROBE_FILL_READBACK))
        else {
            return false;
        };
        self.fill = FillProbe::Waiting { sequence };
        true
    }

    /// Drain, absorb, and report where the fill probe's readback has got to.
    fn fill_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.fill_reason = error.to_string();
            return FILL_UNDECODABLE;
        }
        match &self.fill {
            FillProbe::Unasked => FILL_UNASKED,
            FillProbe::Requested => FILL_REQUESTED,
            FillProbe::Waiting { .. } => FILL_WAITING,
            FillProbe::Pending => FILL_PENDING,
            FillProbe::Ready { .. } => FILL_READY,
        }
    }

    /// The bytes the fill probe's readback came back with, or an empty slice if
    /// it has not.
    fn fill_bytes(&self) -> &[u8] {
        match &self.fill {
            FillProbe::Ready { bytes } => bytes,
            _ => &[],
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

    /// Ask the page to make a pipeline layout on the device it opened.
    ///
    /// `1` when the frame — a bind-group layout and the
    /// [`CreatePipelineLayout`](crate::Command::CreatePipelineLayout) built from
    /// it — is on the stream; `0` on
    /// [`__crcbl_web_gpu_probe_shader_module`]'s three conditions.
    ///
    /// **No `state` beside it and no arguments either**, exactly as its
    /// neighbours have none and for the same two reasons: nothing answers a
    /// creation, and a `GPUPipelineLayout` reports its `label` and nothing else —
    /// not its bind-group layouts, not its push-constant ranges — so a number
    /// chosen by the page could not be read back off the object. The descriptor
    /// is fixed in `crates/crcbl-webgpu/src/probe.rs`, with `push_constants:
    /// None` so it *builds* rather than being refused; the `Some` refusal is the
    /// corpus's to drive.
    ///
    /// **What is new is that this one export encodes a whole *frame*.** A
    /// pipeline layout names a live bind-group layout, so wasm records the layout
    /// before the pipeline layout — and the pipeline layout resolves that layout
    /// out of a table keyed by handle bits it shares with the pipeline layout's
    /// own id, which is what puts the set-index resolution in front of a real
    /// `createPipelineLayout`. `crcbl.gpu.replayer.pipelineLayouts` is the table
    /// the `GPUPipelineLayout` lands in.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_pipeline_layout() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_pipeline_layout()),
            Err(_) => 0,
        })
    }

    /// Ask the page to make a compute pipeline on the device it opened.
    ///
    /// `1` when the frame — a compute shader module, an empty pipeline layout, and
    /// the [`CreateComputePipeline`](crate::Command::CreateComputePipeline) built
    /// from both — is on the stream; `0` on
    /// [`__crcbl_web_gpu_probe_pipeline_layout`]'s three conditions.
    ///
    /// **No `state` beside it and no arguments either**, exactly as its neighbours
    /// have none and for the same two reasons: nothing answers a creation, and a
    /// `GPUComputePipeline` reports its `label` and — unlike every object before it
    /// — `getBindGroupLayout(n)`, neither of which a page could have chosen. The
    /// descriptor is fixed in `crates/crcbl-webgpu/src/probe.rs`.
    ///
    /// **What is new is that the pipeline resolves handles into two *different*
    /// tables.** It records a compute shader module and a pipeline layout before
    /// itself, then resolves one id out of the shader-module table and one out of
    /// the pipeline-layout table — the first command anywhere to do that, which is
    /// what puts it in front of a real `createComputePipeline`.
    /// `crcbl.gpu.replayer.computePipelines` is the table the `GPUComputePipeline`
    /// lands in.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_compute_pipeline() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_compute_pipeline()),
            Err(_) => 0,
        })
    }

    /// Ask the page to make a render pipeline on the device it opened.
    ///
    /// `1` when the frame — a vertex-plus-fragment shader module, an empty
    /// pipeline layout, and the
    /// [`CreateGraphicsPipeline`](crate::Command::CreateGraphicsPipeline) built
    /// from both — is on the stream; `0` on
    /// [`__crcbl_web_gpu_probe_compute_pipeline`]'s three conditions.
    ///
    /// **No `state` beside it and no arguments either**, exactly as its neighbours
    /// have none and for the same two reasons: nothing answers a creation, and a
    /// `GPURenderPipeline` reports its `label` and — like a compute pipeline —
    /// `getBindGroupLayout(n)`, neither of which a page could have chosen. The
    /// descriptor is fixed in `crates/crcbl-webgpu/src/probe.rs`.
    ///
    /// **It is the largest descriptor on the seam**, and the point of this export
    /// is to put its whole nested tree — the primitive state, the reversed-Z
    /// depth-stencil, the multisample state, and the blended colour target — in
    /// front of a real `createRenderPipeline`, where a stride wrong by a byte
    /// anywhere in it would build a different pipeline or none.
    /// `crcbl.gpu.replayer.graphicsPipelines` is the table the `GPURenderPipeline`
    /// lands in.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_graphics_pipeline() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_graphics_pipeline()),
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

    /// Ask the page to clear a texture and start reading it back on the device
    /// it opened.
    ///
    /// `1` when the setup frame — image, view, host buffer, encoder, a
    /// clear-only render pass, the copy, finish, submit and `request_readback` —
    /// is on the stream; `0` when no device has opened yet, the probe is
    /// re-entered, or another channel is installed.
    ///
    /// **This is the decisive observation point of the whole track**: it is the
    /// first command that puts *rendered pixels* into host memory, and
    /// [`__crcbl_web_gpu_probe_readback_state`] plus
    /// [`__crcbl_web_gpu_probe_readback_bytes_ptr`] are how the gate reads them
    /// back to prove they are the clear colour. A stub cannot forge them.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_readback() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_readback()),
            Err(_) => 0,
        })
    }

    /// Poll the in-flight readback, once, on the reply channel.
    ///
    /// `1` when a [`poll_readback`](crate::StreamWriter::poll_readback) is on the
    /// stream with its wait registered; `0` when there is nothing to poll for —
    /// no readback requested, a poll already unanswered, or the bytes already in
    /// — or when the channel would not take it.
    ///
    /// Called each frame after [`__crcbl_web_gpu_probe_readback`]: it is a no-op
    /// until the previous poll is answered, so the gate can call it blindly.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_readback_poll() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.poll_readback()),
            Err(_) => 0,
        })
    }

    /// Drain the replies and report where the readback has got to — one of the
    /// `READBACK_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_readback_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.readback_state(),
            Err(_) => super::READBACK_UNASKED,
        })
    }

    /// A pointer into wasm memory to the bytes the readback came back with.
    ///
    /// Read [`__crcbl_web_gpu_probe_readback_bytes_len`] bytes from here, and
    /// only once [`__crcbl_web_gpu_probe_readback_state`] has answered
    /// [`READBACK_READY`](super::READBACK_READY): before that the length is `0`
    /// and this points at an empty buffer. Nothing here grows wasm memory — the
    /// bytes were allocated when the reply was decoded — so the pointer is stable
    /// until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_readback_bytes_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.readback_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_readback_bytes_ptr`] points at — the
    /// readback's length, or `0` if it has not answered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_readback_bytes_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.readback_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Ask the page to draw a red triangle over a clear and start reading it back
    /// on the device it opened.
    ///
    /// `1` when the setup frame — the pipeline's three resources, image, view,
    /// host buffer, encoder, a render pass that clears then binds and draws, the
    /// copy, finish, submit and `request_readback` — is on the stream; `0` when no
    /// device has opened yet, the probe is re-entered, or another channel is
    /// installed.
    ///
    /// **This is the decisive observation point of the draw arms**: the readback
    /// probe proves a clear reaches host memory, and this proves a `setPipeline` +
    /// `draw` overwrites that clear — its bytes are the fragment's colour, not the
    /// clear's, and a stub that skips the draw cannot forge them.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_draw() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_draw()),
            Err(_) => 0,
        })
    }

    /// Poll the draw's in-flight readback, once, on the reply channel.
    ///
    /// `1` when a [`poll_readback`](crate::StreamWriter::poll_readback) is on the
    /// stream with its wait registered; `0` when there is nothing to poll for — no
    /// draw requested, a poll already unanswered, or the bytes already in — or when
    /// the channel would not take it. A no-op until the previous poll is answered,
    /// so the gate can call it blindly each frame.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_draw_poll() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.poll_draw()),
            Err(_) => 0,
        })
    }

    /// Drain the replies and report where the draw readback has got to — one of
    /// the `DRAW_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_draw_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.draw_state(),
            Err(_) => super::DRAW_UNASKED,
        })
    }

    /// A pointer into wasm memory to the bytes the draw readback came back with.
    ///
    /// Read [`__crcbl_web_gpu_probe_draw_bytes_len`] bytes from here, and only once
    /// [`__crcbl_web_gpu_probe_draw_state`] has answered [`DRAW_READY`](super::DRAW_READY):
    /// before that the length is `0` and this points at an empty buffer. Nothing
    /// here grows wasm memory, so the pointer is stable until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_draw_bytes_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.draw_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_draw_bytes_ptr`] points at — the draw
    /// readback's length, or `0` if it has not answered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_draw_bytes_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.draw_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Ask the page to run a compute dispatch that writes a known pattern into a
    /// storage buffer and start reading it back on the device it opened.
    ///
    /// `1` when the setup frame — the two buffers, the pipeline's four resources,
    /// an encoder, a compute pass that binds and dispatches, the copy, finish,
    /// submit and `request_readback` — is on the stream; `0` when no device has
    /// opened yet, the probe is re-entered, or another channel is installed.
    ///
    /// **This is the decisive observation point of the dispatch arms**: a fresh
    /// WebGPU buffer is zero-initialised, so a readback of
    /// [`PROBE_DISPATCH_PATTERN`](super::PROBE_DISPATCH_PATTERN) can only come from
    /// a `dispatchWorkgroups` that actually ran — a stub that skips the dispatch
    /// reads back zeros.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_compute() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_compute()),
            Err(_) => 0,
        })
    }

    /// Poll the dispatch's in-flight readback, once, on the reply channel.
    ///
    /// `1` when a [`poll_readback`](crate::StreamWriter::poll_readback) is on the
    /// stream with its wait registered; `0` when there is nothing to poll for — no
    /// dispatch requested, a poll already unanswered, or the bytes already in — or
    /// when the channel would not take it. A no-op until the previous poll is
    /// answered, so the gate can call it blindly each frame.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_compute_poll() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.poll_compute()),
            Err(_) => 0,
        })
    }

    /// Drain the replies and report where the dispatch readback has got to — one
    /// of the `COMPUTE_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_compute_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.compute_state(),
            Err(_) => super::COMPUTE_UNASKED,
        })
    }

    /// A pointer into wasm memory to the bytes the dispatch readback came back
    /// with.
    ///
    /// Read [`__crcbl_web_gpu_probe_compute_bytes_len`] bytes from here, and only
    /// once [`__crcbl_web_gpu_probe_compute_state`] has answered
    /// [`COMPUTE_READY`](super::COMPUTE_READY): before that the length is `0` and
    /// this points at an empty buffer. Nothing here grows wasm memory, so the
    /// pointer is stable until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_compute_bytes_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.compute_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_compute_bytes_ptr`] points at — the
    /// dispatch readback's length, or `0` if it has not answered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_compute_bytes_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.compute_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Ask the page to run the copy chain — a dispatch that fills a storage
    /// buffer red, a buffer→image copy into a texture, an image→image copy to a
    /// second texture, and an image→buffer copy out to a host buffer — and start
    /// reading it back on the device it opened.
    ///
    /// `1` when the setup frame is on the stream; `0` when no device has opened
    /// yet, the probe is re-entered, or another channel is installed.
    ///
    /// **This observes both new copies at once**: a fresh WebGPU texture is
    /// zero-initialised, so a red read-back can only come from the buffer→image
    /// AND image→image copies both running — a stub that skips either reads back
    /// zeros.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_copychain() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_copychain()),
            Err(_) => 0,
        })
    }

    /// Poll the copy chain's in-flight readback, once, on the reply channel.
    ///
    /// `1` when a [`poll_readback`](crate::StreamWriter::poll_readback) is on the
    /// stream with its wait registered; `0` when there is nothing to poll for. A
    /// no-op until the previous poll is answered, so the gate can call it blindly.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_copychain_poll() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.poll_copychain()),
            Err(_) => 0,
        })
    }

    /// Drain the replies and report where the copy chain's readback has got to —
    /// one of the `COPYCHAIN_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_copychain_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.copychain_state(),
            Err(_) => super::COPYCHAIN_UNASKED,
        })
    }

    /// A pointer into wasm memory to the bytes the copy chain's readback came back
    /// with.
    ///
    /// Read [`__crcbl_web_gpu_probe_copychain_bytes_len`] bytes from here, and
    /// only once [`__crcbl_web_gpu_probe_copychain_state`] has answered
    /// [`COPYCHAIN_READY`](super::COPYCHAIN_READY). Nothing here grows wasm
    /// memory, so the pointer is stable until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_copychain_bytes_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.copychain_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_copychain_bytes_ptr`] points at —
    /// the copy chain's readback length, or `0` if it has not answered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_copychain_bytes_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.copychain_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Ask the page to run the fill probe — a dispatch that fills a storage buffer
    /// with a pattern, a zero [`fill_buffer`](crate::StreamWriter::fill_buffer)
    /// over its first half, and a copy to a host buffer — and start reading it
    /// back on the device it opened.
    ///
    /// `1` when the setup frame is on the stream; `0` when no device has opened
    /// yet, the probe is re-entered, or another channel is installed.
    ///
    /// **This observes `clearBuffer` zeroing exactly its sub-range**: the dispatch
    /// writes the whole buffer, the fill zeroes only the first half, so the read
    /// back is zero there and the pattern beyond — a stub `clearBuffer` leaves the
    /// pattern in the half that should be zero.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_fill() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_fill()),
            Err(_) => 0,
        })
    }

    /// Poll the fill probe's in-flight readback, once, on the reply channel.
    ///
    /// `1` when a [`poll_readback`](crate::StreamWriter::poll_readback) is on the
    /// stream with its wait registered; `0` when there is nothing to poll for. A
    /// no-op until the previous poll is answered, so the gate can call it blindly.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_fill_poll() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.poll_fill()),
            Err(_) => 0,
        })
    }

    /// Drain the replies and report where the fill probe's readback has got to —
    /// one of the `FILL_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_fill_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.fill_state(),
            Err(_) => super::FILL_UNASKED,
        })
    }

    /// A pointer into wasm memory to the bytes the fill probe's readback came back
    /// with.
    ///
    /// Read [`__crcbl_web_gpu_probe_fill_bytes_len`] bytes from here, and only
    /// once [`__crcbl_web_gpu_probe_fill_state`] has answered
    /// [`FILL_READY`](super::FILL_READY). Nothing here grows wasm memory, so the
    /// pointer is stable until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_fill_bytes_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.fill_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_fill_bytes_ptr`] points at — the
    /// fill probe's readback length, or `0` if it has not answered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_fill_bytes_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.fill_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Ask the page to present a frame to the canvas `canvas_id` names and start
    /// reading it back on the device it opened.
    ///
    /// `1` when the setup frame — the surface, the configured swapchain, the
    /// acquired frame, the host buffer, an encoder, a pass that clears the acquired
    /// view red, the copy, finish, submit, present and `request_readback` — is on
    /// the stream; `0` when no device has opened yet, the probe is re-entered, or
    /// another channel is installed.
    ///
    /// **This is the decisive observation point of the present arm, and the first
    /// proof the real canvas-context path works end to end**: a stub that skips the
    /// configure/acquire/render leaves a black/zero canvas, so reading back
    /// [`PROBE_PRESENT_COLOR_BYTES`](super::PROBE_PRESENT_COLOR_BYTES) can only come
    /// from a `configure` + `getCurrentTexture` + render + copy that actually ran.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_present(canvas_id: u32) -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_present(canvas_id)),
            Err(_) => 0,
        })
    }

    /// Poll the present probe's in-flight readback, once, on the reply channel.
    ///
    /// `1` when a [`poll_readback`](crate::StreamWriter::poll_readback) is on the
    /// stream with its wait registered; `0` when there is nothing to poll for. A
    /// no-op until the previous poll is answered, so the gate can call it blindly.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_present_poll() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.poll_present()),
            Err(_) => 0,
        })
    }

    /// Drain the replies and report where the present probe's readback has got to
    /// — one of the `PRESENT_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_present_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.present_state(),
            Err(_) => super::PRESENT_UNASKED,
        })
    }

    /// A pointer into wasm memory to the bytes the present probe's readback came
    /// back with.
    ///
    /// Read [`__crcbl_web_gpu_probe_present_bytes_len`] bytes from here, and only
    /// once [`__crcbl_web_gpu_probe_present_state`] has answered
    /// [`PRESENT_READY`](super::PRESENT_READY). Nothing here grows wasm memory, so
    /// the pointer is stable until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_present_bytes_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.present_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_present_bytes_ptr`] points at — the
    /// present probe's readback length, or `0` if it has not answered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_present_bytes_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.present_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
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
        __crcbl_web_gpu_probe_buffer, __crcbl_web_gpu_probe_compute,
        __crcbl_web_gpu_probe_compute_bytes_len, __crcbl_web_gpu_probe_compute_bytes_ptr,
        __crcbl_web_gpu_probe_compute_pipeline, __crcbl_web_gpu_probe_compute_poll,
        __crcbl_web_gpu_probe_compute_state, __crcbl_web_gpu_probe_copychain,
        __crcbl_web_gpu_probe_copychain_bytes_len, __crcbl_web_gpu_probe_copychain_bytes_ptr,
        __crcbl_web_gpu_probe_copychain_poll, __crcbl_web_gpu_probe_copychain_state,
        __crcbl_web_gpu_probe_device, __crcbl_web_gpu_probe_device_features_hi,
        __crcbl_web_gpu_probe_device_features_lo, __crcbl_web_gpu_probe_device_max_image_2d,
        __crcbl_web_gpu_probe_device_reason_len, __crcbl_web_gpu_probe_device_reason_ptr,
        __crcbl_web_gpu_probe_device_state, __crcbl_web_gpu_probe_draw,
        __crcbl_web_gpu_probe_draw_bytes_len, __crcbl_web_gpu_probe_draw_bytes_ptr,
        __crcbl_web_gpu_probe_draw_poll, __crcbl_web_gpu_probe_draw_state,
        __crcbl_web_gpu_probe_features_hi, __crcbl_web_gpu_probe_features_lo,
        __crcbl_web_gpu_probe_fill, __crcbl_web_gpu_probe_fill_bytes_len,
        __crcbl_web_gpu_probe_fill_bytes_ptr, __crcbl_web_gpu_probe_fill_poll,
        __crcbl_web_gpu_probe_fill_state, __crcbl_web_gpu_probe_graphics_pipeline,
        __crcbl_web_gpu_probe_image, __crcbl_web_gpu_probe_image_view,
        __crcbl_web_gpu_probe_max_image_2d, __crcbl_web_gpu_probe_pipeline_layout,
        __crcbl_web_gpu_probe_present, __crcbl_web_gpu_probe_present_bytes_len,
        __crcbl_web_gpu_probe_present_bytes_ptr, __crcbl_web_gpu_probe_present_poll,
        __crcbl_web_gpu_probe_present_state, __crcbl_web_gpu_probe_sampler,
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

    /// The pipeline layout shares its bits with everything else, so eight kinds
    /// now stand on one index and one generation, distinguished only by the
    /// opcode — and here two of those eight, the pipeline layout and the
    /// bind-group layout it is built from, are alive at once.
    #[test]
    fn the_probes_pipeline_layout_names_the_same_handle_bits_as_everything_else() {
        assert_eq!(PROBE_PIPELINE_LAYOUT.to_bits(), PROBE_BIND_GROUP.to_bits());
        assert_eq!(
            PROBE_PIPELINE_LAYOUT.to_bits(),
            PROBE_BIND_GROUP_LAYOUT.to_bits()
        );
    }

    /// The pipeline-layout half: **one export, a whole frame.** A pipeline layout
    /// names a live bind-group layout, so the export records the layout before
    /// the pipeline layout, and the pipeline layout carries that layout's handle
    /// in its set list.
    #[test]
    fn the_pipeline_layout_export_encodes_the_layout_and_the_pipeline_layout() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_pipeline_layout(), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec!["CreateBindGroupLayout", "CreatePipelineLayout"],
            "the frame builds the bind-group layout before the pipeline layout"
        );
        assert_eq!(
            commands.last(),
            Some(&Command::CreatePipelineLayout {
                layout: PROBE_PIPELINE_LAYOUT,
                label: Some("crcbl-webgpu probe pipeline layout".into()),
                bind_group_layouts: PROBE_PIPELINE_LAYOUT_BIND_GROUP_LAYOUTS.to_vec(),
                push_constants: None,
            })
        );
    }

    /// **Nothing waits on the frame**, for the image pair's reason: every command
    /// in it is a creation, and a creation is answered by nothing.
    #[test]
    fn the_pipeline_layout_request_registers_no_wait_because_nothing_answers_it() {
        open_device();
        let before = waiting_replies();
        assert_eq!(__crcbl_web_gpu_probe_pipeline_layout(), 1);
        assert_eq!(waiting_replies(), before);
        assert_eq!(take_frame().len(), 2);
    }

    /// **A device has to have opened first**, for the sampler export's reason:
    /// both commands the frame carries are device methods.
    #[test]
    fn a_pipeline_layout_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_pipeline_layout(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);

        grant(&granted("no device yet"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_pipeline_layout(), 0);
        assert_eq!(take_frame().len(), 1);
    }

    /// The compute pipeline shares its bits with everything else, so nine kinds
    /// now stand on one index and one generation, distinguished only by the
    /// opcode — and here three of those nine, the pipeline and the shader module
    /// and pipeline layout it is built from, are alive at once.
    #[test]
    fn the_probes_compute_pipeline_names_the_same_handle_bits_as_everything_else() {
        assert_eq!(
            PROBE_COMPUTE_PIPELINE.to_bits(),
            PROBE_PIPELINE_LAYOUT.to_bits()
        );
        assert_eq!(
            PROBE_COMPUTE_PIPELINE.to_bits(),
            PROBE_SHADER_MODULE.to_bits()
        );
    }

    /// The compute-pipeline half: **one export, a whole frame.** A compute
    /// pipeline names a live shader module and a live pipeline layout, so the
    /// export records the compute shader and the empty pipeline layout before the
    /// pipeline — and the pipeline carries one handle for its layout and one for
    /// its compute module, into two *different* tables.
    #[test]
    fn the_compute_pipeline_export_encodes_the_shader_layout_and_the_pipeline() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_compute_pipeline(), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateShaderModule",
                "CreatePipelineLayout",
                "CreateComputePipeline",
            ],
            "the frame builds the shader module and pipeline layout before the pipeline"
        );
        assert_eq!(
            commands.last(),
            Some(&Command::CreateComputePipeline {
                pipeline: PROBE_COMPUTE_PIPELINE,
                label: Some("crcbl-webgpu probe compute pipeline".into()),
                layout: PROBE_PIPELINE_LAYOUT,
                module: PROBE_SHADER_MODULE,
                entry_point: "main".into(),
                workgroup_size: [1, 1, 1],
            })
        );
    }

    /// **Nothing waits on the frame**, for the image pair's reason: every command
    /// in it is a creation, and a creation is answered by nothing.
    #[test]
    fn the_compute_pipeline_request_registers_no_wait_because_nothing_answers_it() {
        open_device();
        let before = waiting_replies();
        assert_eq!(__crcbl_web_gpu_probe_compute_pipeline(), 1);
        assert_eq!(waiting_replies(), before);
        assert_eq!(take_frame().len(), 3);
    }

    /// **A device has to have opened first**, for the sampler export's reason:
    /// every command the frame carries is a device method.
    #[test]
    fn a_compute_pipeline_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_compute_pipeline(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);

        grant(&granted("no device yet"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_compute_pipeline(), 0);
        assert_eq!(take_frame().len(), 1);
    }

    /// The graphics pipeline names the same handle bits as everything else, for
    /// the compute pipeline's reason: it, its shader module and its pipeline
    /// layout are alive at once and a page filing them under one key would be a
    /// replayer with one table where the crate docs require several.
    #[test]
    fn the_probes_graphics_pipeline_names_the_same_handle_bits_as_everything_else() {
        assert_eq!(
            PROBE_GRAPHICS_PIPELINE.to_bits(),
            PROBE_PIPELINE_LAYOUT.to_bits()
        );
        assert_eq!(
            PROBE_GRAPHICS_PIPELINE.to_bits(),
            PROBE_SHADER_MODULE.to_bits()
        );
    }

    /// The graphics-pipeline half: **one export, a whole frame.** A raster
    /// pipeline names a live shader module and a live pipeline layout, so the
    /// export records the vertex-plus-fragment shader and the empty pipeline
    /// layout before the pipeline — and the pipeline names its layout once and its
    /// module twice (vertex and fragment), all resolving out of tables the two
    /// creations before it fill.
    #[test]
    fn the_graphics_pipeline_export_encodes_the_shader_layout_and_the_pipeline() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_graphics_pipeline(), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateShaderModule",
                "CreatePipelineLayout",
                "CreateGraphicsPipeline",
            ],
            "the frame builds the shader module and pipeline layout before the pipeline"
        );
        assert_eq!(
            commands.last(),
            Some(&Command::CreateGraphicsPipeline {
                pipeline: PROBE_GRAPHICS_PIPELINE,
                label: Some("crcbl-webgpu probe raster pipeline".into()),
                layout: PROBE_PIPELINE_LAYOUT,
                vertex_module: PROBE_SHADER_MODULE,
                vertex_entry_point: "vsMain".into(),
                fragment: Some((PROBE_SHADER_MODULE, "fsMain".into())),
                primitive: PROBE_GRAPHICS_PIPELINE_DESC.primitive,
                depth_stencil: PROBE_GRAPHICS_PIPELINE_DESC.depth_stencil,
                multisample: PROBE_GRAPHICS_PIPELINE_DESC.multisample,
                color_targets: PROBE_GRAPHICS_COLOR_TARGETS.to_vec(),
            })
        );
    }

    /// **Nothing waits on the frame**, for the compute pipeline's reason: every
    /// command in it is a creation, and a creation is answered by nothing.
    #[test]
    fn the_graphics_pipeline_request_registers_no_wait_because_nothing_answers_it() {
        open_device();
        let before = waiting_replies();
        assert_eq!(__crcbl_web_gpu_probe_graphics_pipeline(), 1);
        assert_eq!(waiting_replies(), before);
        assert_eq!(take_frame().len(), 3);
    }

    /// **A device has to have opened first**, for the compute pipeline's reason:
    /// every command the frame carries is a device method.
    #[test]
    fn a_graphics_pipeline_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_graphics_pipeline(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);

        grant(&granted("no device yet"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_graphics_pipeline(), 0);
        assert_eq!(take_frame().len(), 1);
    }

    /// The draw probe's bytes, read the way JS reads them.
    fn draw_bytes() -> Vec<u8> {
        let len = __crcbl_web_gpu_probe_draw_bytes_len() as usize;
        let ptr = __crcbl_web_gpu_probe_draw_bytes_ptr();
        if len == 0 {
            return Vec::new();
        }
        assert!(!ptr.is_null(), "the draw answered a length with no pointer");
        // SAFETY: `ptr` and `len` are this thread's `Probe::draw` bytes, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        bytes.to_vec()
    }

    /// **Every draw handle is a generation past every other probe's**, which is
    /// the whole point of `2 << 32`: the draw frame has an image, a view, a
    /// buffer, a shader module, a pipeline layout, a pipeline, a command buffer, a
    /// queue and a readback all live at once, and none of them may land in the
    /// slot the readback or graphics-pipeline probe (both at `1 << 32`) files its
    /// own resources under in the shared page.
    #[test]
    fn the_draw_handles_are_a_generation_past_every_other_probe() {
        for bits in [
            PROBE_DRAW_IMAGE.to_bits(),
            PROBE_DRAW_IMAGE_VIEW.to_bits(),
            PROBE_DRAW_BUFFER.to_bits(),
            PROBE_DRAW_SHADER_MODULE.to_bits(),
            PROBE_DRAW_PIPELINE_LAYOUT.to_bits(),
            PROBE_DRAW_PIPELINE.to_bits(),
            PROBE_DRAW_COMMAND_BUFFER.to_bits(),
            PROBE_DRAW_QUEUE.to_bits(),
            PROBE_DRAW_READBACK.to_bits(),
        ] {
            assert_eq!(bits, 2 << 32, "every draw handle is generation two");
        }
        // The graphics-pipeline probe (group R) and the readback probe (group S)
        // both sit at `1 << 32`, so the draw probe is one generation clear of both.
        assert_ne!(
            PROBE_DRAW_PIPELINE.to_bits(),
            PROBE_GRAPHICS_PIPELINE.to_bits()
        );
        assert_ne!(PROBE_DRAW_IMAGE.to_bits(), PROBE_IMAGE.to_bits());
        assert_ne!(PROBE_DRAW_READBACK.to_bits(), PROBE_READBACK.to_bits());
    }

    /// The draw half: **one export, a whole frame** that clears and then draws.
    /// The pipeline's three resources come first, then the encoder and a render
    /// pass that clears — and, unlike the readback probe's clear-only pass, binds
    /// the pipeline and draws before the copy, the finish, the submit and the
    /// `request_readback` that the poll will chase.
    #[test]
    fn the_draw_export_encodes_the_pipeline_the_clear_the_bind_and_the_draw() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_draw(), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateImage",
                "CreateImageView",
                "CreateBuffer",
                "CreateShaderModule",
                "CreatePipelineLayout",
                "CreateGraphicsPipeline",
                "CreateCommandEncoder",
                "BeginRenderPass",
                "BindGraphicsPipeline",
                "Draw",
                "EndRenderPass",
                "CopyImageToBuffer",
                "Finish",
                "Submit",
                "RequestReadback",
            ],
            "the frame builds the pipeline, then clears, binds, draws and reads back"
        );
        // The two commands the draw arms add inside the pass, verbatim: the bind
        // names the draw pipeline, and the draw is three vertices of one instance
        // — the fullscreen triangle.
        assert!(commands.contains(&Command::BindGraphicsPipeline {
            pipeline: PROBE_DRAW_PIPELINE,
        }));
        assert!(commands.contains(&Command::Draw {
            vertices: 0..3,
            instances: 0..1,
        }));
    }

    /// **Nothing waits on the setup frame**: every command in it is caller-
    /// allocated and answered by nothing, so the wait belongs to the poll, not
    /// here — a registered wait would hold a slot for a reply never coming.
    #[test]
    fn the_draw_setup_frame_registers_no_wait_because_the_poll_is_what_is_awaited() {
        open_device();
        let before = waiting_replies();
        assert_eq!(__crcbl_web_gpu_probe_draw(), 1);
        assert_eq!(waiting_replies(), before);
        assert_eq!(__crcbl_web_gpu_probe_draw_state(), DRAW_REQUESTED);
    }

    /// **A device has to have opened first**, the readback probe's ordering rule:
    /// every command the frame carries is a device method.
    #[test]
    fn a_draw_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_draw(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);
        assert_eq!(__crcbl_web_gpu_probe_draw_state(), DRAW_UNASKED);

        grant(&granted("no device yet"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_draw(), 0);
        assert_eq!(take_frame().len(), 1);
    }

    /// The whole draw exchange through the exports alone: request, poll, and a
    /// `ReadbackReady` carrying the drawn pixels — which reach the bytes exports
    /// as the draw colour. This is the browser gate's path with the replayer
    /// replaced by a `ReplyWriter`, as a `cargo test` has no `navigator.gpu`.
    #[test]
    fn the_draw_readback_reaches_the_bytes_exports_as_the_drawn_colour() {
        // `open_device` spends sequences 0 (the adapter) and 1 (the device), so
        // the setup frame starts at 2 and the poll that follows it is the command
        // after the frame's own — read the length off the frame rather than
        // hard-wiring it so a later command added to the frame does not silently
        // point the reply at the wrong sequence.
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_draw(), 1);
        let setup = take_frame();
        let poll_sequence = 2 + setup.len() as u64;
        assert_eq!(__crcbl_web_gpu_probe_draw_state(), DRAW_REQUESTED);

        assert_eq!(__crcbl_web_gpu_probe_draw_poll(), 1);
        assert_eq!(__crcbl_web_gpu_probe_draw_state(), DRAW_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::PollReadback {
                readback: PROBE_DRAW_READBACK,
            }]
        );

        let mut drawn = Vec::new();
        for _ in 0..(PROBE_READBACK_SIZE * PROBE_READBACK_SIZE) {
            drawn.extend_from_slice(&PROBE_DRAW_COLOR_BYTES);
        }
        let mut replies = ReplyWriter::new();
        replies.readback_ready(poll_sequence, PROBE_DRAW_READBACK, &drawn);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_draw_state(), DRAW_READY);
        assert_eq!(draw_bytes(), drawn);
        assert_eq!(&draw_bytes()[..4], PROBE_DRAW_COLOR_BYTES);
        // The draw colour is not the clear's byte form the pass loaded with —
        // the whole evidence the gate reads back from the browser. The draw
        // probe's clear shares the readback probe's channels, so its bytes are
        // `PROBE_READBACK_CLEAR_BYTES`.
        assert_ne!(PROBE_DRAW_COLOR_BYTES, PROBE_READBACK_CLEAR_BYTES);
    }

    /// A `ReadbackPending` for the poll's sequence drops the draw back to
    /// `Pending`, so the next frame polls again — [`DrawProbe::absorb`]'s pending
    /// arm, tested at the enum because the sequence is known there.
    #[test]
    fn a_readback_pending_reply_drops_the_draw_back_to_pending() {
        let mut draw = DrawProbe::Waiting { sequence: 7 };
        let advanced = draw.absorb(&[(
            7,
            Reply::ReadbackPending {
                readback: PROBE_DRAW_READBACK,
            },
        )]);
        assert!(advanced);
        assert_eq!(draw, DrawProbe::Pending);
    }

    /// A `ReadbackReady` for the poll's sequence carries the bytes into `Ready`.
    #[test]
    fn a_readback_ready_reply_carries_the_draw_bytes_into_ready() {
        let mut draw = DrawProbe::Waiting { sequence: 7 };
        let bytes = vec![255, 0, 0, 255];
        let advanced = draw.absorb(&[(
            7,
            Reply::ReadbackReady {
                readback: PROBE_DRAW_READBACK,
                data: bytes.clone(),
            },
        )]);
        assert!(advanced);
        assert_eq!(draw, DrawProbe::Ready { bytes });
    }

    /// A reply for another sequence leaves the draw waiting, exactly as it leaves
    /// every other probe: one channel carries every probe's replies, and each
    /// takes only its own.
    #[test]
    fn a_draw_probe_ignores_a_reply_for_another_sequence() {
        let mut draw = DrawProbe::Waiting { sequence: 7 };
        let advanced = draw.absorb(&[(
            8,
            Reply::ReadbackReady {
                readback: PROBE_DRAW_READBACK,
                data: vec![1, 2, 3, 4],
            },
        )]);
        assert!(!advanced);
        assert_eq!(draw, DrawProbe::Waiting { sequence: 7 });
    }

    /// The present probe's bytes, read the way JS reads them.
    fn present_bytes() -> Vec<u8> {
        let len = __crcbl_web_gpu_probe_present_bytes_len() as usize;
        let ptr = __crcbl_web_gpu_probe_present_bytes_ptr();
        if len == 0 {
            return Vec::new();
        }
        assert!(
            !ptr.is_null(),
            "the present answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::present` bytes, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        bytes.to_vec()
    }

    /// **Every present handle is a generation past every other probe's**, which is
    /// the whole point of `5 << 32`: the present frame has a surface, a swapchain,
    /// an image, a view, a buffer, a command buffer, a queue and a readback all
    /// live at once, and none of them may land in the slot the copy-chain or fill
    /// probe (both at `4 << 32`) files its own resources under in the shared page.
    #[test]
    fn the_present_handles_are_a_generation_past_every_other_probe() {
        for bits in [
            PROBE_PRESENT_SURFACE.to_bits(),
            PROBE_PRESENT_SWAPCHAIN.to_bits(),
            PROBE_PRESENT_IMAGE.to_bits(),
            PROBE_PRESENT_VIEW.to_bits(),
            PROBE_PRESENT_BUFFER.to_bits(),
            PROBE_PRESENT_COMMAND_BUFFER.to_bits(),
            PROBE_PRESENT_QUEUE.to_bits(),
            PROBE_PRESENT_READBACK.to_bits(),
        ] {
            assert_eq!(bits >> 32, 5, "every present handle is generation five");
        }
        // A generation clear of the copy-chain and fill probes (both `4 << 32`),
        // and of every earlier probe below that.
        assert_ne!(PROBE_PRESENT_IMAGE.to_bits(), PROBE_DRAW_IMAGE.to_bits());
        assert_ne!(
            PROBE_PRESENT_READBACK.to_bits(),
            PROBE_FILL_READBACK.to_bits()
        );
        assert_ne!(PROBE_PRESENT_SURFACE.to_bits(), PROBE_SURFACE.to_bits());
    }

    /// The present half: **one export, a whole frame** that creates a surface,
    /// configures a swapchain on it, acquires the frame, clears the *acquired*
    /// view, copies it out, submits, presents (a no-op) and reads back — the first
    /// probe to drive a real canvas context. The copy is recorded before the
    /// present, which is what makes the acquired frame observable.
    #[test]
    fn the_present_export_encodes_the_surface_swapchain_acquire_clear_and_present() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_present(7), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateSurface",
                "CreateSwapchain",
                "AcquireNextFrame",
                "CreateBuffer",
                "CreateCommandEncoder",
                "BeginRenderPass",
                "EndRenderPass",
                "CopyImageToBuffer",
                "Finish",
                "Submit",
                "Present",
                "RequestReadback",
            ],
            "the frame configures a canvas, acquires, clears, copies, submits, presents and reads back"
        );
        // The acquire binds the present probe's own image and view under the
        // swapchain, and the present names that swapchain with no waits — the
        // no-op the browser composites on rAF.
        assert!(commands.contains(&Command::AcquireNextFrame {
            swapchain: PROBE_PRESENT_SWAPCHAIN,
            image: PROBE_PRESENT_IMAGE,
            view: PROBE_PRESENT_VIEW,
        }));
        assert!(commands.contains(&Command::Present {
            swapchain: PROBE_PRESENT_SWAPCHAIN,
            waits: Vec::new(),
            present_id: None,
        }));
    }

    /// **Nothing waits on the setup frame**: every command in it is caller-
    /// allocated and answered by nothing — the present included, being a no-op —
    /// so the wait belongs to the poll, not here.
    #[test]
    fn the_present_setup_frame_registers_no_wait_because_the_poll_is_what_is_awaited() {
        open_device();
        let before = waiting_replies();
        assert_eq!(__crcbl_web_gpu_probe_present(7), 1);
        assert_eq!(waiting_replies(), before);
        assert_eq!(__crcbl_web_gpu_probe_present_state(), PRESENT_REQUESTED);
    }

    /// **A device has to have opened first**, the draw probe's ordering rule:
    /// every command after the surface is a device method.
    #[test]
    fn a_present_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_present(7), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);
        assert_eq!(__crcbl_web_gpu_probe_present_state(), PRESENT_UNASKED);

        grant(&granted("no device yet"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_present(7), 0);
        assert_eq!(take_frame().len(), 1);
    }

    /// The whole present exchange through the exports alone: request, poll, and a
    /// `ReadbackReady` carrying the presented pixels — which reach the bytes
    /// exports as the present colour. The browser gate's path with the replayer
    /// replaced by a `ReplyWriter`, as a `cargo test` has no `navigator.gpu`.
    #[test]
    fn the_present_readback_reaches_the_bytes_exports_as_the_present_colour() {
        // `open_device` spends sequences 0 (the adapter) and 1 (the device), so
        // the setup frame starts at 2 and the poll that follows it is the command
        // after the frame's own — read the length off the frame rather than
        // hard-wiring it.
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_present(7), 1);
        let setup = take_frame();
        let poll_sequence = 2 + setup.len() as u64;
        assert_eq!(__crcbl_web_gpu_probe_present_state(), PRESENT_REQUESTED);

        assert_eq!(__crcbl_web_gpu_probe_present_poll(), 1);
        assert_eq!(__crcbl_web_gpu_probe_present_state(), PRESENT_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::PollReadback {
                readback: PROBE_PRESENT_READBACK,
            }]
        );

        let mut presented = Vec::new();
        for _ in 0..(PROBE_READBACK_SIZE * PROBE_READBACK_SIZE) {
            presented.extend_from_slice(&PROBE_PRESENT_COLOR_BYTES);
        }
        let mut replies = ReplyWriter::new();
        replies.readback_ready(poll_sequence, PROBE_PRESENT_READBACK, &presented);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_present_state(), PRESENT_READY);
        assert_eq!(present_bytes(), presented);
        assert_eq!(&present_bytes()[..4], PROBE_PRESENT_COLOR_BYTES);
    }

    /// A `ReadbackPending` for the poll's sequence drops the present back to
    /// `Pending`, so the next frame polls again — [`PresentProbe::absorb`]'s
    /// pending arm, tested at the enum because the sequence is known there.
    #[test]
    fn a_readback_pending_reply_drops_the_present_back_to_pending() {
        let mut present = PresentProbe::Waiting { sequence: 7 };
        let advanced = present.absorb(&[(
            7,
            Reply::ReadbackPending {
                readback: PROBE_PRESENT_READBACK,
            },
        )]);
        assert!(advanced);
        assert_eq!(present, PresentProbe::Pending);
    }

    /// A `ReadbackReady` for the poll's sequence carries the bytes into `Ready`.
    #[test]
    fn a_readback_ready_reply_carries_the_present_bytes_into_ready() {
        let mut present = PresentProbe::Waiting { sequence: 7 };
        let bytes = vec![255, 0, 0, 255];
        let advanced = present.absorb(&[(
            7,
            Reply::ReadbackReady {
                readback: PROBE_PRESENT_READBACK,
                data: bytes.clone(),
            },
        )]);
        assert!(advanced);
        assert_eq!(present, PresentProbe::Ready { bytes });
    }

    /// A reply for another sequence leaves the present waiting, exactly as it
    /// leaves every other probe: one channel carries every probe's replies, and
    /// each takes only its own.
    #[test]
    fn a_present_probe_ignores_a_reply_for_another_sequence() {
        let mut present = PresentProbe::Waiting { sequence: 7 };
        let advanced = present.absorb(&[(
            8,
            Reply::ReadbackReady {
                readback: PROBE_PRESENT_READBACK,
                data: vec![1, 2, 3, 4],
            },
        )]);
        assert!(!advanced);
        assert_eq!(present, PresentProbe::Waiting { sequence: 7 });
    }

    /// The dispatch probe's bytes, read the way JS reads them.
    fn compute_bytes() -> Vec<u8> {
        let len = __crcbl_web_gpu_probe_compute_bytes_len() as usize;
        let ptr = __crcbl_web_gpu_probe_compute_bytes_ptr();
        if len == 0 {
            return Vec::new();
        }
        assert!(
            !ptr.is_null(),
            "the dispatch answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::compute` bytes, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        bytes.to_vec()
    }

    /// **Every dispatch handle is a generation past every other probe's**, which
    /// is the whole point of `3 << 32`: the dispatch frame has two buffers, a
    /// shader, a bind-group layout, a bind group, a pipeline layout, a pipeline, a
    /// command buffer, a queue and a readback all live at once, and none of them
    /// may land in the slot the readback (`1 << 32`) or draw (`2 << 32`) probe
    /// files its own resources under in the shared page. The two buffers are the
    /// one case that shares a type, so they differ by index within generation 3.
    #[test]
    fn the_dispatch_handles_are_a_generation_past_every_other_probe() {
        for bits in [
            PROBE_DISPATCH_STORAGE_BUFFER.to_bits(),
            PROBE_DISPATCH_HOST_BUFFER.to_bits(),
            PROBE_DISPATCH_SHADER_MODULE.to_bits(),
            PROBE_DISPATCH_BIND_GROUP_LAYOUT.to_bits(),
            PROBE_DISPATCH_BIND_GROUP.to_bits(),
            PROBE_DISPATCH_PIPELINE_LAYOUT.to_bits(),
            PROBE_DISPATCH_PIPELINE.to_bits(),
            PROBE_DISPATCH_COMMAND_BUFFER.to_bits(),
            PROBE_DISPATCH_QUEUE.to_bits(),
            PROBE_DISPATCH_READBACK.to_bits(),
        ] {
            assert_eq!(bits >> 32, 3, "every dispatch handle is generation three");
        }
        // The two buffers share a kind, so they must not share bits.
        assert_ne!(
            PROBE_DISPATCH_STORAGE_BUFFER.to_bits(),
            PROBE_DISPATCH_HOST_BUFFER.to_bits()
        );
        // A generation clear of both the readback probe (`1 << 32`) and the draw
        // probe (`2 << 32`).
        assert_ne!(
            PROBE_DISPATCH_STORAGE_BUFFER.to_bits(),
            PROBE_DRAW_BUFFER.to_bits()
        );
        assert_ne!(
            PROBE_DISPATCH_PIPELINE.to_bits(),
            PROBE_COMPUTE_PIPELINE.to_bits()
        );
        assert_ne!(
            PROBE_DISPATCH_READBACK.to_bits(),
            PROBE_DRAW_READBACK.to_bits()
        );
    }

    /// The dispatch half: **one export, a whole frame** that dispatches into a
    /// storage buffer and copies it out. The two buffers and the pipeline's four
    /// resources come first, then the encoder and a compute pass that binds the
    /// pipeline, binds the storage group and dispatches before the buffer→buffer
    /// copy, the finish, the submit and the `request_readback` the poll chases.
    #[test]
    fn the_compute_export_encodes_the_pipeline_the_bind_and_the_dispatch() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_compute(), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateBuffer",
                "CreateBuffer",
                "CreateShaderModule",
                "CreateBindGroupLayout",
                "CreateBindGroup",
                "CreatePipelineLayout",
                "CreateComputePipeline",
                "CreateCommandEncoder",
                "BeginComputePass",
                "BindComputePipeline",
                "BindGroup",
                "Dispatch",
                "EndComputePass",
                "CopyBufferToBuffer",
                "Finish",
                "Submit",
                "RequestReadback",
            ],
            "the frame builds the pipeline, then binds, dispatches and reads back"
        );
        // The three commands the dispatch arms add inside the pass, verbatim: the
        // pipeline bind names the dispatch pipeline, the group binds the storage
        // buffer at slot 0 with no dynamic offsets, and the dispatch is one
        // workgroup in each dimension.
        assert!(commands.contains(&Command::BindComputePipeline {
            pipeline: PROBE_DISPATCH_PIPELINE,
        }));
        assert!(commands.contains(&Command::BindGroup {
            slot: 0,
            group: PROBE_DISPATCH_BIND_GROUP,
            dynamic_offsets: Vec::new(),
            layout: PROBE_DISPATCH_PIPELINE_LAYOUT,
        }));
        assert!(commands.contains(&Command::Dispatch { x: 1, y: 1, z: 1 }));
    }

    /// **Nothing waits on the setup frame**: every command in it is caller-
    /// allocated and answered by nothing, so the wait belongs to the poll, not
    /// here — [`the_draw_setup_frame_registers_no_wait_because_the_poll_is_what_is_awaited`]'s
    /// rule on the dispatch's frame.
    #[test]
    fn the_compute_setup_frame_registers_no_wait_because_the_poll_is_what_is_awaited() {
        open_device();
        let before = waiting_replies();
        assert_eq!(__crcbl_web_gpu_probe_compute(), 1);
        assert_eq!(waiting_replies(), before);
        assert_eq!(__crcbl_web_gpu_probe_compute_state(), COMPUTE_REQUESTED);
    }

    /// **A device has to have opened first**, the readback probe's ordering rule:
    /// every command the frame carries is a device method.
    #[test]
    fn a_compute_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_compute(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);
        assert_eq!(__crcbl_web_gpu_probe_compute_state(), COMPUTE_UNASKED);

        grant(&granted("no device yet"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_compute(), 0);
        assert_eq!(take_frame().len(), 1);
    }

    /// The whole dispatch exchange through the exports alone: request, poll, and a
    /// `ReadbackReady` carrying the dispatched pattern — which reaches the bytes
    /// exports. This is the browser gate's path with the replayer replaced by a
    /// `ReplyWriter`, as a `cargo test` has no `navigator.gpu`.
    #[test]
    fn the_compute_readback_reaches_the_bytes_exports_as_the_dispatch_pattern() {
        // `open_device` spends sequences 0 and 1, so the setup frame starts at 2
        // and the poll follows it — read the length off the frame rather than
        // hard-wiring it, so a later command added to the frame does not point the
        // reply at the wrong sequence.
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_compute(), 1);
        let setup = take_frame();
        let poll_sequence = 2 + setup.len() as u64;
        assert_eq!(__crcbl_web_gpu_probe_compute_state(), COMPUTE_REQUESTED);

        assert_eq!(__crcbl_web_gpu_probe_compute_poll(), 1);
        assert_eq!(__crcbl_web_gpu_probe_compute_state(), COMPUTE_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::PollReadback {
                readback: PROBE_DISPATCH_READBACK,
            }]
        );

        let mut written = Vec::new();
        for _ in 0..PROBE_DISPATCH_SLOTS {
            written.extend_from_slice(&PROBE_DISPATCH_PATTERN_BYTES);
        }
        let mut replies = ReplyWriter::new();
        replies.readback_ready(poll_sequence, PROBE_DISPATCH_READBACK, &written);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_compute_state(), COMPUTE_READY);
        assert_eq!(compute_bytes(), written);
        assert_eq!(&compute_bytes()[..4], PROBE_DISPATCH_PATTERN_BYTES);
        // The pattern is a value a zero-initialised buffer cannot hold — the whole
        // evidence the gate reads back from the browser.
        assert_ne!(PROBE_DISPATCH_PATTERN_BYTES, [0, 0, 0, 0]);
    }

    /// A `ReadbackPending` for the poll's sequence drops the dispatch back to
    /// `Pending`, so the next frame polls again — [`ComputeProbe::absorb`]'s
    /// pending arm.
    #[test]
    fn a_readback_pending_reply_drops_the_dispatch_back_to_pending() {
        let mut compute = ComputeProbe::Waiting { sequence: 7 };
        let advanced = compute.absorb(&[(
            7,
            Reply::ReadbackPending {
                readback: PROBE_DISPATCH_READBACK,
            },
        )]);
        assert!(advanced);
        assert_eq!(compute, ComputeProbe::Pending);
    }

    /// A `ReadbackReady` for the poll's sequence carries the bytes into `Ready`.
    #[test]
    fn a_readback_ready_reply_carries_the_dispatch_bytes_into_ready() {
        let mut compute = ComputeProbe::Waiting { sequence: 7 };
        let bytes = vec![0xEF, 0xBE, 0xAD, 0xDE];
        let advanced = compute.absorb(&[(
            7,
            Reply::ReadbackReady {
                readback: PROBE_DISPATCH_READBACK,
                data: bytes.clone(),
            },
        )]);
        assert!(advanced);
        assert_eq!(compute, ComputeProbe::Ready { bytes });
    }

    /// A reply for another sequence leaves the dispatch waiting, exactly as it
    /// leaves every other probe.
    #[test]
    fn a_compute_probe_ignores_a_reply_for_another_sequence() {
        let mut compute = ComputeProbe::Waiting { sequence: 7 };
        let advanced = compute.absorb(&[(
            8,
            Reply::ReadbackReady {
                readback: PROBE_DISPATCH_READBACK,
                data: vec![1, 2, 3, 4],
            },
        )]);
        assert!(!advanced);
        assert_eq!(compute, ComputeProbe::Waiting { sequence: 7 });
    }

    /// The copy-chain probe's bytes, read the way JS reads them.
    fn copychain_bytes() -> Vec<u8> {
        let len = __crcbl_web_gpu_probe_copychain_bytes_len() as usize;
        let ptr = __crcbl_web_gpu_probe_copychain_bytes_ptr();
        if len == 0 {
            return Vec::new();
        }
        assert!(
            !ptr.is_null(),
            "the copy chain answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::copychain` bytes,
        // which nothing between the two calls above can have moved — neither
        // export allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        bytes.to_vec()
    }

    /// **Every copy-chain handle is generation four**, a generation past every
    /// other probe: the frame has two buffers, two textures, and the pipeline's
    /// resources, a command buffer, a queue and a readback all live at once, and
    /// none may land in the dispatch (`3 << 32`), draw (`2 << 32`) or readback
    /// (`1 << 32`) probe's slot in the shared page. The two buffers and the two
    /// textures each share a kind, so they differ by index within generation 4.
    #[test]
    fn the_copychain_handles_are_a_generation_past_every_other_probe() {
        for bits in [
            PROBE_COPYCHAIN_STORAGE_BUFFER.to_bits(),
            PROBE_COPYCHAIN_HOST_BUFFER.to_bits(),
            PROBE_COPYCHAIN_IMAGE_A.to_bits(),
            PROBE_COPYCHAIN_IMAGE_B.to_bits(),
            PROBE_COPYCHAIN_SHADER_MODULE.to_bits(),
            PROBE_COPYCHAIN_BIND_GROUP_LAYOUT.to_bits(),
            PROBE_COPYCHAIN_BIND_GROUP.to_bits(),
            PROBE_COPYCHAIN_PIPELINE_LAYOUT.to_bits(),
            PROBE_COPYCHAIN_PIPELINE.to_bits(),
            PROBE_COPYCHAIN_COMMAND_BUFFER.to_bits(),
            PROBE_COPYCHAIN_QUEUE.to_bits(),
            PROBE_COPYCHAIN_READBACK.to_bits(),
        ] {
            assert_eq!(bits >> 32, 4, "every copy-chain handle is generation four");
        }
        // The two buffers share a kind, so they must not share bits; likewise the
        // two textures.
        assert_ne!(
            PROBE_COPYCHAIN_STORAGE_BUFFER.to_bits(),
            PROBE_COPYCHAIN_HOST_BUFFER.to_bits()
        );
        assert_ne!(
            PROBE_COPYCHAIN_IMAGE_A.to_bits(),
            PROBE_COPYCHAIN_IMAGE_B.to_bits()
        );
        // A generation clear of the dispatch probe (`3 << 32`).
        assert_ne!(
            PROBE_COPYCHAIN_STORAGE_BUFFER.to_bits(),
            PROBE_DISPATCH_STORAGE_BUFFER.to_bits()
        );
    }

    /// The copy-chain half: **one export, a whole frame** that dispatches red into
    /// a storage buffer and moves it through two textures to a host buffer. The
    /// three copies — buffer→image, image→image, image→buffer — are the point.
    #[test]
    fn the_copychain_export_encodes_the_dispatch_and_the_three_copies() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_copychain(), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateBuffer",
                "CreateBuffer",
                "CreateImage",
                "CreateImage",
                "CreateShaderModule",
                "CreateBindGroupLayout",
                "CreateBindGroup",
                "CreatePipelineLayout",
                "CreateComputePipeline",
                "CreateCommandEncoder",
                "BeginComputePass",
                "BindComputePipeline",
                "BindGroup",
                "Dispatch",
                "EndComputePass",
                "PipelineBarrier",
                "CopyBufferToImage",
                "CopyImageToImage",
                "CopyImageToBuffer",
                "Finish",
                "Submit",
                "RequestReadback",
            ],
            "the frame dispatches, barriers the storage buffer, then copies \
             buffer→image→image→buffer and reads back"
        );
        // The no-op barrier sits at the seam between the dispatch (ShaderWrite)
        // and the first copy (TransferSrc), carried whole though the replayer
        // records nothing.
        assert!(commands.contains(&Command::PipelineBarrier {
            buffers: vec![BufferBarrier {
                buffer: PROBE_COPYCHAIN_STORAGE_BUFFER,
                from: ResourceState::ShaderWrite,
                to: ResourceState::TransferSrc,
                queue_transfer: None,
            }],
            images: Vec::new(),
            global: false,
        }));
        // The three copies, verbatim: the upload into the first texture, the
        // texture→texture copy, and the read-out into the host buffer.
        assert!(commands.contains(&Command::CopyBufferToImage {
            copy: probe_copychain_buffer_to_image(),
        }));
        assert!(commands.contains(&Command::CopyImageToImage {
            copy: probe_copychain_image_to_image(),
        }));
        assert!(commands.contains(&Command::Dispatch {
            x: PROBE_COPYCHAIN_SIZE,
            y: 1,
            z: 1,
        }));
    }

    /// A copy-chain request before a device opens is refused and encodes nothing.
    #[test]
    fn a_copychain_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_copychain(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);
        assert_eq!(__crcbl_web_gpu_probe_copychain_state(), COPYCHAIN_UNASKED);
    }

    /// The whole copy-chain exchange through the exports alone: request, poll, and
    /// a `ReadbackReady` carrying the red pattern for every texel, which reaches
    /// the bytes exports. A `cargo test` has no `navigator.gpu`, so the replayer
    /// is stood in for by a `ReplyWriter`.
    #[test]
    fn the_copychain_readback_reaches_the_bytes_exports_as_the_red_pattern() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_copychain(), 1);
        let setup = take_frame();
        let poll_sequence = 2 + setup.len() as u64;
        assert_eq!(__crcbl_web_gpu_probe_copychain_state(), COPYCHAIN_REQUESTED);

        assert_eq!(__crcbl_web_gpu_probe_copychain_poll(), 1);
        assert_eq!(__crcbl_web_gpu_probe_copychain_state(), COPYCHAIN_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::PollReadback {
                readback: PROBE_COPYCHAIN_READBACK,
            }]
        );

        let mut red = Vec::new();
        for _ in 0..PROBE_COPYCHAIN_SLOTS {
            red.extend_from_slice(&PROBE_COPYCHAIN_PATTERN_BYTES);
        }
        let mut replies = ReplyWriter::new();
        replies.readback_ready(poll_sequence, PROBE_COPYCHAIN_READBACK, &red);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_copychain_state(), COPYCHAIN_READY);
        assert_eq!(copychain_bytes(), red);
        assert_eq!(&copychain_bytes()[..4], PROBE_COPYCHAIN_PATTERN_BYTES);
        // Red is a value a zero-initialised texture cannot hold — the evidence the
        // gate reads back that both copies ran.
        assert_ne!(PROBE_COPYCHAIN_PATTERN_BYTES, [0, 0, 0, 0]);
    }

    /// The fill probe's bytes, read the way JS reads them.
    fn fill_bytes() -> Vec<u8> {
        let len = __crcbl_web_gpu_probe_fill_bytes_len() as usize;
        let ptr = __crcbl_web_gpu_probe_fill_bytes_ptr();
        if len == 0 {
            return Vec::new();
        }
        assert!(!ptr.is_null(), "the fill answered a length with no pointer");
        // SAFETY: `ptr` and `len` are this thread's `Probe::fill` bytes, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        bytes.to_vec()
    }

    /// **Every fill handle is generation four and distinct from the copy chain's**
    /// — the two probes share generation 4 and the kinds they both use are given
    /// distinct indices, so their live resources never collide in the shared page.
    #[test]
    fn the_fill_handles_are_generation_four_and_distinct_from_the_copychain() {
        for bits in [
            PROBE_FILL_STORAGE_BUFFER.to_bits(),
            PROBE_FILL_HOST_BUFFER.to_bits(),
            PROBE_FILL_SHADER_MODULE.to_bits(),
            PROBE_FILL_BIND_GROUP_LAYOUT.to_bits(),
            PROBE_FILL_BIND_GROUP.to_bits(),
            PROBE_FILL_PIPELINE_LAYOUT.to_bits(),
            PROBE_FILL_PIPELINE.to_bits(),
            PROBE_FILL_COMMAND_BUFFER.to_bits(),
            PROBE_FILL_QUEUE.to_bits(),
            PROBE_FILL_READBACK.to_bits(),
        ] {
            assert_eq!(bits >> 32, 4, "every fill handle is generation four");
        }
        // The buffer kind is shared with the copy chain (which uses indices 0 and
        // 1), so the fill's two buffers take 2 and 3 and must not collide.
        for (fill, chain) in [
            (
                PROBE_FILL_STORAGE_BUFFER.to_bits(),
                PROBE_COPYCHAIN_STORAGE_BUFFER.to_bits(),
            ),
            (
                PROBE_FILL_HOST_BUFFER.to_bits(),
                PROBE_COPYCHAIN_HOST_BUFFER.to_bits(),
            ),
            (
                PROBE_FILL_SHADER_MODULE.to_bits(),
                PROBE_COPYCHAIN_SHADER_MODULE.to_bits(),
            ),
            (
                PROBE_FILL_PIPELINE.to_bits(),
                PROBE_COPYCHAIN_PIPELINE.to_bits(),
            ),
            (
                PROBE_FILL_READBACK.to_bits(),
                PROBE_COPYCHAIN_READBACK.to_bits(),
            ),
        ] {
            assert_ne!(fill, chain, "a fill handle collides with the copy chain's");
        }
    }

    /// The fill half: **one export, a whole frame** that dispatches a pattern into
    /// a storage buffer, zeroes its first half with a `fill_buffer`, and copies it
    /// out. The `FillBuffer` between the pass and the copy is the point.
    #[test]
    fn the_fill_export_encodes_the_dispatch_the_fill_and_the_copy() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_fill(), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateBuffer",
                "CreateBuffer",
                "CreateShaderModule",
                "CreateBindGroupLayout",
                "CreateBindGroup",
                "CreatePipelineLayout",
                "CreateComputePipeline",
                "CreateCommandEncoder",
                "BeginComputePass",
                "BindComputePipeline",
                "BindGroup",
                "Dispatch",
                "EndComputePass",
                "FillBuffer",
                "CopyBufferToBuffer",
                "Finish",
                "Submit",
                "RequestReadback",
            ],
            "the frame dispatches, fills half the buffer to zero, then copies and reads back"
        );
        // The fill zeroes exactly the first half of the storage buffer.
        assert!(commands.contains(&Command::FillBuffer {
            buffer: PROBE_FILL_STORAGE_BUFFER,
            offset: 0,
            size: PROBE_FILL_ZEROED_BYTES,
            value: 0,
        }));
    }

    /// A fill request before a device opens is refused and encodes nothing.
    #[test]
    fn a_fill_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_fill(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);
        assert_eq!(__crcbl_web_gpu_probe_fill_state(), FILL_UNASKED);
    }

    /// The whole fill exchange through the exports alone: a `ReadbackReady`
    /// carrying the first half zeroed and the second half still the pattern, which
    /// reaches the bytes exports and is what the gate checks.
    #[test]
    fn the_fill_readback_reaches_the_bytes_exports_zeroed_then_pattern() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_fill(), 1);
        let setup = take_frame();
        let poll_sequence = 2 + setup.len() as u64;
        assert_eq!(__crcbl_web_gpu_probe_fill_state(), FILL_REQUESTED);

        assert_eq!(__crcbl_web_gpu_probe_fill_poll(), 1);
        assert_eq!(__crcbl_web_gpu_probe_fill_state(), FILL_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::PollReadback {
                readback: PROBE_FILL_READBACK,
            }]
        );

        // The bytes a real `clearBuffer` over the first half would leave: zeros to
        // `PROBE_FILL_ZEROED_BYTES`, then the dispatch's pattern for the rest.
        let total = (PROBE_FILL_SLOTS as usize) * 4;
        let zeroed = PROBE_FILL_ZEROED_BYTES as usize;
        let mut filled = vec![0u8; zeroed];
        while filled.len() < total {
            filled.extend_from_slice(&PROBE_FILL_PATTERN_BYTES);
        }
        let mut replies = ReplyWriter::new();
        replies.readback_ready(poll_sequence, PROBE_FILL_READBACK, &filled);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_fill_state(), FILL_READY);
        let bytes = fill_bytes();
        assert_eq!(bytes, filled);
        assert!(
            bytes[..zeroed].iter().all(|&byte| byte == 0),
            "the fill zeroed its whole sub-range"
        );
        assert_eq!(&bytes[zeroed..zeroed + 4], PROBE_FILL_PATTERN_BYTES);
        // The pattern is a value a zero fill cannot leave — the evidence the fill
        // stopped at its size and did not zero the whole buffer.
        assert_ne!(PROBE_FILL_PATTERN_BYTES, [0, 0, 0, 0]);
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
