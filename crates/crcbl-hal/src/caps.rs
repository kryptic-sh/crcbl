//! Adapter identification and device capability reporting.
//!
//! This module is what makes topic 03's **renderer tiers** expressible. The
//! renderer never asks "am I on Vulkan?" — it asks [`DeviceCaps::tier`] and
//! [`Features::contains`], exactly as `crcbl-shell` exposes `ShellCaps` instead
//! of letting anyone sniff for Wayland.
//!
//! ```text
//! Tier A (vk / mtl / dx12) : descriptor indexing + BDA + draw_indirect_count
//! Tier B (WebGPU via wgpu) : fixed-size arrays, per-batch indirect, SSBO indices
//! ```
//!
//! Per topic 03, **Tier B is a constraint on data layout, not a separate
//! renderer**: both tiers consume the same geometry pools, instance buffers and
//! material tables, and only the draw-emission tail differs. The flags here are
//! the seam that lets one renderer make that choice at runtime.

use core::fmt;

/// Which backend implementation is behind the seam.
///
/// For logs, capture-tool selection and bug reports only. Renderer *behaviour*
/// must key off [`Features`] and [`RendererTier`], never off this — that is the
/// difference between a capability system and platform sniffing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// `crcbl-vk` — Vulkan 1.3.
    Vulkan,
    /// `crcbl-wgpu` — wgpu, native or WebGPU.
    Wgpu,
    /// `crcbl-mtl` — Metal (P14).
    Metal,
    /// `crcbl-dx12` — Direct3D 12 (P14).
    Dx12,
    /// The in-crate recording no-op backend. See [`crate::null`].
    Null,
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Vulkan => "vulkan",
            Self::Wgpu => "wgpu",
            Self::Metal => "metal",
            Self::Dx12 => "dx12",
            Self::Null => "null",
        };
        f.write_str(name)
    }
}

/// Broad class of physical device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeviceType {
    /// Software rasteriser — lavapipe, WARP, swiftshader. Correct, slow; the
    /// P1 golden-image CI target.
    Cpu,
    /// A GPU integrated with the CPU package.
    Integrated,
    /// A separate GPU card.
    Discrete,
    /// A virtualised or para-virtualised GPU.
    Virtual,
    /// The backend declined to say.
    Other,
}

/// Index of an adapter in [`Instance::adapters`](crate::Instance::adapters).
///
/// A newtype rather than a bare `u32` so it cannot be confused with a queue
/// index or an image index at a call site — those are all `u32` too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AdapterId(pub u32);

/// What an adapter says about itself, before a device exists.
///
/// Cloneable and owned: adapter enumeration happens once at startup, so the two
/// `String`s are not worth avoiding, and the alternative (borrowing from the
/// instance) would infect every consumer with a lifetime.
#[derive(Clone, Debug, PartialEq)]
pub struct AdapterInfo {
    /// Position in the enumeration; pass to [`DeviceDesc`](crate::DeviceDesc).
    pub id: AdapterId,
    /// Human-readable device name, e.g. `"AMD Radeon RX 7900 XTX (RADV)"`.
    pub name: String,
    /// PCI vendor id, or `0` if unknown.
    pub vendor_id: u32,
    /// PCI device id, or `0` if unknown.
    pub device_id: u32,
    /// Device class.
    pub device_type: DeviceType,
    /// Driver name and version, as the backend reports them.
    pub driver: String,
    /// Which backend enumerated it.
    pub backend: BackendKind,
    /// Everything this adapter can do. Available *before* device creation so
    /// selection logic can reject an adapter without paying for a device.
    pub caps: DeviceCaps,
}

