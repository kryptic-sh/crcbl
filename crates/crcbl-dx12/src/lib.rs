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
//! **Adapter enumeration, and the resource half of the seam.**
//! `Dx12Instance` lists every D3D12 adapter with its capabilities filled in,
//! which is the one thing [`Instance::adapters`](crcbl_hal::Instance::adapters)
//! promises can be answered before a device exists. `request_device` then opens
//! a real `ID3D12Device` and a `D3D12_COMMAND_LIST_TYPE_DIRECT` queue, and the
//! device it hands back creates buffers, images, image views and samplers,
//! writes into a host-visible buffer, and waits for the GPU on a fence.
//!
//! Everything past that — shader modules, pipelines, bind groups, command
//! recording, submission, swapchains, queries and readback — refuses with
//! [`HalError::Unsupported`](crcbl_hal::HalError::Unsupported) whose `what`
//! names the slice the answer arrives in, so a caller reads "not yet" rather
//! than "broken". What is **not** folded into that refusal is anything the
//! backend can genuinely diagnose: an out-of-range adapter is
//! [`NoSuchAdapter`](crcbl_hal::HalError::NoSuchAdapter), a stale handle is
//! [`InvalidHandle`](crcbl_hal::HalError::InvalidHandle), one from another
//! device is [`ForeignObject`](crcbl_hal::HalError::ForeignObject), and a
//! descriptor D3D12 cannot satisfy is
//! [`InvalidDescriptor`](crcbl_hal::HalError::InvalidDescriptor) saying which
//! field and why.
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
//! covers Tier B there and the CI half of DX12's justification collapses to Xbox
//! plus tooling.
//!
//! So enumeration asks DXGI for WARP **by name**, beside whatever hardware is
//! present, and this crate's tests publish what each adapter actually answered:
//! its `ResourceBindingTier`, its `HighestShaderModel`, whether the two together
//! clear SM6.6 dynamic resources, and its derived
//! [`RendererTier`](crcbl_hal::RendererTier). See `crcbl_dx12::instance`'s tests
//! for the report line and how to read it out of a run.
//!
//! **The answer comes from a machine, not from this crate.** Nothing here has
//! ever executed on the development box, which is Linux; the Metal backend found
//! out the hard way that GitHub's `macos-latest` exposes an
//! `Apple Paravirtual device` that cannot execute a shader at all, and the
//! Windows runner is owed the same suspicion.
//!
//! # Every adapter is Tier B so far, and that is *this backend* speaking
//!
//! [`DeviceCaps::tier`](crcbl_hal::DeviceCaps::tier) is derived from
//! [`Features`](crcbl_hal::Features) precisely so a backend cannot claim a tier
//! it has not earned, and the flags it has not earned are still the majority of
//! the Tier A set. So the derived tier is **B for every adapter, including a
//! Tier-A-capable GPU** — because [`COMPUTE`](crcbl_hal::Features::COMPUTE),
//! [`TIMELINE_SEMAPHORE`](crcbl_hal::Features::TIMELINE_SEMAPHORE),
//! [`MULTI_DRAW_INDIRECT`](crcbl_hal::Features::MULTI_DRAW_INDIRECT) and
//! [`DRAW_INDIRECT_COUNT`](crcbl_hal::Features::DRAW_INDIRECT_COUNT) all wait on
//! calls this crate does not make yet, not because any adapter lacks them.
//!
//! **Read the WARP verdict off the SM6.6 line, never off the tier.** The tier is
//! a statement about how much of this backend is written; the dynamic-resources
//! answer is the statement about the adapter, and it is the one the backlog
//! asked for.
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
//! The other three reported flags do have a call behind them:
//! [`BUFFER_DEVICE_ADDRESS`](crcbl_hal::Features::BUFFER_DEVICE_ADDRESS) is not
//! optional in D3D12 and has no query to make,
//! [`TEXTURE_COMPRESSION_BC`](crcbl_hal::Features::TEXTURE_COMPRESSION_BC) is
//! measured per format and gates `create_image`, and
//! [`SAMPLER_ANISOTROPY`](crcbl_hal::Features::SAMPLER_ANISOTROPY) arrived with
//! `create_sampler`.
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
//! this crate holds — `IDXGIFactory4`, `IDXGIAdapter1`, `ID3D12Device`,
//! `ID3D12CommandQueue`, `ID3D12Resource`, `ID3D12DescriptorHeap` and
//! `ID3D12Fence` — so the markers come from the compiler.
//!
//! That is worth stating because `crcbl-mtl` could not do it: `MTLBuffer` and
//! `MTLTexture` inherit from `MTLResource`, which objc2 leaves unmarked, so its
//! device slice had to write the impl and justify it. The one Win32 type here
//! that is not `Send` is the event `HANDLE` a fence wait is armed with, and
//! `Device::wait_idle` creates and closes it inside the call rather than storing
//! it — which is the second reason for a decision the first reason (two
//! concurrent waiters must not share an auto-reset event) already forced.
//!
//! The factory and the adapters **are** kept, behind an `Arc` shared with every
//! device: `D3D12CreateDevice` takes an `IDXGIAdapter1`, and the seam obliges a
//! `Device` to outlive its `Instance`. That obligation is discharged by
//! construction rather than by a rule someone has to remember, exactly as
//! `crcbl-mtl` discharged it.

#[cfg(target_os = "windows")]
mod adapter;
#[cfg(target_os = "windows")]
mod command;
#[cfg(target_os = "windows")]
mod conv;
#[cfg(target_os = "windows")]
mod descriptor;
#[cfg(target_os = "windows")]
mod device;
#[cfg(target_os = "windows")]
mod handle;
#[cfg(target_os = "windows")]
mod instance;
#[cfg(target_os = "windows")]
mod view;

#[cfg(target_os = "windows")]
pub use instance::Dx12Instance;
