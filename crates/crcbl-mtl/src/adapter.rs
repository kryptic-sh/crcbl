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
    let features = features_of(device);
    AdapterInfo {
        id: AdapterId(index),
        name: device.name().to_string(),
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
/// * [`Features::COMPUTE`] — unconditional in Metal, and backed by
///   `crcbl_mtl::pipeline`'s `create_compute_pipeline_impl`:
///   `MTLDevice::newComputePipelineStateWithDescriptor:options:reflection:error:`
///   is a real call this backend makes, so a compute pipeline can now be built.
///   **What is still missing is the dispatch**, and it is missing for a seam
///   reason rather than a Metal one: `dispatchThreadgroups:threadsPerThreadgroup:`
///   takes the workgroup size at the *call*, MSL has nowhere to declare it, and
///   [`ComputePipelineDesc`](crcbl_hal::ComputePipelineDesc) carries no field
///   for it. `crcbl_mtl::command`'s `dispatch` refuses by name, so the flag
///   says "compute pipelines exist" and nothing more.
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
/// * [`Features::DEPTH_CLAMP`], [`Features::DEPTH_BIAS_CLAMP`] and
///   [`Features::POLYGON_MODE_LINE`] — all three are unconditional in Metal
///   and all three are now replayed onto the render encoder by
///   `bind_graphics_pipeline`: `setDepthClipMode:`,
///   `setDepthBias:slopeScale:clamp:` (whose third argument *is* the clamp) and
///   `setTriangleFillMode:`. They are reported because those calls are made,
///   which is the same standard `SAMPLER_ANISOTROPY` and `TIMELINE_SEMAPHORE`
///   were held to.
///
/// # Absent, with the reason for each
///
/// * [`Features::OCCLUSION_QUERY`] — `visibilityResultBuffer` is a property of
///   the *render pass descriptor* and needs somewhere to put the results, which
///   is a query set; `create_query_set` refuses, so this would be a promise
///   about a call this backend cannot make.
/// * [`Features::DESCRIPTOR_INDEXING`] — **removed by the binding slice, and
///   this is the entry worth reading before changing anything here.**
///   `argumentBuffersSupport` does report `MTLArgumentBuffersTier::Tier2` on
///   every machine this backend targets, and MTL1 reported the flag from it.
///   What has changed is that bind groups now exist, and `crcbl_mtl::binding`
///   binds Metal's **flat argument tables** rather than argument buffers —
///   because every MSL artifact `crcbl-shaders` commits declares plain
///   `[[buffer(n)]]`/`[[texture(n)]]`/`[[sampler(n)]]` arguments, which an
///   argument buffer cannot feed. A flat table has no runtime-sized array, so
///   `create_bind_group_layout` refuses every
///   [`BindingFlags`](crcbl_hal::BindingFlags), and `crcbl_hal::pipeline` is
///   explicit that a backend which refuses them must not report the feature.
///   Reporting it while refusing the layouts it promises is the shape this
///   crate treats as a lie. It comes back with argument buffers, which need
///   `crcbl-shaders` to emit MSL declaring them.
/// * [`Features::DRAW_INDIRECT_COUNT`] — the count comes from **GPU memory**,
///   and Metal's only execution that reads one is
///   `executeCommandsInBuffer:indirectBuffer:indirectBufferOffset:` over an
///   `MTLIndirectCommandBuffer` whose commands already exist. Encoding those
///   commands from the seam's argument buffer needs a compute kernel running
///   before the render encoder the seam calls `draw_indirect_count` *inside*
///   was opened, which this backend's encode-straight-through shape cannot
///   reach. This is the one Tier A feature still missing, and it is why the
///   derived tier is still B; see the crate docs.
/// * [`Features::TIMESTAMP_QUERY`] and
///   [`Features::PIPELINE_STATISTICS_QUERY`] — `supportsCounterSampling:` would
///   answer both, but reporting them obliges a
///   [`Limits::timestamp_period_ns`], and Metal has no fixed tick period to
///   report: the GPU clock is correlated to the host's through
///   `sampleTimestamps:gpuTimestamp:` at sample time. A fabricated period is
///   exactly the number this workspace refuses to write down, so the feature
///   waits for the slice that can measure it.
/// * [`Features::ASYNC_COMPUTE_QUEUE`] and [`Features::TRANSFER_QUEUE`] —
///   Metal has one `MTLCommandQueue` type and no queue families at all, which
///   `crcbl_hal::QueueKind` already records as the reason it is not named
///   `QueueFamily`.
/// * [`Features::PUSH_CONSTANTS`] — `setVertexBytes:length:atIndex:` and its
///   siblings are the closest fit and cap at 4 KiB, and the binding slice
///   sharpened the obstacle rather than removing it. The block competes for the
///   same buffer table as every bind group's buffers, and the artifact that
///   uses one — `msl/ui.metal` — has Slang place it at `buffer(0)`, **ahead of
///   the bound buffers**, which no flattening of a
///   [`PipelineLayoutDesc`](crcbl_hal::PipelineLayoutDesc) can reproduce.
///   `crcbl_mtl::pipeline`'s `create_pipeline_layout` refuses a push-constant
///   range by name and says so, which is what `crcbl_hal::pipeline` requires of
///   a backend without the feature.
/// * [`Features::SHADER_DEBUG_PRINTF`] — `MTLLogState` exists, and nothing
///   routes it into `log` yet.
fn features_of(device: &ProtocolObject<dyn MTLDevice>) -> Features {
    let mut out = Features::empty();
    if device.supportsFamily(MTLGPUFamily::Metal3) {
        out |= Features::BUFFER_DEVICE_ADDRESS;
    }
    if device.supportsBCTextureCompression() {
        out |= Features::TEXTURE_COMPRESSION_BC;
    }
    // No query for any of these: every Metal device has anisotropic sampling,
    // `MTLSharedEvent`, `pushDebugGroup:`, compute pipelines, depth clamping,
    // a depth-bias clamp and a line fill mode — and the slice that calls each
    // one has now landed. See the doc comment above for which call backs each.
    out |= Features::SAMPLER_ANISOTROPY;
    out |= Features::TIMELINE_SEMAPHORE;
    out |= Features::DEBUG_MARKERS;
    out |= Features::COMPUTE;
    out |= Features::DEPTH_CLAMP;
    out |= Features::DEPTH_BIAS_CLAMP;
    out |= Features::POLYGON_MODE_LINE;
    // Likewise no query: every Metal device takes an indirect buffer on a draw
    // and reads `baseInstance` out of it, and the binding slice's `indirect`
    // loop is the call that makes both true of this backend.
    out |= Features::MULTI_DRAW_INDIRECT;
    out |= Features::INDIRECT_FIRST_INSTANCE;
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
/// reported, and the bindless capacity, the push-constant budget and the
/// timestamp period all stay at the floor's zeroes because their features are
/// absent — `max_bindless_descriptors` since the binding slice, which took
/// [`Features::DESCRIPTOR_INDEXING`] off for the reason `features_of` gives.
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
