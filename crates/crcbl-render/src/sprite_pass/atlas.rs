//! GPU-rendered sheets: an atlas of fixed-size cells that a rendered image is
//! **copied** into, drawn like any other sheet.
//!
//! ```text
//! create_atlas ──▶ one sheet, every texel transparent, cells × columns × rows
//!   allocate_slot ──▶ AtlasSlot (a cell, a generation, its UVs)   ─┐ or
//!                                                    SheetError::AtlasFull
//!   add_slot_copies(graph, [SlotCopy { source, slot }])  ── a copy pass in
//!                     the caller's graph: source ─▶ the slot's cell
//!   add_pass ──▶ sprites naming the atlas sample it, after those copies
//!   free_slot ──▶ the cell is free for the next allocate; nothing is destroyed
//! ```
//!
//! # A copy, not a sampled target
//!
//! The two ways a rendered image can become a sheet are to sample the image
//! itself, or to copy it into an image this renderer owns. **This copies.**
//!
//! Sampling the target directly would make every registered target an image
//! whose lifetime the caller and the frame ring share: releasing it is only
//! safe once every frame that sampled it has retired, so the renderer would
//! have to take ownership and defer destruction by the ring's depth, and a
//! target rendered and sampled in the same frame would need the sprite pass to
//! declare a read of an image it only knows by a caller's handle. The copy
//! costs one `cell` of bandwidth when an image is **registered** — a 256×128
//! icon is 128 KiB, once, on a cache miss — and in exchange:
//!
//! * **No lifetime is shared.** The atlas is created once and destroyed only
//!   with the renderer. The source is an ordinary graph image — a transient the
//!   pool retires on its own, or an import whose owner already follows the
//!   rule every import follows.
//! * **Every barrier is the graph's.** The copy is a graph copy pass writing the
//!   atlas as an import, and [`SpriteRenderer::add_pass`] declares a read of
//!   every atlas it samples, so the transfer-to-sampled transition is emitted
//!   where the graph says and not written by hand.
//! * **Many icons are one batch.** Every slot of an atlas is the same sheet, so
//!   a grid of icons from one atlas is one draw where one sheet per icon is one
//!   draw per icon.
//!
//! # When a slot may be released or overwritten
//!
//! **Any time between frames**, including while frames that sampled the old
//! texels are still in flight, and nothing waits for them to retire:
//!
//! * [`SpriteRenderer::free_slot`] destroys nothing. The cell's texels stay
//!   where they are until a later copy writes the cell.
//! * That later copy is a pass in a *later* submission on the same queue, and
//!   the graph's barrier out of [`ResourceState::ShaderRead`] into
//!   [`ResourceState::TransferDst`] orders it after every fragment read any
//!   earlier submission made. So a frame already submitted draws the texels it
//!   was recorded against, and the next frame to copy draws the new ones.
//!   `tests/sprite_e2e/sprite/atlas.rs` frees and refills a slot with the
//!   frame that drew it still unread and reads both frames back.
//! * A freed slot's [`AtlasSlot`] is refused from then on
//!   ([`SheetError::StaleSlot`]) by [`SpriteRenderer::free_slot`] and
//!   [`SpriteRenderer::add_slot_copies`], so a cache that frees twice or copies
//!   into a slot it gave back is told so rather than overwriting whoever holds
//!   the cell now.
//!
//! What **cannot** be enforced here: a [`Sprite`](super::Sprite) carries a
//! sheet and UVs, not a slot, so a sprite built from a slot that was since freed
//! draws whoever occupies the cell now. The caller that frees a slot drops every
//! key naming it in the same step — which is what a cache eviction is anyway.
//!
//! Within one frame, the copy writes the cell before the sprite pass samples it
//! **if [`SpriteRenderer::add_slot_copies`] is called before
//! [`SpriteRenderer::add_pass`]** — the graph runs passes in declaration order.
//! Called after, the frame draws the cell's previous texels and the next frame
//! the new ones; nothing is torn either way.
//!
//! **One queue.** The ordering above is the ordering of one queue. A caller
//! that ran the copy and the sprite pass on different queues would need a
//! semaphore between them, which nothing here records.
//!
//! # The gutter
//!
//! Cells sit [`GUTTER`] texel apart and that far from the atlas's edge, and no
//! copy ever writes the gutter, so it stays the transparent black the atlas was
//! created with. Linear filtering at a slot's edge reads half a texel outside
//! the cell; without the gutter that half texel is the neighbouring icon.

