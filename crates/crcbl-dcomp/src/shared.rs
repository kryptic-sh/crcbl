//! Presenting another API's frames to a transparent window: the D3D11 half.
//!
//! A backend whose own swapchain cannot be a DirectComposition visual's content
//! — Vulkan's, on Win32 — renders into textures this module creates and shares
//! through NT handles, and hands each finished one back with a fence value.
//! This module copies it into a composition swapchain bound to the window's
//! [`Composition`] and presents that.
//!
//! # The two fences
//!
//! * **`ready`** is signalled by the renderer when an image's frame is done,
//!   with a value it counts itself. [`Interop`] makes the D3D11 queue wait for
//!   that value before it reads the image — a GPU wait, so nothing on the CPU
//!   blocks.
//! * **`done`** is signalled here once the copy out of an image has run, with
//!   a value this module counts and returns. The renderer waits for it before
//!   it draws into that image again.
//!
//! Both are D3D11 fences shared through NT handles, which a Vulkan timeline
//! semaphore imports (`VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_D3D12_FENCE_BIT`
//! covers a D3D11 fence: both are the same kernel object).
//!
//! # Why D3D11
//!
//! The device here only copies and presents. D3D11 does both with an immediate
//! context and no command lists, allocators or queues of its own to manage,
//! which is less to get wrong than a second D3D12 device would be.
//!
//! # The cost
//!
//! One GPU copy of the frame per present, and a second driver's device in the
//! process. The copy reads and writes the frame once each; the device is what
//! start-up pays for. `docs/backlog.md` (_Vulkan on a transparent Win32
//! window_) records what was measured.

use std::sync::Arc;

use crcbl_hal::{CompositeAlpha, Format, HalError};
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, HMODULE, HWND, LUID, RECT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_1};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_FENCE_FLAG_SHARED, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_RESOURCE_MISC_SHARED, D3D11_RESOURCE_MISC_SHARED_NTHANDLE,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11Device5, ID3D11DeviceContext, ID3D11DeviceContext4,
    ID3D11Fence, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM,
    DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
    DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SHARED_RESOURCE_READ,
    DXGI_SHARED_RESOURCE_WRITE, DXGI_SWAP_CHAIN_FLAG, IDXGIAdapter1, IDXGIFactory4, IDXGIResource1,
    IDXGISwapChain1,
};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{Interface, PCWSTR};

use crate::composition::{Binding, Composition, failed};
use crate::desc::swap_chain_desc;

#[cfg(test)]
mod tests;

/// The formats a shared image can be, in the order a surface offers them:
/// sRGB first, because the engine presents an sRGB swapchain.
///
/// Each is a format a composition swapchain takes, or the sRGB spelling of one,
/// which shares its buffer: [`Interop`] copies an sRGB image into a linear
/// buffer of the same layout, which D3D11's `CopyResource` allows within one
/// type group.
pub const SHARED_FORMATS: &[Format] = &[
    Format::Bgra8UnormSrgb,
    Format::Rgba8UnormSrgb,
    Format::Bgra8Unorm,
    Format::Rgba8Unorm,
    Format::Rgb10a2Unorm,
    Format::Rgba16Float,
];

/// Buffers in the composition swapchain. Two: a frame is copied in and
/// presented, so the buffer being composed and the one being written are all
/// a flip-model chain ever needs here.
const SWAPCHAIN_BUFFERS: u32 = 2;

/// The shared image's format, and the swapchain buffer format it is copied
/// into, for a format in [`SHARED_FORMATS`].
fn dxgi_formats(format: Format) -> Option<(DXGI_FORMAT, DXGI_FORMAT)> {
    Some(match format {
        Format::Bgra8UnormSrgb => (DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_B8G8R8A8_UNORM),
        Format::Rgba8UnormSrgb => (DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_UNORM),
        Format::Bgra8Unorm => (DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM),
        Format::Rgba8Unorm => (DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM),
        Format::Rgb10a2Unorm => (DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R10G10B10A2_UNORM),
        Format::Rgba16Float => (
            DXGI_FORMAT_R16G16B16A16_FLOAT,
            DXGI_FORMAT_R16G16B16A16_FLOAT,
        ),
        _ => return None,
    })
}

