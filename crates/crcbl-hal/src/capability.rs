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
//! and the answer it is forced to give names *whose* answer it is:
//! [`Support::Yes`], [`Support::No`] with a reason a human wrote, or
//! [`Support::NotOnThisDevice`] — which only [`Support::granted`] hands out, and
//! which says the refusal came from the device rather than from the backend.
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
//! is knowingly absent, and [`parity_verdict`] is the rule that reads it. It is
//! the difference between divergence somebody decided and divergence that
//! happened: a backend that starts refusing something new fails the parity test
//! until the pair is added with a reason, and a backend that starts supporting
//! something on the list fails until the pair is removed.
//!
//! # Why a refusal has to say whose it is
//!
//! The rule needs one more thing to be sound, and it is the reason
//! [`Support::NotOnThisDevice`] exists. "This device reports no `MESH_SHADER`"
//! and "this backend never wrote the mesh path" are both a refusal, and the
//! parity record must excuse the first and demand a row for the second — so the
//! rule used to guess which it was, by asking whether the *device* reported the
//! capability's [`gating_feature`](Capability::gating_feature).
//!
//! That guess made every gated row **unprovable**: on a device reporting the
//! flag clear, a pair was excused whether or not [`DIVERGENCES`] named it, so
//! deleting the row changed nothing anybody could observe and the capability
//! became one nobody claimed and nothing checked. Eight of `crcbl-mtl`'s nine
//! rows were that shape.
//!
//! A refusal now carries whose it is instead of having it inferred, and it costs
//! no backend a line to say so: [`Support::granted`] is the one constructor a
//! device gate is ever expressed through, so every gated arm in every backend
//! already routes through it. What a backend writes by hand is
//! [`Support::No`] — its own refusal, which [`parity_verdict`] demands a row for
//! on **every** device, including one that happens to withhold the flag.
//!
//! A reason alone cannot say **what is left**, though — "Metal's blit fill takes
//! a byte, not a word" and "this backend has not written the code yet" are the
//! same shape as prose. [`DivergenceKind`] is the field that separates them and
//! [`parity_blockers`] is the query over it: every row that somebody could still
//! close, on a backend crcbl is keeping. That set is what shrinks to nothing
//! when `crcbl-wgpu` can be deleted, and this module's tests snapshot it so it
//! shrinks deliberately.
//!
//! ```
//! use crcbl_hal::{BackendKind, Capability, DIVERGENCES, DivergenceKind, divergence, parity_blockers};
//!
//! // WebGPU has no mesh stage. No work in this repository closes that, so it
//! // is written down and then left alone.
//! let permanent = divergence(Capability::MeshShading, BackendKind::WebGpu)
//!     .expect("WebGPU's missing mesh stage is on the list");
//! assert_eq!(permanent.kind, DivergenceKind::ApiAbsence);
//! assert!(!permanent.kind.blocks_parity());
//!
//! // Metal's mesh stage is owed rather than absent, so it is one of the rows
//! // standing between crcbl and its end state.
//! assert!(parity_blockers().any(|entry| {
//!     entry.capability == Capability::MeshShading && entry.backend == BackendKind::Metal
//! }));
//!
//! // And Metal's GPU-side draw count is not, because that work landed — the
//! // list is a record of what is left, not of what was ever hard.
//! assert!(parity_blockers().all(|entry| {
//!     entry.capability != Capability::DrawIndirectCount || entry.backend != BackendKind::Metal
//! }));
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

            /// How the variant is spelled in source, e.g. `"BufferFillZero"`.
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

    /// [`clear_buffer`](crate::CommandEncoder::clear_buffer).
    ///
    /// **Every backend answers `Yes`, and it is kept anyway.** This enum covers
    /// what the seam does rather than only what backends disagree about, so a
    /// capability with no `DIVERGENCES` row is the shape a settled one takes —
    /// and the agnostic suite still drives it, which is what would notice a
    /// backend quietly dropping the call.
    ///
    /// It had two valued siblings until 2026-08-19,
    /// `BufferFillRepeatedByte` and `BufferFillWord`. They described how badly
    /// each backend could keep a promise the seam should not have made: Metal's
    /// fill repeats a byte and WebGPU has no valued fill at all, so three of
    /// five had to refuse most values. The verb lost its `value` instead — see
    /// [`clear_buffer`](crate::CommandEncoder::clear_buffer) — and both rows
    /// went with it.
    BufferFillZero,



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
    ///
    /// **WHICH depth format and WHICH direction remains the API's own rule.**
    /// This asks whether a backend has an expression for the copy at all; a
    /// backend answering [`Support::Yes`] still refuses the pairs its API's
    /// format table withholds, by name and at the call. WebGPU is the one that
    /// withholds any — and `wgpu` enforces the same table, so both backends on
    /// it agree: the depth plane of [`Format::D32Float`](crate::Format::D32Float),
    /// [`D32FloatS8Uint`](crate::Format::D32FloatS8Uint) and
    /// [`D16Unorm`](crate::Format::D16Unorm) copies **out** to a buffer, only
    /// `D16Unorm`'s copies back **in**, and
    /// [`D24UnormS8Uint`](crate::Format::D24UnormS8Uint)'s copies neither way,
    /// because WebGPU's `depth24plus` is whatever the driver chose to store and
    /// so has no memory layout to lay a buffer out against. The readback a
    /// shadow atlas needs is inside every one of those.
    DepthImageCopy,

    // --- render pass ---

    /// A [`ColorAttachment::resolve`](crate::ColorAttachment::resolve) view: the
    /// pass resolves its multisampled colour target into a single-sampled one as
    /// it ends.
    MsaaResolveAttachment,

    /// [`set_stencil_reference`](crate::CommandEncoder::set_stencil_reference) —
    /// the dynamic stencil reference value subsequent draws compare against.
    ///
    /// **The claim is the whole rule, not just that the call is wired.** A
    /// backend declaring this says the reference is pass state that survives a
    /// pipeline bind, and that a pass which never calls it draws against
    /// [`stencil::INITIAL_REFERENCE`](crate::stencil::INITIAL_REFERENCE); see
    /// [`StencilState`](crate::StencilState) on why there is no pipeline-side
    /// reference to compete with it.
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
    /// [`PassTimestampWrites`](crate::PassTimestampWrites) that fill it
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

    /// [`signal_semaphore`](crate::Device::signal_semaphore): the **CPU**
    /// advancing a timeline, rather than a submission.
    ///
    /// Its own capability for the same reason
    /// [`CpuTimelineWait`](Self::CpuTimelineWait) is: the two host-side halves
    /// are separable. A backend that emulates a timeline over per-submission
    /// completion could move a counter on demand and still have no queue-side
    /// wait to pair it with, and one with a real semaphore object has both.
    ///
    /// The claim is the observable one — that
    /// [`semaphore_value`](crate::Device::semaphore_value) reports the value the
    /// host asked for, and that a value which would move the timeline backwards
    /// is refused rather than accepted.
    CpuTimelineSignal,

    /// Waiting — from the CPU, or in a [`SubmitInfo`](crate::SubmitInfo) — on a
    /// timeline value that **nothing submitted so far will signal**, expecting
    /// a later signal to satisfy it.
    ///
    /// A real semaphore object blocks until somebody signals it, and
    /// [`CpuTimelineSignal`](Self::CpuTimelineSignal) is what lets the host be
    /// the somebody. A backend that emulates timelines over per-submission
    /// completion has no object for a future signal to arrive on, so such a wait
    /// could only ever stop the queue; refusing is the honest answer and it is a
    /// genuine behavioural divergence rather than a missing call.
    TimelineWaitBeforeSignal,
}

