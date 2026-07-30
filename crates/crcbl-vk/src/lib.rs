//! Vulkan implementation of the [`crcbl-hal`](crcbl_hal) seam.
//!
//! ```text
//! VkInstance::open() ──▶ adapters() ──▶ create_device() ──▶ Box<dyn Device>
//!        │                                                       │
//!        └── create_surface(SurfaceTarget) ──▶ SurfaceHandle ──▶ swapchain
//! ```
//!
//! Nothing above the seam names a type from this crate: the sandbox and the
//! renderer hold `Box<dyn Instance>` and `Box<dyn Device>`, and the only place
//! `crcbl-vk` is spelled at all is the backend registry in the `crcbl` umbrella
//! — exactly as `crcbl-shell`'s registry is the only place `WaylandShell` is
//! spelled. `ash` is a dependency of this crate and of no other, per
//! `docs/plan/01-foundations.md` §1.3.
//!
//! # Decision: the Vulkan loader is `dlopen`ed, not linked
//!
//! `ash::Entry::load` resolves `libvulkan.so.1` (or `vulkan-1.dll`, or
//! `libvulkan.dylib`) at runtime. The alternative, ash's `linked` feature, puts
//! a `DT_NEEDED` on it and lets the dynamic linker resolve it at process start.
//! This crate uses **`load`**, and the argument is the one `crcbl-shell`
//! already made for `libwayland-client.so.0` and `libxcb.so.1`:
//!
//! 1. **A backend registry is a fall-through list, so every entry must be able
//!    to fail.** The `crcbl` umbrella tries Vulkan and offers the null backend
//!    by name. A crate linked against the Vulkan loader cannot fail that way:
//!    on a machine without it the process dies in `ld.so` **before `main`**, so
//!    the list never runs and the error message is the dynamic linker's, not
//!    the engine's. [`OpenError::NoLoader`] is what a missing loader should
//!    look like.
//! 2. **The engine is one binary and the CI matrix is not one machine.** The
//!    same `sandbox` binary runs on a developer's radv box, on a CI runner with
//!    lavapipe and nothing else, and — per `docs/plan/11-cli-headless.md` — in
//!    headless jobs that may have no graphics stack at all. `--headless
//!    --backend null` has to work on the last of those, and on macOS and
//!    Windows, where this crate compiles and finds no loader at all.
//! 3. **It costs one `dlopen`.** Loader dispatch is unchanged either way; only
//!    who calls `dlopen` differs, and doing it ourselves is what buys the error
//!    path.
//!
//! The cost is real and worth stating: `Entry::load` is `unsafe`, because a
//! mismatched `libvulkan.so.1` on the search path would export the right
//! symbols with the wrong signatures. That is the same trust the process
//! already extends to every system library it resolves, and it is contained in
//! exactly one call.
//!
//! # Validation is a test result, not a log line
//!
//! Debug builds enable `VK_LAYER_KHRONOS_validation` with the debug-utils
//! messenger routed into `log`, **and** into a counted sink.
//! [`VkInstance::validation_report`] snapshots it and
//! [`ValidationReport::assert_clean`] fails a test on any error or warning —
//! including the case where the layer was never loaded, which is the failure
//! mode a "zero validation errors" criterion silently passes on otherwise. See
//! [`debug`].
//!
//! # Scope: the whole milestone ladder
//!
//! `docs/plan/02-vulkan-backend.md`'s ladder is (1) clear colour through the
//! graph, (2) triangle, (3) depth-tested mesh, (4) forward lit, (5) ortho. As of
//! P1.3 this crate carries **all five**: device and queues, surfaces and
//! swapchains (including the offscreen ring), command recording with `sync2`
//! barriers and dynamic rendering, timeline-semaphore submission, buffers with
//! readback, image views, depth attachments, timestamp queries, the deletion
//! queue behind `destroy_*`, shader modules, descriptor layouts, bind groups,
//! samplers and graphics/compute pipelines.
//!
//! Rungs 3–5 are not implemented *here*, and that is the architecture rather
//! than a gap: the render graph, the camera and the forward pass live in
//! `crcbl-render`, above the seam, exactly as §2.4 requires. This crate's job at
//! P1.3 was to have nothing left that the graph needed and could not get — and
//! it did not, which is the most useful thing that can be said about a backend
//! at the end of a stage.
//!
//! Milestone 2 is a triangle whose vertices are **pulled from a storage
//! buffer**; milestone 3's cube adds an index buffer and pulls the same way.
//! `pipeline.rs` never fills in a vertex-input state, because the seam has none
//! to fill in.
//!
//! The one thing that does *not* go through the deletion queue is a swapchain:
//! the queue is keyed on the submission timeline, and `vkQueuePresentKHR` is
//! queue work no timeline semaphore signals, so a swapchain is retired after an
//! explicit device idle instead; `device::DeviceInner::retire_swapchain` is
//! where that is spelled out.
//!
//! What is still absent is a pipeline cache.
//! [`HalError::Unsupported`](crcbl_hal::HalError::Unsupported) only ever names a
//! genuine *device* limitation, never a phase.
//!
//! Shaders are authored in **Slang** and their SPIR-V is committed; see
//! `crcbl-shaders` for that decision and for the three checks that stop the
//! artifacts rotting. Nothing in this crate depends on that one — a backend
//! takes `&[u32]`.
//!
//! # What this backend changed in the seam
//!
//! The seam is **provisional** until P5 exit, and a backend meeting it for the
//! first time is supposed to report back. Two of P1.1's findings were fixed in
//! the seam rather than worked around here:
//!
//! * **The configured swapchain extent was unobservable.** `SwapchainDesc`
//!   names the extent the caller *wants*, and Vulkan may legally refuse it: on
//!   X11 `minImageExtent == maxImageExtent == currentExtent`, so a shell resize
//!   event one frame behind has *no* legal swapchain at its size. This backend
//!   must clamp, and there was no way to say what it clamped to — a caller
//!   could render a frame at a size the swapchain did not have.
//!   [`AcquiredFrame::extent`](crcbl_hal::AcquiredFrame::extent) now carries
//!   it, and the extent obligations are reworded around request-versus-answer.
//! * **A render pass needs an `ImageViewHandle`, and acquire yielded only an
//!   `ImageHandle`**, so every caller built the identical per-image view cache.
//!   [`AcquiredFrame::view`](crcbl_hal::AcquiredFrame::view) now sits beside
//!   the semaphores, owned and reissued by the swapchain, for exactly the
//!   reason they do.
//!
//! P1.2 added four more, three now closed and one still open:
//!
//! * **`bind_group` and `push_constants` gained explicit `layout` parameters
//!   on 2026-07-31.** The seam now carries `PipelineLayoutHandle` on both
//!   commands, ending the implicit pipeline-before-bind-group ordering rule
//!   and the inference from last-bound-pipeline.
//! * **`BindGroupDesc` gained a `variable_count` field on 2026-07-31.**
//!   The group now provides the actual array length explicitly rather than
//!   having the backend infer it from entries — which is what the bindless
//!   model (allocate N, write a subset, stream the rest through
//!   `update_bind_group` later) needs. See `pipeline::variable_count_from_entries`
//!   for the fallback inference path.
//! * **Vertex pulling needs `shaderDrawParameters`.** `SV_VertexID` lowers to
//!   `gl_VertexIndex - gl_BaseVertex`, which declares SPIR-V's
//!   `DrawParameters` capability. It is Vulkan 1.1 core-optional and present on
//!   radv and lavapipe, so it joins this backend's floor rather than becoming a
//!   `Features` bit the renderer would branch on — but it *is* a requirement
//!   the seam's vocabulary cannot express.
//! * **Synchronisation validation had never run.**
//!   `VK_EXT_validation_features` is a *layer* extension, and the probe only
//!   enumerated the loader's implicit list, so `CRCBL_VK_SYNC_VALIDATION=1`
//!   bought a log line and no checking — here and in CI. Fixed in `instance.rs`,
//!   and `tests/vk_e2e.rs` now provokes a deliberate read-after-write so the
//!   mitigation cannot go quiet again.
//!
//! P1.3 drove the seam from *above* for the first time — `crcbl-render`'s graph
//! is the first consumer that is not a hand-written frame — and added two
//! observations. Neither was worth a seam change, and both are worth writing
//! down:
//!
//! * **`DeviceDesc::for_adapter` opens a device that cannot be profiled.**
//!   [`Features::TIMESTAMP_QUERY`](crcbl_hal::Features::TIMESTAMP_QUERY) is
//!   deliberately not part of
//!   [`TIER_A`](crcbl_hal::Features::TIER_A) — topic 10's browsers may lack it —
//!   and `for_adapter` asks for no optional features at all, so the constructor
//!   documented as "the shape a headless render test or `crcbl screenshot` uses"
//!   silently produces a device on which `PassTimers::new` declines and every
//!   per-pass timing is blank. It is not a bug: optional features cannot fail,
//!   so a caller that wants timers asks for them, and `apps/sandbox` and the
//!   graph-compile suite both now do. It is a trap, and it caught P1.3 once.
//! * **Barriers name whole images, because `crcbl-render` has no sub-resource
//!   vocabulary.** The seam is fine — [`ImageBarrier::range`](crcbl_hal::ImageBarrier::range)
//!   takes an [`ImageSubresourceRange`](crcbl_hal::ImageSubresourceRange) — but
//!   the graph's `ImageId` names a whole image, so every transition it computes
//!   covers every mip. P7's depth pyramid transitions one mip at a time and will
//!   need the graph to grow a sub-resource id. That is a change *above* the
//!   seam, which is the right place for it, and it is worth knowing before the
//!   pyramid is written rather than during.
//!
//! Three from P1.1 are recorded and deliberately left alone:
//!
//! * **`present` cannot report `suboptimal`.** `vkQueuePresentKHR` returns it,
//!   and `AcquiredFrame` is the only place the seam carries it, so this backend
//!   remembers it and folds it into the next acquire. That is the standard
//!   shape and costs one `bool`; a second reporting channel would cost every
//!   consumer a branch.
//! * **Adapter enumeration is surface-blind.**
//!   [`adapters`](crcbl_hal::Instance::adapters) knows nothing about any
//!   window, so finding an adapter that can present means asking
//!   [`surface_caps`](crcbl_hal::Instance::surface_caps) per adapter and
//!   treating `Err` as "try the next" — which is now written into that method's
//!   contract, and which this backend relies on: under Xvfb a discrete radv GPU
//!   cannot present at all while lavapipe beside it can. wgpu takes a
//!   `compatible_surface` at `request_adapter`; settling that belongs with
//!   `crcbl-render` owning device selection at P1.3.
//! * **`crcbl-vk` is a dependency of the `crcbl` umbrella.**
//!   `docs/plan/11-cli-headless.md`'s rule is about a *sample* naming a
//!   backend, and `apps/sandbox` names none — it asks the registry for one by
//!   value. P1.2 expected the registry to move to `crcbl-render` at P1.3, and it
//!   **did not**: this crate's own e2e suite now depends on `crcbl-render`,
//!   because the only honest way to test a render graph against a real driver is
//!   to run the real graph, and moving the registry the other way would make
//!   that a dependency cycle through dev-dependencies. Cargo permits one; nobody
//!   reading the manifests would thank us for it. The registry stays in `crcbl`,
//!   which is already the single place a backend is spelled.

