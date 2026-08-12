//! Buffers, images, image views, samplers — and the handle scheme all HAL
//! objects use.
//!
//! # Marker types and handles
//!
//! Each resource kind gets an **uninhabited marker type** ([`Buffer`],
//! [`Image`], …) that exists only as the type parameter of a
//! [`Handle`]. The marker cannot be constructed, so a
//! `BufferHandle` can never be confused with the thing it names, and
//! `Handle<Buffer>` vs `Handle<Image>` is a compile error rather than a runtime
//! surprise.
//!
//! Handles are 8 bytes, `Copy`, `Hash`, and generational: destroying a resource
//! invalidates every outstanding handle to it, and a stale one produces
//! [`HalError::InvalidHandle`](crate::HalError::InvalidHandle) rather than
//! aliasing whatever moved into the slot. See `crcbl-core`'s handle module for
//! the representation argument.
//!
//! This is the choice that keeps the seam object-safe — see the crate docs.

use crcbl_core::Handle;

use crate::Format;

/// Marker type for buffer handles. Uninhabited; only ever a type parameter.
#[derive(Debug)]
pub enum Buffer {}
/// Marker type for image handles. Uninhabited; only ever a type parameter.
#[derive(Debug)]
pub enum Image {}
/// Marker type for image-view handles. Uninhabited; only ever a type parameter.
#[derive(Debug)]
pub enum ImageView {}
/// Marker type for sampler handles. Uninhabited; only ever a type parameter.
#[derive(Debug)]
pub enum Sampler {}

/// A GPU buffer.
pub type BufferHandle = Handle<Buffer>;
/// A GPU image.
pub type ImageHandle = Handle<Image>;
/// A view onto a subrange of an [`Image`].
pub type ImageViewHandle = Handle<ImageView>;
/// A sampler state object.
pub type SamplerHandle = Handle<Sampler>;

bitflags::bitflags! {
    /// How a buffer will be used.
    ///
    /// Declared up front because Vulkan and DX12 both need it at creation time,
    /// and because a backend that knows a buffer is never an indirect argument
    /// can place it more cheaply.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct BufferUsage: u32 {
        /// Source of a copy.
        const TRANSFER_SRC = 1 << 0;
        /// Destination of a copy or a
        /// [`fill_buffer`](crate::CommandEncoder::fill_buffer).
        const TRANSFER_DST = 1 << 1;
        /// Bound as a uniform buffer.
        const UNIFORM = 1 << 2;
        /// Bound as a storage buffer. The engine's default: vertex pulling,
        /// instance arrays, material tables and the visible-instance list are
        /// all storage buffers.
        ///
        /// A storage buffer a **shader writes** must live in
        /// [`MemoryLocation::DeviceLocal`] — see that type for why. The
        /// read-only ones named above are free to be host-visible, and are.
        const STORAGE = 1 << 3;
        /// Bound with [`bind_index_buffer`](crate::CommandEncoder::bind_index_buffer).
        const INDEX = 1 << 4;
        /// Holds indirect draw/dispatch arguments or an indirect count.
        ///
        /// Not optional in this engine: topic 03's steady state is a compute
        /// pass writing draw arguments that the same frame consumes.
        const INDIRECT = 1 << 5;
        /// Its GPU address may be taken. Requires
        /// [`Features::BUFFER_DEVICE_ADDRESS`](crate::Features::BUFFER_DEVICE_ADDRESS);
        /// A backend without it never sets this.
        const DEVICE_ADDRESS = 1 << 6;
        /// Destination of a query resolve.
        const QUERY_RESOLVE = 1 << 7;
    }
}

bitflags::bitflags! {
    /// How an image will be used.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ImageUsage: u32 {
        /// Source of a copy or blit.
        const TRANSFER_SRC = 1 << 0;
        /// Destination of a copy or clear.
        const TRANSFER_DST = 1 << 1;
        /// Sampled in a shader.
        const SAMPLED = 1 << 2;
        /// Read/written as a storage image (compute output, depth pyramid).
        const STORAGE = 1 << 3;
        /// Usable as a colour attachment.
        const COLOR_ATTACHMENT = 1 << 4;
        /// Usable as a depth/stencil attachment.
        const DEPTH_STENCIL_ATTACHMENT = 1 << 5;
        /// Owned by a swapchain and eventually presented.
        const PRESENT = 1 << 6;
    }
}