use crcbl_core::{Handle, Pool};
use crcbl_hal::{
    Extent3d, Format, HalError, ImageAspect, ImageCopy, ImageHandle, ImageSubresourceLayers,
    ImageUsage, Offset3d, ResourceState,
};
use crcbl_sprite::SampleMode;

use super::{SheetId, SpriteRenderer};
use crate::graph::{ImageId, ImportedImage, InitialClaim, RenderGraph};
use crate::texture::UploadedTexture;

/// Texels between two cells, and between a cell and the atlas's edge.
///
/// One is enough: a linear sample at a slot's UV edge reaches half a texel
/// outside it, and the gutter is what that half texel lands on. See the
/// [module docs](self).
pub const GUTTER: u32 = 1;

/// The one format an atlas is created in, and so the one a copy source must be.
///
/// The format every [`SpriteRenderer::register_sheet`] upload uses, for the
/// reason given there: the sampler decodes to linear and the blend happens in
/// linear light. A copy moves bytes, not colours, so a source in any other
/// format would arrive reinterpreted — which is why the copy refuses one
/// rather than converting it.
pub const ATLAS_FORMAT: Format = Format::Rgba8UnormSrgb;

/// An atlas to create: the size of one cell, how many of them, and how the
/// sheet is sampled.
///
/// A struct for the reason [`SheetDesc`](super::SheetDesc) is one: `columns`
/// and `rows` are two adjacent `u32`s a call site could swap.
#[derive(Clone, Copy, Debug)]
pub struct AtlasDesc<'a> {
    /// Names the image and its view.
    pub label: &'a str,
    /// One cell's size in texels, `(width, height)` — and therefore the exact
    /// extent every image copied into a slot must have.
    pub cell: (u32, u32),
    /// Cells across.
    pub columns: u32,
    /// Cells down.
    pub rows: u32,
    /// How sprites drawn from the atlas are sampled.
    pub sample: SampleMode,
}

/// The marker type an atlas cell's [`Handle`] names.
///
/// Uninhabited: a cell holds nothing on the host, and the pool it lives in is
/// there for its generations.
#[derive(Debug)]
enum Cell {}

/// One cell of an atlas, handed out by [`SpriteRenderer::allocate_slot`].
///
/// Generational: once [`SpriteRenderer::free_slot`] has given the cell back,
/// this value is refused everywhere it is accepted, even after the same cell is
/// handed out again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtlasSlot {
    sheet: SheetId,
    cell: Handle<Cell>,
    uv: [f32; 4],
}

impl AtlasSlot {
    /// The atlas this slot is a cell of — the sheet a [`Sprite`](super::Sprite)
    /// drawing it names.
    #[must_use]
    pub const fn sheet(&self) -> SheetId {
        self.sheet
    }

    /// The cell's normalised UVs, `[u0, v0, u1, v1]` top-left first — exactly
    /// the cell and none of the gutter around it.
    #[must_use]
    pub const fn uv(&self) -> [f32; 4] {
        self.uv
    }

    /// Which cell this is, row-major from the top-left, for diagnostics and
    /// tests. A freed and re-allocated slot can have the same index as the
    /// one it replaced; it never compares equal to it.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.cell.index()
    }
}

/// One image to copy into a slot this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotCopy {
    /// The rendered image, in the graph the copy is added to: a transient a
    /// render pass drew, or an import. Exactly the slot's cell size, in
    /// [`ATLAS_FORMAT`].
    pub source: ImageId,
    /// Where it goes.
    pub slot: AtlasSlot,
}

