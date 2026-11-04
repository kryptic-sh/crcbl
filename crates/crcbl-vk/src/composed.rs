//! A swapchain for a transparent Win32 window.
//!
//! Such a window shows only a DirectComposition visual, and a `VkSwapchainKHR`
//! cannot be a visual's content. So the swapchain here is a ring of images
//! shared with a small D3D11 device — `crcbl_dcomp::SharedPresenter` — which
//! copies each presented image into a composition swapchain bound to the
//! window's visual. To everything above the seam it is a swapchain like any
//! other: `acquire_next_frame` hands out an image and its view, the frame is
//! recorded and submitted, and `present` puts it on the window.
//!
//! # One present, step by step
//!
//! 1. `present` submits, on the caller's queue, a wait on the frame's
//!    semaphores, the image's **release** to `VK_QUEUE_FAMILY_EXTERNAL` (a
//!    command buffer recorded once per image), and a signal of the shared
//!    `ready` fence with the next value.
//! 2. `SharedPresenter::present` queues D3D11's wait for that value, the copy
//!    into the back buffer, a signal of the shared `done` fence, and the
//!    present. Nothing on the CPU waits for the GPU.
//! 3. The image's `done` value is kept. `acquire_next_frame` waits for it on
//!    the host before handing the image out again: with the ring sized as the
//!    caller asked, the copy finished long before, so the wait returns at once.
//!
//! The next frame's barrier on the image declares `Undefined`, which takes it
//! back from the external queue family with its contents discarded, as an
//! acquired swapchain image's contents are.
//!
//! # Locking
//!
//! Every queue submission here is made under the device's state lock, which
//! is what externally synchronises the queue everywhere in this backend (see
//! `Device::wait_idle`).

use std::sync::Arc;

use ash::vk;
use crcbl_dcomp::{Composition, SHARED_FORMATS, SharedPresenter};
use crcbl_hal::{
    AcquiredFrame, CompositeAlpha, HalError, PresentMode, SurfaceCaps, SurfaceError, SwapchainDesc,
};

use crate::conv;
use crate::device::{DeviceInner, VkDevice};
use crate::swapchain::{self, SwapchainEntry};

/// The fewest images a composed ring has: one being drawn while D3D11 copies
/// the last.
const MIN_IMAGES: u32 = 2;
/// The most a composed ring has, as for the offscreen ring.
const MAX_IMAGES: u32 = 8;

/// How long `acquire_next_frame` waits for D3D11 to finish copying an image
/// out before calling the swapchain lost. The copy is a few microseconds of
/// GPU work behind a frame that was already presented, so a wait this long is
/// a device that stopped, not a slow one.
const DONE_TIMEOUT_NS: u64 = 5_000_000_000;

/// A transparent Win32 surface's composition, and the window it is on.
pub(crate) type ComposedSurface = (Arc<Composition>, core::ptr::NonNull<core::ffi::c_void>);

/// What a transparent window's surface offers: the formats a composition
/// swapchain takes, both alpha modes, and the window's client area.
pub(crate) fn surface_caps(hwnd: core::ptr::NonNull<core::ffi::c_void>) -> SurfaceCaps {
    SurfaceCaps {
        formats: SHARED_FORMATS.to_vec(),
        present_modes: vec![PresentMode::Fifo, PresentMode::Immediate],
        // `Opaque` first, as `crcbl-dx12` offers a transparent window's modes.
        composite_alpha: vec![CompositeAlpha::Opaque, CompositeAlpha::PreMultiplied],
        min_image_count: MIN_IMAGES,
        max_image_count: MAX_IMAGES,
        current_extent: crcbl_dcomp::client_extent(hwnd),
    }
}

