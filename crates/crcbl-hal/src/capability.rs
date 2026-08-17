//! Seam behaviours a backend must answer for, and the parity record of which
//! ones actually diverge.
//!
//! # Why this is not [`Features`]
//!
//! The two are different questions, and merging them loses the property this
//! module exists for:
//!
//! | | [`Features`] | [`Capability`] |
//! | --- | --- | --- |
//! | Names | an optional capability a **caller requests** at device creation | a seam behaviour a **backend must answer for** |
//! | Shape | `bitflags` | `enum` + exhaustive `match` |
//! | A backend that says nothing | reports the flag clear, silently | **does not compile** |
//!
//! [`Features`] is a *request* channel: a caller names what it
//! needs in [`DeviceDesc::required_features`](crate::DeviceDesc::required_features),
//! an adapter that lacks it fails loudly, and everything else degrades. That is
//! the right shape for what it does and it is not changing.
//!
//! What a bitflag cannot do is make a *backend* answer. A flag is a bit, and a
//! backend that never sets a new one compiles, links and runs — it simply
//! reports the capability absent, which is indistinguishable from a device that
//! genuinely lacks it. So a behaviour added to one backend costs nothing
//! anywhere else: the others never implement it, their refusal is invisible
//! until something runs, and no test can name what is missing because nothing
//! declares it exists.
//!
//! An `enum` with no wildcard arm inverts that. Adding a variant here is a
//! compile error in **every** backend that has not said what it does about it,
//! and the answer it is forced to give is one of exactly two things:
//! [`Support::Yes`], or [`Support::No`] with a reason a human wrote.
//!
//! # Prior art, and why this shape
//!
//! Every cross-API engine negotiates capabilities, and the ones that reach for a
//! bitmask reach for it because C has no better closed set:
//!
//! * **wgpu** — `Features` bitflags plus `DownlevelFlags`, validated at call
//!   time. The shape [`Features`] already is, with wgpu's
//!   problem: a backend that omits a flag reports "unsupported" and nothing says
//!   so.
//! * **Dawn** — `wgpu::FeatureName` is an *enum*, and each backend answers it in
//!   a `switch` compiled with `-Werror=switch`. That is this design, in C++.
//! * **bgfx** — a `supported` bitmask plus `caps->formats[]`, an array indexed by
//!   the format enum with a static assertion that its length matches the
//!   enumerator count. The array is C's way of buying the same "there must be an
//!   entry for every variant" property the `match` gives here for free.
//! * **Vulkan itself** — `VkPhysicalDeviceFeatures` is a struct of `VkBool32`, so
//!   a field added to it zero-initialises to `VK_FALSE` in every implementation
//!   that has not heard of it. The same silent-omission failure, one layer down.
//!
//! The consistent lesson is that the anti-rot property comes from a **closed
//! set** plus **an exhaustiveness check**, and from nothing else — not from
//! discipline, and not from a wider bitmask. Rust supplies both natively, so
//! this is the known-good design rather than a new one.
//!
//! # Cost
//!
//! [`Device::supports`](crate::Device::supports) takes a `Copy` enum and returns
//! a `Copy` enum holding at most a `&'static str`. No allocation, no lock, no
//! formatting, no table to walk: a backend's implementation is a `match` the
//! compiler lowers to a jump table or a comparison chain. It is a question asked
//! at set-up rather than in a frame, but it would be cheap enough to ask in one.
//!
//! # What is deliberately **not** a capability here
//!
//! * **Which [`SurfaceTarget`](crate::SurfaceTarget) kinds a backend accepts.**
//!   `crcbl-mtl` refuses a Wayland window and `crcbl-vk` refuses an AppKit one,
//!   but that is a platform axis `crcbl-shell` settles before a surface is
//!   asked for — Metal declining X11 is not a parity gap, and modelling it would
//!   add a variant per window system to describe something no two platforms were
//!   ever going to agree on.
//! * **Numeric ceilings.** [`Limits`](crate::Limits) is the right shape for a
//!   number, and "how big" is not a yes/no question.
//! * **Caller bugs.** A misaligned `fill_buffer` offset, a bind group written
//!   twice at one index, a swapchain retargeted to a different surface: every
//!   backend refuses these and *should*. A capability is something a caller
//!   could legitimately ask for and be told no.
//! * **Anything with no refusal behind it.** Every variant below was derived
//!   from a place some backend actually declines to do the thing.
//!
//! # The parity record
//!
//! [`DIVERGENCES`] is the reviewed list of every (capability, backend) pair that
//! is knowingly absent, and [`is_parity_gap`] is the rule that reads it. It is
//! the difference between divergence somebody decided and divergence that
//! happened: a backend that starts refusing something new fails the parity test
//! until the pair is added with a reason, and a backend that starts supporting
//! something on the list fails until the pair is removed.
//!
//! ```
//! use crcbl_hal::{BackendKind, Capability, DIVERGENCES, divergence};
//!
//! // Metal has no GPU-side draw count — a real API absence, written down.
//! let known = divergence(Capability::DrawIndirectCount, BackendKind::Metal)
//!     .expect("Metal's missing draw count is on the list");
//! assert!(known.why.contains("MTLIndirectCommandBuffer"));
//!
//! // Every entry names a backend that actually drives a GPU.
//! assert!(DIVERGENCES.iter().all(|entry| entry.backend.is_gpu()));
//! ```

use core::fmt;

use crate::{BackendKind, Features};

/// Declares [`Capability`] and everything derived from it from one variant list.
///
/// The list appears **once**. [`Capability::ALL`] and [`Capability::name`] are
/// generated from the same tokens as the enum, so neither can drift from it —
/// which matters because `ALL` is what the parity report and the agnostic seam
/// suite iterate, and a hand-written array that silently lost a variant would
/// make both report a clean sweep over a set with a hole in it.
macro_rules! capabilities {
    ($(
        $(#[$meta:meta])*
        $variant:ident,
    )+) => {
        /// One seam behaviour a backend either performs or refuses.
        ///
        /// **Every variant exists because some backend refuses it.** See the
        /// module docs for what is deliberately not modelled, and
        /// [`Device::supports`](crate::Device::supports) for how a backend
        /// answers.
        ///
        /// Adding a variant is a compile error in every backend until it says
        /// what it does — that is the whole point, and it is why this is an
        /// `enum` and not another [`Features`] bit.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Capability {
            $(
                $(#[$meta])*
                $variant,
            )+
        }

        impl Capability {
            /// Every capability, in declaration order.
            ///
            /// Generated from the same list as the enum, so it cannot be missing
            /// one. The parity report and the agnostic seam suite both walk it.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// How the variant is spelled in source, e.g. `"BufferFillWord"`.
            ///
            /// Taken from the declaration rather than written out beside it, so
            /// a renamed variant cannot keep an old name in a log line.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant),)+
                }
            }
        }
    };
}

