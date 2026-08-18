//! Which subresources a colour attachment's resolve view names, and which
//! pairings D3D12 has no resolve for at all.
//!
//! # A resolve is a call after the pass, not a field of the attachment
//!
//! Vulkan's `VkRenderingAttachmentInfo` carries a `resolveImageView` and
//! Metal's `MTLRenderPassColorAttachmentDescriptor` carries a `resolveTexture`,
//! so on both the resolve is part of the pass description and the driver places
//! it. D3D12's `OMSetRenderTargets` takes render target descriptors and nothing
//! else: the resolve is `ID3D12GraphicsCommandList::ResolveSubresource`, one
//! call per subresource pair, recorded once the pass's own commands are done.
//! `crate::command`'s `end_render_pass` is where those calls go and this module
//! is what decides them.
//!
//! # One call names one subresource on each side
//!
//! `ResolveSubresource` takes a destination resource and index, a source
//! resource and index, and one format. It has no array range and no scaling: an
//! attachment view covering several array layers is several calls, exactly as
//! an image-to-image copy of several layers is several `CopyTextureRegion`s —
//! see `crate::command`'s `plan_image_copy`, which reaches the same shape from
//! the same restriction.
//!
//! # What it refuses, and why refusing beats approximating
//!
//! [`plan`] answers [`HalError::InvalidDescriptor`] for every pairing the seam
//! permits and D3D12 does not: a single-sampled source, a multisampled
//! destination, two different formats, two different resource types, a volume,
//! differing layer counts and differing extents. None of those is a shape this
//! module could do *something* reasonable with — `ResolveSubresource` returns
//! `void`, so the alternative to refusing is a debug-layer message on a machine
//! with the debug layer on and a wrong or missing picture everywhere else.
//!
//! # Not Windows-only, and that is the point
//!
//! This module holds no `windows` type — it is `u32` arithmetic, three seam
//! enums and one [`HalError`] — so off Windows it exists in the test build alone
//! and `cargo test` on any host checks it, exactly as [`crate::sync`] and
//! [`crate::query`] are compiled for. The arithmetic is worth that: a
//! subresource index that is wrong by one array layer resolves the wrong slice
//! and reports nothing.

use crcbl_hal::{Format, HalError, ImageType, ImageViewType};

/// What one colour attachment view addresses, in the terms a resolve is planned
/// in.
///
/// Built by `crate::device`'s `create_image_view` while the image's own geometry
/// is in hand, and carried on the view so that a pass can plan a resolve without
/// the image handle it was never given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Attachment {
    /// The image's format, which is also the view's — `create_image_view`
    /// refuses a differing one.
    pub(crate) format: Format,
    /// The image's type. `ResolveSubresource` requires both resources to be the
    /// same one.
    pub(crate) image_type: ImageType,
    /// Samples per texel of the image being viewed.
    pub(crate) samples: u32,
    /// Subresource index of the view's **first** array layer, in plane zero —
    /// the only plane a colour format has.
    pub(crate) subresource: u32,
    /// Distance in subresource indices between consecutive array layers, which
    /// is the image's mip level count per `D3D12CalcSubresource`.
    pub(crate) layer_stride: u32,
    /// Array layers the view's attachment descriptor addresses. See
    /// [`attached_layers`], which is not the same as the layer count of the
    /// range the view was created from.
    pub(crate) layers: u32,
    /// The extent of the mip the view addresses, in texels.
    pub(crate) extent: (u32, u32, u32),
}

/// How many array layers an attachment descriptor of this view type addresses.
///
/// **Not the view's layer count.** `crate::view`'s `render_target` builds a
/// `D3D12_TEX2D_RTV` for [`ImageViewType::D2`] and that struct has no
/// `FirstArraySlice` and no `ArraySize` — it addresses layer zero and nothing
/// else — while the seam happily lets a `D2` view of a six-layer image resolve
/// its range to all six. Only the array dimensions carry the pair, so only they
/// address more than one layer, and reading `layer_count` here instead would
/// plan six resolves for a descriptor that covers one.
pub(crate) const fn attached_layers(view_type: ImageViewType, layer_count: u32) -> u32 {
    match view_type {
        ImageViewType::D2Array | ImageViewType::Cube | ImageViewType::CubeArray => layer_count,
        // `D3`'s render target view does carry `FirstWSlice`/`WSize`, but a
        // volume's slices are not subresources and [`plan`] refuses one outright
        // — see the reason it gives.
        ImageViewType::D1 | ImageViewType::D2 | ImageViewType::D3 => 1,
    }
}

