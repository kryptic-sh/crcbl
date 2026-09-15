//! What each argument-table slot of the open encoder already holds, so a
//! `set*` that would change nothing is never made.
//!
//! # The finding this exists for
//!
//! Metal's debug layer answers a `set*` whose argument equals what the slot
//! already holds with `redundant setting of <object>`, and prints a descriptor
//! dump of the object under it. [`crate::binding`]'s
//! [`apply`](crate::binding::apply) re-sets every binding of a group at every
//! bind and [`crate::command`]'s `push_constants` re-sends the whole block at
//! every write, so a pass that binds one material for a hundred draws makes a
//! hundred identical calls — a message send each, and with
//! `MTL_DEBUG_LAYER_WARNING_MODE=nslog` a paragraph of log each.
//! `docs/notes/backends.md` carries the counts that were measured.
//!
//! Every Metal backend carries this cache and for this reason: wgpu-hal's Metal
//! `CommandState` and MoltenVK's `MVKResourcesCommandEncoderState` are the two
//! this one was modelled on.
//!
//! # Per encoder, and only per encoder
//!
//! Metal's argument tables belong to the `MTLCommandEncoder` rather than to the
//! command buffer: `endEncoding` takes them with it and the next encoder starts
//! with every slot empty. So a cache lives exactly as long as the encoder it
//! mirrors, which is what [`crate::command`] does with the two it keeps — one
//! on the stack of `encode_render_pass` beside the render encoder, and one on
//! the command encoder for the compute pass, cleared by `close_open` with the
//! rest of the state an encoder takes with it.
//!
//! **A pipeline bind does not empty the tables**, which is why the cache
//! survives one: `setRenderPipelineState:` and `setComputePipelineState:`
//! change what the argument tables are *read* as, not what they hold.
//!
//! # Not macOS-only, and that is the point
//!
//! This module holds no Objective-C type — a slot is an integer and a resource
//! is an address — so off macOS it exists in the test build alone and
//! `cargo test` on any host checks it, exactly as [`crate::argument`],
//! [`crate::pass`], [`crate::present`], [`crate::query`] and [`crate::quirk`]
//! are compiled for.
//!
//! That matters here because **a wrong answer is silent**. A cache that reports
//! a hit for a slot holding something else drops a bind, and the draw then
//! reads the resource the previous bind left there: no Metal call fails, no
//! layer reports it, and the only evidence is a wrong pixel. The Metal tests
//! that would catch it are behind the `mtl-e2e` feature and `#[ignore]`, so
//! they run under `tests/run-mtl-e2e.sh` and nowhere else — the host test is
//! the check this logic gets on every push.

/// The identity of a Metal object, as the address of the object itself.
///
/// An address rather than a `Retained` for the reason the module header gives:
/// this type is compiled and tested on hosts that have no Objective-C runtime.
///
/// # Why an address cannot be reused under the cache
///
/// A freed object's address can be handed to the next allocation, which would
/// make a stale entry match a different resource. It cannot happen here,
/// because an address only enters the cache when the argument it identifies is
/// passed to a `set*` on an encoder of this command buffer — and, as
/// [`crate::device`]'s header states in the course of explaining why this
/// backend has no deletion queue, an `MTLCommandBuffer` retains every resource
/// it references. The object therefore outlives the command buffer's
/// completion, which is well past the `endEncoding` that empties this cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResourceId(usize);

impl ResourceId {
    /// The identity of the object at `address`.
    pub(crate) const fn new(address: usize) -> Self {
        Self(address)
    }
}

/// Which of Metal's argument-table sets a call writes.
///
/// A render encoder has one set per raster stage and a separate selector for
/// each — `setVertexBuffer:offset:atIndex:` and `setFragmentBuffer:…` — while a
/// compute encoder has one unqualified set. They are three independent tables,
/// so a slot number means nothing without one of these beside it. Object and
/// mesh stages each have another independent set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    Vertex,
    Fragment,
    Compute,
    Object,
    Mesh,
}

