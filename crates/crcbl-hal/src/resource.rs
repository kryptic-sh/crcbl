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
//! [`HalError::InvalidHandle`] rather than
//! aliasing whatever moved into the slot. See `crcbl-core`'s handle module for
//! the representation argument.
//!
//! This is the choice that keeps the seam object-safe — see the crate docs.

use crcbl_core::Handle;

use crate::{Format, HalError, Limits};

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
/// # An image is [`DeviceLocal`](Self::DeviceLocal), and cannot say otherwise
///
/// This location is not a field of [`ImageDesc`] at all. Every image the seam
/// creates is device-local, so there is nothing for a caller to pass and
/// nothing to get wrong — a stronger rule than the buffer one above, which
/// forbids a *combination*.
///
/// It is D3D12's rule again, and this time it is the heap rather than a flag on
/// it: `D3D12_HEAP_TYPE_UPLOAD` and `D3D12_HEAP_TYPE_READBACK` admit
/// `D3D12_RESOURCE_DIMENSION_BUFFER` and nothing else, so a texture on one is
/// not a slow texture but a resource `CreateCommittedResource` refuses. The
/// route to texel data there is a copy from a host-visible buffer, which is
/// what the seam's upload path already is on all four backends.
///
/// The other three had nothing to offer for the ask either, which is why the
/// field cost nothing to drop:
///
/// * `crcbl-wgpu` never read it. `wgpu::TextureDescriptor` has no member for
///   it, because WebGPU has no host-visible texture.
/// * `crcbl-vk` and `crcbl-mtl` did honour it, and the seam offers no call that
///   can observe the result. There is a
///   [`write_buffer`](crate::Device::write_buffer) and no `write_image`, no
///   image mapping and no subresource layout — and there could not be one for
///   Vulkan, because `vkGetImageSubresourceLayout` is defined only for
///   `VK_IMAGE_TILING_LINEAR` and this seam creates every image
///   `VK_IMAGE_TILING_OPTIMAL`. Both spent host-visible memory on an image
///   whose bytes no caller could reach.
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
    /// Size in bytes.
    ///
    /// **Must be non-zero**, and every backend answers
    /// [`HalError::InvalidDescriptor`] for a
    /// zero — said here because a rule with no named answer is one a backend
    /// can hold differently without being wrong, which is how `crcbl-webgpu`
    /// came to serve it while the other four refused it.
    /// `a_zero_size_buffer_is_refused_instead_of_served` in
    /// `crates/crcbl/tests/hal_seam_e2e.rs` holds the four native backends to
    /// it, and `a_zero_size_buffer_is_refused_without_encoding_anything` in
    /// `crcbl-webgpu`'s `hal::tests` holds the browser one, which that suite
    /// cannot reach.
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
///
/// There is no memory location here, unlike [`BufferDesc`]: an image is always
/// [`MemoryLocation::DeviceLocal`], and [`MemoryLocation`] says why.
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
    ///
    /// Above `1` this must be a [`ImageType::D2`] image with one mip
    /// level: see [`check`](Self::check), which refuses the rest.
    pub samples: u32,
    /// Permitted uses.
    pub usage: ImageUsage,
}

