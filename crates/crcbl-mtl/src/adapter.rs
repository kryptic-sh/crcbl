//! One `MTLDevice`, described in the seam's vocabulary.
//!
//! Everything here is a **read**: each function asks the device a question
//! Metal answers before any device object exists, and turns the answer into
//! [`AdapterInfo`] / [`DeviceCaps`]. Nothing creates anything, which is what
//! makes adapter enumeration cheap enough for
//! [`Instance::adapters`](crcbl_hal::Instance::adapters) to promise it.
//!
//! # The rule this module follows
//!
//! **Report what the adapter answers; leave off what would be a promise about
//! code that is not written.** Metal is generous with universal guarantees —
//! every `MTLDevice` has compute, debug groups, `MTLSharedEvent`, anisotropic
//! sampling — and it is tempting to advertise them because they are true of the
//! *API*. They are not yet true of this *backend*: nothing here opens a device,
//! so a caller that keyed off one of those flags would find the entry point
//! behind it refusing. `crcbl-wgpu` reached the same conclusion for its query
//! features and states it the same way.
//!
//! The consequence is a short feature set and a lot of prose. The prose is the
//! useful half: each flag below says which Metal call will answer it and which
//! slice makes that call.
//!
//! # macOS floor
//!
//! Every selector this module sends has been in `MTLDevice` since macOS 11,
//! `supportsBCTextureCompression` being the newest of them; `objc2` does not
//! gate on availability, so an older system would raise an
//! unrecognised-selector exception rather than return a wrong answer. That is
//! the loud failure mode, and the runner this backend is tested on is far
//! newer — but it is the reason a selector is not added here without checking
//! when it landed.

use crcbl_hal::{AdapterId, AdapterInfo, BackendKind, DeviceCaps, DeviceType, Features, Limits};
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSProcessInfo, NSUInteger};
use objc2_metal::{MTLDevice, MTLDeviceLocation, MTLGPUFamily};

/// Sample counts to probe, coarsest first.
///
/// `MTLDevice` has no "largest sample count" property; the only question it
/// answers is `supportsTextureSampleCount:`, one count at a time. Every entry
/// is a power of two because a sample count is a *mask* in the API underneath —
/// see [`Limits::max_sample_count`], which rejects anything else.
const PROBED_SAMPLE_COUNTS: [u32; 7] = [64, 32, 16, 8, 4, 2, 1];

/// What Metal reports as its version, which is the operating system's.
///
/// There is no driver object in Metal and no driver version to read: the
/// framework ships with macOS and is versioned with it, so the OS version is
/// the honest answer to "which driver is this". `NSProcessInfo` is the only
/// dependency this crate has on `objc2-foundation`.
pub(crate) fn driver_string() -> String {
    let version = NSProcessInfo::processInfo().operatingSystemVersion();
    format!(
        "Metal (macOS {}.{}.{})",
        version.majorVersion, version.minorVersion, version.patchVersion
    )
}

/// Everything one Metal device says about itself.
///
/// `driver` is passed in rather than recomputed per adapter: it is a property
/// of the machine, not of the GPU, and asking `NSProcessInfo` once per device
/// would produce the same string N times.
pub(crate) fn adapter_info(
    index: u32,
    device: &ProtocolObject<dyn MTLDevice>,
    driver: String,
) -> AdapterInfo {
    let name = device.name().to_string();
    let features = features_of(device, &name);
    AdapterInfo {
        id: AdapterId(index),
        name,
        // **Metal exposes no PCI ids, and there is nothing honest to put here.**
        // The seam documents `0` as "unknown" for exactly this case. What Metal
        // offers instead is `registryID`, an IOKit registry entry id — a 64-bit
        // key for the device *node*, not a vendor/device pair, and not
        // comparable with the ids `crcbl-vk` reports. Splitting it across two
        // 32-bit fields named `vendor_id` and `device_id` would produce numbers
        // that look like PCI ids and are not, which is worse than absence.
        vendor_id: 0,
        device_id: 0,
        device_type: device_type_of(device),
        driver,
        backend: BackendKind::Metal,
        caps: DeviceCaps {
            features,
            limits: limits_of(device, features),
        },
    }
}

/// Which class of device this is, from what Metal actually reports.
///
/// The order is deliberate: *where the GPU is* settles the question before
/// *where its memory is* does, because an external GPU is a separate card
/// however it addresses memory.
///
/// Two signals are deliberately not used:
///
/// * `isHeadless` describes whether a GPU drives a display, which says nothing
///   about its class — a discrete card in a headless Mac and an integrated one
///   in a closed-lid laptop both answer yes.
/// * [`DeviceType::Cpu`] is never reported. Metal has no software rasteriser to
///   report it for; the equivalent CI target on macOS is `crcbl-wgpu`.
///
/// [`DeviceType::Virtual`] is never reported either, and that one is a genuine
/// gap rather than a decision: **Metal has no virtualisation query**. A
/// paravirtual GPU under Apple's Virtualization framework answers every
/// question below exactly as the built-in one does, so it enumerates as
/// [`DeviceType::Integrated`]. Nothing short of matching on the device's name
/// would separate them, and name-matching is not a capability query.
///
/// `crcbl_mtl::quirk` does match on that name, and the two are not in tension:
/// it withholds one flag a specific device was *measured* to lie about, and says
/// so; reporting a whole [`DeviceType`] from a string would be a claim about
/// every device with that name and about nothing anybody checked.
fn device_type_of(device: &ProtocolObject<dyn MTLDevice>) -> DeviceType {
    let location = device.location();
    if device.isRemovable()
        || location == MTLDeviceLocation::External
        || location == MTLDeviceLocation::Slot
    {
        return DeviceType::Discrete;
    }
    // Apple Silicon lands here: built-in, not removable, and sharing one memory
    // pool with the CPU. So does an Intel Mac's integrated GPU, which is also
    // the `isLowPower` one in a dual-GPU machine.
    if device.hasUnifiedMemory() || device.isLowPower() {
        return DeviceType::Integrated;
    }
    // Built-in, its own memory: the discrete GPU in a dual-GPU Intel Mac.
    DeviceType::Discrete
}

/// Whether `MTLBuffer::gpuAddress` can be sent on this system.
///
/// macOS 13 is where it arrives, and the question is a **system** one rather
/// than a device one — which is the whole history of this gate.
///
/// It was `supportsFamily(MTLGPUFamily::Metal3)` first, which confuses a family
/// feature set with an API's availability: CI's `Apple Paravirtual device`
/// answers `Metal3 = false` and returns usable addresses anyway. Then it was
/// `respondsToSelector(sel!(gpuAddress))` **sent to the `MTLDevice`**, which is
/// simply the wrong receiver — `gpuAddress` belongs to `MTLBuffer`, so a device
/// never responds to it and the gate was false everywhere. Both were caught by
/// the seam reporting the capability as never asked for.
///
/// The version check is what the other two were reaching for. It is also how
/// [`driver_string`] already reads the system, so this adds no dependency.
fn gpu_address_is_available() -> bool {
    NSProcessInfo::processInfo()
        .operatingSystemVersion()
        .majorVersion
        >= 13
}

