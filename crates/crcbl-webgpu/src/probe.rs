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
//! | [`__crcbl_web_gpu_probe_present`](shim::__crcbl_web_gpu_probe_present) | `(i32) -> i32` | Encode one frame — a surface on the canvas `canvas_id` names, an **sRGB** swapchain configured on it, the acquired frame, a pass that clears the acquired view to [`PROBE_PRESENT_COLOR`], the copy, submit, present, and a `request_readback` against [`PROBE_PRESENT_READBACK`]. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_present_poll`](shim::__crcbl_web_gpu_probe_present_poll) | `() -> i32` | Poll the present probe's readback once. `1` when a poll is on the stream, `0` when there is nothing to poll for. |
//! | [`__crcbl_web_gpu_probe_present_state`](shim::__crcbl_web_gpu_probe_present_state) | `() -> i32` | Drain, and answer one of the `PRESENT_*` codes. |
//! | [`__crcbl_web_gpu_probe_present_bytes_ptr`](shim::__crcbl_web_gpu_probe_present_bytes_ptr) | `() -> i32` | Where the presented bytes start, once [`__crcbl_web_gpu_probe_present_state`](shim::__crcbl_web_gpu_probe_present_state) answers [`PRESENT_READY`]. |
//! | [`__crcbl_web_gpu_probe_present_bytes_len`](shim::__crcbl_web_gpu_probe_present_bytes_len) | `() -> i32` | How many bytes there are, or `0` if the present probe has not answered. |
//! | [`__crcbl_web_gpu_probe_reconfigure`](shim::__crcbl_web_gpu_probe_reconfigure) | `(i32) -> i32` | Encode one frame — a surface on the canvas `canvas_id` names, a swapchain configured `Rgba8Unorm`, that swapchain reconfigured `Bgra8Unorm`, the acquired frame, a pass that clears it to [`PROBE_RECONFIG_COLOR`], the copy, submit, present, and a `request_readback` against [`PROBE_RECONFIG_READBACK`]. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_reconfigure_poll`](shim::__crcbl_web_gpu_probe_reconfigure_poll) | `() -> i32` | Poll the reconfigure probe's readback once. `1` when a poll is on the stream, `0` when there is nothing to poll for. |
//! | [`__crcbl_web_gpu_probe_reconfigure_state`](shim::__crcbl_web_gpu_probe_reconfigure_state) | `() -> i32` | Drain, and answer one of the `RECONFIG_*` codes. |
//! | [`__crcbl_web_gpu_probe_reconfigure_bytes_ptr`](shim::__crcbl_web_gpu_probe_reconfigure_bytes_ptr) | `() -> i32` | Where the reconfigured bytes start, once [`__crcbl_web_gpu_probe_reconfigure_state`](shim::__crcbl_web_gpu_probe_reconfigure_state) answers [`RECONFIG_READY`]. |
//! | [`__crcbl_web_gpu_probe_reconfigure_bytes_len`](shim::__crcbl_web_gpu_probe_reconfigure_bytes_len) | `() -> i32` | How many bytes there are, or `0` if the reconfigure probe has not answered. |
//! | [`__crcbl_web_gpu_probe_indirect`](shim::__crcbl_web_gpu_probe_indirect) | `() -> i32` | Encode one frame — the draw probe's pipeline, a `write_buffer` filling an indirect-args buffer with [`PROBE_INDIRECT_ARGS_BYTES`] and an index buffer with [`PROBE_INDIRECT_INDEX_BYTES`], a pass that clears to [`PROBE_INDIRECT_CLEAR`] then binds the pipeline and index buffer and records a `drawIndexedIndirect`, the copy, and a `request_readback` against [`PROBE_INDIRECT_READBACK`]. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_indirect_poll`](shim::__crcbl_web_gpu_probe_indirect_poll) | `() -> i32` | Poll the indirect draw's readback once. `1` when a poll is on the stream, `0` when there is nothing to poll for. |
//! | [`__crcbl_web_gpu_probe_indirect_state`](shim::__crcbl_web_gpu_probe_indirect_state) | `() -> i32` | Drain, and answer one of the `INDIRECT_*` codes. |
//! | [`__crcbl_web_gpu_probe_indirect_bytes_ptr`](shim::__crcbl_web_gpu_probe_indirect_bytes_ptr) | `() -> i32` | Where the drawn pixels start, once [`__crcbl_web_gpu_probe_indirect_state`](shim::__crcbl_web_gpu_probe_indirect_state) answers [`INDIRECT_READY`]. |
//! | [`__crcbl_web_gpu_probe_indirect_bytes_len`](shim::__crcbl_web_gpu_probe_indirect_bytes_len) | `() -> i32` | How many bytes there are, or `0` if the indirect draw has not answered. |
//! | [`__crcbl_web_gpu_probe_depth`](shim::__crcbl_web_gpu_probe_depth) | `() -> i32` | Encode one frame — a [`Format::D32Float`] atlas and a view of it, a pass whose only attachment is that view cleared to [`PROBE_DEPTH_CLEAR`] and stored, an image→buffer copy of its [`ImageAspect::DEPTH`] plane, and a `request_readback` against [`PROBE_DEPTH_READBACK`]. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_depth_poll`](shim::__crcbl_web_gpu_probe_depth_poll) | `() -> i32` | Poll the depth readback once. `1` when a poll is on the stream, `0` when there is nothing to poll for. |
//! | [`__crcbl_web_gpu_probe_depth_state`](shim::__crcbl_web_gpu_probe_depth_state) | `() -> i32` | Drain, and answer one of the `DEPTH_*` codes. |
//! | [`__crcbl_web_gpu_probe_depth_bytes_ptr`](shim::__crcbl_web_gpu_probe_depth_bytes_ptr) | `() -> i32` | Where the depth plane starts, once [`__crcbl_web_gpu_probe_depth_state`](shim::__crcbl_web_gpu_probe_depth_state) answers [`DEPTH_READY`]. |
//! | [`__crcbl_web_gpu_probe_depth_bytes_len`](shim::__crcbl_web_gpu_probe_depth_bytes_len) | `() -> i32` | How many bytes there are, or `0` if the depth probe has not answered. |
//! | [`__crcbl_web_gpu_probe_stencil`](shim::__crcbl_web_gpu_probe_stencil) | `() -> i32` | Encode one frame — an `Rgba8Unorm` target and a [`Format::D24UnormS8Uint`] one, a pipeline comparing the stencil plane [`CompareOp::Equal`] against [`PROBE_STENCIL_BAKED`], a pass that clears the plane to [`PROBE_STENCIL_CLEARED`] and draws twice with `set_stencil_reference` before each, the copy, and a `request_readback` against [`PROBE_STENCIL_READBACK`]. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_stencil_poll`](shim::__crcbl_web_gpu_probe_stencil_poll) | `() -> i32` | Poll the stencil readback once. `1` when a poll is on the stream, `0` when there is nothing to poll for. |
//! | [`__crcbl_web_gpu_probe_stencil_state`](shim::__crcbl_web_gpu_probe_stencil_state) | `() -> i32` | Drain, and answer one of the `STENCIL_*` codes. |
//! | [`__crcbl_web_gpu_probe_stencil_bytes_ptr`](shim::__crcbl_web_gpu_probe_stencil_bytes_ptr) | `() -> i32` | Where the drawn pixels start, once [`__crcbl_web_gpu_probe_stencil_state`](shim::__crcbl_web_gpu_probe_stencil_state) answers [`STENCIL_READY`]. |
//! | [`__crcbl_web_gpu_probe_stencil_bytes_len`](shim::__crcbl_web_gpu_probe_stencil_bytes_len) | `() -> i32` | How many bytes there are, or `0` if the stencil probe has not answered. |
//! | [`__crcbl_web_gpu_probe_msaa_samples`](shim::__crcbl_web_gpu_probe_msaa_samples) | `() -> i32` | The opened device's [`Limits::max_sample_count`](crcbl_hal::Limits::max_sample_count) — what [`__crcbl_web_gpu_probe_msaa`](shim::__crcbl_web_gpu_probe_msaa) decides on. `0` if no device has opened. |
//! | [`__crcbl_web_gpu_probe_msaa`](shim::__crcbl_web_gpu_probe_msaa) | `() -> i32` | Encode one frame — a [`PROBE_MSAA_SAMPLES`]-sample colour target and a single-sampled one, a buffer→image prime filling the second with [`PROBE_MSAA_POISON_BYTES`], a pass with **no draws** that clears the first to [`PROBE_MSAA_CLEAR_BYTES`] and names the second in [`ColorAttachment::resolve`], the copy, and a `request_readback` against [`PROBE_MSAA_READBACK`]. `1`, or `0` if no device has opened, the device reports fewer than [`PROBE_MSAA_SAMPLES`] samples, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_msaa_poll`](shim::__crcbl_web_gpu_probe_msaa_poll) | `() -> i32` | Poll the resolve's readback once. `1` when a poll is on the stream, `0` when there is nothing to poll for. |
//! | [`__crcbl_web_gpu_probe_msaa_state`](shim::__crcbl_web_gpu_probe_msaa_state) | `() -> i32` | Drain, and answer one of the `MSAA_*` codes. |
//! | [`__crcbl_web_gpu_probe_msaa_bytes_ptr`](shim::__crcbl_web_gpu_probe_msaa_bytes_ptr) | `() -> i32` | Where the resolved texels start, once [`__crcbl_web_gpu_probe_msaa_state`](shim::__crcbl_web_gpu_probe_msaa_state) answers [`MSAA_READY`]. |
//! | [`__crcbl_web_gpu_probe_msaa_bytes_len`](shim::__crcbl_web_gpu_probe_msaa_bytes_len) | `() -> i32` | How many bytes there are, or `0` if the MSAA probe has not answered. |
//! | [`__crcbl_web_gpu_probe_occlusion`](shim::__crcbl_web_gpu_probe_occlusion) | `() -> i32` | Encode one frame — a [`PROBE_OCCLUSION_QUERIES`]-query [`QueryKind::Occlusion`] set, a `QUERY_RESOLVE` destination filled with [`PROBE_OCCLUSION_SENTINEL`], the reset and the resolve over it, the copy, a `request_readback` against [`PROBE_OCCLUSION_READBACK`], and a `query_results` ask reading the same queries the other way. `1`, or `0` if no device has opened, the probe is re-entered, or another channel is installed. |
//! | [`__crcbl_web_gpu_probe_occlusion_poll`](shim::__crcbl_web_gpu_probe_occlusion_poll) | `() -> i32` | Poll the occlusion readback once. `1` when a poll is on the stream, `0` when there is nothing to poll for. |
//! | [`__crcbl_web_gpu_probe_occlusion_state`](shim::__crcbl_web_gpu_probe_occlusion_state) | `() -> i32` | Drain, and answer one of the `OCCLUSION_*` codes. |
//! | [`__crcbl_web_gpu_probe_occlusion_bytes_ptr`](shim::__crcbl_web_gpu_probe_occlusion_bytes_ptr) | `() -> i32` | Where the resolved values start, once [`__crcbl_web_gpu_probe_occlusion_state`](shim::__crcbl_web_gpu_probe_occlusion_state) answers [`OCCLUSION_READY`]. |
//! | [`__crcbl_web_gpu_probe_occlusion_bytes_len`](shim::__crcbl_web_gpu_probe_occlusion_bytes_len) | `() -> i32` | How many bytes there are, or `0` if the occlusion probe has not answered. |
//! | [`__crcbl_web_gpu_probe_occlusion_values_state`](shim::__crcbl_web_gpu_probe_occlusion_values_state) | `() -> i32` | Drain, and answer one of the `OCCLUSION_VALUES_*` codes — where the **direct read** has got to. |
//! | [`__crcbl_web_gpu_probe_occlusion_values_ptr`](shim::__crcbl_web_gpu_probe_occlusion_values_ptr) | `() -> i32` | Where that read's values start, one little-endian `u64` per query. |
//! | [`__crcbl_web_gpu_probe_occlusion_values_len`](shim::__crcbl_web_gpu_probe_occlusion_values_len) | `() -> i32` | How many bytes there are. **Zero is a failed read**, not an empty success. |
//! | [`__crcbl_web_gpu_probe_timestamp_supported`](shim::__crcbl_web_gpu_probe_timestamp_supported) | `() -> i32` | Whether the opened device has the browser's `timestamp-query`, as `1` or `0`. Read this before asking. |
//! | [`__crcbl_web_gpu_probe_timestamp`](shim::__crcbl_web_gpu_probe_timestamp) | `() -> i32` | Encode a compute pass whose descriptor names two timestamp queries, submit it, and ask for both values. |
//! | [`__crcbl_web_gpu_probe_timestamp_state`](shim::__crcbl_web_gpu_probe_timestamp_state) | `() -> i32` | Drain, and answer one of the `TIMESTAMP_*` codes. |
//! | [`__crcbl_web_gpu_probe_timestamp_ptr`](shim::__crcbl_web_gpu_probe_timestamp_ptr) | `() -> i32` | Where the two ticks start, as little-endian `u64`. |
//! | [`__crcbl_web_gpu_probe_timestamp_len`](shim::__crcbl_web_gpu_probe_timestamp_len) | `() -> i32` | How many bytes there are. **Zero is a failed read**, and two zero values are a pass nothing timed. |
//! | [`__crcbl_web_gpu_probe_parity`](shim::__crcbl_web_gpu_probe_parity) | `() -> i32` | Build a [`WebGpuDevice`] around the opened device's caps, walk its whole [`supports`](crcbl_hal::Device::supports) matrix and hold it against [`DIVERGENCES`](crcbl_hal::DIVERGENCES). One of the `PARITY_*` codes. Asks the browser nothing. |
//! | [`__crcbl_web_gpu_probe_parity_checked`](shim::__crcbl_web_gpu_probe_parity_checked) | `() -> i32` | How many capabilities that walked, or `0` if it has not run. |
//! | [`__crcbl_web_gpu_probe_parity_held`](shim::__crcbl_web_gpu_probe_parity_held) | `() -> i32` | How many of those were settled, rather than left unprovable by a device that withheld the gating feature. |
//! | [`__crcbl_web_gpu_probe_parity_report_ptr`](shim::__crcbl_web_gpu_probe_parity_report_ptr) | `() -> i32` | Where the matrix starts — one `Capability=verdict` token per capability. |
//! | [`__crcbl_web_gpu_probe_parity_report_len`](shim::__crcbl_web_gpu_probe_parity_report_len) | `() -> i32` | How long it is, in UTF-8 bytes. |
//! | [`__crcbl_web_gpu_probe_parity_failures_ptr`](shim::__crcbl_web_gpu_probe_parity_failures_ptr) | `() -> i32` | Where the disagreements start, one per line. Empty on [`PARITY_MATCHED`]. |
//! | [`__crcbl_web_gpu_probe_parity_failures_len`](shim::__crcbl_web_gpu_probe_parity_failures_len) | `() -> i32` | How long that text is, in UTF-8 bytes. |
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
//! — core WebGPU, so every browser can satisfy it — and asks for **one optional
//! feature and no more**. The parsimony is not timidity: it is what makes the
//! answer checkable. A device opened with nearly nothing is one the page can
//! open a second time for itself and compare against, and its capabilities
//! differ from the *adapter's* on any machine whose adapter reports more than
//! what was asked for. A request that asked for everything the adapter had would
//! produce a device whose capabilities equal the adapter's, and a backend that
//! reported the adapter's record for its device would then pass.
//!
//! The one exception is
//! [`TIMESTAMP_QUERY`](crcbl_hal::Features::TIMESTAMP_QUERY), and it is asked
//! for because without it there is nothing to observe: a `GPUQuerySet` of type
//! `'timestamp'` needs the browser's `timestamp-query`, so a device that did not
//! ask could not create one and group AF would have no claim to hold. It is
//! *optional* rather than required, so a browser without it still opens a device
//! and the probe reports [`TIMESTAMP_UNSUPPORTED`] with a reason rather than
//! failing to start. `web/tools/probe-groups.mjs` opens its reference device
//! with the same request, so the two are compared like for like.
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
//! adapter's.** A page can open a device with the descriptor the replayer uses
//! and read `device.features` and `device.limits.maxTextureDimension2D` off it,
//! which is what the gate holds wasm's numbers to.
//!
//! It is the *features* that carry that distinction now, not the limits. The
//! replayer asks `requestDevice` for every limit the adapter reports — WebGPU's
//! default of eight storage buffers per shader stage is below what
//! `crcbl-render` binds — so a device's limits equal its adapter's by
//! construction, while an optional feature the request did not name is still not
//! granted. A backend that copied the adapter's record would therefore still be
//! visible, and in the one place it can be.
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
//! [`formats`](SurfaceCaps::formats) is the field the browser fills, and what
//! [`preferred_format`](SurfaceCaps::preferred_format) answers off it is the
//! **sRGB counterpart of `getPreferredCanvasFormat()`** — that call is what
//! `web/engine/gpu-replay.js` leads the list with, and `preferred_format` takes
//! the first sRGB entry. **That varies by browser and by machine and the page can
//! ask for it independently**, so it is the one value here a gate check can
//! corroborate instead of restate, and it is exported as its wire code. The
//! counterpart is not a format a canvas can be *configured* with — no `-srgb` one
//! is — so what the gate corroborates is the pairing, which is exactly the step
//! that was missing while the deployed site presented every frame a transfer
//! function too dark.
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

use core::fmt::Write as _;
use std::cell::RefCell;
use std::rc::Rc;

use crcbl_hal::{
    AdapterId, Barriers, BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc,
    BindGroupLayoutEntry, BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource,
    BlendState, BufferBarrier, BufferCopy, BufferDesc, BufferHandle, BufferImageCopy, BufferUsage,
    Capability, ClearValue, ColorAttachment, ColorTargetState, ColorWrites, CommandBufferHandle,
    CommandEncoderDesc, CompareOp, CompositeAlpha, ComputePassDesc, ComputePipelineDesc,
    ComputePipelineHandle, CullMode, DepthBias, DepthStencilAttachment, DepthStencilState, Device,
    DeviceDesc, Extent3d, Features, FilterMode, Format, FrontFace, GraphicsPipelineDesc,
    GraphicsPipelineHandle, ImageAspect, ImageCopy, ImageDesc, ImageHandle, ImageSubresourceLayers,
    ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType,
    IndexFormat, LoadOp, MemoryLocation, MultisampleState, Offset3d, ParityVerdict,
    PassTimestampWrites, PipelineLayoutDesc, PipelineLayoutHandle, PolygonMode, PresentInfo,
    PresentMode, PrimitiveState, PrimitiveTopology, QueryKind, QuerySetDesc, QuerySetHandle,
    QueueHandle, ReadbackDesc, ReadbackHandle, Rect2d, RenderPassDesc, ResourceState, SampleType,
    SamplerAddressMode, SamplerDesc, SamplerHandle, ShaderEntry, ShaderModuleDesc,
    ShaderModuleHandle, ShaderStages, StoreOp, SubmitInfo, SurfaceCaps, SurfaceHandle,
    SwapchainDesc, SwapchainHandle, divergence, parity_verdict,
};

use crate::device::DeviceProbe;
use crate::hal::{HandlePool, SharedChannel, WebGpuDevice};
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
/// host buffer, an encoder, a render pass that clears the acquired view to
/// [`PROBE_PRESENT_COLOR`],
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
/// texels, every one [`PROBE_PRESENT_COLOR_BYTES`] to within
/// [`PROBE_PRESENT_COLOR_TOLERANCE`] if the real canvas context path acquired,
/// rendered, sRGB-encoded and copied a frame end to end.
pub const PRESENT_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`COMPUTE_UNDECODABLE`]'s twin.
pub const PRESENT_UNDECODABLE: u32 = 5;

/// Nothing has been asked, or there is no channel to ask through.
pub const RECONFIG_UNASKED: u32 = 0;
/// The setup frame — a surface, a swapchain configured `Rgba8Unorm`, that same
/// swapchain *reconfigured* `Bgra8Unorm`, the acquired frame, the host buffer,
/// an encoder, a render pass that clears the acquired view to red, the copy, the
/// submit, the present, and the request — is on the stream, and no poll has been
/// issued.
pub const RECONFIG_REQUESTED: u32 = 1;
/// A [`poll_readback`](crate::StreamWriter::poll_readback) is out and its reply
/// has not arrived.
pub const RECONFIG_WAITING: u32 = 2;
/// The last poll was answered [`Pending`](crcbl_hal::ReadbackState::Pending):
/// the map has not resolved yet, so the next frame polls again.
pub const RECONFIG_PENDING: u32 = 3;
/// The bytes are in. [`shim::__crcbl_web_gpu_probe_reconfigure_bytes_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_reconfigure_bytes_len`] carry them — 64×64
/// `Bgra8Unorm` texels, every one [`PROBE_RECONFIG_COLOR_BYTES`] if the
/// reconfigure re-ran `configure` with the new format. A stub that skipped it
/// leaves the swapchain `Rgba8Unorm` and reads back `[255, 0, 0, 255]` instead.
pub const RECONFIG_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`COMPUTE_UNDECODABLE`]'s twin.
pub const RECONFIG_UNDECODABLE: u32 = 5;

/// Nothing has been asked, or there is no channel to ask through.
pub const INDIRECT_UNASKED: u32 = 0;
/// The setup frame — the pipeline, the args and index buffers filled by
/// `write_buffer`, a clear, a bound pipeline, a bound index buffer, an indexed
/// indirect draw, the copy, the submit and the request — is on the stream, and no
/// poll has been issued.
pub const INDIRECT_REQUESTED: u32 = 1;
/// A [`poll_readback`](crate::StreamWriter::poll_readback) is out and its reply
/// has not arrived.
pub const INDIRECT_WAITING: u32 = 2;
/// The last poll was answered [`Pending`](crcbl_hal::ReadbackState::Pending):
/// the map has not resolved yet, so the next frame polls again.
pub const INDIRECT_PENDING: u32 = 3;
/// The bytes are in. [`shim::__crcbl_web_gpu_probe_indirect_bytes_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_indirect_bytes_len`] carry them — one drawn texel
/// per four, which the gate checks is the draw colour and not the clear, proving
/// an indirect draw put exactly what a direct draw would on the frame.
pub const INDIRECT_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`DRAW_UNDECODABLE`]'s twin.
pub const INDIRECT_UNDECODABLE: u32 = 5;

/// Nothing has been asked, or there is no channel to ask through.
pub const DEPTH_UNASKED: u32 = 0;
/// The setup frame — a [`Format::D32Float`] image and a view of it, the host
/// buffer, an encoder, a render pass whose only attachment is that view cleared
/// to [`PROBE_DEPTH_CLEAR`] and stored, an image→buffer copy of the **depth
/// plane**, the submit and the request — is on the stream, and no poll has been
/// issued.
pub const DEPTH_REQUESTED: u32 = 1;
/// A [`poll_readback`](crate::StreamWriter::poll_readback) is out and its reply
/// has not arrived.
pub const DEPTH_WAITING: u32 = 2;
/// The last poll was answered [`Pending`](crcbl_hal::ReadbackState::Pending):
/// the map has not resolved yet, so the next frame polls again.
pub const DEPTH_PENDING: u32 = 3;
/// The bytes are in. [`shim::__crcbl_web_gpu_probe_depth_bytes_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_depth_bytes_len`] carry them — 64×64
/// `depth32float` texels, every one [`PROBE_DEPTH_CLEAR`] if the browser copied
/// a depth plane out to a buffer at all.
pub const DEPTH_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`DRAW_UNDECODABLE`]'s twin.
pub const DEPTH_UNDECODABLE: u32 = 5;

/// Nothing has been asked, or there is no channel to ask through.
pub const STENCIL_UNASKED: u32 = 0;
/// The setup frame — a colour target and a [`Format::D24UnormS8Uint`]
/// depth-stencil target, a pipeline that compares the stencil plane
/// [`CompareOp::Equal`] against [`PROBE_STENCIL_BAKED`], a pass that clears the
/// plane to [`PROBE_STENCIL_CLEARED`] and draws twice with a
/// `set_stencil_reference` before each, the copy, the submit and the request —
/// is on the stream, and no poll has been issued.
pub const STENCIL_REQUESTED: u32 = 1;
/// A [`poll_readback`](crate::StreamWriter::poll_readback) is out and its reply
/// has not arrived.
pub const STENCIL_WAITING: u32 = 2;
/// The last poll was answered [`Pending`](crcbl_hal::ReadbackState::Pending):
/// the map has not resolved yet, so the next frame polls again.
pub const STENCIL_PENDING: u32 = 3;
/// The bytes are in. [`shim::__crcbl_web_gpu_probe_stencil_bytes_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_stencil_bytes_len`] carry them — 64×64
/// `Rgba8Unorm` texels, every one [`PROBE_STENCIL_FIRST_BYTES`] if both
/// references reached the browser and were applied.
pub const STENCIL_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`DRAW_UNDECODABLE`]'s twin.
pub const STENCIL_UNDECODABLE: u32 = 5;

/// Nothing has been asked, or there is no channel to ask through.
pub const MSAA_UNASKED: u32 = 0;
/// The setup frame — a [`PROBE_MSAA_SAMPLES`]-sample colour target and a
/// single-sampled one, the prime that fills the second with
/// [`PROBE_MSAA_POISON_BYTES`], a pass whose only content is a clear of the
/// multisampled target and which names the single-sampled view in
/// [`ColorAttachment::resolve`], the copy, the submit and the request — is on the
/// stream, and no poll has been issued.
pub const MSAA_REQUESTED: u32 = 1;
/// A [`poll_readback`](crate::StreamWriter::poll_readback) is out and its reply
/// has not arrived.
pub const MSAA_WAITING: u32 = 2;
/// The last poll was answered [`Pending`](crcbl_hal::ReadbackState::Pending):
/// the map has not resolved yet, so the next frame polls again.
pub const MSAA_PENDING: u32 = 3;
/// The bytes are in. [`shim::__crcbl_web_gpu_probe_msaa_bytes_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_msaa_bytes_len`] carry them — the resolve
/// target's `Rgba8Unorm` texels, every one [`PROBE_MSAA_CLEAR_BYTES`] if the
/// multisampled clear was resolved into it.
pub const MSAA_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`DRAW_UNDECODABLE`]'s twin.
pub const MSAA_UNDECODABLE: u32 = 5;
/// The **device** reported a
/// [`max_sample_count`](crcbl_hal::Limits::max_sample_count) below
/// [`PROBE_MSAA_SAMPLES`], so nothing was encoded: there is no multisampled
/// colour target for a resolve to resolve from, and a frame that made a
/// single-sampled one instead would pass while proving nothing.
/// [`shim::__crcbl_web_gpu_probe_msaa_samples`] is what the device reported.
///
/// The one code here with no [`STENCIL_UNASKED`] counterpart, because it is the
/// one probe whose fixture the device can refuse to supply.
pub const MSAA_UNSUPPORTED: u32 = 6;

/// Nothing has been asked, or there is no channel to ask through.
pub const OCCLUSION_UNASKED: u32 = 0;
/// The setup frame — a [`PROBE_OCCLUSION_QUERIES`]-query occlusion set, the
/// resolve destination filled with [`PROBE_OCCLUSION_SENTINEL`], the readback
/// buffer, an encoder that resets the whole range and resolves it over the
/// sentinel, the copy, the submit, the request, and the
/// [`query_results`](crate::StreamWriter::query_results) ask that reads the same
/// queries the other way — is on the stream, and no poll has been issued.
pub const OCCLUSION_REQUESTED: u32 = 1;
/// A [`poll_readback`](crate::StreamWriter::poll_readback) is out and its reply
/// has not arrived.
pub const OCCLUSION_WAITING: u32 = 2;
/// The last poll was answered [`Pending`](crcbl_hal::ReadbackState::Pending):
/// the map has not resolved yet, so the next frame polls again.
pub const OCCLUSION_PENDING: u32 = 3;
/// The bytes are in. [`shim::__crcbl_web_gpu_probe_occlusion_bytes_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_occlusion_bytes_len`] carry them — one
/// little-endian `u64` per query, and every byte zero if the resolve reached the
/// destination and the set holds the unwritten queries it was created with.
pub const OCCLUSION_READY: u32 = 4;
/// The committed reply buffer would not decode, or answered a command nobody
/// asked; the reason is the [`DecodeError`](crate::DecodeError).
/// [`DRAW_UNDECODABLE`]'s twin.
pub const OCCLUSION_UNDECODABLE: u32 = 5;

/// Nothing has asked for the values, or there is no channel to ask through.
pub const OCCLUSION_VALUES_UNASKED: u32 = 0;
/// The [`query_results`](crate::StreamWriter::query_results) ask is on the
/// stream and its [`Reply::QueryResults`] has not arrived.
///
/// **No poll, unlike every readback here.** The replayer answers this one when
/// its own map settles, so there is nothing for a later frame to ask again —
/// which is also why there is no pending code between this and
/// [`OCCLUSION_VALUES_READY`].
pub const OCCLUSION_VALUES_WAITING: u32 = 1;
/// The values are in.
/// [`shim::__crcbl_web_gpu_probe_occlusion_values_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_occlusion_values_len`] carry them, one
/// little-endian `u64` per query.
///
/// **An empty list is this reply's only way of saying the read failed**, so a
/// `READY` of zero length is a failure rather than a success with nothing in it
/// — see [`Command::QueryResults`](crate::Command::QueryResults).
pub const OCCLUSION_VALUES_READY: u32 = 2;

/// Nothing has asked for a pass's timestamps, or there is no channel to ask
/// through.
pub const TIMESTAMP_UNASKED: u32 = 0;
/// The setup frame — a two-query timestamp set, an encoder that resets it and
/// opens a compute pass naming both queries in its descriptor, the submit, and
/// the [`query_results`](crate::StreamWriter::query_results) ask that reads them
/// — is on the stream, and its [`Reply::QueryResults`] has not arrived.
///
/// **No poll, for [`OCCLUSION_VALUES_WAITING`]'s reason**: the replayer answers
/// when its own map settles.
pub const TIMESTAMP_WAITING: u32 = 1;
/// The values are in. [`shim::__crcbl_web_gpu_probe_timestamp_ptr`] and
/// [`shim::__crcbl_web_gpu_probe_timestamp_len`] carry them — two little-endian
/// `u64`, the tick the pass opened at and the tick it closed at.
///
/// **An empty list means the read failed**, exactly as it does for
/// [`OCCLUSION_VALUES_READY`], and **two zeros mean the pass was accepted and
/// not timed**: an unwritten query resolves to zero by specification, so a
/// browser that took the descriptor and wrote nothing reads back as zeros. That
/// is the outcome this whole capability was refused over until the seam's
/// timestamps moved into the pass descriptor, so it is the outcome group AF
/// asserts against.
pub const TIMESTAMP_READY: u32 = 2;
/// The **device** opened without the browser's `timestamp-query` feature, so
/// nothing was encoded: no `GPUQuerySet` of type `'timestamp'` could exist, and
/// a frame that made an occlusion set instead would pass while proving nothing.
/// [`MSAA_UNSUPPORTED`]'s shape, and
/// [`shim::__crcbl_web_gpu_probe_timestamp_supported`] is what the device
/// reported.
pub const TIMESTAMP_UNSUPPORTED: u32 = 3;

