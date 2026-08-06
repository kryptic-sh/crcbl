//! Hand-written FFI for `VK_EXT_present_timing`, and the `VK_KHR_present_id2`
//! it depends on.
//!
//! # Why this module exists at all
//!
//! `ash` has no bindings for either extension — the pinned `ash 0.38.0+1.3.281`
//! contains no `present_timing` symbol, and it is the newest published release.
//! `VK_EXT_present_timing` is nonetheless ratified (`supported="vulkan"
//! ratified="vulkan"` in `vk.xml`) and shipping: RADV/Mesa on the development
//! box reports revision 3, which is the revision
//! `/usr/include/vulkan/vulkan_core.h` declares. So the choice is between
//! hand-written FFI and not answering
//! [`Device::display_timing`](crcbl_hal::Device::display_timing) honestly on the
//! one platform that can answer it.
//!
//! It is written the way this workspace already writes the Wayland and X11
//! protocol layers: transcribed from the installed headers, in one named place,
//! with the layout checked at compile time rather than trusted.
//!
//! # What is transcribed and what is not
//!
//! Deliberately the smallest slice that answers the question:
//!
//! * [`Device::swapchain_timing`] — the one entry point,
//!   `vkGetSwapchainTimingPropertiesEXT`, resolved through
//!   `vkGetDeviceProcAddr`.
//! * [`SwapchainTimingPropertiesEXT`] — what it writes.
//! * [`PhysicalDevicePresentTimingFeaturesEXT`] and
//!   [`PhysicalDevicePresentId2FeaturesKHR`] — the two feature structs that have
//!   to be queried before the extensions are asked for and enabled when they
//!   are.
//!
//! Nothing else in either extension is here. In particular
//! `vkSetSwapchainPresentTimingQueueSizeEXT` and `VkPresentTimingsInfoEXT` are
//! absent, and that is what keeps swapchain creation untouched by this feature:
//! `VK_SWAPCHAIN_CREATE_PRESENT_TIMING_BIT_EXT` is required by *those* two and
//! by nothing else. `validusage.json` lists five valid-usage statements for
//! `vkGetSwapchainTimingPropertiesEXT`, and all five are handle validity —
//! `device`, `swapchain`, the two pointers, and the parent relationship between
//! the first two.
//!
//! # Chaining through `ash`
//!
//! The two feature structs implement [`vk::ExtendsDeviceCreateInfo`] and
//! [`vk::ExtendsPhysicalDeviceFeatures2`], which are public `unsafe` traits
//! rather than sealed ones. That is what lets `push_next` splice them into a
//! chain `ash` built, instead of this module hand-rolling a second, parallel
//! `p_next` walk beside `ash`'s — which is the version that goes wrong quietly.
//! The obligation each `unsafe impl` takes on is the one the compile-time block
//! below discharges: `#[repr(C)]`, `s_type` first, `p_next` second, so
//! `ash`'s chain walker reads the right two words.

use core::ffi::{CStr, c_void};
use core::mem::offset_of;

use ash::vk;
use crcbl_hal::{DisplayTiming, display_timing_from_refresh_nanos};

/// `VK_EXT_PRESENT_TIMING_EXTENSION_NAME`, `vulkan_core.h` line 18457.
pub(crate) const PRESENT_TIMING_NAME: &CStr = c"VK_EXT_present_timing";
/// `VK_KHR_PRESENT_ID_2_EXTENSION_NAME`, `vulkan_core.h` line 13168.
///
/// A dependency of [`PRESENT_TIMING_NAME`] rather than something this backend
/// wants for itself: `vk.xml` declares `VK_EXT_present_timing` as
/// `depends="VK_KHR_swapchain+VK_KHR_present_id2+VK_KHR_get_surface_capabilities2+VK_KHR_calibrated_timestamps"`,
/// and only this one of the four is missing from `ash`. None of its commands
/// are needed — the query below uses none — so only its name and its feature
/// struct are transcribed.
pub(crate) const PRESENT_ID2_NAME: &CStr = c"VK_KHR_present_id2";

/// `VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PRESENT_TIMING_FEATURES_EXT`,
/// `vulkan_core.h` line 774.
const PRESENT_TIMING_FEATURES: vk::StructureType = vk::StructureType::from_raw(1_000_208_000);
/// `VK_STRUCTURE_TYPE_SWAPCHAIN_TIMING_PROPERTIES_EXT`, `vulkan_core.h` line 775.
const SWAPCHAIN_TIMING_PROPERTIES: vk::StructureType = vk::StructureType::from_raw(1_000_208_001);
/// `VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PRESENT_ID_2_FEATURES_KHR`,
/// `vulkan_core.h` line 1171.
const PRESENT_ID2_FEATURES: vk::StructureType = vk::StructureType::from_raw(1_000_479_002);