/// Where a resource's memory lives, and who can touch it.
///
/// Three locations rather than a full heap-property matrix: these are the three
/// `crcbl-vk` wraps `gpu-allocator` around (`docs/plan/02-vulkan-backend.md`
/// §2.1), and they are the three that map onto Metal's `private`/`shared` and
/// DX12's `DEFAULT`/`UPLOAD`/`READBACK` without invention.
///
/// # A buffer a shader writes must be [`DeviceLocal`](Self::DeviceLocal)
///
/// Filling a writable storage binding — a
/// [`BindingKind::StorageBuffer`](crate::BindingKind::StorageBuffer) with
/// `read_only: false` — with a buffer in either host-visible location is a seam
/// violation, refused by
/// [`create_bind_group`](crate::Device::create_bind_group). **Read-only
/// bindings of a host-visible buffer are unaffected**, which is how every
/// uniform block and every staged table in the engine is bound.
///
/// The rule is D3D12's. Its `UPLOAD` and `READBACK` heaps reject
/// `D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS` at resource creation, and they
/// pin the resource to `GENERIC_READ` and `COPY_DEST` respectively for its whole
/// lifetime — so there is neither a view a shader could write through nor a
/// state it could write from. `CreateUnorderedAccessView` returns `void`: it
/// writes no descriptor, reports nothing, and the device is removed at the next
/// call.
///
/// Vulkan and Metal both permit the combination, and on unified memory it can be
/// a genuine optimisation. The seam gives that up deliberately. A capability
/// three backends have and one does not is exactly what the seam exists to keep
/// out of call sites, and this one does not degrade on the fourth backend — it
/// removes the device, somewhere only a WARP run ever looks. If host-visible
/// shader writes are ever wanted they arrive as a
/// [`Features`](crate::Features) flag with a documented fallback, not as a
/// silent per-backend difference.
///
/// # An image is always [`DeviceLocal`](Self::DeviceLocal)
///
/// Not "almost always": every other value is refused by
/// [`create_image`](crate::Device::create_image), so
/// [`ImageDesc::memory`](crate::ImageDesc::memory) has one legal value. This is
/// a stronger rule than the buffer one above, which forbids a *combination*.
///
/// It is D3D12's rule again, and this time it is the heap rather than a flag on
/// it: `D3D12_HEAP_TYPE_UPLOAD` and `D3D12_HEAP_TYPE_READBACK` admit
/// `D3D12_RESOURCE_DIMENSION_BUFFER` and nothing else, so a texture on one is
/// not a slow texture but a resource `CreateCommittedResource` refuses. The
/// route to texel data there is a copy from a host-visible buffer, which is
/// what the seam's upload path already is on all four backends.
///
/// The other three give the value nothing to buy, which is why giving it up
/// costs nothing:
///
/// * `crcbl-wgpu` never reads the field. `wgpu::TextureDescriptor` has no
///   member for it, because WebGPU has no host-visible texture — so a
///   host-visible image there is silently an ordinary device-local one.
/// * `crcbl-vk` and `crcbl-mtl` do honour it, and the seam then offers no call
///   that can observe the result. There is a
///   [`write_buffer`](crate::Device::write_buffer) and no `write_image`, no
///   image mapping and no subresource layout — and there could not be one for
///   Vulkan, because `vkGetImageSubresourceLayout` is defined only for
///   `VK_IMAGE_TILING_LINEAR` and this seam creates every image
///   `VK_IMAGE_TILING_OPTIMAL`. Both backends therefore spend host-visible
///   memory on an image whose bytes no caller can reach.
///
/// So the honest reading is that the host-visible locations were never an image
/// feature this seam had: one backend cannot create it, one ignores the ask,
/// and the two that comply produce something unreachable. What they do reliably
/// is remove a D3D12 device on the run nobody watches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemoryLocation {
    /// GPU-local, not CPU-mappable. Everything the GPU reads in a hot loop.
    DeviceLocal,
    /// CPU-writable, GPU-readable — the staging-ring and per-frame-constants
    /// location. Writes are sequential-only: treat mapped memory as
    /// write-combined and never read it back.
    HostUpload,
    /// GPU-writable, CPU-readable and cached. Only for the delayed debug
    /// readback ring (topic 03 §3.5) — the one readback the frame loop permits,
    /// N frames latent, debug builds only.
    HostReadback,
}