// The crate's public surface is deliberately four types: an entry point, a
// device, and the validation report that makes the P1 gate checkable. Every
// module below it is `pub(crate)`, because nothing above the seam may learn a
// Vulkan type and a `pub` module full of `vk::` signatures is how that leaks.
// `debug` is the exception: its types *are* the public API.
pub mod debug;

pub(crate) mod adapter;
pub(crate) mod command;
pub(crate) mod conv;
pub(crate) mod deletion;
pub(crate) mod device;
pub(crate) mod instance;
pub(crate) mod mem;
pub(crate) mod pipeline;
pub(crate) mod spirv;
pub(crate) mod swapchain;

pub use debug::{Severity, ValidationMessage, ValidationReport, ValidationSink};
pub use device::VkDevice;
pub use instance::{API_VERSION, OpenError, VkInstance};

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::{BackendKind, Instance};

    /// The seam's central claim, restated for this backend: nothing about
    /// `VkInstance` requires naming it after construction.
    #[test]
    fn the_backend_is_usable_entirely_through_the_seam() {
        fn takes_dyn(_: Box<dyn Instance>) {}
        let _ = takes_dyn as fn(_);
        // And it identifies itself for logs, never for behaviour.
        assert_eq!(BackendKind::Vulkan.to_string(), "vulkan");
    }

    /// A machine with no Vulkan runtime must produce a *selectable* failure
    /// rather than a crash, which is the whole point of the loader decision.
    /// On a machine that has one this simply succeeds, and that is also fine —
    /// the property under test is that neither outcome panics, which is what
    /// makes this test meaningful on the macOS and Windows CI legs.
    #[test]
    fn opening_the_backend_never_panics_whatever_the_machine_has() {
        match VkInstance::open() {
            Ok(instance) => {
                assert_eq!(instance.backend(), BackendKind::Vulkan);
                assert!(
                    !instance.adapters().is_empty(),
                    "open() rejects an adapter-less loader"
                );
            }
            Err(error) => {
                let text = error.to_string();
                assert!(!text.is_empty(), "a failure must say what happened");
            }
        }
    }
}