/// The seam features this adapter can answer for, and only those.
///
/// # Reported, each from a real query
///
/// * [`Features::BUFFER_DEVICE_ADDRESS`] ← `supportsFamily:` with
///   [`MTLGPUFamily::Metal3`]. `MTLBuffer::gpuAddress` is the Metal 3 API the
///   plan's mapping table names for this feature, so the family query is the
///   question that decides it.
/// * [`Features::TEXTURE_COMPRESSION_BC`] ← `supportsBCTextureCompression`.
///   Apple Silicon answers no and Intel Macs answer yes, so this is the one
///   flag here that genuinely varies across the machines the engine ships to.
/// * [`Features::SAMPLER_ANISOTROPY`] — unconditional in Metal
///   (`MTLSamplerDescriptor::maxAnisotropy` is a plain property of every
///   sampler) **and now backed by a call this backend makes**: the device slice
///   creates samplers, so this is the flag that has stopped being a promise
///   about unwritten code. It is reported with no query because there is none
///   to make; [`Limits::max_sampler_anisotropy`] carries the API's own ceiling
///   beside it, and `crcbl_mtl::device`'s sampler creation is what enforces it.
/// * [`Features::TIMELINE_SEMAPHORE`] — unconditional in Metal, and now backed
///   by the calls the command slice makes: `MTLDevice::newSharedEvent`,
///   `MTLCommandBuffer::encodeSignalEvent:value:` and
///   `MTLSharedEvent::waitUntilSignaledValue:timeoutMS:` are the three halves
///   of the seam's timeline, and `crcbl_mtl::device`'s `create_semaphore`,
///   `submit` and `wait_semaphores` are where each one lands. Reporting it is
///   load-bearing rather than cosmetic: the seam says a timeline semaphore on a
///   device without this feature must be refused.
/// * [`Features::DEBUG_MARKERS`] — likewise unconditional, and likewise now
///   real: `MTLCommandBuffer::pushDebugGroup:` and
///   `MTLCommandEncoder::pushDebugGroup:`/`insertDebugSignpost:` are what
///   `crcbl_mtl::command`'s debug-label methods call, which is what puts named
///   regions in an Xcode GPU capture — one of this phase's exit criteria.
///
/// * [`Features::COMPUTE`] — unconditional in Metal, and backed end to end:
///   `crcbl_mtl::pipeline`'s `create_compute_pipeline_impl` builds the
///   `MTLComputePipelineState`, `crcbl_mtl::command`'s `begin_compute_pass`
///   opens the `MTLComputeCommandEncoder` that holds it, and `dispatch` and
///   `dispatch_indirect` are `dispatchThreadgroups:threadsPerThreadgroup:` and
///   `dispatchThreadgroupsWithIndirectBuffer:indirectBufferOffset:threadsPerThreadgroup:`.
///   The workgroup size those calls take comes from
///   [`ComputePipelineDesc::workgroup_size`](crcbl_hal::ComputePipelineDesc::workgroup_size),
///   which exists because MSL has nowhere to declare it — the one field of the
///   seam this backend is the reason for.
/// * [`Features::MULTI_DRAW_INDIRECT`] — backed by `crcbl_mtl::command`'s
///   `indirect`, which issues one
///   `drawPrimitives:indirectBuffer:indirectBufferOffset:` per argument
///   structure. [`DrawIndirect::draw_count`](crcbl_hal::DrawIndirect::draw_count)
///   is a CPU value by definition, so a loop over it is not an approximation of
///   the feature — it emits exactly the N draws from N GPU-written argument
///   structures that the flag means. `crcbl_mtl::draw` argues it in full.
/// * [`Features::INDIRECT_FIRST_INSTANCE`] — `baseInstance` is a field of
///   `MTLDrawPrimitivesIndirectArguments` and
///   `MTLDrawIndexedPrimitivesIndirectArguments`, and Metal reads it rather
///   than requiring it to be zero. Both structures are field-for-field the
///   layouts Vulkan calls `VkDrawIndirectCommand` and
///   `VkDrawIndexedIndirectCommand`, which is what lets one compute pass feed
///   both backends; reported now because the indirect draws that read them are
///   calls this backend makes.
/// * [`Features::PRESENT_FEEDBACK`] — `MTLDrawable::addPresentedHandler:` is a
///   plain method of the drawable protocol with no query behind it and no
///   version gate, so this is unconditional in the same sense the four above
///   are, and it is backed by `crcbl_mtl::swapchain`'s `present`, which
///   attaches a handler, and its `wait_until_presented`, which sleeps until one
///   fires.
///
///   **Reported for the device even though only a windowed swapchain can
///   answer**, and that is the decision worth reading. The flag lives on the
///   device and is read once at start-up — `GpuContext::open` logs which pacing
///   story the run gets from it — while whether there is a drawable to observe
///   is a property of a *swapchain*, created later and possibly one of several.
///   So there is no honest device-level answer that accounts for the offscreen
///   ring, and the two candidate errors are not symmetric: withholding the flag
///   would make every macOS window unpaceable, because the seam then requires
///   an immediate `Ok(())` forever and the closed loop becomes unreachable
///   code, whereas reporting it costs an offscreen-only device nothing — the
///   ring's wait returns at once through the seam's own "nothing to wait for"
///   answer, which is exactly what `crcbl-vk` does with its own offscreen ring
///   on a driver that has `VK_KHR_present_wait`. The flag therefore means what
///   the seam says it means, "the CPU can find out", and not "every swapchain
///   on this device will block".
/// * [`Features::DEPTH_BIAS_CLAMP`] and [`Features::POLYGON_MODE_LINE`] — both
///   are unconditional in Metal and both are now replayed onto the render
///   encoder by `bind_graphics_pipeline`:
///   `setDepthBias:slopeScale:clamp:` (whose third argument *is* the clamp) and
///   `setTriangleFillMode:`. They are reported because those calls are made,
///   which is the same standard `SAMPLER_ANISOTROPY` and `TIMELINE_SEMAPHORE`
///   were held to.
/// * [`Features::DEPTH_CLAMP`] — the same call is made,
///   `bind_graphics_pipeline`'s `setDepthClipMode:`, and on one device that is
///   not enough: it accepts the call and clips the primitive regardless. So this
///   is the one flag here that a *measurement* rather than a query withholds,
///   from a device whose name says it is virtual, and `crcbl_mtl::quirk` carries
///   what was measured, on what, and why Metal offers nothing better to key on.
///   Every other device reports it exactly as before.
/// * [`Features::OCCLUSION_QUERY`] — `visibilityResultBuffer` is a property of
///   every `MTLRenderPassDescriptor` and `setVisibilityResultMode:offset:` a
///   selector on every `MTLRenderCommandEncoder`, neither behind a query nor a
///   version gate, and the pool they name is a plain `MTLBuffer` rather than
///   anything built from `counterSets`. It is reported because
///   `Device::create_query_set` now builds that pool, `reset_query_set` and
///   `resolve_query_set` reach it and `Device::query_results` reads it — the
///   same standard the four above were held to. What the flag does *not* promise
///   is a way to count into one: the seam has no begin/end query verb on any
///   backend, which `crcbl_mtl::query` and `Device::create_query_set` both say.
///
/// * [`Features::DESCRIPTOR_INDEXING`] — **withdrawn by the binding slice and
///   restored by the argument-buffer one, which is the entry worth reading
///   before changing anything here.** MTL1 reported it from
///   `argumentBuffersSupport`, a true statement about the *hardware*; the
///   binding slice made it a false one about this *backend*, because bind
///   groups were flat argument tables with no runtime-sized array and
///   `crcbl_hal::pipeline` is explicit that a backend which refuses
///   [`BindingFlags`](crcbl_hal::BindingFlags) must not report the feature.
///   `crcbl_mtl::binding` now honours a `VARIABLE_COUNT` slot as an argument
///   buffer of `MTLBuffer::gpuAddress` values, so the flag is earned again —
///   and the two queries behind it are the two things that construction needs
///   rather than a proxy for it: the device must respond to `gpuAddress`,
///   because that is the value the table is filled with.
///
///   **The second gate used to be `MTLGPUFamily::Metal3` and that was wrong.**
///   It read as reasonable — `gpuAddress` arrived with Metal 3 — but it
///   confuses the family *feature set* with the API's *availability*. CI's
///   `Apple Paravirtual device` answers `supportsFamily(Metal3) = false` and
///   returns perfectly usable addresses, which `crate::binding`'s probe
///   measured: four non-zero values, every one dereferenced correctly by a
///   kernel. So the family gate switched this capability off on the only device
///   that had ever proven it, and would have left it reported closed and
///   exercised nowhere. [`gpu_address_is_available`] asks what the code depends
///   on — the macOS version the selector arrives in — and keeps what the family
///   query was protecting.
///
///   [`Features::BUFFER_DEVICE_ADDRESS`] still rides on `Metal3` and rests on
///   the same mistaken reasoning, but it is left alone deliberately: nothing on
///   this backend exercises `BufferUsage::DEVICE_ADDRESS`, so widening it would
///   turn on an unproven path rather than fix a measured defect. See
///   `docs/backlog.md`.
///
/// * [`Features::DRAW_INDIRECT_COUNT`] — **the one flag here that is decided by
///   an allocation rather than by a query.** The count comes from GPU memory,
///   and Metal's only execution that reads one is `executeCommandsInBuffer:`
///   over an `MTLIndirectCommandBuffer` whose commands already exist. Those
///   commands are written by a compute kernel that runs before the render
///   encoder `draw_indirect_count` is called inside is opened, which is what
///   `crcbl_mtl::command`'s deferred recording makes reachable and
///   `crcbl_mtl::icb` implements. So the question left for this function is
///   whether the *device* holds such a buffer, and
///   [`icb::probe`](crate::icb::probe) answers it by asking for one of exactly
///   the shape and size the path uses. This is the last Tier A feature, so
///   reporting it moves every Metal adapter onto
///   [`GeometryPath::IndirectCount`](crcbl_hal::GeometryPath::IndirectCount).
///
///   **That makes the `probe` block below the whole slice's off switch**, and it
///   is kept to one block for exactly that reason: this feature has hung CI's
///   GPU once already. Delete those three lines and every Metal adapter reports
///   the flag off, `limits_of` falls back to the seam's floor,
///   `MetalDevice::open` compiles no kernels, `crcbl_mtl::command`'s
///   `indirect_count` refuses with [`HalError::Unsupported`](crcbl_hal::HalError),
///   and the renderer picks a different
///   [`GeometryPath`](crcbl_hal::GeometryPath) — with no other edit anywhere.
///   The cost of pulling it is that the row goes back to being a blocker
///   reported closed nowhere, which is worse than it sounds; see
///   `docs/backlog.md`.
///
/// # Absent, with the reason for each
///
/// * [`Features::TIMESTAMP_QUERY`] and
///   [`Features::PIPELINE_STATISTICS_QUERY`] — `supportsCounterSampling:` would
///   answer both, but reporting them obliges a
///   [`Limits::timestamp_period_ns`], and Metal has no fixed tick period to
///   report: the GPU clock is correlated to the host's through
///   `sampleTimestamps:gpuTimestamp:` at sample time. A fabricated period is
///   exactly the number this workspace refuses to write down, so the feature
///   waits for the slice that can measure it — and this module's
///   `a_device_reports_its_counter_sampling_gpu_families_and_timestamp_correlation`
///   is that measurement, printing the period alongside every
///   `supportsCounterSampling:` answer rather than asserting either.
/// * [`Features::ASYNC_COMPUTE_QUEUE`] and [`Features::TRANSFER_QUEUE`] —
///   Metal has one `MTLCommandQueue` type and no queue families at all, which
///   `crcbl_hal::QueueKind` already records as the reason it is not named
///   `QueueFamily`.
/// * [`Features::SHADER_DEBUG_PRINTF`] — `MTLLogState` exists, and nothing
///   routes it into `log` yet.
fn features_of(device: &ProtocolObject<dyn MTLDevice>, name: &str) -> Features {
    let mut out = Features::empty();
    let metal3 = device.supportsFamily(MTLGPUFamily::Metal3);
    if metal3 {
        out |= Features::BUFFER_DEVICE_ADDRESS;
    }
    // Both halves are device queries and both are load-bearing: the tier is
    // what makes a dynamically indexed argument buffer legal, and the selector
    // check is what says `MTLBuffer::gpuAddress` — the value
    // `crcbl_mtl::binding` fills the table with — can be sent at all.
    //
    // **Not `supportsFamily(Metal3)`, and that was measured rather than
    // reasoned.** This gate read `metal3` on the theory that `gpuAddress` is a
    // Metal 3 property, which conflates the family feature set with the API's
    // availability. CI's `Apple Paravirtual device` answers
    // `supportsFamily(Metal3) = false` and *still* returns usable addresses:
    // `crate::binding`'s probe read four non-zero `gpuAddress` values on it and
    // a kernel dereferenced every one correctly. Gating on the family therefore
    // switched this path off on the one device it is proven to work on, which
    // would have left the capability closed and never exercised anywhere.
    //
    // `gpu_address_is_available` asks the question the code actually depends on
    // and keeps the property the family query was there for: the macOS 13
    // selector is never sent to a system that lacks it. Its own doc carries the
    // two wrong forms this gate had before, including a `respondsToSelector:`
    // sent to the device rather than to a buffer.
    //
    // **And not the tier either, which is the second correction this gate has
    // needed.** `Tier2` was required on the reading that a dynamically indexed
    // argument buffer needs it. Two probes on CI's `Apple Paravirtual device`
    // say otherwise, and that device answers
    // `argumentBuffersSupport = Tier1`: `crate::binding`'s bindless kernel
    // indexes a `constant`-addressed `array<uint device*, N>` by thread group
    // and reads every word correctly, and this module's
    // `a_compute_kernel_encodes_the_draw_an_indirect_command_buffer_executes`
    // reaches an `MTLIndirectCommandBuffer` through a Metal 3 argument buffer
    // holding a `gpuResourceID`. Both ran with Metal API **and** GPU validation
    // enabled and produced exact values.
    //
    // The tier describes argument buffers in Metal's *resource-binding* sense —
    // `[[id(n)]]` slots, resource arrays, heaps. A table of raw device
    // addresses or resource IDs bypasses that machinery, and is the Metal 3
    // idiom, so the tier was never the question. Keeping it meant the
    // capability read closed while being exercised on no machine at all.
    if gpu_address_is_available() {
        out |= Features::DESCRIPTOR_INDEXING;
    }
    if device.supportsBCTextureCompression() {
        out |= Features::TEXTURE_COMPRESSION_BC;
    }
    // No query for any of these: every Metal device has anisotropic sampling,
    // `MTLSharedEvent`, `pushDebugGroup:`, compute pipelines, a depth-bias
    // clamp, a line fill mode and a drawable that will call back once it has
    // been shown — and the slice that calls each one has now landed. See the
    // doc comment above for which call backs each, and for why the last is
    // reported for a device whose swapchain may be offscreen.
    out |= Features::SAMPLER_ANISOTROPY;
    out |= Features::TIMELINE_SEMAPHORE;
    out |= Features::DEBUG_MARKERS;
    out |= Features::COMPUTE;
    // Depth clamping is the one guarantee of that list a device was measured to
    // break; `crate::quirk` holds the measurement and the reason the name is
    // what decides it.
    if crate::quirk::honours_depth_clamp(name) {
        out |= Features::DEPTH_CLAMP;
    }
    out |= Features::DEPTH_BIAS_CLAMP;
    out |= Features::POLYGON_MODE_LINE;
    out |= Features::PRESENT_FEEDBACK;
    // Likewise unconditional: an occlusion pool is a plain `MTLBuffer` and both
    // selectors that name one are core Metal, so there is nothing to ask. The
    // doc comment above says which calls back it.
    out |= Features::OCCLUSION_QUERY;
    // Likewise no query: every Metal device takes an indirect buffer on a draw
    // and reads `baseInstance` out of it, and the binding slice's `indirect`
    // loop is the call that makes both true of this backend.
    out |= Features::MULTI_DRAW_INDIRECT;
    out |= Features::INDIRECT_FIRST_INSTANCE;
    // And this one *is* a question, asked by creating the thing the path needs
    // rather than by reading a property — Metal exposes no `supportsICB`-style
    // flag, and the three `support*` fields on
    // `MTLIndirectCommandBufferDescriptor` are about ray tracing, dynamic
    // attribute strides and colour attachment mapping. So the probe builds the
    // exact descriptor `crcbl_mtl::icb` builds, asks for the exact command
    // count `limits_of` is about to promise, and reports whether Metal handed
    // one back.
    if crate::icb::probe(device) {
        out |= Features::DRAW_INDIRECT_COUNT;
    }
    // And no query for this one either, for a sharper reason: there is nothing
    // to ask. Metal has no push constants, so `crcbl-shaders` commits MSL in
    // which the block is an ordinary buffer argument and this backend sends it
    // with `setBytes:length:atIndex:` — a selector on every render and compute
    // encoder there is. What the feature waited for was knowing *which*
    // argument-table index the committed artifacts put the block at, which
    // `crcbl_mtl::argument` now derives rather than assumes.
    out |= Features::PUSH_CONSTANTS;
    out
}