impl MemoryLocation {
    /// Whether the CPU can map this memory.
    #[must_use]
    pub const fn is_mappable(self) -> bool {
        matches!(self, Self::HostUpload | Self::HostReadback)
    }
}

/// Creation parameters for a buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferDesc<'a> {
    /// Debug name, surfaced to RenderDoc/PIX/Xcode when
    /// [`Features::DEBUG_MARKERS`](crate::Features::DEBUG_MARKERS) is present.
    ///
    /// Names are set at creation rather than through a separate `set_name` call
    /// so there is no type-erased "any resource" parameter anywhere in the
    /// seam, and so an unnamed object is a visibly missing field.
    pub label: Option<&'a str>,
    /// Size in bytes. Must be non-zero.
    pub size: u64,
    /// Permitted uses.
    pub usage: BufferUsage,
    /// Where the memory lives.
    pub memory: MemoryLocation,
}

/// Image dimensionality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageType {
    /// One-dimensional (LUTs).
    D1,
    /// Two-dimensional. Array layers are still allowed.
    D2,
    /// Three-dimensional (volume textures, froxel grids).
    D3,
}

/// A 3D size in texels. `depth_or_layers` is the depth for
/// [`ImageType::D3`] and the array-layer count otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Extent3d {
    /// Width in texels.
    pub width: u32,
    /// Height in texels; `1` for [`ImageType::D1`].
    pub height: u32,
    /// Depth for 3D images, array layers otherwise.
    pub depth_or_layers: u32,
}

impl Extent3d {
    /// A 2D extent with one layer.
    #[must_use]
    pub const fn d2(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            depth_or_layers: 1,
        }
    }

    /// Number of mip levels in a full chain for this extent.
    ///
    /// `floor(log2(longest_dimension)) + 1`, clamped to at least 1. The mip
    /// count every "generate the whole chain" call site would otherwise
    /// recompute slightly differently.
    ///
    /// `image_type` is required because [`depth_or_layers`](Self::depth_or_layers)
    /// means two different things: array layers, which do **not** mip, and a 3D
    /// depth, which does. A `4x4x64` volume has a seven-level chain, not a
    /// three-level one, and a caller that allocated three under-allocates by
    /// four levels.
    #[must_use]
    pub const fn full_mip_levels(self, image_type: ImageType) -> u32 {
        let mut longest = if self.width > self.height {
            self.width
        } else {
            self.height
        };
        if matches!(image_type, ImageType::D3) && self.depth_or_layers > longest {
            longest = self.depth_or_layers;
        }
        if longest == 0 {
            1
        } else {
            u32::BITS - longest.leading_zeros()
        }
    }
}

/// A 3D offset in texels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Offset3d {
    /// X offset.
    pub x: i32,
    /// Y offset.
    pub y: i32,
    /// Z offset or array layer.
    pub z: i32,
}

/// An axis-aligned rectangle in pixels — scissor rects and render areas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect2d {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Rect2d {
    /// A rectangle at the origin with the given size.
    #[must_use]
    pub const fn from_size(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
}

bitflags::bitflags! {
    /// Which planes of an image a view or copy touches.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ImageAspect: u32 {
        /// Colour plane.
        const COLOR = 1 << 0;
        /// Depth plane.
        const DEPTH = 1 << 1;
        /// Stencil plane.
        const STENCIL = 1 << 2;
    }
}

impl ImageAspect {
    /// The aspects a format actually has.
    ///
    /// Saves every call site from re-deriving "depth format ⇒ DEPTH, plus
    /// STENCIL if it has one", which is exactly the derivation that goes wrong
    /// once and then produces a validation error at a barrier three passes
    /// later.
    #[must_use]
    pub const fn of(format: Format) -> Self {
        if format.is_depth_stencil() {
            let mut aspect = Self::empty();
            if format.has_depth() {
                aspect = aspect.union(Self::DEPTH);
            }
            if format.has_stencil() {
                aspect = aspect.union(Self::STENCIL);
            }
            aspect
        } else {
            Self::COLOR
        }
    }
}

/// A mip/layer subrange of an image, as barriers and views name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageSubresourceRange {
    /// Planes covered.
    pub aspect: ImageAspect,
    /// First mip level.
    pub base_mip: u32,
    /// Mip levels covered. [`u32::MAX`] means "all remaining".
    pub mip_count: u32,
    /// First array layer.
    pub base_layer: u32,
    /// Array layers covered. [`u32::MAX`] means "all remaining".
    pub layer_count: u32,
}

