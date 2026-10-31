//! What a pass constructor has created so far, released if it does not finish.
//!
//! A pass's `new` creates its layouts, pipelines, samplers and buffers one seam
//! call at a time with `?` between them, and the seam's `destroy_*` is
//! explicit — so a failure half way through would otherwise leave everything
//! created before it alive until the device goes. Most of those passes are
//! built lazily between frames, the first time a frame asks for them, so a
//! failure there is retried and would leak again on every retry.
//!
//! [`Partial`] is the one guard they share: each handle goes through
//! [`Partial::add`] as it is created, and the guard destroys every one of them
//! when it drops — **in the reverse order they were added**, which is
//! dependency order, since a pipeline is created after its layout and a group
//! after its layout and buffers — unless [`Partial::keep`] handed them to the
//! finished pass first. Dropping rather than an explicit call is what makes
//! every `?` after the guard exists a release, without a cleanup arm per call.
//!
//! The objects it destroys were created by the constructor that is failing and
//! never reached a frame, so no submitted work can be reading them. A
//! constructor that also records into an
//! [`UploadBatch`](crate::texture::UploadBatch) still has to
//! [abandon](crate::texture::UploadBatch::abandon) it on failure, since the
//! batch would otherwise copy into what this destroyed — `Ssr::new` and
//! `Exposure::new` do.

use crcbl_hal::{
    BindGroupHandle, BindGroupLayoutHandle, BufferHandle, ComputePipelineHandle, Device,
    GraphicsPipelineHandle, ImageHandle, ImageViewHandle, PipelineLayoutHandle, SamplerHandle,
};

use crate::texture::UploadedTexture;

/// One handle a [`Partial`] holds, tagged with the `destroy_*` it needs.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Created {
    Buffer(BufferHandle),
    Image(ImageHandle),
    ImageView(ImageViewHandle),
    Sampler(SamplerHandle),
    BindGroupLayout(BindGroupLayoutHandle),
    BindGroup(BindGroupHandle),
    PipelineLayout(PipelineLayoutHandle),
    GraphicsPipeline(GraphicsPipelineHandle),
    ComputePipeline(ComputePipelineHandle),
}

macro_rules! created_from {
    ($($handle:ty => $variant:ident),+ $(,)?) => {
        $(
            impl From<$handle> for Created {
                fn from(handle: $handle) -> Self {
                    Self::$variant(handle)
                }
            }
        )+
    };
}

created_from! {
    BufferHandle => Buffer,
    ImageHandle => Image,
    ImageViewHandle => ImageView,
    SamplerHandle => Sampler,
    BindGroupLayoutHandle => BindGroupLayout,
    BindGroupHandle => BindGroup,
    PipelineLayoutHandle => PipelineLayout,
    GraphicsPipelineHandle => GraphicsPipeline,
    ComputePipelineHandle => ComputePipeline,
}

impl Created {
    fn destroy(self, device: &dyn Device) {
        match self {
            Self::Buffer(handle) => device.destroy_buffer(handle),
            Self::Image(handle) => device.destroy_image(handle),
            Self::ImageView(handle) => device.destroy_image_view(handle),
            Self::Sampler(handle) => device.destroy_sampler(handle),
            Self::BindGroupLayout(handle) => device.destroy_bind_group_layout(handle),
            Self::BindGroup(handle) => device.destroy_bind_group(handle),
            Self::PipelineLayout(handle) => device.destroy_pipeline_layout(handle),
            Self::GraphicsPipeline(handle) => device.destroy_graphics_pipeline(handle),
            Self::ComputePipeline(handle) => device.destroy_compute_pipeline(handle),
        }
    }
}

/// Every handle a constructor has created so far, destroyed when this drops
/// unless [`keep`](Self::keep) was called — see the module docs.
#[derive(Debug)]
#[must_use = "a guard dropped at once releases nothing it was later meant to hold"]
pub(crate) struct Partial<'d> {
    device: &'d dyn Device,
    created: Vec<Created>,
}

impl<'d> Partial<'d> {
    /// An empty guard over objects `device` creates.
    pub(crate) fn new(device: &'d dyn Device) -> Self {
        Self {
            device,
            created: Vec::new(),
        }
    }

    /// Holds `handle` until the guard is kept or dropped, and hands it back.
    pub(crate) fn add<H: Copy + Into<Created>>(&mut self, handle: H) -> H {
        self.created.push(handle.into());
        handle
    }

    /// Holds an uploaded texture's image and view, and hands it back — the
    /// view released first, as [`UploadedTexture::destroy`] does.
    pub(crate) fn add_texture(&mut self, texture: UploadedTexture) -> UploadedTexture {
        self.add(texture.image);
        self.add(texture.view);
        texture
    }

    /// Lets go of everything without destroying it, once the finished pass owns
    /// every handle and its own `destroy` will release them.
    pub(crate) fn keep(mut self) {
        self.created.clear();
    }
}

impl Drop for Partial<'_> {
    fn drop(&mut self) {
        for created in self.created.drain(..).rev() {
            created.destroy(self.device);
        }
    }
}