impl Capability {
    /// The [`Features`] bit a caller consults before asking for this, if there
    /// is one.
    ///
    /// **What makes the parity rule fair.** A backend that refuses because the
    /// device reports this flag clear has not diverged from anything — the
    /// device is simply lesser, which is exactly what [`Features`] already
    /// models and logs as a downgrade.
    ///
    /// It is the *claim* to be in that position that this flag settles.
    /// [`Support::granted`] is what puts a backend there, and
    /// [`parity_verdict`] checks the claim against the device before excusing
    /// it: a [`Support::NotOnThisDevice`] for a capability with no gate, or on a
    /// device that reports the gate, is
    /// [`ParityVerdict::FalseDeviceGate`]. What the flag no longer does is
    /// excuse a refusal the backend made on its own account, which is how the
    /// rule used to retire [`DIVERGENCES`] rows without anybody deciding to.
    ///
    /// `None` means no flag governs it, so every refusal is the backend's own.
    /// That is the majority here, and it is the measurement this module was
    /// built from: most refusal sites answer to no declared capability at all.
    #[must_use]
    pub const fn gating_feature(self) -> Option<Features> {
        match self {
            // Nothing in `Features` describes a buffer clear, which is why the
            // fills were the divergence that reached CI — and why the two
            // valued ones are gone: see `CommandEncoder::clear_buffer`.
            Self::BufferFillZero
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
            // All four are about one object, so one flag governs them.
            Self::TimelineSemaphore
            | Self::CpuTimelineWait
            | Self::CpuTimelineSignal
            | Self::TimelineWaitBeforeSignal => Some(Features::TIMELINE_SEMAPHORE),
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
/// Three arms, and the third is not a third *degree* of support: "maybe",
/// "partially" and "not yet" are all [`No`](Self::No) with a reason saying
/// which, because a caller can do nothing different with a shade of no. What
/// [`NotOnThisDevice`](Self::NotOnThisDevice) adds is **whose** no it is, which
/// is a different axis and one a caller can act on — a refusal the device
/// caused may be answered by opening a better adapter, and a refusal the backend
/// caused never is.
///
/// It is also what makes the parity record checkable; see the module docs.
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

    /// **The backend** refuses, for the reason given, and would refuse on any
    /// device.
    ///
    /// The reason is written for whoever reads the failure, so it names the
    /// obstacle — the API call that does not exist, the slice that has not
    /// landed — rather than restating the capability. `&'static str`, so asking
    /// costs no allocation.
    ///
    /// Every one of these needs a [`DIVERGENCES`] row, on every device. A
    /// backend reaching for this arm when the device is what withheld the
    /// capability has not cheated the rule so much as opted into a stricter one.
    No(&'static str),

    /// **The device** withheld the capability's
    /// [`gating_feature`](Capability::gating_feature), so the backend never got
    /// to say whether it has it.
    ///
    /// Handed out by [`granted`](Self::granted) and nowhere else in this
    /// workspace, because that is the one place a device gate is read. The
    /// distinction is not cosmetic: a lesser device is what [`Features`] already
    /// models and logs as a downgrade at device creation, and holding a software
    /// rasteriser to a mesh-shader capability would fail the parity suite for a
    /// reason nobody can fix.
    ///
    /// So this arm is *excused* by [`parity_verdict`] — which is exactly why it
    /// is checked rather than taken on trust: a backend claiming it for an
    /// ungated capability, or on a device that did report the flag, is
    /// [`ParityVerdict::FalseDeviceGate`] and fails.
    NotOnThisDevice(&'static str),
}

impl Support {
    /// [`Yes`](Self::Yes) when `features` holds `feature`, otherwise
    /// [`NotOnThisDevice`](Self::NotOnThisDevice) with `why`.
    ///
    /// The shape most arms of a backend's `match` have, written once here
    /// because five backends were otherwise going to spell the same `if` five
    /// ways — and because an arm that reads the *wrong* flag is the one mistake
    /// in this file a reviewer cannot see, so the flag belongs at the call site
    /// with nothing else beside it.
    ///
    /// **Being the only source of [`NotOnThisDevice`](Self::NotOnThisDevice) is
    /// what the parity rule rests on.** A backend does not have to know that:
    /// the arms it already routes through here are precisely the arms whose
    /// refusal belongs to the device, so the classification costs no backend a
    /// line and cannot be forgotten by one.
    ///
    /// `why` stays the caller's: which flag, and what the device would need to
    /// report, is the sentence the reader of a parity report needs.
    #[must_use]
    pub const fn granted(features: Features, feature: Features, why: &'static str) -> Self {
        if features.contains(feature) {
            Self::Yes
        } else {
            Self::NotOnThisDevice(why)
        }
    }

    /// Whether this is [`Support::Yes`].
    #[must_use]
    pub const fn is_yes(self) -> bool {
        matches!(self, Self::Yes)
    }

    /// The refusal reason, or `None` when supported.
    ///
    /// Both refusals answer, because a caller printing "why not" wants the
    /// sentence whichever of them it was.
    #[must_use]
    pub const fn reason(self) -> Option<&'static str> {
        match self {
            Self::Yes => None,
            Self::No(why) | Self::NotOnThisDevice(why) => Some(why),
        }
    }
}

impl fmt::Display for Support {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yes => f.write_str("supported"),
            Self::No(why) => write!(f, "unsupported: {why}"),
            Self::NotOnThisDevice(why) => write!(f, "unsupported on this device: {why}"),
        }
    }
}

/// Which kind of divergence an entry records — and so whether any amount of
/// work in this repository could ever close it.
///
/// **This is what makes the parity goal checkable.** Without it, "Metal's blit
/// fill takes a byte, not a word, and no work changes that", "this backend has
/// not written the code yet" and "it is written and no device here has run it"
/// are the same shape in the data, and nobody can answer "what is left?"
/// without re-reading every reason in [`DIVERGENCES`]. With it,
/// [`parity_blockers`] answers it — and the answer distinguishes work that
/// needs a programmer from work that needs a machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DivergenceKind {
    /// The backend's API cannot express it. **No work here closes it**, so it
    /// is not on anybody's list.
    ///
    /// The reason must carry the evidence: the selector and the type of the
    /// argument, the WebIDL member that is absent, the enumeration that has no
    /// such value. A backend's own comment is a claim rather than evidence — a
    /// row here is a permanent promise, and the two entries this field was
    /// introduced to settle were both backend comments contradicting the list.
    ApiAbsence,