capabilities! {
    // --- copies and fills ---

    /// [`fill_buffer`](crate::CommandEncoder::fill_buffer) with a value of `0`.
    ///
    /// The seam's headline use: zeroing an indirect count buffer at the top of a
    /// frame, which is otherwise a whole compute dispatch for four bytes.
    ///
    /// Separate from the two below because the three are genuinely different
    /// questions with different answers — this is the one every API with a fill
    /// at all can express, including `wgpu`'s `clear_buffer`, which is a zero
    /// fill and nothing else.
    BufferFillZero,

    /// [`fill_buffer`](crate::CommandEncoder::fill_buffer) with a non-zero value
    /// whose four bytes are **equal**, e.g. `0xFFFF_FFFF`.
    ///
    /// Its own capability because Metal's `MTLBlitCommandEncoder`
    /// `fillBuffer:range:value:` takes a **byte**, not a word: it can write this
    /// and cannot write [`BufferFillWord`](Self::BufferFillWord). A backend on
    /// `wgpu` can do neither, because `clear_buffer` writes zero.
    BufferFillRepeatedByte,

    /// [`fill_buffer`](crate::CommandEncoder::fill_buffer) with an arbitrary
    /// `u32` — the repeating 32-bit value the seam's own documentation promises.
    ///
    /// The narrowest of the three and the one fewest backends have. A value
    /// whose bytes differ, `0x1234_5678`, is what separates it from
    /// [`BufferFillRepeatedByte`](Self::BufferFillRepeatedByte).
    BufferFillWord,

    /// [`copy_image_to_image`](crate::CommandEncoder::copy_image_to_image).
    ///
    /// Both sides are texture locations with their own subresource and region,
    /// which is a different call from the buffer↔image copies and is refused
    /// separately where it is refused at all.
    ImageToImageCopy,

    /// A buffer↔image copy —
    /// [`copy_image_to_buffer`](crate::CommandEncoder::copy_image_to_buffer) or
    /// [`copy_buffer_to_image`](crate::CommandEncoder::copy_buffer_to_image) —
    /// whose image carries a **depth or depth-stencil** format and whose
    /// subresource names [`ImageAspect::DEPTH`](crate::ImageAspect::DEPTH).
    ///
    /// Separate from the colour copies every backend performs, because a depth
    /// image is not one plane of one type. An API that stores a *sampled* depth
    /// texture typeless needs the plane and a fully typed footprint chosen at
    /// the copy itself, and [`BufferImageCopy`](crate::BufferImageCopy) carries
    /// neither — so a backend can copy every colour format and still have no
    /// expression for this one.
    ///
    /// It is what a shadow atlas is read back through, which is why it is worth
    /// declaring rather than discovering: without it there is no way to show
    /// that a shadow pass wrote anything, and a shadow pass that wrote nothing
    /// renders a frame in which every surface is lit and nothing looks wrong.
    DepthImageCopy,

    // --- render pass ---

    /// A [`ColorAttachment::resolve`](crate::ColorAttachment::resolve) view: the
    /// pass resolves its multisampled colour target into a single-sampled one as
    /// it ends.
    MsaaResolveAttachment,

    /// [`set_stencil_reference`](crate::CommandEncoder::set_stencil_reference) —
    /// the dynamic stencil reference value subsequent draws compare against.
    StencilReference,

    // --- draws ---

    /// [`draw_indirect_count`](crate::CommandEncoder::draw_indirect_count) and
    /// its indexed sibling: the draw **count** is read from a GPU buffer, so the
    /// CPU never learns how many draws a pass emits.
    ///
    /// [`Features::DRAW_INDIRECT_COUNT`](crate::Features::DRAW_INDIRECT_COUNT)
    /// is the flag a caller requests; this is the backend's answer about the two
    /// encoder methods, which is not the same claim — a backend can report the
    /// flag without having wired the calls up, and Metal reports it clear *and*
    /// refuses the calls for a reason that is not about any one device.
    DrawIndirectCount,

    /// [`DrawIndirect::stride`](crate::DrawIndirect::stride) larger than the
    /// argument structure: the arguments are **padded** in the buffer rather
    /// than tightly packed.
    ///
    /// Distinct from drawing indirectly at all. Vulkan's `vkCmdDrawIndirect`
    /// takes a stride and honours it, and D3D12 builds an `ID3D12CommandSignature`
    /// per stride; `wgpu`'s `multi_draw_indirect` reads its own packed structures
    /// and has nowhere to put one, so a padded stride there would silently read
    /// the wrong words — which is why it is refused rather than ignored.
    IndirectArgumentPaddedStride,

    /// The mesh-shading path: a [`MeshPipelineDesc`](crate::MeshPipelineDesc)
    /// can be created and
    /// [`draw_mesh_tasks`](crate::CommandEncoder::draw_mesh_tasks) and
    /// [`draw_mesh_tasks_indirect`](crate::CommandEncoder::draw_mesh_tasks_indirect)
    /// recorded against it.
    ///
    /// One capability rather than three, because the seam already states the
    /// tie: `draw_mesh_tasks_indirect` "requires
    /// [`Features::MESH_SHADER`](crate::Features::MESH_SHADER) and nothing more
    /// … a device that can launch this stage at all can launch it from memory",
    /// and no backend refuses the pipeline while admitting the draw or the
    /// reverse.
    MeshShading,

    /// The amplification stage in front of a mesh shader —
    /// [`MeshPipelineDesc::task`](crate::MeshPipelineDesc::task) is `Some`.
    ///
    /// Separately optional from [`MeshShading`](Self::MeshShading) for the same
    /// reason [`Features::TASK_SHADER`](crate::Features::TASK_SHADER) is a
    /// separate flag: a mesh shader works with a workgroup count computed
    /// elsewhere, and this stage is only what moves that decision into the draw.
    TaskShaderStage,

    // --- bindings and pipeline layout ---

    /// [`update_bind_group`](crate::Device::update_bind_group): rewriting an
    /// already-created bind group's entries in place.
    ///
    /// The update-after-bind model, and what a bindless texture table is
    /// restreamed through. WebGPU bind groups are immutable once created, so a
    /// backend on it can only rebuild the group.
    ///
    /// A backend answers **whether it has the operation at all**. Vulkan
    /// additionally requires the individual layout to carry
    /// [`BindingFlags::UPDATE_AFTER_BIND`](crate::BindingFlags::UPDATE_AFTER_BIND),
    /// which is a property of the layout the caller built rather than of the
    /// device, and is refused per call.
    UpdateBindGroup,

    /// [`push_constants`](crate::CommandEncoder::push_constants) and the
    /// [`PipelineLayoutDesc::push_constants`](crate::PipelineLayoutDesc::push_constants)
    /// range they are written through.
    ///
    /// The seam requires the refusal at **pipeline-layout creation**, not at the
    /// write, so a caller finds out once rather than losing writes silently
    /// every frame.
    PushConstants,

    /// A runtime-sized descriptor array — a
    /// [`BindGroupLayoutEntry`](crate::BindGroupLayoutEntry) carrying
    /// [`BindingFlags`](crate::BindingFlags), and the
    /// [`BindGroupDesc::variable_count`](crate::BindGroupDesc::variable_count)
    /// that chooses its length.
    ///
    /// What [`BindingModel::Bindless`](crate::BindingModel::Bindless) is built
    /// on. A backend binding flat argument tables has fixed-size arrays and no
    /// way to express this, which is a different answer from having no arrays.
    BindlessDescriptorArray,

    /// A [`BindingKind::StorageImage`](crate::BindingKind::StorageImage) entry
    /// in a bind group layout.
    ///
    /// Its own capability because some implementations need the texel format and
    /// view dimension in the **layout**, which the seam only carries on the
    /// image view — so the layout cannot be built at all, rather than the
    /// binding merely being slower.
    StorageImageBinding,

    // --- rasteriser state ---

    /// [`PolygonMode::Line`](crate::PolygonMode::Line) — wireframe fill.
    PolygonModeLine,

    /// Depth **clamping** rather than clipping, for shadow casters behind the
    /// near plane.
    DepthClamp,

    /// A [`SamplerDesc`](crate::SamplerDesc) with anisotropy above `1.0`.
    SamplerAnisotropy,

    // --- queries ---

    /// A [`QueryKind::Timestamp`](crate::QueryKind::Timestamp) query set, the
    /// [`write_timestamp`](crate::CommandEncoder::write_timestamp) that fills it
    /// and the [`query_results`](crate::Device::query_results) that read it.
    TimestampQuery,

    /// A [`QueryKind::Occlusion`](crate::QueryKind::Occlusion) query set.
    OcclusionQuery,

    /// A [`QueryKind::PipelineStatistics`](crate::QueryKind::PipelineStatistics)
    /// query set.
    ///
    /// Three query variants rather than one, because the three are refused
    /// separately: a device answers each of
    /// [`Features::TIMESTAMP_QUERY`](crate::Features::TIMESTAMP_QUERY),
    /// [`OCCLUSION_QUERY`](crate::Features::OCCLUSION_QUERY) and
    /// [`PIPELINE_STATISTICS_QUERY`](crate::Features::PIPELINE_STATISTICS_QUERY)
    /// independently, and folding them would let a backend with only timestamps
    /// claim the lot.
    PipelineStatisticsQuery,

    // --- synchronisation ---

    /// [`SemaphoreKind::Timeline`](crate::SemaphoreKind::Timeline): a monotonic
    /// `u64` counter a submission signals and both GPU and CPU can observe.
    ///
    /// The claim is the **observable** one — that
    /// [`semaphore_value`](crate::Device::semaphore_value) reports the counter
    /// advancing as submissions complete. A backend that hands out a handle
    /// whose value never moves has not got this, whatever its return codes say.
    TimelineSemaphore,

    /// [`SemaphoreKind::Binary`](crate::SemaphoreKind::Binary): the one-shot,
    /// GPU-only signal WSI acquire/present needs.
    ///
    /// Apart from [`TimelineSemaphore`](Self::TimelineSemaphore) deliberately. A
    /// device may have no timeline and must still hand out a binary one, because
    /// the swapchain is where they come from — and a backend that refused both
    /// would pass every test that only checks the timeline half.
    BinarySemaphore,

    /// [`wait_semaphores`](crate::Device::wait_semaphores): the **CPU** blocking
    /// until a timeline reaches a value.
    ///
    /// Distinct from creating one. A backend can hand out a counter a submission
    /// signals while having no host-side wait to offer, which is what a
    /// fence-shaped API looks like before its wait half is wired up.
    CpuTimelineWait,

    /// Waiting — from the CPU, or in a [`SubmitInfo`](crate::SubmitInfo) — on a
    /// timeline value that **nothing submitted so far will signal**, expecting
    /// later work to satisfy it.
    ///
    /// A real semaphore object blocks until somebody signals it. A backend that
    /// emulates timelines over per-submission completion, or that has one queue
    /// and no CPU-side signal, has nothing for a future signal to arrive on, so
    /// such a wait could only ever stop the queue; refusing is the honest answer
    /// and it is a genuine behavioural divergence rather than a missing call.
    TimelineWaitBeforeSignal,
}