impl ImageSubresourceRange {
    /// Sentinel meaning "every remaining mip level" / "every remaining layer".
    pub const ALL: u32 = u32::MAX;

    /// The whole image, for a format's natural aspects.
    #[must_use]
    pub const fn all(format: Format) -> Self {
        Self {
            aspect: ImageAspect::of(format),
            base_mip: 0,
            mip_count: Self::ALL,
            base_layer: 0,
            layer_count: Self::ALL,
        }
    }
}

/// A single mip level across a layer range — what a copy addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageSubresourceLayers {
    /// Planes covered. A copy must name exactly one plane.
    pub aspect: ImageAspect,
    /// Mip level.
    pub mip: u32,
    /// First array layer.
    pub base_layer: u32,
    /// Array layers covered.
    pub layer_count: u32,
}

/// Creation parameters for an image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageDesc<'a> {
    /// Debug name; see [`BufferDesc::label`].
    pub label: Option<&'a str>,
    /// Dimensionality.
    pub image_type: ImageType,
    /// Size, and depth or array-layer count.
    pub extent: Extent3d,
    /// Texel format.
    pub format: Format,
    /// Mip levels. Use [`Extent3d::full_mip_levels`] for a full chain.
    pub mip_levels: u32,
    /// Samples per texel; `1` for everything except MSAA targets.
    pub samples: u32,
    /// Permitted uses.
    pub usage: ImageUsage,
    /// Where the memory lives. **Always [`MemoryLocation::DeviceLocal`]** —
    /// [`create_image`](crate::Device::create_image) refuses either
    /// host-visible location, because D3D12's upload and readback heaps hold
    /// buffers only and a texture on one is a resource that cannot be created.
    /// [`MemoryLocation`] states the rule and what the other three backends do
    /// with the ask; the short version is that none of them can be relied on
    /// for it and the seam has no call that could read the result anyway.
    ///
    /// The field is still here rather than gone because removing it is an API
    /// break across every image the engine allocates, not because a second
    /// value is coming.
    pub memory: MemoryLocation,
}

/// How a view reinterprets its image's dimensionality.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageViewType {
    /// 1D.
    D1,
    /// 2D, single layer.
    D2,
    /// 2D array.
    D2Array,
    /// Cube map — six layers.
    Cube,
    /// Cube-map array.
    CubeArray,
    /// 3D.
    D3,
}

/// Creation parameters for an image view.
///
/// Views exist as their own object because every backend has them and because
/// the engine needs subrange views for real reasons: one cascade of a shadow
/// atlas, one mip of a depth pyramid, one face of a cube map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageViewDesc<'a> {
    /// Debug name; see [`BufferDesc::label`].
    pub label: Option<&'a str>,
    /// Image being viewed.
    pub image: ImageHandle,
    /// Dimensionality of the view.
    pub view_type: ImageViewType,
    /// Format seen through the view — may differ from the image's format for
    /// sRGB reinterpretation, and must be compatible with it.
    pub format: Format,
    /// Subrange covered.
    pub range: ImageSubresourceRange,
}

/// Minification/magnification/mip filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FilterMode {
    /// Point sampling.
    Nearest,
    /// Bilinear/trilinear.
    Linear,
}

/// Behaviour outside `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SamplerAddressMode {
    /// Wrap around.
    Repeat,
    /// Wrap around, mirrored.
    MirrorRepeat,
    /// Clamp to the edge texel.
    ClampToEdge,
    /// Clamp to a border colour of transparent black.
    ClampToBorder,
}

/// Creation parameters for a sampler.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SamplerDesc<'a> {
    /// Debug name; see [`BufferDesc::label`].
    pub label: Option<&'a str>,
    /// Filter when magnifying.
    pub mag_filter: FilterMode,
    /// Filter when minifying.
    pub min_filter: FilterMode,
    /// Filter between mip levels.
    pub mip_filter: FilterMode,
    /// Addressing on U, V and W.
    pub address_mode: [SamplerAddressMode; 3],
    /// Lowest mip level sampled.
    pub lod_min: f32,
    /// Highest mip level sampled.
    pub lod_max: f32,
    /// Anisotropy; `1.0` disables. Capped by
    /// [`Limits::max_sampler_anisotropy`](crate::Limits::max_sampler_anisotropy).
    pub anisotropy: f32,
    /// When set, the sampler compares against a reference value instead of
    /// returning texels — hardware PCF for shadow maps.
    ///
    /// **Reversed-Z applies here too.** With depth 1.0 at the near plane, a
    /// shadow test asking "is the fragment closer than the stored caster?" is
    /// [`CompareOp::Greater`](crate::CompareOp), not `Less`. See [`crate::depth`].
    pub compare: Option<crate::CompareOp>,
}