bitflags::bitflags! {
    /// Optional device capabilities.
    ///
    /// `crcbl-vk` requires the Tier A set as hard requirements and errors out on
    /// a device that lacks them (`docs/plan/02-vulkan-backend.md`: "no fallback
    /// paths for missing features in MVP"). The flags still exist as flags
    /// because `crcbl-wgpu` genuinely varies — WebGPU has no bindless, no
    /// multi-draw-indirect, no buffer device address and no push constants, and
    /// timestamp queries are browser-dependent.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Features: u64 {
        /// Runtime-sized descriptor arrays with partial binding and
        /// update-after-bind — the bindless model topic 03's Tier A is built
        /// on. Vulkan `descriptorIndexing`, DX12 SM6.6 dynamic resources,
        /// Metal argument buffers tier 2. Absent on WebGPU.
        const DESCRIPTOR_INDEXING = 1 << 0;
        /// Shaders can dereference raw GPU pointers. Vulkan
        /// `bufferDeviceAddress`, DX12 GPU virtual addresses, Metal 3
        /// `gpuAddress`. Absent on WebGPU — Tier B uses indexed SSBO lookups.
        const BUFFER_DEVICE_ADDRESS = 1 << 1;
        /// [`draw_indirect_count`](crate::CommandEncoder::draw_indirect_count)
        /// and its indexed sibling are usable: the draw count itself comes from
        /// a GPU buffer, so the CPU never learns how many draws happen.
        /// Absent on WebGPU.
        const DRAW_INDIRECT_COUNT = 1 << 2;
        /// One indirect call may emit more than one draw (`draw_count > 1` in
        /// [`DrawIndirect`](crate::DrawIndirect)). Distinct from
        /// `DRAW_INDIRECT_COUNT`: WebGPU has neither, but a backend could
        /// plausibly have this without the count buffer.
        const MULTI_DRAW_INDIRECT = 1 << 3;
        /// `first_instance` in an indirect draw argument is honoured rather
        /// than required to be zero. WebGPU requires zero.
        const INDIRECT_FIRST_INSTANCE = 1 << 4;
        /// Timestamp queries are supported. Optional and browser-dependent on
        /// WebGPU, which is why the profiler HUD must degrade rather than
        /// break (topic 10 risk list).
        const TIMESTAMP_QUERY = 1 << 5;
        /// Pipeline statistics queries (primitives generated, invocations).
        const PIPELINE_STATISTICS_QUERY = 1 << 6;
        /// Occlusion queries.
        const OCCLUSION_QUERY = 1 << 7;
        /// Compute shaders. Effectively universal — WebGPU has them, which is
        /// why GPU culling stays GPU-side even in Tier B — but a GL-ES2-class
        /// wgpu fallback would not, and the renderer needs somewhere to say so.
        const COMPUTE = 1 << 8;
        /// Timeline semaphores: monotonic counters usable for both GPU-GPU and
        /// CPU-GPU waits. Vulkan `timelineSemaphore`, DX12 `ID3D12Fence`,
        /// Metal `MTLSharedEvent`. WebGPU has no semaphores at all; see
        /// [`crate::sync`].
        const TIMELINE_SEMAPHORE = 1 << 9;
        /// A queue family that supports compute independently of graphics, so
        /// async compute is possible. Async compute is post-MVP, but the seam
        /// models queues plural from the start.
        const ASYNC_COMPUTE_QUEUE = 1 << 10;
        /// A transfer-only queue family exists. MVP uploads share the
        /// graphics+compute queue; this flag is what a later dedicated
        /// transfer queue keys off.
        const TRANSFER_QUEUE = 1 << 11;
        /// Push constants / root constants are available.
        /// [`Limits::max_push_constant_size`] gives the budget. **Absent on
        /// WebGPU** — Tier B substitutes a dynamic-offset uniform buffer, which
        /// is a data-layout consequence the renderer must plan for.
        const PUSH_CONSTANTS = 1 << 12;
        /// Depth clamping (as opposed to clipping) — shadow-map rendering
        /// wants it so caster geometry behind the near plane still writes
        /// depth.
        const DEPTH_CLAMP = 1 << 13;
        /// A non-zero [`DepthBias::clamp`](crate::DepthBias::clamp) is honoured.
        const DEPTH_BIAS_CLAMP = 1 << 14;
        /// Fill-mode `Line` in [`PolygonMode`](crate::PolygonMode) — wireframe
        /// debug views. Absent on WebGPU and Metal-on-some-hardware.
        const POLYGON_MODE_LINE = 1 << 15;
        /// BC/DXT compressed texture formats are sampleable.
        const TEXTURE_COMPRESSION_BC = 1 << 16;
        /// Anisotropic sampling; [`Limits::max_sampler_anisotropy`] gives the
        /// cap.
        const SAMPLER_ANISOTROPY = 1 << 17;
        /// Debug labels and markers reach the capture tool
        /// (`VK_EXT_debug_utils`, PIX events, Metal debug groups). Without it
        /// the encoder's label calls are accepted and dropped.
        const DEBUG_MARKERS = 1 << 18;
        /// GPU-side shader printf / validation messages are routed into `log`.
        const SHADER_DEBUG_PRINTF = 1 << 19;
        /// The device reports back when a present has actually completed, so a
        /// frame loop can pace on the display instead of on a clock —
        /// [`Device::wait_until_presented`](crate::Device::wait_until_presented).
        ///
        /// Named for the capability rather than for any one API's spelling of
        /// it, because the three that have it do not agree on the shape: one
        /// numbers each present and blocks on the number, one hands out a
        /// waitable object with no number at all, and one only calls back once
        /// a drawable has been shown. What they share is that the CPU can find
        /// out, which is the whole of what this flag promises. Optional, and
        /// deliberately **not** part of [`TIER_A`](Self::TIER_A): a device
        /// without it renders exactly the same frames, it just cannot be paced
        /// by them.
        const PRESENT_FEEDBACK = 1 << 20;

        /// The capability set that defines **Tier A** (topic 03).
        ///
        /// A device holding all of these can run the full GPU-driven path: one
        /// bindless descriptor array for every texture, GPU pointers for
        /// per-draw data, and a single indirect-count call per pass.
        const TIER_A = Self::DESCRIPTOR_INDEXING.bits()
            | Self::BUFFER_DEVICE_ADDRESS.bits()
            | Self::DRAW_INDIRECT_COUNT.bits()
            | Self::MULTI_DRAW_INDIRECT.bits()
            | Self::COMPUTE.bits()
            | Self::TIMELINE_SEMAPHORE.bits();
    }
}