impl Capability {
    /// The [`Features`] bit a caller consults before asking for this, if there
    /// is one.
    ///
    /// **What makes the parity rule fair.** A backend refusing something on a
    /// device that reports the gating flag clear has not diverged from anything
    /// — the device is simply lesser, which is exactly what
    /// [`Features`] already models and logs as a downgrade. A backend refusing
    /// it on a device that *does* report the flag has diverged, and
    /// [`is_parity_gap`] is that distinction.
    ///
    /// `None` means no flag governs it, so every refusal is the backend's own.
    /// That is the majority here, and it is the measurement this module was
    /// built from: most refusal sites answer to no declared capability at all.
    #[must_use]
    pub const fn gating_feature(self) -> Option<Features> {
        match self {
            // Nothing in `Features` describes what a fill may write, which is
            // why the fills were the divergence that reached CI.
            Self::BufferFillZero
            | Self::BufferFillRepeatedByte
            | Self::BufferFillWord
            | Self::ImageToImageCopy
            | Self::DepthImageCopy
            | Self::MsaaResolveAttachment
            | Self::StencilReference
            | Self::IndirectArgumentPaddedStride
            | Self::UpdateBindGroup
            | Self::StorageImageBinding
            | Self::BinarySemaphore => None,
            Self::DrawIndirectCount => Some(Features::DRAW_INDIRECT_COUNT),
            Self::MeshShading => Some(Features::MESH_SHADER),
            Self::TaskShaderStage => Some(Features::TASK_SHADER),
            Self::PushConstants => Some(Features::PUSH_CONSTANTS),
            Self::BindlessDescriptorArray => Some(Features::DESCRIPTOR_INDEXING),
            Self::PolygonModeLine => Some(Features::POLYGON_MODE_LINE),
            Self::DepthClamp => Some(Features::DEPTH_CLAMP),
            Self::SamplerAnisotropy => Some(Features::SAMPLER_ANISOTROPY),
            Self::TimestampQuery => Some(Features::TIMESTAMP_QUERY),
            Self::OcclusionQuery => Some(Features::OCCLUSION_QUERY),
            Self::PipelineStatisticsQuery => Some(Features::PIPELINE_STATISTICS_QUERY),
            // All three are about one object, so one flag governs them.
            Self::TimelineSemaphore | Self::CpuTimelineWait | Self::TimelineWaitBeforeSignal => {
                Some(Features::TIMELINE_SEMAPHORE)
            }
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Whether a backend performs a [`Capability`], and why not when it does not.
///
/// Two arms and no third: "maybe", "partially" and "not yet" are all
/// [`No`](Self::No) with a reason saying which, because a caller can do nothing
/// different with a third answer and every one that existed would be a branch
/// nobody tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Support {
    /// The backend performs it **exactly as the seam documents it**, on this
    /// device.
    ///
    /// Not "the API underneath has it" and not "the code is there": this is the
    /// claim the agnostic seam suite exercises, so a backend saying it and then
    /// refusing — or quietly doing something else — at the call site is a test
    /// failure.
    Yes,

    /// The backend refuses, for the reason given.
    ///
    /// The reason is written for whoever reads the failure, so it names the
    /// obstacle — the API call that does not exist, the feature bit the device
    /// reported clear, the slice that has not landed — rather than restating the
    /// capability. `&'static str`, so asking costs no allocation.
    No(&'static str),
}

impl Support {
    /// [`Yes`](Self::Yes) when `features` holds `feature`, otherwise
    /// [`No`](Self::No) with `why`.
    ///
    /// The shape most arms of a backend's `match` have, written once here
    /// because five backends were otherwise going to spell the same `if` five
    /// ways — and because an arm that reads the *wrong* flag is the one mistake
    /// in this file a reviewer cannot see, so the flag belongs at the call site
    /// with nothing else beside it.
    ///
    /// `why` stays the caller's: a device that reports a flag clear and a
    /// backend that never reports it at all are different sentences, and the
    /// reader of a parity failure needs to know which.
    #[must_use]
    pub const fn granted(features: Features, feature: Features, why: &'static str) -> Self {
        if features.contains(feature) {
            Self::Yes
        } else {
            Self::No(why)
        }
    }

    /// Whether this is [`Support::Yes`].
    #[must_use]
    pub const fn is_yes(self) -> bool {
        matches!(self, Self::Yes)
    }

    /// The refusal reason, or `None` when supported.
    #[must_use]
    pub const fn reason(self) -> Option<&'static str> {
        match self {
            Self::Yes => None,
            Self::No(why) => Some(why),
        }
    }
}

impl fmt::Display for Support {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yes => f.write_str("supported"),
            Self::No(why) => write!(f, "unsupported: {why}"),
        }
    }
}

