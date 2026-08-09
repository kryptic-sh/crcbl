//! The transient resource pool: graph-owned images and buffers, reused across
//! frames and aliased within one.
//!
//! `docs/plan/02-vulkan-backend.md` §2.4 asks for a "transient resource pool:
//! graph-owned images/buffers aliased across non-overlapping passes". Aliasing
//! *within* a frame is [`crate::graph`]'s half — it decides which virtual
//! resources can share a physical one, and it can decide that without a device.
//! This module is the other half: it turns "I need N physical images shaped like
//! this" into handles, and it keeps them between frames so a steady-state frame
//! creates nothing.
//!
//! # Reuse is keyed on the whole description, not on size
//!
//! [`TransientImageDesc`] is the key, and it is `Hash + Eq` for that reason. Two
//! images of the same size but different formats or usages are different
//! resources; treating them as interchangeable is how a depth buffer ends up
//! sampled as colour on one driver and not another.
//!
//! # Retirement is generational, not immediate
//!
//! A resize changes the key of every scene target, and the old ones stop being
//! asked for. They are destroyed after
//! [`RETIRE_AFTER_FRAMES`] consecutive frames without a request rather than at
//! once, because `crcbl-hal` is explicit that "`destroy_*` means *this handle is
//! dead now*; it does **not** promise the GPU is finished with the object", and
//! frames are in flight. Waiting more frames than the loop keeps in flight makes
//! the destroy safe without a device-wide idle.
//!
//! # The pool remembers what each resource was last used for
//!
//! "Transient" names the *virtual* resource's lifetime, not the physical one: a
//! steady-state frame reuses the very same [`ImageHandle`] it used last frame,
//! and the frame before that. So the state a frame leaves a pooled resource in
//! is the state the *next* frame's first barrier has to name as its source, or
//! that barrier orders itself against nothing and the two frames' writes race.
//! That is what [`TransientUse`] carries and what
//! [`TransientPool::image_use`] hands to
//! [`RenderGraph::compile`](crate::graph::RenderGraph::compile); see that
//! module's "the physical resource outlives the frame" section for the failure
//! it fixes.
//!
//! The state lives **on the pooled entry**, so retirement takes it with the
//! resource and a resize cannot leave one resource wearing another's history.

use std::collections::HashMap;

use crcbl_hal::{
    BufferDesc, BufferHandle, BufferUsage, Device, Format, HalError, ImageDesc, ImageHandle,
    ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc, ImageViewHandle, ImageViewType,
    MemoryLocation, QueueHandle, ResourceState,
};

/// Frames a pooled resource may go unused before it is destroyed.
///
/// Strictly greater than the two frames the engine keeps in flight, so a
/// resource dropped at the end of frame *n* is destroyed no earlier than frame
/// *n + 4* — by which time the submission that used it has certainly completed.
pub const RETIRE_AFTER_FRAMES: u32 = 3;

/// What a graph-owned image should be.
///
/// Deliberately smaller than [`ImageDesc`]: a transient is always
/// [`MemoryLocation::DeviceLocal`], always a single-layer 2D image, and never
/// has a caller-chosen label — the graph names it. Fewer fields is fewer ways
/// for two "identical" transients to fail to alias.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransientImageDesc {
    /// Size in pixels.
    pub extent: (u32, u32),
    /// Texel format.
    pub format: Format,
    /// Permitted uses.
    pub usage: ImageUsage,
    /// Samples per pixel; `1` for everything the MVP renders.
    pub samples: u32,
    /// Mip levels. `1` for a plain render target; more for anything a pass
    /// addresses one level of.
    ///
    /// The pool used to hard-code `1` while
    /// [`PassBuilder::use_subresource`](crate::graph::PassBuilder::use_subresource)
    /// happily barriered level 3, so the graph and the image disagreed about
    /// how many levels existed. It is a field for that reason: the graph now
    /// rejects a subresource the image does not have, and it can only do that
    /// if the description says.
    pub mip_levels: u32,
}

impl TransientImageDesc {
    /// A single-mip, single-sample 2D image.
    #[must_use]
    pub const fn new(extent: (u32, u32), format: Format, usage: ImageUsage) -> Self {
        Self {
            extent,
            format,
            usage,
            samples: 1,
            mip_levels: 1,
        }
    }

