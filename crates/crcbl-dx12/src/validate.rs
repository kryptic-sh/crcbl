//! The descriptor checks every image and image view is validated against
//! before anything is created.
//!
//! # Why this module exists
//!
//! [`Device::create_image`](crcbl_hal::Device::create_image) and
//! [`Device::create_image_view`](crcbl_hal::Device::create_image_view) are the
//! only places a caller's descriptor meets D3D12's rules, and the split out of
//! `device.rs` is so the rules read as one list rather than as a preamble to a
//! resource descriptor.
//!
//! # Refusing is the only diagnosis a caller will get
//!
//! The four `Create*View` calls return `void`: an invalid descriptor is a
//! debug-layer message and a slot full of nothing, with no error anywhere.
//! [`check_image`] and [`check_view_type`] turn what would be that into an
//! `Err` at the call that caused it, and [`build_views`] builds every
//! descriptor *before* any heap slot is taken, so a combination D3D12 cannot
//! express costs nothing and leaks nothing.

use crcbl_hal::{
    DeviceCaps, Features, Format, HalError, ImageDesc, ImageType, ImageUsage, ImageViewDesc,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_DEPTH_STENCIL_VIEW_DESC, D3D12_RENDER_TARGET_VIEW_DESC, D3D12_SHADER_RESOURCE_VIEW_DESC,
    D3D12_UNORDERED_ACCESS_VIEW_DESC,
};

use crate::view::Subresource;
use crate::{conv, view};

/// Checks an [`ImageDesc`] against this device's limits and D3D12's own rules,
/// before anything is created.
///
/// Split out of `Device::create_image` so the descriptor rules read as one
/// list rather than as a preamble to a resource descriptor.
pub(crate) fn check_image(caps: &DeviceCaps, desc: &ImageDesc<'_>) -> Result<(), HalError> {
    let limits = caps.limits;
    let extent = desc.extent;
    if extent.width == 0 || extent.height == 0 || extent.depth_or_layers == 0 {
        return Err(HalError::InvalidDescriptor(format!(
            "ImageDesc::extent has a zero dimension: {extent:?}"
        )));
    }
    if desc.usage.is_empty() {
        return Err(HalError::InvalidDescriptor(
            "ImageDesc::usage is empty, so the image could never be used".to_string(),
        ));
    }
    if !desc.samples.is_power_of_two() || desc.samples > limits.max_sample_count {
        return Err(HalError::InvalidDescriptor(format!(
            "ImageDesc::samples is {} but this device supports powers of two up to {}",
            desc.samples, limits.max_sample_count
        )));
    }
    if desc.samples > 1 && desc.mip_levels > 1 {
        return Err(HalError::InvalidDescriptor(format!(
            "a multisampled image has one mip level, not {}",
            desc.mip_levels
        )));
    }
    if matches!(desc.image_type, ImageType::D1) && extent.height != 1 {
        return Err(HalError::InvalidDescriptor(format!(
            "ImageType::D1 requires a height of 1, not {}",
            extent.height
        )));
    }
    let is_3d = matches!(desc.image_type, ImageType::D3);
    let ceiling = if is_3d {
        limits.max_image_3d
    } else {
        limits.max_image_2d
    };
    if extent.width > ceiling || extent.height > ceiling {
        return Err(HalError::InvalidDescriptor(format!(
            "ImageDesc::extent {extent:?} exceeds this device's {ceiling}-texel limit"
        )));
    }
    if is_3d && extent.depth_or_layers > limits.max_image_3d {
        return Err(HalError::InvalidDescriptor(format!(
            "a volume's depth {} exceeds this device's {} limit",
            extent.depth_or_layers, limits.max_image_3d
        )));
    }
    if !is_3d && extent.depth_or_layers > limits.max_image_array_layers {
        return Err(HalError::InvalidDescriptor(format!(
            "{} array layers exceed this device's {} limit",
            extent.depth_or_layers, limits.max_image_array_layers
        )));
    }
    if desc.format.is_compressed() && !caps.features.contains(Features::TEXTURE_COMPRESSION_BC) {
        return Err(HalError::InvalidDescriptor(format!(
            "{:?} needs Features::TEXTURE_COMPRESSION_BC, which this device does not report",
            desc.format
        )));
    }
    Ok(())
}