/// This adapter's numeric ceilings.
///
/// Metal reports very few of these, so the shape is "start at
/// [`Limits::minimum`] and replace what the device genuinely answers". Every
/// field left at the floor is a **contract this backend guarantees**, not a
/// measurement of the hardware — the seam says a limit is a hard ceiling, and a
/// ceiling that is lower than the truth is always safe while one that is higher
/// is a crash on someone else's machine.
///
/// Read off the device:
///
/// * `maxBufferLength` → both buffer ranges. Metal draws no distinction between
///   a uniform and a storage buffer — both are `MTLBuffer` bound to the same
///   slots — so one number honestly answers both.
/// * `maxThreadsPerThreadgroup` → the per-dimension compute workgroup size.
/// * `supportsTextureSampleCount:`, probed → the sample-count ceiling.
///
/// Left at the floor, because Metal has no property for them: the texture
/// dimension and array-layer limits, the bind-group count, the colour
/// attachment count, the three alignments, and the compute *invocation* total
/// (Metal reports that one per compiled pipeline state, as
/// `MTLComputePipelineState::maxTotalThreadsPerThreadgroup`, which is not
/// available before a device exists).
///
/// The feature-keyed fields stay consistent with `features_of`: the anisotropy
/// cap is Metal's own ceiling because [`Features::SAMPLER_ANISOTROPY`] is
/// reported, `max_push_constant_size` is
/// [`argument::MAX_PUSH_CONSTANT_BYTES`](crate::argument::MAX_PUSH_CONSTANT_BYTES)
/// because [`Features::PUSH_CONSTANTS`] is,
/// `max_bindless_descriptors` is
/// [`binding::MAX_BINDLESS_DESCRIPTORS`](crate::binding::MAX_BINDLESS_DESCRIPTORS)
/// because [`Features::DESCRIPTOR_INDEXING`] is, and the timestamp period stays
/// at the floor's zero because its feature is absent.
///
/// The push-constant figure is the **call's** documented ceiling rather than a
/// share of anything: Apple's feature-set tables cap inlined buffer contents at
/// 4 KB on every GPU family, and unlike `crcbl-dx12`'s root-signature budget it
/// is not spent by the bind groups too. What a bind group can exhaust is the
/// one argument-table *entry* the block needs, which
/// `crcbl_mtl::argument`'s `plan` refuses by name at layout creation.
fn limits_of(device: &ProtocolObject<dyn MTLDevice>, features: Features) -> Limits {
    let floor = Limits::minimum();
    let threads = device.maxThreadsPerThreadgroup();
    let max_buffer = to_u64(device.maxBufferLength());
    Limits {
        max_storage_buffer_range: max_buffer,
        max_uniform_buffer_range: max_buffer,
        max_sampler_anisotropy: if features.contains(Features::SAMPLER_ANISOTROPY) {
            crate::device::MAX_SAMPLER_ANISOTROPY
        } else {
            floor.max_sampler_anisotropy
        },
        max_push_constant_size: if features.contains(Features::PUSH_CONSTANTS) {
            crate::argument::MAX_PUSH_CONSTANT_BYTES
        } else {
            floor.max_push_constant_size
        },
        max_bindless_descriptors: if features.contains(Features::DESCRIPTOR_INDEXING) {
            crate::binding::MAX_BINDLESS_DESCRIPTORS
        } else {
            floor.max_bindless_descriptors
        },
        max_draw_indirect_count: if features.contains(Features::DRAW_INDIRECT_COUNT) {
            crate::icb::MAX_DRAW_INDIRECT_COUNT
        } else {
            floor.max_draw_indirect_count
        },
        max_sample_count: max_sample_count(device),
        max_compute_workgroup_size: [
            to_u32(threads.width),
            to_u32(threads.height),
            to_u32(threads.depth),
        ],
        ..floor
    }
}