/// One (capability, backend) pair that is knowingly absent.
///
/// See [`DIVERGENCES`] for what the list as a whole is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Divergence {
    /// The capability the backend does not have.
    pub capability: Capability,
    /// The backend that does not have it.
    pub backend: BackendKind,
    /// Why, in enough detail to decide whether it should stay that way. This is
    /// the sentence a reviewer reads when the pair is added.
    pub why: &'static str,
}

/// Every capability a backend is knowingly without, **on every device it can
/// open**.
///
/// **This is the parity contract.** A capability every GPU backend has appears
/// nowhere here; one that some have and some do not appears once per backend
/// that lacks it, with the reason. The list is checked from both directions
/// against a running device by the agnostic seam suite, so:
///
/// * a backend that starts refusing something new fails until the pair is added,
///   which is a review of the divergence rather than a silence, and
/// * a backend that starts *supporting* something listed fails until the pair is
///   removed, so the list cannot rot into a record of history.
///
/// A refusal that is merely *this device's* — the gating
/// [`Features`] flag is clear — is not listed and is not a gap; see
/// [`is_parity_gap`].
///
/// Entries are grouped by capability. [`BackendKind::Null`] never appears: it
/// records rather than executes, so it is not part of the parity model — see
/// [`BackendKind::is_gpu`].
pub const DIVERGENCES: &[Divergence] = &[
    // --- fills: the worked example, and why there are three variants ---
    Divergence {
        capability: Capability::BufferFillZero,
        backend: BackendKind::Dx12,
        why: "D3D12's fill is ClearUnorderedAccessViewUint, which takes a descriptor from a \
              shader-visible heap that crcbl_dx12::descriptor does not create. A deliberate \
              non-fix: crcbl-render zeroes its draw-generation counters with a clear dispatch, \
              which runs anywhere that can dispatch",
    },
    Divergence {
        capability: Capability::BufferFillRepeatedByte,
        backend: BackendKind::Wgpu,
        why: "wgpu's only fill is clear_buffer, which writes zero and takes no value",
    },
    Divergence {
        capability: Capability::BufferFillRepeatedByte,
        backend: BackendKind::WebGpu,
        why: "WebGPU's only fill is GPUCommandEncoder.clearBuffer, which writes zero; the stream \
              carries the value so the replayer can refuse a non-zero one at the target rather \
              than write the wrong bytes",
    },
    Divergence {
        capability: Capability::BufferFillRepeatedByte,
        backend: BackendKind::Dx12,
        why: "no buffer fill at all on this backend; see the BufferFillZero entry",
    },
    Divergence {
        capability: Capability::BufferFillWord,
        backend: BackendKind::Wgpu,
        why: "wgpu's only fill is clear_buffer, which writes zero and takes no value",
    },
    Divergence {
        capability: Capability::BufferFillWord,
        backend: BackendKind::WebGpu,
        why: "WebGPU's only fill is GPUCommandEncoder.clearBuffer, which writes zero",
    },
    Divergence {
        capability: Capability::BufferFillWord,
        backend: BackendKind::Metal,
        why: "MTLBlitCommandEncoder fillBuffer:range:value: repeats a single byte, so only a u32 \
              whose four bytes are equal has an encoding",
    },
    Divergence {
        capability: Capability::BufferFillWord,
        backend: BackendKind::Dx12,
        why: "no buffer fill at all on this backend; see the BufferFillZero entry",
    },
    // --- copies ---
    Divergence {
        capability: Capability::ImageToImageCopy,
        backend: BackendKind::Dx12,
        why: "both sides are texture locations with their own subresource and box, and neither is \
              the placed footprint plan_copy builds (the DX12 pipeline slice)",
    },
    Divergence {
        capability: Capability::DepthImageCopy,
        backend: BackendKind::Dx12,
        why: "a D3D12 depth format has two planes and a sampled one is created typeless, so the \
              copy needs a PlaneSlice and a fully typed D3D12_PLACED_SUBRESOURCE_FOOTPRINT — \
              neither of which BufferImageCopy carries and neither of which plan_copy can derive \
              from the image alone (the DX12 depth slice)",
    },
    Divergence {
        capability: Capability::DepthImageCopy,
        backend: BackendKind::WebGpu,
        why: "the replayer turns BufferImageCopy::buffer_row_length from texels into WebGPU's \
              bytesPerRow through a bytes-per-texel table that holds the single-plane colour \
              formats only, and it refuses a depth format rather than guessing a figure for one \
              (the TEXEL_BYTES table in web/engine/gpu-replay.js)",
    },
    // --- render pass ---
    Divergence {
        capability: Capability::MsaaResolveAttachment,
        backend: BackendKind::Dx12,
        why: "the render-pass path binds render-target views directly and emits no \
              ResolveSubresource, so there is nothing for a resolve view to attach to (the DX12 \
              pipeline slice)",
    },
    Divergence {
        capability: Capability::StencilReference,
        backend: BackendKind::WebGpu,
        why: "the WebGPU command stream has no SetStencilReference tag yet, so the value would be \
              dropped rather than applied (the WebGPU stream slice)",
    },
    // --- draws ---
    Divergence {
        capability: Capability::DrawIndirectCount,
        backend: BackendKind::Metal,
        why: "Metal's only GPU-side count is an MTLIndirectCommandBuffer whose commands a compute \
              pass must encode before the render pass begins — absent from the API rather than \
              unwritten, and wgpu reached the same conclusion independently",
    },
    Divergence {
        capability: Capability::DrawIndirectCount,
        backend: BackendKind::WebGpu,
        why: "WebGPU has no count-buffer draw and the stream has no tag for one",
    },
    Divergence {
        capability: Capability::IndirectArgumentPaddedStride,
        backend: BackendKind::Wgpu,
        why: "wgpu's multi_draw_indirect family reads its own tightly packed argument structs and \
              takes no stride, so a padded one would read the wrong words silently",
    },
    Divergence {
        capability: Capability::IndirectArgumentPaddedStride,
        backend: BackendKind::WebGpu,
        why: "WebGPU's drawIndirect reads one tightly packed argument structure and has no stride \
              parameter to honour",
    },
    Divergence {
        capability: Capability::MeshShading,
        backend: BackendKind::Metal,
        why: "Metal has the object/mesh stages and Slang emits them, but this backend builds no \
              MTLMeshRenderPipelineDescriptor (the Metal mesh slice)",
    },
    Divergence {
        capability: Capability::MeshShading,
        backend: BackendKind::Dx12,
        why: "D3D12 has the stages and the DXIL is committed, but this backend builds no pipeline \
              state stream for them (the DX12 mesh slice)",
    },
    Divergence {
        capability: Capability::MeshShading,
        backend: BackendKind::Wgpu,
        why: "WebGPU has no mesh stage and wgpu exposes none",
    },
    Divergence {
        capability: Capability::MeshShading,
        backend: BackendKind::WebGpu,
        why: "WebGPU has no mesh stage, and no proposal for one — this is the API rather than the \
              slice, so it is permanent",
    },
    Divergence {
        capability: Capability::TaskShaderStage,
        backend: BackendKind::Metal,
        why: "no mesh pipeline to put an object stage in front of; see the MeshShading entry",
    },
    Divergence {
        capability: Capability::TaskShaderStage,
        backend: BackendKind::Dx12,
        why: "no mesh pipeline to put an amplification stage in front of; see the MeshShading entry",
    },
    Divergence {
        capability: Capability::TaskShaderStage,
        backend: BackendKind::Wgpu,
        why: "WebGPU has no mesh stage; see the MeshShading entry",
    },
    Divergence {
        capability: Capability::TaskShaderStage,
        backend: BackendKind::WebGpu,
        why: "WebGPU has no mesh stage; see the MeshShading entry",
    },
    // --- bindings ---
    Divergence {
        capability: Capability::UpdateBindGroup,
        backend: BackendKind::Wgpu,
        why: "WebGPU bind groups are immutable once created and wgpu exposes no update-after-bind \
              path; a caller rebuilds the group instead",
    },
    Divergence {
        capability: Capability::UpdateBindGroup,
        backend: BackendKind::WebGpu,
        why: "the WebGPU command stream has no update_bind_group command yet (the WebGPU bindless \
              slice)",
    },
    Divergence {
        capability: Capability::PushConstants,
        backend: BackendKind::Metal,
        why: "setVertexBytes:length:atIndex: is Metal's closest fit, and the committed MSL puts \
              its push-constant block at buffer(0) — ahead of every bound buffer, which no \
              flattening of this descriptor can reproduce (the Metal root-constant slice)",
    },
    Divergence {
        capability: Capability::PushConstants,
        backend: BackendKind::Dx12,
        why: "D3D12's root constants are the equivalent, and nothing here knows which root \
              parameter slot the committed DXIL puts one at (the DX12 root-constant slice)",
    },
    Divergence {
        capability: Capability::PushConstants,
        backend: BackendKind::Wgpu,
        why: "wgpu calls them immediates and gates them behind its own IMMEDIATES feature, which \
              this backend does not enable, so a pipeline layout has no immediate block to write",
    },
    Divergence {
        capability: Capability::PushConstants,
        backend: BackendKind::WebGpu,
        why: "WebGPU has no push constants; the substitute is a dynamic-offset uniform buffer, \
              which the seam already carries as bind-group dynamic offsets",
    },
    Divergence {
        capability: Capability::BindlessDescriptorArray,
        backend: BackendKind::Metal,
        why: "this backend binds Metal's flat argument tables, which have a fixed length and no \
              runtime-sized array (the Metal argument-buffer slice)",
    },
    Divergence {
        capability: Capability::BindlessDescriptorArray,
        backend: BackendKind::Wgpu,
        why: "wgpu's binding arrays need a fixed count in the layout and offer no partial \
              binding, so the runtime-sized form the seam describes has no expression",
    },
    Divergence {
        capability: Capability::BindlessDescriptorArray,
        backend: BackendKind::WebGpu,
        why: "WebGPU has no binding arrays at all, fixed-size or runtime-sized, so a material \
              table is indexed through a storage buffer here rather than through descriptors",
    },
    Divergence {
        capability: Capability::StorageImageBinding,
        backend: BackendKind::Wgpu,
        why: "wgpu needs the texel format and view dimension at bind-group-layout creation and \
              BindingKind::StorageImage carries neither — a gap in the seam's descriptor rather \
              than in wgpu",
    },
    Divergence {
        capability: Capability::StorageImageBinding,
        backend: BackendKind::WebGpu,
        why: "GPUStorageTextureBindingLayout requires a texel format and view dimension at layout \
              creation, and BindingKind::StorageImage carries neither — the same seam-descriptor \
              gap crcbl-wgpu names, reached through a different API",
    },
    // --- rasteriser state ---
    //
    // Only one entry, and that is the point: `PolygonModeLine`, `DepthClamp` and
    // `SamplerAnisotropy` are refused by every backend *only* where the device
    // reports the flag clear, which is `Features` doing its job. WebGPU is the
    // exception because it has no wireframe on any device it could ever open.
    Divergence {
        capability: Capability::PolygonModeLine,
        backend: BackendKind::WebGpu,
        why: "WebGPU has no core expression for a wireframe fill mode; unlike the other two \
              rasteriser capabilities this is absent from the API rather than from a given device",
    },
    // --- queries ---
    Divergence {
        capability: Capability::TimestampQuery,
        backend: BackendKind::Metal,
        why: "no query sets are built on this backend; supportsCounterSampling: answers per \
              sampling point, which the seam's query set does not describe (the Metal query slice)",
    },
    Divergence {
        capability: Capability::TimestampQuery,
        backend: BackendKind::Dx12,
        why: "no query sets are built on this backend; the timestamp period only a queue can \
              answer is the missing piece (the DX12 query slice)",
    },
    Divergence {
        capability: Capability::TimestampQuery,
        backend: BackendKind::Wgpu,
        why: "this backend enables neither wgpu's TIMESTAMP_QUERY nor its statistics features, so \
              no query set can be created even on an adapter that has them",
    },
    Divergence {
        capability: Capability::TimestampQuery,
        backend: BackendKind::WebGpu,
        why: "query sets are not wired into the WebGPU stream (the WebGPU query slice)",
    },
    Divergence {
        capability: Capability::OcclusionQuery,
        backend: BackendKind::Metal,
        why: "no query sets are built on this backend; see the TimestampQuery entry",
    },
    Divergence {
        capability: Capability::OcclusionQuery,
        backend: BackendKind::Dx12,
        why: "no query sets are built on this backend; see the TimestampQuery entry",
    },
    Divergence {
        capability: Capability::OcclusionQuery,
        backend: BackendKind::Wgpu,
        why: "no query sets are built on this backend; see the TimestampQuery entry",
    },
    Divergence {
        capability: Capability::OcclusionQuery,
        backend: BackendKind::WebGpu,
        why: "WebGPU has occlusion queries only as a render-pass attachment, which the seam's \
              standalone query set does not describe",
    },
    Divergence {
        capability: Capability::PipelineStatisticsQuery,
        backend: BackendKind::Metal,
        why: "no query sets are built on this backend; see the TimestampQuery entry",
    },
    Divergence {
        capability: Capability::PipelineStatisticsQuery,
        backend: BackendKind::Dx12,
        why: "no query sets are built on this backend; see the TimestampQuery entry",
    },
    Divergence {
        capability: Capability::PipelineStatisticsQuery,
        backend: BackendKind::Wgpu,
        why: "no query sets are built on this backend; see the TimestampQuery entry",
    },
    Divergence {
        capability: Capability::PipelineStatisticsQuery,
        backend: BackendKind::WebGpu,
        why: "WebGPU has no pipeline-statistics queries",
    },
    // --- synchronisation ---
    Divergence {
        capability: Capability::TimelineSemaphore,
        backend: BackendKind::Dx12,
        why: "ID3D12Fence is the seam's timeline almost verbatim, but the other half — \
              ID3D12CommandQueue::Wait and a submission to attach it to — is not built, so a \
              semaphore handed out now would be a counter nothing can signal from the GPU (the \
              DX12 command slice)",
    },
    Divergence {
        capability: Capability::TimelineSemaphore,
        backend: BackendKind::WebGpu,
        why: "WebGPU has no semaphores. It orders submissions implicitly — one queue, executed in \
              order, hazards tracked by the browser — and its only completion signal, \
              GPUQueue.onSubmittedWorkDone(), resolves for everything submitted so far and \
              carries no value, so nothing there could drive a counter. create_semaphore refuses \
              SemaphoreKind::Timeline; it used to hand out a handle whose semaphore_value \
              answered 0 for ever, which is the succeed-while-doing-nothing shape this enum \
              exists to make visible",
    },
    Divergence {
        capability: Capability::BinarySemaphore,
        backend: BackendKind::Dx12,
        why: "create_semaphore refuses both kinds; see the TimelineSemaphore entry (the DX12 \
              command slice)",
    },
    Divergence {
        capability: Capability::CpuTimelineWait,
        backend: BackendKind::Dx12,
        why: "there is no semaphore to wait on; see the TimelineSemaphore entry (the DX12 command \
              slice)",
    },
    Divergence {
        capability: Capability::CpuTimelineWait,
        backend: BackendKind::WebGpu,
        why: "create_semaphore refuses the timeline kind, so there is no counter for a CPU wait to \
              read; the binary semaphore this backend does hand out is GPU-waitable only. \
              wait_semaphores refuses rather than answering Ok(true) for a wait it never \
              evaluated — see the TimelineSemaphore entry",
    },
    Divergence {
        capability: Capability::TimelineWaitBeforeSignal,
        backend: BackendKind::Wgpu,
        why: "wgpu has no standalone semaphore object; a timeline here is per-submission \
              completion, so a wait on a value nothing submitted will signal can only hang",
    },
    Divergence {
        capability: Capability::TimelineWaitBeforeSignal,
        backend: BackendKind::Metal,
        why: "this backend runs one queue and the seam gives no way to signal an MTLSharedEvent \
              from the CPU, so only an earlier submission can satisfy such a wait — waiting would \
              stop the queue rather than wait",
    },
    Divergence {
        capability: Capability::TimelineWaitBeforeSignal,
        backend: BackendKind::Dx12,
        why: "there is no semaphore to wait on at all; see the TimelineSemaphore entry",
    },
    Divergence {
        capability: Capability::TimelineWaitBeforeSignal,
        backend: BackendKind::WebGpu,
        why: "a wait of any kind is refused here, so a wait on a value nothing has signalled is \
              refused with the rest; see the CpuTimelineWait entry",
    },
];