/// Numeric device limits.
///
/// Every field is a hard ceiling the backend guarantees; exceeding one is a
/// [`HalError`](crate::HalError), not undefined behaviour. Fields are named
/// after the thing they bound, not after any one API's spelling of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Largest width/height of a 2D image.
    pub max_image_2d: u32,
    /// Largest edge of a 3D image.
    pub max_image_3d: u32,
    /// Largest layer count of an array image.
    pub max_image_array_layers: u32,
    /// Largest bindable range of a storage buffer, in bytes.
    pub max_storage_buffer_range: u64,
    /// Largest bindable range of a uniform buffer, in bytes.
    pub max_uniform_buffer_range: u64,
    /// Bind groups (descriptor sets) bindable simultaneously.
    pub max_bind_groups: u32,
    /// Descriptors in one bindless array. `0` when
    /// [`Features::DESCRIPTOR_INDEXING`] is absent.
    pub max_bindless_descriptors: u32,
    /// Push-constant bytes. `0` when [`Features::PUSH_CONSTANTS`] is absent.
    pub max_push_constant_size: u32,
    /// Colour attachments in one render pass.
    pub max_color_attachments: u32,
    /// Highest sample count usable for a multisampled image or pipeline.
    ///
    /// Always a power of two in `1..=64`: a sample count is a *mask* in every
    /// API underneath, so `3` is not "three samples", it is `TYPE_1 | TYPE_2`.
    /// Backends reject a request that is not a power of two, or that exceeds
    /// this, with [`HalError::InvalidDescriptor`](crate::HalError::InvalidDescriptor)
    /// — which is the error [`Device::create_image`](crate::Device::create_image)
    /// documents and which had nowhere to be expressed before this field.
    pub max_sample_count: u32,
    /// Draws one [`draw_indirect_count`](crate::CommandEncoder::draw_indirect_count)
    /// may emit.
    pub max_draw_indirect_count: u32,
    /// Per-dimension workgroup size limit for compute dispatches.
    pub max_compute_workgroup_size: [u32; 3],
    /// Total invocations in one compute workgroup.
    pub max_compute_invocations_per_workgroup: u32,
    /// Per-dimension workgroup *count* limit for
    /// [`dispatch`](crate::CommandEncoder::dispatch).
    pub max_compute_workgroups_per_dimension: u32,
    /// Required alignment for a uniform buffer binding offset.
    pub min_uniform_buffer_offset_alignment: u64,
    /// Required alignment for a storage buffer binding offset.
    pub min_storage_buffer_offset_alignment: u64,
    /// Preferred alignment for buffer↔image copy offsets.
    pub optimal_buffer_copy_offset_alignment: u64,
    /// Maximum anisotropy in [`SamplerDesc`](crate::SamplerDesc).
    pub max_sampler_anisotropy: f32,
    /// Nanoseconds per timestamp-query tick. Multiply a timestamp delta by this
    /// to get nanoseconds. `0.0` when [`Features::TIMESTAMP_QUERY`] is absent.
    pub timestamp_period_ns: f32,
}