impl ImageDesc<'_> {
    /// Refuses a descriptor no device could make an image from.
    ///
    /// Every rule here is one a driver reports badly or not at all: an
    /// over-large extent surfaces as a creation failure naming no dimension, a
    /// sample count that is not a power of two reaches an API where samples are
    /// a *bitmask* and becomes a nonsense request rather than an error, and a
    /// mip count past the chain the extent can hold is undefined on some and
    /// clamped on others. So the seam states them, once, and a backend calls
    /// this rather than keeping its own copy — three of them still do, and
    /// `docs/backlog.md` carries that consolidation.
    ///
    /// The wording is `crcbl-hal`'s null backend's, which had the fullest set
    /// and is the reference every other backend is compared against; moving it
    /// here changed no message.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`], naming the field and the limit it
    /// passed.
    pub fn check(&self, limits: &Limits) -> Result<(), HalError> {
        let extent = self.extent;
        if extent.width == 0 || extent.height == 0 || extent.depth_or_layers == 0 {
            return Err(HalError::InvalidDescriptor(
                "image extent must be non-zero in every dimension".to_string(),
            ));
        }
        // [`Extent3d::height`] already says "1 for `ImageType::D1`", and until
        // now nothing held anyone to it. `crcbl-mtl` and `crcbl-dx12` each
        // refused it from their own copies of the rules while the three
        // backends that call this served it, so the seam stating it is what
        // makes the answer the same everywhere. It is the API's rule rather
        // than this seam's: `VUID-VkImageCreateInfo-imageType-00956`, and
        // WebGPU's `GPUTextureDescriptor` validation says the same.
        if self.image_type == ImageType::D1 && extent.height != 1 {
            return Err(HalError::InvalidDescriptor(format!(
                "a D1 image is {} texels high, and a 1D image has a height of 1",
                extent.height
            )));
        }
        // A 3D image is bounded by `max_image_3d` on **every** axis, including
        // its depth; a 1D/2D one by `max_image_2d` on width and height and by
        // `max_image_array_layers` on its layer count. Checking
        // `max(width, height)` against `max_image_2d` for both left
        // `max_image_3d` read by nothing and a volume's depth checked against
        // nothing at all.
        if self.image_type == ImageType::D3 {
            let longest = extent.width.max(extent.height).max(extent.depth_or_layers);
            if longest > limits.max_image_3d {
                return Err(HalError::InvalidDescriptor(format!(
                    "3D image extent {extent:?} exceeds max_image_3d {}",
                    limits.max_image_3d
                )));
            }
        } else {
            let longest = extent.width.max(extent.height);
            if longest > limits.max_image_2d {
                return Err(HalError::InvalidDescriptor(format!(
                    "image extent {longest} exceeds max_image_2d {}",
                    limits.max_image_2d
                )));
            }
            if extent.depth_or_layers > limits.max_image_array_layers {
                return Err(HalError::InvalidDescriptor(format!(
                    "{} array layers exceeds max_image_array_layers {}",
                    extent.depth_or_layers, limits.max_image_array_layers
                )));
            }
        }
        if self.mip_levels == 0 {
            return Err(HalError::InvalidDescriptor(
                "image must have at least one mip level".to_string(),
            ));
        }
        let full_chain = extent.full_mip_levels(self.image_type);
        if self.mip_levels > full_chain {
            return Err(HalError::InvalidDescriptor(format!(
                "{} mip levels exceeds the {full_chain} a {extent:?} {:?} image can have",
                self.mip_levels, self.image_type
            )));
        }
        // A sample count is a bit in a mask on every API underneath, so `3`
        // is not "three samples" — it is two bits set, which reaches a driver
        // as a nonsense request rather than an error.
        if !self.samples.is_power_of_two() || self.samples > limits.max_sample_count {
            return Err(HalError::InvalidDescriptor(format!(
                "{} samples is not a power of two in 1..={}",
                self.samples, limits.max_sample_count
            )));
        }
        // A multisampled image is two-dimensional and has one mip on every API
        // underneath — `VUID-VkImageCreateInfo-samples-02257` states both, and
        // Metal and D3D12 refuse both from their own copies of the rules. The
        // seam was silent, so the three backends that call this accepted a
        // descriptor two backends rejected, and a driver was handed a request
        // it has no answer for. There is nothing to resolve a second mip of a
        // multisampled image *from*: the resolve is what produces the
        // single-sampled texels a chain would be built out of.
        if self.samples > 1 {
            if self.image_type != ImageType::D2 {
                return Err(HalError::InvalidDescriptor(format!(
                    "a {:?} image asks for {} samples, and only a D2 image is multisampled",
                    self.image_type, self.samples
                )));
            }
            if self.mip_levels != 1 {
                return Err(HalError::InvalidDescriptor(format!(
                    "a {}-sample image asks for {} mip levels, and a multisampled image has one",
                    self.samples, self.mip_levels
                )));
            }
        }
        if self.usage.is_empty() {
            return Err(HalError::InvalidDescriptor(
                "an image with no usage flags can never be used".to_string(),
            ));
        }
        Ok(())
    }
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
    ///
    /// **Must be in range of the image**, which [`check`](Self::check) is what
    /// says: a base past the last mip or layer, a count of zero, and a count
    /// running past the end are each refused there rather than handed to a
    /// driver that has no answer for them.
    pub range: ImageSubresourceRange,
}

