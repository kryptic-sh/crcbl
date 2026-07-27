//! Memory-type selection, and the deliberately naive allocator behind it.
//!
//! # Why there is no suballocator yet
//!
//! `docs/plan/02-vulkan-backend.md` §2.1 calls for `gpu-allocator` wrapped
//! behind the seam's three [`MemoryLocation`]s. That is the right answer and it
//! is **not** this slice: P1.1 is "device bring-up through a cleared window",
//! and the only allocations a clear needs are the handful of images an
//! offscreen swapchain rings through plus the one staging buffer a readback
//! reads. A dedicated `vkAllocateMemory` per resource is a dozen lines, has no
//! fragmentation behaviour to get wrong, and is trivially replaced.
//!
//! What it must not do is become load-bearing. Real geometry and texture pools
//! arrive with the render graph at P1.3, and `maxMemoryAllocationCount` is
//! guaranteed to be only 4096 — so a per-resource allocation stops working at
//! exactly the point the engine starts allocating per mesh. The suballocator
//! lands there.
//!
//! # Selection is pure, so it is tested
//!
//! [`find_memory_type`] is the part that goes subtly wrong: picking the first
//! heap that satisfies the required flags instead of the *best* one gives a
//! host-visible upload buffer in uncached memory on one driver and works fine on
//! another. The preference list makes that a stated policy with a test, rather
//! than an accident of heap order.

use ash::vk;

use crcbl_hal::MemoryLocation;

/// What a [`MemoryLocation`] asks of a Vulkan memory type.
///
/// `required` must be present; `preferred` is tried first and dropped if no
/// type has it. The pair is what makes "device-local host-visible if the driver
/// offers it (ReBAR), plain host-visible otherwise" expressible without a
/// per-driver branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemoryRequest {
    pub(crate) required: vk::MemoryPropertyFlags,
    pub(crate) preferred: vk::MemoryPropertyFlags,
}

impl MemoryRequest {
    /// The request a seam memory location makes.
    #[must_use]
    pub(crate) fn for_location(location: MemoryLocation) -> Self {
        use vk::MemoryPropertyFlags as P;
        match location {
            MemoryLocation::DeviceLocal => Self {
                required: P::DEVICE_LOCAL,
                preferred: P::DEVICE_LOCAL,
            },
            // Write-combined by nature: the seam's docs say "never read it
            // back", so `HOST_CACHED` is not asked for and `DEVICE_LOCAL` is
            // preferred so a ReBAR system writes straight into VRAM.
            MemoryLocation::HostUpload => Self {
                required: P::HOST_VISIBLE | P::HOST_COHERENT,
                preferred: P::HOST_VISIBLE | P::HOST_COHERENT | P::DEVICE_LOCAL,
            },
            // Read by the CPU, so cached matters more than anything; coherent
            // as well, to avoid an invalidate call on every poll.
            MemoryLocation::HostReadback => Self {
                required: P::HOST_VISIBLE | P::HOST_COHERENT,
                preferred: P::HOST_VISIBLE | P::HOST_COHERENT | P::HOST_CACHED,
            },
        }
    }
}