/// What an atlas operation refuses to do. Every variant is a bounded failure
/// the caller can recover from; nothing on this path panics or aborts.
#[derive(Debug, thiserror::Error)]
pub enum SheetError {
    /// Every cell is in use. Free one and ask again, or create a larger atlas —
    /// the image is created once, at its full size, and never grows.
    #[error("atlas {sheet:?} has all {capacity} of its cells in use")]
    AtlasFull {
        /// The atlas.
        sheet: SheetId,
        /// Its cells, all of them allocated.
        capacity: u32,
    },
    /// The sheet exists but is a [`register_sheet`](SpriteRenderer::register_sheet)
    /// upload, which has no cells, or it is not this renderer's at all.
    #[error("sheet {sheet:?} is not an atlas of this renderer")]
    NotAnAtlas {
        /// What was named.
        sheet: SheetId,
    },
    /// The slot was freed — and perhaps handed out again to someone else — so
    /// writing it or freeing it again would touch a cell it no longer owns.
    #[error("slot {index} of atlas {sheet:?} was freed; this handle no longer owns the cell")]
    StaleSlot {
        /// The atlas.
        sheet: SheetId,
        /// The cell the stale handle named.
        index: u32,
    },
    /// The source image is not what the slot holds: the wrong size, the wrong
    /// format, not a copy source, or not an image of this graph.
    #[error(
        "copy into slot {index} of atlas {sheet:?}: the source must be a {cell:?} {ATLAS_FORMAT:?} \
         image this graph can copy from, and {found}"
    )]
    SourceMismatch {
        /// The atlas.
        sheet: SheetId,
        /// The slot's cell.
        index: u32,
        /// The extent the source must have.
        cell: (u32, u32),
        /// What the source actually is.
        found: String,
    },
    /// Creating the atlas failed at the seam.
    #[error(transparent)]
    Hal(#[from] HalError),
}

/// An atlas's placement arithmetic and its cells' generations.
#[derive(Debug)]
pub(super) struct Atlas {
    pub(super) sheet: SheetId,
    pub(super) texture: UploadedTexture,
    pub(super) extent: (u32, u32),
    cell: (u32, u32),
    columns: u32,
    capacity: u32,
    cells: Pool<()>,
}

impl Atlas {
    /// The atlas image's size for `desc`, gutters included.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] for an empty cell or grid, or a size
    /// past `u32`. A size past the device's limit is the device's to refuse.
    pub(super) fn extent(desc: &AtlasDesc<'_>) -> Result<(u32, u32), HalError> {
        let (width, height) = desc.cell;
        if width == 0 || height == 0 || desc.columns == 0 || desc.rows == 0 {
            return Err(HalError::InvalidDescriptor(format!(
                "{}: an atlas needs a non-empty cell and at least one column and one row, \
                 not a {width}x{height} cell in {} x {}",
                desc.label, desc.columns, desc.rows
            )));
        }
        let side = |cell: u32, count: u32| {
            (cell.checked_add(GUTTER)?)
                .checked_mul(count)?
                .checked_add(GUTTER)
        };
        let capacity = desc.columns.checked_mul(desc.rows);
        match (side(width, desc.columns), side(height, desc.rows), capacity) {
            (Some(across), Some(down), Some(_)) => Ok((across, down)),
            _ => Err(HalError::InvalidDescriptor(format!(
                "{}: {} x {} cells of {width}x{height} is past what a u32 extent holds",
                desc.label, desc.columns, desc.rows
            ))),
        }
    }

    /// An atlas over an already-created `texture` of `extent`.
    pub(super) fn new(
        sheet: SheetId,
        texture: UploadedTexture,
        extent: (u32, u32),
        desc: &AtlasDesc<'_>,
    ) -> Self {
        let capacity = desc.columns * desc.rows;
        Self {
            sheet,
            texture,
            extent,
            cell: desc.cell,
            columns: desc.columns,
            capacity,
            cells: Pool::with_capacity(capacity as usize),
        }
    }

    /// Hands out a free cell.
    ///
    /// The bound counts retired cells as used, the rule
    /// [`crate::instance_pool`] keeps: a retired cell's generation is spent,
    /// and the pool would otherwise hand out an index past the grid in its
    /// place.
    pub(super) fn allocate(&mut self) -> Result<AtlasSlot, SheetError> {
        let used = self.cells.len() + self.cells.retired_slots();
        if used >= self.capacity as usize {
            return Err(SheetError::AtlasFull {
                sheet: self.sheet,
                capacity: self.capacity,
            });
        }
        let cell = self.cells.insert(()).cast::<Cell>();
        Ok(AtlasSlot {
            sheet: self.sheet,
            cell,
            uv: self.uv(cell.index()),
        })
    }

    /// Gives `slot`'s cell back.
    pub(super) fn free(&mut self, slot: AtlasSlot) -> Result<(), SheetError> {
        self.check(slot)?;
        // `check` just said the handle resolves, so this removes exactly it.
        self.cells.remove(slot.cell.cast::<()>());
        Ok(())
    }