/// An NT handle this module created and closes.
#[derive(Debug)]
struct OwnedHandle(HANDLE);

impl OwnedHandle {
    /// The handle's value, for the importing API.
    fn raw(&self) -> isize {
        self.0.0 as isize
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by `CreateSharedHandle`, which
        // documents closing it with `CloseHandle`, and nothing else closes it.
        // An importer holds its own reference to the object, so closing ours
        // does not take the object away from it.
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            crcbl_core::log::debug!("crcbl-dcomp: closing a shared handle failed: {error}");
        }
    }
}

/// A texture and the NT handle it is shared through.
#[derive(Debug)]
struct SharedImage {
    handle: OwnedHandle,
    texture: ID3D11Texture2D,
}

/// A fence and the NT handle it is shared through.
#[derive(Debug)]
struct SharedFence {
    handle: OwnedHandle,
    fence: ID3D11Fence,
}

/// The D3D11 device on the renderer's adapter, the images it shares, and the
/// two fences — everything but the window, so it can be built and checked
/// without one.
#[derive(Debug)]
pub struct Interop {
    images: Vec<SharedImage>,
    ready: SharedFence,
    done: SharedFence,
    /// The last value [`Interop`] signalled `done` with.
    done_value: u64,
    extent: (u32, u32),
    buffer_format: DXGI_FORMAT,
    context: ID3D11DeviceContext4,
    device: ID3D11Device5,
}

// SAFETY: `windows-rs` marks `ID3D11Device` agile but leaves the immediate
// context and the texture and fence interfaces unmarked. The D3D11 documentation
// makes the device free-threaded and the immediate context *not* thread-safe:
// two threads may use it, one at a time. Every method that touches the context
// takes `&mut self`, so the borrow checker gives exactly that — one caller at a
// time, whichever thread it is on — and textures and fences are only read
// through the context or passed to it.
unsafe impl Send for Interop {}