    /// The same description with `mip_levels` levels.
    #[must_use]
    pub const fn with_mip_levels(mut self, mip_levels: u32) -> Self {
        self.mip_levels = mip_levels;
        self
    }
}

/// What a graph-owned buffer should be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransientBufferDesc {
    /// Size in bytes.
    pub size: u64,
    /// Permitted uses.
    pub usage: BufferUsage,
}

/// What the last frame to use a pooled resource left it in.
///
/// The source scope of the next frame's first barrier against that resource.
/// [`ResourceState::Undefined`] with no queue is the honest answer for a
/// resource no frame has touched yet — nothing has been written, so there is
/// nothing to order against, which is exactly what `Undefined` means.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransientUse {
    /// The state the last access left it in.
    pub state: ResourceState,
    /// The queue that access ran on, so a frame that uses the resource from a
    /// different queue emits the acquire half of a queue-family transfer rather
    /// than reading memory the other queue never released.
    pub queue: Option<QueueHandle>,
}

#[derive(Debug)]
struct PooledImage {
    image: ImageHandle,
    view: ImageViewHandle,
    /// Consecutive frames this has not been asked for.
    idle_frames: u32,
    /// Whether it has already been handed out this frame.
    taken: bool,
    /// What the last frame that used it left it in.
    last: TransientUse,
}

#[derive(Debug)]
struct PooledBuffer {
    buffer: BufferHandle,
    idle_frames: u32,
    taken: bool,
    last: TransientUse,
}

/// Physical backing for the graph's transients.
///
/// Owned by the renderer, not by a graph: a graph lives for one frame and a pool
/// has to outlive it, or "pool" would mean "allocate every frame".
#[derive(Debug, Default)]
pub struct TransientPool {
    images: HashMap<TransientImageDesc, Vec<PooledImage>>,
    buffers: HashMap<TransientBufferDesc, Vec<PooledBuffer>>,
    /// Frames begun. Reported by [`TransientPool::frame_count`]; retirement
    /// counts idle frames per entry rather than reading this.
    frames: u64,
}

impl TransientPool {
    /// An empty pool.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks every pooled resource available again.
    ///
    /// Called by [`CompiledGraph::execute`](crate::graph::CompiledGraph::execute)
    /// before it realises anything, so a caller never has to remember to.
    pub fn begin_frame(&mut self) {
        self.frames = self.frames.wrapping_add(1);
        for pooled in self.images.values_mut().flatten() {
            pooled.taken = false;
        }
        for pooled in self.buffers.values_mut().flatten() {
            pooled.taken = false;
        }
    }

    /// An image matching `desc`, creating one only if every match is already in
    /// use this frame.
    ///
    /// # Errors
    ///
    /// [`HalError`] from image or view creation.
    pub fn image(
        &mut self,
        device: &dyn Device,
        desc: TransientImageDesc,
    ) -> Result<(ImageHandle, ImageViewHandle), HalError> {
        let entries = self.images.entry(desc).or_default();
        if let Some(pooled) = entries.iter_mut().find(|pooled| !pooled.taken) {
            pooled.taken = true;
            pooled.idle_frames = 0;
            return Ok((pooled.image, pooled.view));
        }

        let image = device.create_image(&ImageDesc {
            label: Some("graph transient"),
            image_type: ImageType::D2,
            extent: crcbl_hal::Extent3d::d2(desc.extent.0, desc.extent.1),
            format: desc.format,
            mip_levels: desc.mip_levels,
            samples: desc.samples,
            usage: desc.usage,
            memory: MemoryLocation::DeviceLocal,
        })?;
        let view = match device.create_image_view(&ImageViewDesc {
            label: Some("graph transient"),
            image,
            view_type: ImageViewType::D2,
            format: desc.format,
            range: ImageSubresourceRange::all(desc.format),
        }) {
            Ok(view) => view,
            Err(error) => {
                // Never leak the image on the failure path: it is already the
                // pool's, and nothing else has a handle to it.
                device.destroy_image(image);
                return Err(error);
            }
        };
        log::debug!(
            "graph: created a transient {}x{} {:?} image",
            desc.extent.0,
            desc.extent.1,
            desc.format
        );
        entries.push(PooledImage {
            image,
            view,
            idle_frames: 0,
            taken: true,
            last: TransientUse::default(),
        });
        Ok((image, view))
    }

