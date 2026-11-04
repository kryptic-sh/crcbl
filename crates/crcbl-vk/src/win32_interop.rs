//! Importing images and fences that a D3D11 device shared through NT handles.
//!
//! The Vulkan half of presenting to a transparent Win32 window: a
//! `VkSwapchainKHR` cannot be a DirectComposition visual's content, so the
//! frame is rendered into textures `crcbl_dcomp::Interop` creates, and a
//! composition swapchain presents a copy. This module turns that crate's
//! handles into Vulkan objects; `crate::device` wires them into a swapchain.
//!
//! Windows only, with the DirectComposition it feeds: only Windows drivers
//! list these extensions, and nothing off Windows has a use for them.

use ash::khr;
use ash::vk;
use crcbl_hal::HalError;

use crate::adapter::has_device_extension;
use crate::conv;
use crate::device::DeviceInner;
use crate::mem::{self, MemoryRequest};

/// The device extensions that import a D3D11 texture and a D3D fence.
pub(crate) const EXTENSIONS: [&std::ffi::CStr; 2] = [
    khr::external_memory_win32::NAME,
    khr::external_semaphore_win32::NAME,
];

/// Whether a device lists every extension in [`EXTENSIONS`].
pub(crate) fn listed(device_extensions: &[vk::ExtensionProperties]) -> bool {
    EXTENSIONS
        .iter()
        .all(|name| has_device_extension(device_extensions, name))
}

/// The two extensions' function tables, loaded once the device enabled them.
pub(crate) struct Win32Interop {
    memory: khr::external_memory_win32::Device,
    semaphore: khr::external_semaphore_win32::Device,
}

impl core::fmt::Debug for Win32Interop {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Win32Interop")
    }
}

impl Win32Interop {
    /// Loads both tables for `device`, which must have enabled [`EXTENSIONS`].
    pub(crate) fn load(instance: &ash::Instance, device: &ash::Device) -> Self {
        Self {
            memory: khr::external_memory_win32::Device::new(instance, device),
            semaphore: khr::external_semaphore_win32::Device::new(instance, device),
        }
    }
}

impl DeviceInner {
    /// Imports the D3D11 texture shared through `handle` as an image of
    /// `info`'s shape, bound to a dedicated import of its memory.
    ///
    /// `info` must describe the texture: its extent and a format of its own.
    /// Vulkan keeps its own reference to the memory, so the handle may be
    /// closed afterwards.
    ///
    /// # Errors
    ///
    /// [`HalError::Unsupported`] on a device without [`EXTENSIONS`], and the
    /// driver's error, naming the call, for anything it refuses.
    pub(crate) fn import_d3d11_texture(
        &self,
        handle: isize,
        info: &vk::ImageCreateInfo<'_>,
        label: Option<&str>,
    ) -> Result<(vk::Image, vk::DeviceMemory), HalError> {
        let interop = self.win32_interop()?;
        let handle_type = vk::ExternalMemoryHandleTypeFlags::D3D11_TEXTURE;
        let mut external = vk::ExternalMemoryImageCreateInfo::default().handle_types(handle_type);
        let info = info.push_next(&mut external);
        // SAFETY: `info` is a fully populated descriptor whose only chained
        // struct is the local above.
        let image = unsafe { self.raw.create_image(&info, None) }
            .map_err(|error| conv::hal_error("vkCreateImage", error))?;
        let destroy_image = || {
            // SAFETY: `image` is live, unbound and unused.
            unsafe { self.raw.destroy_image(image, None) };
        };
        // SAFETY: `image` was just created by this device.
        let requirements = unsafe { self.raw.get_image_memory_requirements(image) };
        let mut properties = vk::MemoryWin32HandlePropertiesKHR::default();
        // SAFETY: the handle is a live NT handle to a D3D11 texture, which is
        // what `D3D11_TEXTURE` names, and `properties` is a live local.
        if let Err(error) = unsafe {
            interop
                .memory
                .get_memory_win32_handle_properties(handle_type, handle, &mut properties)
        } {
            destroy_image();
            return Err(conv::hal_error(
                "vkGetMemoryWin32HandlePropertiesKHR",
                error,
            ));
        }
        let Some(type_index) = mem::find_memory_type(
            self.memory_properties(),
            requirements.memory_type_bits & properties.memory_type_bits,
            MemoryRequest::for_location(crcbl_hal::MemoryLocation::DeviceLocal),
        ) else {
            destroy_image();
            return Err(HalError::Backend(format!(
                "no memory type both the image ({:#x}) and the shared texture ({:#x}) allow",
                requirements.memory_type_bits, properties.memory_type_bits
            )));
        };
        // A D3D11 texture is imported whole, into an allocation of its own:
        // the driver reports `DEDICATED_ONLY` for this handle type.
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let mut import = vk::ImportMemoryWin32HandleInfoKHR::default()
            .handle_type(handle_type)
            .handle(handle);
        let allocate = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(type_index)
            .push_next(&mut import)
            .push_next(&mut dedicated);
        // SAFETY: the chained structs are locals that outlive the call, the
        // type index came from both masks above, and `dedicated` names the
        // image this allocation is for.
        let memory = match unsafe { self.raw.allocate_memory(&allocate, None) } {
            Ok(memory) => memory,
            Err(error) => {
                destroy_image();
                return Err(conv::hal_error("vkAllocateMemory (D3D11 import)", error));
            }
        };
        // SAFETY: `memory` is the image's dedicated import and is bound once.
        if let Err(error) = unsafe { self.raw.bind_image_memory(image, memory, 0) } {
            // SAFETY: both are live and unused.
            unsafe { self.raw.free_memory(memory, None) };
            destroy_image();
            return Err(conv::hal_error("vkBindImageMemory", error));
        }
        self.set_object_name(image, label);
        Ok((image, memory))
    }