    /// The API allows it and this crate has not written it.
    ///
    /// The reason says roughly what the work is, and by convention names the
    /// slice that owes it.
    Unwritten,

    /// The code is written and no device available here has executed it.
    ///
    /// **Distinct from [`Unwritten`](Self::Unwritten) because the work owed is
    /// different in kind.** An `Unwritten` row is closed by somebody writing
    /// code; one of these is closed by somebody *running* what is already
    /// there, on hardware this project does not have — a Metal 3 Mac, a device
    /// reporting a counter set, a D3D12 GPU that is not WARP. Collapsing the two
    /// makes the remaining distance to parity read as programming when it is
    /// procurement, which is the opposite of what a reader needs.
    ///
    /// It blocks parity exactly as `Unwritten` does. The rule that a row leaves
    /// [`DIVERGENCES`] only when a device has *run* the path is unchanged, and
    /// this kind is what lets a row say so instead of claiming work is owed that
    /// is already done.
    ///
    /// The reason must name the calls that exist, so the claim is checkable by
    /// reading them, and say what device would settle it.
    Unrun,
}

impl DivergenceKind {
    /// Whether a row of this kind stands between a backend and parity.
    ///
    /// Everything except [`ApiAbsence`](Self::ApiAbsence): an API with no
    /// expression for something is not a backlog item, while unwritten code is
    /// work owed.
    ///
    /// A `match` rather than a comparison against one variant, so a kind added
    /// later has to say which side of the goal it falls on.
    #[must_use]
    pub const fn blocks_parity(self) -> bool {
        match self {
            Self::ApiAbsence => false,
            Self::Unwritten | Self::Unrun => true,
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
    /// Whether anything in this repository could close it. See
    /// [`DivergenceKind`], and [`parity_blockers`] for what the field is for.
    pub kind: DivergenceKind,
    /// Why, in enough detail to decide whether it should stay that way. This is
    /// the sentence a reviewer reads when the pair is added.
    pub why: &'static str,
}

/// Metal's answer for [`Capability::DrawIndirectCount`] on a device that does
/// not report the flag — in `crcbl-mtl`'s `Device::supports` and in the error
/// its encoder refuses with alike.
///
/// **One sentence in two places on purpose**, the shape a backend uses when a
/// refusal has to survive the trip from `Device::supports` to the error a
/// caller reads.
///
/// **It used to be a row in [`DIVERGENCES`], and that row is gone because the
/// work landed.** For three revisions this constant described an
/// `MTLIndirectCommandBuffer` written by a compute kernel — the design that was
/// attempted three times and hung the GPU in a frame every time. What
/// `crcbl-mtl` ships instead needs no such object: a draw of **zero instances**
/// renders nothing, so a kernel that packs the argument structures and leaves
/// every one at or past the GPU-written count with no instances turns a
/// count-limited draw into `max_draw_count` ordinary indirect draws. The
/// capability is answered through [`Support::granted`] now, so this is the
/// sentence a device that withheld
/// [`Features::DRAW_INDIRECT_COUNT`](crate::Features::DRAW_INDIRECT_COUNT)
/// would carry — which, the gate being unconditional, is a device somebody
/// deliberately reverted the slice for.
pub const METAL_NO_DRAW_INDIRECT_COUNT: &str = "this device reports no DRAW_INDIRECT_COUNT. \
     crcbl-mtl's count-limited draw is a compute kernel that packs the argument structures and \
     zeroes the instance count of every one at or past the GPU-written count, followed by \
     max_draw_count ordinary indirect draws — so the flag is unconditional on Metal and a device \
     without it is one the slice was switched off for at crcbl_mtl::adapter's features_of";

/// The browser's answer for [`Capability::UpdateBindGroup`], in the parity
/// record and in `crcbl-webgpu`'s own declaration alike.
///
/// The second sentence that had drifted, and the same treatment as
/// [`METAL_NO_DRAW_INDIRECT_COUNT`]. This list used to say the stream "has no
/// update_bind_group command **yet**", which reads as schedulable work, while
/// the backend said a bind group is immutable and the stream "could not carry
/// one that worked". WebGPU settles it: `GPUBindGroup` exposes a label and
/// nothing else, so there is no mutation to encode and never will be.
pub const WEBGPU_BIND_GROUPS_ARE_IMMUTABLE: &str = "WebGPU bind groups are immutable once created \
     — GPUBindGroup exposes a label and nothing else — so the stream has no update_bind_group \
     command and could not carry one that worked";

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
/// Every row also carries a [`DivergenceKind`], which is what separates the
/// rows that are somebody's work from the rows no work can touch;
/// [`parity_blockers`] is the query that reads it.
///
/// A refusal that is merely *this device's* — the backend answered
/// [`Support::NotOnThisDevice`] because the gating [`Features`] flag is clear —
/// is not listed and is not a gap; see [`parity_verdict`]. A backend's own
/// refusal needs a row here whatever the device reports, which is what stops a
/// row being retired by a device that could not have proved anything either way.
///
/// Entries are grouped by capability. [`BackendKind::Null`] never appears: it
/// records rather than executes, so it is not part of the parity model — see
/// [`BackendKind::is_gpu`].
pub const DIVERGENCES: &[Divergence] = &[
    // --- fills: the worked example, and why there are three variants ---
    //
    // `BufferFillZero` on Dx12 is not here, and its absence is what the list is
    // for: the row left because the work landed. `crcbl_dx12`'s `fill_buffer`
    // copies out of a zeroed device-local resource, which is `wgpu-hal`'s dx12
    // answer to the same call, so the backend that had every fill row now has
    // only the two a *value* has to travel through.
    // --- draws ---
    //
    // `DrawIndirectCount` on Metal is not here, and its absence is the second
    // worked example: the row left because the work landed. Metal still has no
    // count-buffer draw — that never changes — but it does not need one. A draw
    // of zero instances renders nothing, so `crcbl_mtl::indirect_count` packs
    // the argument structures with a compute kernel, gives every structure at
    // or past the GPU-written count no instances, and issues `max_draw_count`
    // ordinary indirect draws. See [`METAL_NO_DRAW_INDIRECT_COUNT`], which the
    // backend still shares for the device-gated refusal.
    Divergence {
        capability: Capability::DrawIndirectCount,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: "GPURenderPassEncoder carries drawIndirect and drawIndexedIndirect and no \
              count-buffer form of either, so the draw count can only come from the CPU; the \
              stream has no tag for one because there is nothing to encode",
    },
    Divergence {
        capability: Capability::IndirectArgumentPaddedStride,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: "GPURenderPassEncoder.drawIndirect(indirectBuffer, indirectOffset) reads one tightly \
              packed argument structure and has no stride parameter to honour",
    },
    Divergence {
        capability: Capability::MeshShading,
        backend: BackendKind::Metal,
        kind: DivergenceKind::Unrun,
        why: "the calls exist — crcbl_mtl::pipeline fills an MTLMeshRenderPipelineDescriptor with \
              the object, mesh and fragment functions and crcbl_mtl::command records \
              drawMeshThreadgroups:threadsPerObjectThreadgroup:threadsPerMeshThreadgroup: and its \
              indirect twin — but no device has ever run them, so crcbl_mtl::adapter reports no \
              Features::MESH_SHADER and the capability cannot answer Yes. Mesh shading is a Metal \
              3 feature gated on supportsFamily:MTLGPUFamilyMetal3, and the Mac CI runs this \
              backend on answers false to Metal3 and to every Apple family above 5 — measured by \
              a_device_reports_its_indirect_command_buffer_support_and_draw_indirect_count_ceiling. \
              Retiring this row takes a Metal 3 Mac running crcbl_mtl's mesh path and the flag \
              being reported off the back of it",
    },
    Divergence {
        capability: Capability::MeshShading,
        backend: BackendKind::Dx12,
        kind: DivergenceKind::Unwritten,
        why: "the calls exist — crcbl_dx12::pipeline packs the D3D12_PIPELINE_STATE_STREAM_DESC an \
              amplification and mesh stage need, and the encoder records DispatchMesh and an \
              ExecuteIndirect of DISPATCH_MESH — but crcbl_dx12::adapter does not report \
              Features::MESH_SHADER, so the capability cannot answer Yes. Reporting it moves every \
              D3D12 adapter onto GeometryPath::MeshShader and re-keys every golden image, which is \
              its own change (the DX12 mesh reporting slice)",
    },
    Divergence {
        capability: Capability::MeshShading,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: "WebGPU has no mesh stage: GPUDevice creates render and compute pipelines only and \
              GPURenderPassEncoder has no draw for one. No proposal for it has reached the \
              specification, so this is the API rather than the slice",
    },
    Divergence {
        capability: Capability::TaskShaderStage,
        backend: BackendKind::Metal,
        kind: DivergenceKind::Unrun,
        why: "MeshPipelineDesc::task reaches Metal's object stage through the same descriptor the \
              mesh one does — setObjectFunction: beside setMeshFunction: — and it is behind the \
              same unreported flag and the same unrun code; see the MeshShading entry",
    },
    Divergence {
        capability: Capability::TaskShaderStage,
        backend: BackendKind::Dx12,
        kind: DivergenceKind::Unwritten,
        why: "MeshPipelineDesc::task reaches D3D12's amplification stage through the same \
              subobject stream the mesh one does, and it is behind the same unreported flag; see \
              the MeshShading entry",
    },
    Divergence {
        capability: Capability::TaskShaderStage,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: "WebGPU has no mesh stage to put an amplification stage in front of; see the \
              MeshShading entry",
    },
    // --- bindings ---
    Divergence {
        capability: Capability::UpdateBindGroup,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: WEBGPU_BIND_GROUPS_ARE_IMMUTABLE,
    },
    Divergence {
        capability: Capability::PushConstants,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: "WebGPU has no push constants — GPUPipelineLayoutDescriptor carries bind group \
              layouts and nothing beside them. The substitute is a dynamic-offset uniform buffer, \
              which the seam already carries as bind-group dynamic offsets",
    },
    // The Metal row that used to sit here is gone, and it left the way a row is
    // meant to: `crcbl-mtl` binds a VARIABLE_COUNT slot as one argument buffer
    // of `MTLBuffer::gpuAddress` values and keeps its contents resident with
    // `useResource:`, so the backend answers `Support::Yes` wherever the device
    // reports `DESCRIPTOR_INDEXING`.
    Divergence {
        capability: Capability::BindlessDescriptorArray,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: "GPUBindGroupLayoutEntry carries a binding, a visibility and one resource layout and \
              no count, so WebGPU has no binding arrays at all, fixed-size or runtime-sized; a \
              material table is indexed through a storage buffer here rather than through \
              descriptors",
    },
    // The WebGPU row that used to sit here is gone: the gap was the seam's own
    // descriptor, and `BindingKind::StorageImage` now carries the `view_type`
    // and `format` `GPUStorageTextureBindingLayout` demands, so `crcbl-webgpu`
    // answers `Support::Yes`. What is left is the backend nobody is finishing.
    // --- rasteriser state ---
    //
    // Only one entry, and that is the point: `PolygonModeLine`, `DepthClamp` and
    // `SamplerAnisotropy` are refused by every backend *only* where the device
    // reports the flag clear, which is `Features` doing its job. WebGPU is the
    // exception because it has no wireframe on any device it could ever open.
    Divergence {
        capability: Capability::PolygonModeLine,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: "GPUPrimitiveState carries topology, stripIndexFormat, frontFace, cullMode and \
              unclippedDepth and no fill mode, so WebGPU has no core expression for a wireframe; \
              unlike the other two rasteriser capabilities this is absent from the API rather than \
              from a given device",
    },
    // --- queries ---
    Divergence {
        capability: Capability::TimestampQuery,
        backend: BackendKind::Metal,
        // Unclassified until 2026-08-19, when the probe adapter.rs was written
        // to answer it with ran on CI's Apple Paravirtual device:
        // supportsCounterSampling AtStageBoundary=false, AtDrawBoundary=true,
        // AtDispatchBoundary=true, AtBlitBoundary=true, counterSets=0, and
        // sampleTimestamps:gpuTimestamp: not moving across a 50ms sleep. Metal
        // expresses the feature, so this was never an ApiAbsence. The code was
        // then written, and the row stays Unwritten for the reason the
        // MeshShading entry above stays Unwritten: no device has executed it.
        kind: DivergenceKind::Unrun,
        why: "the calls exist — crcbl_mtl::device's create_query_set builds an \
              MTLCounterSampleBuffer over MTLCommonCounterSetTimestamp, crcbl_mtl::command puts it \
              in a render or compute pass descriptor's sampleBufferAttachments at the two indices \
              PassTimestampWrites names, resolve_query_set reaches it through the blit encoder's \
              resolveCounters:inRange:destinationBuffer:destinationOffset:, and query_results \
              reads it with resolveCounterRange: and converts to nanoseconds — but no device has \
              ever run them. crcbl_mtl::adapter reports Features::TIMESTAMP_QUERY only for a \
              device that advertises MTLCommonCounterSetTimestamp in MTLDevice::counterSets and \
              answers supportsCounterSampling: at MTLCounterSamplingPointAtStageBoundary, which is \
              the point a pass descriptor samples at and therefore the question the code depends \
              on — not supportsFamily:, which describes a feature set rather than a selector's \
              availability. The Mac CI runs this backend on answers counterSets=0 and \
              AtStageBoundary=false, measured by \
              a_device_reports_its_counter_sampling_gpu_families_and_timestamp_correlation, so it \
              reports the flag clear and every query path degrades there — a device fact, reported \
              per device, and the gate working rather than what leaves the row open. Metal states \
              no tick period at all, so the conversion is two sampleTimestamps:gpuTimestamp: \
              correlations — one at device open, one at the read — and crcbl_mtl::query's \
              timestamp_nanos is the arithmetic, unit-tested off macOS because it is the only part \
              of the path a machine without Metal can check. Nothing has ever checked it against a \
              real GPU clock. Retiring this row takes a Mac that reports the flag running \
              crcbl_mtl's timestamp path and the numbers coming back ordered and non-zero",
    },
    // The WebGPU TimestampQuery row that used to sit here is gone, and it left
    // the way `StorageImageBinding`'s did: the gap was the seam's own verb. It
    // had a free-standing `write_timestamp` naming an arbitrary point in the
    // command stream, which WebGPU has no expression for at all, so
    // `create_query_set` refused a timestamp set rather than hand out a handle a
    // profiler would fill with zeros. `PassTimestampWrites` is
    // `GPURenderPassDescriptor.timestampWrites`' own shape, so the browser
    // backend passes it straight through and answers `Support::Yes`. `crcbl-webgpu`
    // now diverges from nothing.
    Divergence {
        capability: Capability::PipelineStatisticsQuery,
        backend: BackendKind::Metal,
        // Reclassified with TimestampQuery on 2026-08-19, from the same probe
        // output, and written with it. It stays Unwritten for the same reason —
        // no device has run it — plus one this backend cannot fix on its own:
        // see the second half of the reason.
        kind: DivergenceKind::Unrun,
        why: "the calls exist — crcbl_mtl::device's create_query_set builds an \
              MTLCounterSampleBuffer over MTLCommonCounterSetStatistic and crcbl_mtl::command's \
              resolve_query_set reaches it through the blit encoder's \
              resolveCounters:inRange:destinationBuffer:destinationOffset: at the \
              MTLCounterResultStatistic width crcbl_mtl::query derives — but no device has ever \
              run them, and two of the seam's read paths cannot be written at all. **Nothing can \
              sample one**: PassTimestampWrites names timestamps and crcbl_hal::CommandEncoder has \
              no other query verb, so no work a caller records will ever write into this set — \
              which is exactly what Capability::OcclusionQuery means on every backend, and why \
              crcbl_mtl::device's supports claims the create, the resolve and the destroy and \
              nothing more. **And query_results refuses it**: an MTLCounterResultStatistic is \
              eight u64s while that call reads one u64 per query, so there is nowhere for the \
              other seven to go, and returning the first would answer a different question in the \
              shape of this one; crcbl-dx12 refuses the identical read of an eleven-field \
              D3D12_QUERY_DATA_PIPELINE_STATISTICS in the same words, and the fix is a seam that \
              carries a result width. crcbl_mtl::adapter reports \
              Features::PIPELINE_STATISTICS_QUERY for a device advertising \
              MTLCommonCounterSetStatistic, and gates it on that alone rather than on \
              supportsCounterSampling:, because nothing samples this kind and a gate on an answer \
              no line reads decides nothing. The Mac in CI advertises counterSets=0, so it reports \
              the flag clear; that is a device fact reported per device, not what leaves the row \
              open. Retiring this row takes a Mac that reports the flag creating and resolving a \
              set, as the occlusion kind's absence from this list took",
    },
    Divergence {
        capability: Capability::PipelineStatisticsQuery,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: "GPUQueryType is exactly 'occlusion' and 'timestamp', so there is no \
              pipeline-statistics query set for WebGPU to create",
    },
    // --- synchronisation ---
    Divergence {
        capability: Capability::TimelineSemaphore,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: "WebGPU has no semaphores. It orders submissions implicitly — one queue, executed in \
              order, hazards tracked by the browser — and its only completion signal, \
              GPUQueue.onSubmittedWorkDone(), resolves for everything submitted so far and carries \
              no value, so nothing there could drive a counter. create_semaphore refuses \
              SemaphoreKind::Timeline; it used to hand out a handle whose semaphore_value answered \
              0 for ever, which is the succeed-while-doing-nothing shape this enum exists to make \
              visible",
    },
    Divergence {
        capability: Capability::CpuTimelineWait,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: "create_semaphore refuses the timeline kind, so there is no counter for a CPU wait to \
              read; the binary semaphore this backend does hand out is GPU-waitable only. \
              wait_semaphores refuses rather than answering Ok(true) for a wait it never evaluated \
              — see the TimelineSemaphore entry",
    },
    Divergence {
        capability: Capability::CpuTimelineSignal,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: "create_semaphore refuses the timeline kind, so there is no counter for a host signal \
              to advance; WebGPU has no signalable object of any kind — see the TimelineSemaphore \
              entry",
    },
    Divergence {
        capability: Capability::TimelineWaitBeforeSignal,
        backend: BackendKind::WebGpu,
        kind: DivergenceKind::ApiAbsence,
        why: "a wait of any kind is refused here, so a wait on a value nothing has signalled is \
              refused with the rest; see the CpuTimelineWait entry",
    },
];

/// Every divergence that stands between a backend crcbl is **keeping** and
/// parity.
///
/// The answer to "what is left?", derived rather than maintained. A row is here
/// when its backend [drives a GPU](BackendKind::is_gpu) and its
/// [`kind`](Divergence::kind) [blocks parity](DivergenceKind::blocks_parity) —
/// so `crcbl-wgpu`, which is deleted once `crcbl-webgpu` replaces it, is
/// excluded by the first half rather than by a reader remembering to skip it,
/// and [`DivergenceKind::ApiAbsence`] by the second.
///
/// The set is snapshotted by this module's own tests, so it shrinks only when
/// somebody updates the snapshot deliberately. Yields in [`DIVERGENCES`] order.
pub fn parity_blockers() -> impl Iterator<Item = &'static Divergence> {
    DIVERGENCES
        .iter()
        .filter(|entry| entry.backend.is_gpu() && entry.kind.blocks_parity())
}

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

/// What the parity record has to say about one capability on one running
/// device — the outcome [`parity_verdict`] answers with.
///
/// Five outcomes rather than a `bool`, because "not a gap" used to cover two
/// unlike things: a divergence somebody reviewed, and a pair this device could
/// not settle either way. Folding them made a retirement free — see
/// [`UnprovableHere`](Self::UnprovableHere).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParityVerdict {
    /// The backend performs it here. Nothing is owed, and a [`DIVERGENCES`] row
    /// for the pair would be stale.
    Supported,