    /// A buffer matching `desc`, creating one only if every match is already in
    /// use this frame.
    ///
    /// # Errors
    ///
    /// [`HalError`] from buffer creation.
    pub fn buffer(
        &mut self,
        device: &dyn Device,
        desc: TransientBufferDesc,
    ) -> Result<BufferHandle, HalError> {
        let entries = self.buffers.entry(desc).or_default();
        if let Some(pooled) = entries.iter_mut().find(|pooled| !pooled.taken) {
            pooled.taken = true;
            pooled.idle_frames = 0;
            return Ok(pooled.buffer);
        }
        let buffer = device.create_buffer(&BufferDesc {
            label: Some("graph transient"),
            size: desc.size,
            usage: desc.usage,
            memory: MemoryLocation::DeviceLocal,
        })?;
        log::debug!("graph: created a transient {}-byte buffer", desc.size);
        entries.push(PooledBuffer {
            buffer,
            idle_frames: 0,
            taken: true,
            last: TransientUse::default(),
        });
        Ok(buffer)
    }

    /// What the last frame left the `ordinal`-th pooled image matching `desc`
    /// in.
    ///
    /// `ordinal` counts within one description, in the order
    /// [`TransientPool::image`] hands them out — so the *n*-th request for a
    /// description in a frame is the *n*-th entry here, and a graph that has
    /// packed its transients onto physical slots can ask about a slot it has not
    /// realised yet. [`TransientUse::default`] for a description the pool has
    /// never been asked for, which is the correct answer: there is no resource,
    /// so nothing has written one.
    #[must_use]
    pub fn image_use(&self, desc: TransientImageDesc, ordinal: usize) -> TransientUse {
        self.images
            .get(&desc)
            .and_then(|entries| entries.get(ordinal))
            .map_or_else(TransientUse::default, |pooled| pooled.last)
    }

    /// What the last frame left the `ordinal`-th pooled buffer matching `desc`
    /// in. See [`TransientPool::image_use`].
    #[must_use]
    pub fn buffer_use(&self, desc: TransientBufferDesc, ordinal: usize) -> TransientUse {
        self.buffers
            .get(&desc)
            .and_then(|entries| entries.get(ordinal))
            .map_or_else(TransientUse::default, |pooled| pooled.last)
    }

    /// Records what this frame left the `ordinal`-th pooled image in.
    ///
    /// Deliberately not public: the only caller that can know this is
    /// [`CompiledGraph::execute`](crate::graph::CompiledGraph::execute), which
    /// computed the state at compile time and has just recorded the commands
    /// that reach it. A caller who wrote a different number here would move
    /// every following frame's first barrier onto a source scope the GPU never
    /// visits.
    pub(crate) fn set_image_use(
        &mut self,
        desc: TransientImageDesc,
        ordinal: usize,
        last: TransientUse,
    ) {
        if let Some(pooled) = self
            .images
            .get_mut(&desc)
            .and_then(|entries| entries.get_mut(ordinal))
        {
            pooled.last = last;
        }
    }

    /// Records what this frame left the `ordinal`-th pooled buffer in. See
    /// [`TransientPool::set_image_use`].
    pub(crate) fn set_buffer_use(
        &mut self,
        desc: TransientBufferDesc,
        ordinal: usize,
        last: TransientUse,
    ) {
        if let Some(pooled) = self
            .buffers
            .get_mut(&desc)
            .and_then(|entries| entries.get_mut(ordinal))
        {
            pooled.last = last;
        }
    }