/// `VkPhysicalDevicePresentTimingFeaturesEXT`, `vulkan_core.h` line 18481.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct PhysicalDevicePresentTimingFeaturesEXT {
    s_type: vk::StructureType,
    p_next: *mut c_void,
    /// The bit this backend gates [`Features::PRESENT_TIMING`](crcbl_hal::Features::PRESENT_TIMING)
    /// on, and sets when it enables the extension.
    pub(crate) present_timing: vk::Bool32,
    /// Presenting at a chosen absolute time. Read but unused: a later slice
    /// that schedules presents needs it, and leaving the field out would make
    /// the structure the wrong size.
    pub(crate) present_at_absolute_time: vk::Bool32,
    /// Presenting at a chosen relative time. Unused, as above.
    pub(crate) present_at_relative_time: vk::Bool32,
}

/// `VkPhysicalDevicePresentId2FeaturesKHR`, `vulkan_core.h` line 13182.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct PhysicalDevicePresentId2FeaturesKHR {
    s_type: vk::StructureType,
    p_next: *mut c_void,
    /// The dependency's own feature bit, which must also be granted — enabling
    /// an extension whose dependency is merely *named* is invalid usage.
    pub(crate) present_id2: vk::Bool32,
}

/// `VkSwapchainTimingPropertiesEXT`, `vulkan_core.h` line 18506.
///
/// `p_next` is private and always null: unlike the two feature structs, this
/// one's valid usage says `pNext` **must** be `NULL`
/// (`VUID-VkSwapchainTimingPropertiesEXT-pNext-pNext`), so there is nothing for
/// a caller to chain onto it and no reason to offer the field.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct SwapchainTimingPropertiesEXT {
    s_type: vk::StructureType,
    p_next: *mut c_void,
    /// Nanoseconds. See [`display_timing_from_refresh_nanos`] for what the pair
    /// means; this module does not re-derive it.
    refresh_duration: u64,
    /// Nanoseconds, or [`u64::MAX`] for VRR, or zero for "cannot say".
    refresh_interval: u64,
}

/// `PFN_vkGetSwapchainTimingPropertiesEXT`, `vulkan_core.h` line 18572.
type PfnGetSwapchainTimingProperties = unsafe extern "system" fn(
    device: vk::Device,
    swapchain: vk::SwapchainKHR,
    p_swapchain_timing_properties: *mut SwapchainTimingPropertiesEXT,
    p_swapchain_timing_properties_counter: *mut u64,
) -> vk::Result;

/// The C layout, checked against `/usr/include/vulkan/vulkan_core.h` rather
/// than against whatever this file happens to compile to.
///
/// **This is the only check that can catch a transposed field.** A struct with
/// `refresh_duration` and `refresh_interval` the wrong way round compiles,
/// passes review, links, runs, and reports a 60 Hz panel as free-running VRR —
/// nothing else in this change would notice. The numbers come from compiling
/// `sizeof`/`offsetof` against the installed header, not from reading the
/// struct definitions and doing the arithmetic by hand.
///
/// The pointer-width assertion is first and is deliberately unconditional
/// rather than a `#[cfg(target_pointer_width = "64")]` around the rest: the
/// offsets below are the LP64/LLP64 layout, nobody has verified this ABI on a
/// 32-bit target, and a `cfg` that skipped the checks there would report "not
/// verified" as "passed". Every target this crate builds for is 64-bit — CI
/// excludes `crcbl-vk` from the `wasm32-unknown-unknown` job, which is the only
/// non-64-bit target in the matrix — so a 32-bit build failing loudly here is
/// the honest outcome and not a lost capability.
const _: () = {
    assert!(
        size_of::<*mut c_void>() == 8,
        "the transcribed offsets below are the 64-bit Vulkan ABI; nothing here \
         has been verified against a 32-bit one"
    );

    assert!(size_of::<PhysicalDevicePresentTimingFeaturesEXT>() == 32);
    assert!(align_of::<PhysicalDevicePresentTimingFeaturesEXT>() == 8);
    assert!(offset_of!(PhysicalDevicePresentTimingFeaturesEXT, s_type) == 0);
    assert!(offset_of!(PhysicalDevicePresentTimingFeaturesEXT, p_next) == 8);
    assert!(offset_of!(PhysicalDevicePresentTimingFeaturesEXT, present_timing) == 16);
    assert!(
        offset_of!(
            PhysicalDevicePresentTimingFeaturesEXT,
            present_at_absolute_time
        ) == 20
    );
    assert!(
        offset_of!(
            PhysicalDevicePresentTimingFeaturesEXT,
            present_at_relative_time
        ) == 24
    );

    assert!(size_of::<PhysicalDevicePresentId2FeaturesKHR>() == 24);
    assert!(align_of::<PhysicalDevicePresentId2FeaturesKHR>() == 8);
    assert!(offset_of!(PhysicalDevicePresentId2FeaturesKHR, s_type) == 0);
    assert!(offset_of!(PhysicalDevicePresentId2FeaturesKHR, p_next) == 8);
    assert!(offset_of!(PhysicalDevicePresentId2FeaturesKHR, present_id2) == 16);

    assert!(size_of::<SwapchainTimingPropertiesEXT>() == 32);
    assert!(align_of::<SwapchainTimingPropertiesEXT>() == 8);
    assert!(offset_of!(SwapchainTimingPropertiesEXT, s_type) == 0);
    assert!(offset_of!(SwapchainTimingPropertiesEXT, p_next) == 8);
    // The pair this whole module exists to read, and the pair a transposition
    // would silently swap. `refreshDuration` is first in the header.
    assert!(offset_of!(SwapchainTimingPropertiesEXT, refresh_duration) == 16);
    assert!(offset_of!(SwapchainTimingPropertiesEXT, refresh_interval) == 24);
};