    /// Whether `slot` still owns its cell.
    pub(super) fn check(&self, slot: AtlasSlot) -> Result<(), SheetError> {
        if self.cells.contains(slot.cell.cast::<()>()) {
            Ok(())
        } else {
            Err(SheetError::StaleSlot {
                sheet: self.sheet,
                index: slot.cell.index(),
            })
        }
    }

    /// The cell's top-left texel.
    fn origin(&self, index: u32) -> (u32, u32) {
        let (column, row) = (index % self.columns, index / self.columns);
        (
            GUTTER + column * (self.cell.0 + GUTTER),
            GUTTER + row * (self.cell.1 + GUTTER),
        )
    }

    /// The cell's normalised UVs, gutter excluded.
    fn uv(&self, index: u32) -> [f32; 4] {
        let (x, y) = self.origin(index);
        let (width, height) = (self.extent.0 as f32, self.extent.1 as f32);
        [
            x as f32 / width,
            y as f32 / height,
            (x + self.cell.0) as f32 / width,
            (y + self.cell.1) as f32 / height,
        ]
    }

    /// The atlas as an import.
    ///
    /// **One declaration for both importers**, [`SpriteRenderer::add_slot_copies`]
    /// and [`SpriteRenderer::add_pass`]: the graph merges two imports of one
    /// handle into one node and refuses them if they disagree. `ShaderRead` in
    /// and out, because that is what the creating upload left it in and what
    /// every graph that touches it leaves it in again — the ledger behind
    /// [`InitialClaim::Tracked`] checks exactly that.
    pub(super) fn import(&self, graph: &mut RenderGraph<'_>) -> ImageId {
        graph.import_image(
            "sprite atlas",
            ImportedImage {
                image: self.texture.image,
                view: self.texture.view,
                format: ATLAS_FORMAT,
                extent: self.extent,
                initial: ResourceState::ShaderRead,
                claim: InitialClaim::Tracked,
                final_state: ResourceState::ShaderRead,
            },
        )
    }

    /// Where `slot`'s texels go: the image, the cell's top-left texel, and the
    /// cell's size. The source half is the pass body's to fill in, because a
    /// transient has no handle until the graph executes.
    fn destination(&self, slot: AtlasSlot) -> (ImageHandle, (u32, u32), (u32, u32)) {
        (
            self.texture.image,
            self.origin(slot.cell.index()),
            self.cell,
        )
    }

    /// Refuses a source that is not exactly what `slot`'s cell holds.
    fn check_source(&self, graph: &RenderGraph<'_>, copy: &SlotCopy) -> Result<(), SheetError> {
        let mismatch = |found: String| SheetError::SourceMismatch {
            sheet: self.sheet,
            index: copy.slot.cell.index(),
            cell: self.cell,
            found,
        };
        let Some((format, extent, usage)) = graph.image_facts(copy.source) else {
            return Err(mismatch(format!(
                "{:?} is not an image of this graph",
                copy.source
            )));
        };
        if format != ATLAS_FORMAT || extent != self.cell {
            return Err(mismatch(format!("it is a {extent:?} {format:?} image")));
        }
        if usage.is_some_and(|usage| !usage.contains(ImageUsage::TRANSFER_SRC)) {
            return Err(mismatch(
                "it is a transient created without ImageUsage::TRANSFER_SRC".to_string(),
            ));
        }
        Ok(())
    }
}

impl SpriteRenderer {
    /// Creates an atlas — a sheet of `desc.columns × desc.rows` cells, every
    /// texel transparent — and returns the id sprites drawing from it name.
    ///
    /// A startup path like [`register_sheet`](Self::register_sheet): the
    /// clear is a staging copy that blocks on `wait_idle`, and it must not be
    /// called between `begin_frame` and the graph's `execute`. Slots are then
    /// allocated, filled and freed per frame with no further blocking. See the
    /// [atlas module](self) for when a slot may be freed or overwritten.
    ///
    /// # Errors
    ///
    /// [`SheetError::Hal`] for an empty cell or grid, a size past `u32` or
    /// past the device's limit, or any seam call. A failure leaves nothing
    /// behind and no id allocated.
    pub fn create_atlas(
        &mut self,
        device: &dyn crcbl_hal::Device,
        desc: &AtlasDesc<'_>,
    ) -> Result<SheetId, SheetError> {
        let extent = Atlas::extent(desc)?;
        let texture = crate::texture::upload_cleared_texture(
            device,
            self.queue,
            &crate::texture::ClearedTextureDesc {
                label: desc.label,
                format: ATLAS_FORMAT,
                width: extent.0,
                height: extent.1,
                layers: 1,
                view_type: crcbl_hal::ImageViewType::D2,
                patches: &[],
            },
        )?;
        let sheet = self.adopt(device, desc.label, texture, extent, desc.sample)?;
        self.atlases.push(Atlas::new(sheet, texture, extent, desc));
        Ok(sheet)
    }