impl Interop {
    /// A D3D11 device on the adapter whose LUID is `luid`, `count` textures of
    /// `extent` and `format` shared through NT handles, and the two fences.
    ///
    /// `luid` is the renderer's adapter, so both devices are on one GPU and
    /// the shared memory is that GPU's: Vulkan's `VkPhysicalDeviceIDProperties`
    /// reports it as `deviceLUID`.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] for a format outside [`SHARED_FORMATS`],
    /// a zero extent or no images, and [`HalError::Backend`] naming the call
    /// that failed.
    pub fn open(
        luid: [u8; 8],
        extent: (u32, u32),
        format: Format,
        count: u32,
    ) -> Result<Self, HalError> {
        let Some((texture_format, buffer_format)) = dxgi_formats(format) else {
            return Err(HalError::InvalidDescriptor(format!(
                "{format:?} cannot be shared with a composition swapchain; the formats that can \
                 are {SHARED_FORMATS:?}"
            )));
        };
        if extent.0 == 0 || extent.1 == 0 || count == 0 {
            return Err(HalError::InvalidDescriptor(format!(
                "a shared image ring needs a non-zero extent and count, not {extent:?} x {count}"
            )));
        }
        let (device, context) = open_device(luid)?;
        let ready = shared_fence(&device)?;
        let done = shared_fence(&device)?;
        let images = (0..count)
            .map(|_| shared_texture(&device, extent, texture_format))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            images,
            ready,
            done,
            done_value: 0,
            extent,
            buffer_format,
            context,
            device,
        })
    }

    /// How many images are shared.
    #[must_use]
    pub fn image_count(&self) -> u32 {
        u32::try_from(self.images.len()).unwrap_or(u32::MAX)
    }

    /// The NT handle `index`'s texture is shared through, for the renderer to
    /// import (`VK_EXTERNAL_MEMORY_HANDLE_TYPE_D3D11_TEXTURE_BIT`). It stays
    /// open as long as this does; an importer keeps its own reference.
    ///
    /// # Panics
    ///
    /// If `index` is not below [`Interop::image_count`].
    #[must_use]
    pub fn image_handle(&self, index: u32) -> isize {
        self.images[index as usize].handle.raw()
    }

    /// The NT handle of the fence the renderer signals when an image is ready.
    #[must_use]
    pub fn ready_handle(&self) -> isize {
        self.ready.handle.raw()
    }

    /// The NT handle of the fence this signals when a copy out of an image has
    /// run, with the values [`SharedPresenter::present`] returns.
    #[must_use]
    pub fn done_handle(&self) -> isize {
        self.done.handle.raw()
    }

    /// Queues a copy of image `index` into `target` behind the renderer's
    /// `ready` value, then signals `done` and returns its value.
    fn copy_into(
        &mut self,
        target: &ID3D11Texture2D,
        index: u32,
        ready: u64,
    ) -> Result<u64, HalError> {
        let source = &self.images[index as usize].texture;
        // SAFETY: the context and the fence are this device's, and the wait is
        // a GPU wait queued on the context; the renderer signals `ready` with
        // this value once the image is written.
        unsafe { self.context.Wait(&self.ready.fence, ready) }
            .map_err(|error| failed("ID3D11DeviceContext4::Wait", &error))?;
        // SAFETY: both textures are this device's, the same extent, and of one
        // DXGI type group, which is what `CopyResource` requires.
        unsafe { self.context.CopyResource(target, source) };
        self.done_value += 1;
        // SAFETY: as for `Wait` above; the signal is queued behind the copy.
        unsafe { self.context.Signal(&self.done.fence, self.done_value) }
            .map_err(|error| failed("ID3D11DeviceContext4::Signal", &error))?;
        Ok(self.done_value)
    }

    /// Image `index`'s pixels once the renderer has signalled `ready`, rows
    /// packed tightly — for tests and captures, not for presenting.
    ///
    /// Blocks until the copy has run.
    ///
    /// # Errors
    ///
    /// [`HalError::Backend`] naming the call that failed.
    pub fn read_image(&mut self, index: u32, ready: u64) -> Result<Vec<u8>, HalError> {
        let texture_format = self.images[index as usize].texture_format();
        let staging = texture(
            &self.device,
            self.extent,
            texture_format,
            D3D11_USAGE_STAGING.0,
            0,
            D3D11_CPU_ACCESS_READ.0,
            0,
        )?;
        self.copy_into(&staging, index, ready)?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: `staging` is a CPU-readable staging texture of this device,
        // and `Map` blocks until the copy into it has run.
        unsafe {
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped))
        }
        .map_err(|error| failed("ID3D11DeviceContext::Map", &error))?;
        let row = self.extent.0 as usize * bytes_per_texel(self.buffer_format);
        let mut pixels = Vec::with_capacity(row * self.extent.1 as usize);
        for y in 0..self.extent.1 as usize {
            // SAFETY: `Map` succeeded, so `pData` points at `extent.1` rows of
            // `RowPitch` bytes each, and a row's first `row` bytes are its
            // texels.
            let line = unsafe {
                core::slice::from_raw_parts(
                    mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize),
                    row,
                )
            };
            pixels.extend_from_slice(line);
        }
        // SAFETY: mapped above, and unmapped once.
        unsafe { self.context.Unmap(&staging, 0) };
        Ok(pixels)
    }
}

impl SharedImage {
    /// The texture's own format.
    fn texture_format(&self) -> DXGI_FORMAT {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: the texture is live; the call fills `desc`.
        unsafe { self.texture.GetDesc(&raw mut desc) };
        desc.Format
    }
}

/// An [`Interop`] presenting to a transparent window: its images are copied
/// into a composition swapchain bound to the window's [`Composition`].
///
/// Field order is drop order: the binding first, which takes the swapchain
/// off the visual, then the swapchain, then the device that made it.
#[derive(Debug)]
pub struct SharedPresenter {
    /// Held for its drop, which takes the swapchain off the visual.
    _binding: Binding,
    swapchain: IDXGISwapChain1,
    interop: Interop,
}

