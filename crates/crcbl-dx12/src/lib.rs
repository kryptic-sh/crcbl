//! `crcbl-dx12` — the engine's Direct3D 12 backend, and Windows' second path to
//! a GPU.
//!
//! `docs/plan/09-backends-metal-dx12.md` puts DX12 last of the two P14 backends
//! and `docs/backlog.md` says why: Windows already has `crcbl-vk`, which is the
//! same code reaching a different loader, so this backend is never a replacement
//! for it. What it is for is the Xbox door, first-class Windows GPU tooling
//! (PIX, DRED), robustness against a missing vendor ICD — and one thing this
//! slice exists to *measure* rather than assume.
//!
//! # What this backend does, and the question it was written to answer
//!
//! **Adapter enumeration, the resource half of the seam, a cleared pixel read
//! back, a triangle drawn — directly, indexed, indirect and with a GPU-side draw
//! count — a compute dispatch read back, a window presented to, and a headless
//! image ring.** `Dx12Instance` lists every D3D12 adapter
//! with its capabilities filled in, which is the one thing
//! [`Instance::adapters`](crcbl_hal::Instance::adapters) promises can be
//! answered before a device exists. `request_device` then opens a real
//! `ID3D12Device` and a `D3D12_COMMAND_LIST_TYPE_DIRECT` queue, and the device
//! it hands back creates buffers, images, image views and samplers, writes into
//! a host-visible buffer, records an `ID3D12GraphicsCommandList`, runs it on the
//! queue, and reads the result back through an `ID3D12Fence`. Semaphores are
//! that same fence handed to a caller: both seam kinds are created, a submission
//! waits on them and signals them through `ID3D12CommandQueue::Wait` and
//! `Signal`, and the CPU reads and blocks on a timeline. All three query kinds
//! are `ID3D12QueryHeap`s the device creates, writes timestamps into and reads
//! back — the read through a resolve and a submission of its own, because
//! **D3D12 has no CPU-side read of a query heap at all**; see
//! `crcbl_dx12::query`. It also builds a
//! `DXGI_SWAP_EFFECT_FLIP_DISCARD` swapchain on an `HWND`, acquires and presents
//! through it, and answers
//! [`Device::wait_until_presented`](crcbl_hal::Device::wait_until_presented)
//! from the frame-latency waitable object — see `crcbl_dx12::swapchain` and
//! `crcbl_dx12::present`.
//!
//! `SurfaceTarget::Offscreen` is accepted too — named rather than linked, for
//! the reason the `Dx12Instance` section below gives about every type in this
//! crate's Windows-only dependencies — and its "swapchain" is a ring of plain `ID3D12Resource`
//! textures with no DXGI object behind it. That is what lets a headless
//! caller — `crcbl screenshot`, and every harness built on it — render a frame
//! and read it back on a machine with no display, through the *same*
//! acquire/present path a window uses rather than a second one.
//!
//! **Nothing in this crate is a stub that reports success** — a draw recorded
//! into an encoder *fails the encoder*, so `finish` hands back the refusal
//! rather than a command buffer that submits and draws nothing. Everything past
//! the clear that no slice has written — buffer fills, mesh dispatch, a
//! push-constant range at pipeline-layout creation — refuses with
//! [`HalError::Unsupported`](crcbl_hal::HalError::Unsupported)
//! whose `what` names the slice the answer arrives in, so a caller reads "not
//! yet" rather than "broken". A refusal that is *permanent* deliberately does
//! not read that way, and names the absence instead: a Wayland, XCB, AppKit or
//! canvas surface names the backend that owns it, because D3D12 presents to an
//! `HWND` and nothing else, and a buffer copy of a `D24UnormS8Uint` image's
//! depth plane names the DXGI format table, which has no fully typed
//! single-plane spelling for 24-bit unorm elements to lay a footprint out with.
//! What is **not** folded into either refusal is
//! anything the backend can genuinely diagnose: an out-of-range adapter is
//! [`NoSuchAdapter`](crcbl_hal::HalError::NoSuchAdapter), a stale handle is
//! [`InvalidHandle`](crcbl_hal::HalError::InvalidHandle), one from another
//! device is [`ForeignObject`](crcbl_hal::HalError::ForeignObject), and a
//! descriptor D3D12 cannot satisfy is
//! [`InvalidDescriptor`](crcbl_hal::HalError::InvalidDescriptor) saying which
//! field and why.
//!
//! # A D3D12 command list retains nothing, so this crate does
//!
//! `crcbl_dx12::command`'s encoder takes its own reference to every resource it
//! records against, and a submission parks that set on `crcbl_dx12::retire`'s
//! fence-keyed queue along with the command list and allocator — which
//! `ExecuteCommandLists` does not retain either. That is what makes
//! `destroy_buffer` mean "this handle is dead now" without freeing memory the
//! driver is still reading, and it is why every `destroy_*` still releases on
//! the spot. `crcbl-vk` needs a larger mechanism for the same guarantee because
//! a `VkBuffer` has no refcount; `crcbl-mtl` needs none, because an
//! `MTLCommandBuffer` retains what it references.
//!
//! **One capability of the seam is refused rather than implemented**, and it is
//! called out here because it is a divergence from `crcbl-mtl` rather than a
//! missing slice: an [`ImageViewDesc`](crcbl_hal::ImageViewDesc) whose `format`
//! differs from its image's — the sRGB reinterpretation the seam documents — is
//! an error on this backend. D3D12 allows the cast only from a *typeless*
//! resource, or where an optional casting capability is reported, and
//! `crcbl_dx12::device`'s `create_image_view` argues why neither is the right
//! trade for this slice.
//!
//! The question is `docs/backlog.md`'s **"Does WARP clear Tier A?"**. Every
//! software-rasteriser job in `.github/workflows/ci.yml` is
//! `ubuntu-latest`/lavapipe; `windows-latest` has no GPU device at all, which is
//! why Windows has no golden images and no render coverage. WARP is D3D12's
//! software rasteriser and ships in Windows, so if it supports **SM6.6 dynamic
//! resources** — the model this backend is specced around — then DX12 buys
//! Windows the equivalent of lavapipe. If it does not, `crcbl-wgpu` already
//! covers the portable path there and the CI half of DX12's justification
//! collapses to Xbox plus tooling.
//!
//! So enumeration asks DXGI for WARP **by name**, beside whatever hardware is
//! present, and this crate's tests publish what each adapter actually answered:
//! its `ResourceBindingTier`, its `HighestShaderModel`, whether the two together
//! clear SM6.6 dynamic resources, and the
//! [`GeometryPath`](crcbl_hal::GeometryPath),
//! [`BindingModel`](crcbl_hal::BindingModel) and
//! [`LightingPath`](crcbl_hal::LightingPath) those features select. See
//! `crcbl_dx12::instance`'s tests for the report line and how to read it out of
//! a run.
//!
//! **Reporting tier 3 is a claim about the API surface, not about execution.**
//! That half is now measured too:
//! `crcbl_dx12::device`'s `a_d3d12_load_op_preserves_what_clear_replaces`
//! clears an attachment through a real render pass, copies it into a readback
//! buffer, submits, and asserts the texels. If a runner enumerates a
//! capable-looking adapter that cannot execute anything, that test is where it
//! shows — and it panics naming the stage it reached (`finish`, `submit`,
//! `wait_idle`, or a readback still pending after its deadline) rather than
//! running into `slow-timeout` with nothing in the log. The Metal backend found
//! out the hard way that GitHub's `macos-latest` exposes an
//! `Apple Paravirtual device` which hangs the command buffer on any draw while
//! both encoders report `completed`; the Windows runner is owed the same
//! suspicion, and this is how it is paid.
//!
//! **The answer still comes from a machine, not from this crate.** Nothing here
//! has ever executed on the development box, which is Linux.
//!
//! `tests/run-dx12-e2e.sh` is how that machine is asked. It pins
//! `CRCBL_DX12_ADAPTER=warp` so every device this crate's tests open is WARP's
//! rather than whichever adapter DXGI listed first, checks the pin landed off
//! the suite's own output, and fails a run that tested nothing — the three
//! things a plain `cargo nextest run -p crcbl-dx12` cannot tell you. `crcbl-vk`
//! pins lavapipe through `CRCBL_VK_ICD` for the same reason.
//!
//! # A removed device names its reason, and the debug layer names the call
//!
//! `DXGI_ERROR_DEVICE_REMOVED` arrives at the **next** call rather than at the
//! one that caused it, so the code on its own says only that something earlier
//! went wrong. Every failure this backend reports out of a live device now
//! carries `GetDeviceRemovedReason`'s answer where there is one — the call
//! D3D12's own error text tells you to make — plus whatever the debug layer has
//! stored for that device. `crcbl_dx12::debug` is where both come from.
//!
//! The layer itself is off in a release build, on in a debug one, and
//! `CRCBL_DX12_VALIDATION` overrides both directions — the shape
//! `CRCBL_VK_VALIDATION` already has in `crcbl-vk`. It needs Windows' **Graphics
//! Tools** optional feature; without it, `enable_debug_layer` warns and the
//! removal reason still answers, because that one is a vtable call on a device
//! this crate already holds and needs nothing installed.
//!
//! # Every adapter now derives `IndirectCount`, and that is *this backend* speaking
//!
//! [`GeometryPath`](crcbl_hal::GeometryPath) is derived from
//! [`Features`](crcbl_hal::Features) precisely so a backend cannot claim a path
//! it has not earned. The derived path is
//! [`IndirectCount`](crcbl_hal::GeometryPath::IndirectCount) for every adapter
//! since the draw slice: `ExecuteIndirect` takes a **count buffer** as an
//! ordinary parameter, so
//! [`DRAW_INDIRECT_COUNT`](crcbl_hal::Features::DRAW_INDIRECT_COUNT) has a call
//! behind it here where `crcbl-mtl` had to withdraw the same flag — Metal has no
//! count-from-memory execution at all. It is not
//! [`MeshShader`](crcbl_hal::GeometryPath::MeshShader) because
//! `create_mesh_pipeline` and `DispatchMesh` are still unwritten, which is a gap
//! in this crate rather than in any adapter.
//!
//! **Nothing in [`GPU_DRIVEN`](crcbl_hal::Features::GPU_DRIVEN) is waiting on a
//! call any more.** [`COMPUTE`](crcbl_hal::Features::COMPUTE),
//! [`MULTI_DRAW_INDIRECT`](crcbl_hal::Features::MULTI_DRAW_INDIRECT),
//! `DRAW_INDIRECT_COUNT`,
//! [`INDIRECT_FIRST_INSTANCE`](crcbl_hal::Features::INDIRECT_FIRST_INSTANCE)
//! and finally
//! [`TIMELINE_SEMAPHORE`](crcbl_hal::Features::TIMELINE_SEMAPHORE) each left
//! that list on the slice that made its calls and read the result back in this
//! crate's own tests — the last of them when `Device::create_semaphore` began
//! handing an `ID3D12Fence` out and `Device::submit` began consuming it through
//! `ID3D12CommandQueue::Wait` and `Signal`. So
//! [`DeviceDesc::for_adapter`](crcbl_hal::DeviceDesc::for_adapter) opens a
//! device here rather than reporting a gap, and what is left between an adapter
//! and the whole bundle is
//! [`DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING) — which is
//! the WARP question itself.
//!
//! **Read the WARP verdict off the SM6.6 line, never off the selected path.**
//! The path is a statement about how much of this backend is written; the
//! dynamic-resources answer is the statement about the adapter, and it is the
//! one the backlog asked for.
//!
//! # `DESCRIPTOR_INDEXING` is reported ahead of the call behind it, on purpose
//!
//! `crcbl-mtl`'s rule is that a backend reports a feature once a call in the
//! crate makes it true, and it withdrew
//! [`DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING) when its
//! bind groups turned out not to deliver one. This backend reports it anyway,
//! and the reason it is not the same mistake is that **no caller can act on it
//! here**: `create_bind_group_layout` and every pipeline entry point refuse, so
//! there is no layout and no pipeline to be misled. It is an adapter-level fact
//! read from real queries, and answering the backlog's question requires
//! reporting it.
//!
//! What *would* be the mistake is keeping it past the slice that discovers
//! whether this backend's bind groups can deliver a runtime-sized array.
//! `crcbl_dx12::adapter` says so on the flag itself, so the withdrawal is a
//! decision someone has already been warned about rather than a surprise.
//!
//! The other reported flags do have a call behind them:
//! [`BUFFER_DEVICE_ADDRESS`](crcbl_hal::Features::BUFFER_DEVICE_ADDRESS) is not
//! optional in D3D12 and has no query to make,
//! [`TEXTURE_COMPRESSION_BC`](crcbl_hal::Features::TEXTURE_COMPRESSION_BC) is
//! measured per format and gates `create_image`,
//! [`SAMPLER_ANISOTROPY`](crcbl_hal::Features::SAMPLER_ANISOTROPY) arrived with
//! `create_sampler`, [`COMPUTE`](crcbl_hal::Features::COMPUTE) arrived with
//! `create_compute_pipeline` and `dispatch`, the three indirect flags
//! arrived with the `ExecuteIndirect` draws — see `crcbl_dx12::draw` — and the
//! three query flags arrived with `CreateQueryHeap` and the reads behind it.
//!
//! **The query flags are also the one place an adapter and a device from it
//! report different caps.**
//! [`TIMESTAMP_QUERY`](crcbl_hal::Features::TIMESTAMP_QUERY) obliges a
//! [`Limits::timestamp_period_ns`](crcbl_hal::Limits::timestamp_period_ns), and
//! the period is `1e9 / ID3D12CommandQueue::GetTimestampFrequency()` — a queue
//! call, with no queue in sight when `crcbl_dx12::adapter` fills its caps in. So
//! the *flag* is reported at the adapter, because
//! [`DeviceDesc::required_features`](crcbl_hal::DeviceDesc::required_features)
//! is checked against the adapter and a flag that first appeared at device open
//! could never be required, and the *number* is filled in by `Dx12Device::open`
//! once its queue exists.
//!
//! # `Dx12Instance` exists only on Windows, and is unlinked here on purpose
//!
//! The whole backend is behind `#[cfg(target_os = "windows")]`: D3D12 is a
//! Windows component and `windows` is pinned to `cfg(target_os = "windows")` in
//! this crate's manifest, so on Linux, macOS and `wasm32` this crate is these
//! docs and no code at all. That is the same shape `crcbl-mtl` takes off macOS
//! and `crcbl_jobs::Threads` takes on `wasm32`, and it is written the same way:
//! the type is named in backticks rather than linked, because a link to it is
//! unresolvable in exactly the builds that do not have it, and rustdoc is a CI
//! gate in this workspace.
//!
//! No `#[cfg(target_os = …)]` appears *above* the seam as a result —
//! `crcbl-hal`'s rule — because the absence is expressed by the crate having no
//! public items, not by a caller testing the platform.
//!
//! # No `unsafe` marker impls anywhere in this crate
//!
//! [`Instance`](crcbl_hal::Instance) and [`Device`](crcbl_hal::Device) both
//! require [`HalThreadSafe`](crcbl_hal::threading::HalThreadSafe), which is
//! `Send + Sync` on native, and neither is satisfied here by an assertion.
//! `windows-rs` declares `unsafe impl Send` **and** `Sync` for every interface
//! this crate holds — `IDXGIFactory4`, `IDXGIAdapter1`, `IDXGISwapChain3`,
//! `ID3D12Device`, `ID3D12CommandQueue`, `ID3D12Resource`,
//! `ID3D12DescriptorHeap` and `ID3D12Fence` — so the markers come from the
//! compiler.
//!
//! That is worth stating because `crcbl-mtl` could not do it: `MTLBuffer` and
//! `MTLTexture` inherit from `MTLResource`, which objc2 leaves unmarked, so its
//! device slice had to write the impl and justify it. The Win32 types here that
//! are **not** `Send` are all raw pointers — `HANDLE` and `HWND` — and none of
//! them is stored as one:
//!
//! * The event a fence wait is armed with never leaves `Device::wait_idle`,
//!   which creates and closes it inside the call. That is the second reason for
//!   a decision the first reason (two concurrent waiters must not share an
//!   auto-reset event) already forced.
//! * A surface's window and a swapchain's frame-latency waitable object are
//!   both kept as plain addresses and rebuilt at each call site, because both
//!   live in a pool behind a lock inside a struct the seam requires to be
//!   `Send + Sync`. An `HWND` is not a pointer this crate may dereference in
//!   any case — only Win32 may — so the integer loses nothing it was entitled
//!   to use.
//!
//! The factory and the adapters **are** kept, behind an `Arc` shared with every
//! device: `D3D12CreateDevice` takes an `IDXGIAdapter1`, and the seam obliges a
//! `Device` to outlive its `Instance`. That obligation is discharged by
//! construction rather than by a rule someone has to remember, exactly as
//! `crcbl-mtl` discharged it.

