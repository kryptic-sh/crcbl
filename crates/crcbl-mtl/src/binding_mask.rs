//! Metal's three per-stage argument tables, and which of their slots a compiled
//! pipeline actually reads.
//!
//! # The finding this exists for
//!
//! Under `MTL_DEBUG_LAYER_WARNING_MODE=nslog` the mesh suite's Metal jobs print
//! two complaints per draw, and they are opposite halves of one question:
//!
//! * `unused binding in encoder at Buffer index N` — the encoder holds an
//!   argument the bound pipeline does not read. `crcbl_render::forward`'s mesh
//!   layout declares its material table, light list, froxel grid and probe rows
//!   visible to the geometry stage *and* the fragment stage, so
//!   [`crate::binding`]'s `apply` sets each on both tables; only one of them
//!   reads it.
//! * `missing Buffer binding at index N for <function>` — the pipeline declares
//!   an argument nothing is bound at. Slang's Metal emission materialises every
//!   module global into every entry point, so `msl/mesh.metal`'s `fragmentMain`
//!   takes `draw`, `meshes`, `visible_instances`, `vertices` and `instances`
//!   whether its body reads them or not.
//!
//! A layout cannot answer either one: it says which stages a binding is
//! *permitted* to reach, and Slang's materialisation means the permission and
//! the read are different sets. The compiled pipeline is what knows, and Metal
//! hands it over — `MTLPipelineOption::BindingInfo` fills an
//! `MTLRenderPipelineReflection`/`MTLComputePipelineReflection` whose
//! `MTLBinding`s carry `index` and `isUsed`. This
//! module is that answer reduced to a bitmask, and [`crate::binding`] ANDs it
//! into the stage predicates so a bind the pipeline cannot read is never made.
//!
//! # Never bind *less* than is known
//!
//! Every fallback in this module is [`BindingMask::all`]. A mask that wrongly
//! says "used" costs one redundant `set*` and a line of log; a mask that
//! wrongly says "unused" drops a bind, the draw reads whatever the previous
//! bind left in that slot, no Metal call fails and no layer reports it — the
//! same silent-wrong-pixel failure [`crate::bind_cache`] is written against. So
//! reflection that comes back absent, a binding type with no argument table
//! behind it, and an index past the mask's width all mean "bind it".
//!
//! # Plain Rust, so every host runs it
//!
//! There is no Objective-C here — a table is an enum, a slot is a bit — so this
//! module is compiled off macOS in the test build, exactly as
//! [`crate::bind_cache`], [`crate::argument`], [`crate::pass`],
//! [`crate::present`], [`crate::query`] and [`crate::quirk`] are, and for the
//! reason `bind_cache` states: a wrong answer here is silent, so the host test
//! is the check this logic gets on every push.

use crate::argument::BUFFER_TABLE_ENTRIES;
use crate::bind_cache::Stage;

/// Entries in Metal's per-stage **texture** argument table. See
/// [`BUFFER_TABLE_ENTRIES`], which carries the argument for all three and lives
/// in [`crate::argument`] because a push-constant block competes for it.
pub(crate) const TEXTURE_TABLE_ENTRIES: u32 = 128;

/// Entries in Metal's per-stage **sampler** argument table. See
/// [`TEXTURE_TABLE_ENTRIES`].
pub(crate) const SAMPLER_TABLE_ENTRIES: u32 = 16;

/// How many argument tables Metal gives a stage.
pub(crate) const TABLES: usize = 3;

/// Slots one [`BindingMask`] word holds.
///
/// `u128`, because the texture table is the widest of the three — and this
/// module's `the_mask_is_wider_than_every_table` is what holds it to that
/// rather than a comment nothing recomputes.
const MASK_BITS: u32 = u128::BITS;

/// Which of Metal's three per-stage argument tables a binding occupies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Table {
    /// `setVertexBuffer:offset:atIndex:` and its fragment sibling.
    Buffer,
    /// `setVertexTexture:atIndex:` and its fragment sibling.
    Texture,
    /// `setVertexSamplerState:atIndex:` and its fragment sibling.
    Sampler,
}