/// Picks a memory type index for `type_bits` (a `VkMemoryRequirements`
/// `memoryTypeBits` mask) satisfying `request`.
///
/// Types carrying the preferred flags win; otherwise the first type with the
/// required flags does. `None` means the device cannot satisfy the request at
/// all, which is a clean `OutOfDeviceMemory` rather than a wrong heap.
#[must_use]
pub(crate) fn find_memory_type(
    properties: &vk::PhysicalDeviceMemoryProperties,
    type_bits: u32,
    request: MemoryRequest,
) -> Option<u32> {
    let count = properties.memory_type_count as usize;
    let candidates = |flags: vk::MemoryPropertyFlags| {
        (0..count).find(|index| {
            #[allow(clippy::cast_possible_truncation)]
            let selected = type_bits & (1u32 << (*index as u32)) != 0;
            selected
                && properties.memory_types[*index]
                    .property_flags
                    .contains(flags)
        })
    };
    #[allow(clippy::cast_possible_truncation)]
    candidates(request.preferred)
        .or_else(|| candidates(request.required))
        .map(|index| index as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_type(flags: vk::MemoryPropertyFlags, heap: u32) -> vk::MemoryType {
        vk::MemoryType {
            property_flags: flags,
            heap_index: heap,
        }
    }

    /// A radv-shaped layout: plain VRAM, a small ReBAR window, plain system
    /// memory, and a cached readback type.
    fn discrete_properties() -> vk::PhysicalDeviceMemoryProperties {
        use vk::MemoryPropertyFlags as P;
        let mut properties = vk::PhysicalDeviceMemoryProperties {
            memory_type_count: 4,
            ..Default::default()
        };
        properties.memory_types[0] = memory_type(P::DEVICE_LOCAL, 0);
        properties.memory_types[1] =
            memory_type(P::DEVICE_LOCAL | P::HOST_VISIBLE | P::HOST_COHERENT, 0);
        properties.memory_types[2] = memory_type(P::HOST_VISIBLE | P::HOST_COHERENT, 1);
        properties.memory_types[3] =
            memory_type(P::HOST_VISIBLE | P::HOST_COHERENT | P::HOST_CACHED, 1);
        properties
    }

    const ALL_TYPES: u32 = u32::MAX;

    #[test]
    fn device_local_picks_vram() {
        let index = find_memory_type(
            &discrete_properties(),
            ALL_TYPES,
            MemoryRequest::for_location(MemoryLocation::DeviceLocal),
        );
        assert_eq!(index, Some(0));
    }

    /// The preference that matters: an upload buffer wants the ReBAR window if
    /// it exists, so the GPU reads it without a copy.
    #[test]
    fn an_upload_prefers_device_local_host_visible() {
        let index = find_memory_type(
            &discrete_properties(),
            ALL_TYPES,
            MemoryRequest::for_location(MemoryLocation::HostUpload),
        );
        assert_eq!(index, Some(1));
    }

    /// And a readback wants the *cached* type, because the CPU reads it — the
    /// exact opposite preference, which is why one shared "host visible" rule
    /// would be wrong.
    #[test]
    fn a_readback_prefers_cached_memory() {
        let index = find_memory_type(
            &discrete_properties(),
            ALL_TYPES,
            MemoryRequest::for_location(MemoryLocation::HostReadback),
        );
        assert_eq!(index, Some(3));
    }

    /// lavapipe and every UMA part: one host-visible coherent type for
    /// everything. Preferences must degrade to the required set rather than
    /// failing.
    #[test]
    fn a_unified_memory_device_satisfies_every_location_from_one_type() {
        use vk::MemoryPropertyFlags as P;
        let mut properties = vk::PhysicalDeviceMemoryProperties {
            memory_type_count: 1,
            ..Default::default()
        };
        properties.memory_types[0] =
            memory_type(P::DEVICE_LOCAL | P::HOST_VISIBLE | P::HOST_COHERENT, 0);
        for location in [
            MemoryLocation::DeviceLocal,
            MemoryLocation::HostUpload,
            MemoryLocation::HostReadback,
        ] {
            assert_eq!(
                find_memory_type(
                    &properties,
                    ALL_TYPES,
                    MemoryRequest::for_location(location)
                ),
                Some(0),
                "{location:?}"
            );
        }
    }

    /// `memoryTypeBits` is a hard filter: a type the resource cannot use must
    /// never be selected, however well it matches the flags.
    #[test]
    fn the_resources_type_mask_is_respected() {
        let properties = discrete_properties();
        // Only types 2 and 3 are legal for this resource.
        let mask = 0b1100;
        let index = find_memory_type(
            &properties,
            mask,
            MemoryRequest::for_location(MemoryLocation::HostUpload),
        );
        assert_eq!(index, Some(2), "the ReBAR type is not in the mask");

        // Nothing legal at all is a clean `None`, not a wrong heap.
        let index = find_memory_type(
            &properties,
            0b0100,
            MemoryRequest::for_location(MemoryLocation::DeviceLocal),
        );
        assert_eq!(index, None);
    }

    #[test]
    fn a_device_with_no_matching_type_reports_none() {
        use vk::MemoryPropertyFlags as P;
        let mut properties = vk::PhysicalDeviceMemoryProperties {
            memory_type_count: 1,
            ..Default::default()
        };
        properties.memory_types[0] = memory_type(P::DEVICE_LOCAL, 0);
        assert_eq!(
            find_memory_type(
                &properties,
                ALL_TYPES,
                MemoryRequest::for_location(MemoryLocation::HostReadback)
            ),
            None
        );
    }
}
