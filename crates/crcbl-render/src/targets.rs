//! The transients a frame of this renderer has, described once.
//!
//! Each function names one target the forward renderer and its effects create
//! in the render graph: its format, its usage and its sample count. They are
//! this crate's rather than `crcbl-frame`'s because they are facts about this
//! renderer's frame — the HDR scene target, the depth pyramid, the bloom chain
//! — and a caller drawing only a UI has none of them.

use crcbl_hal::{Format, ImageUsage};

use crate::transient::TransientImageDesc;

/// The HDR scene target: `Rgba16Float`, rendered into and then sampled.
///
/// **HDR from P1**, per `docs/plan/ROADMAP.md`'s correction. `Rgba16Float`
/// rather than the swapchain's format is the entire structural commitment;
/// the tonemap pass that resolves it is the other half.
#[must_use]
pub const fn scene_color(extent: (u32, u32)) -> TransientImageDesc {
    TransientImageDesc {
        extent,
        format: Format::Rgba16Float,
        usage: ImageUsage::COLOR_ATTACHMENT
            .union(ImageUsage::SAMPLED)
            // So a headless render can read the HDR values back before the
            // tonemap flattens them, which is what makes "the RGBA16F path
            // is real" checkable.
            .union(ImageUsage::TRANSFER_SRC),
        samples: 1,
        mip_levels: 1,
    }
}

/// The display-space image the tonemap writes when a resolve follows it:
/// the caller's own target format, rendered into and then sampled.
///
/// **The format is a parameter where every other description here fixes
/// one**, and it has to be: this image stands in for the caller's target
/// between the tonemap and the resolve, so a format of this module's
/// choosing would resolve one encoding into another. The antialiasing
/// resolve is the only pass that reads it.
///
/// `TRANSFER_SRC` for [`scene_color`]'s reason — a headless render can read
/// the display-space frame back before the resolve filters it, which is what
/// makes "the antialiasing pass changed the picture" checkable.
#[must_use]
pub const fn display_color(extent: (u32, u32), format: Format) -> TransientImageDesc {
    TransientImageDesc {
        extent,
        format,
        usage: ImageUsage::COLOR_ATTACHMENT
            .union(ImageUsage::SAMPLED)
            .union(ImageUsage::TRANSFER_SRC),
        samples: 1,
        mip_levels: 1,
    }
}

/// One level of topic 48's bloom chain:
/// `Rgba16Float`, rendered into and then sampled.
///
/// [`scene_color`]'s description without the `TRANSFER_SRC` — nothing reads
/// a chain level back, and a usage flag naming a use nothing makes is a
/// claim about a resource rather than a fact about one.
///
/// **`Rgba16Float` and not something narrower.** The chain carries the
/// scene's above-display range, which is the part of it that blooms at all;
/// an eight-bit level would clip exactly that before the tonemap saw it, and
/// a threshold-free bloom would then be a bloom of a picture the frame does
/// not contain.
///
/// **One description per level, at that level's own extent**, because
/// `crcbl_render::bloom` builds its chain out of separate single-mip images
/// rather than one image with mips — that module's header gives the two
/// independent reasons, and both of them are about what a pass can attach.
/// Distinct extents are distinct descriptions, so the pool hands out a
/// distinct physical image for each and every pass gets a correct render
/// area.
#[must_use]
pub const fn bloom_mip(extent: (u32, u32)) -> TransientImageDesc {
    TransientImageDesc {
        extent,
        format: Format::Rgba16Float,
        usage: ImageUsage::COLOR_ATTACHMENT.union(ImageUsage::SAMPLED),
        samples: 1,
        mip_levels: 1,
    }
}

/// The depth buffer: `D32Float`, reversed-Z, written by the depth prepass
/// and **sampled** by the screen-space occlusion pass.
///
/// `SAMPLED` since topic 46's AO slice, which is the
/// prepass that made this image something downstream reads. It costs nothing
/// on a frame that adds no such pass: a usage flag is a promise about what
/// the image may be bound as, not a copy or a layout.
#[must_use]
pub const fn scene_depth(extent: (u32, u32)) -> TransientImageDesc {
    TransientImageDesc {
        extent,
        format: Format::D32Float,
        usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT.union(ImageUsage::SAMPLED),
        samples: 1,
        mip_levels: 1,
    }
}

/// One level of the Hi-Z depth pyramid: `D32Float`, rendered into through
/// `SV_Depth` and then fetched.
///
/// [`scene_depth`]'s description at a level's own extent, which is the
/// point of it being a description of its own: `crcbl_render::hiz` builds
/// the pyramid out of separate single-level images for [`bloom_mip`]'s two
/// reasons, so a level is its own extent and the pool hands out its own
/// physical image for it.
///
/// **A depth format rather than `R32Float`.** `shaders/hiz.slang`'s header
/// carries the whole of it: `R32Float` is unfilterable in WebGPU, and the
/// pyramid would be a different texture type from the prepass that is its
/// own level 0. The seam has since grown
/// [`SampleType::UnfilterableFloat`](crcbl_hal::SampleType::UnfilterableFloat)
/// for the probe visibility maps, so the first half of that reason is now
/// answerable; the second is not, and it is the one that decided this.
#[must_use]
pub const fn hiz_level(extent: (u32, u32)) -> TransientImageDesc {
    TransientImageDesc {
        extent,
        format: Format::D32Float,
        usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT.union(ImageUsage::SAMPLED),
        samples: 1,
        mip_levels: 1,
    }
}