impl Table {
    /// Every table, in the order [`TableCounts`](crate::binding::TableCounts)
    /// indexes them.
    pub(crate) const ALL: [Self; TABLES] = [Self::Buffer, Self::Texture, Self::Sampler];

    /// Position in a [`TableCounts`](crate::binding::TableCounts).
    pub(crate) const fn slot(self) -> usize {
        match self {
            Self::Buffer => 0,
            Self::Texture => 1,
            Self::Sampler => 2,
        }
    }

    /// How many entries this table has.
    pub(crate) const fn capacity(self) -> u32 {
        match self {
            Self::Buffer => BUFFER_TABLE_ENTRIES,
            Self::Texture => TEXTURE_TABLE_ENTRIES,
            Self::Sampler => SAMPLER_TABLE_ENTRIES,
        }
    }
}

/// One `MTLBinding` of a pipeline's reflection, reduced to what a mask keeps.
///
/// `table` is [`None`] for a binding type that occupies no argument table at
/// all — threadgroup memory, an object payload, an imageblock — because those
/// number in a space of their own and marking one would set a bit belonging to
/// a buffer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Reflected {
    /// Which of the pipeline's stages declared it.
    pub(crate) stage: Stage,
    /// The table it occupies, or [`None`] for a binding type that has none.
    pub(crate) table: Option<Table>,
    /// `MTLBinding::index`.
    pub(crate) index: u32,
    /// `MTLBinding::isUsed` — whether the compiled function reads it, rather
    /// than whether it declares it.
    pub(crate) used: bool,
}

/// Which argument-table slots one pipeline reads, a bit per slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BindingMask {
    /// Indexed by [`Stage::slot`], then by [`Table::slot`].
    stages: [[u128; TABLES]; Stage::COUNT],
}

impl BindingMask {
    /// Every slot of every table read — what a pipeline whose reflection could
    /// not be had is treated as, per the module header.
    pub(crate) const fn all() -> Self {
        Self {
            stages: [[u128::MAX; TABLES]; Stage::COUNT],
        }
    }

    /// No slot read.
    ///
    /// What [`from_reflection`](Self::from_reflection) builds up from, and what
    /// a pass holds **before any pipeline is bound**: there is nothing there to
    /// read the tables, and `crcbl_mtl::command` retains every bind group in
    /// force until a draw supplies a mask. Deferring the binds that way rather
    /// than making them under [`all`](Self::all) is what keeps a group bound
    /// ahead of its pipeline from filling slots that pipeline turns out not to
    /// read.
    pub(crate) const fn none() -> Self {
        Self {
            stages: [[0; TABLES]; Stage::COUNT],
        }
    }

    /// The mask a pipeline's reflection describes: the slots whose
    /// [`Reflected::used`] is true, and nothing else.
    ///
    /// A slot no entry names is **not** read. That is the whole point rather
    /// than an oversight — Metal's reflection enumerates every argument the
    /// compiled function declares, so an index missing from it is one no stage
    /// of this pipeline could read at all. The caller is what decides whether
    /// the reflection is trustworthy enough to be reduced this way; see
    /// [`crate::pipeline`], which falls back to [`all`](Self::all) whenever
    /// Metal hands back nothing.
    pub(crate) fn from_reflection(bindings: impl IntoIterator<Item = Reflected>) -> Self {
        let mut mask = Self::none();
        for binding in bindings {
            let (Some(table), true) = (binding.table, binding.used) else {
                continue;
            };
            if binding.index < MASK_BITS {
                mask.stages[binding.stage.slot()][table.slot()] |= 1 << binding.index;
            }
        }
        mask
    }

    /// Whether this pipeline reads `index` of `stage`'s `table`.
    ///
    /// An index past the mask's width answers `true`, on the module header's
    /// terms: nothing above [`Table::capacity`] can be bound in the first place
    /// — [`plan_layout`](crate::binding::plan_layout) refuses a pipeline layout
    /// that overruns a table — so this is the direction to be wrong in rather
    /// than a case that arises.
    pub(crate) const fn uses(&self, stage: Stage, table: Table, index: u32) -> bool {
        if index >= MASK_BITS {
            return true;
        }
        self.stages[stage.slot()][table.slot()] & (1 << index) != 0
    }