/// The `(source, destination)` subresource pairs one attachment's resolve
/// expands into, or why D3D12 has no resolve for the pair.
///
/// `index` names the colour attachment, so a caller reading the error knows
/// which of them it wrote.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for every pairing listed in the module docs.
/// The seam permits each of them and `ResolveSubresource` expresses none.
pub(crate) fn plan(
    index: usize,
    source: &Attachment,
    target: &Attachment,
) -> Result<Vec<(u32, u32)>, HalError> {
    let refuse = |why: String| {
        Err(HalError::InvalidDescriptor(format!(
            "colour attachment {index} names a resolve view, and {why}"
        )))
    };
    if source.samples < 2 {
        return refuse(format!(
            "its own view is {}-sample; ResolveSubresource averages the samples of a multisampled \
             subresource and there is nothing here to average",
            source.samples
        ));
    }
    if target.samples != 1 {
        return refuse(format!(
            "that view is {}-sample; ResolveSubresource writes a single-sampled subresource",
            target.samples
        ));
    }
    if source.format != target.format {
        return refuse(format!(
            "it resolves a {:?} target into a {:?} view; ResolveSubresource takes one format and \
             this backend stores every image with its own, so the two are two DXGI formats",
            source.format, target.format
        ));
    }
    if source.image_type != target.image_type {
        return refuse(format!(
            "it resolves a {:?} target into a {:?} view; ResolveSubresource requires the two \
             resources to be the same type",
            source.image_type, target.image_type
        ));
    }
    if matches!(source.image_type, ImageType::D3) {
        return refuse(
            "both sides are volumes; a volume's depth slices share one subresource, so there is \
             no per-slice resolve to record and D3D12 has no multisampled volume to read from \
             either"
                .to_string(),
        );
    }
    if source.layers != target.layers {
        return refuse(format!(
            "it resolves {} array layers into {}; each layer is a resolve of its own and needs one \
             on the other side",
            source.layers, target.layers
        ));
    }
    if source.layers == 0 {
        return refuse(
            "neither side addresses an array layer; a plan of no calls would record nothing and \
             report nothing"
                .to_string(),
        );
    }
    if source.extent != target.extent {
        let (sw, sh, sd) = source.extent;
        let (tw, th, td) = target.extent;
        return refuse(format!(
            "it resolves a {sw}x{sh}x{sd} mip into a {tw}x{th}x{td} one; ResolveSubresource does \
             not scale and requires the two subresources to have the same dimensions"
        ));
    }
    Ok((0..source.layers)
        .map(|layer| {
            (
                source.subresource + layer * source.layer_stride,
                target.subresource + layer * target.layer_stride,
            )
        })
        .collect())
}