impl Stage {
    /// How many stages [`BindCache`] holds a table set for, and
    /// [`BindingMask`](crate::binding_mask::BindingMask) a set of used slots
    /// for.
    pub(crate) const COUNT: usize = 5;

    /// This stage's index into [`BindCache::stages`], and into the mask beside
    /// it.
    pub(crate) const fn slot(self) -> usize {
        self as usize
    }
}

/// One entry of a stage's **buffer** table.
///
/// Two spellings share the table because Metal's two calls do:
/// `setBuffer:offset:atIndex:` and `setBytes:length:atIndex:` write the same
/// argument-table entry, so a pipeline layout that puts its push-constant block
/// at the index a previous layout bound a buffer at must not find the buffer's
/// entry there and take it for its own.
#[derive(Clone, Debug, PartialEq, Eq)]
enum BufferArgument {
    /// `setBuffer:offset:atIndex:` — the buffer and the offset it was bound at.
    Bound { id: ResourceId, offset: u64 },
    /// `setBytes:length:atIndex:` — the bytes themselves, because that call
    /// copies its argument into the encoder and leaves no object to key on.
    Inline(Vec<u8>),
}

/// The three argument tables one stage has.
#[derive(Debug, Default)]
struct StageCache {
    buffers: Vec<Option<BufferArgument>>,
    textures: Vec<Option<ResourceId>>,
    samplers: Vec<Option<ResourceId>>,
}

/// What the open encoder's argument tables hold.
#[derive(Debug, Default)]
pub(crate) struct BindCache {
    stages: [StageCache; Stage::COUNT],
}

impl BindCache {
    /// Empties every table, for an encoder that has ended.
    ///
    /// The state this forgets is the encoder's, not the command buffer's; see
    /// the module header. Keeping it across `endEncoding` would let the next
    /// encoder skip a bind it has never made.
    pub(crate) fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.buffers.clear();
            stage.textures.clear();
            stage.samplers.clear();
        }
    }

    /// Whether `setBuffer:offset:atIndex:` at this slot would change what is
    /// bound — and records the argument either way, so the answer is `false`
    /// from the second identical call onward.
    pub(crate) fn buffer_changed(
        &mut self,
        stage: Stage,
        slot: u32,
        id: ResourceId,
        offset: u64,
    ) -> bool {
        let held = entry(&mut self.stages[stage.slot()].buffers, slot);
        replace(held, BufferArgument::Bound { id, offset })
    }

    /// [`buffer_changed`](Self::buffer_changed) for
    /// `setBytes:length:atIndex:`, which shares the buffer table.
    ///
    /// **The bytes are the argument.** `setBytes:` copies what it is given into
    /// the encoder, so two calls are interchangeable exactly when their blocks
    /// are equal — the same length and the same contents — and a skip is then a
    /// copy of identical bytes not made. The comparison is what a redundant
    /// push-constant write costs here, and it is a `memcmp` over a block
    /// `crate::argument`'s `plan` bounded by `Limits::max_push_constant_size`
    /// against the message send and the layer's descriptor dump it saves.
    pub(crate) fn bytes_changed(&mut self, stage: Stage, slot: u32, bytes: &[u8]) -> bool {
        let held = entry(&mut self.stages[stage.slot()].buffers, slot);
        if let Some(BufferArgument::Inline(inline)) = held
            && inline.as_slice() == bytes
        {
            return false;
        }
        *held = Some(BufferArgument::Inline(bytes.to_vec()));
        true
    }

    /// [`buffer_changed`](Self::buffer_changed) for
    /// `setTexture:atIndex:`, on the texture table and with no offset.
    pub(crate) fn texture_changed(&mut self, stage: Stage, slot: u32, id: ResourceId) -> bool {
        replace(entry(&mut self.stages[stage.slot()].textures, slot), id)
    }

    /// [`buffer_changed`](Self::buffer_changed) for
    /// `setSamplerState:atIndex:`, on the sampler table.
    pub(crate) fn sampler_changed(&mut self, stage: Stage, slot: u32, id: ResourceId) -> bool {
        replace(entry(&mut self.stages[stage.slot()].samplers, slot), id)
    }
}