/// Nothing has run the report, or there is no channel to have opened a device
/// through.
pub const PARITY_UNASKED: u32 = 0;
/// No device has opened yet, so there is no [`DeviceCaps`](crcbl_hal::DeviceCaps)
/// to build a [`WebGpuDevice`] around and nothing to
/// report. An ordering rule rather than a failure — wait for
/// [`DEVICE_OPENED`].
pub const PARITY_NO_DEVICE: u32 = 1;
/// Every capability's declaration agrees with
/// [`DIVERGENCES`](crcbl_hal::DIVERGENCES).
pub const PARITY_MATCHED: u32 = 2;
/// At least one declaration disagrees with it, in one direction or the other.
/// [`shim::__crcbl_web_gpu_probe_parity_failures_ptr`] carries which, one per
/// line.
pub const PARITY_MISMATCHED: u32 = 3;

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
/// `GPUFeatureName` behind it, and asks for exactly one optional feature — see
/// the [module docs](self#the-device-this-asks-for-and-why-it-asks-for-so-little)
/// for why the parsimony is the point rather than a placeholder, and why
/// [`Features::TIMESTAMP_QUERY`] is the exception.
#[must_use]
pub const fn probe_device_desc(adapter: AdapterId) -> DeviceDesc<'static> {
    DeviceDesc {
        label: Some("crcbl-webgpu probe"),
        adapter,
        required_features: Features::COMPUTE,
        optional_features: Features::TIMESTAMP_QUERY,
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
/// is deliberately absent, and the reason has changed rather than gone: the
/// seam's variant now carries the `view_type` and `format`
/// `GPUStorageTextureBindingLayout` requires, so a fifth entry here would be
/// expressible — but this probe runs against a real browser and the four above
/// already cover the stride, while a storage texture is the one member whose
/// legality depends on the *format*, which is a table `web/tools/gpu-replay.mjs`
/// drives entry by entry against a stub. Adding it here would move that coverage
/// somewhere it can only be tested one format at a time.
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
// It creates a surface on the page's canvas, configures an **sRGB** swapchain on
// it, acquires the frame, clears that acquired image, copies it out to a host
// buffer and reads it back — so the bytes prove the whole canvas-context path
// (configure, getCurrentTexture, render, copy) ran, AND that the frame was sRGB
// encoded on the way. Every handle it names is `5 << 32` — a generation past the
// copy-chain and fill probes' `4 << 32` — so its live resources never land in
// another probe's slot in the shared page.

/// The colour the present probe clears the acquired frame to —
/// `vec4<f32>(0.25, 0.75, 0.125, 1.0)`.
///
/// **EVERY COMPONENT IS A MID-TONE, AND THAT IS THE WHOLE POINT.** `0.0` and
/// `1.0` are fixed points of the sRGB transfer function, so a probe that cleared
/// to red would read the same bytes back out of a linear target and an sRGB one
/// and could not tell them apart — which is exactly how a browser build that
/// presented every frame a transfer function too dark passed this gate. These
/// three land 73, 34 and 67 byte levels away from their unencoded selves.
pub const PROBE_PRESENT_COLOR: [f32; 4] = [0.25, 0.75, 0.125, 1.0];

/// [`PROBE_PRESENT_COLOR`] as the bytes an **sRGB-encoded** texel holds — what
/// the gate checks every pixel against, and the observable that says the canvas
/// frame was encoded.
///
/// The sRGB transfer function `1.055 * u.powf(1.0 / 2.4) - 0.055` applied to
/// each of the three colour components and scaled to 8 bits: `0.25 → 136.96`,
/// `0.75 → 224.61`, `0.125 → 99.09`, with alpha never encoded. **A linear target
/// reads back `[64, 191, 32, 255]` instead** — that is the bug these numbers
/// exist to catch, and the two sets share no component.
///
/// Rounding is the implementation's, so the gate compares within
/// [`PROBE_PRESENT_COLOR_TOLERANCE`] rather than exactly; the nearest pair of
/// values it must still separate is 34 levels apart.
pub const PROBE_PRESENT_COLOR_BYTES: [u8; 4] = [137, 225, 99, 255];

/// How far off [`PROBE_PRESENT_COLOR_BYTES`] a texel may be.
///
/// Every other probe in this file compares bytes exactly, and can: their colours
/// are exact in 8 bits and pass through no transfer function. This one asks the
/// hardware to *evaluate* one, and the specification fixes neither the precision
/// of that evaluation nor the rounding of the result — `0.75` encodes to
/// `224.61`, which is a tenth of a level from the boundary between two bytes. A
/// window of two levels covers that on any implementation and is still 17 times
/// narrower than the gap to the unencoded value.
pub const PROBE_PRESENT_COLOR_TOLERANCE: u8 = 2;

/// [`PROBE_PRESENT_COLOR`] as the bytes a **linear** target would hold, which is
/// what a canvas presented without the sRGB view reads back.
///
/// Not something the probe expects — it is what the probe must **not** see, and
/// it is written down so the gate's failure detail can name what went wrong
/// rather than only that the numbers were unequal. Each component is
/// `round(u * 255)`.
pub const PROBE_PRESENT_UNENCODED_BYTES: [u8; 4] = [64, 191, 32, 255];

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
/// [`Format::Rgba8UnormSrgb`] surface, `Fifo` and `Opaque` (the two a browser
/// canvas offers), on [`PROBE_PRESENT_SURFACE`].
///
/// **sRGB, AND A CANVAS CANNOT BE CONFIGURED WITH IT.**
/// `GPUCanvasConfiguration.format` takes only `rgba8unorm` and `bgra8unorm` and
/// refuses an `-srgb` one, so this is the format the *engine* asks for and not
/// the one the canvas is configured with: `web/engine/gpu-replay.js` configures
/// the base and names this in `viewFormats`, then views the acquired frame
/// through it. Asking for it here is what makes that path load-bearing —
/// [`SurfaceCaps::preferred_format`](crcbl_hal::SurfaceCaps::preferred_format)
/// hands the engine exactly this on a real canvas, because every pass above the
/// seam writes display-referred values and leaves the encode to the hardware.
#[must_use]
pub const fn probe_present_swapchain_desc() -> SwapchainDesc<'static> {
    SwapchainDesc {
        label: Some("crcbl-webgpu present swapchain"),
        surface: PROBE_PRESENT_SURFACE,
        format: Format::Rgba8UnormSrgb,
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

// The reconfigure probe (group Y): the present probe's frame with one command
// more — the swapchain is created `Rgba8Unorm`, then RECONFIGURED `Bgra8Unorm`
// before the acquire. The bytes read back prove the reconfigure re-ran
// `configure`: a swapchain left `Rgba8Unorm` reads red back as `[255, 0, 0, 255]`,
// while one actually reconfigured to `Bgra8Unorm` reads the same red back in BGRA
// byte order, `[0, 0, 255, 255]`. Every handle it names is `6 << 32` — a
// generation past the present probe's `5 << 32` — so the two never collide in the
// shared page and can both run.

/// The colour the reconfigure probe clears the acquired frame to — opaque red,
/// `vec4<f32>(0.2, 0.4, 0.6, 1.0)`.
///
/// **Its own literal, and not [`PROBE_PRESENT_COLOR`].** Three distinct
/// mid-tones, chosen to be wrong in three different ways at once:
///
/// * **Distinct channels** are what makes the *byte order* observable — the
///   whole point of this probe — and three distinct values pin it harder than
///   red's one-versus-zero, which cannot tell `BGRA` from a channel-reversing
///   bug that also happens to swap the two zeroes.
/// * **Mid-tones** are what makes an *encode* observable. Red was a fixed point
///   of the sRGB transfer function, so a reconfigure that wrongly attached an
///   `-srgb` view format still read back `255` and passed. These do not:
///   unencoded they are `51, 102, 153`, and through an sRGB view they land near
///   `124, 170, 203`.
/// * **Neither format here is sRGB**, so the *correct* answer is the unencoded
///   one. This probe is not asserting an encode happens; it is asserting that
///   nothing encodes when nothing should.
///
/// Each component is an exact eighth-ish of full scale — `0.2 × 255 = 51`
/// exactly, and unorm conversion is round-to-nearest by specification on every
/// backend — so the bytes below can still be compared exactly rather than
/// within a tolerance.
pub const PROBE_RECONFIG_COLOR: [f32; 4] = [0.2, 0.4, 0.6, 1.0];

/// [`PROBE_RECONFIG_COLOR`] as the bytes a `Bgra8Unorm` texel holds — B, G, R,
/// A, so `[153, 102, 51, 255]`. THE OBSERVABLE: only a reconfigure that actually
/// re-ran `configure` with `Bgra8Unorm` produces these; a stub that skipped it
/// leaves the swapchain `Rgba8Unorm` and reads back `[51, 102, 153, 255]`
/// instead, and one that attached an sRGB view format reads back neither.
pub const PROBE_RECONFIG_COLOR_BYTES: [u8; 4] = [153, 102, 51, 255];

/// The surface the reconfigure probe creates on the page's canvas. `6 << 32`.
pub const PROBE_RECONFIG_SURFACE: SurfaceHandle = match SurfaceHandle::from_bits(6 << 32) {
    Some(surface) => surface,
    None => panic!("generation 6 is not zero"),
};

/// The swapchain the reconfigure probe configures, then reconfigures. `6 << 32`.
pub const PROBE_RECONFIG_SWAPCHAIN: SwapchainHandle = match SwapchainHandle::from_bits(6 << 32) {
    Some(swapchain) => swapchain,
    None => panic!("generation 6 is not zero"),
};

/// The descriptor the reconfigure probe first *creates* its swapchain with — a
/// 64×64 [`Format::Rgba8Unorm`] surface, `Fifo` and `Opaque`, on
/// [`PROBE_RECONFIG_SURFACE`]. [`probe_reconfigure_swapchain_desc`] is what it is
/// then reconfigured to.
#[must_use]
pub const fn probe_reconfigure_create_desc() -> SwapchainDesc<'static> {
    SwapchainDesc {
        label: Some("crcbl-webgpu reconfigure swapchain"),
        surface: PROBE_RECONFIG_SURFACE,
        format: Format::Rgba8Unorm,
        extent: (PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
        image_count: 2,
        present_mode: PresentMode::Fifo,
        composite_alpha: CompositeAlpha::Opaque,
    }
}

/// The descriptor the reconfigure probe *reconfigures* its swapchain to — the
/// create descriptor with the format changed to [`Format::Bgra8Unorm`]. The
/// format change is the whole point: the acquired frame comes back in BGRA byte
/// order, which is what [`PROBE_RECONFIG_COLOR_BYTES`] checks.
#[must_use]
pub const fn probe_reconfigure_swapchain_desc() -> SwapchainDesc<'static> {
    SwapchainDesc {
        format: Format::Bgra8Unorm,
        ..probe_reconfigure_create_desc()
    }
}

/// The image handle the acquired frame is filed under. `6 << 32`.
pub const PROBE_RECONFIG_IMAGE: ImageHandle = match ImageHandle::from_bits(6 << 32) {
    Some(image) => image,
    None => panic!("generation 6 is not zero"),
};

/// The image-view handle the acquired frame's view is filed under, and the pass
/// clears. `6 << 32`.
pub const PROBE_RECONFIG_VIEW: ImageViewHandle = match ImageViewHandle::from_bits(6 << 32) {
    Some(view) => view,
    None => panic!("generation 6 is not zero"),
};

/// The buffer handle the presented pixels are copied into and read back from.
/// `6 << 32`.
pub const PROBE_RECONFIG_BUFFER: BufferHandle = match BufferHandle::from_bits(6 << 32) {
    Some(buffer) => buffer,
    None => panic!("generation 6 is not zero"),
};

/// The buffer the presented pixels are copied into and read back from — the
/// readback buffer's shape (`64 * 64 * 4` bytes, [`MemoryLocation::HostReadback`],
/// [`BufferUsage::TRANSFER_DST`]) under [`PROBE_RECONFIG_BUFFER`].
#[must_use]
pub const fn probe_reconfigure_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu reconfigure buffer"),
        size: (PROBE_READBACK_SIZE as u64) * (PROBE_READBACK_SIZE as u64) * 4,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    }
}

/// The queue the reconfigure probe names in its command encoder. `6 << 32`.
pub const PROBE_RECONFIG_QUEUE: QueueHandle = match QueueHandle::from_bits(6 << 32) {
    Some(queue) => queue,
    None => panic!("generation 6 is not zero"),
};

/// The command buffer the reconfigure probe finishes its encoder into. `6 << 32`.
pub const PROBE_RECONFIG_COMMAND_BUFFER: CommandBufferHandle =
    match CommandBufferHandle::from_bits(6 << 32) {
        Some(command_buffer) => command_buffer,
        None => panic!("generation 6 is not zero"),
    };

/// The in-flight readback the reconfigure probe requests and polls. `6 << 32`.
pub const PROBE_RECONFIG_READBACK: ReadbackHandle = match ReadbackHandle::from_bits(6 << 32) {
    Some(readback) => readback,
    None => panic!("generation 6 is not zero"),
};

/// The image→buffer copy that moves the acquired-and-cleared pixels into the
/// readback buffer — tightly packed (`64 × 4 = 256` bytes per row), the whole
/// 64×64 mip-0 slice, under the reconfigure probe's own image and buffer handles.
/// [`probe_present_copy`]'s twin on the reconfigure handles.
#[must_use]
pub const fn probe_reconfigure_copy() -> BufferImageCopy {
    BufferImageCopy {
        buffer: PROBE_RECONFIG_BUFFER,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: PROBE_RECONFIG_IMAGE,
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

// The indirect-draw probe (group Z): the draw probe's frame with the draw made
// INDIRECT. Instead of `draw(0..3, 0..1)` it fills an indirect-args buffer and an
// index buffer with `write_buffer`, binds the index buffer, and records a
// `draw_indexed_indirect` — the live 3D-forward geometry path
// (`GeometryPath::IndirectPerBatch`). The pixels read back are still the
// fragment's red, so a read-back of the draw colour proves the indirect draw put
// exactly what the direct draw would. Every handle it names is `7 << 32` — a
// generation past the reconfigure probe's `6 << 32` — so its live resources never
// land in another probe's slot in the shared page. It creates *three* buffers
// (host readback, indirect args, index), which the one type that carries several
// here distinguishes by index; every other resource is a different type, so a
// shared `7 << 32` is distinct by kind.

/// The clear the indirect pass loads with — the colour the indirect draw must
/// overwrite. [`PROBE_DRAW_CLEAR`]'s blue, decisive for its reason: a stub that
/// records no draw leaves these bytes, so reading the draw colour back is what
/// proves the indirect draw ran.
pub const PROBE_INDIRECT_CLEAR: [f32; 4] = PROBE_DRAW_CLEAR;

/// The colour the fragment shader writes, as the bytes a `Rgba8Unorm` texel holds
/// — opaque red, [`PROBE_DRAW_COLOR_BYTES`]'s value. What the gate checks every
/// pixel against, and what only a real indirect draw of the fullscreen triangle
/// produces.
pub const PROBE_INDIRECT_COLOR_BYTES: [u8; 4] = PROBE_DRAW_COLOR_BYTES;

/// The `VkDrawIndexedIndirectCommand` the args buffer is filled with, as the 20
/// little-endian bytes WebGPU's `drawIndexedIndirect` reads: `indexCount = 3`,
/// `instanceCount = 1`, then `firstIndex = 0`, `baseVertex = 0`,
/// `firstInstance = 0`. Three indices of one instance from the origin — exactly
/// the direct `draw_indexed(0..3, 0, 0..1)` the fullscreen triangle would make.
pub const PROBE_INDIRECT_ARGS_BYTES: [u8; 20] = [
    3, 0, 0, 0, // indexCount = 3
    1, 0, 0, 0, // instanceCount = 1
    0, 0, 0, 0, // firstIndex = 0
    0, 0, 0, 0, // baseVertex = 0
    0, 0, 0, 0, // firstInstance = 0
];

/// The index buffer's contents, as the 8 little-endian bytes of four `Uint16`
/// indices `[0, 1, 2, 0]`. The draw reads the first three — vertex indices 0, 1
/// and 2, the fullscreen triangle's corners — and the fourth pads the write to the
/// 4-byte multiple `queue.writeBuffer` requires.
pub const PROBE_INDIRECT_INDEX_BYTES: [u8; 8] = [0, 0, 1, 0, 2, 0, 0, 0];

/// The queue the indirect probe names in its command encoder. `7 << 32`.
pub const PROBE_INDIRECT_QUEUE: QueueHandle = match QueueHandle::from_bits(7 << 32) {
    Some(queue) => queue,
    None => panic!("generation 7 is not zero"),
};

/// The command buffer the indirect probe finishes its encoder into. `7 << 32`.
pub const PROBE_INDIRECT_COMMAND_BUFFER: CommandBufferHandle =
    match CommandBufferHandle::from_bits(7 << 32) {
        Some(command_buffer) => command_buffer,
        None => panic!("generation 7 is not zero"),
    };

/// The in-flight readback the indirect probe requests and polls. `7 << 32`.
pub const PROBE_INDIRECT_READBACK: ReadbackHandle = match ReadbackHandle::from_bits(7 << 32) {
    Some(readback) => readback,
    None => panic!("generation 7 is not zero"),
};

/// The image the indirect probe renders into and copies out of — the draw probe's
/// 64×64 [`Format::Rgba8Unorm`] colour target and copy source, under its own
/// handle.
#[must_use]
pub const fn probe_indirect_image_desc() -> ImageDesc<'static> {
    ImageDesc {
        label: Some("crcbl-webgpu indirect image"),
        image_type: ImageType::D2,
        extent: Extent3d::d2(PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
        format: Format::Rgba8Unorm,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::COLOR_ATTACHMENT.union(ImageUsage::TRANSFER_SRC),
    }
}

/// The image handle the indirect probe renders into. `7 << 32`.
pub const PROBE_INDIRECT_IMAGE: ImageHandle = match ImageHandle::from_bits(7 << 32) {
    Some(image) => image,
    None => panic!("generation 7 is not zero"),
};

/// The image-view handle the indirect probe's pass clears and draws into.
/// `7 << 32`.
pub const PROBE_INDIRECT_IMAGE_VIEW: ImageViewHandle = match ImageViewHandle::from_bits(7 << 32) {
    Some(view) => view,
    None => panic!("generation 7 is not zero"),
};

/// The view of [`probe_indirect_image_desc`]'s image the indirect pass renders
/// into.
pub const PROBE_INDIRECT_VIEW_DESC: ImageViewDesc<'static> = ImageViewDesc {
    label: Some("crcbl-webgpu indirect view"),
    image: PROBE_INDIRECT_IMAGE,
    view_type: ImageViewType::D2,
    format: Format::Rgba8Unorm,
    range: ImageSubresourceRange::all(Format::Rgba8Unorm),
};

/// The buffer handle the drawn pixels are copied into and read back from.
/// `7 << 32`, index `0`.
pub const PROBE_INDIRECT_BUFFER: BufferHandle = match BufferHandle::from_bits(7 << 32) {
    Some(buffer) => buffer,
    None => panic!("generation 7 is not zero"),
};

/// The buffer the drawn pixels are copied into and read back from — the readback
/// buffer's shape (`64 * 64 * 4` bytes, [`MemoryLocation::HostReadback`],
/// [`BufferUsage::TRANSFER_DST`]) under [`PROBE_INDIRECT_BUFFER`].
#[must_use]
pub const fn probe_indirect_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu indirect buffer"),
        size: (PROBE_READBACK_SIZE as u64) * (PROBE_READBACK_SIZE as u64) * 4,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    }
}

/// The indirect-args buffer handle `write_buffer` fills and
/// `draw_indexed_indirect` reads. `7 << 32`, index `1`.
pub const PROBE_INDIRECT_ARGS_BUFFER: BufferHandle = match BufferHandle::from_bits((7 << 32) | 1) {
    Some(buffer) => buffer,
    None => panic!("generation 7 is not zero"),
};

/// The indirect-args buffer — [`PROBE_INDIRECT_ARGS_BYTES`]'s 20 bytes on the
/// device, [`BufferUsage::INDIRECT`] so it can back the draw and
/// [`BufferUsage::TRANSFER_DST`] so `queue.writeBuffer` can fill it.
#[must_use]
pub const fn probe_indirect_args_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu indirect args buffer"),
        size: PROBE_INDIRECT_ARGS_BYTES.len() as u64,
        usage: BufferUsage::INDIRECT.union(BufferUsage::TRANSFER_DST),
        memory: MemoryLocation::DeviceLocal,
    }
}

/// The index buffer handle `write_buffer` fills and `bind_index_buffer` binds.
/// `7 << 32`, index `2`.
pub const PROBE_INDIRECT_INDEX_BUFFER: BufferHandle = match BufferHandle::from_bits((7 << 32) | 2) {
    Some(buffer) => buffer,
    None => panic!("generation 7 is not zero"),
};

/// The index buffer — [`PROBE_INDIRECT_INDEX_BYTES`]'s 8 bytes on the device,
/// [`BufferUsage::INDEX`] so the pass can bind it and
/// [`BufferUsage::TRANSFER_DST`] so `queue.writeBuffer` can fill it.
#[must_use]
pub const fn probe_indirect_index_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu indirect index buffer"),
        size: PROBE_INDIRECT_INDEX_BYTES.len() as u64,
        usage: BufferUsage::INDEX.union(BufferUsage::TRANSFER_DST),
        memory: MemoryLocation::DeviceLocal,
    }
}

/// The image→buffer copy that moves the drawn pixels into the readback buffer —
/// [`probe_draw_copy`]'s shape under the indirect probe's own handles.
#[must_use]
pub const fn probe_indirect_copy() -> BufferImageCopy {
    BufferImageCopy {
        buffer: PROBE_INDIRECT_BUFFER,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: PROBE_INDIRECT_IMAGE,
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

/// The shader module the indirect probe's frame creates for its pipeline — the
/// draw probe's fullscreen-triangle WGSL ([`PROBE_DRAW_WGSL`]), filed at
/// [`PROBE_INDIRECT_SHADER_MODULE`].
pub const PROBE_INDIRECT_SHADER_MODULE_DESC: ShaderModuleDesc<'static> = ShaderModuleDesc {
    label: Some("crcbl-webgpu indirect shader"),
    spirv: &[],
    wgsl: Some(PROBE_DRAW_WGSL),
    msl: None,
    dxil: &[],
};

/// The shader-module handle the indirect probe's pipeline names. `7 << 32`.
pub const PROBE_INDIRECT_SHADER_MODULE: ShaderModuleHandle =
    match ShaderModuleHandle::from_bits(7 << 32) {
        Some(module) => module,
        None => panic!("generation 7 is not zero"),
    };

/// The pipeline-layout handle the indirect probe's pipeline is built against.
/// `7 << 32`.
pub const PROBE_INDIRECT_PIPELINE_LAYOUT: PipelineLayoutHandle =
    match PipelineLayoutHandle::from_bits(7 << 32) {
        Some(layout) => layout,
        None => panic!("generation 7 is not zero"),
    };

/// The pipeline layout the indirect probe's frame creates. **Empty** — the shaders
/// bind nothing, [`PROBE_DRAW_PIPELINE_LAYOUT_DESC`]'s shape.
pub const PROBE_INDIRECT_PIPELINE_LAYOUT_DESC: PipelineLayoutDesc<'static> = PipelineLayoutDesc {
    label: Some("crcbl-webgpu indirect pipeline layout"),
    bind_group_layouts: &[],
    push_constants: None,
};

/// The graphics-pipeline handle the indirect probe binds and draws with.
/// `7 << 32`.
pub const PROBE_INDIRECT_PIPELINE: GraphicsPipelineHandle =
    match GraphicsPipelineHandle::from_bits(7 << 32) {
        Some(pipeline) => pipeline,
        None => panic!("generation 7 is not zero"),
    };

/// The one colour target [`PROBE_INDIRECT_PIPELINE_DESC`] writes —
/// [`PROBE_DRAW_COLOR_TARGETS`]'s opaque `Rgba8Unorm` target.
pub const PROBE_INDIRECT_COLOR_TARGETS: [ColorTargetState; 1] = [ColorTargetState {
    format: Format::Rgba8Unorm,
    blend: None,
    write_mask: ColorWrites::ALL,
}];

/// The pipeline the indirect probe binds before its indirect draw —
/// [`PROBE_DRAW_PIPELINE_DESC`]'s colour-only fullscreen-triangle pipeline under
/// the indirect probe's own handles.
pub const PROBE_INDIRECT_PIPELINE_DESC: GraphicsPipelineDesc<'static> = GraphicsPipelineDesc {
    label: Some("crcbl-webgpu indirect pipeline"),
    layout: PROBE_INDIRECT_PIPELINE_LAYOUT,
    vertex: ShaderEntry {
        module: PROBE_INDIRECT_SHADER_MODULE,
        entry_point: "vsMain",
    },
    fragment: Some(ShaderEntry {
        module: PROBE_INDIRECT_SHADER_MODULE,
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
    color_targets: &PROBE_INDIRECT_COLOR_TARGETS,
};

// The depth probe (group AA): the one gate that reads a DEPTH PLANE back out of
// the browser. Every handle it names is `8 << 32` — a generation past the
// indirect probe's `7 << 32` — so its four live resources never land in another
// probe's slot in the shared page.
//
// It is here because `Capability::DepthImageCopy` is declared supported on this
// backend and no native test can witness that: the seam suite is a native binary
// and this backend runs in a browser. A `Support::Yes` nothing exercises where it
// actually runs is a claim, and the claims this file exists to turn into evidence
// are exactly the ones whose failure looks like success — a shadow atlas that
// read back as nothing renders a frame in which every surface is lit.

/// The depth value the probe's pass clears its atlas to, and the value every
/// texel must read back as.
///
/// **Not `0.0` and not `1.0`**, which are the two numbers a depth buffer holds
/// when nothing wrote it: a clear that never happened, a copy that moved zeroes,
/// and a plane read at the wrong offset all land on one of those, and this value
/// lands on neither. Exact in `f32` by construction — it is written as an `f32`
/// literal, cleared into a `depth32float` plane that stores the float itself,
/// and copied back as those same four bytes.
pub const PROBE_DEPTH_CLEAR: f32 = 0.4275;

/// Texels across and down the depth atlas the probe clears and reads back.
///
/// [`PROBE_READBACK_SIZE`]'s figure and for a sharper reason: a `depth32float`
/// row this wide is 64 × 4 = 256 bytes, which is exactly WebGPU's `bytesPerRow`
/// alignment, so the tightly packed copy the seam sends is already the aligned
/// copy the browser requires and nothing has to pad.
pub const PROBE_DEPTH_SIZE: u32 = PROBE_READBACK_SIZE;

/// The queue the depth probe names in its command encoder. `8 << 32`.
pub const PROBE_DEPTH_QUEUE: QueueHandle = match QueueHandle::from_bits(8 << 32) {
    Some(queue) => queue,
    None => panic!("generation 8 is not zero"),
};

/// The command buffer the depth probe finishes its encoder into. `8 << 32`.
pub const PROBE_DEPTH_COMMAND_BUFFER: CommandBufferHandle =
    match CommandBufferHandle::from_bits(8 << 32) {
        Some(command_buffer) => command_buffer,
        None => panic!("generation 8 is not zero"),
    };

/// The in-flight readback the depth probe requests and polls. `8 << 32`.
pub const PROBE_DEPTH_READBACK: ReadbackHandle = match ReadbackHandle::from_bits(8 << 32) {
    Some(readback) => readback,
    None => panic!("generation 8 is not zero"),
};

/// The image handle the depth probe clears and copies out of. `8 << 32`.
pub const PROBE_DEPTH_IMAGE: ImageHandle = match ImageHandle::from_bits(8 << 32) {
    Some(image) => image,
    None => panic!("generation 8 is not zero"),
};

/// The depth atlas the probe clears — a 64×64 [`Format::D32Float`] target that
/// is both a depth-stencil attachment and a copy source, which is the shadow
/// atlas's own descriptor at the size a readback wants.
#[must_use]
pub const fn probe_depth_image_desc() -> ImageDesc<'static> {
    ImageDesc {
        label: Some("crcbl-webgpu depth image"),
        image_type: ImageType::D2,
        extent: Extent3d::d2(PROBE_DEPTH_SIZE, PROBE_DEPTH_SIZE),
        format: Format::D32Float,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT.union(ImageUsage::TRANSFER_SRC),
    }
}

/// The image-view handle the depth probe's pass writes through. `8 << 32`.
pub const PROBE_DEPTH_IMAGE_VIEW: ImageViewHandle = match ImageViewHandle::from_bits(8 << 32) {
    Some(view) => view,
    None => panic!("generation 8 is not zero"),
};

/// The view of [`probe_depth_image_desc`]'s image the pass clears.
///
/// [`ImageSubresourceRange::all`] of a depth format is
/// [`ImageAspect::DEPTH`] alone, which is what makes this a depth-only
/// attachment: the replayer reads the plane record off the view and leaves the
/// stencil load and store ops off the `GPURenderPassDepthStencilAttachment`
/// entirely, which WebGPU requires for a format with no stencil plane.
pub const PROBE_DEPTH_VIEW_DESC: ImageViewDesc<'static> = ImageViewDesc {
    label: Some("crcbl-webgpu depth view"),
    image: PROBE_DEPTH_IMAGE,
    view_type: ImageViewType::D2,
    format: Format::D32Float,
    range: ImageSubresourceRange::all(Format::D32Float),
};

/// The buffer handle the depth plane is copied into and read back from.
/// `8 << 32`.
pub const PROBE_DEPTH_BUFFER: BufferHandle = match BufferHandle::from_bits(8 << 32) {
    Some(buffer) => buffer,
    None => panic!("generation 8 is not zero"),
};

/// The buffer the depth plane is copied into and read back from — `64 * 64 * 4`
/// bytes, one `f32` a texel.
#[must_use]
pub const fn probe_depth_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu depth buffer"),
        size: (PROBE_DEPTH_SIZE as u64) * (PROBE_DEPTH_SIZE as u64) * 4,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    }
}

/// The clear the depth pass loads with — [`PROBE_DEPTH_CLEAR`] in the depth
/// slot, and [`ClearValue`]'s own defaults everywhere else.
#[must_use]
pub const fn probe_depth_clear_value() -> ClearValue {
    ClearValue {
        color: [0.0; 4],
        depth: PROBE_DEPTH_CLEAR,
        stencil: 0,
    }
}

/// The depth-stencil attachment the probe's pass writes: the whole atlas
/// cleared and **stored**, with no colour attachment beside it.
///
/// `read_only: false` and [`StoreOp::Store`] are what make the copy afterwards
/// mean something — a discarded depth attachment leaves the plane undefined and
/// the readback then compares against whatever the driver had.
#[must_use]
pub const fn probe_depth_attachment() -> DepthStencilAttachment {
    DepthStencilAttachment {
        view: PROBE_DEPTH_IMAGE_VIEW,
        read_only: false,
        depth_load: LoadOp::Clear,
        depth_store: StoreOp::Store,
        stencil_load: LoadOp::DontCare,
        stencil_store: StoreOp::Discard,
        clear: probe_depth_clear_value(),
    }
}

/// The image→buffer copy that moves the depth plane into the readback buffer.
///
/// **[`ImageAspect::DEPTH`], and the whole subresource.** Both are requirements
/// rather than choices: WebGPU rejects a buffer↔texture copy of a depth format
/// that names `'all'`, and it permits no partial copy of one at all — the origin
/// must be zero and the extent the whole mip. Tightly packed, which at
/// [`PROBE_DEPTH_SIZE`] texels of four bytes is already a 256-aligned row.
#[must_use]
pub const fn probe_depth_copy() -> BufferImageCopy {
    BufferImageCopy {
        buffer: PROBE_DEPTH_BUFFER,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: PROBE_DEPTH_IMAGE,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::DEPTH,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: Extent3d::d2(PROBE_DEPTH_SIZE, PROBE_DEPTH_SIZE),
    }
}

