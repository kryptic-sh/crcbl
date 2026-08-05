//! `crcbl-mtl` — the engine's Metal backend, and macOS's only path to a GPU.
//!
//! `docs/plan/09-backends-metal-dx12.md` puts Metal first of the two P14
//! backends because it is the mandatory one — there is no Vulkan on macOS
//! without MoltenVK — and because the API distance from the Vulkan-flavoured
//! seam is the larger of the two, so it is the backend that finds the seam's
//! leaks.
//!
//! # What this slice is
//!
//! **Adapter enumeration, and nothing else.** `MetalInstance` implements
//! [`crcbl_hal::Instance`] far enough to list every Metal device on the machine
//! with its capabilities filled in, which is the one thing
//! [`Instance::adapters`](crcbl_hal::Instance::adapters) promises can be
//! answered *before* a device exists. Every other entry point —
//! [`create_surface`](crcbl_hal::Instance::create_surface),
//! [`destroy_surface`](crcbl_hal::Instance::destroy_surface),
//! [`surface_caps`](crcbl_hal::Instance::surface_caps),
//! [`request_device`](crcbl_hal::Instance::request_device) — refuses with
//! [`HalError::Unsupported`](crcbl_hal::HalError::Unsupported) whose `what`
//! names the slice it arrives in, so a caller reads "not yet" rather than
//! "broken". Nothing here creates a `MTLCommandQueue`, a `CAMetalLayer` or a
//! resource of any kind.
//!
//! # `MetalInstance` exists only on macOS, and is unlinked here on purpose
//!
//! The whole backend is behind `#[cfg(target_os = "macos")]`: Metal is an Apple
//! framework and `objc2-metal` is pinned to `cfg(target_os = "macos")` in this
//! crate's manifest, so on Linux, Windows and `wasm32` this crate is these docs
//! and no code at all. That is the same shape `crcbl_jobs::Threads` takes on
//! `wasm32`, and it is written the same way: the type is named in backticks
//! rather than linked, because a link to it is unresolvable in exactly the
//! builds that do not have it, and rustdoc is a CI gate in this workspace.
//!
//! No `#[cfg(target_os = …)]` appears *above* the seam as a result of any of
//! this — `crcbl-hal`'s rule — because the absence is expressed by the crate
//! having no public items, not by a caller testing the platform.
//!
//! # Decision: the instance holds owned adapter data, not `MTLDevice`
//!
//! `MTLCopyAllDevices` is called once, in `MetalInstance::open`, and every
//! answer the seam wants is read off each device there and turned into an
//! [`AdapterInfo`](crcbl_hal::AdapterInfo). The
//! `Retained<ProtocolObject<dyn MTLDevice>>` values are dropped before `open`
//! returns.
//!
//! This is not a workaround for a thread-safety problem — see below — it is
//! that nothing in this slice reads a device a second time. `request_device`
//! refuses, so a stored device would be a field no code touches, kept for a
//! caller that has not arrived: the shape this workspace deletes rather than
//! keeps. The device slice will store them, and it will not need a marker impl
//! to do it.
//!
//! # The `Send + Sync` question, and why there is no `unsafe` in this crate
//!
//! [`Instance`](crcbl_hal::Instance) requires
//! [`HalThreadSafe`](crcbl_hal::threading::HalThreadSafe), which is
//! `Send + Sync` on native, and the obvious worry is that an Objective-C
//! `Retained` pointer is neither.
//!
//! It is both, and `objc2-metal` says so in the binding rather than leaving
//! each user to assert it: `MTLDevice` is declared
//! `pub unsafe trait MTLDevice: NSObjectProtocol + Send + Sync`, so
//! `ProtocolObject<dyn MTLDevice>` picks the two markers up through
//! `unsafe impl<P: ?Sized + Send> Send for ProtocolObject<P>`, and `Retained<T>`
//! through `unsafe impl<T: ?Sized + Sync + Send> Send for Retained<T>`. Apple
//! documents `MTLDevice` as thread-safe, and those impls are that promise
//! encoded once, upstream.
//!
//! So the newtype-plus-`unsafe impl` this crate was expected to need does not
//! exist: `MetalInstance` holds a `Vec<AdapterInfo>` of plain owned data, which
//! is `Send + Sync` for the ordinary reason. **This crate asserts nothing about
//! thread safety and contains no `unsafe` code outside its tests** — the one
//! `unsafe` block in it is a test *calling* the seam's `unsafe fn
//! create_surface`. A later slice that stores devices inherits the same
//! upstream impls and still needs no marker of its own.
//!
//! # Tier: this slice reports **Tier B**, and that is not a claim about Metal
//!
//! `docs/plan/09-backends-metal-dx12.md` specs this backend as Tier A, and the
//! hardware supports it. [`DeviceCaps::tier`](crcbl_hal::DeviceCaps::tier) is
//! *derived* from [`Features`](crcbl_hal::Features) precisely so that a backend
//! cannot assert a tier it has not earned, and two of the six Tier A features —
//! [`DRAW_INDIRECT_COUNT`](crcbl_hal::Features::DRAW_INDIRECT_COUNT) and
//! [`MULTI_DRAW_INDIRECT`](crcbl_hal::Features::MULTI_DRAW_INDIRECT) — depend on
//! which indirect path this backend takes (indirect command buffers, per the
//! plan's mapping table), a decision the command slice makes. Until then they
//! are off, so the derived tier is B.
//!
//! The two Tier A features that *are* adapter questions —
//! [`DESCRIPTOR_INDEXING`](crcbl_hal::Features::DESCRIPTOR_INDEXING) and
//! [`BUFFER_DEVICE_ADDRESS`](crcbl_hal::Features::BUFFER_DEVICE_ADDRESS) — are
//! reported from real queries today. The full list, with a reason against every
//! flag that is absent, is in this crate's `adapter` module.

#[cfg(target_os = "macos")]
mod adapter;
#[cfg(target_os = "macos")]
mod instance;

#[cfg(target_os = "macos")]
pub use instance::MetalInstance;