impl ImageViewDesc<'_> {
    /// Refuses a view of subresources the image does not have.
    ///
    /// The rule [`Device::create_image_view`](crate::Device::create_image_view)
    /// already promised — "an out-of-range subresource" is
    /// [`HalError::InvalidDescriptor`] — and which nothing on the seam stated,
    /// so each backend that wanted it kept its own copy and the three that did
    /// not kept none. `crcbl-vk` put the range straight into
    /// `vkCreateImageView`, which is
    /// `VUID-VkImageViewCreateInfo-subresourceRange-01478`: a driver answers
    /// `VK_SUCCESS` and the view addresses mips that were never allocated.
    ///
    /// The two counts are taken as parameters rather than read off an
    /// [`ImageDesc`] because the image's descriptor is gone by the time a view
    /// is made — every backend keeps the shape it needs in its own image
    /// table, and this is the shape it needs.
    ///
    /// **`image_layers` is not [`Extent3d::depth_or_layers`].** That field is
    /// the depth for an [`ImageType::D3`] image and the array-layer count for
    /// every other type, so a volume has **one** array layer no matter how deep
    /// it is. A caller passing a 3D image's depth here would accept views of
    /// layers that do not exist; passing `1` for an array image would refuse
    /// every view past the first layer.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`], naming the range and the shape it ran
    /// past.
    pub fn check(&self, image_mip_levels: u32, image_layers: u32) -> Result<(), HalError> {
        let range = self.range;
        if range.base_mip >= image_mip_levels || range.base_layer >= image_layers {
            return Err(HalError::InvalidDescriptor(format!(
                "view starts at mip {} layer {}, and the image has {image_mip_levels} mips and \
                 {image_layers} layers",
                range.base_mip, range.base_layer
            )));
        }
        // Resolved before anything compares a count, because
        // [`ImageSubresourceRange::ALL`] is `u32::MAX` — a sentinel, not a
        // count. Compared raw it makes the whole-image range that
        // [`ImageSubresourceRange::all`] mints, which is nearly every view this
        // engine creates, the widest overrun expressible.
        let mip_count = resolve_count(range.mip_count, range.base_mip, image_mip_levels);
        let layer_count = resolve_count(range.layer_count, range.base_layer, image_layers);
        if mip_count == 0 || layer_count == 0 {
            return Err(HalError::InvalidDescriptor(
                "an image view covering no mip levels or no layers is not a view".to_string(),
            ));
        }
        // Checked, not wrapping: a `base_mip` near `u32::MAX` with a count of
        // three sums to a *small* number, which passes the comparison it was
        // supposed to fail. The base check above rejects that particular pair,
        // but a legal base with a near-`u32::MAX` count reaches here.
        if range
            .base_mip
            .checked_add(mip_count)
            .is_none_or(|end| end > image_mip_levels)
        {
            return Err(HalError::InvalidDescriptor(format!(
                "a view of {mip_count} mips from mip {} runs past the {image_mip_levels} mips \
                 the image has",
                range.base_mip
            )));
        }
        if range
            .base_layer
            .checked_add(layer_count)
            .is_none_or(|end| end > image_layers)
        {
            return Err(HalError::InvalidDescriptor(format!(
                "a view of {layer_count} layers from layer {} runs past the {image_layers} \
                 layers the image has",
                range.base_layer
            )));
        }
        Ok(())
    }
}