/// The [`DIVERGENCES`] entry for this pair, if the pair is a known divergence.
///
/// A linear scan of a `const` slice, which is why the list is kept small enough
/// to read: this is asked once per capability in a test, never in a frame.
#[must_use]
pub fn divergence(capability: Capability, backend: BackendKind) -> Option<&'static Divergence> {
    DIVERGENCES
        .iter()
        .find(|entry| entry.capability == capability && entry.backend == backend)
}

/// Whether a refusal is a **parity gap** — a divergence that has not been
/// written down.
///
/// The rule the agnostic seam suite applies to every capability of a running
/// device, and the whole of what "reviewed divergence" means here:
///
/// * A refusal on a device whose [`Features`] lack the capability's
///   [`gating_feature`](Capability::gating_feature) is **not** a gap. The device
///   is lesser, which [`Features`] already models and logs as a downgrade, and
///   holding a software rasteriser to a mesh-shader capability would make the
///   suite fail for the wrong reason.
/// * A refusal that [`DIVERGENCES`] names is **not** a gap. Somebody decided it
///   and wrote down why.
/// * Any other refusal **is** a gap: this backend cannot do something its peers
///   can, on a device that reports it should, and nobody has said so.
///
/// The stated limit: a backend can still put itself outside this rule by
/// reporting the gating flag clear. That is not a hole so much as a different
/// channel — an absent flag is visible in
/// [`downgrades`](crate::downgrades) at device creation, which is where topic 39
/// requires it to be reported, and it is a claim about the *device* that the
/// adapter tests hold it to.
#[must_use]
pub fn is_parity_gap(capability: Capability, backend: BackendKind, features: Features) -> bool {
    if let Some(gate) = capability.gating_feature()
        && !features.contains(gate)
    {
        return false;
    }
    divergence(capability, backend).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every GPU backend, for the tests that must sweep them. Written out rather
    /// than derived, so that a new [`BackendKind`] fails
    /// [`every_gpu_backend_is_in_this_sweep`] instead of silently escaping the
    /// parity model.
    const GPU_BACKENDS: [BackendKind; 5] = [
        BackendKind::Vulkan,
        BackendKind::Wgpu,
        BackendKind::WebGpu,
        BackendKind::Metal,
        BackendKind::Dx12,
    ];

    #[test]
    fn every_gpu_backend_is_in_this_sweep() {
        for backend in GPU_BACKENDS {
            assert!(backend.is_gpu(), "{backend} must be in the parity model");
        }
        assert!(!BackendKind::Null.is_gpu());
        // The count comes from `is_gpu`'s own match rather than from a literal,
        // so a sixth backend makes this fail instead of quietly sitting outside
        // every sweep below.
        let every = [
            BackendKind::Vulkan,
            BackendKind::Wgpu,
            BackendKind::WebGpu,
            BackendKind::Metal,
            BackendKind::Dx12,
            BackendKind::Null,
        ];
        assert_eq!(
            every.iter().filter(|kind| kind.is_gpu()).count(),
            GPU_BACKENDS.len(),
            "a BackendKind drives a GPU and is not swept by these tests"
        );
    }

    /// `ALL` is generated from the enum's own variant list, so it cannot fail by
    /// omission — what this catches is the macro being replaced by a
    /// hand-written array later, which is the whole reason the macro exists.
    #[test]
    fn all_holds_every_capability_exactly_once() {
        let mut seen: Vec<Capability> = Capability::ALL.to_vec();
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "a capability appears twice in ALL");
        assert!(
            count >= 20,
            "ALL has shrunk to {count}; the capabilities are derived from real refusal sites and \
             those have not gone away"
        );
    }

    /// A name is what a failing parity report prints, so two variants sharing
    /// one would leave the report unable to say which diverged.
    #[test]
    fn every_capability_has_its_own_name() {
        let mut names: Vec<&'static str> = Capability::ALL.iter().map(|c| c.name()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "two capabilities share a name");
        for capability in Capability::ALL {
            assert_eq!(capability.to_string(), capability.name());
            assert!(!capability.name().is_empty());
        }
    }

    /// A gate must be a single flag. `Features::GPU_DRIVEN` is a named union,
    /// and a capability gated on it would be unsatisfiable on any device missing
    /// any one of six unrelated things.
    #[test]
    fn a_gating_feature_is_always_exactly_one_flag() {
        let mut gated = 0;
        for capability in Capability::ALL {
            let Some(gate) = capability.gating_feature() else {
                continue;
            };
            gated += 1;
            assert_eq!(
                gate.bits().count_ones(),
                1,
                "{capability} is gated on {gate:?}, which is not a single flag"
            );
        }
        assert!(
            gated > 0 && gated < Capability::ALL.len(),
            "both halves must be populated: {gated} of {} capabilities are gated, and a run where \
             all or none were would make the parity rule vacuous in one direction",
            Capability::ALL.len()
        );
    }

    #[test]
    fn granted_reads_the_flag_it_was_given() {
        let device = Features::COMPUTE | Features::TIMELINE_SEMAPHORE;
        assert_eq!(
            Support::granted(device, Features::COMPUTE, "no compute"),
            Support::Yes
        );
        assert_eq!(
            Support::granted(device, Features::MESH_SHADER, "no mesh shader"),
            Support::No("no mesh shader")
        );
        // And it is `contains`, not `intersects`: a capability gated on a flag
        // the device half-has would otherwise report supported.
        assert_eq!(
            Support::granted(device, Features::GPU_DRIVEN, "not the whole bundle"),
            Support::No("not the whole bundle")
        );
    }

    #[test]
    fn support_carries_the_reason_and_nothing_else() {
        assert!(Support::Yes.is_yes());
        assert_eq!(Support::Yes.reason(), None);
        assert_eq!(Support::Yes.to_string(), "supported");

        let refused = Support::No("there is no such call");
        assert!(!refused.is_yes());
        assert_eq!(refused.reason(), Some("there is no such call"));
        assert_eq!(
            refused.to_string(),
            "unsupported: there is no such call",
            "the reason must reach the reader, not merely be stored"
        );
    }

    /// One pair, one entry. A duplicate would make [`divergence`] return the
    /// first and hide whichever reason somebody added second.
    #[test]
    fn no_pair_is_listed_twice() {
        let mut pairs: Vec<(Capability, String)> = DIVERGENCES
            .iter()
            .map(|entry| (entry.capability, entry.backend.to_string()))
            .collect();
        let count = pairs.len();
        pairs.sort_unstable();
        pairs.dedup();
        assert_eq!(
            pairs.len(),
            count,
            "a (capability, backend) pair is listed twice"
        );
    }

    /// The list is read by whoever is deciding whether a divergence should stay,
    /// so an entry that says nothing is an entry nobody reviewed.
    #[test]
    fn every_divergence_gives_a_real_reason() {
        for entry in DIVERGENCES {
            assert!(
                entry.why.len() > 40,
                "{} on {}: {:?} is too short to be a reason anyone can act on",
                entry.capability,
                entry.backend,
                entry.why
            );
        }
    }

    /// [`BackendKind::Null`] records rather than executes, so an entry naming it
    /// would be describing a recorder's limitation as a backend divergence.
    #[test]
    fn divergences_name_only_gpu_backends() {
        for entry in DIVERGENCES {
            assert!(
                entry.backend.is_gpu(),
                "{} names {}, which is not a GPU backend",
                entry.capability,
                entry.backend
            );
        }
    }

    /// A capability every GPU backend lacks is not a *divergence* — it is a seam
    /// behaviour nothing implements, and filing it as parity noise hides that.
    ///
    /// The other half too: [`divergence`] must find every entry the list holds,
    /// and must not invent one.
    #[test]
    fn a_divergence_is_something_some_backend_actually_has() {
        for capability in Capability::ALL {
            let lacking = GPU_BACKENDS
                .iter()
                .filter(|backend| divergence(*capability, **backend).is_some())
                .count();
            assert!(
                lacking < GPU_BACKENDS.len(),
                "{capability} is listed as absent on every GPU backend, so it is not a divergence \
                 — it is a seam behaviour nothing implements, and that belongs in the seam's own \
                 docs rather than in a parity exception list"
            );
        }

        for entry in DIVERGENCES {
            assert_eq!(
                divergence(entry.capability, entry.backend),
                Some(entry),
                "the lookup must find every entry the list holds"
            );
        }
        assert_eq!(
            divergence(Capability::BufferFillWord, BackendKind::Vulkan),
            None,
            "Vulkan writes a whole word, and the list would be describing it wrongly"
        );
        assert_eq!(
            divergence(Capability::PushConstants, BackendKind::Null),
            None
        );
    }

    /// The rule, checked on the three cases that decide it. Written as a table
    /// because the failure mode is an `&&` where an `||` belongs, which reads
    /// identically and makes the parity suite either vacuous or unpassable.
    #[test]
    fn a_parity_gap_is_an_unlisted_refusal_on_a_device_that_should_have_it() {
        let all = Features::all();

        // Listed, and the device has the flag: reviewed, not a gap.
        assert!(!is_parity_gap(
            Capability::DrawIndirectCount,
            BackendKind::Metal,
            all
        ));
        // Unlisted, and the device lacks the flag: the device is lesser, which
        // is `Features`' job and not this one's.
        assert!(!is_parity_gap(
            Capability::MeshShading,
            BackendKind::Vulkan,
            all.difference(Features::MESH_SHADER)
        ));
        // Unlisted, device has the flag: nobody wrote this down. A gap.
        assert!(is_parity_gap(
            Capability::MeshShading,
            BackendKind::Vulkan,
            all
        ));
        // Ungated and unlisted: always a gap, whatever the device reports.
        assert_eq!(Capability::BufferFillZero.gating_feature(), None);
        assert!(is_parity_gap(
            Capability::BufferFillZero,
            BackendKind::Vulkan,
            Features::empty()
        ));
        // Ungated and listed: never a gap.
        assert!(!is_parity_gap(
            Capability::BufferFillZero,
            BackendKind::Dx12,
            Features::empty()
        ));
    }
}