/// Refuses a pass whose resolves would transition one subresource twice.
///
/// Each entry is `(colour attachment, resource identity, subresource)`, and the
/// identity is an interface pointer as a plain integer — compared here, never
/// dereferenced, which is the whole reason this stays a module a Linux host can
/// test.
///
/// # Why one pass has to be looked at as a whole
///
/// [`plan`] sees one attachment and can say nothing about the pass around it,
/// and the barriers bracketing the resolves go out as **one** `ResourceBarrier`
/// batch. A batch that names a subresource twice is invalid: the second entry's
/// `StateBefore` is not the state the first entry left it in, and D3D12 reports
/// that through the debug layer or not at all. Two colour attachments naming one
/// resolve target is how a caller reaches it, and that descriptor is wrong on
/// its own terms too — the second resolve would overwrite the first.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] naming the two attachments that collided.
pub(crate) fn check_unique(touched: &[(usize, usize, u32)]) -> Result<(), HalError> {
    for (position, &(attachment, resource, subresource)) in touched.iter().enumerate() {
        let clash = touched[..position]
            .iter()
            .find(|&&(_, held, index)| held == resource && index == subresource);
        if let Some(&(earlier, ..)) = clash {
            return Err(HalError::InvalidDescriptor(format!(
                "colour attachments {earlier} and {attachment} resolve through subresource \
                 {subresource} of one image; the barrier batch that brackets a pass's resolves may \
                 name a subresource once, and the second resolve would overwrite the first in any \
                 case"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4x multisampled colour target, which is what the seam exercise resolves
    /// from.
    fn multisampled() -> Attachment {
        Attachment {
            format: Format::Rgba8UnormSrgb,
            image_type: ImageType::D2,
            samples: 4,
            subresource: 0,
            layer_stride: 1,
            layers: 1,
            extent: (64, 64, 1),
        }
    }

    /// The single-sampled target it resolves into.
    fn single_sampled() -> Attachment {
        Attachment {
            samples: 1,
            ..multisampled()
        }
    }

    fn why(error: &HalError) -> String {
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "a resolve D3D12 cannot express is the caller's descriptor: {error:?}"
        );
        error.to_string()
    }

    #[test]
    fn one_layer_resolves_through_one_call() {
        let pairs = plan(0, &multisampled(), &single_sampled()).expect("a 4x resolve into a 1x");
        assert_eq!(pairs, vec![(0, 0)]);
    }

    /// **Each array layer is its own call, and the two sides step by their own
    /// stride.**
    ///
    /// Red if the strides are read off one side: a source with one mip and a
    /// destination with four are the shape a resolve into a mip chain's top
    /// level has, and stepping the destination by the source's stride would
    /// resolve every layer into the same subresource — the last one wins, the
    /// rest are silently dropped, and nothing anywhere reports it.
    #[test]
    fn each_array_layer_resolves_into_the_layer_it_matches() {
        let source = Attachment {
            layers: 3,
            layer_stride: 1,
            ..multisampled()
        };
        let target = Attachment {
            layers: 3,
            layer_stride: 4,
            samples: 1,
            ..multisampled()
        };
        let pairs = plan(0, &source, &target).expect("three layers, two strides");
        assert_eq!(pairs, vec![(0, 0), (1, 4), (2, 8)]);
    }

    /// A view's base subresource is where the walk starts, on both sides.
    #[test]
    fn the_walk_starts_at_each_views_own_subresource() {
        let source = Attachment {
            layers: 2,
            subresource: 6,
            ..multisampled()
        };
        let target = Attachment {
            layers: 2,
            subresource: 9,
            layer_stride: 2,
            samples: 1,
            ..multisampled()
        };
        assert_eq!(
            plan(0, &source, &target).expect("two layers from an offset"),
            vec![(6, 9), (7, 11)]
        );
    }

    /// **The trap `attached_layers` exists for**: the seam lets a `D2` view of a
    /// layered image resolve its range to every layer, and the `D3D12_TEX2D_RTV`
    /// that view becomes addresses layer zero alone.
    #[test]
    fn only_an_array_view_addresses_more_than_one_layer() {
        assert_eq!(attached_layers(ImageViewType::D2, 6), 1);
        assert_eq!(attached_layers(ImageViewType::D1, 6), 1);
        assert_eq!(attached_layers(ImageViewType::D3, 6), 1);
        assert_eq!(attached_layers(ImageViewType::D2Array, 6), 6);
        assert_eq!(attached_layers(ImageViewType::Cube, 6), 6);
        assert_eq!(attached_layers(ImageViewType::CubeArray, 12), 12);
    }

    #[test]
    fn a_single_sampled_source_has_nothing_to_average() {
        let error = plan(2, &single_sampled(), &single_sampled()).expect_err("nothing to resolve");
        let text = why(&error);
        assert!(
            text.contains("colour attachment 2"),
            "the refusal must say which attachment it is about: {text}"
        );
        assert!(text.contains("1-sample"), "{text}");
    }

    #[test]
    fn a_multisampled_destination_is_not_a_resolve() {
        let text = why(&plan(0, &multisampled(), &multisampled()).expect_err("both 4x"));
        assert!(text.contains("single-sampled subresource"), "{text}");
    }

    #[test]
    fn the_two_formats_must_be_one_format() {
        let target = Attachment {
            format: Format::Rgba8Unorm,
            ..single_sampled()
        };
        let text = why(&plan(0, &multisampled(), &target).expect_err("two formats"));
        assert!(text.contains("Rgba8UnormSrgb"), "{text}");
        assert!(text.contains("Rgba8Unorm"), "{text}");
    }

    #[test]
    fn the_two_resource_types_must_be_one_type() {
        let target = Attachment {
            image_type: ImageType::D1,
            ..single_sampled()
        };
        let text = why(&plan(0, &multisampled(), &target).expect_err("two types"));
        assert!(text.contains("same type"), "{text}");
    }

    /// A volume is refused on its own account rather than left to the type
    /// check, which two volumes would pass.
    #[test]
    fn a_volume_has_no_per_slice_resolve() {
        let source = Attachment {
            image_type: ImageType::D3,
            ..multisampled()
        };
        let target = Attachment {
            image_type: ImageType::D3,
            ..single_sampled()
        };
        let text = why(&plan(0, &source, &target).expect_err("a volume"));
        assert!(
            text.contains("depth slices share one subresource"),
            "{text}"
        );
    }

    #[test]
    fn every_layer_needs_one_on_the_other_side() {
        let source = Attachment {
            layers: 6,
            ..multisampled()
        };
        let text = why(&plan(0, &source, &single_sampled()).expect_err("6 into 1"));
        assert!(text.contains("6 array layers into 1"), "{text}");
    }

    /// Zero layers on both sides passes the equality check above and would plan
    /// no calls at all, which is a resolve that silently did not happen.
    #[test]
    fn a_plan_of_no_calls_is_refused_rather_than_recorded() {
        let source = Attachment {
            layers: 0,
            ..multisampled()
        };
        let target = Attachment {
            layers: 0,
            ..single_sampled()
        };
        let text = why(&plan(0, &source, &target).expect_err("no layers"));
        assert!(text.contains("record nothing and report nothing"), "{text}");
    }

    /// The check that catches a resolve into a mip level of the wrong size,
    /// which is the one shape a matching format and layer count still gets
    /// wrong.
    #[test]
    fn the_two_subresources_must_have_the_same_dimensions() {
        let target = Attachment {
            extent: (32, 32, 1),
            ..single_sampled()
        };
        let text = why(&plan(0, &multisampled(), &target).expect_err("half the size"));
        assert!(text.contains("64x64x1"), "{text}");
        assert!(text.contains("32x32x1"), "{text}");
    }

    /// Two attachments, two multisampled sources, two resolve targets: the
    /// ordinary pass, and the one [`check_unique`] must not object to.
    #[test]
    fn two_attachments_resolving_into_two_targets_are_fine() {
        let touched = [(0, 0x10, 0), (0, 0x20, 0), (1, 0x30, 0), (1, 0x40, 0)];
        check_unique(&touched).expect("four distinct subresources");
        // The same resource at two different subresources is also distinct —
        // resolving two array layers of one image into two of another is what a
        // multi-layer attachment does.
        check_unique(&[(0, 0x10, 0), (0, 0x10, 1), (0, 0x20, 4), (0, 0x20, 5)])
            .expect("one resource, four subresources");
    }

    /// The descriptor this exists for: two colour attachments resolving into one
    /// target.
    #[test]
    fn one_resolve_target_named_twice_is_refused() {
        let error = check_unique(&[(0, 0x10, 0), (0, 0x99, 3), (1, 0x20, 0), (1, 0x99, 3)])
            .expect_err("one target, two attachments");
        let text = why(&error);
        assert!(text.contains("attachments 0 and 1"), "{text}");
        assert!(text.contains("subresource 3"), "{text}");
    }

    /// The identity is the resource *and* the subresource together, so an index
    /// that matches on a different resource is not a collision. Red if the
    /// comparison drops either half — dropping the resource would refuse every
    /// ordinary two-attachment pass, since both resolve through subresource
    /// zero.
    #[test]
    fn a_subresource_index_alone_is_not_a_collision() {
        check_unique(&[(0, 0x10, 0), (1, 0x11, 0)]).expect("two resources, same index");
        check_unique(&[(0, 0x10, 0), (1, 0x10, 1)]).expect("one resource, two indices");
    }
}