    /// Destroys anything that has gone unused for [`RETIRE_AFTER_FRAMES`]
    /// consecutive frames, and reports how many resources went.
    ///
    /// Call once per frame **after** submitting. A resize is the case that makes
    /// this necessary: it changes the key of every scene target, so without it a
    /// window dragged across the screen accumulates one image per size it passed
    /// through.
    pub fn retire_unused(&mut self, device: &dyn Device) -> usize {
        let mut retired = 0;
        self.images.retain(|_, entries| {
            entries.retain(|pooled| {
                let keep = pooled.taken || pooled.idle_frames < RETIRE_AFTER_FRAMES;
                if !keep {
                    device.destroy_image_view(pooled.view);
                    device.destroy_image(pooled.image);
                    retired += 1;
                }
                keep
            });
            !entries.is_empty()
        });
        self.buffers.retain(|_, entries| {
            entries.retain(|pooled| {
                let keep = pooled.taken || pooled.idle_frames < RETIRE_AFTER_FRAMES;
                if !keep {
                    device.destroy_buffer(pooled.buffer);
                    retired += 1;
                }
                keep
            });
            !entries.is_empty()
        });
        // Aged *after* the sweep, so a resource is only destroyed on the frame
        // after it reached the threshold — never on the frame it was last used.
        for pooled in self.images.values_mut().flatten() {
            if !pooled.taken {
                pooled.idle_frames += 1;
            }
        }
        for pooled in self.buffers.values_mut().flatten() {
            if !pooled.taken {
                pooled.idle_frames += 1;
            }
        }
        if retired > 0 {
            log::debug!("graph: retired {retired} transient(s)");
        }
        retired
    }

    /// How many frames have begun against this pool.
    #[must_use]
    pub const fn frame_count(&self) -> u64 {
        self.frames
    }

    /// How many physical images the pool is holding.
    #[must_use]
    pub fn image_count(&self) -> usize {
        self.images.values().map(Vec::len).sum()
    }

    /// How many physical buffers the pool is holding.
    #[must_use]
    pub fn buffer_count(&self) -> usize {
        self.buffers.values().map(Vec::len).sum()
    }