    /// Marks one physical argument-table slot.
    pub(crate) fn insert(&mut self, stage: Stage, table: Table, index: u32) {
        if index < MASK_BITS {
            self.stages[stage.slot()][table.slot()] |= 1 << index;
        }
    }

    /// Slots present in both masks.
    pub(crate) fn intersection(self, other: Self) -> Self {
        let mut shared = Self::none();
        for stage in 0..Stage::COUNT {
            for table in 0..TABLES {
                shared.stages[stage][table] =
                    self.stages[stage][table] & other.stages[stage][table];
            }
        }
        shared
    }

    /// Claims every slot in `candidates` that no later argument claimed yet.
    ///
    /// `self` is the union of later writes while the render replay walks its
    /// logical arguments backwards. The return value is therefore exactly the
    /// physical writes the current argument wins.
    pub(crate) fn claim(&mut self, candidates: Self) -> Self {
        let mut won = Self::none();
        for stage in 0..Stage::COUNT {
            for table in 0..TABLES {
                won.stages[stage][table] =
                    candidates.stages[stage][table] & !self.stages[stage][table];
                self.stages[stage][table] |= candidates.stages[stage][table];
            }
        }
        won
    }

    /// Whether the `set*` for this slot is made: the pipeline reads it, and
    /// **only then** is [`BindCache`](crate::bind_cache::BindCache) asked
    /// whether the call would change anything.
    ///
    /// One call rather than an `&&` at each of [`crate::binding`]'s bind sites,
    /// because the order of the two questions is the whole of the correctness
    /// and an ordering convention is not something the compiler checks. Asking
    /// the cache first would *record* a bind that is then skipped, and the next
    /// pipeline — one that does read the slot — would be told the argument is
    /// already there and given nothing. The slot would still hold whatever the
    /// previous bind left in it, which no Metal call fails on and no layer
    /// reports.
    ///
    /// `changed` is the cache method for this slot's table, taken lazily for
    /// exactly that reason.
    pub(crate) fn issue(
        self,
        stage: Stage,
        table: Table,
        index: u32,
        changed: impl FnOnce() -> bool,
    ) -> bool {
        self.uses(stage, table, index) && changed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bind_cache::{BindCache, ResourceId};

    /// The bitmask has to be wider than the widest table, or a slot near the
    /// top of it has no bit and [`BindingMask::uses`] would answer for the
    /// wrong one.
    #[test]
    fn the_mask_is_wider_than_every_table() {
        for table in Table::ALL {
            assert!(
                table.capacity() <= MASK_BITS,
                "{table:?} has {} entries and the mask holds {MASK_BITS} bits, so its top slots \
                 would fall outside the word",
                table.capacity()
            );
        }
    }

    /// The fallback answers yes to everything, including the last slot of the
    /// widest table — which is the half a mask of the wrong width would fail.
    #[test]
    fn the_fallback_reads_every_slot_of_every_table() {
        let mask = BindingMask::all();
        for stage in [
            Stage::Vertex,
            Stage::Fragment,
            Stage::Compute,
            Stage::Object,
            Stage::Mesh,
        ] {
            for table in Table::ALL {
                for index in [0, 1, table.capacity() - 1] {
                    assert!(
                        mask.uses(stage, table, index),
                        "BindingMask::all() dropped {stage:?} {table:?} {index}"
                    );
                }
            }
        }
    }

    /// **`isUsed` is the field this reads, not the declaration.** A binding the
    /// function declares and does not read is exactly `msl/mesh.metal`'s
    /// `fragmentMain` taking `draw [[buffer(3)]]` and never dereferencing it,
    /// which is the noise this module exists to stop binding for.
    #[test]
    fn a_declared_but_unread_binding_is_not_in_the_mask() {
        let mask = BindingMask::from_reflection([
            Reflected {
                stage: Stage::Fragment,
                table: Some(Table::Buffer),
                index: 6,
                used: true,
            },
            Reflected {
                stage: Stage::Fragment,
                table: Some(Table::Buffer),
                index: 3,
                used: false,
            },
        ]);
        assert!(mask.uses(Stage::Fragment, Table::Buffer, 6));
        assert!(!mask.uses(Stage::Fragment, Table::Buffer, 3));
        // And a slot the reflection never named is not read either, which is
        // what makes an "unused binding in encoder" complaint answerable.
        assert!(!mask.uses(Stage::Fragment, Table::Buffer, 4));
    }

    /// A slot number means nothing without its stage and its table, exactly as
    /// [`crate::bind_cache`] requires of the mirror beside this one: three
    /// stages times three tables are nine independent spaces.
    #[test]
    fn a_bit_belongs_to_one_stage_and_one_table() {
        let mask = BindingMask::from_reflection([Reflected {
            stage: Stage::Vertex,
            table: Some(Table::Buffer),
            index: 5,
            used: true,
        }]);
        assert!(mask.uses(Stage::Vertex, Table::Buffer, 5));
        assert!(!mask.uses(Stage::Fragment, Table::Buffer, 5));
        assert!(!mask.uses(Stage::Compute, Table::Buffer, 5));
        assert!(!mask.uses(Stage::Vertex, Table::Texture, 5));
        assert!(!mask.uses(Stage::Vertex, Table::Sampler, 5));
        assert!(!mask.uses(Stage::Vertex, Table::Buffer, 4));
    }

    /// A binding type with no argument table behind it numbers in a space of
    /// its own — threadgroup memory at index 0 is not buffer 0 — so it sets no
    /// bit at all rather than one belonging to a buffer.
    #[test]
    fn a_binding_with_no_table_sets_no_bit() {
        let mask = BindingMask::from_reflection([Reflected {
            stage: Stage::Compute,
            table: None,
            index: 0,
            used: true,
        }]);
        for table in Table::ALL {
            assert!(
                !mask.uses(Stage::Compute, table, 0),
                "a tableless binding marked {table:?} 0"
            );
        }
    }

    /// **A bind the mask skipped is not recorded as made**, so the next
    /// pipeline that does read the slot is still given it.
    ///
    /// The sequence is the one a pass makes whenever two pipelines share a bind
    /// group: pipeline A does not read buffer 3, the group binding it is
    /// applied and skipped, pipeline B does read buffer 3, and the same group is
    /// applied again. If the skip had been written into [`BindCache`] as a bind,
    /// the second apply would see a hit and the draw would read whatever the
    /// slot held from before the pass.
    ///
    /// **What turns it red.** Recording the skip — evaluating `changed` before
    /// consulting the mask in [`BindingMask::issue`], which is the ordering that
    /// function exists to make structural.
    #[test]
    fn a_slot_masked_out_for_one_pipeline_is_bound_for_the_next() {
        let unread = BindingMask::from_reflection([Reflected {
            stage: Stage::Fragment,
            table: Some(Table::Buffer),
            index: 3,
            used: false,
        }]);
        let read = BindingMask::from_reflection([Reflected {
            stage: Stage::Fragment,
            table: Some(Table::Buffer),
            index: 3,
            used: true,
        }]);
        let mut binds = BindCache::default();
        let resource = ResourceId::new(0x4000);

        // Pipeline A: the layout permits the fragment bind and the mask does
        // not, so `issue` must not let the cache hear about it.
        assert!(
            !unread.issue(Stage::Fragment, Table::Buffer, 3, || {
                binds.buffer_changed(Stage::Fragment, 3, resource, 0)
            }),
            "a slot the pipeline cannot read was bound"
        );

        // Pipeline B reads it, and the bind must still be made.
        assert!(
            read.issue(Stage::Fragment, Table::Buffer, 3, || {
                binds.buffer_changed(Stage::Fragment, 3, resource, 0)
            }),
            "the skipped bind was recorded as made, so the pipeline that reads \
             slot 3 gets whatever was there before the pass"
        );
    }

    /// An index past the mask's width is bound rather than skipped, which is
    /// the module header's one-directional rule applied to the one input that
    /// can carry an out-of-range number — Metal's own reflection.
    #[test]
    fn an_index_past_the_mask_is_bound_rather_than_dropped() {
        let mask = BindingMask::from_reflection([Reflected {
            stage: Stage::Vertex,
            table: Some(Table::Texture),
            index: MASK_BITS,
            used: true,
        }]);
        assert!(mask.uses(Stage::Vertex, Table::Texture, MASK_BITS));
        assert!(!mask.uses(Stage::Vertex, Table::Texture, 0));
    }

    /// **`insert` marks exactly one physical slot**, which is what the render
    /// replay uses to say "this argument writes table `t` slot `i`" without
    /// building a reflection: an index past the word sets nothing, and no
    /// neighbouring slot, table or stage moves with it.
    #[test]
    fn insert_marks_one_slot_and_ignores_one_past_the_mask() {
        let mut mask = BindingMask::none();
        mask.insert(Stage::Mesh, Table::Buffer, 7);
        assert!(mask.uses(Stage::Mesh, Table::Buffer, 7));
        assert!(!mask.uses(Stage::Mesh, Table::Buffer, 6));
        assert!(!mask.uses(Stage::Vertex, Table::Buffer, 7));
        assert!(!mask.uses(Stage::Mesh, Table::Texture, 7));

        let before = mask;
        mask.insert(Stage::Mesh, Table::Buffer, MASK_BITS);
        assert_eq!(mask, before, "an index past the word set a bit somewhere");
    }

    /// [`intersection`](BindingMask::intersection) is "slots both arguments
    /// write", which is how the replay asks whether a new argument replaces one
    /// already materialised on the same physical slot. Symmetric, because a
    /// comparison that depended on which side it was asked from would make the
    /// answer a function of replay order.
    #[test]
    fn intersection_keeps_only_the_slots_both_masks_write() {
        let mut left = BindingMask::none();
        left.insert(Stage::Vertex, Table::Buffer, 1);
        left.insert(Stage::Vertex, Table::Buffer, 2);
        left.insert(Stage::Fragment, Table::Texture, 3);
        let mut right = BindingMask::none();
        right.insert(Stage::Vertex, Table::Buffer, 2);
        right.insert(Stage::Fragment, Table::Texture, 4);

        let shared = left.intersection(right);
        assert!(shared.uses(Stage::Vertex, Table::Buffer, 2));
        assert!(!shared.uses(Stage::Vertex, Table::Buffer, 1));
        assert!(!shared.uses(Stage::Fragment, Table::Texture, 3));
        assert!(!shared.uses(Stage::Fragment, Table::Texture, 4));
        assert_eq!(right.intersection(left), shared);
    }

    /// **The replay walks its arguments backwards, so the last writer of a slot
    /// is the one that reaches Metal.** [`claim`](BindingMask::claim) answers
    /// which slots the current argument wins, given the union of the ones
    /// already visited, and folds candidates into that union.
    #[test]
    fn claim_gives_a_slot_to_the_last_writer_only() {
        let mut claimed = BindingMask::none();

        let mut last = BindingMask::none();
        last.insert(Stage::Vertex, Table::Buffer, 2);
        assert_eq!(claimed.claim(last), last, "the last argument won no slot");

        // An earlier argument writing that slot and one of its own keeps only
        // the slot the later one did not take.
        let mut earlier = BindingMask::none();
        earlier.insert(Stage::Vertex, Table::Buffer, 1);
        earlier.insert(Stage::Vertex, Table::Buffer, 2);
        let mut only_one = BindingMask::none();
        only_one.insert(Stage::Vertex, Table::Buffer, 1);
        assert_eq!(
            claimed.claim(earlier),
            only_one,
            "a slot the later argument already wrote was won twice"
        );
        assert!(
            claimed.uses(Stage::Vertex, Table::Buffer, 1)
                && claimed.uses(Stage::Vertex, Table::Buffer, 2),
            "the union did not take in the earlier argument's slots"
        );

        let mut claimed_again = BindingMask::none();
        claimed_again.insert(Stage::Vertex, Table::Buffer, 1);
        assert_eq!(
            claimed.claim(claimed_again),
            BindingMask::none(),
            "an argument won a slot an earlier one had already claimed"
        );
    }
}