impl Default for SamplerDesc<'_> {
    /// Trilinear, repeating, no anisotropy, no comparison.
    fn default() -> Self {
        Self {
            label: None,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mip_filter: FilterMode::Linear,
            address_mode: [SamplerAddressMode::Repeat; 3],
            lod_min: 0.0,
            lod_max: f32::MAX,
            anisotropy: 1.0,
            compare: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_chain_length_matches_the_textbook_formula() {
        for (width, height, expected) in [
            (1u32, 1u32, 1u32),
            (2, 1, 2),
            (2, 2, 2),
            (4, 4, 3),
            (5, 5, 3),
            (256, 256, 9),
            (1024, 512, 11),
            (4096, 4096, 13),
        ] {
            assert_eq!(
                Extent3d::d2(width, height).full_mip_levels(ImageType::D2),
                expected,
                "{width}x{height}"
            );
        }
        // Degenerate input must not produce a zero-length chain.
        assert_eq!(Extent3d::d2(0, 0).full_mip_levels(ImageType::D2), 1);
    }

    /// `depth_or_layers` mips for a volume and does not for an array — the two
    /// readings of one field, and the reason the image type is a parameter.
    #[test]
    fn a_volumes_depth_joins_the_mip_chain_but_array_layers_do_not() {
        let extent = Extent3d {
            width: 4,
            height: 4,
            depth_or_layers: 64,
        };
        assert_eq!(extent.full_mip_levels(ImageType::D3), 7, "4x4x64 volume");
        assert_eq!(
            extent.full_mip_levels(ImageType::D2),
            3,
            "64 array layers do not mip"
        );
        assert_eq!(extent.full_mip_levels(ImageType::D1), 3);
    }

    #[test]
    fn aspects_follow_the_format() {
        assert_eq!(ImageAspect::of(Format::Rgba16Float), ImageAspect::COLOR);
        assert_eq!(ImageAspect::of(Format::D32Float), ImageAspect::DEPTH);
        assert_eq!(
            ImageAspect::of(Format::D32FloatS8Uint),
            ImageAspect::DEPTH | ImageAspect::STENCIL
        );
        assert!(!ImageAspect::of(Format::D32Float).contains(ImageAspect::COLOR));
    }

    #[test]
    fn whole_image_range_uses_the_sentinel() {
        let range = ImageSubresourceRange::all(Format::D32Float);
        assert_eq!(range.aspect, ImageAspect::DEPTH);
        assert_eq!(range.mip_count, ImageSubresourceRange::ALL);
        assert_eq!(range.layer_count, ImageSubresourceRange::ALL);
        assert_eq!(range.base_mip, 0);
    }

    #[test]
    fn only_the_host_visible_memory_locations_report_themselves_as_mappable() {
        assert!(!MemoryLocation::DeviceLocal.is_mappable());
        assert!(MemoryLocation::HostUpload.is_mappable());
        assert!(MemoryLocation::HostReadback.is_mappable());
    }

    #[test]
    fn default_sampler_is_trilinear_repeat_without_comparison() {
        let sampler = SamplerDesc::default();
        assert_eq!(sampler.mip_filter, FilterMode::Linear);
        assert_eq!(sampler.address_mode, [SamplerAddressMode::Repeat; 3]);
        assert_eq!(sampler.anisotropy, 1.0);
        assert!(
            sampler.compare.is_none(),
            "a comparison sampler must be asked for explicitly; a silent default \
             would be wrong under reversed-Z in one direction or the other"
        );
    }

    #[test]
    fn rect_from_size_is_at_the_origin() {
        let rect = Rect2d::from_size(1920, 1080);
        assert_eq!((rect.x, rect.y), (0, 0));
        assert_eq!((rect.width, rect.height), (1920, 1080));
    }
}