// The stencil probe (group AC): the one gate that shows a SET STENCIL REFERENCE
// deciding which fragments survive. Every handle it names is `9 << 32` — a
// generation past the depth probe's `8 << 32` — so its seven live resources never
// land in another probe's slot in the shared page. It creates two images and two
// views, which the two types that carry two here distinguish by index; the
// buffer, the module, the layouts and the pipeline are each the only one of their
// kind at this generation.
//
// It is here because `Capability::StencilReference` is declared supported on this
// backend and no native test can witness that: the seam suite
// (`exercise_stencil_reference` in `crates/crcbl/tests/hal_seam_e2e.rs`) is a
// native binary and this backend runs in a browser.
//
// **THE OBSERVABLE IS A VALUE, NOT A SURVIVED CALL.** A frame that recorded
// `setStencilReference` and had it ignored raises no error anywhere — the draw
// still runs, the readback still resolves, and the only difference is which
// fragments the stencil test discarded. So the probe is built so that exactly one
// reading follows from "both references were applied" and every other way it can
// go lands somewhere else. See [`probe_stencil_pipeline_desc`] for the three
// outcomes and what each of them means.

/// The stencil value the probe's pass clears its plane to, and the value the
/// first draw's reference matches.
///
/// **Not `0`**, which is WebGPU's own initial reference for a fresh pass and the
/// value a `stencilClearValue` that never crossed would leave: a probe that
/// cleared to zero could not tell "the reference arrived" from "nothing
/// happened and both defaults agreed".
pub const PROBE_STENCIL_CLEARED: u32 = 0x2A;

/// The reference the second draw is given — a value the cleared plane does not
/// hold, so every one of its fragments must be discarded.
pub const PROBE_STENCIL_MISS: u32 = 0x11;

/// The reference baked into the pipeline's
/// [`StencilState::reference`](crcbl_hal::StencilState::reference).
///
/// **It matches nothing.** WebGPU has no pipeline-side reference at all, so the
/// replayer drops this field — and a backend that did honour it instead of the
/// per-pass value would discard *both* draws, which is the third reading the gate
/// distinguishes. Distinct from [`PROBE_STENCIL_CLEARED`] and
/// [`PROBE_STENCIL_MISS`] so it can never be mistaken for either.
pub const PROBE_STENCIL_BAKED: u32 = 0x33;

/// The stencil read mask the comparison sees through — every bit, so the mask is
/// not a second reason a reference could fail to match.
pub const PROBE_STENCIL_READ_MASK: u32 = 0xFF;

/// Texels across and down the colour target the stencil probe draws into and
/// reads back. [`PROBE_READBACK_SIZE`]'s figure, for its 256-byte-row reason.
pub const PROBE_STENCIL_SIZE: u32 = PROBE_READBACK_SIZE;

/// One 8-bit channel as the `f32` a clear colour and a WGSL literal carry it as.
///
/// The probe's three colours are chosen as `Rgba8Unorm` *bytes* — that is what
/// the readback compares — and this is the one place they become floats, so the
/// clear the pass loads with and the bytes the gate asserts cannot drift apart.
const fn unorm8(byte: u8) -> f32 {
    byte as f32 / 255.0
}

/// The colour the pass clears its target to, as the bytes a `Rgba8Unorm` texel
/// holds. **The reading that means the *first* reference never took effect**:
/// neither draw survived, so the pipeline's own [`PROBE_STENCIL_BAKED`] decided
/// both.
///
/// The three colours here are pairwise distinct as multisets, not merely as
/// tuples: no two of them are a channel permutation of each other, so a path that
/// swapped `r` and `b` on the way out cannot turn one into another. Each is a
/// mid-tone — away from `0` and `255`, which is what an untouched or saturated
/// channel reads as.
pub const PROBE_STENCIL_BACKGROUND_BYTES: [u8; 4] = [30, 90, 150, 255];

/// The colour the first draw writes. **The one reading that means both
/// references were applied**, and the only one the gate passes on.
pub const PROBE_STENCIL_FIRST_BYTES: [u8; 4] = [60, 130, 200, 255];

/// The colour the second draw writes. **The reading that means the *second*
/// reference never took effect** — either the call was dropped or the stencil
/// test is not enabled at all, and the draw that should have been discarded drew
/// over the first.
pub const PROBE_STENCIL_SECOND_BYTES: [u8; 4] = [200, 70, 110, 255];

/// The queue the stencil probe names in its command encoder. `9 << 32`.
pub const PROBE_STENCIL_QUEUE: QueueHandle = match QueueHandle::from_bits(9 << 32) {
    Some(queue) => queue,
    None => panic!("generation 9 is not zero"),
};

/// The command buffer the stencil probe finishes its encoder into. `9 << 32`.
pub const PROBE_STENCIL_COMMAND_BUFFER: CommandBufferHandle =
    match CommandBufferHandle::from_bits(9 << 32) {
        Some(command_buffer) => command_buffer,
        None => panic!("generation 9 is not zero"),
    };

/// The in-flight readback the stencil probe requests and polls. `9 << 32`.
pub const PROBE_STENCIL_READBACK: ReadbackHandle = match ReadbackHandle::from_bits(9 << 32) {
    Some(readback) => readback,
    None => panic!("generation 9 is not zero"),
};

/// The colour target the stencil probe draws into and copies out of. `9 << 32`,
/// index `0`.
pub const PROBE_STENCIL_IMAGE: ImageHandle = match ImageHandle::from_bits(9 << 32) {
    Some(image) => image,
    None => panic!("generation 9 is not zero"),
};

/// The colour target's descriptor — the draw probe's image at this generation.
#[must_use]
pub const fn probe_stencil_image_desc() -> ImageDesc<'static> {
    ImageDesc {
        label: Some("crcbl-webgpu stencil colour image"),
        image_type: ImageType::D2,
        extent: Extent3d::d2(PROBE_STENCIL_SIZE, PROBE_STENCIL_SIZE),
        format: Format::Rgba8Unorm,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::COLOR_ATTACHMENT.union(ImageUsage::TRANSFER_SRC),
    }
}

/// The colour target's view. `9 << 32`, index `0`.
pub const PROBE_STENCIL_IMAGE_VIEW: ImageViewHandle = match ImageViewHandle::from_bits(9 << 32) {
    Some(view) => view,
    None => panic!("generation 9 is not zero"),
};

/// The view of [`probe_stencil_image_desc`]'s image the pass renders into.
pub const PROBE_STENCIL_VIEW_DESC: ImageViewDesc<'static> = ImageViewDesc {
    label: Some("crcbl-webgpu stencil colour view"),
    image: PROBE_STENCIL_IMAGE,
    view_type: ImageViewType::D2,
    format: Format::Rgba8Unorm,
    range: ImageSubresourceRange::all(Format::Rgba8Unorm),
};

/// The depth-stencil target whose stencil plane the draws are tested against.
/// `9 << 32`, index `1`.
pub const PROBE_STENCIL_PLANE_IMAGE: ImageHandle = match ImageHandle::from_bits((9 << 32) | 1) {
    Some(image) => image,
    None => panic!("generation 9 is not zero"),
};

/// The depth-stencil target's descriptor.
///
/// **[`Format::D24UnormS8Uint`], which is WebGPU's `depth24plus-stencil8`** — the
/// only stencil format in core WebGPU. Its sibling `depth32float-stencil8` is
/// behind the `depth32float-stencil8` feature, and this probe's device asks for
/// nothing optional, so a probe that named it would be refused on a conforming
/// browser. Nothing here reads the *depth* plane back, which is what makes
/// `depth24plus`'s undefined memory layout irrelevant: the plane is written and
/// tested on the GPU and never copied.
///
/// [`ImageUsage::DEPTH_STENCIL_ATTACHMENT`] alone — no `TRANSFER_SRC`, because
/// the evidence is the colour target and not the plane.
#[must_use]
pub const fn probe_stencil_plane_image_desc() -> ImageDesc<'static> {
    ImageDesc {
        label: Some("crcbl-webgpu stencil plane image"),
        image_type: ImageType::D2,
        extent: Extent3d::d2(PROBE_STENCIL_SIZE, PROBE_STENCIL_SIZE),
        format: Format::D24UnormS8Uint,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
    }
}

/// The depth-stencil target's view. `9 << 32`, index `1`.
pub const PROBE_STENCIL_PLANE_VIEW: ImageViewHandle =
    match ImageViewHandle::from_bits((9 << 32) | 1) {
        Some(view) => view,
        None => panic!("generation 9 is not zero"),
    };

/// The view of [`probe_stencil_plane_image_desc`]'s image the pass attaches.
///
/// [`ImageSubresourceRange::all`] of a depth-stencil format is
/// [`ImageAspect::DEPTH`] **and** [`ImageAspect::STENCIL`], so the replayer
/// records both planes off this view and the attachment reaches WebGPU with all
/// four load and store ops — which a `depth24plus-stencil8` attachment requires
/// and a depth-only one forbids.
pub const PROBE_STENCIL_PLANE_VIEW_DESC: ImageViewDesc<'static> = ImageViewDesc {
    label: Some("crcbl-webgpu stencil plane view"),
    image: PROBE_STENCIL_PLANE_IMAGE,
    view_type: ImageViewType::D2,
    format: Format::D24UnormS8Uint,
    range: ImageSubresourceRange::all(Format::D24UnormS8Uint),
};

/// The buffer the drawn pixels are copied into and read back from. `9 << 32`.
pub const PROBE_STENCIL_BUFFER: BufferHandle = match BufferHandle::from_bits(9 << 32) {
    Some(buffer) => buffer,
    None => panic!("generation 9 is not zero"),
};

/// The readback buffer's descriptor — `64 * 64 * 4` bytes, one `Rgba8Unorm`
/// texel per four.
#[must_use]
pub const fn probe_stencil_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu stencil buffer"),
        size: (PROBE_STENCIL_SIZE as u64) * (PROBE_STENCIL_SIZE as u64) * 4,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    }
}

/// The clear the stencil pass loads with — [`PROBE_STENCIL_BACKGROUND_BYTES`] in
/// the colour slot and [`PROBE_STENCIL_CLEARED`] in the stencil one.
///
/// The depth slot is `0.0` and inert: the pipeline's `depth_compare` is
/// [`CompareOp::Always`] and it writes no depth, so nothing here can discard a
/// fragment for a reason other than the stencil test — which is the whole point
/// of a probe whose evidence is *which* fragments survived.
#[must_use]
pub const fn probe_stencil_clear_value() -> ClearValue {
    ClearValue {
        color: [
            unorm8(PROBE_STENCIL_BACKGROUND_BYTES[0]),
            unorm8(PROBE_STENCIL_BACKGROUND_BYTES[1]),
            unorm8(PROBE_STENCIL_BACKGROUND_BYTES[2]),
            unorm8(PROBE_STENCIL_BACKGROUND_BYTES[3]),
        ],
        depth: 0.0,
        stencil: PROBE_STENCIL_CLEARED,
    }
}

/// The colour attachment the stencil pass writes — cleared to the background and
/// stored, so the copy afterwards reads what the draws left.
#[must_use]
pub const fn probe_stencil_color_attachment() -> ColorAttachment {
    ColorAttachment {
        view: PROBE_STENCIL_IMAGE_VIEW,
        resolve: None,
        load: LoadOp::Clear,
        store: StoreOp::Store,
        clear: probe_stencil_clear_value(),
    }
}

/// The depth-stencil attachment the stencil pass tests against — the plane
/// cleared to [`PROBE_STENCIL_CLEARED`].
///
/// `read_only: false` because a read-only attachment reaches WebGPU with none of
/// the four load and store ops, and the clear is what puts the known value in the
/// plane.
#[must_use]
pub const fn probe_stencil_attachment() -> DepthStencilAttachment {
    DepthStencilAttachment {
        view: PROBE_STENCIL_PLANE_VIEW,
        read_only: false,
        depth_load: LoadOp::Clear,
        depth_store: StoreOp::Discard,
        stencil_load: LoadOp::Clear,
        stencil_store: StoreOp::Store,
        clear: probe_stencil_clear_value(),
    }
}

/// The image→buffer copy that moves the drawn pixels into the readback buffer —
/// the draw probe's copy on this probe's colour target and buffer.
#[must_use]
pub const fn probe_stencil_copy() -> BufferImageCopy {
    BufferImageCopy {
        buffer: PROBE_STENCIL_BUFFER,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: PROBE_STENCIL_IMAGE,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: Extent3d::d2(PROBE_STENCIL_SIZE, PROBE_STENCIL_SIZE),
    }
}

/// Two fullscreen triangles in one WGSL module, each a flat colour of its own.
///
/// **No vertex buffers**, [`PROBE_DRAW_WGSL`]'s trick: `vsMain` positions from
/// `@builtin(vertex_index)` alone and `vertex % 3u` makes vertices `3..6` the
/// same oversized triangle as `0..3`, so one draw of each range covers the whole
/// target. What differs between them is the colour, chosen by `vertex < 3u` —
/// [`PROBE_STENCIL_FIRST_BYTES`] for the first, [`PROBE_STENCIL_SECOND_BYTES`]
/// for the second — and carried to the fragment stage flat, since all three
/// vertices of a triangle agree.
///
/// **Each channel is spelled `n.0/255.0`**, the byte the readback is compared
/// against divided by the `Rgba8Unorm` maximum, rather than a decimal that would
/// have to be kept in step by hand. `the_stencil_wgsl_paints_the_colours_the_gate_asserts`
/// is what holds the string to the constants.
pub const PROBE_STENCIL_WGSL: &str = concat!(
    "struct VsOut { @builtin(position) position: vec4<f32>, ",
    "@location(0) colour: vec4<f32> }; ",
    "@vertex fn vsMain(@builtin(vertex_index) vertex: u32) -> VsOut { ",
    "var positions = array<vec2<f32>, 3>(",
    "vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0)); ",
    "var out: VsOut; ",
    "out.position = vec4<f32>(positions[vertex % 3u], 0.0, 1.0); ",
    "out.colour = select(",
    "vec4<f32>(200.0/255.0, 70.0/255.0, 110.0/255.0, 255.0/255.0), ",
    "vec4<f32>(60.0/255.0, 130.0/255.0, 200.0/255.0, 255.0/255.0), ",
    "vertex < 3u); ",
    "return out; } ",
    "@fragment fn fsMain(in: VsOut) -> @location(0) vec4<f32> { return in.colour; }"
);

/// The shader module the stencil probe's frame creates. WGSL only, on
/// [`PROBE_GRAPHICS_SHADER_MODULE_DESC`]'s terms.
pub const PROBE_STENCIL_SHADER_MODULE_DESC: ShaderModuleDesc<'static> = ShaderModuleDesc {
    label: Some("crcbl-webgpu stencil shader"),
    spirv: &[],
    wgsl: Some(PROBE_STENCIL_WGSL),
    msl: None,
    dxil: &[],
};

/// The shader-module handle the stencil probe's pipeline names. `9 << 32`.
pub const PROBE_STENCIL_SHADER_MODULE: ShaderModuleHandle =
    match ShaderModuleHandle::from_bits(9 << 32) {
        Some(module) => module,
        None => panic!("generation 9 is not zero"),
    };

/// The pipeline-layout handle the stencil probe's pipeline is built against.
/// `9 << 32`.
pub const PROBE_STENCIL_PIPELINE_LAYOUT: PipelineLayoutHandle =
    match PipelineLayoutHandle::from_bits(9 << 32) {
        Some(layout) => layout,
        None => panic!("generation 9 is not zero"),
    };

/// The pipeline layout the stencil probe's frame creates. **Empty** — the shaders
/// bind nothing.
pub const PROBE_STENCIL_PIPELINE_LAYOUT_DESC: PipelineLayoutDesc<'static> = PipelineLayoutDesc {
    label: Some("crcbl-webgpu stencil pipeline layout"),
    bind_group_layouts: &[],
    push_constants: None,
};

/// The graphics-pipeline handle the stencil probe binds and draws with.
/// `9 << 32`.
pub const PROBE_STENCIL_PIPELINE: GraphicsPipelineHandle =
    match GraphicsPipelineHandle::from_bits(9 << 32) {
        Some(pipeline) => pipeline,
        None => panic!("generation 9 is not zero"),
    };

/// The one colour target the stencil pipeline writes — opaque, so a surviving
/// fragment's colour reaches the texel exactly rather than blended with the
/// clear underneath it.
pub const PROBE_STENCIL_COLOR_TARGETS: [ColorTargetState; 1] = [ColorTargetState {
    format: Format::Rgba8Unorm,
    blend: None,
    write_mask: ColorWrites::ALL,
}];

/// Both faces of the stencil test: compare [`CompareOp::Equal`] and write
/// nothing back whatever happens.
///
/// Front and back are identical because the triangles are not culled and their
/// winding is not the subject; keeping every op [`StencilOp::Keep`] beside
/// `write_mask: 0` is what makes the second draw see the same plane the first
/// one did.
const PROBE_STENCIL_FACE: crcbl_hal::StencilFaceState = crcbl_hal::StencilFaceState {
    compare: CompareOp::Equal,
    fail_op: crcbl_hal::StencilOp::Keep,
    depth_fail_op: crcbl_hal::StencilOp::Keep,
    pass_op: crcbl_hal::StencilOp::Keep,
};

/// The pipeline the stencil probe binds before both of its draws.
///
/// # The three readings, and why only one of them is a pass
///
/// The plane is cleared to [`PROBE_STENCIL_CLEARED`] and this pipeline compares
/// [`CompareOp::Equal`] against it, writing nothing back. Two draws follow, in one
/// pass, with the reference set differently before each: [`PROBE_STENCIL_CLEARED`]
/// then the first triangle, [`PROBE_STENCIL_MISS`] then the second. So
///
/// * [`PROBE_STENCIL_FIRST_BYTES`] is the only reading a browser that applied
///   both values produces — the first draw passed the test, the second was
///   discarded;
/// * [`PROBE_STENCIL_SECOND_BYTES`] means the second reference never took effect,
///   so the draw that should have been discarded drew over the first;
/// * [`PROBE_STENCIL_BACKGROUND_BYTES`] means the *first* reference never took
///   effect either, which is what the pipeline's own [`PROBE_STENCIL_BAKED`]
///   produces.
///
/// **The order is not free to reverse.** Drawing the rejected reference first
/// would make "the stencil test is not enabled" and "both references were
/// applied" produce the same texel, because the last draw would win either way.
///
/// `depth_write: false` with [`CompareOp::Always`] keeps the depth half inert, so
/// a discarded fragment has exactly one explanation.
#[must_use]
pub const fn probe_stencil_pipeline_desc() -> GraphicsPipelineDesc<'static> {
    GraphicsPipelineDesc {
        label: Some("crcbl-webgpu stencil pipeline"),
        layout: PROBE_STENCIL_PIPELINE_LAYOUT,
        vertex: ShaderEntry {
            module: PROBE_STENCIL_SHADER_MODULE,
            entry_point: "vsMain",
        },
        fragment: Some(ShaderEntry {
            module: PROBE_STENCIL_SHADER_MODULE,
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
            format: Format::D24UnormS8Uint,
            depth_write: false,
            depth_compare: CompareOp::Always,
            stencil: Some(crcbl_hal::StencilState {
                front: PROBE_STENCIL_FACE,
                back: PROBE_STENCIL_FACE,
                read_mask: PROBE_STENCIL_READ_MASK,
                write_mask: 0,
                reference: PROBE_STENCIL_BAKED,
            }),
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
        color_targets: &PROBE_STENCIL_COLOR_TARGETS,
    }
}

// The MSAA-resolve probe (group AD): the one gate that shows a MULTISAMPLED PASS
// RESOLVING into the single-sampled view it named. Every handle it names is
// `10 << 32` — a generation past the stencil probe's `9 << 32` — so its seven live
// resources never land in another probe's slot in the shared page. It creates two
// images, two views and two buffers, which the three types that carry two here
// distinguish by index.
//
// It is here because `Capability::MsaaResolveAttachment` is declared supported on
// this backend and no native test can witness that: the seam suite
// (`exercise_msaa_resolve` in `crates/crcbl/tests/hal_seam_e2e.rs`) is a native
// binary and this backend runs in a browser. Until this probe, every
// `ColorAttachment` this crate built anywhere carried `resolve: None`, so nothing
// on this backend had ever put a resolve view on the wire.
//
// **A CLEAR NEEDS NO PIPELINE**, which is why this probe compiles no shader at
// all — `exercise_msaa_resolve`'s reasoning exactly. A resolve is an *end-of-pass*
// operation over whatever the samples hold, and `LoadOp::Clear` puts a known value
// in every one of them without a draw. So the pass has no contents: it clears the
// multisampled target, names the single-sampled view in
// `ColorAttachment::resolve`, and ends.
//
// **THE OBSERVABLE IS A VALUE, NOT A SURVIVED CALL.** A frame whose resolve view
// was dropped raises no error anywhere — the pass runs, the copy runs, the
// readback resolves, and only the texels differ. So the resolve target is PRIMED
// with `PROBE_MSAA_POISON_BYTES` through a buffer→image copy first, and the poison
// surviving is the one reading that means the resolve was accepted and never
// performed. That is not a hypothetical: it is the bug that shipped in
// `crcbl-wgpu` once, where every colour attachment was built with
// `resolve_target: None` — no error, no log, a wrong picture.

/// How many samples the colour target this probe resolves *from* carries.
///
/// **Asked of the device, not assumed.** `Probe::request_msaa` compares this
/// against the `max_sample_count` the opened device reported and encodes nothing
/// if the device is below it, so a browser that cannot serve a 4× target leaves
/// the group unexercised rather than passing on a target it never made. What the
/// browser reports comes from `MAX_SAMPLE_COUNT` in `web/engine/gpu-replay.js`,
/// which is the specification's fixed "exactly 1 or 4" rather than a hardware
/// query — WebGPU has no limit to read a larger count from.
pub const PROBE_MSAA_SAMPLES: u32 = 4;

/// Texels across both of the probe's targets.
///
/// [`PROBE_READBACK_SIZE`]'s figure and its reason: a tightly packed
/// [`Format::Rgba8Unorm`] row this wide is exactly 256 bytes, which is the row
/// pitch `copyBufferToTexture` and `copyTextureToBuffer` both require. Every other
/// probe here picks its width the same way.
pub const PROBE_MSAA_WIDTH: u32 = PROBE_READBACK_SIZE;

/// Texels down both of them.
///
/// **More than one row**, `MSAA_RESOLVE_HEIGHT`'s reason in the native exercise:
/// a resolve that wrote only the first row of the target, or a copy that read only
/// the first row of the buffer, fails here instead of passing.
pub const PROBE_MSAA_HEIGHT: u32 = 4;

/// How many bytes one whole copy of the resolve target is.
pub const PROBE_MSAA_BYTES: u64 = (PROBE_MSAA_WIDTH as u64) * (PROBE_MSAA_HEIGHT as u64) * 4;

/// The colour the multisampled attachment is cleared to, as the bytes an
/// `Rgba8Unorm` texel holds. **The one reading that means the resolve happened.**
///
/// Three distinct mid-tone channels — away from the `0` and `255` an untouched or
/// saturated one reads as — and no permutation of [`PROBE_MSAA_POISON_BYTES`], so
/// a path that swapped `r` and `b` on the way out cannot turn one reading into the
/// other.
///
/// [`Format::Rgba8Unorm`] rather than its `-srgb` counterpart, which is what lets
/// this be an exact byte comparison like every other group here: the byte a
/// channel lands on is the byte [`probe_msaa_clear_value`] put in, with no
/// transfer function and no rounding to argue about. The native exercise resolves
/// an sRGB target instead and compares against computed levels within a
/// tolerance; group X is where an sRGB encode is what this backend is held to.
pub const PROBE_MSAA_CLEAR_BYTES: [u8; 4] = [75, 160, 115, 255];

/// What every byte of the resolve target holds **before** the pass, put there by
/// a buffer→image copy.
///
/// **The reading that means the resolve was accepted and dropped.** A fresh
/// texture's contents are undefined on this seam, so without this a backend that
/// never resolved and one that did would be told apart only by luck.
///
/// Its alpha differs from [`PROBE_MSAA_CLEAR_BYTES`]' too, so "the resolve wrote
/// the colour channels and left alpha alone" is a fourth distinguishable outcome
/// rather than a pass.
pub const PROBE_MSAA_POISON_BYTES: [u8; 4] = [165, 60, 210, 17];

/// The bytes the prime buffer is filled with — [`PROBE_MSAA_POISON_BYTES`]
/// repeated over the whole of the resolve target.
///
/// Built rather than spelled out, because [`PROBE_MSAA_BYTES`] of literals is not
/// something a reader can check by eye.
#[must_use]
pub const fn probe_msaa_prime_bytes() -> [u8; PROBE_MSAA_BYTES as usize] {
    let mut bytes = [0u8; PROBE_MSAA_BYTES as usize];
    let mut at = 0;
    while at < bytes.len() {
        bytes[at] = PROBE_MSAA_POISON_BYTES[at % PROBE_MSAA_POISON_BYTES.len()];
        at += 1;
    }
    bytes
}

/// The queue the MSAA probe names in its command encoder. `10 << 32`.
pub const PROBE_MSAA_QUEUE: QueueHandle = match QueueHandle::from_bits(10 << 32) {
    Some(queue) => queue,
    None => panic!("generation 10 is not zero"),
};

/// The command buffer the MSAA probe finishes its encoder into. `10 << 32`.
pub const PROBE_MSAA_COMMAND_BUFFER: CommandBufferHandle =
    match CommandBufferHandle::from_bits(10 << 32) {
        Some(command_buffer) => command_buffer,
        None => panic!("generation 10 is not zero"),
    };

/// The in-flight readback the MSAA probe requests and polls. `10 << 32`.
pub const PROBE_MSAA_READBACK: ReadbackHandle = match ReadbackHandle::from_bits(10 << 32) {
    Some(readback) => readback,
    None => panic!("generation 10 is not zero"),
};

/// The multisampled colour target the pass clears and resolves **from**.
/// `10 << 32`, index `0`.
pub const PROBE_MSAA_IMAGE: ImageHandle = match ImageHandle::from_bits(10 << 32) {
    Some(image) => image,
    None => panic!("generation 10 is not zero"),
};

/// The multisampled target's descriptor.
///
/// **[`ImageUsage::COLOR_ATTACHMENT`] alone.** Nothing copies this image and
/// nothing samples it — a multisampled transfer source is a usage WebGPU does not
/// have at all, and the whole point is that the single-sampled target beside it is
/// what leaves the device.
#[must_use]
pub const fn probe_msaa_image_desc() -> ImageDesc<'static> {
    ImageDesc {
        label: Some("crcbl-webgpu msaa source image"),
        image_type: ImageType::D2,
        extent: Extent3d::d2(PROBE_MSAA_WIDTH, PROBE_MSAA_HEIGHT),
        format: Format::Rgba8Unorm,
        mip_levels: 1,
        samples: PROBE_MSAA_SAMPLES,
        usage: ImageUsage::COLOR_ATTACHMENT,
    }
}

/// The single-sampled target the pass resolves **into**. `10 << 32`, index `1`.
pub const PROBE_MSAA_RESOLVE_IMAGE: ImageHandle = match ImageHandle::from_bits((10 << 32) | 1) {
    Some(image) => image,
    None => panic!("generation 10 is not zero"),
};

/// The resolve target's descriptor.
///
/// All three usages are load-bearing and each names one step of the exercise:
/// [`ImageUsage::TRANSFER_DST`] for the prime that writes the poison,
/// [`ImageUsage::COLOR_ATTACHMENT`] because a WebGPU `resolveTarget` is an
/// attachment, and [`ImageUsage::TRANSFER_SRC`] for the copy that reads it back.
#[must_use]
pub const fn probe_msaa_resolve_image_desc() -> ImageDesc<'static> {
    ImageDesc {
        label: Some("crcbl-webgpu msaa resolve image"),
        image_type: ImageType::D2,
        extent: Extent3d::d2(PROBE_MSAA_WIDTH, PROBE_MSAA_HEIGHT),
        format: Format::Rgba8Unorm,
        mip_levels: 1,
        samples: 1,
        usage: ImageUsage::COLOR_ATTACHMENT
            .union(ImageUsage::TRANSFER_SRC)
            .union(ImageUsage::TRANSFER_DST),
    }
}

/// The view of the multisampled target the pass renders through. `10 << 32`,
/// index `0`.
pub const PROBE_MSAA_IMAGE_VIEW: ImageViewHandle = match ImageViewHandle::from_bits(10 << 32) {
    Some(view) => view,
    None => panic!("generation 10 is not zero"),
};

/// The view of [`probe_msaa_image_desc`]'s image the pass attaches.
///
/// **[`ImageViewType::D2`] of a multisampled image, which nothing in this crate
/// had ever asked for.** [`ImageViewType`] has one D2 spelling and no multisampled
/// one, so a backend has to read the sample count off the image it is viewing;
/// WebGPU agrees, and `'2d'` is the only view dimension it permits of a
/// multisampled texture.
pub const PROBE_MSAA_VIEW_DESC: ImageViewDesc<'static> = ImageViewDesc {
    label: Some("crcbl-webgpu msaa source view"),
    image: PROBE_MSAA_IMAGE,
    view_type: ImageViewType::D2,
    format: Format::Rgba8Unorm,
    range: ImageSubresourceRange::all(Format::Rgba8Unorm),
};

/// The view the pass names in [`ColorAttachment::resolve`]. `10 << 32`, index `1`.
pub const PROBE_MSAA_RESOLVE_VIEW: ImageViewHandle =
    match ImageViewHandle::from_bits((10 << 32) | 1) {
        Some(view) => view,
        None => panic!("generation 10 is not zero"),
    };

/// The view of [`probe_msaa_resolve_image_desc`]'s image the resolve writes.
pub const PROBE_MSAA_RESOLVE_VIEW_DESC: ImageViewDesc<'static> = ImageViewDesc {
    label: Some("crcbl-webgpu msaa resolve view"),
    image: PROBE_MSAA_RESOLVE_IMAGE,
    view_type: ImageViewType::D2,
    format: Format::Rgba8Unorm,
    range: ImageSubresourceRange::all(Format::Rgba8Unorm),
};

/// The buffer `write_buffer` fills with the poison and the prime copies out of.
/// `10 << 32`, index `0`.
pub const PROBE_MSAA_PRIME_BUFFER: BufferHandle = match BufferHandle::from_bits(10 << 32) {
    Some(buffer) => buffer,
    None => panic!("generation 10 is not zero"),
};

/// The prime buffer — [`BufferUsage::TRANSFER_DST`] so `queue.writeBuffer` can
/// fill it and [`BufferUsage::TRANSFER_SRC`] so the copy can read it, on the
/// device rather than host-visible because nothing maps it.
#[must_use]
pub const fn probe_msaa_prime_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu msaa prime buffer"),
        size: PROBE_MSAA_BYTES,
        usage: BufferUsage::TRANSFER_SRC.union(BufferUsage::TRANSFER_DST),
        memory: MemoryLocation::DeviceLocal,
    }
}

/// The buffer the resolved texels are copied into and read back from.
/// `10 << 32`, index `1`.
pub const PROBE_MSAA_BUFFER: BufferHandle = match BufferHandle::from_bits((10 << 32) | 1) {
    Some(buffer) => buffer,
    None => panic!("generation 10 is not zero"),
};

/// The readback buffer — the shape every readback probe here uses, at this
/// probe's size.
///
/// Its own buffer rather than the prime's, because WebGPU lets `MAP_READ` combine
/// with `COPY_DST` and nothing else: one buffer cannot both be copied out of and
/// mapped.
#[must_use]
pub const fn probe_msaa_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu msaa buffer"),
        size: PROBE_MSAA_BYTES,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    }
}