impl Limits {
    /// The floor every backend must clear.
    ///
    /// Chosen to sit at or below WebGPU's guaranteed `downlevel` limits, so a
    /// renderer written against these numbers runs on the weakest tier the
    /// engine targets. It is a *contract*, not a description of any real
    /// device — the null backend's Tier B preset reports exactly this.
    #[must_use]
    pub const fn minimum() -> Self {
        Self {
            max_image_2d: 8192,
            max_image_3d: 2048,
            max_image_array_layers: 256,
            max_storage_buffer_range: 128 << 20,
            max_uniform_buffer_range: 64 << 10,
            max_bind_groups: 4,
            max_bindless_descriptors: 0,
            max_push_constant_size: 0,
            max_color_attachments: 4,
            // WebGPU's downlevel floor: 1x and 4x, and nothing above.
            max_sample_count: 4,
            max_draw_indirect_count: 1,
            max_compute_workgroup_size: [256, 256, 64],
            max_compute_invocations_per_workgroup: 256,
            max_compute_workgroups_per_dimension: 65535,
            min_uniform_buffer_offset_alignment: 256,
            min_storage_buffer_offset_alignment: 256,
            optimal_buffer_copy_offset_alignment: 256,
            max_sampler_anisotropy: 1.0,
            timestamp_period_ns: 0.0,
        }
    }

    /// A representative desktop Tier A device.
    ///
    /// Used by the null backend's Tier A preset and by tests that need
    /// plausible headroom. Not a promise about any specific GPU.
    #[must_use]
    pub const fn desktop() -> Self {
        Self {
            max_image_2d: 16384,
            max_image_3d: 2048,
            max_image_array_layers: 2048,
            max_storage_buffer_range: u32::MAX as u64,
            max_uniform_buffer_range: 64 << 10,
            max_bind_groups: 8,
            max_bindless_descriptors: 500_000,
            max_push_constant_size: 128,
            max_color_attachments: 8,
            max_sample_count: 8,
            max_draw_indirect_count: 1 << 20,
            max_compute_workgroup_size: [1024, 1024, 64],
            max_compute_invocations_per_workgroup: 1024,
            max_compute_workgroups_per_dimension: 65535,
            min_uniform_buffer_offset_alignment: 64,
            min_storage_buffer_offset_alignment: 16,
            optimal_buffer_copy_offset_alignment: 4,
            max_sampler_anisotropy: 16.0,
            timestamp_period_ns: 1.0,
        }
    }
}

/// Which of topic 03's two renderer tiers a device can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RendererTier {
    /// Full GPU-driven path: bindless arrays, buffer device address,
    /// multi-draw-indirect with a GPU-side count.
    A,
    /// Portable path: fixed-size texture arrays, per-batch indirect draws,
    /// indexed SSBO lookups instead of pointers. Culling still runs in compute.
    B,
}

impl RendererTier {
    /// Whether this is [`RendererTier::A`].
    #[must_use]
    pub const fn is_a(self) -> bool {
        matches!(self, Self::A)
    }

    /// Whether this is [`RendererTier::B`].
    #[must_use]
    pub const fn is_b(self) -> bool {
        matches!(self, Self::B)
    }
}

/// Everything the renderer needs to know about a device's abilities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceCaps {
    /// Optional capabilities present.
    pub features: Features,
    /// Numeric ceilings.
    pub limits: Limits,
}

impl DeviceCaps {
    /// The tier this device can run, derived from [`Features::TIER_A`].
    ///
    /// Deriving rather than storing means a backend cannot claim Tier A while
    /// missing a Tier A feature — the one lie that would silently produce a
    /// renderer taking the bindless path on a device without bindless.
    #[must_use]
    pub fn tier(&self) -> RendererTier {
        if self.features.contains(Features::TIER_A) {
            RendererTier::A
        } else {
            RendererTier::B
        }
    }