    /// Hands out a free cell of `atlas`.
    ///
    /// # Errors
    ///
    /// [`SheetError::AtlasFull`] when every cell is in use — the bounded
    /// failure a full atlas is; nothing grows and nothing is evicted behind
    /// the caller's back. [`SheetError::NotAnAtlas`] for a sheet that is not
    /// one of this renderer's atlases.
    pub fn allocate_slot(&mut self, atlas: SheetId) -> Result<AtlasSlot, SheetError> {
        self.atlas_mut(atlas)?.allocate()
    }

    /// Gives `slot`'s cell back for the next [`allocate_slot`](Self::allocate_slot).
    ///
    /// **Destroys nothing and waits for nothing**, and is safe while frames that
    /// sampled the cell are still in flight — the [atlas module](self)
    /// says why. From here on `slot` is refused everywhere.
    ///
    /// # Errors
    ///
    /// [`SheetError::StaleSlot`] for a slot already freed, and
    /// [`SheetError::NotAnAtlas`] for one naming another renderer's atlas.
    pub fn free_slot(&mut self, slot: AtlasSlot) -> Result<(), SheetError> {
        self.atlas_mut(slot.sheet)?.free(slot)
    }

    /// Adds one copy pass to `graph` that writes each `copies` source into its
    /// slot's cell.
    ///
    /// Call it **before** [`add_pass`](Self::add_pass) for this frame's sprites
    /// to see the new texels. A source is typically a transient a render pass
    /// earlier in the same graph drew; the graph moves it from wherever that
    /// pass left it into `TransferSrc`, and the atlas from `ShaderRead` into
    /// `TransferDst` and back.
    ///
    /// Every copy is checked before anything is added, so a refused call adds
    /// no pass at all. An empty `copies` adds nothing.
    ///
    /// # Errors
    ///
    /// [`SheetError::StaleSlot`] or [`SheetError::NotAnAtlas`] for a slot this
    /// renderer does not currently own, and [`SheetError::SourceMismatch`] for
    /// a source that is not exactly the slot's cell size in [`ATLAS_FORMAT`],
    /// is a transient without `TRANSFER_SRC`, or is not an image of `graph`.
    pub fn add_slot_copies(
        &self,
        graph: &mut RenderGraph<'_>,
        copies: &[SlotCopy],
    ) -> Result<(), SheetError> {
        for copy in copies {
            let atlas = self.atlas(copy.slot.sheet)?;
            atlas.check(copy.slot)?;
            atlas.check_source(graph, copy)?;
        }
        if copies.is_empty() {
            return Ok(());
        }

        let mut sources: Vec<ImageId> = Vec::new();
        let mut targets: Vec<ImageId> = Vec::new();
        let mut planned = Vec::with_capacity(copies.len());
        for copy in copies {
            let atlas = self.atlas(copy.slot.sheet)?;
            let target = atlas.import(graph);
            // One declaration per image: a pass naming the same image twice is
            // one access, and the graph is not asked to reconcile repeats.
            if !sources.contains(&copy.source) {
                sources.push(copy.source);
            }
            if !targets.contains(&target) {
                targets.push(target);
            }
            planned.push((copy.source, atlas.destination(copy.slot)));
        }

        let mut pass = graph.add_copy_pass("sprite atlas copies");
        for &source in &sources {
            pass = pass.use_image(source, ResourceState::TransferSrc);
        }
        for &target in &targets {
            pass = pass.use_image(target, ResourceState::TransferDst);
        }
        pass.execute(move |ctx| {
            for (source, (atlas, origin, cell)) in planned {
                let copy = cell_copy(ctx.image(source), atlas, origin, cell);
                ctx.encoder().copy_image_to_image(&copy);
            }
        });
        Ok(())
    }

    /// The atlas behind `sheet`, or why there is none.
    fn atlas(&self, sheet: SheetId) -> Result<&Atlas, SheetError> {
        self.atlases
            .iter()
            .find(|atlas| atlas.sheet == sheet)
            .ok_or(SheetError::NotAnAtlas { sheet })
    }

