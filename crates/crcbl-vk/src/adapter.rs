//! Physical-device enumeration, and honest capability reporting.
//!
//! # Honest, not optimistic
//!
//! Every path selector — [`DeviceCaps::binding_model`](crcbl_hal::DeviceCaps::binding_model)
//! and its two siblings — is *derived* from [`Features`], so a backend cannot
//! claim the bindless model while missing what bindless is built on; the seam
//! made that structurally impossible on purpose. What a backend *can* still do
//! is lie in the other direction: set
//! `DESCRIPTOR_INDEXING` because the extension is present without checking that
//! the specific sub-features the bindless model needs are. [`features_of`]
//! therefore spells every sub-feature out, and the P1 CI matrix — radv on the
//! developer's machine, lavapipe in CI — is what proves the answer differs
//! between implementations rather than being copied from one of them.
//!
//! # Queue families
//!
//! `QueueKind` deliberately is not `QueueFamily`, but a Vulkan backend still
//! has to map one onto the other, and [`QueueFamilies::select`] is where that
//! happens: graphics+compute always, a compute-without-graphics family for
//! [`Features::ASYNC_COMPUTE_QUEUE`], and a transfer-only family for
//! [`Features::TRANSFER_QUEUE`]. The MVP submits everything to the first one
//! (`docs/plan/02-vulkan-backend.md` §2.1), but the seam models queues plural
//! from the start and reporting the other two honestly is what makes a later
//! async-compute slice additive.

use core::ffi::CStr;

use ash::{ext, khr, vk};

use crcbl_hal::{AdapterId, AdapterInfo, BackendKind, DeviceCaps, DeviceType, Features, Limits};

use crate::present_timing;

/// One enumerated physical device, with everything selection needs.
#[derive(Clone, Debug)]
pub(crate) struct AdapterRecord {
    pub(crate) physical: vk::PhysicalDevice,
    pub(crate) info: AdapterInfo,
    pub(crate) families: QueueFamilies,
    /// The three Vulkan 1.3 features this backend cannot work without. Reported
    /// separately from [`Features`] because the seam has no vocabulary for
    /// them: they are not optional capabilities, they are the backend's floor.
    pub(crate) core_1_3: Core13Support,
}

/// Which of the Vulkan 1.3 baseline features a device has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Core13Support {
    pub(crate) dynamic_rendering: bool,
    pub(crate) synchronization2: bool,
    pub(crate) maintenance4: bool,
}

impl Core13Support {
    pub(crate) fn is_complete(self) -> bool {
        self.dynamic_rendering && self.synchronization2 && self.maintenance4
    }

    /// The missing ones, named, for an error a reader can act on.
    pub(crate) fn missing(self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if !self.dynamic_rendering {
            missing.push("dynamicRendering");
        }
        if !self.synchronization2 {
            missing.push("synchronization2");
        }
        if !self.maintenance4 {
            missing.push("maintenance4");
        }
        missing
    }
}

/// The queue-family indices behind [`QueueKind`](crcbl_hal::QueueKind).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct QueueFamilies {
    /// Graphics + compute + transfer. Always present on a device this backend
    /// will open; the MVP uses only this one.
    pub(crate) graphics: Option<u32>,
    /// Compute without graphics — a genuinely independent queue.
    pub(crate) async_compute: Option<u32>,
    /// Transfer without graphics or compute — a DMA engine.
    pub(crate) transfer: Option<u32>,
}

impl QueueFamilies {
    /// Picks a family per kind from a device's family list.
    ///
    /// Pure, so the selection rules are unit-tested against the family layouts
    /// real drivers actually report (AMD's 1+1+1, Intel's single family,
    /// lavapipe's single family) without a device in the room.
    #[must_use]
    pub(crate) fn select(families: &[vk::QueueFamilyProperties]) -> Self {
        let flags = |index: usize| families[index].queue_flags;
        let usable = |index: usize| families[index].queue_count > 0;

        let graphics = (0..families.len())
            .filter(|index| usable(*index))
            .find(|index| {
                flags(*index).contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
            });
        // "Async" means *independent of graphics*. A second queue on the
        // graphics family is not async compute; it shares the same hardware
        // scheduler and buys nothing the seam's flag promises.
        let async_compute = (0..families.len())
            .filter(|index| usable(*index))
            .find(|index| {
                flags(*index).contains(vk::QueueFlags::COMPUTE)
                    && !flags(*index).contains(vk::QueueFlags::GRAPHICS)
            });
        let transfer = (0..families.len())
            .filter(|index| usable(*index))
            .find(|index| {
                flags(*index).contains(vk::QueueFlags::TRANSFER)
                    && !flags(*index).intersects(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
            });

        #[allow(clippy::cast_possible_truncation)]
        Self {
            graphics: graphics.map(|index| index as u32),
            async_compute: async_compute.map(|index| index as u32),
            transfer: transfer.map(|index| index as u32),
        }
    }

    /// The distinct family indices a `vkCreateDevice` must ask for.
    pub(crate) fn distinct(self) -> Vec<u32> {
        let mut indices: Vec<u32> = [self.graphics, self.async_compute, self.transfer]
            .into_iter()
            .flatten()
            .collect();
        indices.sort_unstable();
        indices.dedup();
        indices
    }
}

/// Whether a `vkEnumerateDeviceExtensionProperties` list names `name`.
///
/// The device-side counterpart of the instance's own intersection, and it
/// exists for the same reason: **requesting an extension the device does not
/// have fails `vkCreateDevice` outright**, so anything optional has to be
/// probed before it is asked for. Pure, so the "is it there" half of an
/// optional extension is testable without a driver.
///
/// A property whose name is not NUL-terminated is a driver bug and answers
/// `false` rather than panicking — the honest reading of a name nothing can
/// compare against.
#[must_use]
pub(crate) fn has_device_extension(properties: &[vk::ExtensionProperties], name: &CStr) -> bool {
    properties.iter().any(|entry| {
        entry
            .extension_name_as_c_str()
            .is_ok_and(|found| found == name)
    })
}

/// The capabilities no core feature struct can answer.
///
/// Every member is an extension whose presence was probed **and** a feature bit
/// that came back `VK_TRUE` — the module's "honest, not optimistic" rule applied
/// to the half of [`Features`] that lives outside `VkPhysicalDeviceFeatures` and
/// the version structs. [`describe`] establishes them all before
/// [`features_of`] is called, which is why that function takes answers rather
/// than a driver.
///
/// A struct rather than a row of `bool` parameters: eight positional
/// `true, false, false, …` arguments say nothing at a call site about which
/// capability each one is, and a transposed pair would report the wrong flag
/// and still compile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExtensionSupport {
    /// `VK_EXT_debug_utils` — an *instance* extension, and the only member here
    /// the physical device has no say in.
    pub(crate) debug_utils: bool,
    /// `VK_KHR_present_id` + `VK_KHR_present_wait`, both feature bits granted.
    pub(crate) present_feedback: bool,
    /// The whole `VK_EXT_present_timing` chain, both feature bits granted.
    pub(crate) present_timing: bool,
    /// `VK_EXT_mesh_shader` with `meshShader`.
    pub(crate) mesh_shader: bool,
    /// The same extension with `taskShader`, and only alongside `mesh_shader`:
    /// the amplification stage exists to launch mesh workgroups, so a task
    /// stage with no mesh stage to feed is not a capability anything can use.
    pub(crate) task_shader: bool,
    /// `VK_KHR_acceleration_structure` with `accelerationStructure`.
    pub(crate) acceleration_structure: bool,
    /// `VK_KHR_ray_query` with `rayQuery`, and only alongside
    /// `acceleration_structure` — a ray has nothing to traverse otherwise, and
    /// [`LightingPath::RayTraced`](crcbl_hal::LightingPath::RayTraced) needs
    /// both flags, so a half-reported pair would select a path the device
    /// cannot run.
    pub(crate) ray_query: bool,
    /// `VK_KHR_ray_tracing_pipeline` with `rayTracingPipeline`, and only
    /// alongside `acceleration_structure`, for the same reason.
    pub(crate) ray_tracing_pipeline: bool,
}

