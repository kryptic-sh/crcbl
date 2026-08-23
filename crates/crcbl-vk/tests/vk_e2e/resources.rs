//! Resource-descriptor refusals, against a real driver.
//!
//! `crcbl-vk::create_image` used to carry its own copy of the seam's image
//! rules — one of three, alongside `crcbl-mtl` and `crcbl-dx12` — and nothing
//! tested any of them. The copies had drifted: `mip_levels == 0` was refused by
//! `crcbl-hal`'s null backend and not by this one, so a zero reached
//! `vkCreateImage` as `VUID-VkImageCreateInfo-mipLevels-00947` instead of the
//! descriptor error the seam promises. The rules now live once, as
//! `ImageDesc::check`, and this module is what holds this backend to calling
//! them.
//!
//! It is a **Vulkan** module rather than an agnostic one for the reason the
//! seam suite cannot cover: `crcbl-mtl` silently clamps a zero mip count to one
//! and `crcbl-dx12` does not check it at all, so an agnostic test would redden
//! two deferred backends' jobs. `docs/backlog.md` carries that.

use crate::harness::Headless;
use crcbl_hal::{Extent3d, Format, HalError, ImageDesc, ImageType, ImageUsage};

/// The rules `ImageDesc::check` states, asked of a real device.
///
/// The mip case is the one that was actually broken here; the rest are the
/// company it keeps, and each is paired with the accepting arm so a check that
/// refused everything would fail this too.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn an_image_descriptor_the_seam_refuses_never_reaches_the_driver() {
    let headless = Headless::open_for_triangle();
    let device = headless.device.as_ref();
    let limits = device.caps().limits;

    let image = |mip_levels, samples, usage, extent| ImageDesc {
        label: Some("resource rules"),
        image_type: ImageType::D2,
        extent,
        format: Format::Rgba8UnormSrgb,
        mip_levels,
        samples,
        usage,
    };
    let square = Extent3d {
        width: 64,
        height: 64,
        depth_or_layers: 1,
    };

    for (case, desc) in [
        (
            "a zero extent",
            image(1, 1, ImageUsage::SAMPLED, Extent3d { width: 0, ..square }),
        ),
        // The rule this backend did not have. A zero mip count is not "give me
        // the default": it describes no image, and Vulkan says so in a VUID the
        // caller never sees on a release driver.
        (
            "no mip levels at all",
            image(0, 1, ImageUsage::SAMPLED, square),
        ),
        (
            "more mips than a 64x64 image holds",
            image(99, 1, ImageUsage::SAMPLED, square),
        ),
        (
            "three samples, which is a two-bit mask",
            image(1, 3, ImageUsage::SAMPLED, square),
        ),
        (
            "an extent past this device's 2D limit",
            image(
                1,
                1,
                ImageUsage::SAMPLED,
                Extent3d {
                    width: limits.max_image_2d.saturating_add(1),
                    height: 1,
                    depth_or_layers: 1,
                },
            ),
        ),
        ("no usage at all", image(1, 1, ImageUsage::empty(), square)),
    ] {
        let error = device
            .create_image(&desc)
            .expect_err("the seam refuses this descriptor on every backend");
        assert!(
            matches!(error, HalError::InvalidDescriptor(_)),
            "{case} is a malformed descriptor, not an unsupported feature: {error}"
        );
    }

    // …and the descriptor all six were derived from still makes an image, so a
    // check that refused everything would not pass unnoticed.
    let ok = device
        .create_image(&image(1, 1, ImageUsage::SAMPLED, square))
        .expect("a 64x64 single-sampled sampled image is ordinary");
    device.destroy_image(ok);

    // A full mip chain is accepted at its exact length, which is the boundary
    // the "more mips than it holds" case sits one past.
    let full = square.full_mip_levels(ImageType::D2);
    let chained = device
        .create_image(&image(full, 1, ImageUsage::SAMPLED, square))
        .expect("the exact chain length is not one too many");
    device.destroy_image(chained);

    headless.finish();
}
