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
//! # What this slice is, and the question it was written to answer
//!
//! **Adapter enumeration, and nothing else.** `Dx12Instance` lists every D3D12
//! adapter with its capabilities filled in, which is the one thing
//! [`Instance::adapters`](crcbl_hal::Instance::adapters) promises can be
//! answered before a device exists. Every other entry point refuses with
//! [`HalError::Unsupported`](crcbl_hal::HalError::Unsupported) whose `what`
//! names the slice the answer arrives in, so a caller reads "not yet" rather
//! than "broken". An out-of-range adapter still gets
//! [`NoSuchAdapter`](crcbl_hal::HalError::NoSuchAdapter): that is a caller bug
//! this slice can genuinely diagnose, and folding it into the refusal would lose
//! it.
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
//! # Every adapter is Tier B in this slice, and that is *this backend* speaking
//!
//! [`DeviceCaps::tier`](crcbl_hal::DeviceCaps::tier) is derived from
//! [`Features`](crcbl_hal::Features) precisely so a backend cannot claim a tier
//! it has not earned, and this slice earns two of the six Tier A flags. So the
//! derived tier is **B for every adapter, including a Tier-A-capable GPU** —
//! because [`COMPUTE`](crcbl_hal::Features::COMPUTE),
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
//! # Two flags are reported with no call behind them, on purpose
//!
//! `crcbl-mtl`'s rule is that a backend reports a feature once a call in the
//! crate makes it true, and it withdrew
//! [`DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING) when its
//! bind groups turned out not to deliver one. This slice reports it anyway,
//! together with
//! [`BUFFER_DEVICE_ADDRESS`](crcbl_hal::Features::BUFFER_DEVICE_ADDRESS), and
//! the reason it is not the same mistake is that **no caller can act on either
//! one here**: `request_device` refuses unconditionally, so there is no device,
//! no bind group layout and no buffer to be misled about. Both are adapter-level
//! facts read from real queries, and answering the backlog's question requires
//! reporting the first of them.
//!
//! What *would* be the mistake is keeping `DESCRIPTOR_INDEXING` past the slice
//! that discovers whether this backend's bind groups can deliver a runtime-sized
//! array. `crcbl_dx12::adapter` says so on the flag itself, so the withdrawal is
//! a decision someone has already been warned about rather than a surprise.
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
//! # No `unsafe` marker impls, and no COM object outlives enumeration
//!
//! [`Instance`](crcbl_hal::Instance) requires
//! [`HalThreadSafe`](crcbl_hal::threading::HalThreadSafe), which is
//! `Send + Sync` on native. `Dx12Instance` holds owned
//! [`AdapterInfo`](crcbl_hal::AdapterInfo) and the raw D3D12 answers it was
//! derived from — plain data, no COM pointers — so the markers come from the
//! compiler rather than from an assertion written here.
//!
//! That is a decision about *this* slice, not a claim about `windows-rs`: the
//! bindings do declare `unsafe impl Send`/`Sync` for `IDXGIFactory4`,
//! `IDXGIAdapter1` and `ID3D12Device`, so the device slice can keep them without
//! writing an `unsafe` impl either. What it will have to keep is the factory and
//! the adapters, because `D3D12CreateDevice` takes an `IDXGIAdapter1` — this
//! slice drops them because nothing needs them after the capability read, which
//! is the same call `crcbl-mtl`'s first slice made about its `MTLDevice`
//! objects.

#[cfg(target_os = "windows")]
mod adapter;
#[cfg(target_os = "windows")]
mod instance;

#[cfg(target_os = "windows")]
pub use instance::Dx12Instance;