/// Turns [`ImageSubresourceRange::ALL`] into the count it stands for.
///
/// Only the sentinel is rewritten: a count the caller actually named is left
/// alone so [`ImageViewDesc::check`] can refuse it for running past the end,
/// rather than quietly serving a narrower view than the one asked for.
///
/// `base` is already known to be inside `total` at every call site, so the
/// subtraction is saturating only to keep the function total.
const fn resolve_count(requested: u32, base: u32, total: u32) -> u32 {
    if requested == ImageSubresourceRange::ALL {
        total.saturating_sub(base)
    } else {
        requested
    }
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

    /// **The two shape rules the seam owed and nobody enforced.**
    ///
    /// [`Extent3d::height`] has always said "1 for `ImageType::D1`", and
    /// nothing held a caller to it. The multisample rules were not stated at
    /// all, yet `crcbl-mtl` and `crcbl-dx12` refused both from their own copies
    /// while `crcbl-vk`, the null backend and `crcbl-webgpu` — the three that
    /// call [`check`](ImageDesc::check) — served them. Two backends refusing
    /// what three accept is the divergence; the seam saying which is the fix.
    ///
    /// Both are the API's rules rather than this seam's:
    /// `VUID-VkImageCreateInfo-imageType-00956` for the first and
    /// `VUID-VkImageCreateInfo-samples-02257` for the other two.
    ///
    /// **What turns it red.** Dropping the D1 rule — the first refusal.
    /// Dropping the `ImageType` half of the multisample rule — the second.
    /// Dropping the mip half — the third. And the accepting arms are what stop
    /// the refusals going vacuous: a `check` that refused every image would
    /// satisfy all three `expect_err`s and nothing else here would notice.
    #[test]
    fn a_one_dimensional_or_multisampled_image_is_held_to_its_shape() {
        let limits = Limits::minimum();
        let image = |image_type, extent, mip_levels, samples| ImageDesc {
            label: None,
            image_type,
            extent,
            format: Format::Rgba8Unorm,
            mip_levels,
            samples,
            usage: ImageUsage::SAMPLED,
        };
        let line = Extent3d {
            width: 64,
            height: 1,
            depth_or_layers: 1,
        };
        let square = Extent3d::d2(64, 64);

        image(ImageType::D1, line, 1, 1)
            .check(&limits)
            .expect("a 1D image one texel high is the shape the field describes");
        image(ImageType::D2, square, 1, 4)
            .check(&limits)
            .expect("a four-sample 2D image with one mip is what MSAA is");
        image(ImageType::D2, square, 7, 1)
            .check(&limits)
            .expect("a single-sampled image may carry its whole chain");

        for (desc, what) in [
            (
                image(ImageType::D1, square, 1, 1),
                "a 1D image 64 texels high",
            ),
            (
                image(ImageType::D3, square, 1, 4),
                "a multisampled 3D image",
            ),
            (
                image(ImageType::D2, square, 7, 4),
                "a multisampled image with a mip chain",
            ),
        ] {
            let error = desc
                .check(&limits)
                .expect_err(&format!("{what} must be refused"));
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }
    }

    /// **A view may only name subresources the image has.**
    ///
    /// [`Device::create_image_view`](crate::Device::create_image_view) has
    /// always documented an out-of-range subresource as
    /// [`HalError::InvalidDescriptor`], and the seam provided no check for it.
    /// `crcbl-mtl` and `crcbl-dx12` each enforced it from their own copies;
    /// `crcbl-vk`, the null backend and `crcbl-webgpu` enforced nothing, and
    /// the Vulkan one put the range straight into `vkCreateImageView` — a
    /// driver returns `VK_SUCCESS` and the view addresses mips that were never
    /// allocated (`VUID-VkImageViewCreateInfo-subresourceRange-01478`).
    ///
    /// **What turns it red.** Dropping the base check — the two message
    /// assertions, which are what that branch is for; the cases themselves
    /// would still be refused, by the zero-count rule a resolved `ALL` reaches
    /// and by the past-the-end rule an explicit count reaches. Dropping the
    /// zero-count check — its two cases. Dropping either past-the-end check —
    /// its own case. Reading
    /// [`ImageSubresourceRange::ALL`] as a literal count instead of resolving
    /// it — the whole-image `expect`, which is the range nearly every view in
    /// this engine is built from. And `checked_add` becoming a `+` — the
    /// overflow case, whose sum wraps to `0` and passes a comparison against
    /// four mips.
    ///
    /// The accepting arms are what stop the refusals going vacuous: a `check`
    /// that refused every view would satisfy every `expect_err` here and
    /// nothing else would notice.
    #[test]
    fn a_view_may_only_name_subresources_its_image_has() {
        // Four mips, three array layers — no two of the image's numbers are
        // equal, and none equals a base or count used below, so a body that
        // compared mips against layers would not pass by coincidence.
        const MIPS: u32 = 4;
        const LAYERS: u32 = 3;
        let view = |base_mip, mip_count, base_layer, layer_count| ImageViewDesc {
            label: None,
            image: Handle::from_bits(1 << 32).expect("a non-zero generation"),
            view_type: ImageViewType::D2Array,
            format: Format::Rgba8Unorm,
            range: ImageSubresourceRange {
                aspect: ImageAspect::COLOR,
                base_mip,
                mip_count,
                base_layer,
                layer_count,
            },
        };
        let all = ImageSubresourceRange::ALL;

        view(0, all, 0, all)
            .check(MIPS, LAYERS)
            .expect("both sentinels resolve to the whole image, which is a view of all of it");
        view(3, all, 2, all)
            .check(MIPS, LAYERS)
            .expect("the sentinel from the last mip and last layer is one of each, not none");
        view(0, MIPS, 0, LAYERS)
            .check(MIPS, LAYERS)
            .expect("the whole image named explicitly is the same view");
        view(1, 2, 1, 2)
            .check(MIPS, LAYERS)
            .expect("a subrange ending exactly at the end is inside the image");

        // A base past the end is asserted on its *message*, because the two
        // rules below already cover the case: a resolved `ALL` becomes a zero
        // count and an explicit count runs past the end, so a kind-only
        // assertion here is one no sabotage of the base check could turn red.
        // The sentence is what the check is for — `crcbl-mtl` and `crcbl-dx12`
        // have answered a base past the end in exactly these words from their
        // own copies since before the seam had any, and a caller must read the
        // same refusal from all five backends.
        for (desc, what) in [
            (view(MIPS, all, 0, all), "a view starting past the last mip"),
            (
                view(0, all, LAYERS, all),
                "a view starting past the last layer",
            ),
        ] {
            let error = desc
                .check(MIPS, LAYERS)
                .expect_err(&format!("{what} must be refused"));
            let HalError::InvalidDescriptor(message) = &error else {
                panic!("{what} is a bad descriptor, not {error:?}");
            };
            assert!(
                message.starts_with("view starts at mip"),
                "{what} must be refused for its base rather than for what its \
                 count resolved to: {message}"
            );
        }

        for (desc, what) in [
            (view(0, 0, 0, all), "a view of no mip levels"),
            (view(0, all, 0, 0), "a view of no layers"),
            (view(2, 3, 0, all), "a mip range running past the last mip"),
            (
                view(0, all, 1, 3),
                "a layer range running past the last layer",
            ),
            (
                view(2, u32::MAX - 1, 0, all),
                "a mip count whose sum with its base overflows a u32",
            ),
            (
                view(0, all, 2, u32::MAX - 1),
                "a layer count whose sum with its base overflows a u32",
            ),
        ] {
            let error = desc
                .check(MIPS, LAYERS)
                .expect_err(&format!("{what} must be refused"));
            assert!(
                matches!(error, HalError::InvalidDescriptor(_)),
                "{what}: {error:?}"
            );
        }
    }

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