#[cfg(target_os = "windows")]
mod adapter;
#[cfg(target_os = "windows")]
mod binding;
// What D3D12 requires of a buffer's allocation and of the views over it. Not
// Windows-only for the reason `present` below is not: it holds no `windows`
// type, so off Windows it exists in the test build alone and `cargo test` on any
// host checks the padding, alignment and bounds arithmetic that a `Create*View`
// call — which returns `void` and removes the device at the *next* call — can
// never report.
#[cfg(any(target_os = "windows", test))]
mod buffer;
#[cfg(target_os = "windows")]
mod command;
#[cfg(target_os = "windows")]
mod conv;
// The debug layer, the info queue behind it, and `GetDeviceRemovedReason`.
// Not Windows-only for the reason `present` below is not: the policy deciding
// whether the layer is on holds no `windows` type, so off Windows it exists in
// the test build alone and `cargo test` on any host checks the one part of this
// module a reader can get wrong without a device.
#[cfg(any(target_os = "windows", test))]
mod debug;
#[cfg(target_os = "windows")]
mod descriptor;
#[cfg(target_os = "windows")]
mod device;
// The index-buffer and indirect-argument arithmetic, for the same reason
// `present` below is not Windows-only: it holds no `windows` type and no seam
// handle, so off Windows it exists in the test build alone and `cargo test` on
// any host checks the offsets, strides and bounds a draw's output could never
// reveal.
#[cfg(any(target_os = "windows", test))]
mod draw;
// The DXIL container, parsed as bytes. Not Windows-only for the reason
// `present` below is not: it holds no `windows` type, and off Windows it exists
// in the test build alone so that `cargo test` on any host checks what this
// crate believes about the artifacts it consumes.
#[cfg(any(target_os = "windows", test))]
mod dxil;
#[cfg(target_os = "windows")]
mod handle;
#[cfg(target_os = "windows")]
mod instance;
// Which adapter this crate's device tests open, and the environment variable
// `tests/run-dx12-e2e.sh` pins it with. Test-only — the seam publishes every
// adapter and the caller chooses, so nothing above it needs this — and
// deliberately not Windows-only, because it holds no `windows` type and is the
// one part of the harness that can be exercised on the Linux box this backend
// is written on.
#[cfg(test)]
mod pin;
#[cfg(target_os = "windows")]
mod pipeline;
// The one module that is not Windows-only, and the crate docs say why: it holds
// no `windows` type, and off Windows it exists in the test build alone so that
// `cargo test` on any host runs the swapchain and present-wait arithmetic.
#[cfg(any(target_os = "windows", test))]
mod present;
// What a query result occupies and where one lands in a buffer. Not
// Windows-only for the reason `present` above is not — it holds no `windows`
// type — and that matters here because D3D12 chooses the resolve stride and
// takes no parameter to narrow it: a destination sized at the seam's one `u64`
// per query is a buffer overrun on a statistics set, and the runtime reports
// nothing.
#[cfg(any(target_os = "windows", test))]
mod query;
#[cfg(target_os = "windows")]
mod retire;
// Where every binding of a pipeline layout lands among the root parameters,
// what the signature costs, and how a dynamic offset reaches a root descriptor.
// Not Windows-only for the reason `present` below is not — it holds no `windows`
// type — and that matters more here than anywhere else in this crate: nothing in
// D3D12 reports a root parameter index that disagrees with the one the signature
// was built with.
#[cfg(any(target_os = "windows", test))]
mod root;
#[cfg(target_os = "windows")]
mod swapchain;
// What value a submission's semaphore wait and signal carry. Not Windows-only
// for the reason `present` above is not — it holds no `windows` type — and that
// matters here for the same reason it does in `root`: a fence signalled with a
// value it has already been given does not fail, it hangs whatever was waiting
// past it, and no D3D12 run reports that.
#[cfg(any(target_os = "windows", test))]
mod sync;
#[cfg(target_os = "windows")]
mod validate;
#[cfg(target_os = "windows")]
mod view;

#[cfg(target_os = "windows")]
pub use instance::Dx12Instance;