/// The entry for `slot`, growing the table to reach it.
///
/// Grown on demand rather than sized up front because a stage's tables are
/// filled from the bottom and a pass that binds three slots has no use for the
/// rest. Growth is bounded by the same thing the `set*` calls' own safety
/// arguments rest on: `plan_layout` refuses a pipeline layout whose sets overrun
/// `Table::capacity`, and `crate::argument`'s `plan` bounds the push-constant
/// block's index by `BUFFER_TABLE_ENTRIES`, so no slot reaching this is larger
/// than the largest of Metal's three tables.
fn entry<T: Default>(table: &mut Vec<T>, slot: u32) -> &mut T {
    let slot = slot as usize;
    if slot >= table.len() {
        table.resize_with(slot + 1, T::default);
    }
    &mut table[slot]
}

/// Puts `value` in an entry and says whether it differed from what was there.
fn replace<T: PartialEq>(held: &mut Option<T>, value: T) -> bool {
    if held.as_ref() == Some(&value) {
        return false;
    }
    *held = Some(value);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ids that are distinct and nothing else; the cache reads no memory at
    /// them, which is the whole reason it is testable off macOS.
    const FIRST: ResourceId = ResourceId::new(0x1000);
    const SECOND: ResourceId = ResourceId::new(0x2000);

    /// The first bind of a slot always makes the call: an encoder opens with
    /// every table entry empty, so there is nothing there to match.
    #[test]
    fn the_first_bind_of_a_slot_is_always_made() {
        let mut cache = BindCache::default();
        assert!(cache.buffer_changed(Stage::Vertex, 0, FIRST, 0));
        assert!(cache.texture_changed(Stage::Vertex, 0, FIRST));
        assert!(cache.sampler_changed(Stage::Vertex, 0, FIRST));
        assert!(cache.bytes_changed(Stage::Vertex, 1, &[1, 2, 3]));
    }

    /// The finding this module exists for: the same argument at the same slot,
    /// which is what a group re-bound between two draws is.
    #[test]
    fn the_same_argument_twice_makes_one_call() {
        let mut cache = BindCache::default();
        assert!(cache.buffer_changed(Stage::Fragment, 4, FIRST, 256));
        assert!(!cache.buffer_changed(Stage::Fragment, 4, FIRST, 256));
        assert!(cache.texture_changed(Stage::Fragment, 7, FIRST));
        assert!(!cache.texture_changed(Stage::Fragment, 7, FIRST));
        assert!(cache.sampler_changed(Stage::Fragment, 2, FIRST));
        assert!(!cache.sampler_changed(Stage::Fragment, 2, FIRST));
    }

    /// A different resource at the same slot is a real change, and so is the
    /// same buffer at a different offset — which is the whole of a dynamic
    /// offset, and the one an id-only cache would drop.
    #[test]
    fn a_different_resource_or_offset_is_a_call() {
        let mut cache = BindCache::default();
        assert!(cache.buffer_changed(Stage::Compute, 3, FIRST, 0));
        assert!(cache.buffer_changed(Stage::Compute, 3, SECOND, 0));
        assert!(cache.buffer_changed(Stage::Compute, 3, SECOND, 64));
        assert!(!cache.buffer_changed(Stage::Compute, 3, SECOND, 64));
        assert!(cache.texture_changed(Stage::Compute, 3, FIRST));
        assert!(cache.texture_changed(Stage::Compute, 3, SECOND));
    }

    /// A slot number means nothing without a stage: Metal's three sets of
    /// tables are independent, and a vertex bind must not answer for the
    /// fragment stage's slot of the same number.
    #[test]
    fn the_stages_hold_separate_tables() {
        let mut cache = BindCache::default();
        assert!(cache.texture_changed(Stage::Vertex, 5, FIRST));
        assert!(cache.texture_changed(Stage::Fragment, 5, FIRST));
        assert!(cache.texture_changed(Stage::Compute, 5, FIRST));
        assert!(cache.texture_changed(Stage::Object, 5, FIRST));
        assert!(cache.texture_changed(Stage::Mesh, 5, FIRST));
        assert!(!cache.texture_changed(Stage::Vertex, 5, FIRST));
        assert!(!cache.texture_changed(Stage::Fragment, 5, FIRST));
        assert!(!cache.texture_changed(Stage::Compute, 5, FIRST));
        assert!(!cache.texture_changed(Stage::Object, 5, FIRST));
        assert!(!cache.texture_changed(Stage::Mesh, 5, FIRST));
    }

    /// And neither does a slot number mean anything without its table: the
    /// buffer, texture and sampler tables are three, and index `0` of each is a
    /// different argument.
    #[test]
    fn the_three_tables_do_not_alias() {
        let mut cache = BindCache::default();
        assert!(cache.buffer_changed(Stage::Vertex, 0, FIRST, 0));
        assert!(cache.texture_changed(Stage::Vertex, 0, FIRST));
        assert!(cache.sampler_changed(Stage::Vertex, 0, FIRST));
    }

    /// `setBytes:` copies, so equal blocks are interchangeable and a longer
    /// block with the same prefix is not.
    #[test]
    fn an_inline_block_is_compared_by_its_bytes() {
        let mut cache = BindCache::default();
        assert!(cache.bytes_changed(Stage::Vertex, 6, &[1, 2, 3, 4]));
        assert!(!cache.bytes_changed(Stage::Vertex, 6, &[1, 2, 3, 4]));
        assert!(cache.bytes_changed(Stage::Vertex, 6, &[1, 2, 3, 5]));
        assert!(cache.bytes_changed(Stage::Vertex, 6, &[1, 2, 3, 5, 6]));
        assert!(cache.bytes_changed(Stage::Vertex, 6, &[1, 2, 3, 5]));
    }

    /// `setBuffer:` and `setBytes:` write one table, so neither may find the
    /// other's entry and take it for its own. A layout whose push-constant
    /// block lands where the last pipeline bound a buffer is exactly this.
    #[test]
    fn an_inline_block_and_a_buffer_share_one_slot() {
        let mut cache = BindCache::default();
        assert!(cache.buffer_changed(Stage::Fragment, 8, FIRST, 0));
        assert!(cache.bytes_changed(Stage::Fragment, 8, &[7; 16]));
        assert!(cache.buffer_changed(Stage::Fragment, 8, FIRST, 0));
        assert!(cache.bytes_changed(Stage::Fragment, 8, &[7; 16]));
    }

    /// `endEncoding` empties Metal's argument tables, so the cache that mirrors
    /// them must empty too — a bind the *next* encoder has never made is one it
    /// must still make.
    #[test]
    fn a_reset_forgets_every_stage_and_table() {
        let mut cache = BindCache::default();
        assert!(cache.buffer_changed(Stage::Vertex, 0, FIRST, 0));
        assert!(cache.bytes_changed(Stage::Fragment, 1, &[9]));
        assert!(cache.texture_changed(Stage::Fragment, 2, FIRST));
        assert!(cache.sampler_changed(Stage::Compute, 3, FIRST));
        cache.reset();
        assert!(cache.buffer_changed(Stage::Vertex, 0, FIRST, 0));
        assert!(cache.bytes_changed(Stage::Fragment, 1, &[9]));
        assert!(cache.texture_changed(Stage::Fragment, 2, FIRST));
        assert!(cache.sampler_changed(Stage::Compute, 3, FIRST));
    }

    /// A slot far up a table is reached without disturbing the ones below it,
    /// which is what growing on demand has to get right.
    #[test]
    fn a_high_slot_leaves_the_low_ones_alone() {
        let mut cache = BindCache::default();
        assert!(cache.texture_changed(Stage::Fragment, 0, FIRST));
        assert!(cache.texture_changed(Stage::Fragment, 127, SECOND));
        assert!(!cache.texture_changed(Stage::Fragment, 0, FIRST));
        assert!(!cache.texture_changed(Stage::Fragment, 127, SECOND));
        assert!(cache.texture_changed(Stage::Fragment, 63, FIRST));
    }
}