/// Which of the mesh-shading and ray-tracing device extensions a device lists.
///
/// Kept apart from the feature bits because the list is needed at its own
/// moment: it decides which feature structs may be *chained* onto
/// `VkPhysicalDeviceFeatures2` at all — chaining one whose extension the device
/// lacks reads back as a zeroed "no" for the wrong reason, exactly as the 1.3
/// and present-timing structs in [`describe`] already document — and the bits
/// then decide the answer. Probing once and carrying the result is what keeps
/// the two halves from drifting apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StageExtensions {
    /// `VK_EXT_mesh_shader`, which carries both the mesh and the task stage.
    pub(crate) mesh_shader: bool,
    /// `VK_KHR_acceleration_structure` **and**
    /// `VK_KHR_deferred_host_operations`, which `vk.xml` declares it depends
    /// on. An extension chain is not partially satisfiable — the same rule the
    /// present-timing chain is probed under, and the reason one name covers
    /// two here.
    pub(crate) acceleration_structure: bool,
    /// `VK_KHR_ray_query`.
    pub(crate) ray_query: bool,
    /// `VK_KHR_ray_tracing_pipeline`.
    pub(crate) ray_tracing_pipeline: bool,
}

impl StageExtensions {
    /// Reads the device's extension list. Pure, so the "is it there" half is
    /// testable without a driver.
    #[must_use]
    pub(crate) fn probe(properties: &[vk::ExtensionProperties]) -> Self {
        Self {
            mesh_shader: has_device_extension(properties, ext::mesh_shader::NAME),
            acceleration_structure: has_device_extension(
                properties,
                khr::acceleration_structure::NAME,
            ) && has_device_extension(
                properties,
                khr::deferred_host_operations::NAME,
            ),
            ray_query: has_device_extension(properties, khr::ray_query::NAME),
            ray_tracing_pipeline: has_device_extension(properties, khr::ray_tracing_pipeline::NAME),
        }
    }
}

impl ExtensionSupport {
    /// Folds the mesh-shading and ray-tracing answers into a support set that
    /// already carries the debug and present ones.
    ///
    /// Both halves of every answer, and then the dependencies *between* the
    /// answers: **a capability is reported only when everything it stands on is
    /// reported too.** `RAY_QUERY` without `ACCELERATION_STRUCTURE` is not a
    /// device that traces fewer rays, it is a device that cannot trace any —
    /// and reporting the pair half-set is what would put such a device on
    /// [`LightingPath::RayTraced`](crcbl_hal::LightingPath::RayTraced).
    ///
    /// Pure, so every one of those dependencies is testable against a synthetic
    /// device: the extension present with the bit clear, the bit set with the
    /// extension absent, and the acceleration structure withdrawn from under
    /// both ray capabilities at once.
    #[must_use]
    pub(crate) fn with_stages(
        mut self,
        listed: StageExtensions,
        mesh: &vk::PhysicalDeviceMeshShaderFeaturesEXT<'_>,
        acceleration: &vk::PhysicalDeviceAccelerationStructureFeaturesKHR<'_>,
        ray_query: &vk::PhysicalDeviceRayQueryFeaturesKHR<'_>,
        ray_tracing_pipeline: &vk::PhysicalDeviceRayTracingPipelineFeaturesKHR<'_>,
    ) -> Self {
        self.mesh_shader = listed.mesh_shader && mesh.mesh_shader == vk::TRUE;
        self.task_shader = self.mesh_shader && mesh.task_shader == vk::TRUE;
        self.acceleration_structure =
            listed.acceleration_structure && acceleration.acceleration_structure == vk::TRUE;
        self.ray_query =
            self.acceleration_structure && listed.ray_query && ray_query.ray_query == vk::TRUE;
        self.ray_tracing_pipeline = self.acceleration_structure
            && listed.ray_tracing_pipeline
            && ray_tracing_pipeline.ray_tracing_pipeline == vk::TRUE;
        self
    }
}

/// Maps a Vulkan device type onto the seam's.
#[must_use]
fn device_type(raw: vk::PhysicalDeviceType) -> DeviceType {
    match raw {
        vk::PhysicalDeviceType::DISCRETE_GPU => DeviceType::Discrete,
        vk::PhysicalDeviceType::INTEGRATED_GPU => DeviceType::Integrated,
        // lavapipe, swiftshader, WARP — the P1 golden-image CI target.
        vk::PhysicalDeviceType::CPU => DeviceType::Cpu,
        vk::PhysicalDeviceType::VIRTUAL_GPU => DeviceType::Virtual,
        _ => DeviceType::Other,
    }
}

/// Everything the seam's [`Features`] asks about, answered from the real
/// feature structs.
///
/// Pure so that "which features does this device have" can be tested against a
/// synthetic feature set — including the bindless boundary, which is the answer
/// most likely to differ between radv and lavapipe.
#[must_use]
pub(crate) fn features_of(
    core: &vk::PhysicalDeviceFeatures,
    vulkan_1_2: &vk::PhysicalDeviceVulkan12Features<'_>,
    limits: &vk::PhysicalDeviceLimits,
    families: QueueFamilies,
    extensions: ExtensionSupport,
) -> Features {
    let mut features = Features::empty();

    // The full bindless set the seam documents, not merely "the extension is
    // present". `BindingModel::Bindless` needs runtime-sized arrays, partial
    // binding and update-after-bind together; any one of them missing means the
    // bindless path cannot run, so any one of them missing must demote.
    if vulkan_1_2.descriptor_indexing == vk::TRUE
        && vulkan_1_2.runtime_descriptor_array == vk::TRUE
        && vulkan_1_2.descriptor_binding_partially_bound == vk::TRUE
        && vulkan_1_2.descriptor_binding_sampled_image_update_after_bind == vk::TRUE
        && vulkan_1_2.descriptor_binding_storage_buffer_update_after_bind == vk::TRUE
        && vulkan_1_2.descriptor_binding_variable_descriptor_count == vk::TRUE
        && vulkan_1_2.shader_sampled_image_array_non_uniform_indexing == vk::TRUE
        && vulkan_1_2.shader_storage_buffer_array_non_uniform_indexing == vk::TRUE
    {
        features |= Features::DESCRIPTOR_INDEXING;
    }
    if vulkan_1_2.buffer_device_address == vk::TRUE {
        features |= Features::BUFFER_DEVICE_ADDRESS;
    }
    if vulkan_1_2.draw_indirect_count == vk::TRUE {
        features |= Features::DRAW_INDIRECT_COUNT;
    }
    if vulkan_1_2.timeline_semaphore == vk::TRUE {
        features |= Features::TIMELINE_SEMAPHORE;
    }
    if core.multi_draw_indirect == vk::TRUE {
        features |= Features::MULTI_DRAW_INDIRECT;
    }
    if core.draw_indirect_first_instance == vk::TRUE {
        features |= Features::INDIRECT_FIRST_INSTANCE;
    }
    if core.pipeline_statistics_query == vk::TRUE {
        features |= Features::PIPELINE_STATISTICS_QUERY;
    }
    if core.occlusion_query_precise == vk::TRUE {
        features |= Features::OCCLUSION_QUERY;
    }
    if core.depth_clamp == vk::TRUE {
        features |= Features::DEPTH_CLAMP;
    }
    if core.depth_bias_clamp == vk::TRUE {
        features |= Features::DEPTH_BIAS_CLAMP;
    }
    if core.fill_mode_non_solid == vk::TRUE {
        features |= Features::POLYGON_MODE_LINE;
    }
    if core.texture_compression_bc == vk::TRUE {
        features |= Features::TEXTURE_COMPRESSION_BC;
    }
    if core.sampler_anisotropy == vk::TRUE {
        features |= Features::SAMPLER_ANISOTROPY;
    }

    // Every Vulkan device has compute; the flag exists so a GL-ES2-class wgpu
    // fallback has somewhere to say otherwise.
    if families.graphics.is_some() {
        features |= Features::COMPUTE;
    }
    if families.async_compute.is_some() {
        features |= Features::ASYNC_COMPUTE_QUEUE;
    }
    if families.transfer.is_some() {
        features |= Features::TRANSFER_QUEUE;
    }

    if limits.max_push_constants_size > 0 {
        features |= Features::PUSH_CONSTANTS;
    }
    // A timestamp period of zero means the ticks are meaningless, so the
    // profiler HUD must degrade rather than divide by it.
    if limits.timestamp_compute_and_graphics == vk::TRUE && limits.timestamp_period > 0.0 {
        features |= Features::TIMESTAMP_QUERY;
    }
    if extensions.debug_utils {
        features |= Features::DEBUG_MARKERS;
    }
    // Both extensions *and* both feature bits, which is what `describe`
    // establishes before it calls this — the only place the argument is
    // computed. Anything less is the optimistic lie the module docs warn
    // about: `VK_KHR_present_wait` being listed says the driver knows the
    // name, not that `presentWait` came back `VK_TRUE`.
    if extensions.present_feedback {
        features |= Features::PRESENT_FEEDBACK;
    }
    // The whole four-extension chain *and* both feature bits, established by
    // `describe` for the same reason as the pair above. A shorter check would
    // promise an answer this backend cannot obtain.
    if extensions.present_timing {
        features |= Features::PRESENT_TIMING;
    }
    // The five capabilities `ExtensionSupport::with_stages` resolved, each
    // already carrying its extension, its feature bit and whatever it depends
    // on. They are the only flags in this function a *core* feature struct is
    // silent about, and they are what makes `GeometryPath` and `LightingPath`
    // able to select anything above their floor.
    if extensions.mesh_shader {
        features |= Features::MESH_SHADER;
    }
    if extensions.task_shader {
        features |= Features::TASK_SHADER;
    }
    if extensions.acceleration_structure {
        features |= Features::ACCELERATION_STRUCTURE;
    }
    if extensions.ray_query {
        features |= Features::RAY_QUERY;
    }
    if extensions.ray_tracing_pipeline {
        features |= Features::RAY_TRACING_PIPELINE;
    }
    // SHADER_DEBUG_PRINTF needs the validation layer's printf feature wired to
    // a message handler; it lands with the debug UI at P7 and is
    // reported absent until it works, rather than claimed and dropped.

    features
}