impl Default for PhysicalDevicePresentTimingFeaturesEXT {
    fn default() -> Self {
        Self {
            s_type: PRESENT_TIMING_FEATURES,
            p_next: core::ptr::null_mut(),
            present_timing: vk::FALSE,
            present_at_absolute_time: vk::FALSE,
            present_at_relative_time: vk::FALSE,
        }
    }
}

impl PhysicalDevicePresentTimingFeaturesEXT {
    /// The struct as it is handed to `vkCreateDevice`: asking for
    /// `presentTiming` and for neither scheduling feature, because nothing in
    /// this slice schedules a present.
    pub(crate) fn enabling_timing() -> Self {
        Self {
            present_timing: vk::TRUE,
            ..Self::default()
        }
    }
}

impl Default for PhysicalDevicePresentId2FeaturesKHR {
    fn default() -> Self {
        Self {
            s_type: PRESENT_ID2_FEATURES,
            p_next: core::ptr::null_mut(),
            present_id2: vk::FALSE,
        }
    }
}

impl PhysicalDevicePresentId2FeaturesKHR {
    /// As handed to `vkCreateDevice`, with the dependency's feature asked for.
    pub(crate) fn enabling_present_id2() -> Self {
        Self {
            present_id2: vk::TRUE,
            ..Self::default()
        }
    }
}

impl Default for SwapchainTimingPropertiesEXT {
    fn default() -> Self {
        Self {
            s_type: SWAPCHAIN_TIMING_PROPERTIES,
            p_next: core::ptr::null_mut(),
            refresh_duration: 0,
            refresh_interval: 0,
        }
    }
}

// SAFETY: `#[repr(C)]`, with `s_type` at offset 0 and `p_next` at offset 8, so
// the structure begins with the `VkBaseOutStructure` prefix that `ash`'s chain
// walker reads and writes through. The compile-time block above is what makes
// that a checked fact rather than a claim. `s_type` is set by every
// constructor, and the extension is only ever chained onto a device whose
// extension list names it — which is the other half of what makes the chain
// legal, and is enforced at the two call sites in `adapter` and `device`.
unsafe impl vk::ExtendsDeviceCreateInfo for PhysicalDevicePresentTimingFeaturesEXT {}
// SAFETY: as above.
unsafe impl vk::ExtendsPhysicalDeviceFeatures2 for PhysicalDevicePresentTimingFeaturesEXT {}
// SAFETY: as above.
unsafe impl vk::ExtendsDeviceCreateInfo for PhysicalDevicePresentId2FeaturesKHR {}
// SAFETY: as above.
unsafe impl vk::ExtendsPhysicalDeviceFeatures2 for PhysicalDevicePresentId2FeaturesKHR {}