/// The clear the MSAA pass loads its multisampled attachment with —
/// [`PROBE_MSAA_CLEAR_BYTES`] as the floats a clear value carries, and
/// [`ClearValue`]'s own defaults in the slots a colour-only pass does not use.
#[must_use]
pub const fn probe_msaa_clear_value() -> ClearValue {
    ClearValue {
        color: [
            unorm8(PROBE_MSAA_CLEAR_BYTES[0]),
            unorm8(PROBE_MSAA_CLEAR_BYTES[1]),
            unorm8(PROBE_MSAA_CLEAR_BYTES[2]),
            unorm8(PROBE_MSAA_CLEAR_BYTES[3]),
        ],
        depth: 0.0,
        stencil: 0,
    }
}

/// The colour attachment that is the whole of the pass: the multisampled view
/// cleared, and the single-sampled view named as its
/// [`resolve`](ColorAttachment::resolve).
///
/// [`StoreOp::Store`] rather than [`StoreOp::Discard`], which WebGPU would also
/// accept beside a resolve: storing keeps "the samples were written" and "the
/// resolve ran" as two separate things the frame did, so a discard cannot be the
/// reason the target is empty.
#[must_use]
pub const fn probe_msaa_color_attachment() -> ColorAttachment {
    ColorAttachment {
        view: PROBE_MSAA_IMAGE_VIEW,
        resolve: Some(PROBE_MSAA_RESOLVE_VIEW),
        load: LoadOp::Clear,
        store: StoreOp::Store,
        clear: probe_msaa_clear_value(),
    }
}

/// The buffer→image copy that primes the resolve target with the poison, before
/// the pass that is supposed to overwrite all of it.
#[must_use]
pub const fn probe_msaa_prime_copy() -> BufferImageCopy {
    BufferImageCopy {
        buffer: PROBE_MSAA_PRIME_BUFFER,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: PROBE_MSAA_RESOLVE_IMAGE,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: Extent3d::d2(PROBE_MSAA_WIDTH, PROBE_MSAA_HEIGHT),
    }
}

/// The image→buffer copy that moves the resolved texels out — the same copy the
/// other way round, off the same image.
#[must_use]
pub const fn probe_msaa_copy() -> BufferImageCopy {
    BufferImageCopy {
        buffer: PROBE_MSAA_BUFFER,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: PROBE_MSAA_RESOLVE_IMAGE,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d { x: 0, y: 0, z: 0 },
        image_extent: Extent3d::d2(PROBE_MSAA_WIDTH, PROBE_MSAA_HEIGHT),
    }
}

// The occlusion probe (group AE): the one gate that shows a QUERY SET being
// built, recorded against and read. Every handle it names is `11 << 32` — a
// generation past the MSAA probe's `10 << 32` — so its four live resources never
// land in another probe's slot in the shared page. It creates two buffers, which
// the one type that carries two here distinguishes by index; the query set, the
// queue, the command buffer and the readback are each the only one of their kind
// at this generation.
//
// It is here because `Capability::OcclusionQuery` is declared supported on this
// backend and no native test can witness that: the seam suite
// (`exercise_query_set_creation` in `crates/crcbl/tests/hal_seam_e2e.rs`) is a
// native binary and this backend runs in a browser.
//
// **WHAT THE CAPABILITY CLAIMS IS A SET, AND NOTHING MORE.**
// `crcbl_hal::CommandEncoder` has no begin/end query verb — its whole query
// vocabulary is the reset, the timestamp write and the resolve — so nothing a
// caller records through this seam can ever *write* an occlusion query, on this
// backend or on the Vulkan one. So the observable is not a count: it is that the
// set exists, that the seam's verbs reach the browser naming it, and that a
// resolve of it lands where it was told to.
//
// **THE SENTINEL IS WHAT MAKES THE ZERO MEAN SOMETHING.** An unwritten query
// resolves to zero on the implementation this gate runs against, and zero is
// also what an untouched allocation reads as — so on its own "all zero" is the
// answer a replayer that did nothing would give. The destination is therefore
// filled with `PROBE_OCCLUSION_SENTINEL` first, and the two readings then mean
// two different things:
//
//   every byte zero        the resolve ran and overwrote the sentinel, over a
//                          set the browser really created. The only pass.
//   the sentinel survives  the resolve was dropped, or refused — a validation
//                          error is reported out of band and the copy still
//                          runs, so this is what a refused resolve looks like.
//
// **THE ZERO IS DAWN'S, NOT THE SPECIFICATION'S**, and that is worth stating
// because the gate asserts it: `gpuweb/gpuweb#1072` opened the question of
// whether resolving a never-begun query should be disallowed or should follow
// D3D12 and Metal in producing 0, and the specification's validation rules for
// `resolveQuerySet` (a 256-aligned `destinationOffset`, a `QUERY_RESOLVE`
// destination, a range inside the set) say nothing about the value. This gate
// runs under Chromium on every platform — `web/run-probe-e2e.sh` says so — so
// what it measures is one implementation's answer, and a browser that answered
// otherwise would fail here naming the value it gave rather than passing
// quietly.

/// How many queries the occlusion probe's set holds.
///
/// Thirty-two, which is [`PROBE_OCCLUSION_BYTES`] of resolved values — a
/// multiple of the 256-byte `destinationOffset` alignment WebGPU imposes, so the
/// probe could resolve at a non-zero offset without growing the buffer, and far
/// enough from `1` that a replayer creating a one-query set would be caught by
/// the length that came back.
pub const PROBE_OCCLUSION_QUERIES: u32 = 32;

/// How many bytes [`PROBE_OCCLUSION_QUERIES`] resolve to.
///
/// Eight per query — `wgpu-types`' `QUERY_SIZE`, and what
/// `GPUCommandEncoder.resolveQuerySet` writes per query for both of WebGPU's
/// query types.
pub const PROBE_OCCLUSION_BYTES: u64 = PROBE_OCCLUSION_QUERIES as u64 * 8;

/// The byte the resolve destination is filled with before the resolve.
///
/// **Not zero**, which is what an unwritten query resolves to and what an
/// untouched allocation reads as: the whole point is that a destination the
/// resolve never reached is distinguishable from one it did. Not `0xFF` either,
/// which is the other value a driver or a debug allocator is apt to poison with.
pub const PROBE_OCCLUSION_SENTINEL: u8 = 0xA7;

/// The occlusion set the probe creates, records against and reads. `11 << 32`.
pub const PROBE_OCCLUSION_SET: QuerySetHandle = match QuerySetHandle::from_bits(11 << 32) {
    Some(set) => set,
    None => panic!("generation 11 is not zero"),
};

/// The set's descriptor: [`QueryKind::Occlusion`], which is the one kind this
/// backend serves, at [`PROBE_OCCLUSION_QUERIES`] queries.
#[must_use]
pub const fn probe_occlusion_set_desc() -> QuerySetDesc<'static> {
    QuerySetDesc {
        label: Some("crcbl-webgpu occlusion set"),
        kind: QueryKind::Occlusion,
        count: PROBE_OCCLUSION_QUERIES,
    }
}

/// The buffer the queries resolve into. `11 << 32`, index `0`.
pub const PROBE_OCCLUSION_RESOLVE_BUFFER: BufferHandle = match BufferHandle::from_bits(11 << 32) {
    Some(buffer) => buffer,
    None => panic!("generation 11 is not zero"),
};

/// The resolve destination's descriptor.
///
/// [`BufferUsage::QUERY_RESOLVE`] because WebGPU refuses a resolve into a buffer
/// without it — one of the two rules the replayer names rather than letting the
/// browser report out of band. [`BufferUsage::TRANSFER_DST`] so the sentinel can
/// be written into it and [`BufferUsage::TRANSFER_SRC`] so the copy can read it
/// back out; on the device rather than host-visible, because WebGPU lets
/// `MAP_READ` combine with `COPY_DST` and nothing else.
#[must_use]
pub const fn probe_occlusion_resolve_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu occlusion resolve buffer"),
        size: PROBE_OCCLUSION_BYTES,
        usage: BufferUsage::QUERY_RESOLVE
            .union(BufferUsage::TRANSFER_SRC)
            .union(BufferUsage::TRANSFER_DST),
        memory: MemoryLocation::DeviceLocal,
    }
}

/// The buffer the resolved values are copied into and read back from.
/// `11 << 32`, index `1`.
pub const PROBE_OCCLUSION_BUFFER: BufferHandle = match BufferHandle::from_bits((11 << 32) | 1) {
    Some(buffer) => buffer,
    None => panic!("generation 11 is not zero"),
};

/// The readback buffer — the shape every readback probe here uses, at this
/// probe's size.
#[must_use]
pub const fn probe_occlusion_buffer_desc() -> BufferDesc<'static> {
    BufferDesc {
        label: Some("crcbl-webgpu occlusion buffer"),
        size: PROBE_OCCLUSION_BYTES,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    }
}

/// The sentinel, as the bytes `write_buffer` uploads into the resolve
/// destination.
#[must_use]
pub fn probe_occlusion_sentinel_bytes() -> Vec<u8> {
    vec![PROBE_OCCLUSION_SENTINEL; PROBE_OCCLUSION_BYTES as usize]
}

/// The copy that carries the resolved values into the host-readable buffer.
#[must_use]
pub const fn probe_occlusion_copy() -> BufferCopy {
    BufferCopy {
        src: PROBE_OCCLUSION_RESOLVE_BUFFER,
        src_offset: 0,
        dst: PROBE_OCCLUSION_BUFFER,
        dst_offset: 0,
        size: PROBE_OCCLUSION_BYTES,
    }
}

/// The queue the occlusion probe names in its command encoder. `11 << 32`.
pub const PROBE_OCCLUSION_QUEUE: QueueHandle = match QueueHandle::from_bits(11 << 32) {
    Some(queue) => queue,
    None => panic!("generation 11 is not zero"),
};

/// The command buffer the occlusion probe finishes its encoder into. `11 << 32`.
pub const PROBE_OCCLUSION_COMMAND_BUFFER: CommandBufferHandle =
    match CommandBufferHandle::from_bits(11 << 32) {
        Some(command_buffer) => command_buffer,
        None => panic!("generation 11 is not zero"),
    };

/// Queries the timestamp probe's set holds: one for each boundary of one pass.
///
/// Two, because that is what a pass takes and what the seam's
/// [`PassTimestampWrites`] names. A larger set would leave unwritten queries
/// beside the written ones, and "unwritten resolves to zero" is precisely the
/// value this probe reads a failure as — so every query in this set is one the
/// pass was asked to write.
pub const PROBE_TIMESTAMP_QUERIES: u32 = 2;

/// The timestamp set the probe creates, times a pass with, and reads. `12 << 32`.
pub const PROBE_TIMESTAMP_SET: QuerySetHandle = match QuerySetHandle::from_bits(12 << 32) {
    Some(set) => set,
    None => panic!("generation 12 is not zero"),
};

/// The set's descriptor: [`QueryKind::Timestamp`], the kind that needs the
/// browser's `timestamp-query` feature, at [`PROBE_TIMESTAMP_QUERIES`] queries.
#[must_use]
pub const fn probe_timestamp_set_desc() -> QuerySetDesc<'static> {
    QuerySetDesc {
        label: Some("crcbl-webgpu timestamp set"),
        kind: QueryKind::Timestamp,
        count: PROBE_TIMESTAMP_QUERIES,
    }
}

/// The two queries the timed pass names, in its descriptor.
///
/// **The whole of what this probe is about.** WebGPU takes a timestamp nowhere
/// else, which is why the seam has no free-standing write left; these three
/// fields become `GPUComputePassDescriptor.timestampWrites`' `querySet`,
/// `beginningOfPassWriteIndex` and `endOfPassWriteIndex`.
#[must_use]
pub const fn probe_timestamp_writes() -> PassTimestampWrites {
    PassTimestampWrites {
        set: PROBE_TIMESTAMP_SET,
        beginning_of_pass: 0,
        end_of_pass: 1,
    }
}

/// The pass the two timestamps bracket.
///
/// **Empty on purpose**: a compute pass with no pipeline and no dispatch is a
/// legal WebGPU pass, and what this probe is measuring is whether the browser
/// writes the two queries at all — not how long anything took. A pass with work
/// in it would need a pipeline, a layout and a shader, none of which the claim
/// needs, and the timings a browser reports are quantised anyway.
#[must_use]
pub const fn probe_timestamp_pass_desc() -> ComputePassDesc<'static> {
    ComputePassDesc {
        label: Some("crcbl-webgpu timed pass"),
        timestamp_writes: Some(probe_timestamp_writes()),
    }
}

/// The queue the timestamp probe names in its command encoder. `12 << 32`.
pub const PROBE_TIMESTAMP_QUEUE: QueueHandle = match QueueHandle::from_bits(12 << 32) {
    Some(queue) => queue,
    None => panic!("generation 12 is not zero"),
};

/// The command buffer the timestamp probe finishes its encoder into. `12 << 32`.
pub const PROBE_TIMESTAMP_COMMAND_BUFFER: CommandBufferHandle =
    match CommandBufferHandle::from_bits(12 << 32) {
        Some(buffer) => buffer,
        None => panic!("generation 12 is not zero"),
    };

/// The in-flight readback the occlusion probe requests and polls. `11 << 32`.
pub const PROBE_OCCLUSION_READBACK: ReadbackHandle = match ReadbackHandle::from_bits(11 << 32) {
    Some(readback) => readback,
    None => panic!("generation 11 is not zero"),
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
        /// The bytes read back — 64×64 texels, every one
        /// [`PROBE_PRESENT_COLOR_BYTES`] to within
        /// [`PROBE_PRESENT_COLOR_TOLERANCE`] if the canvas-context path ran and
        /// encoded the frame.
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

/// One reconfigure-and-read-back — [`PresentProbe`]'s state machine again, on the
/// reconfigure path.
///
/// The frame it encodes is the present frame with one command more: the swapchain
/// is created `Rgba8Unorm` and then RECONFIGURED `Bgra8Unorm` before the acquire.
/// It ends in the same `request_readback` and is answered by the same replies, so
/// the transitions mirror [`PresentProbe`]'s exactly — only the bytes it expects
/// differ, being BGRA rather than RGBA red.
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the bytes.
#[derive(Clone, Debug, Default, PartialEq)]
enum ReconfigProbe {
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
        /// The bytes read back — 64×64 `Bgra8Unorm` texels, every one
        /// [`PROBE_RECONFIG_COLOR_BYTES`] if the reconfigure re-ran `configure`.
        bytes: Vec<u8>,
    },
}

impl ReconfigProbe {
    /// The sequence this is waiting on, or `None` if it is not waiting.
    const fn sequence(&self) -> Option<u64> {
        match self {
            Self::Waiting { sequence } => Some(*sequence),
            _ => None,
        }
    }

    /// Take this probe's answer out of a drained frame's replies, if it is
    /// there — [`PresentProbe::absorb`]'s logic, on this probe's sequence.
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

/// One indirect-draw-and-read-back, from the frame that drew to the bytes read
/// back — [`DrawProbe`]'s state machine again, on the indirect path.
///
/// The two probes differ only in the frame they encode: a draw rasterises a
/// triangle with `draw`, this one fills an args buffer and records a
/// `draw_indexed_indirect`. Both end in the same `request_readback` and are
/// answered by the same [`Reply::ReadbackReady`](crate::Reply::ReadbackReady) /
/// [`Reply::ReadbackPending`](crate::Reply::ReadbackPending), so the transitions
/// mirror [`DrawProbe`]'s exactly.
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the bytes.
#[derive(Clone, Debug, Default, PartialEq)]
enum IndirectProbe {
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
        /// drawn colour if the indirect draw ran.
        bytes: Vec<u8>,
    },
}

impl IndirectProbe {
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

/// One depth-plane readback, from the frame that cleared the atlas to the bytes
/// read back — [`DrawProbe`]'s state machine on the depth path.
///
/// The two probes differ only in the frame they encode: a draw rasterises a
/// triangle into a colour attachment, this one clears a `depth32float`
/// attachment and copies its depth plane out. Both end in the same
/// `request_readback` and are answered by the same
/// [`Reply::ReadbackReady`](crate::Reply::ReadbackReady) /
/// [`Reply::ReadbackPending`](crate::Reply::ReadbackPending), so the transitions
/// mirror [`DrawProbe`]'s exactly.
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the bytes.
#[derive(Clone, Debug, Default, PartialEq)]
enum DepthProbe {
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
        /// The bytes read back — one `depth32float` texel per four, every one
        /// [`PROBE_DEPTH_CLEAR`] if the depth copy ran.
        bytes: Vec<u8>,
    },
}

impl DepthProbe {
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

/// One stencil-reference exercise, from the frame that set it up to the bytes
/// that came back.
///
/// [`DrawProbe`]'s state machine on the stencil probe's handle: the setup frame
/// ends in the same `request_readback` and is answered by the same
/// [`Reply::ReadbackReady`](crate::Reply::ReadbackReady) /
/// [`Reply::ReadbackPending`](crate::Reply::ReadbackPending).
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the bytes.
#[derive(Clone, Debug, Default, PartialEq)]
enum StencilProbe {
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
        /// The bytes read back — one `Rgba8Unorm` texel per four, every one
        /// [`PROBE_STENCIL_FIRST_BYTES`] if both references were applied.
        bytes: Vec<u8>,
    },
}

impl StencilProbe {
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

/// One MSAA-resolve exercise, from the frame that set it up to the bytes that
/// came back.
///
/// [`DrawProbe`]'s state machine on the MSAA probe's handle, with one variant the
/// others have no use for: [`Unsupported`](Self::Unsupported), which is the device
/// declining to supply the fixture rather than the exercise failing.
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the bytes.
#[derive(Clone, Debug, Default, PartialEq)]
enum MsaaProbe {
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
        /// The bytes read back — one `Rgba8Unorm` texel per four, every one
        /// [`PROBE_MSAA_CLEAR_BYTES`] if the resolve reached the target.
        bytes: Vec<u8>,
    },
    /// The device reported a
    /// [`max_sample_count`](crcbl_hal::Limits::max_sample_count) below
    /// [`PROBE_MSAA_SAMPLES`], so nothing was encoded and nothing will be.
    Unsupported,
}

impl MsaaProbe {
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

/// One occlusion-query exercise's **readback** half, from the frame that set it
/// up to the bytes that came back.
///
/// [`StencilProbe`]'s state machine on the occlusion probe's handle: the setup
/// frame ends in the same `request_readback` and is answered by the same
/// [`Reply::ReadbackReady`] / [`Reply::ReadbackPending`].
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the bytes.
#[derive(Clone, Debug, Default, PartialEq)]
enum OcclusionProbe {
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
        /// The bytes read back — one little-endian `u64` per eight, every one
        /// zero if the resolve overwrote [`PROBE_OCCLUSION_SENTINEL`].
        bytes: Vec<u8>,
    },
}

impl OcclusionProbe {
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

/// The same exercise's **direct-read** half: the
/// [`query_results`](crate::StreamWriter::query_results) ask and the
/// [`Reply::QueryResults`] that answers it.
///
/// **A second, independent path to one answer.** The readback half above reads
/// the bytes a resolve *this probe recorded* wrote into a buffer it owns; this
/// half asks the replayer to read the same queries its own way — which on WebGPU
/// is a resolve, a copy and a map of the replayer's own making, because a
/// `GPUQuerySet` has no accessor. Two mechanisms, one set, one expected answer.
///
/// **No poll and no pending state**, unlike every readback here: the replayer
/// answers when its map settles rather than when it is asked again, so there is
/// nothing for a later frame to re-ask and nothing between `Waiting` and
/// `Ready`.
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the values.
#[derive(Clone, Debug, Default, PartialEq)]
enum OcclusionValuesProbe {
    /// Nothing has been asked, or the channel had no room.
    #[default]
    Unasked,
    /// The ask is on the stream and its answer has not arrived.
    Waiting {
        /// Sequence of the [`QueryResults`](crate::Command::QueryResults), which
        /// the reply will name.
        sequence: u64,
    },
    /// The values are in — **empty if the replayer could not serve the read**,
    /// which is the only way [`Reply::QueryResults`] can say so.
    Ready {
        /// One little-endian `u64` per query, in query order.
        bytes: Vec<u8>,
    },
}

impl OcclusionValuesProbe {
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
    /// The values are flattened to little-endian bytes rather than carried as
    /// `u64`s, so the shim hands them out through the same pointer-and-length
    /// pair every other probe here uses — and through a *defined* encoding
    /// rather than whatever a `Vec<u64>` happens to occupy.
    fn absorb(&mut self, replies: &[(u64, Reply)]) -> bool {
        let Some(waiting) = self.sequence() else {
            return false;
        };
        let Some((_, reply)) = replies.iter().find(|(sequence, _)| *sequence == waiting) else {
            return false;
        };
        let bytes = match reply {
            Reply::QueryResults { values, .. } => values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect(),
            // A reply of another shape naming this ask is a replayer bug; it is
            // answered exactly once either way, so this cannot go on waiting.
            _ => Vec::new(),
        };
        *self = Self::Ready { bytes };
        true
    }
}

/// A pass's two timestamps: the [`query_results`](crate::StreamWriter::query_results)
/// ask that reads them and the [`Reply::QueryResults`] that answers it.
///
/// [`OcclusionValuesProbe`]'s shape — one ask, no poll, no pending state — on a
/// [`QueryKind::Timestamp`] set. What differs is what the answer means: an
/// occlusion query nothing began legitimately resolves to zero, and a
/// **timestamp** query that resolves to zero is a query the browser never wrote.
/// So this probe's failure and its success are told apart by the values
/// themselves, which is the whole reason `Capability::TimestampQuery` was
/// refused on this backend until the seam's timestamps moved into the pass
/// descriptor: a set that could never be written is a handle a profiler would
/// fill with zeros and report as timings.
///
/// **Not [`Eq`]**, because [`Ready`](Self::Ready) holds the values.
#[derive(Clone, Debug, Default, PartialEq)]
enum TimestampProbe {
    /// Nothing has been asked, or the channel had no room.
    #[default]
    Unasked,
    /// The device opened without `timestamp-query`, so nothing was encoded.
    Unsupported,
    /// The ask is on the stream and its answer has not arrived.
    Waiting {
        /// Sequence of the [`QueryResults`](crate::Command::QueryResults), which
        /// the reply will name.
        sequence: u64,
    },
    /// The values are in — **empty if the replayer could not serve the read**,
    /// and **two zeros if it served a pass nothing timed**.
    Ready {
        /// Two little-endian `u64`: the opening tick, then the closing one.
        bytes: Vec<u8>,
    },
}

impl TimestampProbe {
    /// The sequence this is waiting on, or `None` if it is not waiting.
    const fn sequence(&self) -> Option<u64> {
        match self {
            Self::Waiting { sequence } => Some(*sequence),
            _ => None,
        }
    }

    /// Take this probe's answer out of a drained frame's replies, if it is
    /// there — [`OcclusionValuesProbe::absorb`]'s logic on this probe's
    /// sequence.
    fn absorb(&mut self, replies: &[(u64, Reply)]) -> bool {
        let Some(waiting) = self.sequence() else {
            return false;
        };
        let Some((_, reply)) = replies.iter().find(|(sequence, _)| *sequence == waiting) else {
            return false;
        };
        if let Reply::QueryResults { values, .. } = reply {
            *self = Self::Ready {
                bytes: values
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect(),
            };
            return true;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// The parity report
// ---------------------------------------------------------------------------

/// The last run of the parity report: what every capability answered, and every
/// answer that disagrees with [`DIVERGENCES`](crcbl_hal::DIVERGENCES).
///
/// **The browser counterpart of `the_parity_report_matches_the_reviewed_divergence_list`**
/// in `crates/crcbl/tests/hal_seam_e2e.rs`. That test is a native binary and
/// this backend runs in a browser, so every [`Support`] answer
/// [`WebGpuDevice`] gives is a declaration nothing
/// holds it to — the browser gate's other groups drive the writer and the
/// replayer and never construct a [`Device`](crcbl_hal::Device) at all. This is
/// what constructs one and reads its whole matrix.
#[derive(Debug)]
struct ParityReport {
    /// One of the `PARITY_*` codes.
    state: u32,
    /// How many capabilities were walked — [`Capability::ALL`]'s length once the
    /// report has run, and `0` when it has not. The vacuity guard: a report that
    /// walked nothing agrees with everything.
    checked: u32,
    /// How many of those were **settled** — every verdict except
    /// [`ParityVerdict::UnprovableHere`], which is a device that withheld the
    /// gate and so a run that learnt nothing. See [`Probe::run_parity`] for why
    /// the two differ here and not in the native suite.
    held: u32,
    /// The whole matrix, one `Capability=verdict` token per capability separated
    /// by spaces, so a CI log carries the report on one line rather than only the
    /// verdict.
    report: String,
    /// The disagreements, one per line, in [`Capability::ALL`] order. Empty when
    /// [`state`](Self::state) is [`PARITY_MATCHED`].
    failures: String,
}

impl ParityReport {
    /// A report nothing has run yet.
    const fn new() -> Self {
        Self {
            state: PARITY_UNASKED,
            checked: 0,
            held: 0,
            report: String::new(),
            failures: String::new(),
        }
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
    reconfig: ReconfigProbe,
    /// A decode error the reconfigure drain hit, for [`RECONFIG_UNDECODABLE`]. Its
    /// own string for [`reason`](Self::reason)'s reason.
    reconfig_reason: String,
    indirect: IndirectProbe,
    /// A decode error the indirect-draw drain hit, for [`INDIRECT_UNDECODABLE`].
    /// Its own string for [`reason`](Self::reason)'s reason.
    indirect_reason: String,
    depth: DepthProbe,
    /// A decode error the depth drain hit, for [`DEPTH_UNDECODABLE`]. Its own
    /// string for [`reason`](Self::reason)'s reason.
    depth_reason: String,
    stencil: StencilProbe,
    /// A decode error the stencil drain hit, for [`STENCIL_UNDECODABLE`]. Its own
    /// string for [`reason`](Self::reason)'s reason.
    stencil_reason: String,
    msaa: MsaaProbe,
    /// A decode error the MSAA drain hit, for [`MSAA_UNDECODABLE`]. Its own
    /// string for [`reason`](Self::reason)'s reason.
    msaa_reason: String,
    occlusion: OcclusionProbe,
    /// A decode error the occlusion drain hit, for [`OCCLUSION_UNDECODABLE`].
    /// Its own string for [`reason`](Self::reason)'s reason.
    occlusion_reason: String,
    /// The direct-read half of the same exercise — see [`OcclusionValuesProbe`].
    /// It has no reason of its own: a decode error is one channel's, and
    /// [`occlusion_reason`](Self::occlusion_reason) is where it is reported.
    occlusion_values: OcclusionValuesProbe,
    /// A pass's two timestamps — see [`TimestampProbe`]. No reason of its own,
    /// for [`occlusion_values`](Self::occlusion_values)'s reason.
    timestamp: TimestampProbe,
    /// The last run of the parity report. Nothing on the channel feeds it — see
    /// [`run_parity`](Self::run_parity).
    parity: ParityReport,
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
            reconfig: ReconfigProbe::Unasked,
            reconfig_reason: String::new(),
            indirect: IndirectProbe::Unasked,
            indirect_reason: String::new(),
            depth: DepthProbe::Unasked,
            depth_reason: String::new(),
            stencil: StencilProbe::Unasked,
            stencil_reason: String::new(),
            msaa: MsaaProbe::Unasked,
            msaa_reason: String::new(),
            occlusion: OcclusionProbe::Unasked,
            occlusion_reason: String::new(),
            occlusion_values: OcclusionValuesProbe::Unasked,
            timestamp: TimestampProbe::Unasked,
            parity: ParityReport::new(),
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
                self.reconfig.absorb(&replies);
                self.indirect.absorb(&replies);
                self.depth.absorb(&replies);
                self.stencil.absorb(&replies);
                self.msaa.absorb(&replies);
                self.occlusion.absorb(&replies);
                self.occlusion_values.absorb(&replies);
                self.timestamp.absorb(&replies);
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
                    timestamp_writes: None,
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
                    timestamp_writes: None,
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
    /// [`PROBE_PRESENT_VIEW`] to [`PROBE_PRESENT_COLOR`], the copy out of the
    /// acquired image, the
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
                    timestamp_writes: None,
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

    /// Encode the reconfigure setup frame: [`request_present`](Self::request_present)'s
    /// frame with one command more — the swapchain is created `Rgba8Unorm`, then
    /// *reconfigured* `Bgra8Unorm` before the acquire, so the acquired frame comes
    /// back in BGRA byte order.
    ///
    /// **One frame, many commands, no reply.** It records the surface (naming the
    /// canvas `canvas_id` is the page's key for), the swapchain created
    /// [`Format::Rgba8Unorm`] ([`probe_reconfigure_create_desc`]), the *reconfigure*
    /// of that same swapchain to [`Format::Bgra8Unorm`]
    /// ([`probe_reconfigure_swapchain_desc`]), the acquire (a `getCurrentTexture`
    /// that now hands back a `Bgra8Unorm` texture under [`PROBE_RECONFIG_IMAGE`]),
    /// the host buffer, an encoder, a render pass that clears
    /// [`PROBE_RECONFIG_VIEW`] to red, the copy out, the finish, the submit, the
    /// present (a no-op) and the `request_readback` under [`PROBE_RECONFIG_READBACK`].
    ///
    /// **The reconfigure is the load-bearing command**: without it the acquired
    /// frame is `Rgba8Unorm` and reads red back as `[255, 0, 0, 255]`;
    /// with it the frame is `Bgra8Unorm` and the same red reads back as
    /// [`PROBE_RECONFIG_COLOR_BYTES`]. That byte-order difference is the whole
    /// observable — see [`shim::__crcbl_web_gpu_probe_reconfigure`].
    ///
    /// `false` until a device has opened, [`request_present`](Self::request_present)'s
    /// ordering rule.
    fn request_reconfigure(&mut self, canvas_id: u32) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_surface(PROBE_RECONFIG_SURFACE, canvas_id);
                stream.create_swapchain(PROBE_RECONFIG_SWAPCHAIN, &probe_reconfigure_create_desc());
                stream.reconfigure_swapchain(
                    PROBE_RECONFIG_SWAPCHAIN,
                    &probe_reconfigure_swapchain_desc(),
                );
                stream.acquire_next_frame(
                    PROBE_RECONFIG_SWAPCHAIN,
                    PROBE_RECONFIG_IMAGE,
                    PROBE_RECONFIG_VIEW,
                );
                stream.create_buffer(PROBE_RECONFIG_BUFFER, &probe_reconfigure_buffer_desc());
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu reconfigure encoder"),
                    queue: PROBE_RECONFIG_QUEUE,
                });
                let attachments = [ColorAttachment {
                    view: PROBE_RECONFIG_VIEW,
                    resolve: None,
                    load: LoadOp::Clear,
                    store: StoreOp::Store,
                    clear: ClearValue::color(PROBE_RECONFIG_COLOR),
                }];
                stream.begin_render_pass(&RenderPassDesc {
                    label: Some("crcbl-webgpu reconfigure clear"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    render_area: Rect2d::from_size(PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
                    timestamp_writes: None,
                });
                stream.end_render_pass();
                stream.copy_image_to_buffer(&probe_reconfigure_copy());
                stream.finish(PROBE_RECONFIG_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_RECONFIG_COMMAND_BUFFER]));
                stream.present(&PresentInfo {
                    swapchain: PROBE_RECONFIG_SWAPCHAIN,
                    waits: &[],
                    present_id: None,
                });
                stream.request_readback(
                    PROBE_RECONFIG_READBACK,
                    &ReadbackDesc {
                        label: Some("crcbl-webgpu reconfigure readback"),
                        buffer: PROBE_RECONFIG_BUFFER,
                        offset: 0,
                        size: probe_reconfigure_buffer_desc().size,
                        after: None,
                    },
                )
            })
            .is_some();
        if encoded {
            self.reconfig = ReconfigProbe::Requested;
            self.reconfig_reason.clear();
        }
        encoded
    }