    /// Imports the D3D fence shared through `handle` as a timeline semaphore.
    ///
    /// The fence's value and the semaphore's counter are then one value: a
    /// signal on either side is seen by a wait on the other.
    ///
    /// # Errors
    ///
    /// As [`DeviceInner::import_d3d11_texture`].
    pub(crate) fn import_d3d_fence(
        &self,
        handle: isize,
        label: Option<&str>,
    ) -> Result<vk::Semaphore, HalError> {
        let interop = self.win32_interop()?;
        let mut timeline = vk::SemaphoreTypeCreateInfo::default()
            .semaphore_type(vk::SemaphoreType::TIMELINE)
            .initial_value(0);
        let info = vk::SemaphoreCreateInfo::default().push_next(&mut timeline);
        // SAFETY: the chained struct is a local that outlives the call.
        let semaphore = unsafe { self.raw.create_semaphore(&info, None) }
            .map_err(|error| conv::hal_error("vkCreateSemaphore", error))?;
        // `D3D12_FENCE` is the handle type for a D3D11 fence too: both are the
        // same kernel object.
        let import = vk::ImportSemaphoreWin32HandleInfoKHR::default()
            .semaphore(semaphore)
            .handle_type(vk::ExternalSemaphoreHandleTypeFlags::D3D12_FENCE)
            .handle(handle);
        // SAFETY: `semaphore` is a live timeline semaphore of this device and
        // the handle a live NT handle to a shared fence.
        if let Err(error) = unsafe { interop.semaphore.import_semaphore_win32_handle(&import) } {
            // SAFETY: live and unused.
            unsafe { self.raw.destroy_semaphore(semaphore, None) };
            return Err(conv::hal_error("vkImportSemaphoreWin32HandleKHR", error));
        }
        self.set_object_name(semaphore, label);
        Ok(semaphore)
    }