/// The D3D11 side of a composed swapchain, and the Vulkan objects that talk
/// to it.
#[derive(Debug)]
pub(crate) struct ComposedRing {
    presenter: SharedPresenter,
    /// The shared `ready` fence, imported: signalled here, waited on by D3D11.
    ready: vk::Semaphore,
    /// The shared `done` fence, imported: signalled by D3D11, waited on here.
    done: vk::Semaphore,
    /// The last value `ready` was signalled with.
    ready_value: u64,
    /// Per image, the `done` value its last copy out signals; 0 for an image
    /// never presented.
    done_values: Vec<u64>,
    /// The pool the release command buffers come from.
    pool: vk::CommandPool,
    /// Per image, the recorded release to `VK_QUEUE_FAMILY_EXTERNAL`.
    releases: Vec<vk::CommandBuffer>,
    /// `1` to wait for vertical blank, `0` not to; from the present mode.
    sync_interval: u32,
}

impl ComposedRing {
    /// Waits on the host until every copy D3D11 was asked for has run.
    fn wait_all_done(&self, raw: &ash::Device) -> Result<(), HalError> {
        let last = self.done_values.iter().copied().max().unwrap_or(0);
        wait_done(raw, self.done, last)
    }

    /// Releases the Vulkan objects; the presenter goes with `self`, which
    /// takes the swapchain off the window's visual.
    ///
    /// The caller has waited for every copy and for the device to go idle.
    pub(crate) fn destroy(self, raw: &ash::Device) {
        // SAFETY: nothing uses these any more (the caller's waits), and each
        // was created by `raw`.
        unsafe {
            raw.destroy_command_pool(self.pool, None);
            raw.destroy_semaphore(self.ready, None);
            raw.destroy_semaphore(self.done, None);
        }
    }

    /// Waits for every copy out to have run, for a teardown. A failure is
    /// logged: the teardown goes ahead, as a device teardown does.
    pub(crate) fn drain(&self, raw: &ash::Device) {
        if let Err(error) = self.wait_all_done(raw) {
            crcbl_core::log::warn!(
                "crcbl-vk: waiting for a transparent window's last copies failed: {error}"
            );
        }
    }
}

/// Waits on the host until `done` reaches `value`.
fn wait_done(raw: &ash::Device, done: vk::Semaphore, value: u64) -> Result<(), HalError> {
    if value == 0 {
        return Ok(());
    }
    let semaphores = [done];
    let values = [value];
    let info = vk::SemaphoreWaitInfo::default()
        .semaphores(&semaphores)
        .values(&values);
    // SAFETY: `done` is a live timeline semaphore of `raw`; the arrays are
    // locals that outlive the call.
    unsafe { raw.wait_semaphores(&info, DONE_TIMEOUT_NS) }
        .map_err(|error| conv::hal_error("vkWaitSemaphores (shared done)", error))
}

impl VkDevice {
    /// A composed ring for `composition`'s window: the D3D11 presenter, the
    /// imported fences, and the images.
    pub(crate) fn build_composed_ring(
        &self,
        desc: &SwapchainDesc<'_>,
        composition: &Arc<Composition>,
        hwnd: core::ptr::NonNull<core::ffi::c_void>,
    ) -> Result<SwapchainEntry, SurfaceError> {
        let inner = self.inner();
        let caps = surface_caps(hwnd);
        swapchain::check_swapchain_format(desc.format, &caps.formats)?;
        swapchain::check_swapchain_composite_alpha(desc.composite_alpha, &caps.composite_alpha)?;
        let (extent, count) = composed_shape(inner, desc)?;
        let luid = inner.luid.ok_or(HalError::Unsupported {
            backend: crcbl_hal::BackendKind::Vulkan,
            what: "this adapter reports no LUID, so no D3D11 device can share its memory",
        })?;
        let presenter = SharedPresenter::new(
            composition,
            luid.to_le_bytes(),
            extent,
            desc.format,
            count,
            desc.composite_alpha,
        )?;
        let ready = inner.import_d3d_fence(presenter.interop().ready_handle(), desc.label)?;
        let done = match inner.import_d3d_fence(presenter.interop().done_handle(), desc.label) {
            Ok(done) => done,
            Err(error) => {
                // SAFETY: imported moments ago and never used.
                unsafe { inner.raw.destroy_semaphore(ready, None) };
                return Err(error.into());
            }
        };
        let pool_info = vk::CommandPoolCreateInfo::default()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(inner.graphics_family());
        // SAFETY: a plain pool on this device's graphics family.
        let pool = match unsafe { inner.raw.create_command_pool(&pool_info, None) } {
            Ok(pool) => pool,
            Err(error) => {
                // SAFETY: imported moments ago and never used.
                unsafe {
                    inner.raw.destroy_semaphore(ready, None);
                    inner.raw.destroy_semaphore(done, None);
                }
                return Err(conv::hal_error("vkCreateCommandPool", error).into());
            }
        };
        let ring = ComposedRing {
            presenter,
            ready,
            done,
            ready_value: 0,
            done_values: Vec::new(),
            pool,
            releases: Vec::new(),
            sync_interval: sync_interval(desc.present_mode),
        };
        let entry = self.fill_composed(desc, extent, ring)?;
        crcbl_core::log::info!(
            "crcbl-vk: composed swapchain {}x{} {:?} {:?}, {count} image(s), through \
             DirectComposition",
            extent.0,
            extent.1,
            desc.format,
            desc.composite_alpha,
        );
        Ok(entry)
    }