impl SharedPresenter {
    /// An [`Interop`] as [`Interop::open`] builds it, and a composition
    /// swapchain of its extent on `composition`'s visual, blended by `alpha`.
    ///
    /// # Errors
    ///
    /// As [`Interop::open`]; [`HalError::InvalidDescriptor`] for an alpha mode
    /// a composition swapchain cannot take, or while another swapchain is
    /// bound to the visual; [`HalError::Backend`] naming the call that failed.
    pub fn new(
        composition: &Arc<Composition>,
        luid: [u8; 8],
        extent: (u32, u32),
        format: Format,
        count: u32,
        alpha: CompositeAlpha,
    ) -> Result<Self, HalError> {
        let alpha = match alpha {
            CompositeAlpha::PreMultiplied => DXGI_ALPHA_MODE_PREMULTIPLIED,
            CompositeAlpha::Opaque => DXGI_ALPHA_MODE_IGNORE,
            other => {
                return Err(HalError::InvalidDescriptor(format!(
                    "a composition swapchain blends premultiplied or not at all, not {other:?}"
                )));
            }
        };
        let interop = Interop::open(luid, extent, format, count)?;
        // SAFETY: no flags; the factory is released at the end of this call.
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }
            .map_err(|error| failed("CreateDXGIFactory2", &error))?;
        let desc = swap_chain_desc(
            extent,
            interop.buffer_format,
            SWAPCHAIN_BUFFERS,
            DXGI_SWAP_CHAIN_FLAG(0),
            alpha,
        );
        // SAFETY: the device is live and `desc` is the flip-model, stretched
        // descriptor `CreateSwapChainForComposition` requires.
        let swapchain = unsafe {
            factory.CreateSwapChainForComposition(&interop.device, &raw const desc, None)
        }
        .map_err(|error| failed("IDXGIFactory2::CreateSwapChainForComposition", &error))?;
        let binding = composition.bind(&swapchain)?;
        Ok(Self {
            _binding: binding,
            swapchain,
            interop,
        })
    }

    /// The shared images and fences.
    #[must_use]
    pub const fn interop(&self) -> &Interop {
        &self.interop
    }

    /// Rebuilds the shared images at `extent` in `format`, `count` of them,
    /// and resizes the swapchain's buffers to match — the binding, the visual
    /// and the fences stay, so the window never shows an unbound visual.
    ///
    /// The caller must have stopped using every old image, on both sides: the
    /// renderer's imports released and every copy this signalled `done` for
    /// complete. The old handles close here.
    ///
    /// # Errors
    ///
    /// As [`Interop::open`]. On an error the images are gone and the
    /// swapchain may be at either size; the caller rebuilds or destroys it.
    pub fn reconfigure(
        &mut self,
        extent: (u32, u32),
        format: Format,
        count: u32,
    ) -> Result<(), HalError> {
        let Some((texture_format, buffer_format)) = dxgi_formats(format) else {
            return Err(HalError::InvalidDescriptor(format!(
                "{format:?} cannot be shared with a composition swapchain; the formats that can \
                 are {SHARED_FORMATS:?}"
            )));
        };
        if extent.0 == 0 || extent.1 == 0 || count == 0 {
            return Err(HalError::InvalidDescriptor(format!(
                "a shared image ring needs a non-zero extent and count, not {extent:?} x {count}"
            )));
        }
        self.interop.images.clear();
        // SAFETY: nothing holds a reference to a back buffer: `present`
        // releases the one it takes before returning, which is what
        // `ResizeBuffers` requires.
        unsafe {
            self.swapchain.ResizeBuffers(
                SWAPCHAIN_BUFFERS,
                extent.0,
                extent.1,
                buffer_format,
                DXGI_SWAP_CHAIN_FLAG(0),
            )
        }
        .map_err(|error| failed("IDXGISwapChain::ResizeBuffers", &error))?;
        self.interop.images = (0..count)
            .map(|_| shared_texture(&self.interop.device, extent, texture_format))
            .collect::<Result<Vec<_>, _>>()?;
        self.interop.extent = extent;
        self.interop.buffer_format = buffer_format;
        Ok(())
    }

    /// Presents image `index` once the renderer has signalled `ready` with
    /// that value: copies it into the swapchain's back buffer and presents it
    /// with `sync_interval` (`1` waits for vertical blank, `0` does not).
    ///
    /// Returns the `done` value the copy signals; the image may be drawn into
    /// again once `done` reaches it.
    ///
    /// # Errors
    ///
    /// [`HalError::Backend`] naming the call that failed.
    pub fn present(&mut self, index: u32, ready: u64, sync_interval: u32) -> Result<u64, HalError> {
        // SAFETY: buffer 0 of a D3D11 flip-model swapchain is always the
        // current back buffer, and it is a 2D texture.
        let back: ID3D11Texture2D = unsafe { self.swapchain.GetBuffer(0) }
            .map_err(|error| failed("IDXGISwapChain::GetBuffer", &error))?;
        let done = self.interop.copy_into(&back, index, ready)?;
        drop(back);
        // SAFETY: the swapchain is live; no flags.
        unsafe { self.swapchain.Present(sync_interval, DXGI_PRESENT(0)) }
            .ok()
            .map_err(|error| failed("IDXGISwapChain::Present", &error))?;
        Ok(done)
    }
}