/// Maps Vulkan limits onto the seam's.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn limits_of(
    limits: &vk::PhysicalDeviceLimits,
    vulkan_1_2_props: &vk::PhysicalDeviceVulkan12Properties<'_>,
    features: Features,
) -> Limits {
    Limits {
        max_image_2d: limits.max_image_dimension2_d,
        max_image_3d: limits.max_image_dimension3_d,
        max_image_array_layers: limits.max_image_array_layers,
        max_storage_buffer_range: u64::from(limits.max_storage_buffer_range),
        max_uniform_buffer_range: u64::from(limits.max_uniform_buffer_range),
        max_bind_groups: limits.max_bound_descriptor_sets,
        max_bindless_descriptors: if features.contains(Features::DESCRIPTOR_INDEXING) {
            // The binding that matters for the bindless texture array. The
            // update-after-bind limit is the real ceiling, not the plain one.
            vulkan_1_2_props.max_descriptor_set_update_after_bind_sampled_images
        } else {
            0
        },
        max_push_constant_size: if features.contains(Features::PUSH_CONSTANTS) {
            limits.max_push_constants_size
        } else {
            0
        },
        max_color_attachments: limits.max_color_attachments,
        max_sample_count: common_sample_count(limits),
        max_draw_indirect_count: if features.contains(Features::DRAW_INDIRECT_COUNT) {
            limits.max_draw_indirect_count
        } else {
            1
        },
        max_compute_workgroup_size: limits.max_compute_work_group_size,
        max_compute_invocations_per_workgroup: limits.max_compute_work_group_invocations,
        max_compute_workgroups_per_dimension: limits
            .max_compute_work_group_count
            .into_iter()
            .min()
            .unwrap_or(0),
        min_uniform_buffer_offset_alignment: limits.min_uniform_buffer_offset_alignment,
        min_storage_buffer_offset_alignment: limits.min_storage_buffer_offset_alignment,
        optimal_buffer_copy_offset_alignment: limits.optimal_buffer_copy_offset_alignment,
        max_sampler_anisotropy: if features.contains(Features::SAMPLER_ANISOTROPY) {
            limits.max_sampler_anisotropy
        } else {
            1.0
        },
        timestamp_period_ns: if features.contains(Features::TIMESTAMP_QUERY) {
            limits.timestamp_period
        } else {
            0.0
        },
    }
}

/// The highest sample count usable for **both** a colour and a depth
/// attachment.
///
/// Vulkan reports a separate mask per attachment kind and they genuinely
/// differ; the seam has one number, so the honest one is the intersection —
/// a caller told "8" for colour and handed a 4x-only depth format has been
/// lied to. Always a power of two, and never below 1: a driver reporting an
/// empty mask is confused rather than authoritative, and every device supports
/// one sample.
#[must_use]
fn common_sample_count(limits: &vk::PhysicalDeviceLimits) -> u32 {
    let common = limits.framebuffer_color_sample_counts.as_raw()
        & limits.framebuffer_depth_sample_counts.as_raw()
        & limits.sampled_image_color_sample_counts.as_raw();
    // `VkSampleCountFlagBits` is `1 << log2(samples)`, so the highest set bit
    // *is* the count.
    if common == 0 {
        1
    } else {
        (1u32 << (31 - common.leading_zeros())).min(64)
    }
}

/// Enumerates every physical device the instance can see.
///
/// Discrete devices are listed first (`docs/plan/02-vulkan-backend.md` §2.1:
/// "prefer discrete"), so a caller taking `adapters()[0]` gets the right answer
/// without knowing the rule — which is what `apps/sandbox` does.
pub(crate) fn enumerate(
    instance: &ash::Instance,
    debug_utils: bool,
    surface_caps2: bool,
) -> Vec<AdapterRecord> {
    // SAFETY: `instance` is live and the call only reads driver-owned tables.
    let physicals = match unsafe { instance.enumerate_physical_devices() } {
        Ok(physicals) => physicals,
        Err(error) => {
            log::warn!("crcbl-vk: vkEnumeratePhysicalDevices failed: {error:?}");
            return Vec::new();
        }
    };

    let mut records: Vec<AdapterRecord> = physicals
        .into_iter()
        .map(|physical| describe(instance, physical, debug_utils, surface_caps2))
        .collect();

    // **Openability first, speed second.** The crate docs say `apps/sandbox`
    // takes `adapters()[0]` blind, so an adapter this backend cannot open must
    // never be first — and sorting on device type alone put an unopenable
    // discrete GPU ahead of a perfectly good integrated one, which surfaces as
    // "crcbl refuses to start on this laptop".
    records.sort_by_key(|record| {
        let openable = u8::from(!record.core_1_3.is_complete());
        let speed = match record.info.device_type {
            DeviceType::Discrete => 0,
            DeviceType::Integrated => 1,
            DeviceType::Virtual => 2,
            DeviceType::Other => 3,
            // Last among openable ones: a software rasteriser is correct and
            // slow, and picking it by accident on a machine with a GPU looks
            // like a performance bug.
            DeviceType::Cpu => 4,
        };
        (openable, speed)
    });
    for (index, record) in records.iter_mut().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        {
            record.info.id = AdapterId(index as u32);
        }
    }
    records
}