    /// Imports `ring`'s images, records their releases and registers them,
    /// returning the swapchain entry that owns them all.
    ///
    /// On an error `ring` is destroyed here, after the device is idle.
    fn fill_composed(
        &self,
        desc: &SwapchainDesc<'_>,
        extent: (u32, u32),
        mut ring: ComposedRing,
    ) -> Result<SwapchainEntry, SurfaceError> {
        let inner = self.inner();
        let count = ring.presenter.interop().image_count();
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(conv::format(desc.format))
            .extent(vk::Extent3D {
                width: extent.0,
                height: extent.1,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            // The offscreen ring's usage, for the offscreen ring's reasons:
            // an attachment, a screenshot source, a tonemap input.
            .usage(
                vk::ImageUsageFlags::COLOR_ATTACHMENT
                    | vk::ImageUsageFlags::TRANSFER_SRC
                    | vk::ImageUsageFlags::TRANSFER_DST
                    | vk::ImageUsageFlags::SAMPLED,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let mut images = Vec::with_capacity(count as usize);
        let mut memory = Vec::with_capacity(count as usize);
        let fail = |images: Vec<vk::Image>, memory: Vec<vk::DeviceMemory>, ring: ComposedRing| {
            // SAFETY: everything here was created by this device moments ago
            // and never submitted.
            unsafe {
                for image in images {
                    inner.raw.destroy_image(image, None);
                }
                for block in memory {
                    inner.raw.free_memory(block, None);
                }
            }
            ring.destroy(&inner.raw);
        };
        for index in 0..count {
            let label = desc.label.map(|label| format!("{label} [{index}]"));
            match inner.import_d3d11_texture(
                ring.presenter.interop().image_handle(index),
                &info,
                label.as_deref(),
            ) {
                Ok((image, block)) => {
                    images.push(image);
                    memory.push(block);
                }
                Err(error) => {
                    fail(images, memory, ring);
                    return Err(error.into());
                }
            }
        }
        match record_releases(inner, ring.pool, &images) {
            Ok(releases) => ring.releases = releases,
            Err(error) => {
                fail(images, memory, ring);
                return Err(error.into());
            }
        }
        ring.done_values = vec![0; count as usize];

        let mut state = inner.state();
        let image_handles = self.register_ring_images(&mut state, &images, desc.format);
        let (views, view_handles) =
            match self.build_views(&mut state, &images, desc.format, desc.label) {
                Ok(views) => views,
                Err(error) => {
                    self.forget_ring_images(&mut state, &image_handles);
                    drop(state);
                    fail(images, memory, ring);
                    return Err(error);
                }
            };
        drop(state);
        Ok(SwapchainEntry {
            memory,
            composed: Some(Box::new(ring)),
            ..SwapchainEntry::fresh(inner.id, extent, images, views, view_handles, image_handles)
        })
    }

    /// Resizes a composed swapchain in place: the presenter, its binding to
    /// the window's visual and the fences stay, the images are rebuilt.
    pub(crate) fn reconfigure_composed(
        &self,
        swapchain: crcbl_hal::SwapchainHandle,
        desc: &SwapchainDesc<'_>,
        hwnd: core::ptr::NonNull<core::ffi::c_void>,
    ) -> Result<(), SurfaceError> {
        let inner = self.inner();
        let caps = surface_caps(hwnd);
        swapchain::check_swapchain_format(desc.format, &caps.formats)?;
        // A composition swapchain's alpha mode is fixed at creation, and
        // `ResizeBuffers` cannot change it; `crcbl-dx12` refuses the same.
        let (extent, _) = composed_shape(inner, desc)?;
        let (mut ring, old) = {
            let mut state = inner.state();
            let entry =
                crate::device::lookup_mut(&mut state.swapchains, "swapchain", swapchain, inner)?;
            let Some(ring) = entry.composed.take() else {
                return Err(SurfaceError::Hal(HalError::InvalidDescriptor(
                    "reconfigure_swapchain cannot move a swapchain to a different surface"
                        .to_string(),
                )));
            };
            let old = core::mem::replace(
                entry,
                SwapchainEntry::fresh(
                    inner.id,
                    extent,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                ),
            );
            crate::device::forget_swapchain_rows(inner, &mut state, &old);
            (*ring, old)
        };
        ring.drain(&inner.raw);
        // The old images are finished on both sides once D3D11's copies have
        // run and this device is idle; they are this swapchain's alone, so
        // they go now rather than through the deletion queue.
        inner.wait_idle_raw()?;
        // SAFETY: idle above, and owned by the entry taken out of the table.
        unsafe {
            for view in old.views {
                inner.raw.destroy_image_view(view, None);
            }
            for image in old.images {
                inner.raw.destroy_image(image, None);
            }
            for block in old.memory {
                inner.raw.free_memory(block, None);
            }
            inner
                .raw
                .reset_command_pool(ring.pool, vk::CommandPoolResetFlags::empty())
                .map_err(|error| conv::hal_error("vkResetCommandPool", error))?;
            inner.raw.free_command_buffers(ring.pool, &ring.releases);
        }
        ring.releases.clear();
        let count = desc.image_count.clamp(MIN_IMAGES, MAX_IMAGES);
        if let Err(error) = ring.presenter.reconfigure(extent, desc.format, count) {
            ring.destroy(&inner.raw);
            return Err(error.into());
        }
        ring.sync_interval = sync_interval(desc.present_mode);
        let built = self.fill_composed(desc, extent, ring)?;
        let mut state = inner.state();
        let entry =
            crate::device::lookup_mut(&mut state.swapchains, "swapchain", swapchain, inner)?;
        *entry = built;
        Ok(())
    }
}

impl DeviceInner {
    /// Hands out the next image of a composed ring, once D3D11 has finished
    /// copying it out after its last present.
    pub(crate) fn acquire_composed(
        &self,
        entry: &mut SwapchainEntry,
    ) -> Result<AcquiredFrame, SurfaceError> {
        let Some(ring) = entry.composed.as_ref() else {
            return Err(SurfaceError::Lost);
        };
        let index = entry.next_offscreen;
        wait_done(&self.raw, ring.done, ring.done_values[index as usize])?;
        entry.acquired = Some(index);
        Ok(entry.frame(index, None, false))
    }

    /// Presents image `index` of a composed ring: the release and the `ready`
    /// signal on `queue`, then D3D11's copy and present.
    ///
    /// The caller holds the state lock, which synchronises `queue`.
    pub(crate) fn present_composed(
        &self,
        entry: &mut SwapchainEntry,
        queue: vk::Queue,
        index: u32,
        waits: &[vk::Semaphore],
    ) -> Result<(), SurfaceError> {
        let Some(ring) = entry.composed.as_mut() else {
            return Err(SurfaceError::Lost);
        };
        let value = ring.ready_value + 1;
        let wait_infos: Vec<vk::SemaphoreSubmitInfo<'_>> = waits
            .iter()
            .map(|semaphore| {
                vk::SemaphoreSubmitInfo::default()
                    .semaphore(*semaphore)
                    .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
            })
            .collect();
        let commands = [
            vk::CommandBufferSubmitInfo::default().command_buffer(ring.releases[index as usize])
        ];
        let signals = [vk::SemaphoreSubmitInfo::default()
            .semaphore(ring.ready)
            .value(value)
            .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)];
        let submit = vk::SubmitInfo2::default()
            .wait_semaphore_infos(&wait_infos)
            .command_buffer_infos(&commands)
            .signal_semaphore_infos(&signals);
        // SAFETY: `queue` is this device's and synchronised by the state lock
        // the caller holds; every handle in `submit` is live.
        unsafe { self.raw.queue_submit2(queue, &[submit], vk::Fence::null()) }
            .map_err(|error| conv::hal_error("vkQueueSubmit2 (composed present)", error))?;
        ring.ready_value = value;
        let done = ring.presenter.present(index, value, ring.sync_interval)?;
        ring.done_values[index as usize] = done;
        #[allow(clippy::cast_possible_truncation)]
        {
            entry.next_offscreen = (index + 1) % entry.images.len() as u32;
        }
        Ok(())
    }
}

/// The extent and image count a composed ring is built at.
fn composed_shape(
    inner: &DeviceInner,
    desc: &SwapchainDesc<'_>,
) -> Result<((u32, u32), u32), SurfaceError> {
    let extent = swapchain::resolve_swapchain_extent(
        desc.extent,
        &vk::SurfaceCapabilitiesKHR {
            min_image_extent: vk::Extent2D {
                width: 1,
                height: 1,
            },
            max_image_extent: vk::Extent2D {
                width: inner.caps.limits.max_image_2d,
                height: inner.caps.limits.max_image_2d,
            },
            ..Default::default()
        },
    )?;
    Ok((
        extent.configured,
        desc.image_count.clamp(MIN_IMAGES, MAX_IMAGES),
    ))
}

/// `1` to wait for vertical blank, `0` not to.
const fn sync_interval(mode: PresentMode) -> u32 {
    match mode {
        PresentMode::Fifo | PresentMode::FifoRelaxed => 1,
        _ => 0,
    }
}

/// One recorded command buffer per image, releasing it from
/// `PRESENT_SRC_KHR` on the graphics family to `VK_QUEUE_FAMILY_EXTERNAL`.
fn record_releases(
    inner: &DeviceInner,
    pool: vk::CommandPool,
    images: &[vk::Image],
) -> Result<Vec<vk::CommandBuffer>, HalError> {
    let allocate = vk::CommandBufferAllocateInfo::default()
        .command_pool(pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(u32::try_from(images.len()).unwrap_or(u32::MAX));
    // SAFETY: `pool` is a live pool of this device.
    let buffers = unsafe { inner.raw.allocate_command_buffers(&allocate) }
        .map_err(|error| conv::hal_error("vkAllocateCommandBuffers", error))?;
    let family = inner.graphics_family();
    for (buffer, image) in buffers.iter().zip(images) {
        let release = vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
            .src_access_mask(vk::AccessFlags2::MEMORY_WRITE)
            .old_layout(vk::ImageLayout::PRESENT_SRC_KHR)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_queue_family_index(family)
            .dst_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
            .image(*image)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            });
        // SAFETY: a fresh primary buffer of this device, recorded once and
        // submitted only by `present_composed`; the barrier names an image of
        // this device.
        unsafe {
            inner
                .raw
                .begin_command_buffer(*buffer, &vk::CommandBufferBeginInfo::default())
                .map_err(|error| conv::hal_error("vkBeginCommandBuffer", error))?;
            inner.raw.cmd_pipeline_barrier2(
                *buffer,
                &vk::DependencyInfo::default().image_memory_barriers(&[release]),
            );
            inner
                .raw
                .end_command_buffer(*buffer)
                .map_err(|error| conv::hal_error("vkEndCommandBuffer", error))?;
        }
    }
    Ok(buffers)
}
