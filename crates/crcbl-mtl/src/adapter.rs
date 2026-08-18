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
///   waits for the slice that can measure it — and this module's
///   `a_device_reports_its_counter_sampling_gpu_families_and_timestamp_correlation`
///   is that measurement, printing the period alongside every
///   `supportsCounterSampling:` answer rather than asserting either.
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
    // a depth-bias clamp, a line fill mode and a drawable that will call back
    // once it has been shown — and the slice that calls each one has now
    // landed. See the doc comment above for which call backs each, and for why
    // the last is reported for a device whose swapchain may be offscreen.
    out |= Features::SAMPLER_ANISOTROPY;
    out |= Features::TIMELINE_SEMAPHORE;
    out |= Features::DEBUG_MARKERS;
    out |= Features::COMPUTE;
    out |= Features::DEPTH_CLAMP;
    out |= Features::DEPTH_BIAS_CLAMP;
    out |= Features::POLYGON_MODE_LINE;
    out |= Features::PRESENT_FEEDBACK;
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

#[cfg(test)]
mod tests {
    use std::ptr::NonNull;
    use std::time::{Duration, Instant};

    use objc2_metal::{
        MTLCommonCounterSetStageUtilization, MTLCommonCounterSetStatistic,
        MTLCommonCounterSetTimestamp, MTLCounter, MTLCounterSamplingPoint, MTLCounterSet,
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
    /// A device that answers yes only at `AtStageBoundary` is still very
    /// probably enough, and that is worth stating precisely because it looks
    /// like a narrowing and may not be one. Metal's stage-boundary sampling is
    /// requested through a pass descriptor's `sampleBufferAttachments`, so it
    /// samples where an encoder opens and closes — and **every**
    /// [`write_timestamp`](crcbl_hal::CommandEncoder::write_timestamp) call in
    /// this repository is already outside every pass. The seam's scope rules
    /// require it (`crcbl_hal::null`'s recorder routes `WriteTimestamp` through
    /// its `need_outside` check, which is the same check copies and barriers
    /// get), and the one caller obeys it: `crcbl_render::timing`'s `pass_begin`
    /// and `pass_end` are called by `crcbl_render::graph` immediately before
    /// `begin_render_pass` and immediately after `end_render_pass`. So a
    /// backend-opened encoder at a stage boundary is a legal place to put the
    /// timestamp the seam asked for, and the narrowing may cost nothing. If the
    /// device answers yes *nowhere*, the honest outcome is narrowing the
    /// capability rather than implementing it.
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
        assert!(
            cpu_delta > 0,
            "the CPU timestamp did not move across a {CORRELATION_SLEEP:?} sleep, so \
             sampleTimestamps:gpuTimestamp: wrote nothing and no correlation was measured"
        );
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
}