/// `vkGetSwapchainTimingPropertiesEXT`, resolved for one device.
///
/// Shaped like `ash`'s own per-device extension tables (`khr::present_wait::Device`
/// and friends): it holds the device handle beside the function pointer, so a
/// call site cannot pass a handle from a different device by accident.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Device {
    handle: vk::Device,
    get_swapchain_timing_properties: PfnGetSwapchainTimingProperties,
}

impl Device {
    /// Resolves the entry point, or `None` if the loader does not have it.
    ///
    /// `None` is not an error: `vkGetDeviceProcAddr` returning null for a
    /// command whose extension was enabled would be a driver bug, but the
    /// answer this backend gives on it is the same as the answer for a device
    /// without the extension — [`DisplayTiming::Unknown`] — so there is nothing
    /// to fail. Resolving through the *device* rather than the instance is what
    /// skips the loader's dispatch thunk, which is why `ash` does the same.
    pub(crate) fn load(instance: &ash::Instance, device: &ash::Device) -> Option<Self> {
        let handle = device.handle();
        // SAFETY: `handle` belongs to `instance` and is live — `device` is
        // borrowed for this call — and the name is a `'static` NUL-terminated
        // literal. `vkGetDeviceProcAddr` only reads the driver's dispatch
        // table.
        let address = unsafe {
            instance.get_device_proc_addr(handle, c"vkGetSwapchainTimingPropertiesEXT".as_ptr())
        }?;
        // SAFETY: the loader returned this pointer for the name above, so it is
        // that command, and `PfnGetSwapchainTimingProperties` is that command's
        // prototype transcribed from `vulkan_core.h` line 18572 —
        // `extern "system"`, matching `VKAPI_PTR`. `vk::PFN_vkVoidFunction` is
        // a function pointer already, so this is a signature cast and not a
        // data-pointer one; both types are non-null function pointers of the
        // same size.
        let get_swapchain_timing_properties: PfnGetSwapchainTimingProperties =
            unsafe { core::mem::transmute(address) };
        Some(Self {
            handle,
            get_swapchain_timing_properties,
        })
    }