/// Whether a view of this shape can name a subrange of an image of that
/// shape.
///
/// **A view type D3D12 disagrees with is written, not rejected**: the four
/// `Create*View` calls return `void`, so a 2D view of a volume produces a
/// descriptor the debug layer objects to and the runtime ignores — a sample
/// that reads nothing, with no error at any call. This is the check that
/// turns it into one.
///
/// The dimensionality rule is D3D12's. The layer rule is this seam's: a
/// non-array view type has no `FirstArraySlice` field in any of the four
/// descriptors, so a view of layer 3 through
/// [`ImageViewType::D2`](crcbl_hal::ImageViewType::D2) would silently be a
/// view of layer 0.
pub(crate) fn check_view_type(
    image_type: ImageType,
    layers: u32,
    desc: &ImageViewDesc<'_>,
) -> Result<(), HalError> {
    use crcbl_hal::ImageViewType as V;
    let compatible = match image_type {
        ImageType::D1 => matches!(desc.view_type, V::D1),
        // A cube map's storage *is* a layered 2D image, which is why the
        // seam has a cube view type and no cube image type.
        ImageType::D2 => matches!(desc.view_type, V::D2 | V::D2Array | V::Cube | V::CubeArray),
        ImageType::D3 => matches!(desc.view_type, V::D3),
    };
    if !compatible {
        return Err(HalError::InvalidDescriptor(format!(
            "a {:?} view of a {image_type:?} image is not a D3D12 view: the view's \
             dimensionality must be the image's, and a cube is a layered 2D image",
            desc.view_type
        )));
    }
    let arrayed = matches!(desc.view_type, V::D2Array | V::Cube | V::CubeArray);
    if !arrayed && desc.range.base_layer != 0 {
        return Err(HalError::InvalidDescriptor(format!(
            "a {:?} view cannot start at layer {} of a {layers}-layer image: the descriptor \
             has no first-slice field, so the layer would be dropped rather than honoured — \
             use ImageViewType::D2Array",
            desc.view_type, desc.range.base_layer
        )));
    }
    Ok(())
}

/// Builds every descriptor an image view needs, or says which one D3D12 has
/// no member for.
///
/// They are built *before* any heap slot is taken, so a combination D3D12
/// cannot express costs nothing and leaks nothing.
pub(crate) fn build_views(
    entry_format: Format,
    usage: ImageUsage,
    desc: &ImageViewDesc<'_>,
    sub: Subresource,
) -> Result<BuiltViews, HalError> {
    let refuse = |what: &str| {
        HalError::InvalidDescriptor(format!(
            "D3D12 has no {what} for a {:?} view of a {entry_format:?} image with \
             {} samples and {} layers",
            desc.view_type, sub.samples, sub.layer_count
        ))
    };
    let mut built = BuiltViews::default();
    if usage.contains(ImageUsage::SAMPLED) {
        // A depth image is stored typeless when it is sampled, and the
        // shader view names the depth plane's own format. See
        // `conv::depth_read_format`.
        let format =
            conv::depth_read_format(desc.format).unwrap_or_else(|| conv::dxgi_format(desc.format));
        built.shader_resource = Some(
            view::shader_resource(format, desc.view_type, sub)
                .ok_or_else(|| refuse("shader resource view"))?,
        );
    }
    if usage.contains(ImageUsage::STORAGE) {
        built.unordered_access = Some(
            view::unordered_access(conv::dxgi_format(desc.format), desc.view_type, sub)
                .ok_or_else(|| refuse("unordered access view"))?,
        );
    }
    if usage.contains(ImageUsage::COLOR_ATTACHMENT) {
        built.render_target = Some(
            view::render_target(conv::dxgi_format(desc.format), desc.view_type, sub)
                .ok_or_else(|| refuse("render target view"))?,
        );
    }
    if usage.contains(ImageUsage::DEPTH_STENCIL_ATTACHMENT) {
        // Both flag sets, because a DSV's read-only flags are baked into the
        // descriptor and the render pass says which it wants only at bind
        // time. See `view::depth_stencil`.
        let format = conv::dxgi_format(desc.format);
        let stencil = desc.format.has_stencil();
        built.depth_stencil = Some(
            view::depth_stencil(format, desc.view_type, sub, false, stencil)
                .ok_or_else(|| refuse("depth stencil view"))?,
        );
        built.depth_stencil_read_only = Some(
            view::depth_stencil(format, desc.view_type, sub, true, stencil)
                .ok_or_else(|| refuse("read-only depth stencil view"))?,
        );
    }
    if built.is_empty() {
        return Err(HalError::InvalidDescriptor(
            "an image with no sampled, storage or attachment usage has no D3D12 view to \
             create; a transfer-only image is addressed by the copy itself"
                .to_string(),
        ));
    }
    Ok(built)
}

/// The view descriptors an image view will write, before any heap slot exists.
///
/// No `Debug`: the D3D12 structs are unions, and a derived formatter would
/// have to pick a member to print without knowing which one is live.
#[derive(Default)]
pub(crate) struct BuiltViews {
    pub(crate) shader_resource: Option<D3D12_SHADER_RESOURCE_VIEW_DESC>,
    pub(crate) unordered_access: Option<D3D12_UNORDERED_ACCESS_VIEW_DESC>,
    pub(crate) render_target: Option<D3D12_RENDER_TARGET_VIEW_DESC>,
    pub(crate) depth_stencil: Option<D3D12_DEPTH_STENCIL_VIEW_DESC>,
    pub(crate) depth_stencil_read_only: Option<D3D12_DEPTH_STENCIL_VIEW_DESC>,
}

impl BuiltViews {
    pub(crate) fn is_empty(&self) -> bool {
        self.shader_resource.is_none()
            && self.unordered_access.is_none()
            && self.render_target.is_none()
            && self.depth_stencil.is_none()
            && self.depth_stencil_read_only.is_none()
    }
}