    /// Which of `required` this device is missing — empty if it satisfies them.
    ///
    /// The shape `Instance::create_device`
    /// uses to build [`HalError::UnsupportedFeatures`](crate::HalError::UnsupportedFeatures),
    /// so the error names the gap instead of saying "unsupported".
    #[must_use]
    pub fn missing(&self, required: Features) -> Features {
        required.difference(self.features)
    }

    /// Whether this device satisfies `required`.
    #[must_use]
    pub fn supports(&self, required: Features) -> bool {
        self.features.contains(required)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HalError;

    #[test]
    fn tier_is_derived_from_features_not_asserted() {
        let a = DeviceCaps {
            features: Features::TIER_A,
            limits: Limits::desktop(),
        };
        assert_eq!(a.tier(), RendererTier::A);
        assert!(a.tier().is_a() && !a.tier().is_b());

        // Drop exactly one Tier A feature and the tier must fall back. This is
        // the check that stops "claims Tier A, lacks bindless" from existing.
        for feature in Features::TIER_A.iter() {
            let caps = DeviceCaps {
                features: Features::TIER_A.difference(feature),
                limits: Limits::desktop(),
            };
            assert_eq!(
                caps.tier(),
                RendererTier::B,
                "missing {feature:?} must demote to tier B"
            );
        }
    }

    #[test]
    fn tier_a_requires_exactly_the_documented_capabilities() {
        // Guards the doc table in `docs/plan/03-gpu-driven-rendering.md`. Named
        // exhaustively, and checked for equality rather than containment: a
        // feature added to `TIER_A` without a doc update is the thing this
        // catches, and a `contains` list of a subset never would.
        let documented = Features::DESCRIPTOR_INDEXING
            | Features::BUFFER_DEVICE_ADDRESS
            | Features::DRAW_INDIRECT_COUNT
            | Features::MULTI_DRAW_INDIRECT
            | Features::COMPUTE
            | Features::TIMELINE_SEMAPHORE;
        assert_eq!(Features::TIER_A, documented);
        // Push constants are NOT part of Tier A: WebGPU lacks them, but so do
        // some Tier A-capable configurations, and the renderer must have a
        // uniform-buffer path regardless.
        assert!(!Features::TIER_A.contains(Features::PUSH_CONSTANTS));
    }

    #[test]
    fn missing_names_the_gap() {
        let caps = DeviceCaps {
            features: Features::COMPUTE | Features::TIMELINE_SEMAPHORE,
            limits: Limits::minimum(),
        };
        let missing = caps.missing(Features::TIER_A);
        assert!(missing.contains(Features::DESCRIPTOR_INDEXING));
        assert!(missing.contains(Features::BUFFER_DEVICE_ADDRESS));
        assert!(!missing.contains(Features::COMPUTE));
        assert!(!caps.supports(Features::TIER_A));
        assert!(caps.supports(Features::COMPUTE));
        assert!(caps.missing(Features::COMPUTE).is_empty());
    }

    /// Strictly below, not merely "not above". The regression worth guarding is
    /// the desktop preset being pasted into `minimum()`, and a table of `<=`
    /// passes on two byte-identical presets while the name says otherwise.
    #[test]
    fn minimum_limits_are_below_desktop_limits() {
        let min = Limits::minimum();
        let desktop = Limits::desktop();
        assert!(min.max_image_2d < desktop.max_image_2d);
        assert!(min.max_image_array_layers < desktop.max_image_array_layers);
        assert!(min.max_storage_buffer_range < desktop.max_storage_buffer_range);
        assert!(min.max_bind_groups < desktop.max_bind_groups);
        assert!(min.max_bindless_descriptors < desktop.max_bindless_descriptors);
        assert!(min.max_push_constant_size < desktop.max_push_constant_size);
        assert!(min.max_color_attachments < desktop.max_color_attachments);
        assert!(min.max_sample_count < desktop.max_sample_count);
        assert!(min.max_draw_indirect_count < desktop.max_draw_indirect_count);
        assert!(min.max_compute_workgroup_size[0] < desktop.max_compute_workgroup_size[0]);
        assert!(min.max_compute_workgroup_size[1] < desktop.max_compute_workgroup_size[1]);
        assert!(
            min.max_compute_invocations_per_workgroup
                < desktop.max_compute_invocations_per_workgroup
        );
        assert!(min.max_sampler_anisotropy < desktop.max_sampler_anisotropy);
        assert!(min.timestamp_period_ns < desktop.timestamp_period_ns);

        // The fields the two presets legitimately agree on. Listed rather than
        // omitted, so "the floor already matches a desktop device here" is a
        // stated fact instead of a gap in the table above.
        assert!(min.max_image_3d <= desktop.max_image_3d);
        assert!(min.max_uniform_buffer_range <= desktop.max_uniform_buffer_range);
        assert!(min.max_compute_workgroup_size[2] <= desktop.max_compute_workgroup_size[2]);
        assert!(
            min.max_compute_workgroups_per_dimension
                <= desktop.max_compute_workgroups_per_dimension
        );

        // A sample count is a bit in a mask underneath, so a preset reporting a
        // non-power-of-two ceiling would make every backend's check nonsense.
        for limits in [min, desktop] {
            assert!(limits.max_sample_count.is_power_of_two());
            assert!(limits.max_sample_count <= 64);
        }
        // Alignment limits run the other way: the floor is the *coarser* one,
        // and strictly so for the same reason.
        assert!(
            min.min_uniform_buffer_offset_alignment > desktop.min_uniform_buffer_offset_alignment
        );
        assert!(
            min.min_storage_buffer_offset_alignment > desktop.min_storage_buffer_offset_alignment
        );
        assert!(
            min.optimal_buffer_copy_offset_alignment > desktop.optimal_buffer_copy_offset_alignment
        );
    }

    /// Every flag is a hand-written `1 << N`, and nothing checked the shifts
    /// were distinct. A repeated shift makes [`Features::contains`] vacuously
    /// true for the shadowed flag — it is never absent, so no test that asks
    /// "is X missing" can ever see it.
    ///
    /// The count comes from the flag table rather than a literal, so the check
    /// grows with the enum instead of needing an edit that would not happen.
    #[test]
    fn every_feature_owns_a_distinct_bit() {
        use bitflags::Flags;

        // `TIER_A` is a named union of other flags, not a bit of its own.
        let single_bit: Vec<&'static str> = Features::FLAGS
            .iter()
            .filter(|flag| flag.value().bits().is_power_of_two())
            .map(bitflags::Flag::name)
            .collect();
        assert_eq!(
            Features::all().bits().count_ones() as usize,
            single_bit.len(),
            "two of {single_bit:?} share a bit"
        );
    }

    /// Every optional feature must be individually observable: `missing` names
    /// exactly it, and a device without it refuses it.
    ///
    /// Driven off the flag table, because the `TIER_A` loop above is a good
    /// self-growing pattern that structurally cannot reach the optional half —
    /// nine flags had no mention in any test at all, including the two the
    /// queue selection keys off.
    #[test]
    fn every_feature_is_reported_one_at_a_time() {
        use bitflags::Flags;

        for flag in Features::FLAGS {
            let feature = *flag.value();
            if !feature.bits().is_power_of_two() {
                continue;
            }
            let name = flag.name();
            let without = DeviceCaps {
                features: Features::all().difference(feature),
                limits: Limits::desktop(),
            };
            assert_eq!(without.missing(feature), feature, "{name}");
            assert!(!without.supports(feature), "{name}");

            let only = DeviceCaps {
                features: feature,
                limits: Limits::minimum(),
            };
            assert!(only.supports(feature), "{name}");
            assert!(only.missing(feature).is_empty(), "{name}");
            assert!(
                HalError::UnsupportedFeatures {
                    missing: without.missing(feature)
                }
                .to_string()
                .contains(name),
                "the error a backend owes must name {name}"
            );
        }
    }

    #[test]
    fn minimum_limits_report_no_bindless_or_push_constants() {
        let min = Limits::minimum();
        assert_eq!(min.max_bindless_descriptors, 0);
        assert_eq!(min.max_push_constant_size, 0);
        assert_eq!(
            min.max_draw_indirect_count, 1,
            "Tier B emits one draw per call"
        );
    }

    #[test]
    fn backend_kind_displays_lowercase() {
        assert_eq!(BackendKind::Vulkan.to_string(), "vulkan");
        assert_eq!(BackendKind::Null.to_string(), "null");
    }
}