    /// The backend refuses and [`DIVERGENCES`] says why. The reviewed case, and
    /// the entry is carried so a report can print the sentence.
    Reviewed(&'static Divergence),

    /// The device withheld the gating [`Features`] flag, so this run learned
    /// **nothing** about the backend — neither that it has the capability nor
    /// that it lacks it.
    ///
    /// Not a failure: the device is lesser, which `Features` already models and
    /// logs as a downgrade at creation, and there is no work anybody could do to
    /// make a software rasteriser grow a mesh stage.
    ///
    /// It is reported by name all the same, because "unprovable here" is a
    /// coverage gap and used to be indistinguishable from a clean pass. That
    /// indistinguishability is the whole defect this enum was split to fix: a
    /// gated pair reached it whether or not [`DIVERGENCES`] named the pair, so a
    /// row could be deleted and no run anywhere would notice.
    UnprovableHere(Features),

    /// **The backend's own refusal, with nobody's name on it.** The gap.
    ///
    /// [`Support::No`] means this backend would refuse on every device, so no
    /// device's [`Features`] excuse it: either a row was never written, or one
    /// was retired without the backend having learnt to do the thing.
    Unreviewed,

    /// The backend answered [`Support::NotOnThisDevice`] and the device did not
    /// withhold anything — the capability has no
    /// [`gating_feature`](Capability::gating_feature), or it has one and this
    /// device reports it.
    ///
    /// A gap, and a louder one than [`Unreviewed`](Self::Unreviewed): the
    /// excused arm is the one thing a backend could hide behind, so it is the
    /// one arm whose claim is checked against the device rather than believed.
    FalseDeviceGate,
}

impl ParityVerdict {
    /// Whether this verdict is a **parity gap** — something nobody has accounted
    /// for.
    ///
    /// The two failing outcomes and no others. [`UnprovableHere`] is deliberately
    /// not one: a device that cannot answer is not a defect in the backend.
    ///
    /// [`UnprovableHere`]: Self::UnprovableHere
    #[must_use]
    pub const fn is_gap(self) -> bool {
        match self {
            Self::Supported | Self::Reviewed(_) | Self::UnprovableHere(_) => false,
            Self::Unreviewed | Self::FalseDeviceGate => true,
        }
    }
}

/// What the parity record says about `capability` on `backend`, given what the
/// backend `declared` on a device reporting `features`.
///
/// The rule the agnostic seam suite applies to every capability of a running
/// device, and the whole of what "reviewed divergence" means here:
///
/// * [`Support::Yes`] is [`Supported`](ParityVerdict::Supported). The backend
///   does it, and the suite's other half holds it to that by driving the call.
/// * [`Support::NotOnThisDevice`] is
///   [`UnprovableHere`](ParityVerdict::UnprovableHere) — **if the device really
///   did withhold the gate**, which is checked here rather than assumed.
///   Otherwise the backend blamed a device that gave it what it asked for, and
///   that is [`FalseDeviceGate`](ParityVerdict::FalseDeviceGate).
/// * [`Support::No`] that [`DIVERGENCES`] names is
///   [`Reviewed`](ParityVerdict::Reviewed). Somebody decided it and wrote down
///   why.
/// * [`Support::No`] that it does not name is
///   [`Unreviewed`](ParityVerdict::Unreviewed): this backend cannot do something
///   its peers can and nobody has said so. **The device's `features` do not
///   enter into it** — that is the fix. The old rule waived a refusal whenever
///   the device reported the gate clear, which waived it for backends that had
///   never looked at the gate, and so retired their rows for them.
///
/// Deliberately blind to [`DivergenceKind`]: a written reason is what makes a
/// refusal reviewed, whatever kind of divergence it turned out to be. "Which of
/// them is still somebody's work" is the other question, and
/// [`parity_blockers`] is where it is asked.
#[must_use]
pub fn parity_verdict(
    capability: Capability,
    backend: BackendKind,
    declared: Support,
    features: Features,
) -> ParityVerdict {
    match declared {
        Support::Yes => ParityVerdict::Supported,
        Support::NotOnThisDevice(_) => match capability.gating_feature() {
            Some(gate) if !features.contains(gate) => ParityVerdict::UnprovableHere(gate),
            _ => ParityVerdict::FalseDeviceGate,
        },
        Support::No(_) => match divergence(capability, backend) {
            Some(entry) => ParityVerdict::Reviewed(entry),
            None => ParityVerdict::Unreviewed,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every GPU backend, for the tests that must sweep them. Written out rather
    /// than derived, so that a new [`BackendKind`] fails
    /// [`every_gpu_backend_is_in_this_sweep`] instead of silently escaping the
    /// parity model.
    const GPU_BACKENDS: [BackendKind; 4] = [
        BackendKind::Vulkan,
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
        // so a fifth backend makes this fail instead of quietly sitting outside
        // every sweep below.
        let every = [
            BackendKind::Vulkan,
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

    /// The one constructor a device gate is read through, and the only source of
    /// [`Support::NotOnThisDevice`] — so this asserts the *variant*, not merely
    /// that it said no. A `granted` that answered [`Support::No`] would put every
    /// lesser device back in front of [`ParityVerdict::Unreviewed`].
    #[test]
    fn granted_reads_the_flag_it_was_given() {
        let device = Features::COMPUTE | Features::TIMELINE_SEMAPHORE;
        assert_eq!(
            Support::granted(device, Features::COMPUTE, "no compute"),
            Support::Yes
        );
        assert_eq!(
            Support::granted(device, Features::MESH_SHADER, "no mesh shader"),
            Support::NotOnThisDevice("no mesh shader")
        );
        // And it is `contains`, not `intersects`: a capability gated on a flag
        // the device half-has would otherwise report supported.
        assert_eq!(
            Support::granted(device, Features::GPU_DRIVEN, "not the whole bundle"),
            Support::NotOnThisDevice("not the whole bundle")
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

        // The two refusals read differently, because a reader deciding whether
        // to try another adapter needs to know which one this was.
        let withheld = Support::NotOnThisDevice("this device reports no MESH_SHADER");
        assert!(!withheld.is_yes());
        assert_eq!(
            withheld.reason(),
            Some("this device reports no MESH_SHADER")
        );
        assert_eq!(
            withheld.to_string(),
            "unsupported on this device: this device reports no MESH_SHADER"
        );
        assert_ne!(
            withheld,
            Support::No("this device reports no MESH_SHADER"),
            "the same sentence in the two arms must not compare equal, or the parity rule is \
             reading a string rather than a classification"
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
            divergence(
                Capability::IndirectArgumentPaddedStride,
                BackendKind::Vulkan
            ),
            None,
            "Vulkan takes a padded stride, and the list would be describing it wrongly"
        );
        assert_eq!(
            divergence(Capability::PushConstants, BackendKind::Null),
            None
        );
    }

    /// The rule, checked on every case that decides it. Written as a table
    /// because the failure mode is an `&&` where an `||` belongs, which reads
    /// identically and makes the parity suite either vacuous or unpassable.
    #[test]
    fn a_parity_gap_is_a_backend_refusal_nobody_wrote_down() {
        let all = Features::all();
        let no_mesh = all.difference(Features::MESH_SHADER);
        let refused = Support::No("this backend has not written it");
        let withheld = Support::NotOnThisDevice("this device reports no MESH_SHADER");

        // Supported: nothing owed, whatever the list says.
        assert_eq!(
            parity_verdict(
                Capability::MeshShading,
                BackendKind::Vulkan,
                Support::Yes,
                all
            ),
            ParityVerdict::Supported
        );
        // A backend refusal the list names: reviewed.
        assert_eq!(
            parity_verdict(
                Capability::IndirectArgumentPaddedStride,
                BackendKind::WebGpu,
                refused,
                all
            ),
            ParityVerdict::Reviewed(
                divergence(
                    Capability::IndirectArgumentPaddedStride,
                    BackendKind::WebGpu
                )
                .expect("WebGPU's packed-only indirect stride is on the list")
            )
        );
        // The device withheld the gate: this run proves nothing either way, and
        // saying so is not a failure.
        assert_eq!(
            parity_verdict(
                Capability::MeshShading,
                BackendKind::Vulkan,
                withheld,
                no_mesh
            ),
            ParityVerdict::UnprovableHere(Features::MESH_SHADER)
        );
        // A backend refusal nobody wrote down: a gap.
        assert_eq!(
            parity_verdict(Capability::MeshShading, BackendKind::Vulkan, refused, all),
            ParityVerdict::Unreviewed
        );

        // **The hole this rule was rewritten to close.** The same unlisted
        // backend refusal on a device that withheld the gate: the old rule
        // waived it, so retiring the row cost nothing and no run anywhere
        // noticed. It is the same gap it is on a device that reports the flag.
        assert_eq!(
            parity_verdict(
                Capability::MeshShading,
                BackendKind::Vulkan,
                refused,
                no_mesh
            ),
            ParityVerdict::Unreviewed,
            "a backend's own refusal is a gap on every device; a lesser one cannot excuse it"
        );

        // And the escape hatch is checked rather than believed, in both of its
        // failure shapes: an ungated capability has no device to blame, and a
        // device that reported the flag did not withhold it.
        assert_eq!(Capability::BufferFillZero.gating_feature(), None);
        assert_eq!(
            parity_verdict(
                Capability::BufferFillZero,
                BackendKind::Dx12,
                withheld,
                Features::empty()
            ),
            ParityVerdict::FalseDeviceGate
        );
        assert_eq!(
            parity_verdict(Capability::MeshShading, BackendKind::Vulkan, withheld, all),
            ParityVerdict::FalseDeviceGate
        );

        // Ungated and unlisted: always a gap, whatever the device reports.
        assert!(
            parity_verdict(
                Capability::BufferFillZero,
                BackendKind::Vulkan,
                refused,
                Features::empty()
            )
            .is_gap()
        );
        // Ungated and listed: never a gap. The indirect stride, because
        // nothing in `Features` describes it and WebGPU's row is permanent.
        assert!(
            !parity_verdict(
                Capability::IndirectArgumentPaddedStride,
                BackendKind::WebGpu,
                refused,
                Features::empty()
            )
            .is_gap()
        );
    }

    /// Which verdicts fail, as a table: the failure mode is a variant landing on
    /// the wrong side, which would either make the suite unpassable or make the
    /// whole report advisory.
    #[test]
    fn only_an_unaccounted_refusal_is_a_gap() {
        let entry = &DIVERGENCES[0];
        assert!(!ParityVerdict::Supported.is_gap());
        assert!(!ParityVerdict::Reviewed(entry).is_gap());
        assert!(!ParityVerdict::UnprovableHere(Features::MESH_SHADER).is_gap());
        assert!(ParityVerdict::Unreviewed.is_gap());
        assert!(ParityVerdict::FalseDeviceGate.is_gap());
    }

    /// **Every row in the list is one a running backend can be held to.**
    ///
    /// The property the rule rewrite buys, asserted over the data rather than
    /// argued in a doc comment: a row exists to excuse a [`Support::No`], and
    /// [`parity_verdict`] answers [`ParityVerdict::Reviewed`] for that pair on
    /// **every** device, including one reporting nothing at all. So deleting any
    /// row turns that same pair into [`ParityVerdict::Unreviewed`] on the
    /// backend's own CI arm — there is no device on which the deletion passes
    /// unnoticed, which is what "a retirement must be earned" means here.
    #[test]
    fn no_row_can_be_retired_without_a_run_noticing() {
        let refused = Support::No("the refusal this row exists to excuse");
        for entry in DIVERGENCES {
            for features in [Features::empty(), Features::all()] {
                assert_eq!(
                    parity_verdict(entry.capability, entry.backend, refused, features),
                    ParityVerdict::Reviewed(entry),
                    "{} on {}: the row must excuse the refusal on every device",
                    entry.capability,
                    entry.backend
                );
                // The counterfactual — that same refusal with no row behind it.
                // `divergence` is the only thing the rule reads for a
                // `Support::No`, so an unlisted pair *is* what a deleted row
                // leaves. Vulkan stands in for one because the list names it
                // nowhere, which is checked here rather than remembered.
                assert_eq!(
                    divergence(entry.capability, BackendKind::Vulkan),
                    None,
                    "the list has grown a Vulkan row, so it is no longer the unlisted backend this \
                     counterfactual needs"
                );
                assert_eq!(
                    parity_verdict(entry.capability, BackendKind::Vulkan, refused, features),
                    ParityVerdict::Unreviewed,
                    "{}: an unlisted backend refusal must be a gap on every device, or deleting \
                     the row above would cost nothing",
                    entry.capability
                );
            }
        }
    }

    /// Every divergence standing between crcbl and its stated end state, as
    /// reviewed — the whole of what "reach parity, then delete `crcbl-wgpu`"
    /// still asks for. **Update this deliberately.**
    ///
    /// A snapshot rather than a rule, and that is the point: a row leaves only
    /// when somebody writes the code, overturns the decline, or shows the API
    /// cannot express it after all. Each of those is a review, and a test that
    /// merely counted the rows — or checked that each had *some* kind — would
    /// pass whether the classification were right or wrong.
    const REVIEWED_BLOCKERS: &[(Capability, BackendKind, DivergenceKind)] = &[
        // `crcbl-dx12` is the whole of its own list: D3D12 expresses every
        // capability here, and the two *valued* fills are the only rows anybody
        // chose. `BufferFillZero` was a third and left the way a row is meant
        // to: the backend fills to zero now, by copying out of a zeroed
        // resource.
        (
            Capability::MeshShading,
            BackendKind::Dx12,
            DivergenceKind::Unwritten,
        ),
        (
            Capability::TaskShaderStage,
            BackendKind::Dx12,
            DivergenceKind::Unwritten,
        ),
        // Metal: the byte-wide fill is the API and is not here, and neither is
        // the occlusion query — that pool is a plain MTLBuffer and `crcbl-mtl`
        // builds it. `DrawIndirectCount` is not here either, and that one left
        // the way a row is meant to: `crcbl_mtl::indirect_count` packs the
        // argument structures with a compute kernel and draws
        // `max_draw_count` times, so the count is honoured with no indirect
        // command buffer anywhere.
        //
        // The two counter-sampled query rows were `Unclassified` until
        // 2026-08-19, on the grounds that settling them needed a measurement
        // this workspace could not take. `crcbl_mtl::adapter`'s probe has since
        // taken it, on CI: `AtStageBoundary=false`, `AtDrawBoundary=true`,
        // `counterSets=0`. Metal expresses both features, so neither is an
        // `ApiAbsence` and both are `Unwritten`. **The blocker count did not
        // move** — an unanswered question and unwritten work both block parity,
        // and reclassifying one as the other is honesty about what is owed,
        // not progress against it.
        (
            Capability::MeshShading,
            BackendKind::Metal,
            DivergenceKind::Unrun,
        ),
        (
            Capability::TaskShaderStage,
            BackendKind::Metal,
            DivergenceKind::Unrun,
        ),
        (
            Capability::TimestampQuery,
            BackendKind::Metal,
            DivergenceKind::Unrun,
        ),
        (
            Capability::PipelineStatisticsQuery,
            BackendKind::Metal,
            DivergenceKind::Unrun,
        ),
        // `crcbl-webgpu` is not on this list at all, and that is the point:
        // everything WebGPU refuses is WebGPU itself refusing, which is an
        // `ApiAbsence` and not a blocker. The browser backend had two rows and
        // both left the way a row is supposed to — the work landed.
        // `StorageImageBinding` went when `BindingKind::StorageImage` grew its
        // `view_type` and `format`; `TimestampQuery` went when the seam's
        // free-standing `write_timestamp` became `PassTimestampWrites` on the
        // pass descriptor, which is `timestampWrites`' own shape.
    ];

    /// The one that makes the goal checkable: what
    /// [`parity_blockers`] answers must be what somebody reviewed, row for row.
    ///
    /// Fails when a row joins or leaves the blocker set, when a blocking row is
    /// reclassified, and when a row is moved to another backend — each of which
    /// is a change to "what is left" and none of which should reach `main`
    /// without a human editing [`REVIEWED_BLOCKERS`] to match.
    #[test]
    fn the_parity_blockers_are_exactly_the_reviewed_list() {
        let mut actual: Vec<(Capability, String, DivergenceKind)> = parity_blockers()
            .map(|entry| (entry.capability, entry.backend.to_string(), entry.kind))
            .collect();
        actual.sort_unstable();
        let mut reviewed: Vec<(Capability, String, DivergenceKind)> = REVIEWED_BLOCKERS
            .iter()
            .map(|(capability, backend, kind)| (*capability, backend.to_string(), *kind))
            .collect();
        reviewed.sort_unstable();
        assert_eq!(
            actual, reviewed,
            "the set of divergences between crcbl and parity has changed. That is the goal moving, \
             so say so: update REVIEWED_BLOCKERS to match, and if a row left the set, make sure it \
             left because the work landed or the API was shown to lack it — not because a kind was \
             widened to ApiAbsence to make it disappear"
        );
    }

    /// **Every backend in the model is now a parity target, and the filter that
    /// said otherwise is gone.**
    ///
    /// `parity_blockers` used to filter on `BackendKind::is_parity_target`,
    /// whose only `false` arm besides `Null` was `Wgpu` — the bridge backend
    /// that was always going to be deleted, and whose divergences were therefore
    /// not work anybody would do. `crcbl-wgpu` went on 2026-08-21 and the two
    /// predicates became the same function, so the second was deleted rather
    /// than kept for a hypothetical successor.
    ///
    /// What is asserted here is the property that replaced it: the filter is
    /// `is_gpu`, every GPU backend passes it, and `Null` does not — so a row on
    /// any real backend reaches the blocker set and cannot be excluded by
    /// anything except its own kind.
    #[test]
    fn every_gpu_backend_is_inside_the_goal() {
        for backend in GPU_BACKENDS {
            assert!(
                backend.is_gpu(),
                "{backend} is a GPU backend and must reach the blocker set"
            );
        }
        assert!(!BackendKind::Null.is_gpu());
        assert!(
            parity_blockers().all(|entry| entry.backend.is_gpu()),
            "a non-GPU row reached the blocker set"
        );
    }

    /// The rule the query is built on, as a table: the failure mode is a
    /// negation dropped or added, which reads identically and would either
    /// empty the blocker set or fill it with rows nobody can act on.
    #[test]
    fn only_an_api_absence_is_off_the_hook() {
        assert!(!DivergenceKind::ApiAbsence.blocks_parity());
        assert!(DivergenceKind::Unwritten.blocks_parity());
    }

    /// A kind nothing uses is a distinction nobody made. Each of the four has to
    /// describe a real row, or it is vocabulary rather than classification.
    #[test]
    fn every_kind_describes_at_least_one_real_row() {
        for kind in [
            DivergenceKind::ApiAbsence,
            DivergenceKind::Unwritten,
            DivergenceKind::Unrun,
        ] {
            assert!(
                DIVERGENCES.iter().any(|entry| entry.kind == kind),
                "no divergence is classified {kind:?}, so the variant is a name with nothing \
                 behind it"
            );
        }
    }

    /// A shared reason is shared with the row it describes.
    ///
    /// The other half of each pairing is a `Support::No(…)` in the backend
    /// crate, which the compiler checks by being a use of the same constant.
    /// This end is what stops somebody pasting the text back into the row as a
    /// literal and letting the two drift again.
    ///
    /// [`METAL_NO_DRAW_INDIRECT_COUNT`] is **not** checked here any more, and
    /// the assertion below is why: its row left [`DIVERGENCES`] when
    /// `crcbl-mtl` learned to honour a GPU-side count, so the constant is now
    /// shared between that backend's `Device::supports` gate and the error its
    /// encoder refuses with — two places the compiler checks, and neither of
    /// them a row. A row reappearing under it would mean the slice was reverted
    /// without the entry being restored, which is the state
    /// `the_parity_blockers_are_exactly_the_reviewed_list` fails on.
    #[test]
    fn a_shared_reason_is_the_reason_its_row_carries() {
        assert_eq!(
            divergence(Capability::DrawIndirectCount, BackendKind::Metal),
            None,
            "Metal's GPU-side draw count is implemented; the row left when the work landed"
        );
        assert_eq!(
            divergence(Capability::UpdateBindGroup, BackendKind::WebGpu).map(|entry| entry.why),
            Some(WEBGPU_BIND_GROUPS_ARE_IMMUTABLE)
        );
    }
}