// SAFETY: `Interop`'s argument covers the device and context; the swapchain is
// only used through `&mut self` as the context is, and `Binding` is already
// `Send` (its `Composition` is `Send + Sync`).
unsafe impl Send for SharedPresenter {}

/// The client area of `hwnd`, in pixels, or `None` when the window will not
/// say — what a transparent window's surface reports as its current extent.
#[must_use]
pub fn client_extent(hwnd: core::ptr::NonNull<core::ffi::c_void>) -> Option<(u32, u32)> {
    let mut rect = RECT::default();
    // SAFETY: the caller's surface contract makes `hwnd` a live window of this
    // process; `GetClientRect` only writes `rect`.
    unsafe { GetClientRect(HWND(hwnd.as_ptr()), &raw mut rect) }.ok()?;
    let width = u32::try_from(rect.right - rect.left).ok()?;
    let height = u32::try_from(rect.bottom - rect.top).ok()?;
    Some((width, height))
}

/// A D3D11.4 device and its immediate context on the adapter whose LUID is
/// `luid`.
fn open_device(luid: [u8; 8]) -> Result<(ID3D11Device5, ID3D11DeviceContext4), HalError> {
    let luid = LUID {
        LowPart: u32::from_le_bytes([luid[0], luid[1], luid[2], luid[3]]),
        HighPart: i32::from_le_bytes([luid[4], luid[5], luid[6], luid[7]]),
    };
    // SAFETY: no flags; the factory is released at the end of this call.
    let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }
        .map_err(|error| failed("CreateDXGIFactory2", &error))?;
    // SAFETY: the factory is live; a LUID that names no adapter is an error.
    let adapter: IDXGIAdapter1 = unsafe { factory.EnumAdapterByLuid(luid) }
        .map_err(|error| failed("IDXGIFactory4::EnumAdapterByLuid", &error))?;
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    // SAFETY: an explicit adapter takes `D3D_DRIVER_TYPE_UNKNOWN` and no
    // software module, as documented; the out-pointers are live locals.
    unsafe {
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_1]),
            D3D11_SDK_VERSION,
            Some(&raw mut device),
            None,
            Some(&raw mut context),
        )
    }
    .map_err(|error| failed("D3D11CreateDevice", &error))?;
    let (Some(device), Some(context)) = (device, context) else {
        return Err(HalError::Backend(
            "D3D11CreateDevice succeeded and returned no device".to_string(),
        ));
    };
    // Shared fences need D3D11.4: `ID3D11Device5::CreateFence` and the
    // context's `Wait`/`Signal`.
    let device: ID3D11Device5 = device
        .cast()
        .map_err(|error| failed("ID3D11Device::QueryInterface(ID3D11Device5)", &error))?;
    let context: ID3D11DeviceContext4 = context.cast().map_err(|error| {
        failed(
            "ID3D11DeviceContext::QueryInterface(ID3D11DeviceContext4)",
            &error,
        )
    })?;
    Ok((device, context))
}