/// A screen-space occlusion channel: `Rgba8Unorm`, rendered into and then
/// fetched.
///
/// Topic 46's AO chain — `ssao.slang` writes one of
/// these, `ssao_blur.slang` reads it and writes another and
/// `ssao_upsample.slang` writes the last, which is why this is one
/// description rather than three that agree today.
///
/// **`r` is the visibility and `gba` are the bent direction.** Eight bits
/// each: the scalar is a fraction of a hemisphere a 4×4 kernel has just
/// averaged sixteen samples into, and the direction is a unit vector three
/// filters have averaged and renormalised, so the quantisation is far below
/// what either has already smoothed away. `ssao.slang`'s header carries the
/// encoding and why it is three channels rather than an octahedral pair.
///
/// **It was `R8Unorm` until the bent direction arrived**, and the three
/// channels are the bandwidth the AO tier split is about, which
/// `docs/notes/rendering.md` records.
#[must_use]
pub const fn ambient_occlusion(extent: (u32, u32)) -> TransientImageDesc {
    TransientImageDesc {
        extent,
        format: Format::Rgba8Unorm,
        usage: ImageUsage::COLOR_ATTACHMENT.union(ImageUsage::SAMPLED),
        samples: 1,
        mip_levels: 1,
    }
}

/// A screen-space contact-shadow visibility mask: `R8Unorm`, rendered into
/// and fetched by the forward pass.
///
/// Separate from [`ambient_occlusion`], whose extra
/// channels carry a bent normal and therefore cannot back the `R8Unorm`
/// contact-shadow pipeline.
#[must_use]
pub const fn contact_shadows(extent: (u32, u32)) -> TransientImageDesc {
    TransientImageDesc::new(
        extent,
        Format::R8Unorm,
        ImageUsage::COLOR_ATTACHMENT.union(ImageUsage::SAMPLED),
    )
}

/// The reflectivity channel: `Rgba8Unorm`, `rgb` the surface's `F0` and `a`
/// its roughness, written by the forward pass beside its colour target.
///
/// Topic 47's screen-space reflections run after
/// that pass, so the only per-pixel data they would otherwise have are the
/// depth buffer and the scene colour — neither of which says which surfaces
/// reflect or how sharply. `mesh.slang`'s `FragmentOutput` is the other half
/// of this description, and it carries the material's *inputs* rather than an
/// evaluated shading term.
///
/// Eight bits per channel on the argument [`ambient_occlusion`] already
/// makes for its one: every value here is bounded in `0..=1`, and the
/// quantisation is below what a reflection approximating a whole room can
/// resolve.
///
/// `TRANSFER_SRC` for [`scene_color`]'s reason — so a headless probe can read
/// the attachment back and say what it holds, which is what makes "the
/// forward pass writes the bound material row" a test result rather than a
/// claim about a buffer nothing looks at.
///
/// `SAMPLED` since the reflection slice, and not before it: the attachment
/// landed one slice ahead of the pass that reads it, and a usage flag naming
/// a binding nothing binds is a claim about a resource rather than a fact
/// about one. `crcbl_render::ssr` is that reader.
#[must_use]
pub const fn reflectivity(extent: (u32, u32)) -> TransientImageDesc {
    TransientImageDesc {
        extent,
        format: Format::Rgba8Unorm,
        usage: ImageUsage::COLOR_ATTACHMENT
            .union(ImageUsage::TRANSFER_SRC)
            .union(ImageUsage::SAMPLED),
        samples: 1,
        mip_levels: 1,
    }
}

/// The motion-vector channel: `crate::forward`'s `MOTION_FORMAT`, written
/// by the forward pass beside its colour and reflectivity targets.
///
/// Topic 43 §9's third attachment — the one that
/// blocked TAA, temporal SSR, temporal upscaling, per-object motion blur and
/// SSGI's accumulation. What it holds is **current minus previous, in
/// texture coordinates, `+y` down**: a pass holding the previous frame reads
/// its history at `uv - motion`. `mesh.slang`'s `motion_vector` writes it
/// and `MOTION_FORMAT` states the convention on the host side.
///
/// The format is named there rather than here for [`ambient_occlusion`]'s
/// reason inverted: the shader's return struct and the pipeline's colour
/// target have to agree, and the pass that builds the pipeline is the one
/// that owns the answer.
///
/// **Cleared to zero, and no `TRANSFER_SRC` or `SAMPLED` yet.** Nothing
/// reads the attachment — `crcbl_render::forward`'s `DebugView::Motion`
/// observes the *subtraction*, through the scene target, rather than this
/// image — and a usage flag naming a use nothing makes is a claim about a
/// resource rather than a fact about one, which is [`bloom_mip`]'s argument.
/// The first consumer is the one that adds the flag it needs.
#[must_use]
pub const fn motion(extent: (u32, u32)) -> TransientImageDesc {
    TransientImageDesc {
        extent,
        format: crate::forward::MOTION_FORMAT,
        usage: ImageUsage::COLOR_ATTACHMENT,
        samples: 1,
        mip_levels: 1,
    }
}