    /// [`atlas`](Self::atlas), mutably.
    fn atlas_mut(&mut self, sheet: SheetId) -> Result<&mut Atlas, SheetError> {
        self.atlases
            .iter_mut()
            .find(|atlas| atlas.sheet == sheet)
            .ok_or(SheetError::NotAnAtlas { sheet })
    }
}

/// A whole-cell copy from the top-left of `source` to `origin` in `atlas`.
fn cell_copy(
    source: ImageHandle,
    atlas: ImageHandle,
    origin: (u32, u32),
    cell: (u32, u32),
) -> ImageCopy {
    let layers = ImageSubresourceLayers {
        aspect: ImageAspect::COLOR,
        mip: 0,
        base_layer: 0,
        layer_count: 1,
    };
    ImageCopy {
        src: source,
        src_subresource: layers,
        src_offset: Offset3d::default(),
        dst: atlas,
        dst_subresource: layers,
        dst_offset: Offset3d {
            x: origin.0 as i32,
            y: origin.1 as i32,
            z: 0,
        },
        extent: Extent3d::d2(cell.0, cell.1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprite_pass::Sprite;
    use crate::sprite_pass::tests::{open, target};
    use crate::transient::{TransientImageDesc, TransientPool};
    use crcbl_hal::null::{Command, ObjectKind, Recorder};
    use crcbl_hal::{CommandEncoderDesc, Device, QueueHandle};

    const CELL: (u32, u32) = (4, 2);

    const SOURCE_USAGE: ImageUsage = ImageUsage::COLOR_ATTACHMENT.union(ImageUsage::TRANSFER_SRC);

    fn desc(columns: u32, rows: u32) -> AtlasDesc<'static> {
        AtlasDesc {
            label: "test atlas",
            cell: CELL,
            columns,
            rows,
            sample: SampleMode::Smooth,
        }
    }

    fn renderer(device: &dyn Device, queue: QueueHandle) -> SpriteRenderer {
        SpriteRenderer::new(device, queue, Format::Bgra8UnormSrgb)
            .expect("the null backend builds the pass")
    }

    /// A transient in `graph` of `extent` in the atlas's format.
    fn source(graph: &mut RenderGraph<'_>, extent: (u32, u32), usage: ImageUsage) -> ImageId {
        graph.create_image(
            "rendered icon",
            TransientImageDesc::new(extent, ATLAS_FORMAT, usage),
        )
    }

    /// Runs one frame — a clear into a cell-sized transient, its copy into
    /// `slot`, and a sprite drawing `slot` — and returns what it recorded.
    fn frame(
        device: &dyn Device,
        queue: QueueHandle,
        recorder: &Recorder,
        renderer: &mut SpriteRenderer,
        slot: AtlasSlot,
    ) -> Vec<Command> {
        let sprites = [Sprite::new(slot.sheet(), [0.0, 0.0, 1.0, 1.0], slot.uv())];
        renderer
            .begin_frame(device, &sprites, glam::Mat4::IDENTITY, (256, 192))
            .expect("the ring is writable");
        let before = recorder.commands().len();
        let imported = target(device);
        let mut pool = TransientPool::new();
        {
            let mut graph = RenderGraph::new(queue);
            let swap = graph.import_image("swapchain", imported);
            let icon = source(&mut graph, CELL, SOURCE_USAGE);
            graph
                .add_render_pass("icon")
                .clear_color(icon, [1.0, 0.0, 0.0, 1.0])
                .execute(|_| {});
            renderer
                .add_slot_copies(&mut graph, &[SlotCopy { source: icon, slot }])
                .expect("a cell-sized source copies");
            renderer.add_pass(&mut graph, swap);
            let compiled = graph.compile(&pool).expect("a legal frame");
            let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
                label: Some("atlas frame"),
                queue,
            });
            compiled
                .execute(device, &mut pool, encoder.as_mut(), None)
                .expect("execution succeeded");
            let commands = encoder.finish().expect("recording succeeded");
            device.destroy_command_buffer(commands);
        }
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
        recorder.commands().split_off(before)
    }

    /// **A full atlas is an error the caller gets back, not a panic**, and it
    /// is bounded: freeing one cell makes exactly one more allocation succeed.
    #[test]
    fn a_full_atlas_refuses_the_next_slot_and_recovers_when_one_is_freed() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = renderer(device.as_ref(), queue);
        let atlas = renderer
            .create_atlas(device.as_ref(), &desc(2, 1))
            .expect("the atlas is created");

        let first = renderer.allocate_slot(atlas).expect("cell 0 is free");
        let second = renderer.allocate_slot(atlas).expect("cell 1 is free");
        assert_ne!(first.index(), second.index());
        match renderer.allocate_slot(atlas) {
            Err(SheetError::AtlasFull { sheet, capacity }) => {
                assert_eq!(sheet, atlas);
                assert_eq!(capacity, 2);
            }
            other => panic!("a third slot in a two-cell atlas must be AtlasFull, got {other:?}"),
        }

        renderer.free_slot(first).expect("the slot is live");
        let again = renderer
            .allocate_slot(atlas)
            .expect("the freed cell is handed out again");
        assert_eq!(
            again.index(),
            first.index(),
            "the freed cell, not a new one"
        );
        assert!(
            matches!(
                renderer.allocate_slot(atlas),
                Err(SheetError::AtlasFull { .. })
            ),
            "and the atlas is full again"
        );
        renderer.destroy(device.as_ref());
    }

    /// A freed slot is refused everywhere — a second free, and a copy — even
    /// after its cell has been handed to someone else, and the new owner's slot
    /// is not equal to it.
    #[test]
    fn a_freed_slot_is_stale_even_after_its_cell_is_reused() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = renderer(device.as_ref(), queue);
        let atlas = renderer
            .create_atlas(device.as_ref(), &desc(1, 1))
            .expect("the atlas is created");
        let old = renderer.allocate_slot(atlas).expect("a free cell");
        renderer.free_slot(old).expect("the slot is live");
        let new = renderer.allocate_slot(atlas).expect("the cell again");
        assert_eq!(old.index(), new.index());
        assert_ne!(old, new, "a reused cell's slot is a different slot");

        assert!(matches!(
            renderer.free_slot(old),
            Err(SheetError::StaleSlot { index: 0, .. })
        ));
        let mut graph = RenderGraph::new(queue);
        let icon = source(&mut graph, CELL, SOURCE_USAGE);
        let copy = SlotCopy {
            source: icon,
            slot: old,
        };
        assert!(matches!(
            renderer.add_slot_copies(&mut graph, &[copy]),
            Err(SheetError::StaleSlot { .. })
        ));
        assert_eq!(graph.pass_count(), 0, "a refused copy adds no pass");
        drop(graph);
        renderer.destroy(device.as_ref());
    }

    /// An uploaded sheet has no cells, and a source that is not the cell's
    /// size or usage is refused before any pass is added.
    #[test]
    fn a_source_that_is_not_the_cell_is_refused_before_anything_is_added() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = renderer(device.as_ref(), queue);
        let pixels = [255u8; 2 * 2 * 4];
        let sheet = renderer
            .register_sheet(
                device.as_ref(),
                &super::super::SheetDesc {
                    label: "uploaded",
                    width: 2,
                    height: 2,
                    sample: SampleMode::Pixel,
                    pixels: &pixels,
                },
            )
            .expect("the upload succeeds");
        assert!(matches!(
            renderer.allocate_slot(sheet),
            Err(SheetError::NotAnAtlas { .. })
        ));

        let atlas = renderer
            .create_atlas(device.as_ref(), &desc(1, 1))
            .expect("the atlas is created");
        let slot = renderer.allocate_slot(atlas).expect("a free cell");
        let mut graph = RenderGraph::new(queue);
        let too_big = source(&mut graph, (CELL.0 + 1, CELL.1), SOURCE_USAGE);
        let no_copy = source(&mut graph, CELL, ImageUsage::COLOR_ATTACHMENT);
        for bad in [too_big, no_copy] {
            let copy = SlotCopy { source: bad, slot };
            let refused = renderer.add_slot_copies(&mut graph, &[copy]);
            assert!(
                matches!(refused, Err(SheetError::SourceMismatch { .. })),
                "{refused:?}"
            );
        }
        assert_eq!(graph.pass_count(), 0, "a refused copy adds no pass");
        drop(graph);
        renderer.destroy(device.as_ref());
    }

    /// The cell arithmetic: a one-texel gutter round every cell, and UVs that
    /// cover the cell and none of the gutter.
    #[test]
    fn slots_sit_inside_a_one_texel_gutter() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = renderer(device.as_ref(), queue);
        assert_eq!(Atlas::extent(&desc(2, 1)).expect("a valid grid"), (11, 4));
        let atlas = renderer
            .create_atlas(device.as_ref(), &desc(2, 1))
            .expect("the atlas is created");
        let first = renderer.allocate_slot(atlas).expect("cell 0");
        let second = renderer.allocate_slot(atlas).expect("cell 1");
        assert_eq!(first.uv(), [1.0 / 11.0, 0.25, 5.0 / 11.0, 0.75]);
        assert_eq!(second.uv(), [6.0 / 11.0, 0.25, 10.0 / 11.0, 0.75]);
        assert!(matches!(
            renderer.create_atlas(device.as_ref(), &desc(0, 1)),
            Err(SheetError::Hal(HalError::InvalidDescriptor(_)))
        ));
        renderer.destroy(device.as_ref());
    }

    /// **The copy lands in the slot's cell, and the sprite pass samples it only
    /// after the graph has moved the atlas back to `ShaderRead`.**
    ///
    /// Without `add_pass`'s read declaration the copy still records, but the
    /// atlas's return to `ShaderRead` becomes the graph's *trailing* barrier —
    /// after the draw — and the fragment stage samples an image still in
    /// `TransferDst`.
    #[test]
    fn a_copied_cell_is_barriered_back_to_sampled_before_the_draw() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = renderer(device.as_ref(), queue);
        let atlas = renderer
            .create_atlas(device.as_ref(), &desc(2, 1))
            .expect("the atlas is created");
        let image = renderer.atlases[0].texture.image;
        let _first = renderer.allocate_slot(atlas).expect("cell 0");
        let second = renderer.allocate_slot(atlas).expect("cell 1");

        let commands = frame(device.as_ref(), queue, &recorder, &mut renderer, second);
        let position = |wanted: &dyn Fn(&Command) -> bool| {
            commands
                .iter()
                .position(wanted)
                .unwrap_or_else(|| panic!("missing from {commands:#?}"))
        };
        // Cell 1 starts past the leading gutter, cell 0 and the gutter after it.
        let origin = Offset3d {
            x: (GUTTER + CELL.0 + GUTTER) as i32,
            y: GUTTER as i32,
            z: 0,
        };
        let copy = position(&|command| {
            matches!(command, Command::CopyImageToImage(copy)
                if copy.dst == image
                    && copy.dst_offset == origin
                    && copy.extent == Extent3d::d2(CELL.0, CELL.1))
        });
        let sampled = position(&|command| {
            matches!(command, Command::Barrier { images, .. }
                if images.iter().any(|barrier| barrier.image == image
                    && barrier.from == ResourceState::TransferDst
                    && barrier.to == ResourceState::ShaderRead))
        });
        let draw = position(&|command| matches!(command, Command::Draw { .. }));
        assert!(
            copy < sampled && sampled < draw,
            "copy at {copy}, back to ShaderRead at {sampled}, draw at {draw}"
        );
        renderer.destroy(device.as_ref());
    }

    /// **Freeing a slot while a frame that sampled it is in flight destroys
    /// nothing**, so there is nothing for that frame to lose: the atlas image
    /// lives until the renderer is destroyed, and refilling the cell is a
    /// later, queue-ordered copy. `tests/sprite_e2e/sprite/atlas.rs` reads
    /// both frames back on a real device.
    #[test]
    fn freeing_and_refilling_a_slot_destroys_no_image() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = renderer(device.as_ref(), queue);
        let atlas = renderer
            .create_atlas(device.as_ref(), &desc(1, 1))
            .expect("the atlas is created");
        let slot = renderer.allocate_slot(atlas).expect("a free cell");
        frame(device.as_ref(), queue, &recorder, &mut renderer, slot);

        let images = recorder.live_objects(ObjectKind::Image);
        renderer.free_slot(slot).expect("the slot is live");
        let refilled = renderer.allocate_slot(atlas).expect("the cell again");
        let commands = frame(device.as_ref(), queue, &recorder, &mut renderer, refilled);
        assert_eq!(
            recorder.live_objects(ObjectKind::Image),
            images,
            "a free and a refill neither create nor destroy an image"
        );
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, Command::CopyImageToImage(_))),
            "the refill is a copy in the next frame"
        );
        renderer.destroy(device.as_ref());
        assert_eq!(
            recorder.live_objects(ObjectKind::Image),
            0,
            "destroy releases the atlas"
        );
    }
}