/// A fence created shared, and its NT handle.
fn shared_fence(device: &ID3D11Device5) -> Result<SharedFence, HalError> {
    let mut fence: Option<ID3D11Fence> = None;
    // SAFETY: the device is live; the out-pointer is a live local.
    unsafe { device.CreateFence(0, D3D11_FENCE_FLAG_SHARED, &raw mut fence) }
        .map_err(|error| failed("ID3D11Device5::CreateFence", &error))?;
    let Some(fence) = fence else {
        return Err(HalError::Backend(
            "ID3D11Device5::CreateFence succeeded and returned no fence".to_string(),
        ));
    };
    // SAFETY: the fence was created shared, which `CreateSharedHandle`
    // requires; no security attributes and no name.
    let handle = unsafe { fence.CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null()) }
        .map_err(|error| failed("ID3D11Fence::CreateSharedHandle", &error))?;
    Ok(SharedFence {
        handle: OwnedHandle(handle),
        fence,
    })
}

/// A render-target texture created shared through an NT handle, and the
/// handle.
fn shared_texture(
    device: &ID3D11Device5,
    extent: (u32, u32),
    format: DXGI_FORMAT,
) -> Result<SharedImage, HalError> {
    let texture = texture(
        device,
        extent,
        format,
        D3D11_USAGE_DEFAULT.0,
        D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0,
        0,
        D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 | D3D11_RESOURCE_MISC_SHARED.0,
    )?;
    let resource: IDXGIResource1 = texture
        .cast()
        .map_err(|error| failed("ID3D11Texture2D::QueryInterface(IDXGIResource1)", &error))?;
    // SAFETY: the texture was created with `SHARED_NTHANDLE`, which
    // `CreateSharedHandle` requires; no security attributes and no name.
    let handle = unsafe {
        resource.CreateSharedHandle(
            None,
            DXGI_SHARED_RESOURCE_READ.0 | DXGI_SHARED_RESOURCE_WRITE.0,
            PCWSTR::null(),
        )
    }
    .map_err(|error| failed("IDXGIResource1::CreateSharedHandle", &error))?;
    Ok(SharedImage {
        handle: OwnedHandle(handle),
        texture,
    })
}

/// A single-sampled, single-mip 2D texture.
fn texture(
    device: &ID3D11Device5,
    extent: (u32, u32),
    format: DXGI_FORMAT,
    usage: i32,
    bind: i32,
    cpu: i32,
    misc: i32,
) -> Result<ID3D11Texture2D, HalError> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: extent.0,
        Height: extent.1,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: windows::Win32::Graphics::Direct3D11::D3D11_USAGE(usage),
        BindFlags: bind.cast_unsigned(),
        CPUAccessFlags: cpu.cast_unsigned(),
        MiscFlags: misc.cast_unsigned(),
    };
    let mut texture: Option<ID3D11Texture2D> = None;
    // SAFETY: `desc` describes a valid 2D texture and there is no initial data;
    // the out-pointer is a live local.
    unsafe { device.CreateTexture2D(&raw const desc, None, Some(&raw mut texture)) }
        .map_err(|error| failed("ID3D11Device::CreateTexture2D", &error))?;
    texture.ok_or_else(|| {
        HalError::Backend("ID3D11Device::CreateTexture2D succeeded and returned no texture".into())
    })
}

/// Bytes per texel of a format in [`SHARED_FORMATS`]'s buffer formats.
fn bytes_per_texel(format: DXGI_FORMAT) -> usize {
    if format == DXGI_FORMAT_R16G16B16A16_FLOAT {
        8
    } else {
        4
    }
}