fn describe(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    debug_utils: bool,
    surface_caps2: bool,
) -> AdapterRecord {
    let mut vulkan_1_2_props = vk::PhysicalDeviceVulkan12Properties::default();
    let mut properties2 = vk::PhysicalDeviceProperties2::default().push_next(&mut vulkan_1_2_props);
    // SAFETY: `physical` came from this instance, and the chained structs are
    // stack locals that outlive the call.
    unsafe { instance.get_physical_device_properties2(physical, &mut properties2) };
    let properties = properties2.properties;

    // SAFETY: as above; the call only reads driver-owned tables.
    let device_extensions =
        match unsafe { instance.enumerate_device_extension_properties(physical) } {
            Ok(properties) => properties,
            Err(error) => {
                // Every optional extension then reads as absent, which is the safe
                // direction: nothing is requested, `vkCreateDevice` still succeeds,
                // and the capability is reported missing rather than promised.
                log::warn!("crcbl-vk: vkEnumerateDeviceExtensionProperties failed: {error:?}");
                Vec::new()
            }
        };
    // Both or neither: numbering a present buys nothing without a way to wait
    // on the number, and the wait has nothing to wait for without the number.
    let present_feedback_extensions =
        has_device_extension(&device_extensions, khr::present_id::NAME)
            && has_device_extension(&device_extensions, khr::present_wait::NAME);
    // All four or none. `vk.xml` declares `VK_EXT_present_timing` as depending
    // on `VK_KHR_swapchain + VK_KHR_present_id2 +
    // VK_KHR_get_surface_capabilities2 + VK_KHR_calibrated_timestamps`, and an
    // extension chain is not partially satisfiable: asking for a dependent
    // extension without its dependency is invalid usage that a validation layer
    // flags and a driver may simply not honour. `surface_caps2` is the
    // instance's half, decided before any device existed, so a headless
    // instance with no WSI at all reports this absent rather than requesting an
    // extension whose dependency it never enabled.
    //
    // `khr::swapchain::NAME` is in the list even though this backend always
    // enables it: the point is that the *dependency chain* was checked, not
    // that this call site happens to know one member is unconditional.
    let present_timing_extensions = surface_caps2
        && has_device_extension(&device_extensions, khr::swapchain::NAME)
        && has_device_extension(&device_extensions, present_timing::PRESENT_ID2_NAME)
        && has_device_extension(&device_extensions, khr::calibrated_timestamps::NAME)
        && has_device_extension(&device_extensions, present_timing::PRESENT_TIMING_NAME);
    // The mesh-shading and ray-tracing extensions, probed once: the answer
    // decides both which feature structs may be chained below and, with those
    // structs' bits, which capabilities are real.
    let stage_extensions = StageExtensions::probe(&device_extensions);

    let mut vulkan_1_1 = vk::PhysicalDeviceVulkan11Features::default();
    let mut vulkan_1_2 = vk::PhysicalDeviceVulkan12Features::default();
    let mut vulkan_1_3 = vk::PhysicalDeviceVulkan13Features::default();
    let mut present_id_features = vk::PhysicalDevicePresentIdFeaturesKHR::default();
    let mut present_wait_features = vk::PhysicalDevicePresentWaitFeaturesKHR::default();
    let mut present_timing_features =
        present_timing::PhysicalDevicePresentTimingFeaturesEXT::default();
    let mut present_id2_features = present_timing::PhysicalDevicePresentId2FeaturesKHR::default();
    let mut mesh_features = vk::PhysicalDeviceMeshShaderFeaturesEXT::default();
    let mut acceleration_features = vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default();
    let mut ray_query_features = vk::PhysicalDeviceRayQueryFeaturesKHR::default();
    let mut ray_pipeline_features = vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default();
    // Only chained on a device that has the version defining it. A
    // `VkPhysicalDeviceVulkan13Features` in the chain of a 1.2 device is not
    // merely useless — the spec forbids it, and a driver that honours the
    // prohibition leaves the struct untouched, which reads as "no dynamic
    // rendering" and happens to be the right answer for the wrong reason.
    let mut features2 = vk::PhysicalDeviceFeatures2::default()
        .push_next(&mut vulkan_1_1)
        .push_next(&mut vulkan_1_2);
    if properties.api_version >= vk::API_VERSION_1_3 {
        features2 = features2.push_next(&mut vulkan_1_3);
    }
    // The same rule as the 1.3 struct above, for the same reason: a feature
    // struct belonging to an extension the device does not support is left
    // untouched by a conforming driver, so chaining it unconditionally reads as
    // "no" for the wrong reason — and it is the extension list, not the zeroed
    // struct, that says which.
    if present_feedback_extensions {
        features2 = features2
            .push_next(&mut present_id_features)
            .push_next(&mut present_wait_features);
    }
    // Same rule again, and the structs are this crate's own transcriptions
    // rather than `ash`'s, because `ash` has no bindings for either extension.
    // See `crate::present_timing` for what that costs and what checks it.
    if present_timing_extensions {
        features2 = features2
            .push_next(&mut present_timing_features)
            .push_next(&mut present_id2_features);
    }
    // The same rule a fifth time, for the extensions this backend reports mesh
    // shading and ray tracing from. Nothing below reads a struct that was not
    // chained here — `with_stages` takes `stage_extensions` too — so a zeroed
    // one could not be mistaken for a driver's answer even if it were.
    if stage_extensions.mesh_shader {
        features2 = features2.push_next(&mut mesh_features);
    }
    if stage_extensions.acceleration_structure {
        features2 = features2.push_next(&mut acceleration_features);
    }
    if stage_extensions.ray_query {
        features2 = features2.push_next(&mut ray_query_features);
    }
    if stage_extensions.ray_tracing_pipeline {
        features2 = features2.push_next(&mut ray_pipeline_features);
    }
    // SAFETY: as above.
    unsafe { instance.get_physical_device_features2(physical, &mut features2) };
    // Copied out before any chained struct is read: the chain holds a mutable
    // borrow of each of them for as long as `features2` is used.
    let core_features = features2.features;
    let present_feedback = present_feedback_extensions
        && present_id_features.present_id == vk::TRUE
        && present_wait_features.present_wait == vk::TRUE;
    // Belt and braces, exactly as for present feedback and for the same
    // reason: a driver that lists an extension and then answers `VK_FALSE` for
    // its feature exists, and `Features::PRESENT_TIMING` promises the query
    // actually works — not that the driver knows the name.
    let present_timing = present_timing_extensions
        && present_timing_features.present_timing == vk::TRUE
        && present_id2_features.present_id2 == vk::TRUE;

    // SAFETY: as above.
    let family_properties =
        unsafe { instance.get_physical_device_queue_family_properties(physical) };
    let families = QueueFamilies::select(&family_properties);

    let extensions = ExtensionSupport {
        debug_utils,
        present_feedback,
        present_timing,
        ..ExtensionSupport::default()
    }
    .with_stages(
        stage_extensions,
        &mesh_features,
        &acceleration_features,
        &ray_query_features,
        &ray_pipeline_features,
    );
    let features = features_of(
        &core_features,
        &vulkan_1_2,
        &properties.limits,
        families,
        extensions,
    );
    let limits = limits_of(&properties.limits, &vulkan_1_2_props, features);

    let name = properties.device_name_as_c_str().map_or_else(
        |_| "unnamed adapter".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let driver_name = vulkan_1_2_props
        .driver_name_as_c_str()
        .ok()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("{:?}", vulkan_1_2_props.driver_id));
    let driver_info = vulkan_1_2_props
        .driver_info_as_c_str()
        .ok()
        .map(|info| info.to_string_lossy().into_owned())
        .filter(|info| !info.is_empty());
    let driver = match driver_info {
        Some(info) => format!("{driver_name} ({info})"),
        None => format!("{driver_name} {}", properties.driver_version),
    };

    let core_1_3 = Core13Support {
        dynamic_rendering: vulkan_1_3.dynamic_rendering == vk::TRUE,
        synchronization2: vulkan_1_3.synchronization2 == vk::TRUE,
        maintenance4: vulkan_1_3.maintenance4 == vk::TRUE,
    };
    if !core_1_3.is_complete() {
        log::warn!(
            "crcbl-vk: adapter {name} lacks {:?}; crcbl-vk cannot open it \
             (docs/plan/02-vulkan-backend.md: no fallback paths in the MVP)",
            core_1_3.missing()
        );
    }

    AdapterRecord {
        physical,
        info: AdapterInfo {
            // Rewritten by `enumerate` once the list is sorted.
            id: AdapterId(0),
            name,
            vendor_id: properties.vendor_id,
            device_id: properties.device_id,
            device_type: device_type(properties.device_type),
            driver,
            backend: BackendKind::Vulkan,
            caps: DeviceCaps { features, limits },
        },
        families,
        core_1_3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seam has one sample-count ceiling and Vulkan reports three masks, so
    /// the honest answer is their intersection: a caller told "8" for colour
    /// and handed a 4x-only depth format has been lied to.
    #[test]
    fn the_sample_count_ceiling_is_the_intersection_of_the_masks() {
        let mut limits = vk::PhysicalDeviceLimits {
            framebuffer_color_sample_counts: vk::SampleCountFlags::TYPE_1
                | vk::SampleCountFlags::TYPE_2
                | vk::SampleCountFlags::TYPE_4
                | vk::SampleCountFlags::TYPE_8,
            framebuffer_depth_sample_counts: vk::SampleCountFlags::TYPE_1
                | vk::SampleCountFlags::TYPE_2
                | vk::SampleCountFlags::TYPE_4,
            sampled_image_color_sample_counts: vk::SampleCountFlags::TYPE_1
                | vk::SampleCountFlags::TYPE_2
                | vk::SampleCountFlags::TYPE_4
                | vk::SampleCountFlags::TYPE_8,
            ..Default::default()
        };
        assert_eq!(common_sample_count(&limits), 4, "depth is the binding one");

        limits.framebuffer_depth_sample_counts = limits.framebuffer_color_sample_counts;
        assert_eq!(common_sample_count(&limits), 8);

        // A driver reporting nothing at all is confused, not authoritative;
        // every device supports one sample.
        limits.framebuffer_color_sample_counts = vk::SampleCountFlags::empty();
        assert_eq!(common_sample_count(&limits), 1);
    }

    use crcbl_hal::{BindingModel, GeometryPath, LightingPath};

    /// One `VkExtensionProperties` naming `name`, for the probes that decide
    /// which optional extensions a device has.
    fn named(name: &CStr) -> vk::ExtensionProperties {
        let mut properties = vk::ExtensionProperties::default();
        let bytes = name.to_bytes_with_nul();
        for (slot, byte) in properties.extension_name.iter_mut().zip(bytes) {
            #[allow(clippy::cast_possible_wrap)]
            {
                *slot = *byte as core::ffi::c_char;
            }
        }
        properties
    }

    fn family(flags: vk::QueueFlags, count: u32) -> vk::QueueFamilyProperties {
        vk::QueueFamilyProperties {
            queue_flags: flags,
            queue_count: count,
            timestamp_valid_bits: 64,
            min_image_transfer_granularity: vk::Extent3D {
                width: 1,
                height: 1,
                depth: 1,
            },
        }
    }

    const GRAPHICS: vk::QueueFlags = vk::QueueFlags::from_raw(
        vk::QueueFlags::GRAPHICS.as_raw()
            | vk::QueueFlags::COMPUTE.as_raw()
            | vk::QueueFlags::TRANSFER.as_raw(),
    );
    const COMPUTE_ONLY: vk::QueueFlags = vk::QueueFlags::from_raw(
        vk::QueueFlags::COMPUTE.as_raw() | vk::QueueFlags::TRANSFER.as_raw(),
    );
    const TRANSFER_ONLY: vk::QueueFlags = vk::QueueFlags::TRANSFER;

    /// AMD's layout: one universal family, one compute-only, one transfer-only.
    #[test]
    fn a_three_family_device_gets_all_three_queue_kinds() {
        let families = QueueFamilies::select(&[
            family(GRAPHICS, 1),
            family(COMPUTE_ONLY, 4),
            family(TRANSFER_ONLY, 1),
        ]);
        assert_eq!(families.graphics, Some(0));
        assert_eq!(families.async_compute, Some(1));
        assert_eq!(families.transfer, Some(2));
        assert_eq!(families.distinct(), vec![0, 1, 2]);
    }

    /// lavapipe's layout, and Intel's: one family that does everything. The MVP
    /// works fine — it submits everything to the graphics queue anyway — but the
    /// backend must not *claim* async compute it does not have, or a later
    /// slice keys off a lie.
    #[test]
    fn a_single_family_device_reports_no_async_compute_or_transfer_queue() {
        let families = QueueFamilies::select(&[family(GRAPHICS, 1)]);
        assert_eq!(families.graphics, Some(0));
        assert_eq!(families.async_compute, None, "one family is not async");
        assert_eq!(families.transfer, None);
        assert_eq!(families.distinct(), vec![0]);
    }

    /// A second queue on the *graphics* family is not async compute: it shares
    /// the same scheduler and buys none of what the flag promises.
    #[test]
    fn extra_queues_on_the_graphics_family_are_not_async_compute() {
        let families = QueueFamilies::select(&[family(GRAPHICS, 8)]);
        assert_eq!(families.async_compute, None);
    }

    #[test]
    fn families_with_no_queues_are_skipped() {
        let families = QueueFamilies::select(&[family(GRAPHICS, 0), family(GRAPHICS, 1)]);
        assert_eq!(families.graphics, Some(1));
    }

    fn fully_featured_vulkan_1_2<'a>() -> vk::PhysicalDeviceVulkan12Features<'a> {
        vk::PhysicalDeviceVulkan12Features::default()
            .descriptor_indexing(true)
            .runtime_descriptor_array(true)
            .descriptor_binding_partially_bound(true)
            .descriptor_binding_sampled_image_update_after_bind(true)
            .descriptor_binding_storage_buffer_update_after_bind(true)
            .descriptor_binding_variable_descriptor_count(true)
            .shader_sampled_image_array_non_uniform_indexing(true)
            .shader_storage_buffer_array_non_uniform_indexing(true)
            .buffer_device_address(true)
            .draw_indirect_count(true)
            .timeline_semaphore(true)
    }

    fn desktop_limits() -> vk::PhysicalDeviceLimits {
        vk::PhysicalDeviceLimits {
            max_push_constants_size: 128,
            timestamp_compute_and_graphics: vk::TRUE,
            timestamp_period: 1.0,
            max_draw_indirect_count: 1 << 20,
            max_sampler_anisotropy: 16.0,
            max_compute_work_group_count: [65535, 65535, 65535],
            ..Default::default()
        }
    }

    /// The synthetic fully featured device. This is the test that pins what
    /// [`Features::GPU_DRIVEN`] means in Vulkan terms, and it is deliberately
    /// expressed as a feature set rather than a driver name.
    #[test]
    fn a_full_featured_device_reports_the_whole_gpu_driven_bundle() {
        let features = features_of(
            &vk::PhysicalDeviceFeatures::default().multi_draw_indirect(true),
            &fully_featured_vulkan_1_2(),
            &desktop_limits(),
            QueueFamilies::select(&[family(GRAPHICS, 1)]),
            ExtensionSupport {
                debug_utils: true,
                ..ExtensionSupport::default()
            },
        );
        let caps = DeviceCaps {
            features,
            limits: Limits::desktop(),
        };
        assert_eq!(caps.geometry_path(), GeometryPath::IndirectCount);
        assert_eq!(caps.binding_model(), BindingModel::Bindless);
        assert!(caps.missing(Features::GPU_DRIVEN).is_empty());
        assert!(features.contains(Features::TIMESTAMP_QUERY));
        assert!(features.contains(Features::PUSH_CONSTANTS));
        assert!(features.contains(Features::DEBUG_MARKERS));
        // Present feedback is a device *extension* pair, so nothing in a
        // feature struct can imply it — and the GPU-driven bundle does not
        // need it, which is why a fully featured device that lacks it still
        // satisfies the bundle.
        assert!(!features.contains(Features::PRESENT_FEEDBACK));
        assert!(!Features::GPU_DRIVEN.contains(Features::PRESENT_FEEDBACK));
    }

    /// `PRESENT_FEEDBACK` comes from the extension probe and the feature query
    /// together, and it is the argument `describe` computes from both.
    ///
    /// Reported here rather than inferred from a feature struct: a driver that
    /// lists `VK_KHR_present_wait` and answers `presentWait = VK_FALSE` exists
    /// on paper, and the seam's flag promises the CPU can actually find out.
    #[test]
    fn present_feedback_is_reported_only_when_it_was_established() {
        let device = |present_feedback| {
            features_of(
                &vk::PhysicalDeviceFeatures::default(),
                &fully_featured_vulkan_1_2(),
                &desktop_limits(),
                QueueFamilies::select(&[family(GRAPHICS, 1)]),
                ExtensionSupport {
                    present_feedback,
                    ..ExtensionSupport::default()
                },
            )
        };
        assert!(device(true).contains(Features::PRESENT_FEEDBACK));
        assert!(!device(false).contains(Features::PRESENT_FEEDBACK));
        // Nothing else moves with it: a capability that dragged another flag
        // along would change the path a device selects on a driver that merely
        // lacks the extension pair.
        assert_eq!(
            device(true).difference(device(false)),
            Features::PRESENT_FEEDBACK
        );
    }

    /// `PRESENT_TIMING` is the same shape as `PRESENT_FEEDBACK` above: the
    /// argument `describe` computes from the four-extension chain **and** both
    /// feature bits, never from a feature struct.
    ///
    /// Its own test rather than a line added to that one, because the two
    /// capabilities are independent — a driver can have either without the
    /// other, and the flag that moves must be exactly the one asked for.
    #[test]
    fn present_timing_is_reported_only_when_it_was_established() {
        let device = |present_timing| {
            features_of(
                &vk::PhysicalDeviceFeatures::default(),
                &fully_featured_vulkan_1_2(),
                &desktop_limits(),
                QueueFamilies::select(&[family(GRAPHICS, 1)]),
                ExtensionSupport {
                    present_timing,
                    ..ExtensionSupport::default()
                },
            )
        };
        assert!(device(true).contains(Features::PRESENT_TIMING));
        assert!(!device(false).contains(Features::PRESENT_TIMING));
        assert_eq!(
            device(true).difference(device(false)),
            Features::PRESENT_TIMING
        );
        // Not part of the GPU-driven bundle, so a device without it selects
        // exactly the same paths — the same argument as for present feedback,
        // and the reason neither belongs in `GPU_DRIVEN`.
        assert!(!Features::GPU_DRIVEN.contains(Features::PRESENT_TIMING));
        let paths = |features| {
            let caps = DeviceCaps {
                features,
                limits: Limits::desktop(),
            };
            (
                caps.geometry_path(),
                caps.binding_model(),
                caps.lighting_path(),
            )
        };
        assert_eq!(paths(device(true)), paths(device(false)));
    }

    /// The two present capabilities are separate bits and separate probes, so
    /// neither can be granted by the other's answer. A backend that folded them
    /// into one flag would report timing on every driver with
    /// `VK_KHR_present_wait`, which is most of them and wrong on all of them.
    #[test]
    fn the_two_present_capabilities_are_independent() {
        let device = |present_feedback, present_timing| {
            features_of(
                &vk::PhysicalDeviceFeatures::default(),
                &fully_featured_vulkan_1_2(),
                &desktop_limits(),
                QueueFamilies::select(&[family(GRAPHICS, 1)]),
                ExtensionSupport {
                    present_feedback,
                    present_timing,
                    ..ExtensionSupport::default()
                },
            )
        };
        assert!(!device(true, false).contains(Features::PRESENT_TIMING));
        assert!(!device(false, true).contains(Features::PRESENT_FEEDBACK));
        assert!(device(true, true).contains(Features::PRESENT_FEEDBACK | Features::PRESENT_TIMING));
        assert!(
            device(false, false)
                .intersection(Features::PRESENT_FEEDBACK | Features::PRESENT_TIMING)
                .is_empty()
        );
    }

    /// The probe itself. `vkCreateDevice` fails outright on an extension the
    /// device does not have, so "is it in the list" is load-bearing rather than
    /// advisory — and a name that merely *starts* the same must not match.
    #[test]
    fn the_extension_probe_matches_whole_names_only() {
        let available = [named(khr::swapchain::NAME), named(khr::present_id::NAME)];
        assert!(has_device_extension(&available, khr::swapchain::NAME));
        assert!(has_device_extension(&available, khr::present_id::NAME));
        assert!(!has_device_extension(&available, khr::present_wait::NAME));
        assert!(!has_device_extension(&[], khr::present_id::NAME));

        // **`VK_KHR_present_id` is a proper prefix of `VK_KHR_present_id2`**,
        // and the same holds for the wait pair — a driver offering only the
        // `2` extensions is a driver this backend must not ask for the `1`
        // ones, because `vkCreateDevice` fails on a name that was never
        // listed. A probe that matched prefixes would report exactly that.
        let successors = [named(c"VK_KHR_present_id2"), named(c"VK_KHR_present_wait2")];
        assert!(!has_device_extension(&successors, khr::present_id::NAME));
        assert!(!has_device_extension(&successors, khr::present_wait::NAME));
        assert!(has_device_extension(&successors, c"VK_KHR_present_id2"));

        // **The premise above is now live in both directions.** This backend
        // probes for `VK_KHR_present_id` (present feedback) *and*
        // `VK_KHR_present_id2` (present timing's dependency), so a prefix match
        // would grant each capability on the other's driver. The predecessor
        // list must not satisfy the successor name any more than the reverse.
        let predecessors = [named(khr::present_id::NAME), named(khr::present_wait::NAME)];
        assert!(!has_device_extension(
            &predecessors,
            present_timing::PRESENT_ID2_NAME
        ));
        assert!(has_device_extension(
            &successors,
            present_timing::PRESENT_ID2_NAME
        ));
        // The extension this whole chain exists for, checked the same way.
        assert!(!has_device_extension(
            &successors,
            present_timing::PRESENT_TIMING_NAME
        ));
        assert!(has_device_extension(
            &[named(present_timing::PRESENT_TIMING_NAME)],
            present_timing::PRESENT_TIMING_NAME
        ));
    }

    /// The half of "honest reporting" that matters: a device missing one
    /// bindless sub-feature must report [`BindingModel::ArrayPages`], not
    /// bindless with a caveat. Dropping each sub-feature in turn is what stops
    /// someone "simplifying" the check to `descriptor_indexing == TRUE`.
    #[test]
    fn dropping_any_bindless_sub_feature_demotes_to_array_pages() {
        type Setter = fn(
            vk::PhysicalDeviceVulkan12Features<'static>,
        ) -> vk::PhysicalDeviceVulkan12Features<'static>;
        let disablers: &[(&str, Setter)] = &[
            ("runtime_descriptor_array", |f| {
                f.runtime_descriptor_array(false)
            }),
            ("descriptor_binding_partially_bound", |f| {
                f.descriptor_binding_partially_bound(false)
            }),
            ("sampled_image_update_after_bind", |f| {
                f.descriptor_binding_sampled_image_update_after_bind(false)
            }),
            ("storage_buffer_update_after_bind", |f| {
                f.descriptor_binding_storage_buffer_update_after_bind(false)
            }),
            ("variable_descriptor_count", |f| {
                f.descriptor_binding_variable_descriptor_count(false)
            }),
            ("sampled_image_non_uniform_indexing", |f| {
                f.shader_sampled_image_array_non_uniform_indexing(false)
            }),
            ("storage_buffer_non_uniform_indexing", |f| {
                f.shader_storage_buffer_array_non_uniform_indexing(false)
            }),
        ];
        for (name, disable) in disablers {
            let vulkan_1_2 = disable(fully_featured_vulkan_1_2());
            let features = features_of(
                &vk::PhysicalDeviceFeatures::default().multi_draw_indirect(true),
                &vulkan_1_2,
                &desktop_limits(),
                QueueFamilies::select(&[family(GRAPHICS, 1)]),
                ExtensionSupport {
                    debug_utils: true,
                    ..ExtensionSupport::default()
                },
            );
            assert!(
                !features.contains(Features::DESCRIPTOR_INDEXING),
                "{name} must demote DESCRIPTOR_INDEXING"
            );
            let caps = DeviceCaps {
                features,
                limits: Limits::minimum(),
            };
            assert_eq!(caps.binding_model(), BindingModel::ArrayPages, "{name}");
        }
    }

    /// The lavapipe-shaped case the P1 CI matrix exists to catch: a device
    /// without `drawIndirectCount` or `multiDrawIndirect` falls to the
    /// per-batch geometry path, and the gap is named rather than merely
    /// "unsupported".
    #[test]
    fn a_device_without_indirect_count_falls_back_and_says_which_feature() {
        let features = features_of(
            &vk::PhysicalDeviceFeatures::default(),
            &fully_featured_vulkan_1_2().draw_indirect_count(false),
            &desktop_limits(),
            QueueFamilies::select(&[family(GRAPHICS, 1)]),
            ExtensionSupport::default(),
        );
        let caps = DeviceCaps {
            features,
            limits: Limits::minimum(),
        };
        assert_eq!(caps.geometry_path(), GeometryPath::IndirectPerBatch);
        let missing = caps.missing(Features::GPU_DRIVEN);
        assert!(missing.contains(Features::DRAW_INDIRECT_COUNT));
        assert!(missing.contains(Features::MULTI_DRAW_INDIRECT));
        assert!(!missing.contains(Features::DESCRIPTOR_INDEXING));
        assert!(
            !features.contains(Features::DEBUG_MARKERS),
            "no debug-utils instance extension means no debug markers"
        );
    }

    /// A device with no timestamp support must report a zero period, because
    /// the profiler HUD multiplies by it and dividing a frame time by zero is a
    /// worse failure than a blank readout.
    #[test]
    fn absent_capabilities_zero_the_limits_that_depend_on_them() {
        let raw = vk::PhysicalDeviceLimits {
            timestamp_compute_and_graphics: vk::FALSE,
            timestamp_period: 42.0,
            max_push_constants_size: 0,
            max_draw_indirect_count: 1 << 20,
            max_sampler_anisotropy: 16.0,
            ..Default::default()
        };
        let features = features_of(
            &vk::PhysicalDeviceFeatures::default(),
            &vk::PhysicalDeviceVulkan12Features::default(),
            &raw,
            QueueFamilies::select(&[family(GRAPHICS, 1)]),
            ExtensionSupport::default(),
        );
        let limits = limits_of(
            &raw,
            &vk::PhysicalDeviceVulkan12Properties::default(),
            features,
        );
        assert_eq!(limits.timestamp_period_ns, 0.0);
        assert_eq!(limits.max_push_constant_size, 0);
        assert_eq!(limits.max_bindless_descriptors, 0);
        assert_eq!(limits.max_sampler_anisotropy, 1.0);
        assert_eq!(
            limits.max_draw_indirect_count, 1,
            "without DRAW_INDIRECT_COUNT one call emits one draw, whatever the \
             raw limit says"
        );
    }

    /// The workgroup-count limit is per dimension in the seam and per axis in
    /// Vulkan; taking the minimum is the only answer that cannot exceed a real
    /// ceiling.
    #[test]
    fn the_workgroup_count_limit_is_the_tightest_axis() {
        let raw = vk::PhysicalDeviceLimits {
            max_compute_work_group_count: [65535, 65535, 1024],
            ..desktop_limits()
        };
        let limits = limits_of(
            &raw,
            &vk::PhysicalDeviceVulkan12Properties::default(),
            Features::empty(),
        );
        assert_eq!(limits.max_compute_workgroups_per_dimension, 1024);
    }

    /// Every mesh-shading and ray-tracing extension listed at once, so each
    /// test below removes exactly one thing and watches exactly one answer go.
    fn all_stage_extensions() -> StageExtensions {
        StageExtensions {
            mesh_shader: true,
            acceleration_structure: true,
            ray_query: true,
            ray_tracing_pipeline: true,
        }
    }

    fn mesh_features() -> vk::PhysicalDeviceMeshShaderFeaturesEXT<'static> {
        vk::PhysicalDeviceMeshShaderFeaturesEXT::default()
            .mesh_shader(true)
            .task_shader(true)
    }

    fn acceleration_features() -> vk::PhysicalDeviceAccelerationStructureFeaturesKHR<'static> {
        vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default().acceleration_structure(true)
    }

    fn ray_query_features() -> vk::PhysicalDeviceRayQueryFeaturesKHR<'static> {
        vk::PhysicalDeviceRayQueryFeaturesKHR::default().ray_query(true)
    }

    fn ray_pipeline_features() -> vk::PhysicalDeviceRayTracingPipelineFeaturesKHR<'static> {
        vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default().ray_tracing_pipeline(true)
    }

    /// The full report for one synthetic device, everything else held at the
    /// desktop baseline so only the stage answers move.
    fn stage_report(
        listed: StageExtensions,
        mesh: &vk::PhysicalDeviceMeshShaderFeaturesEXT<'_>,
        acceleration: &vk::PhysicalDeviceAccelerationStructureFeaturesKHR<'_>,
        ray_query: &vk::PhysicalDeviceRayQueryFeaturesKHR<'_>,
        ray_tracing_pipeline: &vk::PhysicalDeviceRayTracingPipelineFeaturesKHR<'_>,
    ) -> Features {
        features_of(
            &vk::PhysicalDeviceFeatures::default().multi_draw_indirect(true),
            &fully_featured_vulkan_1_2(),
            &desktop_limits(),
            QueueFamilies::select(&[family(GRAPHICS, 1)]),
            ExtensionSupport::default().with_stages(
                listed,
                mesh,
                acceleration,
                ray_query,
                ray_tracing_pipeline,
            ),
        )
    }

    /// The whole set, which every test below subtracts from.
    const STAGE_FLAGS: Features = Features::MESH_SHADER
        .union(Features::TASK_SHADER)
        .union(Features::ACCELERATION_STRUCTURE)
        .union(Features::RAY_QUERY)
        .union(Features::RAY_TRACING_PIPELINE);

    /// A device with every extension listed and every feature bit set reports
    /// all five flags — the baseline the "one thing missing" tests need, and
    /// the only test here that would still pass if the mapping reported
    /// everything unconditionally.
    #[test]
    fn a_full_mesh_and_ray_device_reports_all_five_flags() {
        let features = stage_report(
            all_stage_extensions(),
            &mesh_features(),
            &acceleration_features(),
            &ray_query_features(),
            &ray_pipeline_features(),
        );
        assert_eq!(features.intersection(STAGE_FLAGS), STAGE_FLAGS);
    }

    /// The extension half. Every feature bit stays `VK_TRUE` and the name
    /// leaves the device's list — which is the case that matters most, because
    /// `vkCreateDevice` fails outright on an extension that was never listed,
    /// so a flag reported from the bits alone is a device this backend then
    /// cannot open.
    #[test]
    fn every_mesh_and_ray_flag_needs_its_extension_listed() {
        let all = stage_report(
            all_stage_extensions(),
            &mesh_features(),
            &acceleration_features(),
            &ray_query_features(),
            &ray_pipeline_features(),
        );
        type Withdraw = fn(StageExtensions) -> StageExtensions;
        let cases: &[(&str, Features, Withdraw)] = &[
            (
                "VK_EXT_mesh_shader",
                Features::MESH_SHADER.union(Features::TASK_SHADER),
                |mut listed| {
                    listed.mesh_shader = false;
                    listed
                },
            ),
            // Withdrawing the acceleration structure takes both ray
            // capabilities with it: this is `VK_KHR_deferred_host_operations`
            // missing as much as it is `VK_KHR_acceleration_structure`, and
            // either way there is nothing left to traverse.
            (
                "VK_KHR_acceleration_structure",
                Features::ACCELERATION_STRUCTURE
                    .union(Features::RAY_QUERY)
                    .union(Features::RAY_TRACING_PIPELINE),
                |mut listed| {
                    listed.acceleration_structure = false;
                    listed
                },
            ),
            ("VK_KHR_ray_query", Features::RAY_QUERY, |mut listed| {
                listed.ray_query = false;
                listed
            }),
            (
                "VK_KHR_ray_tracing_pipeline",
                Features::RAY_TRACING_PIPELINE,
                |mut listed| {
                    listed.ray_tracing_pipeline = false;
                    listed
                },
            ),
        ];
        for (name, lost, withdraw) in cases {
            let features = stage_report(
                withdraw(all_stage_extensions()),
                &mesh_features(),
                &acceleration_features(),
                &ray_query_features(),
                &ray_pipeline_features(),
            );
            // A difference, not a `contains`: the flags that go must be exactly
            // these, so a capability that dragged an unrelated one down would
            // fail here too.
            assert_eq!(
                all.difference(features),
                *lost,
                "{name} absent must clear exactly {lost:?}"
            );
        }
    }

    /// The feature-bit half. Every extension stays listed and the bit comes
    /// back `VK_FALSE` — the "the driver knows the name" case the present
    /// probes already guard against, now for five more capabilities.
    #[test]
    fn every_mesh_and_ray_flag_needs_its_feature_bit() {
        let all = stage_report(
            all_stage_extensions(),
            &mesh_features(),
            &acceleration_features(),
            &ray_query_features(),
            &ray_pipeline_features(),
        );
        // `meshShader` clear takes the task stage with it: an amplification
        // stage with no mesh stage to launch is not something a caller can use.
        let mesh_off = stage_report(
            all_stage_extensions(),
            &vk::PhysicalDeviceMeshShaderFeaturesEXT::default().task_shader(true),
            &acceleration_features(),
            &ray_query_features(),
            &ray_pipeline_features(),
        );
        assert_eq!(
            all.difference(mesh_off),
            Features::MESH_SHADER | Features::TASK_SHADER
        );

        // …and `taskShader` clear on its own takes only itself, which is why
        // the seam has two flags rather than one.
        let task_off = stage_report(
            all_stage_extensions(),
            &vk::PhysicalDeviceMeshShaderFeaturesEXT::default().mesh_shader(true),
            &acceleration_features(),
            &ray_query_features(),
            &ray_pipeline_features(),
        );
        assert_eq!(all.difference(task_off), Features::TASK_SHADER);

        let acceleration_off = stage_report(
            all_stage_extensions(),
            &mesh_features(),
            &vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default(),
            &ray_query_features(),
            &ray_pipeline_features(),
        );
        assert_eq!(
            all.difference(acceleration_off),
            Features::ACCELERATION_STRUCTURE | Features::RAY_QUERY | Features::RAY_TRACING_PIPELINE,
            "neither ray capability survives the structure it traverses"
        );

        let ray_query_off = stage_report(
            all_stage_extensions(),
            &mesh_features(),
            &acceleration_features(),
            &vk::PhysicalDeviceRayQueryFeaturesKHR::default(),
            &ray_pipeline_features(),
        );
        assert_eq!(all.difference(ray_query_off), Features::RAY_QUERY);

        let ray_pipeline_off = stage_report(
            all_stage_extensions(),
            &mesh_features(),
            &acceleration_features(),
            &ray_query_features(),
            &vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default(),
        );
        assert_eq!(
            all.difference(ray_pipeline_off),
            Features::RAY_TRACING_PIPELINE
        );
    }

    /// What the flags are *for*. Reporting them is only worth anything if the
    /// selectors move, and until this slice landed neither could leave its
    /// floor on any device.
    #[test]
    fn mesh_shading_and_ray_query_select_the_paths_they_gate() {
        let caps = |features| DeviceCaps {
            features,
            limits: Limits::desktop(),
        };

        // Mesh shading alone. `fully_featured_vulkan_1_2` has
        // `drawIndirectCount`, so this is also the assertion that the mesh path
        // outranks the indirect-count one rather than merely being available.
        let mesh_only = stage_report(
            StageExtensions {
                mesh_shader: true,
                ..StageExtensions::default()
            },
            &mesh_features(),
            &acceleration_features(),
            &ray_query_features(),
            &ray_pipeline_features(),
        );
        assert_eq!(caps(mesh_only).geometry_path(), GeometryPath::MeshShader);
        assert_eq!(caps(mesh_only).lighting_path(), LightingPath::Rasterised);

        // Ray query with the structure under it, and no mesh shading: the two
        // selectors are independent axes and must not drag each other.
        let ray_only = stage_report(
            StageExtensions {
                mesh_shader: false,
                acceleration_structure: true,
                ray_query: true,
                ray_tracing_pipeline: false,
            },
            &mesh_features(),
            &acceleration_features(),
            &ray_query_features(),
            &ray_pipeline_features(),
        );
        assert_eq!(caps(ray_only).lighting_path(), LightingPath::RayTraced);
        assert_eq!(caps(ray_only).geometry_path(), GeometryPath::IndirectCount);

        // And the floor, which is what every device reported before this slice:
        // nothing listed, nothing selected above the fallback.
        let neither = stage_report(
            StageExtensions::default(),
            &mesh_features(),
            &acceleration_features(),
            &ray_query_features(),
            &ray_pipeline_features(),
        );
        assert!(neither.intersection(STAGE_FLAGS).is_empty());
        assert_eq!(caps(neither).geometry_path(), GeometryPath::IndirectCount);
        assert_eq!(caps(neither).lighting_path(), LightingPath::Rasterised);
    }

    /// `VK_KHR_acceleration_structure` declares
    /// `VK_KHR_deferred_host_operations` as a dependency, so the probe answers
    /// for the pair rather than for the name anyone would think to look for.
    /// Asking `vkCreateDevice` for a dependent extension without its dependency
    /// is invalid usage a driver need not honour.
    #[test]
    fn the_acceleration_structure_probe_wants_the_whole_chain() {
        let both = [
            named(khr::acceleration_structure::NAME),
            named(khr::deferred_host_operations::NAME),
        ];
        assert!(StageExtensions::probe(&both).acceleration_structure);
        assert!(
            !StageExtensions::probe(&both[..1]).acceleration_structure,
            "the dependency is not optional"
        );
        assert!(!StageExtensions::probe(&both[1..]).acceleration_structure);

        // The other three are each one name, and each is probed for its own —
        // a probe reading a neighbour's answer would report ray tracing on
        // every device with mesh shaders.
        let listed = StageExtensions::probe(&[
            named(ext::mesh_shader::NAME),
            named(khr::ray_query::NAME),
            named(khr::ray_tracing_pipeline::NAME),
        ]);
        assert_eq!(
            listed,
            StageExtensions {
                mesh_shader: true,
                acceleration_structure: false,
                ray_query: true,
                ray_tracing_pipeline: true,
            }
        );
        assert_eq!(StageExtensions::probe(&[]), StageExtensions::default());
    }

    #[test]
    fn device_types_map_and_software_rasterisers_are_recognised() {
        assert_eq!(
            device_type(vk::PhysicalDeviceType::DISCRETE_GPU),
            DeviceType::Discrete
        );
        assert_eq!(device_type(vk::PhysicalDeviceType::CPU), DeviceType::Cpu);
        assert_eq!(
            device_type(vk::PhysicalDeviceType::OTHER),
            DeviceType::Other
        );
    }

    #[test]
    fn the_vulkan_1_3_floor_names_exactly_what_is_missing() {
        let complete = Core13Support {
            dynamic_rendering: true,
            synchronization2: true,
            maintenance4: true,
        };
        assert!(complete.is_complete());
        assert!(complete.missing().is_empty());

        let partial = Core13Support {
            dynamic_rendering: true,
            synchronization2: false,
            maintenance4: false,
        };
        assert!(!partial.is_complete());
        assert_eq!(partial.missing(), vec!["synchronization2", "maintenance4"]);
    }
}