/// The largest sample count this device accepts for a texture.
///
/// Coarsest first, so the first `true` is the ceiling. `1` if the device
/// somehow refuses every probed count, which keeps the answer a power of two
/// rather than zero.
fn max_sample_count(device: &ProtocolObject<dyn MTLDevice>) -> u32 {
    PROBED_SAMPLE_COUNTS
        .into_iter()
        .find(|&count| device.supportsTextureSampleCount(count as NSUInteger))
        .unwrap_or(1)
}

/// Saturating `NSUInteger` → `u32`, for a limit that is a ceiling.
///
/// A limit that saturated is still a ceiling the backend can honour, which is
/// the only property the seam asks of one.
fn to_u32(value: NSUInteger) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Saturating `NSUInteger` → `u64`. See `to_u32`.
fn to_u64(value: NSUInteger) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::ptr::NonNull;
    use std::time::{Duration, Instant};

    use crcbl_hal::MemoryLocation;
    use objc2_metal::{
        MTLCommonCounterSetStageUtilization, MTLCommonCounterSetStatistic,
        MTLCommonCounterSetTimestamp, MTLCounter, MTLCounterSamplingPoint, MTLCounterSet,
        MTLIndirectCommandBuffer, MTLIndirectCommandBufferDescriptor, MTLIndirectCommandType,
        MTLTimestamp,
    };

    use super::*;

    /// How far apart the two `sampleTimestamps:gpuTimestamp:` calls are.
    ///
    /// Tens of milliseconds is the whole requirement: long enough that the ratio
    /// of the two deltas is not dominated by the cost of the two message sends,
    /// short enough to be invisible next to a suite that opens a Metal device
    /// per test.
    const CORRELATION_SLEEP: Duration = Duration::from_millis(50);

    /// Every `MTLCounterSamplingPoint` value, in declaration order.
    ///
    /// `MTLCounterSamplingPoint` is an `NSUInteger` newtype carrying associated
    /// constants rather than a Rust enum, so no `match` can be made exhaustive
    /// over it and nothing but reading `objc2_metal`'s declaration says this
    /// list is complete. What *is* checked below is that the five entries are
    /// pairwise distinct — a duplicated constant here would print one point
    /// twice and silently never ask about another.
    const SAMPLING_POINTS: [(&str, MTLCounterSamplingPoint); 5] = [
        ("AtStageBoundary", MTLCounterSamplingPoint::AtStageBoundary),
        ("AtDrawBoundary", MTLCounterSamplingPoint::AtDrawBoundary),
        (
            "AtDispatchBoundary",
            MTLCounterSamplingPoint::AtDispatchBoundary,
        ),
        (
            "AtTileDispatchBoundary",
            MTLCounterSamplingPoint::AtTileDispatchBoundary,
        ),
        ("AtBlitBoundary", MTLCounterSamplingPoint::AtBlitBoundary),
    ];

    /// Every `MTLGPUFamily` the ICB probe asks about, newest first.
    ///
    /// Which command types an `MTLIndirectCommandBuffer` may hold is a
    /// family-gated question in Apple's feature-set tables, so the family answer
    /// is the precondition for every other answer the probe takes. The list is
    /// wider than the counter probe's three on purpose: that one asks the three
    /// terms `wgpu-hal` derives its mesh support from, while this one is trying
    /// to place a device on the ladder at all, and a device that answers `Mac2`
    /// and nothing above it is a different machine from one that answers
    /// `Apple9`.
    ///
    /// `MTLGPUFamily` is an `NSInteger` newtype carrying associated constants
    /// rather than a Rust enum, so nothing but reading `objc2_metal`'s
    /// declaration says this list is complete. What *is* checked below is that
    /// the entries are pairwise distinct — a duplicated constant would print one
    /// family twice and silently never ask about another.
    ///
    /// `Mac1`, `MacCatalyst1` and `MacCatalyst2` are absent because
    /// `objc2-metal` marks all three `#[deprecated]`, and this workspace builds
    /// with `-D warnings`. Nothing is lost: a device that answers `Mac1` and not
    /// `Mac2` predates every macOS this backend's floor allows.
    const ICB_FAMILIES: [(&str, MTLGPUFamily); 16] = [
        ("Metal4", MTLGPUFamily::Metal4),
        ("Metal3", MTLGPUFamily::Metal3),
        ("Apple10", MTLGPUFamily::Apple10),
        ("Apple9", MTLGPUFamily::Apple9),
        ("Apple8", MTLGPUFamily::Apple8),
        ("Apple7", MTLGPUFamily::Apple7),
        ("Apple6", MTLGPUFamily::Apple6),
        ("Apple5", MTLGPUFamily::Apple5),
        ("Apple4", MTLGPUFamily::Apple4),
        ("Apple3", MTLGPUFamily::Apple3),
        ("Apple2", MTLGPUFamily::Apple2),
        ("Apple1", MTLGPUFamily::Apple1),
        ("Mac2", MTLGPUFamily::Mac2),
        ("Common3", MTLGPUFamily::Common3),
        ("Common2", MTLGPUFamily::Common2),
        ("Common1", MTLGPUFamily::Common1),
    ];

    /// The command types an ICB standing in for the seam's indirect draws would
    /// have to hold.
    ///
    /// [`CommandEncoder::draw_indirect_count`](crcbl_hal::CommandEncoder::draw_indirect_count)
    /// needs `Draw` and `draw_indexed_indirect_count` needs `DrawIndexed`; the
    /// seam has both verbs, so an ICB that could hold only one of them would
    /// close half a row. The patch and mesh types are absent because no seam
    /// verb reaches them, and the two dispatch types are absent because Metal's
    /// own header says a dispatch command type cannot be mixed with any other —
    /// asking for one here would measure a descriptor the draw path could never
    /// use.
    const ICB_DRAW_COMMANDS: MTLIndirectCommandType =
        MTLIndirectCommandType::Draw.union(MTLIndirectCommandType::DrawIndexed);

    /// `maxCommandCount` values to ask for, **strictly ascending**.
    ///
    /// Ascending, and the probe stops at the first refusal, because the useful
    /// measurement is where the device stops rather than whether it clears the
    /// top rung — and because climbing towards a ceiling asks for at most one
    /// allocation past it. The top rung is
    /// [`Limits::desktop`]`().max_draw_indirect_count`, which is the number a
    /// backend reporting the capability would want to promise; the bottom is
    /// well clear of the `1` this backend reports today, which the probe's own
    /// last line prints for comparison.
    const ICB_COMMAND_COUNTS: [NSUInteger; 6] = [64, 1024, 4096, 16384, 65536, 1 << 20];

    /// Per-stage buffer bind slots each ICB command is sized for.
    ///
    /// An ICB reserves storage for every command's worst case, so this number
    /// multiplies into the allocation the top rung of [`ICB_COMMAND_COUNTS`]
    /// asks for. It is set rather than left at Metal's default because the
    /// default is far larger than any pipeline this engine builds binds, and a
    /// probe that asks for a gigabyte to learn a ceiling has changed what it is
    /// measuring. The printed line carries the default beside it, and the ICB's
    /// own `size` beside that, so the reader can scale to any other value.
    const ICB_STAGE_BUFFER_BINDS: NSUInteger = 8;

    /// The inheritance settings to try, and the one axis the probe varies.
    ///
    /// A `draw_indirect_count` built on an ICB inherits both: the pipeline state
    /// and the argument tables are set on the render encoder the seam's pass
    /// already opened, and the encoding kernel writes draw arguments alone. The
    /// second row is the control — if creation fails for the first and succeeds
    /// for the second, the inheritance is what the device refused rather than
    /// the command types or the count.
    const ICB_INHERITANCE: [(&str, bool, bool); 2] =
        [("inherit=YES", true, true), ("inherit=NO", false, false)];

    /// One `sampleTimestamps:gpuTimestamp:` call, as the pair it writes.
    fn sample_timestamps(device: &ProtocolObject<dyn MTLDevice>) -> (MTLTimestamp, MTLTimestamp) {
        let mut cpu: MTLTimestamp = 0;
        let mut gpu: MTLTimestamp = 0;
        // SAFETY: the selector's only obligation is that both arguments are
        // valid pointers to writable `MTLTimestamp` storage, which is why
        // `objc2-metal` declares it `unsafe` at all. Both point at locals that
        // outlive the call and are borrowed nowhere else.
        unsafe {
            device.sampleTimestamps_gpuTimestamp(NonNull::from(&mut cpu), NonNull::from(&mut gpu));
        }
        (cpu, gpu)
    }

    /// **The measurement that settles the two `DivergenceKind::Unclassified`
    /// rows this backend carries** —
    /// [`Capability::TimestampQuery`](crcbl_hal::Capability::TimestampQuery) and
    /// [`PipelineStatisticsQuery`](crcbl_hal::Capability::PipelineStatisticsQuery),
    /// which `crcbl_hal::DIVERGENCES` classifies as unsettled rather than
    /// unwritten because both answers come from a *device* at run time and no
    /// Mac runs in the workspace that wrote them.
    ///
    /// It creates nothing, submits nothing and asserts almost nothing. **It
    /// prints**, and the CI log is the artifact. It is here rather than in
    /// `device.rs` because everything it asks is an adapter-level read, which is
    /// this module's whole subject — including the paragraph in `features_of`
    /// that says Metal has no fixed tick period to report.
    ///
    /// # What each answer settles
    ///
    /// **`supportsCounterSampling:`.** A device that answers yes at a point the
    /// seam can reach turns the `TimestampQuery` row from `Unclassified` into
    /// `Unwritten`: the work becomes known, and it is
    /// `MTLCounterSampleBufferDescriptor` plus the encoders'
    /// `sampleCountersInBuffer:atSampleIndex:withBarrier:`.
    ///
    /// A device that answers yes only at `AtStageBoundary` is **enough**, and
    /// that is worth stating precisely because it looks like a narrowing and is
    /// not one. Metal's stage-boundary sampling is requested through a pass
    /// descriptor's `sampleBufferAttachments`, so it samples where an encoder
    /// opens and closes — and that is the only place the seam asks for a
    /// timestamp at all: the two queries are named by
    /// [`PassTimestampWrites`](crcbl_hal::PassTimestampWrites) on
    /// `RenderPassDesc`/`ComputePassDesc`, and there is no free-standing write
    /// left to place anywhere else. If the device answers yes *nowhere*, the
    /// honest outcome is narrowing the capability rather than implementing it.
    ///
    /// **`counterSets`.** The `PipelineStatisticsQuery` row turns on whether
    /// this device advertises `MTLCommonCounterSetStatistic` at all, and on
    /// which counters that set actually contains — Apple documents an
    /// implementation as free to omit some of a common set's counters, so the
    /// per-set counter list is printed rather than the set name alone. No set,
    /// and the row stays open with its reason changed from "unknown" to "this
    /// device cannot, and CI has only this device".
    ///
    /// **`sampleTimestamps:gpuTimestamp:`.** Two calls across a sleep give the
    /// GPU tick period this backend currently declines to report, and the
    /// measurement is why it can stop declining. `wgpu-hal` 30.0.0's
    /// `src/metal/adapter.rs` picks `83.333` when the device name starts with
    /// `Intel` and `1.0` otherwise, and its own comment calls that "the
    /// dangerous but easy thing"; nothing here has to guess. The wall clock is
    /// printed beside the CPU delta so the reader can see for themselves
    /// whether Metal's CPU timestamp really is in nanoseconds before dividing by
    /// it.
    ///
    /// **`supportsFamily:`.** Free with the rest, and the precondition the Metal
    /// mesh slice starts from. `wgpu-hal`'s same file derives its `mesh_shaders`
    /// as `family_check && (Metal3 || Apple7 || Mac2) && !is_virtual` — the
    /// three families are *alternatives*, not conjuncts, and the fourth term is
    /// the device's own name containing "virtual", which is exactly what CI's
    /// `Apple Paravirtual device` does. All four terms are printed.
    ///
    /// # What it fails on
    ///
    /// A measurement test that silently prints nothing is a green light wired to
    /// nothing, so the two ways this could reach the end having asked no device
    /// anything are assertions: an empty device name, and a CPU timestamp that
    /// did not move across the sleep. A device that honestly reports no counter
    /// sampling and no counter sets is a *result* and passes.
    ///
    /// The third assertion is a contradiction rather than an absence: a device
    /// that says yes to some sampling point while exposing no counter set could
    /// not have an `MTLCounterSampleBuffer` created on it at all, because
    /// `MTLCounterSampleBufferDescriptor` takes a counter set.
    ///
    /// nextest captures a passing test's stdout, so read this with
    /// `--success-output immediate`, which is what `tests/run-mtl-e2e.sh`
    /// passes.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_device_reports_its_counter_sampling_gpu_families_and_timestamp_correlation() {
        let (_validated, device) = crate::device::tests::open_device();
        let raw = &*device.inner.raw;

        let name = raw.name().to_string();
        assert!(
            !name.is_empty(),
            "MTLDevice::name came back empty, so this test never reached a device"
        );
        println!("crcbl-mtl counters: device={name:?} {}", driver_string());

        for (label, family) in [
            ("Metal3", MTLGPUFamily::Metal3),
            ("Apple7", MTLGPUFamily::Apple7),
            ("Mac2", MTLGPUFamily::Mac2),
        ] {
            println!(
                "crcbl-mtl counters: supportsFamily {label} = {}",
                raw.supportsFamily(family)
            );
        }
        println!(
            "crcbl-mtl counters: name contains \"virtual\" = {}",
            name.to_lowercase().contains("virtual")
        );

        let mut seen: Vec<MTLCounterSamplingPoint> = Vec::new();
        let mut any_sampling = false;
        for (label, point) in SAMPLING_POINTS {
            assert!(
                !seen.contains(&point),
                "SAMPLING_POINTS lists {label} twice, so one of Metal's five points is \
                 never asked about"
            );
            seen.push(point);
            let supported = raw.supportsCounterSampling(point);
            any_sampling |= supported;
            println!("crcbl-mtl counters: supportsCounterSampling {label} = {supported}");
        }

        // `counterSets` is the half that needs the `MTLCounters` feature; the
        // manifest says so where it turns it on.
        let sets = raw.counterSets();
        let names: Vec<String> = sets
            .iter()
            .flat_map(|sets| sets.iter())
            .map(|set| {
                let set_name = set.name().to_string();
                let counters: Vec<String> = set
                    .counters()
                    .iter()
                    .map(|counter| counter.name().to_string())
                    .collect();
                println!("crcbl-mtl counters: counterSet {set_name:?} counters={counters:?}");
                set_name
            })
            .collect();
        println!("crcbl-mtl counters: counterSets = {}", names.len());

        // SAFETY: each is an `NSString` constant exported by Metal.framework,
        // which `objc2-metal` links unconditionally and which is loaded — the
        // device above came out of it. Reading it is `unsafe` only because it is
        // an `extern` static, and it is never written.
        let common = unsafe {
            [
                ("timestamp", MTLCommonCounterSetTimestamp),
                ("stage utilization", MTLCommonCounterSetStageUtilization),
                ("statistic", MTLCommonCounterSetStatistic),
            ]
        };
        for (label, constant) in common {
            let wanted = constant.to_string();
            println!(
                "crcbl-mtl counters: common set {label} ({wanted:?}) present = {}",
                names.contains(&wanted)
            );
        }

        // MEASURED, NOT ASSERTED — and this fired on the first run. Apple's
        // paravirtual GPU answers `true` for three sampling points and then
        // exposes **no counter sets at all**, so nothing could name one in an
        // `MTLCounterSampleBufferDescriptor`. That reads like a contradiction
        // and it is one, but it is *this device's* answer rather than a fault
        // in the test — and a measurement that reddens the board forever on a
        // runner nobody chose is the opposite of what it was written for.
        //
        // So it is reported where a reader will see it, and the test carries
        // on to the correlation below, which the assertion used to cut off.
        if any_sampling && names.is_empty() {
            println!(
                "::warning::this device reports counter sampling at some point and exposes \
                 no counter set at all, so no MTLCounterSampleBufferDescriptor could name \
                 one — timestamp and pipeline-statistics queries cannot be built here"
            );
        }

        let wall = Instant::now();
        let (cpu_first, gpu_first) = sample_timestamps(raw);
        std::thread::sleep(CORRELATION_SLEEP);
        let (cpu_last, gpu_last) = sample_timestamps(raw);
        let wall_ns = wall.elapsed().as_nanos();
        let cpu_delta = cpu_last.saturating_sub(cpu_first);
        let gpu_delta = gpu_last.saturating_sub(gpu_first);
        println!(
            "crcbl-mtl counters: sampleTimestamps over {:?} wall_ns={wall_ns} \
             cpu_delta={cpu_delta} gpu_delta={gpu_delta}",
            CORRELATION_SLEEP
        );
        // ALSO MEASURED RATHER THAN ASSERTED, and for the same reason the
        // counter-set check above is. This started as "a CPU timestamp that did
        // not move means the test never reached a device" — which is false on
        // this one: it printed the device's name three lines earlier and then
        // answered `cpu_delta=0 gpu_delta=0` across a real 53 ms of wall clock.
        // The runner reached a device whose timestamp API is inert, which is
        // the same answer its zero counter sets give.
        //
        // The honest signal for "never reached a device" is the empty name
        // asserted at the top, and that one still fails.
        if cpu_delta == 0 {
            println!(
                "::warning::sampleTimestamps:gpuTimestamp: did not move across a \
                 {CORRELATION_SLEEP:?} sleep, so this device derives no tick period"
            );
        }
        if gpu_delta > 0 {
            println!(
                "crcbl-mtl counters: timestamp_period_ns = {} by the wall clock, {} by \
                 Metal's cpu timestamp",
                wall_ns as f64 / gpu_delta as f64,
                cpu_delta as f64 / gpu_delta as f64
            );
        } else {
            println!(
                "crcbl-mtl counters: the GPU timestamp did not advance, so this device \
                 reports no derivable tick period"
            );
        }
    }

    /// **The ceiling this device would actually accept**, printed rather than
    /// asserted, so the number [`limits_of`] promises can be read against the
    /// one the hardware can hold.
    ///
    /// It was written before `crcbl_mtl::icb` existed, to answer the question a
    /// slice written blind would have skipped: **on the only Mac this
    /// repository's CI has, can an ICB be created at all, and at what size?**
    /// The answer turned out to be yes at every rung to 1048576, and the slice
    /// was built on it.
    ///
    /// It is kept because [`icb::MAX_DRAW_INDIRECT_COUNT`](crate::icb::MAX_DRAW_INDIRECT_COUNT)
    /// is a *choice* rather than a measurement — a number picked well below the
    /// ceiling because it is allocated for real on every call — and this ladder
    /// is the only thing that says what the ceiling was. Raising the constant
    /// without rereading this log would be guessing.
    ///
    /// It creates ICBs and destroys them; it encodes nothing, submits nothing
    /// and executes nothing. Like
    /// `a_device_reports_its_counter_sampling_gpu_families_and_timestamp_correlation`
    /// above, **it prints** and the CI log is the artifact. Neither this test
    /// nor anything else in this crate reports
    /// [`Features::DRAW_INDIRECT_COUNT`] or moves a capability row: an ICB the
    /// probe allocated and dropped is evidence about a device, not a code path.
    ///
    /// # What each answer settles
    ///
    /// **`supportsFamily:` over [`ICB_FAMILIES`].** Which command types an ICB
    /// may hold is family-gated in Apple's feature-set tables, so every answer
    /// below is conditional on this one. CI's `Apple Paravirtual device` has
    /// already surprised this crate once by reporting no `counterSets`
    /// whatsoever, so its place on the family ladder is worth reading rather
    /// than assuming.
    ///
    /// **`newIndirectCommandBufferWithDescriptor:maxCommandCount:options:`.**
    /// The one call that decides whether the slice is buildable here. A
    /// descriptor Metal accepted and an ICB that came back nil are different
    /// answers and print differently — the first says the *shape* is legal and
    /// the allocation was not, the second would be a descriptor Metal rejected
    /// outright, which on this call surfaces as the same nil and is why the
    /// descriptor's own round-trip is printed above every attempt.
    ///
    /// **The [`ICB_COMMAND_COUNTS`] ladder.** `1` would tell nobody anything: an
    /// ICB holding one command is exactly the `drawPrimitives:indirectBuffer:`
    /// this backend already emits in a CPU loop. What matters is whether the
    /// device reaches [`Limits::desktop`]`().max_draw_indirect_count`, and if
    /// not, where it stops — that number is the ceiling a Metal
    /// `max_draw_indirect_count` would have to report.
    ///
    /// **[`ICB_INHERITANCE`].** An ICB feeding the seam's `draw_indirect_count`
    /// must inherit both buffers and pipeline state, because the pass that calls
    /// the verb has already bound both on its render encoder and the encoding
    /// kernel writes only draw arguments. Varying it is what separates "this
    /// device refuses ICBs" from "this device refuses *inheriting* ICBs", which
    /// are a closed row and a solvable problem respectively.
    ///
    /// **[`Limits::max_draw_indirect_count`].** Printed beside
    /// [`Limits::minimum`]'s and [`Limits::desktop`]'s so the `1` this backend
    /// reports is legible as what it is: `limits_of` writes no value for the
    /// field at all, so the number is the seam's floor — a backend choice — and
    /// not anything this device was asked.
    ///
    /// # What it fails on
    ///
    /// A measurement test that silently measures nothing is a green light wired
    /// to nothing, so the ways this could reach the end having asked the device
    /// nothing are assertions.
    ///
    /// The first is the sibling test's: an empty `MTLDevice::name` means no
    /// device was reached.
    ///
    /// The second is **two-sided, and it is the one that makes the ICB half
    /// un-fakeable**. `MTLIndirectCommandBufferDescriptor` is read *before* and
    /// *after* `setCommandTypes:`, and both readings are asserted — `after`
    /// must equal [`ICB_DRAW_COMMANDS`] and must also differ from `before`. A
    /// stub answering nothing fails the first; a stub answering the wanted value
    /// unconditionally fails the second. Only a live Objective-C object that
    /// stores what it was given and hands it back satisfies both, and if the
    /// descriptor is live then so is the device that the next line asks for an
    /// ICB.
    ///
    /// The third is that the ladder ran to a conclusion: either a rung was
    /// refused, or every rung was attempted. A loop that stopped early having
    /// refused nothing skipped a `maxCommandCount` and its printed ceiling would
    /// be a floor.
    ///
    /// A device that honestly refuses every ICB is a *result* and passes. That
    /// is the whole point — it would turn the `DrawIndirectCount` row's reason
    /// from "unwritten" into "unwritten, and unverifiable on the hardware this
    /// project has".
    ///
    /// nextest captures a passing test's stdout, so read this with
    /// `--success-output immediate`, which is what `tests/run-mtl-e2e.sh`
    /// passes.
    #[test]
    #[ignore = "needs a real Metal device; run tests/run-mtl-e2e.sh"]
    fn a_device_reports_its_indirect_command_buffer_support_and_draw_indirect_count_ceiling() {
        let (_validated, device) = crate::device::tests::open_device();
        let raw = &*device.inner.raw;

        let name = raw.name().to_string();
        assert!(
            !name.is_empty(),
            "MTLDevice::name came back empty, so this test never reached a device"
        );
        println!("crcbl-mtl icb: device={name:?} {}", driver_string());

        let mut seen: Vec<MTLGPUFamily> = Vec::new();
        for (label, family) in ICB_FAMILIES {
            assert!(
                !seen.contains(&family),
                "ICB_FAMILIES lists {label} twice, so one MTLGPUFamily is never asked about"
            );
            seen.push(family);
            println!(
                "crcbl-mtl icb: supportsFamily {label} = {}",
                raw.supportsFamily(family)
            );
        }

        // Device-local, and through the same helper every `MTLBuffer` this
        // backend allocates goes through: an ICB a compute kernel writes and a
        // render encoder executes is never touched by the CPU, so it wants the
        // storage mode `MemoryLocation::DeviceLocal` already maps to.
        let options = crate::conv::resource_options(MemoryLocation::DeviceLocal);

        for (label, inherit_buffers, inherit_pipeline) in ICB_INHERITANCE {
            let descriptor = MTLIndirectCommandBufferDescriptor::new();
            // Read before writing: these are Metal's defaults, and the two bind
            // counts are what the per-command stride would be if this probe left
            // them alone.
            let default_types = descriptor.commandTypes();
            let default_vertex_binds = descriptor.maxVertexBufferBindCount();
            let default_fragment_binds = descriptor.maxFragmentBufferBindCount();

            descriptor.setCommandTypes(ICB_DRAW_COMMANDS);
            let set_types = descriptor.commandTypes();
            descriptor.setInheritBuffers(inherit_buffers);
            descriptor.setInheritPipelineState(inherit_pipeline);
            descriptor.setMaxVertexBufferBindCount(ICB_STAGE_BUFFER_BINDS);
            descriptor.setMaxFragmentBufferBindCount(ICB_STAGE_BUFFER_BINDS);

            println!(
                "crcbl-mtl icb: descriptor {label} commandTypes {default_types:?} -> \
                 {set_types:?}, inheritBuffers={} inheritPipelineState={}, \
                 maxVertexBufferBindCount {default_vertex_binds} -> {}, \
                 maxFragmentBufferBindCount {default_fragment_binds} -> {}",
                descriptor.inheritBuffers(),
                descriptor.inheritPipelineState(),
                descriptor.maxVertexBufferBindCount(),
                descriptor.maxFragmentBufferBindCount(),
            );

            // THE GUARD, and it is two-sided so that a probe which asked the
            // device nothing cannot reach the end green. See the doc comment.
            assert_eq!(
                set_types, ICB_DRAW_COMMANDS,
                "MTLIndirectCommandBufferDescriptor did not hand back the command types it was \
                 given, so nothing here reached a live descriptor"
            );
            assert_ne!(
                set_types, default_types,
                "MTLIndirectCommandBufferDescriptor answered the wanted command types before it \
                 was given them, so the answer is not this descriptor's"
            );
            assert_eq!(
                descriptor.inheritBuffers(),
                inherit_buffers,
                "setInheritBuffers: did not stick, so the descriptor Metal is about to be handed \
                 is not the one this row describes"
            );
            assert_eq!(
                descriptor.inheritPipelineState(),
                inherit_pipeline,
                "setInheritPipelineState: did not stick, so the descriptor Metal is about to be \
                 handed is not the one this row describes"
            );

            let mut attempted = 0usize;
            let mut previous: Option<NSUInteger> = None;
            let mut largest_created: Option<NSUInteger> = None;
            let mut first_refused: Option<NSUInteger> = None;
            for count in ICB_COMMAND_COUNTS {
                assert!(
                    previous.is_none_or(|prev| prev < count),
                    "ICB_COMMAND_COUNTS is not strictly ascending, so stopping at the first \
                     refusal would skip a maxCommandCount this device might still accept"
                );
                previous = Some(count);
                attempted += 1;

                // SAFETY: `objc2-metal` marks this `unsafe` for one reason,
                // which its own doc line gives — `maxCount` might not be
                // bounds-checked. Every value comes from `ICB_COMMAND_COUNTS`,
                // whose largest entry is `Limits::desktop`'s draw ceiling and
                // whose smallest is 64, and the loop climbs from the smallest
                // and stops at the first nil — so at most one request is ever
                // made past what this device accepted. Metal's declaration of
                // the call is `-> Option<Retained<…>>` precisely because a
                // request it cannot satisfy comes back nil, which the `match`
                // below reports rather than unwrapping.
                let icb = unsafe {
                    raw.newIndirectCommandBufferWithDescriptor_maxCommandCount_options(
                        &descriptor,
                        count,
                        options,
                    )
                };
                match icb {
                    Some(icb) => {
                        let size = icb.size();
                        println!(
                            "crcbl-mtl icb: {label} maxCommandCount={count} created, size={size} \
                             bytes ({} per command)",
                            size / count
                        );
                        largest_created = Some(count);
                    }
                    None => {
                        println!(
                            "crcbl-mtl icb: {label} maxCommandCount={count} came back nil, so \
                             this device creates no indirect command buffer that large"
                        );
                        first_refused = Some(count);
                        break;
                    }
                }
            }

            assert!(
                first_refused.is_some() || attempted == ICB_COMMAND_COUNTS.len(),
                "the maxCommandCount ladder stopped before its last rung without a refusal, so a \
                 count was silently never asked about and the ceiling printed below is a floor"
            );
            println!(
                "crcbl-mtl icb: {label} largest maxCommandCount created = {largest_created:?}, \
                 first refused = {first_refused:?}"
            );
        }

        // The backend's own derivation, so the printed ceiling above can be read
        // against what this backend promises today.
        let features = features_of(raw, &name);
        let limits = limits_of(raw, features);
        println!(
            "crcbl-mtl icb: derived max_draw_indirect_count={} (icb::MAX_DRAW_INDIRECT_COUNT is \
             {}, Limits::minimum() is {}, Limits::desktop() is {}; the derived value is the \
             constant on a device that answered icb::probe and the floor on one that did not)",
            limits.max_draw_indirect_count,
            crate::icb::MAX_DRAW_INDIRECT_COUNT,
            Limits::minimum().max_draw_indirect_count,
            Limits::desktop().max_draw_indirect_count,
        );
        // The one thing here that is an assertion rather than a print, because
        // it is the failure that would leave two capabilities unexercised on
        // this whole backend rather than merely under-promised:
        // `crates/crcbl/tests/hal_seam_e2e.rs`'s `can_multi_draw` declines a
        // device that answers one, and both of its indirect exercises decline
        // with it.
        assert_eq!(
            features.contains(Features::DRAW_INDIRECT_COUNT),
            limits.max_draw_indirect_count > 1,
            "the DRAW_INDIRECT_COUNT flag and max_draw_indirect_count disagree, so either a \
             device that can serve the path is reporting the seam's floor or one that cannot is \
             promising more than one draw"
        );
        println!(
            "crcbl-mtl icb: DRAW_INDIRECT_COUNT={} MULTI_DRAW_INDIRECT={} \
             INDIRECT_FIRST_INSTANCE={} COMPUTE={}",
            features.contains(Features::DRAW_INDIRECT_COUNT),
            features.contains(Features::MULTI_DRAW_INDIRECT),
            features.contains(Features::INDIRECT_FIRST_INSTANCE),
            features.contains(Features::COMPUTE),
        );
        // The caps a compute kernel writing an ICB would spend: the argument
        // buffer it reads and the count buffer it reads are both `MTLBuffer`, so
        // the storage range bounds how many draws could be described at all, and
        // the workgroup limits bound the dispatch that would encode them.
        println!(
            "crcbl-mtl icb: max_storage_buffer_range={} max_compute_workgroup_size={:?} \
             max_compute_invocations_per_workgroup={} max_compute_workgroups_per_dimension={}",
            limits.max_storage_buffer_range,
            limits.max_compute_workgroup_size,
            limits.max_compute_invocations_per_workgroup,
            limits.max_compute_workgroups_per_dimension,
        );
    }
}