    /// Destroys everything. Call after the device is idle, at shutdown.
    pub fn destroy(&mut self, device: &dyn Device) {
        for pooled in self.images.values().flatten() {
            device.destroy_image_view(pooled.view);
            device.destroy_image(pooled.image);
        }
        for pooled in self.buffers.values().flatten() {
            device.destroy_buffer(pooled.buffer);
        }
        self.images.clear();
        self.buffers.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::NullInstance;
    use crcbl_hal::{DeviceDesc, Instance};

    fn device() -> Box<dyn Device> {
        let instance = NullInstance::gpu_driven();
        let adapter = instance.adapters().remove(0);
        instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens")
    }

    fn scene(extent: (u32, u32)) -> TransientImageDesc {
        TransientImageDesc {
            extent,
            format: Format::Rgba16Float,
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
            samples: 1,
            mip_levels: 1,
        }
    }

    #[test]
    fn a_steady_state_frame_creates_nothing_after_the_first() {
        let device = device();
        let mut pool = TransientPool::new();

        pool.begin_frame();
        let first = pool
            .image(device.as_ref(), scene((64, 48)))
            .expect("created");
        assert_eq!(pool.image_count(), 1);

        for _ in 0..8 {
            pool.begin_frame();
            let again = pool
                .image(device.as_ref(), scene((64, 48)))
                .expect("reused");
            assert_eq!(again, first, "a steady frame must reuse, not allocate");
            assert_eq!(pool.image_count(), 1);
            pool.retire_unused(device.as_ref());
        }
        pool.destroy(device.as_ref());
    }

    /// Two live requests in one frame must not hand back the same image — that
    /// is the difference between a pool and a cache, and getting it wrong is a
    /// pass rendering over another pass's output.
    #[test]
    fn two_requests_in_one_frame_get_two_images() {
        let device = device();
        let mut pool = TransientPool::new();
        pool.begin_frame();
        let first = pool
            .image(device.as_ref(), scene((64, 48)))
            .expect("created");
        let second = pool
            .image(device.as_ref(), scene((64, 48)))
            .expect("created");
        assert_ne!(first, second);
        assert_eq!(pool.image_count(), 2);

        // And the next frame reuses both rather than growing to four.
        pool.begin_frame();
        let (a, b) = (
            pool.image(device.as_ref(), scene((64, 48)))
                .expect("reused"),
            pool.image(device.as_ref(), scene((64, 48)))
                .expect("reused"),
        );
        assert_eq!((a, b), (first, second));
        assert_eq!(pool.image_count(), 2);
        pool.destroy(device.as_ref());
    }

    /// Different descriptions are different resources, whatever their size.
    #[test]
    fn the_key_is_the_whole_description() {
        let device = device();
        let mut pool = TransientPool::new();
        pool.begin_frame();
        let color = pool
            .image(device.as_ref(), scene((64, 48)))
            .expect("created");
        let depth = pool
            .image(
                device.as_ref(),
                TransientImageDesc {
                    extent: (64, 48),
                    format: Format::D32Float,
                    usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
                    samples: 1,
                    mip_levels: 1,
                },
            )
            .expect("created");
        assert_ne!(color, depth);
        assert_eq!(pool.image_count(), 2);
        pool.destroy(device.as_ref());
    }

    /// A resize must not leak: the old size stops being asked for and is
    /// eventually destroyed — but not before the frames it might still be in
    /// have drained.
    #[test]
    fn a_resize_retires_the_old_size_after_the_frames_in_flight_have_drained() {
        let device = device();
        let mut pool = TransientPool::new();

        pool.begin_frame();
        pool.image(device.as_ref(), scene((64, 48)))
            .expect("created");
        pool.retire_unused(device.as_ref());
        assert_eq!(pool.image_count(), 1);

        // The resize.
        for frame in 0..RETIRE_AFTER_FRAMES {
            pool.begin_frame();
            pool.image(device.as_ref(), scene((128, 96)))
                .expect("created");
            pool.retire_unused(device.as_ref());
            assert_eq!(
                pool.image_count(),
                2,
                "frame {frame}: the old size must survive until the in-flight \
                 frames that could still reference it have drained"
            );
        }

        pool.begin_frame();
        pool.image(device.as_ref(), scene((128, 96)))
            .expect("reused");
        assert_eq!(pool.retire_unused(device.as_ref()), 1);
        assert_eq!(pool.image_count(), 1, "the old size is gone");
        pool.destroy(device.as_ref());
    }

    /// A resize storm must converge rather than accumulate one image per size
    /// the window passed through.
    #[test]
    fn a_resize_storm_converges() {
        let device = device();
        let mut pool = TransientPool::new();
        for width in 100..160u32 {
            pool.begin_frame();
            pool.image(device.as_ref(), scene((width, 96)))
                .expect("created");
            pool.retire_unused(device.as_ref());
            assert!(
                pool.image_count() <= usize::try_from(RETIRE_AFTER_FRAMES).expect("small") + 1,
                "at width {width} the pool held {} images",
                pool.image_count()
            );
        }
        pool.destroy(device.as_ref());
    }

    #[test]
    fn buffers_pool_on_the_same_rules() {
        let device = device();
        let mut pool = TransientPool::new();
        let desc = TransientBufferDesc {
            size: 4096,
            usage: BufferUsage::STORAGE,
        };
        pool.begin_frame();
        let first = pool.buffer(device.as_ref(), desc).expect("created");
        assert_eq!(pool.buffer_count(), 1);
        pool.begin_frame();
        assert_eq!(pool.buffer(device.as_ref(), desc).expect("reused"), first);
        assert_eq!(pool.buffer_count(), 1);
        pool.destroy(device.as_ref());
        assert_eq!(pool.buffer_count(), 0);
    }

    #[test]
    fn destroy_releases_everything() {
        let device = device();
        let mut pool = TransientPool::new();
        pool.begin_frame();
        pool.image(device.as_ref(), scene((64, 48)))
            .expect("created");
        pool.buffer(
            device.as_ref(),
            TransientBufferDesc {
                size: 256,
                usage: BufferUsage::STORAGE,
            },
        )
        .expect("created");
        pool.destroy(device.as_ref());
        assert_eq!(pool.image_count(), 0);
        assert_eq!(pool.buffer_count(), 0);
        // And destroying twice is harmless.
        pool.destroy(device.as_ref());
    }
}