    /// Asks the presentation engine what it is doing with `swapchain`.
    ///
    /// **The caller must hold `swapchain`'s external synchronisation** for the
    /// duration: `vk.xml` marks this command's `swapchain` parameter
    /// `externsync="true"`. `crate::device` discharges that with the device
    /// state lock, the same one `vkWaitForPresentKHR` is called under. The
    /// requirement is stated here rather than only there because a second call
    /// site would otherwise have to rediscover it.
    ///
    /// `VK_NOT_READY` becomes [`DisplayTiming::Unknown`] rather than an error,
    /// and the extension proposal is explicit that this is ordinary traffic:
    /// *"`vkGetSwapchainTimingPropertiesEXT` can return `VK_NOT_READY`, because
    /// some platforms may not provide timing properties until after at least
    /// one image has been presented to the swapchain."* It says the same of a
    /// swapchain whose properties have just changed. A newly created swapchain
    /// answering "not yet" is the normal first frame, not a failure.
    ///
    /// # Errors
    ///
    /// The raw `VkResult` for anything else, for the caller to map: `vk.xml`
    /// lists `VK_ERROR_OUT_OF_HOST_MEMORY`, `VK_ERROR_OUT_OF_DEVICE_MEMORY`,
    /// `VK_ERROR_SURFACE_LOST_KHR`, `VK_ERROR_UNKNOWN` and
    /// `VK_ERROR_VALIDATION_FAILED`.
    pub(crate) fn swapchain_timing(
        &self,
        swapchain: vk::SwapchainKHR,
    ) -> Result<DisplayTiming, vk::Result> {
        let mut properties = SwapchainTimingPropertiesEXT::default();
        // SAFETY: `self.handle` is the device this table was resolved for and
        // outlives it — `DeviceInner` owns both and drops the table with the
        // device. `swapchain` is a live `VkSwapchainKHR` of that same device,
        // which is the whole of this command's valid usage beyond the pointers.
        // `properties` is a stack local of the right layout with `s_type` set
        // and `p_next` null, as its valid usage requires, and it outlives the
        // call. The counter pointer is optional and null is the documented way
        // to decline it.
        let result = unsafe {
            (self.get_swapchain_timing_properties)(
                self.handle,
                swapchain,
                &mut properties,
                core::ptr::null_mut(),
            )
        };
        match result {
            vk::Result::SUCCESS => Ok(display_timing_from_refresh_nanos(
                properties.refresh_duration,
                properties.refresh_interval,
            )),
            // Documented traffic, not a failure: nothing has been presented to
            // this swapchain yet, so the platform has nothing to report. The
            // next frame asks again — `display_timing` is a live query.
            vk::Result::NOT_READY => Ok(DisplayTiming::Unknown),
            error => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `sType` values, against `vulkan_core.h`. Transcribed constants, so
    /// the thing worth pinning is that they are the numbers the header gives
    /// and not each other's.
    #[test]
    fn the_structure_types_are_the_headers_numbers() {
        assert_eq!(PRESENT_TIMING_FEATURES.as_raw(), 1_000_208_000);
        assert_eq!(SWAPCHAIN_TIMING_PROPERTIES.as_raw(), 1_000_208_001);
        assert_eq!(PRESENT_ID2_FEATURES.as_raw(), 1_000_479_002);
    }

    /// Every constructor stamps its own `sType`, because a zeroed one is
    /// `VK_STRUCTURE_TYPE_APPLICATION_INFO` and a driver reading the chain
    /// would follow it into the wrong branch.
    #[test]
    fn every_constructor_stamps_its_structure_type() {
        assert_eq!(
            PhysicalDevicePresentTimingFeaturesEXT::default().s_type,
            PRESENT_TIMING_FEATURES
        );
        assert_eq!(
            PhysicalDevicePresentTimingFeaturesEXT::enabling_timing().s_type,
            PRESENT_TIMING_FEATURES
        );
        assert_eq!(
            PhysicalDevicePresentId2FeaturesKHR::default().s_type,
            PRESENT_ID2_FEATURES
        );
        assert_eq!(
            PhysicalDevicePresentId2FeaturesKHR::enabling_present_id2().s_type,
            PRESENT_ID2_FEATURES
        );
        assert_eq!(
            SwapchainTimingPropertiesEXT::default().s_type,
            SWAPCHAIN_TIMING_PROPERTIES
        );
    }

    /// `p_next` starts null everywhere. For the properties struct that is its
    /// valid usage; for the two feature structs it is what lets `push_next`
    /// splice them into a chain without inheriting a stale tail.
    #[test]
    fn every_constructor_starts_with_a_null_chain() {
        assert!(
            PhysicalDevicePresentTimingFeaturesEXT::default()
                .p_next
                .is_null()
        );
        assert!(
            PhysicalDevicePresentTimingFeaturesEXT::enabling_timing()
                .p_next
                .is_null()
        );
        assert!(
            PhysicalDevicePresentId2FeaturesKHR::default()
                .p_next
                .is_null()
        );
        assert!(SwapchainTimingPropertiesEXT::default().p_next.is_null());
    }

    /// The two structs handed to `vkCreateDevice` ask for exactly the features
    /// this slice uses: the extension's own bit and its dependency's, and
    /// neither scheduling feature — asking for a feature nothing exercises is
    /// how a device fails to open on a driver that has the extension and not
    /// the extra.
    #[test]
    fn the_create_device_structs_ask_for_the_two_bits_and_no_others() {
        let timing = PhysicalDevicePresentTimingFeaturesEXT::enabling_timing();
        assert_eq!(timing.present_timing, vk::TRUE);
        assert_eq!(timing.present_at_absolute_time, vk::FALSE);
        assert_eq!(timing.present_at_relative_time, vk::FALSE);
        assert_eq!(
            PhysicalDevicePresentId2FeaturesKHR::enabling_present_id2().present_id2,
            vk::TRUE
        );
    }

    /// The names, spelled once here and compared against the literals the
    /// header defines. A typo in either fails `vkCreateDevice` outright, which
    /// is loud — but it fails it on every machine, including ones where the
    /// extension is present, and this says which string was wrong.
    #[test]
    fn the_extension_names_match_the_header() {
        assert_eq!(PRESENT_TIMING_NAME.to_bytes(), b"VK_EXT_present_timing");
        assert_eq!(PRESENT_ID2_NAME.to_bytes(), b"VK_KHR_present_id2");
    }

    /// The queried struct is all zeroes apart from its `sType`, so a driver
    /// that returns `VK_NOT_READY` without writing anything cannot leave a
    /// stale reading behind for the mapping to read as real.
    #[test]
    fn the_queried_struct_starts_reporting_nothing() {
        let properties = SwapchainTimingPropertiesEXT::default();
        assert_eq!(properties.refresh_duration, 0);
        assert_eq!(properties.refresh_interval, 0);
        assert_eq!(
            display_timing_from_refresh_nanos(
                properties.refresh_duration,
                properties.refresh_interval
            ),
            DisplayTiming::Unknown
        );
    }
}