    /// Encode one [`poll_readback`](crate::StreamWriter::poll_readback) for the
    /// reconfigure's readback and register its wait, unless it is already waiting
    /// or ready — [`poll_present`](Self::poll_present)'s protocol on the
    /// reconfigure's handle.
    fn poll_reconfigure(&mut self) -> bool {
        if !matches!(
            self.reconfig,
            ReconfigProbe::Requested | ReconfigProbe::Pending
        ) {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(sequence) =
            channel.encode_awaited(|stream| stream.poll_readback(PROBE_RECONFIG_READBACK))
        else {
            return false;
        };
        self.reconfig = ReconfigProbe::Waiting { sequence };
        true
    }

    /// Drain, absorb, and report where the reconfigure readback has got to.
    fn reconfigure_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.reconfig_reason = error.to_string();
            return RECONFIG_UNDECODABLE;
        }
        match &self.reconfig {
            ReconfigProbe::Unasked => RECONFIG_UNASKED,
            ReconfigProbe::Requested => RECONFIG_REQUESTED,
            ReconfigProbe::Waiting { .. } => RECONFIG_WAITING,
            ReconfigProbe::Pending => RECONFIG_PENDING,
            ReconfigProbe::Ready { .. } => RECONFIG_READY,
        }
    }

    /// The bytes the reconfigure readback came back with, or an empty slice if it
    /// has not.
    fn reconfigure_bytes(&self) -> &[u8] {
        match &self.reconfig {
            ReconfigProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// Encode the indirect-draw setup frame: [`request_draw`](Self::request_draw)'s
    /// frame with the draw made INDIRECT.
    ///
    /// **One frame, many commands, no reply** — the draw probe's shape on the
    /// indirect path, the live 3D-forward geometry path
    /// ([`GeometryPath::IndirectPerBatch`](crcbl_hal::GeometryPath::IndirectPerBatch)).
    /// It records the image, its view, the host readback buffer, the indirect-args
    /// buffer ([`BufferUsage::INDIRECT`]) and the index buffer
    /// ([`BufferUsage::INDEX`]), the pipeline's three resources, then **fills the
    /// args and index buffers with [`write_buffer`](crate::StreamWriter::write_buffer)**
    /// — the args are [`PROBE_INDIRECT_ARGS_BYTES`] (a 3-index single draw), the
    /// indices [`PROBE_INDIRECT_INDEX_BYTES`]. A command encoder, a render pass that
    /// clears [`PROBE_INDIRECT_VIEW_DESC`]'s view to [`PROBE_INDIRECT_CLEAR`], binds
    /// the pipeline, binds the index buffer and records
    /// [`draw_indexed_indirect`](crate::StreamWriter::draw_indexed_indirect) reading
    /// the args at offset 0 with `draw_count` 1. The copy out, the finish, the
    /// submit, and the `request_readback` under [`PROBE_INDIRECT_READBACK`] close it.
    ///
    /// The writes are recorded before the encoder so `queue.writeBuffer` is ordered
    /// on the queue ahead of the submit that reads the buffers.
    ///
    /// None is answered — every handle is caller-allocated — so it is
    /// [`encode`](StreamChannel::encode); the poll is what is awaited.
    ///
    /// `false` until a device has opened, [`request_draw`](Self::request_draw)'s
    /// ordering rule — every command after the resources is a device method.
    fn request_indirect(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_image(PROBE_INDIRECT_IMAGE, &probe_indirect_image_desc());
                stream.create_image_view(PROBE_INDIRECT_IMAGE_VIEW, &PROBE_INDIRECT_VIEW_DESC);
                stream.create_buffer(PROBE_INDIRECT_BUFFER, &probe_indirect_buffer_desc());
                stream.create_buffer(
                    PROBE_INDIRECT_ARGS_BUFFER,
                    &probe_indirect_args_buffer_desc(),
                );
                stream.create_buffer(
                    PROBE_INDIRECT_INDEX_BUFFER,
                    &probe_indirect_index_buffer_desc(),
                );
                stream.create_shader_module(
                    PROBE_INDIRECT_SHADER_MODULE,
                    &PROBE_INDIRECT_SHADER_MODULE_DESC,
                );
                stream.create_pipeline_layout(
                    PROBE_INDIRECT_PIPELINE_LAYOUT,
                    &PROBE_INDIRECT_PIPELINE_LAYOUT_DESC,
                );
                stream.create_graphics_pipeline(
                    PROBE_INDIRECT_PIPELINE,
                    &PROBE_INDIRECT_PIPELINE_DESC,
                );
                stream.write_buffer(PROBE_INDIRECT_ARGS_BUFFER, 0, &PROBE_INDIRECT_ARGS_BYTES);
                stream.write_buffer(PROBE_INDIRECT_INDEX_BUFFER, 0, &PROBE_INDIRECT_INDEX_BYTES);
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu indirect encoder"),
                    queue: PROBE_INDIRECT_QUEUE,
                });
                let attachments = [ColorAttachment {
                    view: PROBE_INDIRECT_IMAGE_VIEW,
                    resolve: None,
                    load: LoadOp::Clear,
                    store: StoreOp::Store,
                    clear: ClearValue::color(PROBE_INDIRECT_CLEAR),
                }];
                stream.begin_render_pass(&RenderPassDesc {
                    label: Some("crcbl-webgpu indirect pass"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    render_area: Rect2d::from_size(PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
                    timestamp_writes: None,
                });
                stream.bind_graphics_pipeline(PROBE_INDIRECT_PIPELINE);
                stream.bind_index_buffer(PROBE_INDIRECT_INDEX_BUFFER, 0, IndexFormat::Uint16);
                stream.draw_indexed_indirect(PROBE_INDIRECT_ARGS_BUFFER, 0, 1, 0);
                stream.end_render_pass();
                stream.copy_image_to_buffer(&probe_indirect_copy());
                stream.finish(PROBE_INDIRECT_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_INDIRECT_COMMAND_BUFFER]));
                stream.request_readback(
                    PROBE_INDIRECT_READBACK,
                    &ReadbackDesc {
                        label: Some("crcbl-webgpu indirect readback"),
                        buffer: PROBE_INDIRECT_BUFFER,
                        offset: 0,
                        size: probe_indirect_buffer_desc().size,
                        after: None,
                    },
                )
            })
            .is_some();
        if encoded {
            self.indirect = IndirectProbe::Requested;
            self.indirect_reason.clear();
        }
        encoded
    }

    /// Encode one [`poll_readback`](crate::StreamWriter::poll_readback) for the
    /// indirect draw's readback and register its wait, unless it is already waiting
    /// or ready — [`poll_draw`](Self::poll_draw)'s protocol on the indirect handle.
    fn poll_indirect(&mut self) -> bool {
        if !matches!(
            self.indirect,
            IndirectProbe::Requested | IndirectProbe::Pending
        ) {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(sequence) =
            channel.encode_awaited(|stream| stream.poll_readback(PROBE_INDIRECT_READBACK))
        else {
            return false;
        };
        self.indirect = IndirectProbe::Waiting { sequence };
        true
    }

    /// Drain, absorb, and report where the indirect-draw readback has got to.
    fn indirect_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.indirect_reason = error.to_string();
            return INDIRECT_UNDECODABLE;
        }
        match &self.indirect {
            IndirectProbe::Unasked => INDIRECT_UNASKED,
            IndirectProbe::Requested => INDIRECT_REQUESTED,
            IndirectProbe::Waiting { .. } => INDIRECT_WAITING,
            IndirectProbe::Pending => INDIRECT_PENDING,
            IndirectProbe::Ready { .. } => INDIRECT_READY,
        }
    }

    /// The bytes the indirect-draw readback came back with, or an empty slice if it
    /// has not.
    fn indirect_bytes(&self) -> &[u8] {
        match &self.indirect {
            IndirectProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// Encode the depth setup frame: a `depth32float` atlas and a view of it, a
    /// host buffer, a pass whose only attachment is that view cleared to
    /// [`PROBE_DEPTH_CLEAR`] and stored, and the copy of its **depth plane** out
    /// to the buffer that is read back.
    ///
    /// **One frame, many commands, no reply** —
    /// [`request_draw`](Self::request_draw)'s shape with the colour attachment
    /// replaced by a depth one and nothing drawn: the clear is the write, which
    /// is why this needs no pipeline and no shader.
    ///
    /// `false` until a device has opened, [`request_readback`](Self::request_readback)'s
    /// ordering rule — every command here is a device method.
    fn request_depth(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_image(PROBE_DEPTH_IMAGE, &probe_depth_image_desc());
                stream.create_image_view(PROBE_DEPTH_IMAGE_VIEW, &PROBE_DEPTH_VIEW_DESC);
                stream.create_buffer(PROBE_DEPTH_BUFFER, &probe_depth_buffer_desc());
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu depth encoder"),
                    queue: PROBE_DEPTH_QUEUE,
                });
                stream.begin_render_pass(&RenderPassDesc {
                    label: Some("crcbl-webgpu depth pass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(probe_depth_attachment()),
                    render_area: Rect2d::from_size(PROBE_DEPTH_SIZE, PROBE_DEPTH_SIZE),
                    timestamp_writes: None,
                });
                stream.end_render_pass();
                stream.copy_image_to_buffer(&probe_depth_copy());
                stream.finish(PROBE_DEPTH_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_DEPTH_COMMAND_BUFFER]));
                stream.request_readback(
                    PROBE_DEPTH_READBACK,
                    &ReadbackDesc {
                        label: Some("crcbl-webgpu depth readback"),
                        buffer: PROBE_DEPTH_BUFFER,
                        offset: 0,
                        size: probe_depth_buffer_desc().size,
                        after: None,
                    },
                )
            })
            .is_some();
        if encoded {
            self.depth = DepthProbe::Requested;
            self.depth_reason.clear();
        }
        encoded
    }

    /// Encode one [`poll_readback`](crate::StreamWriter::poll_readback) for the
    /// depth readback and register its wait, unless it is already waiting or
    /// ready — [`poll_draw`](Self::poll_draw)'s protocol on the depth handle.
    fn poll_depth(&mut self) -> bool {
        if !matches!(self.depth, DepthProbe::Requested | DepthProbe::Pending) {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(sequence) =
            channel.encode_awaited(|stream| stream.poll_readback(PROBE_DEPTH_READBACK))
        else {
            return false;
        };
        self.depth = DepthProbe::Waiting { sequence };
        true
    }

    /// Drain, absorb, and report where the depth readback has got to.
    fn depth_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.depth_reason = error.to_string();
            return DEPTH_UNDECODABLE;
        }
        match &self.depth {
            DepthProbe::Unasked => DEPTH_UNASKED,
            DepthProbe::Requested => DEPTH_REQUESTED,
            DepthProbe::Waiting { .. } => DEPTH_WAITING,
            DepthProbe::Pending => DEPTH_PENDING,
            DepthProbe::Ready { .. } => DEPTH_READY,
        }
    }

    /// The bytes the depth readback came back with, or an empty slice if it has
    /// not.
    fn depth_bytes(&self) -> &[u8] {
        match &self.depth {
            DepthProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// Encode the stencil setup frame: a colour target and a
    /// `depth24plus-stencil8` one, the pipeline that compares the plane
    /// [`CompareOp::Equal`], a pass that clears both and draws twice with a
    /// **different stencil reference before each**, and the copy of the colour
    /// target out to the buffer that is read back.
    ///
    /// **One frame, many commands, no reply** — [`request_draw`](Self::request_draw)'s
    /// shape with a depth-stencil attachment added and a second draw, and the two
    /// `set_stencil_reference` commands that are the whole point: the first names
    /// [`PROBE_STENCIL_CLEARED`], which the plane holds, and the second
    /// [`PROBE_STENCIL_MISS`], which it does not. See
    /// [`probe_stencil_pipeline_desc`] for what each possible reading means.
    ///
    /// `false` until a device has opened, [`request_readback`](Self::request_readback)'s
    /// ordering rule — every command here is a device method.
    fn request_stencil(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_image(PROBE_STENCIL_IMAGE, &probe_stencil_image_desc());
                stream.create_image_view(PROBE_STENCIL_IMAGE_VIEW, &PROBE_STENCIL_VIEW_DESC);
                stream.create_image(PROBE_STENCIL_PLANE_IMAGE, &probe_stencil_plane_image_desc());
                stream.create_image_view(PROBE_STENCIL_PLANE_VIEW, &PROBE_STENCIL_PLANE_VIEW_DESC);
                stream.create_buffer(PROBE_STENCIL_BUFFER, &probe_stencil_buffer_desc());
                stream.create_shader_module(
                    PROBE_STENCIL_SHADER_MODULE,
                    &PROBE_STENCIL_SHADER_MODULE_DESC,
                );
                stream.create_pipeline_layout(
                    PROBE_STENCIL_PIPELINE_LAYOUT,
                    &PROBE_STENCIL_PIPELINE_LAYOUT_DESC,
                );
                stream.create_graphics_pipeline(
                    PROBE_STENCIL_PIPELINE,
                    &probe_stencil_pipeline_desc(),
                );
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu stencil encoder"),
                    queue: PROBE_STENCIL_QUEUE,
                });
                let attachments = [probe_stencil_color_attachment()];
                stream.begin_render_pass(&RenderPassDesc {
                    label: Some("crcbl-webgpu stencil pass"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: Some(probe_stencil_attachment()),
                    render_area: Rect2d::from_size(PROBE_STENCIL_SIZE, PROBE_STENCIL_SIZE),
                    timestamp_writes: None,
                });
                stream.bind_graphics_pipeline(PROBE_STENCIL_PIPELINE);
                stream.set_stencil_reference(PROBE_STENCIL_CLEARED);
                stream.draw(0..3, 0..1);
                stream.set_stencil_reference(PROBE_STENCIL_MISS);
                stream.draw(3..6, 0..1);
                stream.end_render_pass();
                stream.copy_image_to_buffer(&probe_stencil_copy());
                stream.finish(PROBE_STENCIL_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_STENCIL_COMMAND_BUFFER]));
                stream.request_readback(
                    PROBE_STENCIL_READBACK,
                    &ReadbackDesc {
                        label: Some("crcbl-webgpu stencil readback"),
                        buffer: PROBE_STENCIL_BUFFER,
                        offset: 0,
                        size: probe_stencil_buffer_desc().size,
                        after: None,
                    },
                )
            })
            .is_some();
        if encoded {
            self.stencil = StencilProbe::Requested;
            self.stencil_reason.clear();
        }
        encoded
    }

    /// Encode one [`poll_readback`](crate::StreamWriter::poll_readback) for the
    /// stencil readback and register its wait, unless it is already waiting or
    /// ready — [`poll_draw`](Self::poll_draw)'s protocol on the stencil handle.
    fn poll_stencil(&mut self) -> bool {
        if !matches!(
            self.stencil,
            StencilProbe::Requested | StencilProbe::Pending
        ) {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(sequence) =
            channel.encode_awaited(|stream| stream.poll_readback(PROBE_STENCIL_READBACK))
        else {
            return false;
        };
        self.stencil = StencilProbe::Waiting { sequence };
        true
    }

    /// Drain, absorb, and report where the stencil readback has got to.
    fn stencil_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.stencil_reason = error.to_string();
            return STENCIL_UNDECODABLE;
        }
        match &self.stencil {
            StencilProbe::Unasked => STENCIL_UNASKED,
            StencilProbe::Requested => STENCIL_REQUESTED,
            StencilProbe::Waiting { .. } => STENCIL_WAITING,
            StencilProbe::Pending => STENCIL_PENDING,
            StencilProbe::Ready { .. } => STENCIL_READY,
        }
    }

    /// The bytes the stencil readback came back with, or an empty slice if it
    /// has not.
    fn stencil_bytes(&self) -> &[u8] {
        match &self.stencil {
            StencilProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// What the opened device reported as its
    /// [`max_sample_count`](crcbl_hal::Limits::max_sample_count), or `0` if no
    /// device has opened.
    ///
    /// The number [`request_msaa`](Self::request_msaa) decides on, exported so a
    /// gate that finds [`MSAA_UNSUPPORTED`] can say what the device actually
    /// reported instead of guessing why.
    fn msaa_samples(&self) -> u32 {
        self.opened().map_or(0, |caps| caps.limits.max_sample_count)
    }

    /// Encode the MSAA setup frame: a multisampled colour target and a
    /// single-sampled one, the prime that fills the second with the poison, a pass
    /// whose only content is a clear of the first **and which names the second in
    /// [`ColorAttachment::resolve`]**, and the copy of the second out to the buffer
    /// that is read back.
    ///
    /// **One frame, no pipeline, no reply** — the pass has no draws at all, for
    /// the reason the section comment above [`PROBE_MSAA_SAMPLES`] gives: a
    /// resolve is an end-of-pass operation over whatever the samples hold, and a
    /// clear puts a known value in every one of them.
    ///
    /// `false` on three counts, and the third is not a failure:
    ///
    /// * no device has opened — [`request_readback`](Self::request_readback)'s
    ///   ordering rule, since every command here is a device method;
    /// * the channel had no room, or another channel is installed;
    /// * **the device reported a `max_sample_count` below
    ///   [`PROBE_MSAA_SAMPLES`]**, which leaves the probe
    ///   [`MSAA_UNSUPPORTED`] and encodes nothing. There is no multisampled
    ///   target for a resolve to resolve from on such a device, and a frame that
    ///   quietly made a single-sampled one instead would pass while proving
    ///   nothing at all.
    fn request_msaa(&mut self) -> bool {
        let Some(caps) = self.opened() else {
            return false;
        };
        if caps.limits.max_sample_count < PROBE_MSAA_SAMPLES {
            self.msaa = MsaaProbe::Unsupported;
            self.msaa_reason.clear();
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_image(PROBE_MSAA_IMAGE, &probe_msaa_image_desc());
                stream.create_image_view(PROBE_MSAA_IMAGE_VIEW, &PROBE_MSAA_VIEW_DESC);
                stream.create_image(PROBE_MSAA_RESOLVE_IMAGE, &probe_msaa_resolve_image_desc());
                stream.create_image_view(PROBE_MSAA_RESOLVE_VIEW, &PROBE_MSAA_RESOLVE_VIEW_DESC);
                stream.create_buffer(PROBE_MSAA_PRIME_BUFFER, &probe_msaa_prime_buffer_desc());
                stream.create_buffer(PROBE_MSAA_BUFFER, &probe_msaa_buffer_desc());
                stream.write_buffer(PROBE_MSAA_PRIME_BUFFER, 0, &probe_msaa_prime_bytes());
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu msaa encoder"),
                    queue: PROBE_MSAA_QUEUE,
                });
                // The prime, and it is inside the encoder rather than a queue
                // write for the ordering: `copy_buffer_to_image` and the pass that
                // overwrites what it wrote are commands of the same encoder, so
                // the poison is in the target before the resolve and cannot race
                // it.
                stream.copy_buffer_to_image(&probe_msaa_prime_copy());
                let attachments = [probe_msaa_color_attachment()];
                stream.begin_render_pass(&RenderPassDesc {
                    label: Some("crcbl-webgpu msaa resolve pass"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    render_area: Rect2d::from_size(PROBE_MSAA_WIDTH, PROBE_MSAA_HEIGHT),
                    timestamp_writes: None,
                });
                stream.end_render_pass();
                stream.copy_image_to_buffer(&probe_msaa_copy());
                stream.finish(PROBE_MSAA_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_MSAA_COMMAND_BUFFER]));
                stream.request_readback(
                    PROBE_MSAA_READBACK,
                    &ReadbackDesc {
                        label: Some("crcbl-webgpu msaa readback"),
                        buffer: PROBE_MSAA_BUFFER,
                        offset: 0,
                        size: probe_msaa_buffer_desc().size,
                        after: None,
                    },
                )
            })
            .is_some();
        if encoded {
            self.msaa = MsaaProbe::Requested;
            self.msaa_reason.clear();
        }
        encoded
    }

    /// Encode one [`poll_readback`](crate::StreamWriter::poll_readback) for the
    /// MSAA readback and register its wait, unless it is already waiting, ready
    /// or unsupported — [`poll_draw`](Self::poll_draw)'s protocol on the MSAA
    /// handle.
    fn poll_msaa(&mut self) -> bool {
        if !matches!(self.msaa, MsaaProbe::Requested | MsaaProbe::Pending) {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(sequence) =
            channel.encode_awaited(|stream| stream.poll_readback(PROBE_MSAA_READBACK))
        else {
            return false;
        };
        self.msaa = MsaaProbe::Waiting { sequence };
        true
    }

    /// Drain, absorb, and report where the MSAA readback has got to.
    fn msaa_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.msaa_reason = error.to_string();
            return MSAA_UNDECODABLE;
        }
        match &self.msaa {
            MsaaProbe::Unasked => MSAA_UNASKED,
            MsaaProbe::Requested => MSAA_REQUESTED,
            MsaaProbe::Waiting { .. } => MSAA_WAITING,
            MsaaProbe::Pending => MSAA_PENDING,
            MsaaProbe::Ready { .. } => MSAA_READY,
            MsaaProbe::Unsupported => MSAA_UNSUPPORTED,
        }
    }

    /// The bytes the MSAA readback came back with, or an empty slice if it has
    /// not.
    fn msaa_bytes(&self) -> &[u8] {
        match &self.msaa {
            MsaaProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// Encode the occlusion setup frame: an occlusion query set, a resolve
    /// destination filled with [`PROBE_OCCLUSION_SENTINEL`], an encoder that
    /// resets the whole range and resolves it over that sentinel, the copy into a
    /// host-readable buffer, the submit, the readback request — **and the
    /// [`query_results`](crate::StreamWriter::query_results) ask that reads the
    /// same queries the other way**.
    ///
    /// **Both paths in one frame, and that is what makes them a pair.** They read
    /// the same set of the same queries at the same point in the stream: one
    /// through a resolve this probe recorded, one through a resolve the replayer
    /// records for itself, because a `GPUQuerySet` has no accessor. Two
    /// mechanisms disagreeing would be a finding, and issuing them a frame apart
    /// would let a reader wonder whether the set had changed in between.
    ///
    /// The reset is in there deliberately although WebGPU has no reset: the seam
    /// requires every caller to record one, so a frame that skipped it would not
    /// be the frame a caller writes. It is the documented no-op, and what it
    /// exercises is that the replayer takes it and records nothing rather than
    /// refusing the command buffer.
    ///
    /// `false` until a device has opened, [`request_readback`](Self::request_readback)'s
    /// ordering rule — every command here is a device method.
    fn request_occlusion(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_query_set(PROBE_OCCLUSION_SET, &probe_occlusion_set_desc());
                stream.create_buffer(
                    PROBE_OCCLUSION_RESOLVE_BUFFER,
                    &probe_occlusion_resolve_buffer_desc(),
                );
                stream.create_buffer(PROBE_OCCLUSION_BUFFER, &probe_occlusion_buffer_desc());
                stream.write_buffer(
                    PROBE_OCCLUSION_RESOLVE_BUFFER,
                    0,
                    &probe_occlusion_sentinel_bytes(),
                );
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu occlusion encoder"),
                    queue: PROBE_OCCLUSION_QUEUE,
                });
                stream.reset_query_set(PROBE_OCCLUSION_SET, 0..PROBE_OCCLUSION_QUERIES);
                stream.resolve_query_set(
                    PROBE_OCCLUSION_SET,
                    0..PROBE_OCCLUSION_QUERIES,
                    PROBE_OCCLUSION_RESOLVE_BUFFER,
                    0,
                );
                stream.copy_buffer_to_buffer(&probe_occlusion_copy());
                stream.finish(PROBE_OCCLUSION_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_OCCLUSION_COMMAND_BUFFER]));
                stream.request_readback(
                    PROBE_OCCLUSION_READBACK,
                    &ReadbackDesc {
                        label: Some("crcbl-webgpu occlusion readback"),
                        buffer: PROBE_OCCLUSION_BUFFER,
                        offset: 0,
                        size: PROBE_OCCLUSION_BYTES,
                        after: None,
                    },
                )
            })
            .is_some();
        if !encoded {
            return false;
        }
        self.occlusion = OcclusionProbe::Requested;
        self.occlusion_reason.clear();
        // The direct read is *awaited* where everything above is fire-and-forget,
        // so it goes through `encode_awaited`: a `Reply::QueryResults` naming a
        // sequence nobody registered is refused for the whole buffer.
        let Some(channel) = self.channel() else {
            return false;
        };
        if let Some(sequence) = channel.encode_awaited(|stream| {
            stream.query_results(PROBE_OCCLUSION_SET, 0, PROBE_OCCLUSION_QUERIES)
        }) {
            self.occlusion_values = OcclusionValuesProbe::Waiting { sequence };
        }
        true
    }

    /// Encode one [`poll_readback`](crate::StreamWriter::poll_readback) for the
    /// occlusion readback and register its wait, unless it is already waiting or
    /// ready — [`poll_draw`](Self::poll_draw)'s protocol on the occlusion handle.
    ///
    /// The direct read is not polled: the replayer answers it when its own map
    /// settles, so there is nothing to ask again. See [`OcclusionValuesProbe`].
    fn poll_occlusion(&mut self) -> bool {
        if !matches!(
            self.occlusion,
            OcclusionProbe::Requested | OcclusionProbe::Pending
        ) {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let Some(sequence) =
            channel.encode_awaited(|stream| stream.poll_readback(PROBE_OCCLUSION_READBACK))
        else {
            return false;
        };
        self.occlusion = OcclusionProbe::Waiting { sequence };
        true
    }

    /// Drain, absorb, and report where the occlusion readback has got to.
    fn occlusion_state(&mut self) -> u32 {
        if let Some(error) = self.drain() {
            self.occlusion_reason = error.to_string();
            return OCCLUSION_UNDECODABLE;
        }
        match &self.occlusion {
            OcclusionProbe::Unasked => OCCLUSION_UNASKED,
            OcclusionProbe::Requested => OCCLUSION_REQUESTED,
            OcclusionProbe::Waiting { .. } => OCCLUSION_WAITING,
            OcclusionProbe::Pending => OCCLUSION_PENDING,
            OcclusionProbe::Ready { .. } => OCCLUSION_READY,
        }
    }

    /// The bytes the occlusion readback came back with, or an empty slice if it
    /// has not.
    fn occlusion_bytes(&self) -> &[u8] {
        match &self.occlusion {
            OcclusionProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// Drain, absorb, and report where the direct read has got to.
    ///
    /// It shares [`drain`](Self::drain) with the readback half rather than
    /// draining again, which is the module's rule: one drain per frame, dispatched
    /// to every waiter. A decode error is reported through
    /// [`occlusion_state`](Self::occlusion_state) — one channel, one reason.
    fn occlusion_values_state(&mut self) -> u32 {
        let _ = self.drain();
        match &self.occlusion_values {
            OcclusionValuesProbe::Unasked => OCCLUSION_VALUES_UNASKED,
            OcclusionValuesProbe::Waiting { .. } => OCCLUSION_VALUES_WAITING,
            OcclusionValuesProbe::Ready { .. } => OCCLUSION_VALUES_READY,
        }
    }

    /// The values the direct read came back with, as little-endian bytes, or an
    /// empty slice if it has not answered.
    fn occlusion_values_bytes(&self) -> &[u8] {
        match &self.occlusion_values {
            OcclusionValuesProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// Whether the device this page opened has the browser's `timestamp-query`.
    ///
    /// [`msaa_samples`](Self::msaa_samples)' shape: a number the *device*
    /// reported, so a probe that could not run says why rather than being
    /// silently skipped.
    fn timestamp_supported(&mut self) -> bool {
        self.opened()
            .is_some_and(|caps| caps.features.contains(Features::TIMESTAMP_QUERY))
    }

    /// Encode one timed pass and the read of the two queries it names.
    ///
    /// A two-query [`QueryKind::Timestamp`] set, an encoder that resets it, a
    /// compute pass whose descriptor names both queries, the submit, and an
    /// **awaited** [`query_results`](crate::StreamWriter::query_results) — the
    /// same awaited path the occlusion values take, and for the same reason: a
    /// `GPUQuerySet` has no accessor, so the replayer serves the read with a
    /// resolve, a copy and a map of its own.
    ///
    /// Answers `false` and encodes nothing on a device without the feature,
    /// leaving [`TimestampProbe::Unsupported`]: a set of another kind would let
    /// this pass while proving nothing about timestamps.
    fn request_timestamp(&mut self) -> bool {
        if self.opened().is_none() {
            return false;
        }
        if !self.timestamp_supported() {
            self.timestamp = TimestampProbe::Unsupported;
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        let encoded = channel
            .encode(|stream| {
                stream.create_query_set(PROBE_TIMESTAMP_SET, &probe_timestamp_set_desc());
                stream.create_command_encoder(&CommandEncoderDesc {
                    label: Some("crcbl-webgpu timestamp encoder"),
                    queue: PROBE_TIMESTAMP_QUEUE,
                });
                // Unconditional, as the seam asks of every caller: a documented
                // no-op on WebGPU and required on Vulkan.
                stream.reset_query_set(PROBE_TIMESTAMP_SET, 0..PROBE_TIMESTAMP_QUERIES);
                stream.begin_compute_pass(&probe_timestamp_pass_desc());
                stream.end_compute_pass();
                stream.finish(PROBE_TIMESTAMP_COMMAND_BUFFER);
                stream.submit(&SubmitInfo::new(&[PROBE_TIMESTAMP_COMMAND_BUFFER]))
            })
            .is_some();
        if !encoded {
            return false;
        }
        let Some(channel) = self.channel() else {
            return false;
        };
        if let Some(sequence) = channel.encode_awaited(|stream| {
            stream.query_results(PROBE_TIMESTAMP_SET, 0, PROBE_TIMESTAMP_QUERIES)
        }) {
            self.timestamp = TimestampProbe::Waiting { sequence };
        }
        true
    }

    /// Drain, absorb, and report where the timed pass's read has got to.
    ///
    /// Shares [`drain`](Self::drain) with every other probe, which is the
    /// module's rule: one drain per frame, dispatched to every waiter.
    fn timestamp_state(&mut self) -> u32 {
        let _ = self.drain();
        match &self.timestamp {
            TimestampProbe::Unasked => TIMESTAMP_UNASKED,
            TimestampProbe::Unsupported => TIMESTAMP_UNSUPPORTED,
            TimestampProbe::Waiting { .. } => TIMESTAMP_WAITING,
            TimestampProbe::Ready { .. } => TIMESTAMP_READY,
        }
    }

    /// The two ticks the timed pass reported, as little-endian bytes, or an
    /// empty slice if it has not answered.
    fn timestamp_bytes(&self) -> &[u8] {
        match &self.timestamp {
            TimestampProbe::Ready { bytes } => bytes,
            _ => &[],
        }
    }

    /// Walk a real [`WebGpuDevice`]'s whole
    /// [`supports`](crcbl_hal::Device::supports) matrix and hold every answer
    /// against [`DIVERGENCES`](crcbl_hal::DIVERGENCES), in every direction
    /// [`parity_verdict`] distinguishes.
    ///
    /// **`the_parity_report_matches_the_reviewed_divergence_list` in
    /// `crates/crcbl/tests/hal_seam_e2e.rs`, in a browser.** The rule is
    /// [`parity_verdict`]'s and so is that test's: a
    /// [`Support::No`](crcbl_hal::Support::No) needs a row on every device, a
    /// [`Support::Yes`](crcbl_hal::Support::Yes) for a listed pair means the row
    /// is stale, and a
    /// [`Support::NotOnThisDevice`](crcbl_hal::Support::NotOnThisDevice) is
    /// excused only when the device really did withhold the gate.
    ///
    /// # Why this exists at all
    ///
    /// Every `supports` answer this backend gives is a declaration nothing holds
    /// it to. The native seam suite is a native binary and this backend runs in a
    /// browser; the browser gate's other groups drive [`StreamWriter`] and the
    /// replayer and never construct a [`Device`](crcbl_hal::Device). This is what
    /// constructs one and reads its whole matrix.
    ///
    /// # The device is real; its channel is not, and does not need to be
    ///
    /// [`supports`](crcbl_hal::Device::supports) reads
    /// [`DeviceCaps::features`](crcbl_hal::DeviceCaps) and encodes nothing, so
    /// the device is built around [`opened`](Self::opened) — **the caps the
    /// browser reported for the device this page actually opened** — over a
    /// [`SharedChannel::new`] that is never installed and never written to. That
    /// is what makes this report safe to run at any point in the page's order: it
    /// puts no command on the stream, registers no wait and drains nothing, so it
    /// can neither be stranded by work queued ahead of it nor strand anything
    /// behind it. Taking the page's own channel would need a [`SharedChannel`]
    /// built from the probe's [`Rc`], which exists only on `wasm32` — a one-sided
    /// `cfg` for a channel no call here would touch.
    ///
    /// # What this device can and cannot settle
    ///
    /// [`probe_device_desc`] asks for **one optional feature**, so the
    /// capabilities whose answers come from
    /// [`Support::granted`](crcbl_hal::Support::granted) are
    /// [`UnprovableHere`](ParityVerdict::UnprovableHere) on this page — with the
    /// one exception of `TimestampQuery` on a browser that has
    /// `timestamp-query`, which this device does ask for. Everywhere else the
    /// device withheld the gate, and a run that cannot answer is not a pass.
    /// [`held`](ParityReport::held) is the count that *was* settled, beside
    /// [`checked`](ParityReport::checked), so a report cannot look complete by
    /// being empty. Every other capability — every `Support::No` and every
    /// `Support::Yes` — is held here exactly as the native suite holds it.
    fn run_parity(&mut self) -> u32 {
        let Some(caps) = self.opened() else {
            self.parity = ParityReport::new();
            self.parity.state = PARITY_NO_DEVICE;
            return PARITY_NO_DEVICE;
        };
        let device = WebGpuDevice::new(SharedChannel::new(), caps, HandlePool::new());
        // Read off the device rather than written here: a report that named the
        // backend itself would go on agreeing with the list after the device
        // started answering as something else.
        let backend = device.backend();
        let features = device.caps().features;

        let mut report = String::new();
        let mut failures = String::new();
        let mut checked: u32 = 0;
        let mut held: u32 = 0;
        for capability in Capability::ALL {
            let declared = device.supports(*capability);
            let verdict = parity_verdict(*capability, backend, declared, features);
            let why = declared.reason().unwrap_or("no reason given");
            checked = checked.saturating_add(1);
            if !report.is_empty() {
                report.push(' ');
            }
            report.push_str(capability.name());
            match verdict {
                // The stale direction, and the one `parity_verdict` cannot answer
                // on its own: it reports that the backend performs the capability,
                // and whether that contradicts a row is this report's question.
                ParityVerdict::Supported => {
                    held = held.saturating_add(1);
                    match divergence(*capability, backend) {
                        Some(entry) => {
                            report.push_str("=yes:STALE-ROW");
                            let _ = writeln!(
                                failures,
                                "{capability}: DIVERGENCES says {backend} lacks it — {} — and this \
                                 device has it. Delete the entry.",
                                entry.why
                            );
                        }
                        None => report.push_str("=yes"),
                    }
                }
                ParityVerdict::Reviewed(_) => {
                    held = held.saturating_add(1);
                    report.push_str("=no:reviewed");
                }
                // Not a failure and not a pass: this device withheld the gate, so
                // the run learnt nothing about the backend. Counted out of `held`
                // rather than folded into the greens.
                ParityVerdict::UnprovableHere(_) => report.push_str("=unprovable-here"),
                ParityVerdict::Unreviewed => {
                    held = held.saturating_add(1);
                    report.push_str("=no:UNREVIEWED");
                    let _ = writeln!(
                        failures,
                        "{capability}: {backend} refuses it ({why}) and no DIVERGENCES entry says \
                         why. Add one with the reason, or fix the backend.",
                    );
                }
                ParityVerdict::FalseDeviceGate => {
                    held = held.saturating_add(1);
                    report.push_str("=no:FALSE-DEVICE-GATE");
                    let _ = writeln!(
                        failures,
                        "{capability}: {backend} blamed this device ({why}), but the device \
                         withheld nothing — the capability's gate is {:?} and this device reports \
                         {:?}. Declare Support::No with a reason and add a DIVERGENCES row, or fix \
                         the backend.",
                        capability.gating_feature(),
                        features
                    );
                }
            }
        }

        let state = if failures.is_empty() {
            PARITY_MATCHED
        } else {
            PARITY_MISMATCHED
        };
        self.parity = ParityReport {
            state,
            checked,
            held,
            report,
            failures,
        };
        state
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
                    timestamp_writes: None,
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
                    timestamp_writes: None,
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
                    timestamp_writes: None,
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
    /// view, the copy, finish, submit, present and `request_readback` — is on the
    /// stream; `0` when no device has opened yet, the probe is re-entered, or
    /// another channel is installed.
    ///
    /// **This is the decisive observation point of the present arm, and the first
    /// proof the real canvas-context path works end to end**: a stub that skips the
    /// configure/acquire/render leaves a black/zero canvas, so reading back
    /// [`PROBE_PRESENT_COLOR_BYTES`](super::PROBE_PRESENT_COLOR_BYTES) can only come
    /// from a `configure` + `getCurrentTexture` + render + copy that actually ran.
    ///
    /// **And the swapchain it asks for is sRGB**, so those bytes are also the only
    /// proof anywhere that a canvas frame is *encoded*. A canvas cannot be
    /// configured `-srgb`, so the page has to configure the base format and reach
    /// the encode through `viewFormats`; skip either half and the readback is
    /// [`PROBE_PRESENT_UNENCODED_BYTES`](super::PROBE_PRESENT_UNENCODED_BYTES) —
    /// the whole frame a transfer function too dark, which is what shipped.
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

    /// Ask the page to reconfigure a swapchain and present a frame in the new
    /// format, and start reading it back on the device it opened.
    ///
    /// `1` when the setup frame — the surface, a swapchain configured `Rgba8Unorm`,
    /// that swapchain *reconfigured* `Bgra8Unorm`, the acquired frame, the host
    /// buffer, an encoder, a pass that clears the acquired view red, the copy,
    /// finish, submit, present and `request_readback` — is on the stream; `0` when
    /// no device has opened yet, the probe is re-entered, or another channel is
    /// installed.
    ///
    /// **This is the decisive observation point of the reconfigure arm.** A stub
    /// that skipped the reconfigure leaves the swapchain `Rgba8Unorm`, and reading
    /// red back as `[255, 0, 0, 255]` rather than
    /// [`PROBE_RECONFIG_COLOR_BYTES`](super::PROBE_RECONFIG_COLOR_BYTES) is how that
    /// shows: only a `configure` re-run with `Bgra8Unorm` hands back a frame whose
    /// red is the BGRA `[0, 0, 255, 255]`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_reconfigure(canvas_id: u32) -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_reconfigure(canvas_id)),
            Err(_) => 0,
        })
    }

    /// Poll the reconfigure probe's in-flight readback, once, on the reply channel.
    ///
    /// `1` when a [`poll_readback`](crate::StreamWriter::poll_readback) is on the
    /// stream with its wait registered; `0` when there is nothing to poll for. A
    /// no-op until the previous poll is answered, so the gate can call it blindly.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_reconfigure_poll() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.poll_reconfigure()),
            Err(_) => 0,
        })
    }

    /// Drain the replies and report where the reconfigure probe's readback has got
    /// to — one of the `RECONFIG_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_reconfigure_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.reconfigure_state(),
            Err(_) => super::RECONFIG_UNASKED,
        })
    }

    /// A pointer into wasm memory to the bytes the reconfigure probe's readback
    /// came back with.
    ///
    /// Read [`__crcbl_web_gpu_probe_reconfigure_bytes_len`] bytes from here, and
    /// only once [`__crcbl_web_gpu_probe_reconfigure_state`] has answered
    /// [`RECONFIG_READY`](super::RECONFIG_READY). Nothing here grows wasm memory,
    /// so the pointer is stable until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_reconfigure_bytes_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.reconfigure_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_reconfigure_bytes_ptr`] points at —
    /// the reconfigure probe's readback length, or `0` if it has not answered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_reconfigure_bytes_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.reconfigure_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Ask the page to render one frame with an INDIRECT draw and start reading it
    /// back on the device it opened.
    ///
    /// `1` when the setup frame — the pipeline's three resources, image, view,
    /// host buffer, indirect-args and index buffers, the two `write_buffer` fills,
    /// an encoder, a render pass that clears then binds the pipeline and index
    /// buffer and records a `drawIndexedIndirect`, the copy, finish, submit and
    /// `request_readback` — is on the stream; `0` when no device has opened yet,
    /// the probe is re-entered, or another channel is installed.
    ///
    /// **This is the decisive observation point of the indirect-draw arms**: the
    /// draw probe proves a direct `draw` overwrites a clear, and this proves an
    /// indirect `drawIndexedIndirect` reading its counts from a buffer puts exactly
    /// the same pixels there — the fragment's colour, not the clear's, and a stub
    /// that skips the draw cannot forge them.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_indirect() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_indirect()),
            Err(_) => 0,
        })
    }

    /// Poll the indirect draw's in-flight readback, once, on the reply channel.
    ///
    /// `1` when a [`poll_readback`](crate::StreamWriter::poll_readback) is on the
    /// stream with its wait registered; `0` when there is nothing to poll for — no
    /// indirect draw requested, a poll already unanswered, or the bytes already in
    /// — or when the channel would not take it. A no-op until the previous poll is
    /// answered, so the gate can call it blindly each frame.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_indirect_poll() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.poll_indirect()),
            Err(_) => 0,
        })
    }

    /// Drain the replies and report where the indirect-draw readback has got to —
    /// one of the `INDIRECT_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_indirect_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.indirect_state(),
            Err(_) => super::INDIRECT_UNASKED,
        })
    }

    /// A pointer into wasm memory to the bytes the indirect-draw readback came back
    /// with.
    ///
    /// Read [`__crcbl_web_gpu_probe_indirect_bytes_len`] bytes from here, and only
    /// once [`__crcbl_web_gpu_probe_indirect_state`] has answered
    /// [`INDIRECT_READY`](super::INDIRECT_READY): before that the length is `0` and
    /// this points at an empty buffer. Nothing here grows wasm memory, so the
    /// pointer is stable until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_indirect_bytes_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.indirect_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_indirect_bytes_ptr`] points at — the
    /// indirect-draw readback's length, or `0` if it has not answered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_indirect_bytes_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.indirect_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Encode the depth setup frame — a `depth32float` atlas, a pass that clears
    /// it, the depth-plane copy, and the readback request.
    ///
    /// `1` when the frame is on the stream, `0` if no device has opened, the
    /// probe is re-entered, or another channel is installed.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_depth() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_depth()),
            Err(_) => 0,
        })
    }

    /// Poll the depth readback once. `1` when a poll is on the stream, `0` when
    /// there is nothing to poll for.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_depth_poll() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.poll_depth()),
            Err(_) => 0,
        })
    }

    /// Drain, and answer one of the `DEPTH_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_depth_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.depth_state(),
            Err(_) => super::DEPTH_UNASKED,
        })
    }

    /// Where the depth plane's bytes start, once
    /// [`__crcbl_web_gpu_probe_depth_state`] answers [`super::DEPTH_READY`]. Null
    /// while it has not; the pointer is stable until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_depth_bytes_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.depth_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_depth_bytes_ptr`] points at — the
    /// depth readback's length, or `0` if it has not answered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_depth_bytes_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.depth_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Encode the stencil setup frame — two targets, the masked pipeline, a pass
    /// that draws twice with a different stencil reference before each, the copy
    /// and the readback request.
    ///
    /// `1` when the frame is on the stream, `0` if no device has opened, the
    /// probe is re-entered, or another channel is installed.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_stencil() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_stencil()),
            Err(_) => 0,
        })
    }

    /// Poll the stencil readback once. `1` when a poll is on the stream, `0` when
    /// there is nothing to poll for.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_stencil_poll() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.poll_stencil()),
            Err(_) => 0,
        })
    }

    /// Drain, and answer one of the `STENCIL_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_stencil_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.stencil_state(),
            Err(_) => super::STENCIL_UNASKED,
        })
    }

    /// Where the stencil probe's drawn pixels start, once
    /// [`__crcbl_web_gpu_probe_stencil_state`] answers
    /// [`super::STENCIL_READY`]. Null while it has not; the pointer is stable
    /// until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_stencil_bytes_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.stencil_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_stencil_bytes_ptr`] points at — the
    /// stencil readback's length, or `0` if it has not answered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_stencil_bytes_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.stencil_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// What the opened device reported as its
    /// [`max_sample_count`](crcbl_hal::Limits::max_sample_count), or `0` if no
    /// device has opened.
    ///
    /// **Read before `__crcbl_web_gpu_probe_msaa`, and the reason the MSAA probe
    /// has a number of its own**: it is what says whether a
    /// [`super::MSAA_UNSUPPORTED`] is a device that cannot serve
    /// [`super::PROBE_MSAA_SAMPLES`] or a request that never happened.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_msaa_samples() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.msaa_samples(),
            Err(_) => 0,
        })
    }

    /// Encode the MSAA setup frame — the two targets, the prime, a pass that
    /// clears the multisampled one and resolves into the single-sampled one, the
    /// copy and the readback request.
    ///
    /// `1` when the frame is on the stream, `0` if no device has opened, the
    /// device reported a sample count below [`super::PROBE_MSAA_SAMPLES`], the
    /// probe is re-entered, or another channel is installed.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_msaa() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_msaa()),
            Err(_) => 0,
        })
    }

    /// Poll the MSAA readback once. `1` when a poll is on the stream, `0` when
    /// there is nothing to poll for.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_msaa_poll() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.poll_msaa()),
            Err(_) => 0,
        })
    }

    /// Drain, and answer one of the `MSAA_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_msaa_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.msaa_state(),
            Err(_) => super::MSAA_UNASKED,
        })
    }

    /// Where the resolve target's texels start, once
    /// [`__crcbl_web_gpu_probe_msaa_state`] answers [`super::MSAA_READY`]. Null
    /// while it has not; the pointer is stable until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_msaa_bytes_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.msaa_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_msaa_bytes_ptr`] points at — the
    /// MSAA readback's length, or `0` if it has not answered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_msaa_bytes_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.msaa_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Encode the occlusion setup frame — an occlusion query set, a resolve
    /// destination primed with a sentinel, the reset and the resolve, the copy,
    /// the readback request, and the direct-read ask beside it.
    ///
    /// `1` when the frame is on the stream, `0` if no device has opened, the
    /// probe is re-entered, or another channel is installed.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_occlusion() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_occlusion()),
            Err(_) => 0,
        })
    }

    /// Poll the occlusion readback once. `1` when a poll is on the stream, `0`
    /// when there is nothing to poll for. The direct read is never polled — the
    /// replayer answers it when its own map settles.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_occlusion_poll() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.poll_occlusion()),
            Err(_) => 0,
        })
    }

    /// Drain, and answer one of the `OCCLUSION_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_occlusion_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.occlusion_state(),
            Err(_) => super::OCCLUSION_UNASKED,
        })
    }

    /// Where the resolved values start, once
    /// [`__crcbl_web_gpu_probe_occlusion_state`] answers
    /// [`super::OCCLUSION_READY`]. Null while it has not; the pointer is stable
    /// until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_occlusion_bytes_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.occlusion_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_occlusion_bytes_ptr`] points at —
    /// the occlusion readback's length, or `0` if it has not answered.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_occlusion_bytes_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.occlusion_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Drain, and answer one of the `OCCLUSION_VALUES_*` codes — where the
    /// **direct read** has got to.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_occlusion_values_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.occlusion_values_state(),
            Err(_) => super::OCCLUSION_VALUES_UNASKED,
        })
    }

    /// Where the direct read's values start, once
    /// [`__crcbl_web_gpu_probe_occlusion_values_state`] answers
    /// [`super::OCCLUSION_VALUES_READY`]. One little-endian `u64` per query.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_occlusion_values_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.occlusion_values_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_occlusion_values_ptr`] points at.
    ///
    /// **Zero is a failure rather than an empty success**: the seam never asks
    /// for no values, so an empty
    /// [`Reply::QueryResults`](crate::Reply::QueryResults) is the only way that
    /// reply can say the read could not be served.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_occlusion_values_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.occlusion_values_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Whether the opened device has the browser's `timestamp-query` feature,
    /// as `1` or `0`; `0` if no device has opened.
    ///
    /// **Read before `__crcbl_web_gpu_probe_timestamp`, and the reason the
    /// timestamp probe has a flag of its own** —
    /// [`__crcbl_web_gpu_probe_msaa_samples`]'s job: it is what says whether a
    /// [`super::TIMESTAMP_UNSUPPORTED`] is a device that cannot serve a
    /// timestamp set or a request that never happened.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_timestamp_supported() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.timestamp_supported()),
            Err(_) => 0,
        })
    }

    /// Encode the timed-pass frame — the two-query timestamp set, the reset, a
    /// compute pass naming both queries in its descriptor, the submit, and the
    /// read of the two queries.
    ///
    /// `1` when the frame is on the stream, `0` if no device has opened, the
    /// device has no `timestamp-query`, the probe is re-entered, or another
    /// channel is installed.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_timestamp() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => u32::from(probe.request_timestamp()),
            Err(_) => 0,
        })
    }

    /// Drain, and answer one of the `TIMESTAMP_*` codes.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_timestamp_state() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.timestamp_state(),
            Err(_) => super::TIMESTAMP_UNASKED,
        })
    }

    /// Where the two ticks start, once
    /// [`__crcbl_web_gpu_probe_timestamp_state`] answers
    /// [`super::TIMESTAMP_READY`]. Null while it has not; the pointer is stable
    /// until the next drain.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_timestamp_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.timestamp_bytes().as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How many bytes [`__crcbl_web_gpu_probe_timestamp_ptr`] points at —
    /// sixteen once the read is answered, and **zero when the replayer could not
    /// serve it**, which is the only way [`Reply::QueryResults`](crate::Reply)
    /// says so.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_timestamp_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.timestamp_bytes().len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Run the parity report against the device the browser opened, and answer
    /// one of the `PARITY_*` codes.
    ///
    /// **THE ONE EXPORT HERE THAT ASKS THE BROWSER NOTHING.** Every other probe
    /// puts a command on the stream and waits; this one builds a
    /// [`WebGpuDevice`](crate::hal::WebGpuDevice) around the caps that already
    /// came back, walks its whole
    /// [`supports`](crcbl_hal::Device::supports) matrix and compares it with
    /// [`DIVERGENCES`](crcbl_hal::DIVERGENCES) — so it is one call with no `poll`
    /// and no `state` beside it, the way `__crcbl_web_gpu_probe_surface` is one
    /// call for its own reason. It
    /// neither drains nor encodes, so calling it disturbs no probe in flight.
    ///
    /// [`super::PARITY_NO_DEVICE`] when nothing has opened yet, which is ordering
    /// rather than failure — wait for [`__crcbl_web_gpu_probe_device_state`] to
    /// answer [`super::DEVICE_OPENED`]. **Allocates**, so any view onto wasm
    /// memory is built after it rather than before.
    ///
    /// Calling it twice reports twice: the report is rebuilt from the device's
    /// current caps each time and replaces the last one.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_parity() -> u32 {
        PROBE.with(|probe| match probe.try_borrow_mut() {
            Ok(mut probe) => probe.run_parity(),
            Err(_) => super::PARITY_UNASKED,
        })
    }

    /// How many capabilities the last [`__crcbl_web_gpu_probe_parity`] walked —
    /// [`Capability::ALL`](crcbl_hal::Capability::ALL)'s length once it has run,
    /// and `0` when it has not.
    ///
    /// **The vacuity guard, and the reason it is exported at all**: a report that
    /// walked nothing agrees with every list there is, so the gate asserts this
    /// against the matrix it received rather than trusting a green verdict.
    /// Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_parity_checked() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.parity.checked,
            Err(_) => 0,
        })
    }

    /// How many of those were **settled** rather than left unprovable by a
    /// device that withheld the capability's gating feature. Allocates nothing.
    ///
    /// Lower than [`__crcbl_web_gpu_probe_parity_checked`] here and equal to it
    /// in the native suite, because the device this probe opens asks for nothing
    /// optional — the [module docs](super#the-device-this-asks-for-and-why-it-asks-for-so-little)
    /// say why it asks for so little.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_parity_held() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.parity.held,
            Err(_) => 0,
        })
    }

    /// Where the matrix belonging to the last [`__crcbl_web_gpu_probe_parity`]
    /// starts — one `Capability=verdict` token per capability, space separated.
    /// Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_parity_report_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.parity.report.as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How long that matrix is, in UTF-8 bytes. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_parity_report_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.parity.report.len()).unwrap_or(u32::MAX),
            Err(_) => 0,
        })
    }

    /// Where the disagreements start — one per line, empty when the last
    /// [`__crcbl_web_gpu_probe_parity`] answered
    /// [`super::PARITY_MATCHED`]. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_parity_failures_ptr() -> *const u8 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => probe.parity.failures.as_ptr(),
            Err(_) => core::ptr::null(),
        })
    }

    /// How long that text is, in UTF-8 bytes. Allocates nothing.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_gpu_probe_parity_failures_len() -> u32 {
        PROBE.with(|probe| match probe.try_borrow() {
            Ok(probe) => u32::try_from(probe.parity.failures.len()).unwrap_or(u32::MAX),
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
        __crcbl_web_gpu_probe_depth, __crcbl_web_gpu_probe_depth_bytes_len,
        __crcbl_web_gpu_probe_depth_bytes_ptr, __crcbl_web_gpu_probe_depth_poll,
        __crcbl_web_gpu_probe_depth_state, __crcbl_web_gpu_probe_device,
        __crcbl_web_gpu_probe_device_features_hi, __crcbl_web_gpu_probe_device_features_lo,
        __crcbl_web_gpu_probe_device_max_image_2d, __crcbl_web_gpu_probe_device_reason_len,
        __crcbl_web_gpu_probe_device_reason_ptr, __crcbl_web_gpu_probe_device_state,
        __crcbl_web_gpu_probe_draw, __crcbl_web_gpu_probe_draw_bytes_len,
        __crcbl_web_gpu_probe_draw_bytes_ptr, __crcbl_web_gpu_probe_draw_poll,
        __crcbl_web_gpu_probe_draw_state, __crcbl_web_gpu_probe_features_hi,
        __crcbl_web_gpu_probe_features_lo, __crcbl_web_gpu_probe_fill,
        __crcbl_web_gpu_probe_fill_bytes_len, __crcbl_web_gpu_probe_fill_bytes_ptr,
        __crcbl_web_gpu_probe_fill_poll, __crcbl_web_gpu_probe_fill_state,
        __crcbl_web_gpu_probe_graphics_pipeline, __crcbl_web_gpu_probe_image,
        __crcbl_web_gpu_probe_image_view, __crcbl_web_gpu_probe_indirect,
        __crcbl_web_gpu_probe_indirect_bytes_len, __crcbl_web_gpu_probe_indirect_bytes_ptr,
        __crcbl_web_gpu_probe_indirect_poll, __crcbl_web_gpu_probe_indirect_state,
        __crcbl_web_gpu_probe_max_image_2d, __crcbl_web_gpu_probe_msaa,
        __crcbl_web_gpu_probe_msaa_bytes_len, __crcbl_web_gpu_probe_msaa_bytes_ptr,
        __crcbl_web_gpu_probe_msaa_poll, __crcbl_web_gpu_probe_msaa_samples,
        __crcbl_web_gpu_probe_msaa_state, __crcbl_web_gpu_probe_occlusion,
        __crcbl_web_gpu_probe_occlusion_bytes_len, __crcbl_web_gpu_probe_occlusion_bytes_ptr,
        __crcbl_web_gpu_probe_occlusion_poll, __crcbl_web_gpu_probe_occlusion_state,
        __crcbl_web_gpu_probe_occlusion_values_len, __crcbl_web_gpu_probe_occlusion_values_ptr,
        __crcbl_web_gpu_probe_occlusion_values_state, __crcbl_web_gpu_probe_parity,
        __crcbl_web_gpu_probe_parity_checked, __crcbl_web_gpu_probe_parity_failures_len,
        __crcbl_web_gpu_probe_parity_failures_ptr, __crcbl_web_gpu_probe_parity_held,
        __crcbl_web_gpu_probe_parity_report_len, __crcbl_web_gpu_probe_parity_report_ptr,
        __crcbl_web_gpu_probe_pipeline_layout, __crcbl_web_gpu_probe_present,
        __crcbl_web_gpu_probe_present_bytes_len, __crcbl_web_gpu_probe_present_bytes_ptr,
        __crcbl_web_gpu_probe_present_poll, __crcbl_web_gpu_probe_present_state,
        __crcbl_web_gpu_probe_reconfigure, __crcbl_web_gpu_probe_reconfigure_bytes_len,
        __crcbl_web_gpu_probe_reconfigure_bytes_ptr, __crcbl_web_gpu_probe_reconfigure_poll,
        __crcbl_web_gpu_probe_reconfigure_state, __crcbl_web_gpu_probe_sampler,
        __crcbl_web_gpu_probe_shader_module, __crcbl_web_gpu_probe_state,
        __crcbl_web_gpu_probe_stencil, __crcbl_web_gpu_probe_stencil_bytes_len,
        __crcbl_web_gpu_probe_stencil_bytes_ptr, __crcbl_web_gpu_probe_stencil_poll,
        __crcbl_web_gpu_probe_stencil_state, __crcbl_web_gpu_probe_surface,
        __crcbl_web_gpu_probe_surface_caps, __crcbl_web_gpu_probe_surface_caps_cause,
        __crcbl_web_gpu_probe_surface_caps_format, __crcbl_web_gpu_probe_surface_caps_has_extent,
        __crcbl_web_gpu_probe_surface_caps_present_modes,
        __crcbl_web_gpu_probe_surface_caps_reason_len,
        __crcbl_web_gpu_probe_surface_caps_reason_ptr, __crcbl_web_gpu_probe_surface_caps_state,
        __crcbl_web_gpu_probe_text_len, __crcbl_web_gpu_probe_text_ptr,
        __crcbl_web_gpu_probe_timestamp, __crcbl_web_gpu_probe_timestamp_len,
        __crcbl_web_gpu_probe_timestamp_ptr, __crcbl_web_gpu_probe_timestamp_state,
        __crcbl_web_gpu_probe_timestamp_supported,
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
                optional_features: Features::TIMESTAMP_QUERY,
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

    /// The indirect probe's bytes, read the way JS reads them —
    /// [`draw_bytes`]'s indirect sibling.
    fn indirect_bytes() -> Vec<u8> {
        let len = __crcbl_web_gpu_probe_indirect_bytes_len() as usize;
        let ptr = __crcbl_web_gpu_probe_indirect_bytes_ptr();
        if len == 0 {
            return Vec::new();
        }
        assert!(
            !ptr.is_null(),
            "the indirect probe answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::indirect` bytes, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        bytes.to_vec()
    }

    /// **Every indirect handle is a generation past every other probe's** — the
    /// point of `7 << 32`: the indirect frame has an image, a view, three buffers,
    /// a shader module, a pipeline layout, a pipeline, a command buffer, a queue
    /// and a readback live at once, and none may land in another probe's slot in
    /// the shared page. The three buffers share the generation but not the index.
    #[test]
    fn the_indirect_handles_are_a_generation_past_every_other_probe() {
        for bits in [
            PROBE_INDIRECT_IMAGE.to_bits(),
            PROBE_INDIRECT_IMAGE_VIEW.to_bits(),
            PROBE_INDIRECT_BUFFER.to_bits(),
            PROBE_INDIRECT_ARGS_BUFFER.to_bits(),
            PROBE_INDIRECT_INDEX_BUFFER.to_bits(),
            PROBE_INDIRECT_SHADER_MODULE.to_bits(),
            PROBE_INDIRECT_PIPELINE_LAYOUT.to_bits(),
            PROBE_INDIRECT_PIPELINE.to_bits(),
            PROBE_INDIRECT_COMMAND_BUFFER.to_bits(),
            PROBE_INDIRECT_QUEUE.to_bits(),
            PROBE_INDIRECT_READBACK.to_bits(),
        ] {
            assert_eq!(bits >> 32, 7, "every indirect handle is generation seven");
        }
        // The three buffers are distinct by index within the shared generation.
        assert_ne!(
            PROBE_INDIRECT_BUFFER.to_bits(),
            PROBE_INDIRECT_ARGS_BUFFER.to_bits()
        );
        assert_ne!(
            PROBE_INDIRECT_ARGS_BUFFER.to_bits(),
            PROBE_INDIRECT_INDEX_BUFFER.to_bits()
        );
        // And a generation clear of the draw probe it mirrors.
        assert_ne!(PROBE_INDIRECT_IMAGE.to_bits(), PROBE_DRAW_IMAGE.to_bits());
        assert_ne!(
            PROBE_INDIRECT_READBACK.to_bits(),
            PROBE_DRAW_READBACK.to_bits()
        );
    }

    /// The indirect half: **one export, a whole frame** that fills the args and
    /// index buffers with `WriteBuffer`, then clears, binds the pipeline and index
    /// buffer, and records a `DrawIndexedIndirect` — the draw probe's frame with
    /// the draw made indirect. The two writes land before the encoder so
    /// `queue.writeBuffer` is ordered ahead of the submit that reads them.
    #[test]
    fn the_indirect_export_encodes_the_writes_the_bind_and_the_indirect_draw() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_indirect(), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateImage",
                "CreateImageView",
                "CreateBuffer",
                "CreateBuffer",
                "CreateBuffer",
                "CreateShaderModule",
                "CreatePipelineLayout",
                "CreateGraphicsPipeline",
                "WriteBuffer",
                "WriteBuffer",
                "CreateCommandEncoder",
                "BeginRenderPass",
                "BindGraphicsPipeline",
                "BindIndexBuffer",
                "DrawIndexedIndirect",
                "EndRenderPass",
                "CopyImageToBuffer",
                "Finish",
                "Submit",
                "RequestReadback",
            ],
            "the frame builds the pipeline, fills the buffers, binds and draws indirect, then reads back"
        );
        // The args write carries the 3-index single-draw command, and the indirect
        // draw reads it at offset 0 with a CPU-known count of one.
        assert!(commands.contains(&Command::WriteBuffer {
            buffer: PROBE_INDIRECT_ARGS_BUFFER,
            offset: 0,
            data: PROBE_INDIRECT_ARGS_BYTES.to_vec(),
        }));
        assert!(commands.contains(&Command::DrawIndexedIndirect {
            buffer: PROBE_INDIRECT_ARGS_BUFFER,
            offset: 0,
            draw_count: 1,
            stride: 0,
        }));
    }

    /// **A device has to have opened first**, the draw probe's ordering rule:
    /// every command the frame carries is a device method.
    #[test]
    fn an_indirect_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_indirect(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);
        assert_eq!(__crcbl_web_gpu_probe_indirect_state(), INDIRECT_UNASKED);
    }

    /// The whole indirect exchange through the exports alone: request, poll, and a
    /// `ReadbackReady` carrying the drawn pixels — which reach the bytes exports as
    /// the draw colour, proving the indirect draw put exactly what a direct draw
    /// would. The browser gate's path with the replayer replaced by a
    /// `ReplyWriter`, as a `cargo test` has no `navigator.gpu`.
    #[test]
    fn the_indirect_readback_reaches_the_bytes_exports_as_the_drawn_colour() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_indirect(), 1);
        let setup = take_frame();
        let poll_sequence = 2 + setup.len() as u64;
        assert_eq!(__crcbl_web_gpu_probe_indirect_state(), INDIRECT_REQUESTED);

        assert_eq!(__crcbl_web_gpu_probe_indirect_poll(), 1);
        assert_eq!(__crcbl_web_gpu_probe_indirect_state(), INDIRECT_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::PollReadback {
                readback: PROBE_INDIRECT_READBACK,
            }]
        );

        let mut drawn = Vec::new();
        for _ in 0..(PROBE_READBACK_SIZE * PROBE_READBACK_SIZE) {
            drawn.extend_from_slice(&PROBE_INDIRECT_COLOR_BYTES);
        }
        let mut replies = ReplyWriter::new();
        replies.readback_ready(poll_sequence, PROBE_INDIRECT_READBACK, &drawn);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_indirect_state(), INDIRECT_READY);
        assert_eq!(indirect_bytes(), drawn);
        assert_eq!(&indirect_bytes()[..4], PROBE_INDIRECT_COLOR_BYTES);
        // The draw colour is not the clear the pass loaded with — the whole
        // evidence the gate reads back from the browser.
        assert_ne!(PROBE_INDIRECT_COLOR_BYTES, PROBE_READBACK_CLEAR_BYTES);
    }

    /// A `ReadbackPending` for the poll's sequence drops the indirect probe back
    /// to `Pending`, so the next frame polls again — [`IndirectProbe::absorb`]'s
    /// pending arm.
    #[test]
    fn a_readback_pending_reply_drops_the_indirect_probe_back_to_pending() {
        let mut indirect = IndirectProbe::Waiting { sequence: 7 };
        let advanced = indirect.absorb(&[(
            7,
            Reply::ReadbackPending {
                readback: PROBE_INDIRECT_READBACK,
            },
        )]);
        assert!(advanced);
        assert_eq!(indirect, IndirectProbe::Pending);
    }

    /// A `ReadbackReady` for the poll's sequence carries the bytes into `Ready`.
    #[test]
    fn a_readback_ready_reply_carries_the_indirect_bytes_into_ready() {
        let mut indirect = IndirectProbe::Waiting { sequence: 7 };
        let bytes = vec![255, 0, 0, 255];
        let advanced = indirect.absorb(&[(
            7,
            Reply::ReadbackReady {
                readback: PROBE_INDIRECT_READBACK,
                data: bytes.clone(),
            },
        )]);
        assert!(advanced);
        assert_eq!(indirect, IndirectProbe::Ready { bytes });
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

    /// **The present probe asks for an sRGB swapchain and expects the encoded
    /// bytes**, which is the whole of what lets group X see a canvas presenting
    /// unencoded frames.
    ///
    /// Both halves are load-bearing and both are checked against the transfer
    /// function rather than against themselves: a linear swapchain reads the clear
    /// colour straight back, and expected bytes that drifted onto the unencoded
    /// ones would pass on exactly the build that shipped a whole transfer function
    /// too dark. The third assertion is the one that keeps the *colour* honest —
    /// `0.0` and `1.0` are fixed points of the encode, so a probe clearing to red
    /// cannot tell the two targets apart in any channel.
    #[test]
    fn the_present_probe_asks_for_an_srgb_swapchain_and_expects_encoded_bytes() {
        assert!(
            probe_present_swapchain_desc().format.is_srgb(),
            "a linear swapchain reads the clear colour back unchanged and proves nothing",
        );
        for channel in 0..3 {
            let linear = PROBE_PRESENT_COLOR[channel];
            // The sRGB transfer function, above its linear toe — every component
            // here is well past `0.0031308`.
            let encoded = 1.055 * linear.powf(1.0 / 2.4) - 0.055;
            assert_eq!(
                PROBE_PRESENT_COLOR_BYTES[channel],
                byte_of(encoded),
                "channel {channel} of PROBE_PRESENT_COLOR_BYTES is not the sRGB encode of {linear}",
            );
            assert_eq!(
                PROBE_PRESENT_UNENCODED_BYTES[channel],
                byte_of(linear),
                "channel {channel} of PROBE_PRESENT_UNENCODED_BYTES is not {linear} unencoded",
            );
            let apart =
                PROBE_PRESENT_COLOR_BYTES[channel].abs_diff(PROBE_PRESENT_UNENCODED_BYTES[channel]);
            assert!(
                apart > PROBE_PRESENT_COLOR_TOLERANCE * 2,
                "channel {channel} moves only {apart} levels under the encode, which the \
                 gate's tolerance of {PROBE_PRESENT_COLOR_TOLERANCE} cannot separate",
            );
        }
        // Alpha is never encoded, so it is the one component the two agree on.
        assert_eq!(
            PROBE_PRESENT_COLOR_BYTES[3],
            PROBE_PRESENT_UNENCODED_BYTES[3]
        );
    }

    /// A unorm component as the byte it is stored in, for the check above.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the argument is clamped into 0..=255 on the line before the cast"
    )]
    fn byte_of(value: f32) -> u8 {
        (value * 255.0).round().clamp(0.0, 255.0) as u8
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

    /// The reconfigure probe's bytes, read the way JS reads them.
    fn reconfigure_bytes() -> Vec<u8> {
        let len = __crcbl_web_gpu_probe_reconfigure_bytes_len() as usize;
        let ptr = __crcbl_web_gpu_probe_reconfigure_bytes_ptr();
        if len == 0 {
            return Vec::new();
        }
        assert!(
            !ptr.is_null(),
            "the reconfigure answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::reconfig` bytes, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        bytes.to_vec()
    }

    /// **Every reconfigure handle is a generation past the present probe's** — the
    /// whole point of `6 << 32`: the two probes can both run in the shared page
    /// without one's live resource landing in the other's slot.
    #[test]
    fn the_reconfigure_handles_are_a_generation_past_the_present_probe() {
        for bits in [
            PROBE_RECONFIG_SURFACE.to_bits(),
            PROBE_RECONFIG_SWAPCHAIN.to_bits(),
            PROBE_RECONFIG_IMAGE.to_bits(),
            PROBE_RECONFIG_VIEW.to_bits(),
            PROBE_RECONFIG_BUFFER.to_bits(),
            PROBE_RECONFIG_COMMAND_BUFFER.to_bits(),
            PROBE_RECONFIG_QUEUE.to_bits(),
            PROBE_RECONFIG_READBACK.to_bits(),
        ] {
            assert_eq!(bits >> 32, 6, "every reconfigure handle is generation six");
        }
        assert_ne!(
            PROBE_RECONFIG_IMAGE.to_bits(),
            PROBE_PRESENT_IMAGE.to_bits()
        );
        assert_ne!(
            PROBE_RECONFIG_READBACK.to_bits(),
            PROBE_PRESENT_READBACK.to_bits()
        );
    }

    /// The reconfigure half: **one export, a whole frame** that creates a surface,
    /// configures a swapchain `Rgba8Unorm`, RECONFIGURES it `Bgra8Unorm`, acquires
    /// the frame, clears it, copies it out, submits, presents and reads back. The
    /// reconfigure is the command the present frame does not have.
    #[test]
    fn the_reconfigure_export_encodes_the_create_reconfigure_acquire_and_present() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_reconfigure(7), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateSurface",
                "CreateSwapchain",
                "ReconfigureSwapchain",
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
            "the frame creates, reconfigures, acquires, clears, copies, submits, presents and reads back"
        );
        // The swapchain is created `Rgba8Unorm` and then reconfigured `Bgra8Unorm`
        // — the format change that is the whole observable — on the same handle.
        assert!(commands.contains(&Command::CreateSwapchain {
            swapchain: PROBE_RECONFIG_SWAPCHAIN,
            label: Some("crcbl-webgpu reconfigure swapchain".into()),
            surface: PROBE_RECONFIG_SURFACE,
            format: Format::Rgba8Unorm,
            extent: (PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
            image_count: 2,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        }));
        assert!(commands.contains(&Command::ReconfigureSwapchain {
            swapchain: PROBE_RECONFIG_SWAPCHAIN,
            label: Some("crcbl-webgpu reconfigure swapchain".into()),
            surface: PROBE_RECONFIG_SURFACE,
            format: Format::Bgra8Unorm,
            extent: (PROBE_READBACK_SIZE, PROBE_READBACK_SIZE),
            image_count: 2,
            present_mode: PresentMode::Fifo,
            composite_alpha: CompositeAlpha::Opaque,
        }));
    }

    /// **A device has to have opened first**, the present probe's ordering rule:
    /// every command after the surface is a device method.
    #[test]
    fn a_reconfigure_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_reconfigure(7), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);
        assert_eq!(__crcbl_web_gpu_probe_reconfigure_state(), RECONFIG_UNASKED);

        grant(&granted("no device yet"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_WAITING);
        assert_eq!(__crcbl_web_gpu_probe_reconfigure(7), 0);
        assert_eq!(take_frame().len(), 1);
    }

    /// The whole reconfigure exchange through the exports alone: request, poll, and
    /// a `ReadbackReady` carrying the reconfigured pixels — which reach the bytes
    /// exports as the BGRA present colour, `[0, 0, 255, 255]`, not RGBA's
    /// `[255, 0, 0, 255]`. That byte order is the proof the reconfigure ran.
    #[test]
    fn the_reconfigure_readback_reaches_the_bytes_exports_as_the_bgra_present_colour() {
        // `open_device` spends sequences 0 and 1, so the setup frame starts at 2
        // and its poll is the command after the frame's own — read the length off
        // the frame rather than hard-wiring it.
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_reconfigure(7), 1);
        let setup = take_frame();
        let poll_sequence = 2 + setup.len() as u64;
        assert_eq!(
            __crcbl_web_gpu_probe_reconfigure_state(),
            RECONFIG_REQUESTED
        );

        assert_eq!(__crcbl_web_gpu_probe_reconfigure_poll(), 1);
        assert_eq!(__crcbl_web_gpu_probe_reconfigure_state(), RECONFIG_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::PollReadback {
                readback: PROBE_RECONFIG_READBACK,
            }]
        );

        let mut reconfigured = Vec::new();
        for _ in 0..(PROBE_READBACK_SIZE * PROBE_READBACK_SIZE) {
            reconfigured.extend_from_slice(&PROBE_RECONFIG_COLOR_BYTES);
        }
        let mut replies = ReplyWriter::new();
        replies.readback_ready(poll_sequence, PROBE_RECONFIG_READBACK, &reconfigured);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_reconfigure_state(), RECONFIG_READY);
        assert_eq!(reconfigure_bytes(), reconfigured);
        assert_eq!(&reconfigure_bytes()[..4], PROBE_RECONFIG_COLOR_BYTES);
        // And it is the BGRA red, not RGBA's — a stub that skipped the reconfigure
        // would have left the swapchain `Rgba8Unorm` and answered this instead.
        // Spelled out rather than named: this probe's colour has one RGBA
        // spelling, and it is not the present probe's, which is a mid-tone.
        assert_ne!(&reconfigure_bytes()[..4], [255, 0, 0, 255]);
    }

    /// A `ReadbackPending` for the poll's sequence drops the reconfigure back to
    /// `Pending`, so the next frame polls again — [`ReconfigProbe::absorb`]'s
    /// pending arm, tested at the enum because the sequence is known there.
    #[test]
    fn a_readback_pending_reply_drops_the_reconfigure_back_to_pending() {
        let mut reconfig = ReconfigProbe::Waiting { sequence: 7 };
        let advanced = reconfig.absorb(&[(
            7,
            Reply::ReadbackPending {
                readback: PROBE_RECONFIG_READBACK,
            },
        )]);
        assert!(advanced);
        assert_eq!(reconfig, ReconfigProbe::Pending);
    }

    /// A `ReadbackReady` for the poll's sequence carries the bytes into `Ready`.
    #[test]
    fn a_readback_ready_reply_carries_the_reconfigure_bytes_into_ready() {
        let mut reconfig = ReconfigProbe::Waiting { sequence: 7 };
        let bytes = vec![0, 0, 255, 255];
        let advanced = reconfig.absorb(&[(
            7,
            Reply::ReadbackReady {
                readback: PROBE_RECONFIG_READBACK,
                data: bytes.clone(),
            },
        )]);
        assert!(advanced);
        assert_eq!(reconfig, ReconfigProbe::Ready { bytes });
    }

    /// A reply for another sequence leaves the reconfigure waiting, exactly as it
    /// leaves every other probe.
    #[test]
    fn a_reconfigure_probe_ignores_a_reply_for_another_sequence() {
        let mut reconfig = ReconfigProbe::Waiting { sequence: 7 };
        let advanced = reconfig.absorb(&[(
            8,
            Reply::ReadbackReady {
                readback: PROBE_RECONFIG_READBACK,
                data: vec![1, 2, 3, 4],
            },
        )]);
        assert!(!advanced);
        assert_eq!(reconfig, ReconfigProbe::Waiting { sequence: 7 });
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
    /// the sRGB counterpart of the preferred canvas format first, then the other
    /// counterpart, then the two formats a canvas can be configured with —
    /// `preferred` leading that pair. One present mode, no extent.
    ///
    /// `preferred` is named as its **counterpart** because that is what
    /// `preferred_format` picks and therefore what the format export reports; the
    /// linear pair is what a `GPUCanvasContext.configure` may be handed.
    /// The `-srgb` counterpart of a canvas format, for [`canvas_caps`].
    fn srgb_of(format: Format) -> Format {
        match format {
            Format::Rgba8Unorm => Format::Rgba8UnormSrgb,
            Format::Bgra8Unorm => Format::Bgra8UnormSrgb,
            other => panic!("{other:?} is not a canvas format"),
        }
    }

    fn canvas_caps(preferred: Format) -> crcbl_hal::SurfaceCaps {
        let other = if preferred == Format::Rgba8Unorm {
            Format::Bgra8Unorm
        } else {
            Format::Rgba8Unorm
        };
        crcbl_hal::SurfaceCaps {
            formats: vec![srgb_of(preferred), srgb_of(other), preferred, other],
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
        // `Bgra8UnormSrgb` rather than the list's first entry by accident:
        // `preferred_format` takes the first sRGB entry, and the list leads with
        // the counterpart of what this browser preferred.
        assert_eq!(
            __crcbl_web_gpu_probe_surface_caps_format(),
            u32::from(tag::format_code(Format::Bgra8UnormSrgb))
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
            u32::from(tag::format_code(Format::Rgba8UnormSrgb))
        );
        assert_ne!(
            tag::format_code(Format::Rgba8UnormSrgb),
            tag::format_code(Format::Bgra8UnormSrgb),
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
            u32::from(tag::format_code(Format::Bgra8UnormSrgb))
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

    /// The depth probe's bytes, read the way JS reads them.
    fn depth_bytes() -> Vec<u8> {
        let len = __crcbl_web_gpu_probe_depth_bytes_len() as usize;
        let ptr = __crcbl_web_gpu_probe_depth_bytes_ptr();
        if len == 0 {
            return Vec::new();
        }
        assert!(
            !ptr.is_null(),
            "the depth probe answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::depth` bytes, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        bytes.to_vec()
    }

    /// **Every depth handle is generation eight**, a generation past the indirect
    /// probe's `7 << 32` and every probe before it: the atlas, its view, the host
    /// buffer, the command buffer, the queue and the readback are all live at once
    /// and none may land in another probe's slot in the shared page.
    #[test]
    fn the_depth_handles_are_a_generation_past_every_other_probe() {
        for bits in [
            PROBE_DEPTH_IMAGE.to_bits(),
            PROBE_DEPTH_IMAGE_VIEW.to_bits(),
            PROBE_DEPTH_BUFFER.to_bits(),
            PROBE_DEPTH_COMMAND_BUFFER.to_bits(),
            PROBE_DEPTH_QUEUE.to_bits(),
            PROBE_DEPTH_READBACK.to_bits(),
        ] {
            assert_eq!(bits >> 32, 8, "every depth handle is generation eight");
        }
        // A generation clear of the indirect probe (`7 << 32`), the nearest
        // neighbour.
        assert_ne!(PROBE_DEPTH_IMAGE.to_bits(), PROBE_INDIRECT_IMAGE.to_bits());
    }

    /// **The copy names the depth plane and the whole subresource**, which is
    /// what WebGPU permits and nothing else: an `'all'` aspect or a partial
    /// region is refused by the replayer or by the browser, and either way the
    /// atlas comes back as nothing.
    #[test]
    fn the_depth_copy_names_the_depth_aspect_of_the_whole_subresource() {
        let copy = probe_depth_copy();
        assert_eq!(copy.image_subresource.aspect, ImageAspect::DEPTH);
        assert_eq!(copy.image_offset, Offset3d { x: 0, y: 0, z: 0 });
        assert_eq!(
            copy.image_extent,
            Extent3d::d2(PROBE_DEPTH_SIZE, PROBE_DEPTH_SIZE)
        );
        // Tightly packed, and the row that produces is already aligned to
        // WebGPU's `bytesPerRow` rule — a fixed number in the specification
        // rather than a limit anything reports, and one the seam has no padding
        // field to satisfy any other way.
        const BYTES_PER_ROW_ALIGNMENT: u32 = 256;
        assert_eq!(copy.buffer_row_length, 0);
        assert_eq!(
            PROBE_DEPTH_SIZE * 4 % BYTES_PER_ROW_ALIGNMENT,
            0,
            "a depth32float row {PROBE_DEPTH_SIZE} texels wide must already be aligned"
        );
    }

    /// The depth half: **one export, a whole frame** that clears a `depth32float`
    /// atlas and copies its depth plane out. No pipeline and no draw — the clear
    /// is the write.
    #[test]
    fn the_depth_export_encodes_the_clear_pass_and_the_plane_copy() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_depth(), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateImage",
                "CreateImageView",
                "CreateBuffer",
                "CreateCommandEncoder",
                "BeginRenderPass",
                "EndRenderPass",
                "CopyImageToBuffer",
                "Finish",
                "Submit",
                "RequestReadback",
            ],
            "the frame clears a depth attachment and copies its plane out"
        );
        let want = probe_depth_copy();
        assert!(
            commands.iter().any(|command| matches!(
                command,
                Command::CopyImageToBuffer {
                    buffer,
                    buffer_offset,
                    buffer_row_length,
                    buffer_image_height,
                    image,
                    image_subresource,
                    image_offset,
                    image_extent,
                } if *buffer == want.buffer
                    && *buffer_offset == want.buffer_offset
                    && *buffer_row_length == want.buffer_row_length
                    && *buffer_image_height == want.buffer_image_height
                    && *image == want.image
                    && *image_subresource == want.image_subresource
                    && *image_offset == want.image_offset
                    && *image_extent == want.image_extent
            )),
            "the frame carries the depth-plane copy field for field"
        );
        // The pass has NO colour attachment and a stored depth one. Both halves
        // matter: a colour attachment beside it would make the readback a colour
        // readback, and a discarded depth attachment leaves the plane undefined.
        let pass = commands
            .iter()
            .find_map(|command| match command {
                Command::BeginRenderPass {
                    color_attachments,
                    depth_stencil_attachment,
                    ..
                } => Some((color_attachments.clone(), *depth_stencil_attachment)),
                _ => None,
            })
            .expect("the frame begins a render pass");
        assert!(pass.0.is_empty(), "the depth pass has no colour attachment");
        assert_eq!(pass.1, Some(probe_depth_attachment()));
    }

    /// A depth request before a device opens is refused and encodes nothing.
    #[test]
    fn a_depth_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_depth(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);
        assert_eq!(__crcbl_web_gpu_probe_depth_state(), DEPTH_UNASKED);
    }

    /// The whole depth exchange through the exports alone: request, poll, and a
    /// `ReadbackReady` carrying [`PROBE_DEPTH_CLEAR`] for every texel, which
    /// reaches the bytes exports. A `cargo test` has no `navigator.gpu`, so the
    /// replayer is stood in for by a `ReplyWriter`.
    #[test]
    fn the_depth_readback_reaches_the_bytes_exports_as_the_cleared_depth() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_depth(), 1);
        let setup = take_frame();
        let poll_sequence = 2 + setup.len() as u64;
        assert_eq!(__crcbl_web_gpu_probe_depth_state(), DEPTH_REQUESTED);

        assert_eq!(__crcbl_web_gpu_probe_depth_poll(), 1);
        assert_eq!(__crcbl_web_gpu_probe_depth_state(), DEPTH_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::PollReadback {
                readback: PROBE_DEPTH_READBACK,
            }]
        );

        let texels = (PROBE_DEPTH_SIZE as usize) * (PROBE_DEPTH_SIZE as usize);
        let mut cleared = Vec::new();
        for _ in 0..texels {
            cleared.extend_from_slice(&PROBE_DEPTH_CLEAR.to_le_bytes());
        }
        let mut replies = ReplyWriter::new();
        replies.readback_ready(poll_sequence, PROBE_DEPTH_READBACK, &cleared);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_depth_state(), DEPTH_READY);
        assert_eq!(depth_bytes(), cleared);
        // The clear value is neither of the two numbers an unwritten depth plane
        // holds, which is what makes the gate's comparison evidence.
        const { assert!(PROBE_DEPTH_CLEAR > 0.0 && PROBE_DEPTH_CLEAR < 1.0) };
    }

    /// The stencil probe's bytes, read the way JS reads them.
    fn stencil_bytes() -> Vec<u8> {
        let len = __crcbl_web_gpu_probe_stencil_bytes_len() as usize;
        let ptr = __crcbl_web_gpu_probe_stencil_bytes_ptr();
        if len == 0 {
            return Vec::new();
        }
        assert!(
            !ptr.is_null(),
            "the stencil probe answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::stencil` bytes, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        bytes.to_vec()
    }

    /// How [`PROBE_STENCIL_WGSL`] spells one `Rgba8Unorm` colour: each channel as
    /// its byte over the format's maximum, in `r, g, b, a` order.
    ///
    /// Built here from the byte constants rather than copied out of the shader,
    /// so the assertions below compare the string with the values the gate reads
    /// back instead of with themselves.
    fn wgsl_colour(bytes: [u8; 4]) -> String {
        format!(
            "vec4<f32>({}.0/255.0, {}.0/255.0, {}.0/255.0, {}.0/255.0)",
            bytes[0], bytes[1], bytes[2], bytes[3]
        )
    }

    /// **The shader paints the two colours the gate tells apart, on the right
    /// sides of the `select`.**
    ///
    /// The whole `select(…)` is matched rather than the two colours separately:
    /// with the arms swapped each colour is still present, the shader still
    /// compiles, and the probe reports the exact opposite of what happened.
    #[test]
    fn the_stencil_wgsl_paints_the_colours_the_gate_asserts() {
        let select = format!(
            "select({}, {}, vertex < 3u)",
            wgsl_colour(PROBE_STENCIL_SECOND_BYTES),
            wgsl_colour(PROBE_STENCIL_FIRST_BYTES),
        );
        assert!(
            PROBE_STENCIL_WGSL.contains(&select),
            "the WGSL gives vertices 0..3 the first colour and 3..6 the second: {PROBE_STENCIL_WGSL}"
        );
    }

    /// **The clear the pass loads with is the background byte for byte**, so a
    /// texel no draw reached reads back as the value the gate calls "neither
    /// reference took effect" rather than as something near it.
    #[test]
    fn the_stencil_clear_is_the_background_colour_the_gate_asserts() {
        let clear = probe_stencil_clear_value();
        for (channel, byte) in clear.color.iter().zip(PROBE_STENCIL_BACKGROUND_BYTES) {
            let encoded = (channel * 255.0).round() as u8;
            assert_eq!(encoded, byte, "the clear encodes to the background bytes");
        }
        assert_eq!(clear.stencil, PROBE_STENCIL_CLEARED);
    }

    /// **No two of the three colours are a channel permutation of each other**,
    /// so a path that swapped `r` and `b` on the way out cannot turn one reading
    /// into another — which would make the probe report a pass for a failure.
    /// Each channel is also a mid-tone, away from the `0` and `255` an untouched
    /// or saturated one reads as.
    #[test]
    fn the_three_stencil_colours_survive_a_channel_swap() {
        let sorted = |bytes: [u8; 4]| {
            let mut rgb = [bytes[0], bytes[1], bytes[2]];
            rgb.sort_unstable();
            rgb
        };
        let background = sorted(PROBE_STENCIL_BACKGROUND_BYTES);
        let first = sorted(PROBE_STENCIL_FIRST_BYTES);
        let second = sorted(PROBE_STENCIL_SECOND_BYTES);
        assert_ne!(background, first);
        assert_ne!(background, second);
        assert_ne!(first, second);
        for bytes in [
            PROBE_STENCIL_BACKGROUND_BYTES,
            PROBE_STENCIL_FIRST_BYTES,
            PROBE_STENCIL_SECOND_BYTES,
        ] {
            let [r, g, b, a] = bytes;
            assert_eq!(a, 255, "every colour is opaque");
            assert!(
                r != g && g != b && r != b,
                "the three channels differ: {bytes:?}"
            );
            for channel in [r, g, b] {
                assert!(
                    channel > 0 && channel < 255,
                    "every channel is a mid-tone: {bytes:?}"
                );
            }
        }
    }

    /// **Every stencil handle is generation nine**, a generation past the depth
    /// probe's `8 << 32` and every probe before it: the two images, their two
    /// views, the readback buffer, the command buffer, the queue and the readback
    /// are all live at once and none may land in another probe's slot in the
    /// shared page.
    #[test]
    fn the_stencil_handles_are_a_generation_past_every_other_probe() {
        for bits in [
            PROBE_STENCIL_IMAGE.to_bits(),
            PROBE_STENCIL_IMAGE_VIEW.to_bits(),
            PROBE_STENCIL_PLANE_IMAGE.to_bits(),
            PROBE_STENCIL_PLANE_VIEW.to_bits(),
            PROBE_STENCIL_BUFFER.to_bits(),
            PROBE_STENCIL_SHADER_MODULE.to_bits(),
            PROBE_STENCIL_PIPELINE_LAYOUT.to_bits(),
            PROBE_STENCIL_PIPELINE.to_bits(),
            PROBE_STENCIL_COMMAND_BUFFER.to_bits(),
            PROBE_STENCIL_QUEUE.to_bits(),
            PROBE_STENCIL_READBACK.to_bits(),
        ] {
            assert_eq!(bits >> 32, 9, "every stencil handle is generation nine");
        }
        // The two images and the two views share a generation, so their indices
        // are what keeps them apart.
        assert_ne!(
            PROBE_STENCIL_IMAGE.to_bits(),
            PROBE_STENCIL_PLANE_IMAGE.to_bits()
        );
        assert_ne!(
            PROBE_STENCIL_IMAGE_VIEW.to_bits(),
            PROBE_STENCIL_PLANE_VIEW.to_bits()
        );
        // A generation clear of the depth probe (`8 << 32`), the nearest
        // neighbour.
        assert_ne!(PROBE_STENCIL_IMAGE.to_bits(), PROBE_DEPTH_IMAGE.to_bits());
    }

    /// **The pipeline compares `Equal` against a baked reference that matches
    /// nothing, and writes no stencil back.**
    ///
    /// Each of those is load-bearing: a comparison other than `Equal` would let
    /// the miss through, a baked reference equal to the cleared value would make
    /// "the reference arrived" indistinguishable from "the pipeline decided", and
    /// a non-zero write mask would let the first draw change the plane the second
    /// is tested against.
    #[test]
    fn the_stencil_pipeline_can_only_be_satisfied_by_the_per_pass_reference() {
        let stencil = probe_stencil_pipeline_desc()
            .depth_stencil
            .and_then(|ds| ds.stencil)
            .expect("the stencil pipeline has a stencil state");
        assert_eq!(stencil.front, stencil.back);
        assert_eq!(stencil.front.compare, CompareOp::Equal);
        assert_eq!(stencil.write_mask, 0, "nothing writes the plane back");
        assert_eq!(stencil.read_mask, PROBE_STENCIL_READ_MASK);
        assert_eq!(stencil.reference, PROBE_STENCIL_BAKED);
        // The three values are pairwise distinct, so each of the gate's three
        // readings has exactly one cause.
        assert_ne!(PROBE_STENCIL_BAKED, PROBE_STENCIL_CLEARED);
        assert_ne!(PROBE_STENCIL_BAKED, PROBE_STENCIL_MISS);
        assert_ne!(PROBE_STENCIL_CLEARED, PROBE_STENCIL_MISS);
        // And `0` is none of them: it is WebGPU's own initial reference for a
        // fresh pass, so a probe that used it could not tell a reference that
        // arrived from one that never did.
        assert_ne!(PROBE_STENCIL_CLEARED, 0);
        assert_ne!(PROBE_STENCIL_MISS, 0);
    }

    /// The stencil half: **one export, a whole frame** whose two draws are each
    /// preceded by a `SetStencilReference`, in that order.
    #[test]
    fn the_stencil_export_encodes_a_reference_before_each_draw() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_stencil(), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateImage",
                "CreateImageView",
                "CreateImage",
                "CreateImageView",
                "CreateBuffer",
                "CreateShaderModule",
                "CreatePipelineLayout",
                "CreateGraphicsPipeline",
                "CreateCommandEncoder",
                "BeginRenderPass",
                "BindGraphicsPipeline",
                "SetStencilReference",
                "Draw",
                "SetStencilReference",
                "Draw",
                "EndRenderPass",
                "CopyImageToBuffer",
                "Finish",
                "Submit",
                "RequestReadback",
            ],
            "each draw is preceded by its own stencil reference"
        );
        // The values and their order, which the name list above cannot see. The
        // matching reference comes first: reversing the two would make "the
        // stencil test is not enabled" and "both references were applied" produce
        // the same texel.
        let references: Vec<u32> = commands
            .iter()
            .filter_map(|command| match command {
                Command::SetStencilReference { reference } => Some(*reference),
                _ => None,
            })
            .collect();
        assert_eq!(references, vec![PROBE_STENCIL_CLEARED, PROBE_STENCIL_MISS]);
        // The two draws cover the same triangle from different vertex ranges, so
        // the colour is the only thing that differs between them.
        let draws: Vec<(u32, u32)> = commands
            .iter()
            .filter_map(|command| match command {
                Command::Draw { vertices, .. } => Some((vertices.start, vertices.end)),
                _ => None,
            })
            .collect();
        assert_eq!(draws, vec![(0, 3), (3, 6)]);
        // The pass clears the stencil plane and stores it, and has the colour
        // target the copy afterwards reads.
        let pass = commands
            .iter()
            .find_map(|command| match command {
                Command::BeginRenderPass {
                    color_attachments,
                    depth_stencil_attachment,
                    ..
                } => Some((color_attachments.clone(), *depth_stencil_attachment)),
                _ => None,
            })
            .expect("the frame begins a render pass");
        assert_eq!(pass.0, vec![probe_stencil_color_attachment()]);
        assert_eq!(pass.1, Some(probe_stencil_attachment()));
    }

    /// A stencil request before a device opens is refused and encodes nothing.
    #[test]
    fn a_stencil_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_stencil(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);
        assert_eq!(__crcbl_web_gpu_probe_stencil_state(), STENCIL_UNASKED);
    }

    /// The whole stencil exchange through the exports alone: request, poll, and a
    /// `ReadbackReady` carrying the first draw's colour for every texel, which
    /// reaches the bytes exports. A `cargo test` has no `navigator.gpu`, so the
    /// replayer is stood in for by a `ReplyWriter` — which is why this proves the
    /// state machine and the browser gate proves the value.
    #[test]
    fn the_stencil_readback_reaches_the_bytes_exports_as_the_first_draws_colour() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_stencil(), 1);
        let setup = take_frame();
        let poll_sequence = 2 + setup.len() as u64;
        assert_eq!(__crcbl_web_gpu_probe_stencil_state(), STENCIL_REQUESTED);

        assert_eq!(__crcbl_web_gpu_probe_stencil_poll(), 1);
        assert_eq!(__crcbl_web_gpu_probe_stencil_state(), STENCIL_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::PollReadback {
                readback: PROBE_STENCIL_READBACK,
            }]
        );

        let texels = (PROBE_STENCIL_SIZE as usize) * (PROBE_STENCIL_SIZE as usize);
        let mut masked = Vec::new();
        for _ in 0..texels {
            masked.extend_from_slice(&PROBE_STENCIL_FIRST_BYTES);
        }
        let mut replies = ReplyWriter::new();
        replies.readback_ready(poll_sequence, PROBE_STENCIL_READBACK, &masked);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_stencil_state(), STENCIL_READY);
        assert_eq!(stencil_bytes(), masked);
        assert_eq!(&stencil_bytes()[..4], PROBE_STENCIL_FIRST_BYTES);
    }

    /// The occlusion probe's readback bytes, read the way JS reads them.
    fn occlusion_bytes() -> Vec<u8> {
        let len = __crcbl_web_gpu_probe_occlusion_bytes_len() as usize;
        let ptr = __crcbl_web_gpu_probe_occlusion_bytes_ptr();
        if len == 0 {
            return Vec::new();
        }
        assert!(
            !ptr.is_null(),
            "the occlusion probe answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::occlusion` bytes,
        // which nothing between the two calls above can have moved — neither
        // export allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        bytes.to_vec()
    }

    /// The occlusion probe's direct-read values, read the way JS reads them.
    fn occlusion_values() -> Vec<u8> {
        let len = __crcbl_web_gpu_probe_occlusion_values_len() as usize;
        let ptr = __crcbl_web_gpu_probe_occlusion_values_ptr();
        if len == 0 {
            return Vec::new();
        }
        assert!(
            !ptr.is_null(),
            "the occlusion probe answered a values length with no pointer"
        );
        // SAFETY: as `occlusion_bytes`, on `Probe::occlusion_values`.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        bytes.to_vec()
    }

    /// An occlusion request before a device opens is refused and encodes nothing.
    #[test]
    fn an_occlusion_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_occlusion(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);
        assert_eq!(__crcbl_web_gpu_probe_occlusion_state(), OCCLUSION_UNASKED);
        assert_eq!(
            __crcbl_web_gpu_probe_occlusion_values_state(),
            OCCLUSION_VALUES_UNASKED
        );
    }

    /// **The setup frame is the whole spine, in the order a caller records it.**
    ///
    /// The set, the two buffers, the sentinel upload, the encoder, the reset over
    /// the whole range, the resolve over the sentinel, the copy, the finish, the
    /// submit, the readback request and the direct-read ask — and the reset is in
    /// there although WebGPU has no reset, because the seam requires every caller
    /// to record one and a frame that skipped it would not be the frame a caller
    /// writes.
    #[test]
    fn the_occlusion_export_encodes_the_whole_query_spine() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_occlusion(), 1);
        let frame = take_frame();
        let names: Vec<&str> = frame.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateQuerySet",
                "CreateBuffer",
                "CreateBuffer",
                "WriteBuffer",
                "CreateCommandEncoder",
                "ResetQuerySet",
                "ResolveQuerySet",
                "CopyBufferToBuffer",
                "Finish",
                "Submit",
                "RequestReadback",
                "QueryResults",
            ],
        );
        assert_eq!(
            frame[0],
            Command::CreateQuerySet {
                set: PROBE_OCCLUSION_SET,
                label: Some("crcbl-webgpu occlusion set".into()),
                kind: QueryKind::Occlusion,
                count: PROBE_OCCLUSION_QUERIES,
            }
        );
        assert_eq!(
            frame[6],
            Command::ResolveQuerySet {
                set: PROBE_OCCLUSION_SET,
                first_query: 0,
                query_count: PROBE_OCCLUSION_QUERIES,
                dst: PROBE_OCCLUSION_RESOLVE_BUFFER,
                dst_offset: 0,
            }
        );
        assert_eq!(
            frame[11],
            Command::QueryResults {
                set: PROBE_OCCLUSION_SET,
                first_query: 0,
                query_count: PROBE_OCCLUSION_QUERIES,
            }
        );
        assert_eq!(__crcbl_web_gpu_probe_occlusion_state(), OCCLUSION_REQUESTED);
        assert_eq!(
            __crcbl_web_gpu_probe_occlusion_values_state(),
            OCCLUSION_VALUES_WAITING
        );
    }

    /// **A device without `timestamp-query` encodes nothing at all**, and says
    /// which of the two reasons that was.
    ///
    /// The distinction the export exists for: a probe that quietly did nothing
    /// and a browser that cannot serve one read the same from outside, so the
    /// supported flag is what the gate reads to tell "this browser has no
    /// timestamps" from "the request never happened". A set of another kind
    /// encoded here instead would let the whole group pass while proving nothing
    /// about timestamps.
    #[test]
    fn a_timestamp_request_without_the_feature_encodes_nothing_and_says_why() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_timestamp_supported(), 0);
        assert_eq!(__crcbl_web_gpu_probe_timestamp(), 0);
        assert!(take_frame().is_empty(), "a refused request encodes nothing");
        assert_eq!(
            __crcbl_web_gpu_probe_timestamp_state(),
            TIMESTAMP_UNSUPPORTED
        );
        assert_eq!(__crcbl_web_gpu_probe_timestamp_len(), 0);
    }

    /// **The timed frame is a pass that names both of its queries, and the read
    /// that follows it** — the whole of `Capability::TimestampQuery` on this
    /// backend, as a command stream.
    ///
    /// The set, the encoder, the reset the seam requires of every caller, the
    /// pass carrying its [`PassTimestampWrites`], the finish, the submit and the
    /// direct read. The pass command is asserted whole rather than by name: a
    /// `BeginComputePass` that arrived with `timestamp_writes: None` would give
    /// the same command names and replay as a pass that runs and measures
    /// nothing, which is the outcome this capability was refused over.
    #[test]
    fn the_timestamp_export_encodes_a_pass_that_names_both_its_queries() {
        grant(&granted("has timestamps"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(take_frame().len(), 1);
        let mut replies = ReplyWriter::new();
        replies.device(
            1,
            &DeviceCaps {
                features: Features::COMPUTE | Features::TIMESTAMP_QUERY,
                ..device_caps()
            },
        );
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_OPENED);

        assert_eq!(__crcbl_web_gpu_probe_timestamp_supported(), 1);
        assert_eq!(__crcbl_web_gpu_probe_timestamp(), 1);
        let frame = take_frame();
        let names: Vec<&str> = frame.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateQuerySet",
                "CreateCommandEncoder",
                "ResetQuerySet",
                "BeginComputePass",
                "EndComputePass",
                "Finish",
                "Submit",
                "QueryResults",
            ],
        );
        assert_eq!(
            frame[0],
            Command::CreateQuerySet {
                set: PROBE_TIMESTAMP_SET,
                label: Some("crcbl-webgpu timestamp set".into()),
                kind: QueryKind::Timestamp,
                count: PROBE_TIMESTAMP_QUERIES,
            }
        );
        assert_eq!(
            frame[3],
            Command::BeginComputePass {
                label: Some("crcbl-webgpu timed pass".into()),
                timestamp_writes: Some(PassTimestampWrites {
                    set: PROBE_TIMESTAMP_SET,
                    beginning_of_pass: 0,
                    end_of_pass: 1,
                }),
            }
        );
        assert_eq!(
            frame[7],
            Command::QueryResults {
                set: PROBE_TIMESTAMP_SET,
                first_query: 0,
                query_count: PROBE_TIMESTAMP_QUERIES,
            }
        );
        assert_eq!(__crcbl_web_gpu_probe_timestamp_state(), TIMESTAMP_WAITING);

        // And the answer reaches the bytes exports, in the order it was sent:
        // the opening tick first, the closing one second.
        let mut replies = ReplyWriter::new();
        // Sequence 9: the enumeration spent 0 and the device request 1, then
        // the seven commands of the frame above, so the awaited read is ninth.
        replies.query_results(9, PROBE_TIMESTAMP_SET, 0, &[41, 99]);
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_timestamp_state(), TIMESTAMP_READY);
        assert_eq!(__crcbl_web_gpu_probe_timestamp_len(), 16);
        let ptr = __crcbl_web_gpu_probe_timestamp_ptr();
        assert!(!ptr.is_null());
        // SAFETY: the length above is the probe's own, and nothing has called
        // back into wasm since it was read.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, 16) };
        let ticks: Vec<u64> = bytes
            .chunks_exact(8)
            .map(|word| u64::from_le_bytes(word.try_into().expect("eight bytes")))
            .collect();
        assert_eq!(ticks, vec![41, 99]);
    }

    /// **The sentinel covers the whole destination**, so a resolve that reached
    /// only part of it leaves the rest saying so rather than reading as a zero the
    /// resolve wrote. And the sentinel is not zero, which is the value the gate
    /// asserts the resolve produces.
    #[test]
    fn the_occlusion_sentinel_covers_the_destination_and_is_not_the_answer() {
        let sentinel = probe_occlusion_sentinel_bytes();
        assert_eq!(sentinel.len() as u64, PROBE_OCCLUSION_BYTES);
        assert_eq!(sentinel.len() as u64, probe_occlusion_copy().size);
        assert_eq!(
            sentinel.len() as u64,
            probe_occlusion_resolve_buffer_desc().size
        );
        assert!(
            sentinel
                .iter()
                .all(|byte| *byte == PROBE_OCCLUSION_SENTINEL)
        );
        assert_ne!(PROBE_OCCLUSION_SENTINEL, 0);
    }

    /// **The resolve destination carries the usage WebGPU demands of one.**
    /// `QUERY_RESOLVE` is the bit the replayer refuses a resolve without, and the
    /// two transfer bits are what let the sentinel in and the values out.
    #[test]
    fn the_occlusion_resolve_destination_is_usable_as_one() {
        let desc = probe_occlusion_resolve_buffer_desc();
        assert!(desc.usage.contains(BufferUsage::QUERY_RESOLVE));
        assert!(desc.usage.contains(BufferUsage::TRANSFER_DST));
        assert!(desc.usage.contains(BufferUsage::TRANSFER_SRC));
        assert_eq!(desc.memory, MemoryLocation::DeviceLocal);
        // And the readback buffer is a separate one, because WebGPU lets
        // `MAP_READ` combine with `COPY_DST` and nothing else.
        assert_ne!(PROBE_OCCLUSION_RESOLVE_BUFFER, PROBE_OCCLUSION_BUFFER);
        assert_eq!(
            probe_occlusion_buffer_desc().memory,
            MemoryLocation::HostReadback
        );
    }

    /// The whole occlusion exchange through the exports alone: request, poll, a
    /// `ReadbackReady` carrying zeros for every query, and a `QueryResults`
    /// carrying the same values the other way. A `cargo test` has no
    /// `navigator.gpu`, so the replayer is stood in for by a `ReplyWriter` — which
    /// is why this proves the state machine and the browser gate proves the value.
    #[test]
    fn both_occlusion_reads_reach_their_exports_as_the_values_the_browser_answered() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_occlusion(), 1);
        let setup = take_frame();
        // The direct-read ask is the frame's last command, and the poll below is
        // the next command on the channel.
        let values_sequence = 1 + setup.len() as u64;
        let poll_sequence = 2 + setup.len() as u64;

        assert_eq!(__crcbl_web_gpu_probe_occlusion_poll(), 1);
        assert_eq!(__crcbl_web_gpu_probe_occlusion_state(), OCCLUSION_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::PollReadback {
                readback: PROBE_OCCLUSION_READBACK,
            }]
        );

        let resolved = vec![0u8; PROBE_OCCLUSION_BYTES as usize];
        let values = vec![0u64; PROBE_OCCLUSION_QUERIES as usize];
        let mut replies = ReplyWriter::new();
        replies.readback_ready(poll_sequence, PROBE_OCCLUSION_READBACK, &resolved);
        replies.query_results(values_sequence, PROBE_OCCLUSION_SET, 0, &values);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_occlusion_state(), OCCLUSION_READY);
        assert_eq!(occlusion_bytes(), resolved);
        assert_eq!(
            __crcbl_web_gpu_probe_occlusion_values_state(),
            OCCLUSION_VALUES_READY
        );
        assert_eq!(
            occlusion_values().len() as u64,
            PROBE_OCCLUSION_BYTES,
            "one little-endian u64 per query"
        );
        assert!(occlusion_values().iter().all(|byte| *byte == 0));
    }

    /// **A direct read the replayer could not serve answers an empty list**, and
    /// that is the only way `Reply::QueryResults` can say so — so the exports must
    /// not report it as a `READY` with values. The state is `READY` and the length
    /// is zero, which is what the gate distinguishes.
    #[test]
    fn an_unservable_direct_read_answers_ready_with_no_values() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_occlusion(), 1);
        let values_sequence = 1 + take_frame().len() as u64;

        let mut replies = ReplyWriter::new();
        replies.query_results(values_sequence, PROBE_OCCLUSION_SET, 0, &[]);
        deliver(replies.bytes());

        assert_eq!(
            __crcbl_web_gpu_probe_occlusion_values_state(),
            OCCLUSION_VALUES_READY
        );
        assert_eq!(__crcbl_web_gpu_probe_occlusion_values_len(), 0);
        assert!(occlusion_values().is_empty());
    }

    /// The MSAA probe's bytes, read the way JS reads them.
    fn msaa_bytes() -> Vec<u8> {
        let len = __crcbl_web_gpu_probe_msaa_bytes_len() as usize;
        let ptr = __crcbl_web_gpu_probe_msaa_bytes_ptr();
        if len == 0 {
            return Vec::new();
        }
        assert!(
            !ptr.is_null(),
            "the MSAA probe answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `Probe::msaa` bytes, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        bytes.to_vec()
    }

    /// **The clear and the poison are not a channel permutation of each other**,
    /// so a path that swapped `r` and `b` on the way out cannot turn "the resolve
    /// was dropped" into "the resolve ran". Each colour channel is a mid-tone,
    /// away from the `0` and `255` an untouched or saturated one reads as, and the
    /// two alphas differ so a resolve that left alpha alone is its own reading.
    #[test]
    fn the_msaa_clear_and_poison_survive_a_channel_swap() {
        let sorted = |bytes: [u8; 4]| {
            let mut channels = bytes;
            channels.sort_unstable();
            channels
        };
        assert_ne!(
            sorted(PROBE_MSAA_CLEAR_BYTES),
            sorted(PROBE_MSAA_POISON_BYTES)
        );
        assert_ne!(PROBE_MSAA_CLEAR_BYTES[3], PROBE_MSAA_POISON_BYTES[3]);
        for bytes in [PROBE_MSAA_CLEAR_BYTES, PROBE_MSAA_POISON_BYTES] {
            let [r, g, b, _] = bytes;
            assert!(
                r != g && g != b && r != b,
                "the three colour channels differ: {bytes:?}"
            );
            for channel in [r, g, b] {
                assert!(
                    channel > 0 && channel < 255,
                    "every colour channel is a mid-tone: {bytes:?}"
                );
            }
        }
        // And every channel is far enough from its counterpart that a resolve
        // which wrote one channel and dropped another is visible rather than
        // arguable.
        for (clear, poison) in PROBE_MSAA_CLEAR_BYTES
            .iter()
            .zip(PROBE_MSAA_POISON_BYTES.iter())
        {
            assert!(
                clear.abs_diff(*poison) > 16,
                "{clear} and {poison} are too close to tell apart"
            );
        }
    }

    /// The clear value the pass carries is the bytes the gate asserts, through
    /// the one conversion between them.
    #[test]
    fn the_msaa_clear_is_the_colour_the_gate_asserts() {
        let clear = probe_msaa_clear_value();
        for (channel, byte) in clear.color.iter().zip(PROBE_MSAA_CLEAR_BYTES) {
            let encoded = (channel * 255.0).round() as u8;
            assert_eq!(encoded, byte, "the clear encodes to the asserted bytes");
        }
    }

    /// The prime covers the **whole** resolve target with the poison, in the
    /// channel order the readback compares in. A prime that covered less would
    /// leave the uncovered part undefined, and an undefined byte that happened to
    /// equal the clear reads as a resolve that ran.
    #[test]
    fn the_msaa_prime_is_the_poison_over_the_whole_target() {
        let prime = probe_msaa_prime_bytes();
        assert_eq!(prime.len() as u64, PROBE_MSAA_BYTES);
        assert_eq!(prime.len() as u64, probe_msaa_prime_buffer_desc().size);
        assert_eq!(prime.len() as u64, probe_msaa_buffer_desc().size);
        for texel in prime.chunks_exact(4) {
            assert_eq!(texel, PROBE_MSAA_POISON_BYTES);
        }
    }

    /// **A row of the copy is a multiple of 256 bytes, and there is more than one
    /// of them.** The first is what `copyBufferToTexture` and
    /// `copyTextureToBuffer` both require of a tightly packed copy; the second is
    /// what makes a resolve that wrote only row zero fail here.
    #[test]
    fn the_msaa_copy_rows_are_aligned_and_there_is_more_than_one() {
        assert_eq!((PROBE_MSAA_WIDTH as usize) * 4 % 256, 0);
        const { assert!(PROBE_MSAA_HEIGHT > 1) };
        assert_eq!(
            PROBE_MSAA_BYTES,
            u64::from(PROBE_MSAA_WIDTH) * u64::from(PROBE_MSAA_HEIGHT) * 4
        );
    }

    /// **Every MSAA handle is generation ten**, a generation past the stencil
    /// probe's `9 << 32` and every probe before it: the two images, their two
    /// views, the two buffers, the command buffer, the queue and the readback are
    /// all live at once and none may land in another probe's slot in the shared
    /// page.
    #[test]
    fn the_msaa_handles_are_a_generation_past_every_other_probe() {
        for bits in [
            PROBE_MSAA_IMAGE.to_bits(),
            PROBE_MSAA_RESOLVE_IMAGE.to_bits(),
            PROBE_MSAA_IMAGE_VIEW.to_bits(),
            PROBE_MSAA_RESOLVE_VIEW.to_bits(),
            PROBE_MSAA_PRIME_BUFFER.to_bits(),
            PROBE_MSAA_BUFFER.to_bits(),
            PROBE_MSAA_COMMAND_BUFFER.to_bits(),
            PROBE_MSAA_QUEUE.to_bits(),
            PROBE_MSAA_READBACK.to_bits(),
        ] {
            assert_eq!(bits >> 32, 10, "every MSAA handle is generation ten");
        }
        // The pairs that share a generation are kept apart by their indices.
        assert_ne!(
            PROBE_MSAA_IMAGE.to_bits(),
            PROBE_MSAA_RESOLVE_IMAGE.to_bits()
        );
        assert_ne!(
            PROBE_MSAA_IMAGE_VIEW.to_bits(),
            PROBE_MSAA_RESOLVE_VIEW.to_bits()
        );
        assert_ne!(
            PROBE_MSAA_PRIME_BUFFER.to_bits(),
            PROBE_MSAA_BUFFER.to_bits()
        );
        // A generation clear of the stencil probe (`9 << 32`), the nearest
        // neighbour.
        assert_ne!(PROBE_MSAA_IMAGE.to_bits(), PROBE_STENCIL_IMAGE.to_bits());
    }

    /// **The multisampled target is the only multisampled thing, and the resolve
    /// target is the only one anything copies.** Both halves are what make the
    /// readback mean something: a single-sampled source would make the resolve a
    /// no-op the gate could not see, and a multisampled destination is a transfer
    /// source WebGPU has no usage bit for.
    #[test]
    fn the_msaa_source_is_multisampled_and_only_the_resolve_target_is_copied() {
        let source = probe_msaa_image_desc();
        let target = probe_msaa_resolve_image_desc();
        assert_eq!(source.samples, PROBE_MSAA_SAMPLES);
        const { assert!(PROBE_MSAA_SAMPLES > 1) };
        assert_eq!(source.usage, ImageUsage::COLOR_ATTACHMENT);
        assert_eq!(target.samples, 1);
        assert!(target.usage.contains(ImageUsage::COLOR_ATTACHMENT));
        assert!(target.usage.contains(ImageUsage::TRANSFER_DST));
        assert!(target.usage.contains(ImageUsage::TRANSFER_SRC));
        assert_eq!(source.extent, target.extent);
        assert_eq!(source.format, target.format);
        // Both copies name the resolve target, never the multisampled source.
        assert_eq!(probe_msaa_prime_copy().image, PROBE_MSAA_RESOLVE_IMAGE);
        assert_eq!(probe_msaa_copy().image, PROBE_MSAA_RESOLVE_IMAGE);
    }

    /// The MSAA half: **one export, a whole frame** whose pass has no draws at
    /// all and whose one attachment names the primed target as its resolve.
    #[test]
    fn the_msaa_export_encodes_a_clear_that_resolves_into_the_primed_target() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_msaa(), 1);
        let commands = take_frame();
        let names: Vec<&str> = commands.iter().map(Command::name).collect();
        assert_eq!(
            names,
            vec![
                "CreateImage",
                "CreateImageView",
                "CreateImage",
                "CreateImageView",
                "CreateBuffer",
                "CreateBuffer",
                "WriteBuffer",
                "CreateCommandEncoder",
                "CopyBufferToImage",
                "BeginRenderPass",
                "EndRenderPass",
                "CopyImageToBuffer",
                "Finish",
                "Submit",
                "RequestReadback",
            ],
            "the prime lands before the pass, and the pass has no contents"
        );
        // The whole claim of the probe: the pass's one attachment is the
        // multisampled view, and it names the single-sampled one as its resolve.
        let pass = commands
            .iter()
            .find_map(|command| match command {
                Command::BeginRenderPass {
                    color_attachments, ..
                } => Some(color_attachments.clone()),
                _ => None,
            })
            .expect("the frame begins a render pass");
        assert_eq!(pass, vec![probe_msaa_color_attachment()]);
        assert_eq!(pass[0].view, PROBE_MSAA_IMAGE_VIEW);
        assert_eq!(pass[0].resolve, Some(PROBE_MSAA_RESOLVE_VIEW));
        // And the prime really carries the poison, rather than an empty write
        // that would leave the target undefined.
        let written = commands
            .iter()
            .find_map(|command| match command {
                Command::WriteBuffer { buffer, data, .. } => Some((*buffer, data.clone())),
                _ => None,
            })
            .expect("the frame writes the prime buffer");
        assert_eq!(written.0, PROBE_MSAA_PRIME_BUFFER);
        assert_eq!(written.1, probe_msaa_prime_bytes());
    }

    /// An MSAA request before a device opens is refused and encodes nothing.
    #[test]
    fn an_msaa_request_before_a_device_opens_is_refused_and_encodes_nothing() {
        assert_eq!(__crcbl_web_gpu_probe_msaa(), 0);
        assert_eq!(__crcbl_web_gpu_stream_len(), 0);
        assert_eq!(__crcbl_web_gpu_probe_msaa_state(), MSAA_UNASKED);
        assert_eq!(__crcbl_web_gpu_probe_msaa_samples(), 0);
    }

    /// **A device below [`PROBE_MSAA_SAMPLES`] leaves the group unexercised, not
    /// passed.** Nothing goes on the stream, the state says why, and the sample
    /// count the device reported is readable — so the gate can name the number
    /// rather than guess at the reason.
    #[test]
    fn a_device_below_the_probes_sample_count_encodes_nothing_and_says_what_it_reported() {
        grant(&granted("one sample only"));
        assert_eq!(__crcbl_web_gpu_probe_device(), 1);
        assert_eq!(take_frame().len(), 1);
        let mut replies = ReplyWriter::new();
        replies.device(
            1,
            &DeviceCaps {
                features: Features::COMPUTE,
                limits: Limits {
                    max_sample_count: 1,
                    ..device_caps().limits
                },
            },
        );
        deliver(replies.bytes());
        assert_eq!(__crcbl_web_gpu_probe_device_state(), DEVICE_OPENED);

        assert_eq!(__crcbl_web_gpu_probe_msaa_samples(), 1);
        assert_eq!(__crcbl_web_gpu_probe_msaa(), 0);
        assert_eq!(__crcbl_web_gpu_probe_msaa_state(), MSAA_UNSUPPORTED);
        // And there is nothing to poll for either, so a gate that polls blindly
        // cannot put a command on the stream for a frame that was never encoded.
        assert_eq!(__crcbl_web_gpu_probe_msaa_poll(), 0);
        assert_eq!(
            take_frame(),
            vec![],
            "an unsupported device leaves the stream empty"
        );
    }

    /// The whole MSAA exchange through the exports alone: request, poll, and a
    /// `ReadbackReady` carrying the clear colour for every texel, which reaches
    /// the bytes exports. A `cargo test` has no `navigator.gpu`, so the replayer
    /// is stood in for by a `ReplyWriter` — which is why this proves the state
    /// machine and the browser gate proves the value.
    #[test]
    fn the_msaa_readback_reaches_the_bytes_exports_as_the_clear_colour() {
        open_device();
        assert_eq!(__crcbl_web_gpu_probe_msaa_samples(), PROBE_MSAA_SAMPLES);
        assert_eq!(__crcbl_web_gpu_probe_msaa(), 1);
        let setup = take_frame();
        let poll_sequence = 2 + setup.len() as u64;
        assert_eq!(__crcbl_web_gpu_probe_msaa_state(), MSAA_REQUESTED);

        assert_eq!(__crcbl_web_gpu_probe_msaa_poll(), 1);
        assert_eq!(__crcbl_web_gpu_probe_msaa_state(), MSAA_WAITING);
        assert_eq!(
            take_frame(),
            vec![Command::PollReadback {
                readback: PROBE_MSAA_READBACK,
            }]
        );

        let mut resolved = Vec::new();
        while (resolved.len() as u64) < PROBE_MSAA_BYTES {
            resolved.extend_from_slice(&PROBE_MSAA_CLEAR_BYTES);
        }
        let mut replies = ReplyWriter::new();
        replies.readback_ready(poll_sequence, PROBE_MSAA_READBACK, &resolved);
        deliver(replies.bytes());

        assert_eq!(__crcbl_web_gpu_probe_msaa_state(), MSAA_READY);
        assert_eq!(msaa_bytes(), resolved);
        assert_eq!(&msaa_bytes()[..4], PROBE_MSAA_CLEAR_BYTES);
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

    /// The matrix the last `parity` call left, read the way JS reads it.
    fn parity_report() -> String {
        let len = __crcbl_web_gpu_probe_parity_report_len() as usize;
        let ptr = __crcbl_web_gpu_probe_parity_report_ptr();
        assert!(
            !ptr.is_null(),
            "the probe answered a length with no pointer"
        );
        // SAFETY: `ptr` and `len` are this thread's `ParityReport::report`, which
        // nothing between the two calls above can have moved — neither export
        // allocates.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        String::from_utf8(bytes.to_vec()).expect("the probe's report is a Rust String")
    }

    /// The disagreements the last `parity` call left, read the same way.
    fn parity_failures() -> String {
        let len = __crcbl_web_gpu_probe_parity_failures_len() as usize;
        let ptr = __crcbl_web_gpu_probe_parity_failures_ptr();
        assert!(
            !ptr.is_null(),
            "the probe answered a length with no pointer"
        );
        // SAFETY: as `parity_report`, on `ParityReport::failures`.
        let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
        String::from_utf8(bytes.to_vec()).expect("the probe's failures are a Rust String")
    }

    /// **The report cannot pass by walking nothing**, which is the one way a
    /// parity check agrees with every list there is.
    ///
    /// The browser group asserts the same three numbers against a real device;
    /// this asserts them through the exports with no browser, so the ABI — six
    /// symbols and a `PARITY_*` code — is held here rather than only in a gate
    /// that needs a GPU. What it deliberately does **not** assert is the verdict:
    /// the answers depend on `DIVERGENCES` and on a real device's features, and
    /// a native copy of that comparison would be a second place to update.
    #[test]
    fn the_parity_report_refuses_before_a_device_and_then_walks_every_capability() {
        // Before a device there is nothing to build a `WebGpuDevice` around, and
        // the counts stay zero rather than reporting an empty agreement.
        assert_eq!(__crcbl_web_gpu_probe_parity(), PARITY_NO_DEVICE);
        assert_eq!(__crcbl_web_gpu_probe_parity_checked(), 0);
        assert_eq!(__crcbl_web_gpu_probe_parity_held(), 0);

        open_device();
        let state = __crcbl_web_gpu_probe_parity();
        assert!(
            state == PARITY_MATCHED || state == PARITY_MISMATCHED,
            "a device has opened, so the report must have run; got {state}"
        );
        let checked = __crcbl_web_gpu_probe_parity_checked() as usize;
        assert_eq!(
            checked,
            Capability::ALL.len(),
            "the report walked {checked} of {} capabilities",
            Capability::ALL.len()
        );

        // Every capability is named once, and every verdict is one of the five
        // tokens — a token this test does not know about is a verdict the report
        // learnt to print and nothing learnt to read.
        let report = parity_report();
        let tokens: Vec<&str> = report.split(' ').collect();
        assert_eq!(tokens.len(), checked, "{report}");
        for capability in Capability::ALL {
            assert!(
                tokens
                    .iter()
                    .any(|token| token.starts_with(&format!("{}=", capability.name()))),
                "{capability} is missing from the matrix: {report}"
            );
        }
        for token in &tokens {
            let (_, verdict) = token.split_once('=').expect("every token is name=verdict");
            assert!(
                matches!(
                    verdict,
                    "yes"
                        | "yes:STALE-ROW"
                        | "no:reviewed"
                        | "no:UNREVIEWED"
                        | "no:FALSE-DEVICE-GATE"
                        | "unprovable-here"
                ),
                "unknown verdict token {token:?} in {report}"
            );
        }

        // `held` is the settled count, and it is what stops an all-`unprovable`
        // run reading as a pass. This device reports only `Features::COMPUTE`, so
        // the two feature-gated capabilities are unprovable and the rest are not.
        let held = __crcbl_web_gpu_probe_parity_held() as usize;
        let unprovable = tokens
            .iter()
            .filter(|token| token.ends_with("=unprovable-here"))
            .count();
        assert_eq!(held, checked - unprovable, "{report}");
        assert!(
            held > 0,
            "nothing was settled, so the report asserted nothing"
        );

        // The failures text and the verdict agree — one of them alone would let a
        // mismatch print as a pass, or a pass print a message.
        let failures = parity_failures();
        assert_eq!(
            failures.is_empty(),
            state == PARITY_MATCHED,
            "state {state} against failures {failures:?}"
        );
    }
}