    /// The loaded tables, or the refusal a device without them answers with.
    fn win32_interop(&self) -> Result<&Win32Interop, HalError> {
        self.win32_interop.as_ref().ok_or(HalError::Unsupported {
            backend: crcbl_hal::BackendKind::Vulkan,
            what: "VK_KHR_external_memory_win32 and VK_KHR_external_semaphore_win32 are not \
                   both available on this device",
        })
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use std::sync::Arc;

    use ash::vk;
    use crcbl_hal::{DeviceDesc, Format};

    use crate::device::VkDevice;

    /// The shared image's extent: 64 texels of four bytes is one 256-byte row,
    /// a pitch every driver lays out without padding.
    const EXTENT: (u32, u32) = (64, 4);

    /// The clear colour, and the bytes it lands as in a `Bgra8Unorm` texture.
    /// Every channel is a multiple of 17, so `f32` to `unorm8` is exact.
    const CLEAR: ([f32; 4], [u8; 4]) = (
        [34.0 / 255.0, 68.0 / 255.0, 102.0 / 255.0, 136.0 / 255.0],
        [0x66, 0x44, 0x22, 0x88],
    );

    /// **A texture D3D11 shares is drawn into by Vulkan and read back by D3D11
    /// with exactly what Vulkan wrote, ordered by the shared fence alone.**
    ///
    /// No window: this is the half of presenting to a transparent window that
    /// a headless run can check. Vulkan imports the texture and the `ready`
    /// fence, clears the image, releases it to the external queue family and
    /// signals `ready` with 1; D3D11 waits for 1 on its own queue and copies
    /// the texture out.
    ///
    /// Red if the import binds anything but the shared memory (D3D11 then
    /// reads a texture nothing wrote), or if the fence import does not reach
    /// D3D11's fence (its wait never ends, and the read hangs rather than
    /// passing).
    #[test]
    #[ignore = "needs a Windows Vulkan driver with VK_KHR_external_memory_win32"]
    fn a_shared_texture_cleared_by_vulkan_reads_back_in_d3d11() {
        let instance = crate::VkInstance::open().expect("a Vulkan instance");
        let record = &instance.inner().adapters[0];
        assert!(
            record.win32_interop,
            "{:?} lists no Win32 interop, so this test could check nothing",
            record.info.name
        );
        let luid = record
            .info
            .luid
            .expect("a Windows driver reports a LUID")
            .to_le_bytes();
        let device = VkDevice::open(
            Arc::clone(instance.inner()),
            record,
            &DeviceDesc::for_adapter(record.info.id),
            None,
        )
        .expect("a device");
        let inner = device.inner();
        let raw = &inner.raw;

        let mut interop = crcbl_dcomp::Interop::open(luid, EXTENT, Format::Bgra8Unorm, 1)
            .expect("a D3D11 device on the same adapter");
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::B8G8R8A8_UNORM)
            .extent(vk::Extent3D {
                width: EXTENT.0,
                height: EXTENT.1,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let (image, memory) = inner
            .import_d3d11_texture(interop.image_handle(0), &info, Some("shared clear"))
            .expect("the shared texture imports");
        let ready = inner
            .import_d3d_fence(interop.ready_handle(), Some("shared ready"))
            .expect("the shared fence imports");

        let family = record.families.graphics.expect("a graphics family");
        // SAFETY: test-only driver calls on a device this test owns, from one
        // thread; every object is destroyed below after the queue is idle.
        unsafe {
            let queue = raw.get_device_queue(family, 0);
            let pool = raw
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default().queue_family_index(family),
                    None,
                )
                .expect("a pool");
            let commands = raw
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .expect("a command buffer")[0];
            raw.begin_command_buffer(commands, &vk::CommandBufferBeginInfo::default())
                .expect("begin");
            let range = vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            };
            let to_clear = vk::ImageMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::NONE)
                .dst_stage_mask(vk::PipelineStageFlags2::CLEAR)
                .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .image(image)
                .subresource_range(range);
            raw.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&[to_clear]),
            );
            raw.cmd_clear_color_image(
                commands,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &vk::ClearColorValue { float32: CLEAR.0 },
                &[range],
            );
            // The release: the clear's writes are made available and the image
            // leaves this queue family for the API on the other side.
            let release = vk::ImageMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::CLEAR)
                .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(family)
                .dst_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
                .image(image)
                .subresource_range(range);
            raw.cmd_pipeline_barrier2(
                commands,
                &vk::DependencyInfo::default().image_memory_barriers(&[release]),
            );
            raw.end_command_buffer(commands).expect("end");
            let command_infos = [vk::CommandBufferSubmitInfo::default().command_buffer(commands)];
            let signals = [vk::SemaphoreSubmitInfo::default()
                .semaphore(ready)
                .value(1)
                .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)];
            let submit = vk::SubmitInfo2::default()
                .command_buffer_infos(&command_infos)
                .signal_semaphore_infos(&signals);
            raw.queue_submit2(queue, &[submit], vk::Fence::null())
                .expect("submit");

            let pixels = interop.read_image(0, 1).expect("D3D11 reads the texture");
            assert_eq!(pixels.len(), (EXTENT.0 * EXTENT.1 * 4) as usize);
            assert!(
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|texel| *texel == CLEAR.1),
                "D3D11 read {:?} where Vulkan cleared {:?}",
                &pixels[..4],
                CLEAR.1
            );

            raw.device_wait_idle().expect("idle");
            raw.destroy_command_pool(pool, None);
            raw.destroy_semaphore(ready, None);
            raw.destroy_image(image, None);
            raw.free_memory(memory, None);
        }
    }
}
