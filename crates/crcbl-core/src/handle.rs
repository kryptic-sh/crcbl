//! Typed handles and the slot pool that issues them.
//!
//! A [`Handle<T>`] is the engine-wide way to name a resource that lives in a
//! [`Pool<T>`]. Handles are values: `Copy`, comparable, hashable, and cheap to
//! store in arrays or send over the wire. They are *not* pointers — a handle
//! whose slot has been recycled fails lookup instead of aliasing whatever moved
//! in afterwards.
//!
//! # Representation
//!
//! A handle is a 32-bit slot index plus a 32-bit generation, stored as two
//! fields rather than a packed `u64`:
//!
//! * The struct is 8 bytes with 4-byte alignment either way, so packing buys no
//!   space — it would only add shift/mask noise at every use site.
//! * The generation is a [`NonZeroU32`], which gives the struct a niche, so
//!   `Option<Handle<T>>` is *also* 8 bytes. That matters: optional handles are
//!   everywhere in component arrays.
//! * [`Handle::to_bits`] / [`Handle::from_bits`] provide the packed `u64` form
//!   for serialization and FFI without forcing it on in-memory layout.
//!
//! The type parameter is carried as `PhantomData<fn() -> T>`: covariant in `T`
//! (an owned-value marker), and unconditionally `Send`, `Sync`, and `Unpin`
//! regardless of `T`. A handle is an integer; it must not inherit thread-safety
//! restrictions from a type it merely names. For the same reason every trait
//! impl below is hand-written — `#[derive]` would bound `Clone`/`Copy`/`Eq`/…
//! on `T: Clone`/`T: Copy`/`T: Eq`, which is wrong for a plain index.

use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;
use core::mem;
use core::num::NonZeroU32;

/// The generation every fresh slot starts at.
const FIRST_GENERATION: NonZeroU32 = NonZeroU32::MIN;

/// A typed, generational reference to a value stored in a [`Pool<T>`].
///
/// See the [module docs](self) for the representation and variance rationale.
#[repr(C)]
pub struct Handle<T> {
    index: u32,
    generation: NonZeroU32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    /// Builds a handle from its parts.
    ///
    /// Only the owning [`Pool`] should need this; a hand-made handle simply
    /// fails lookup unless it happens to match a live slot.
    #[inline]
    const fn new(index: u32, generation: NonZeroU32) -> Self {
        Self {
            index,
            generation,
            marker: PhantomData,
        }
    }

    /// The slot index this handle points at.
    #[inline]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// The generation this handle was issued with.
    ///
    /// Never zero — see the [module docs](self).
    #[inline]
    pub const fn generation(self) -> u32 {
        self.generation.get()
    }

    /// Packs the handle into a `u64` (generation in the high half).
    ///
    /// The packed form is stable and never zero, which makes it safe to use as
    /// a wire/serialization representation with `0` reserved for "none".
    #[inline]
    pub const fn to_bits(self) -> u64 {
        ((self.generation.get() as u64) << 32) | self.index as u64
    }

    /// Unpacks a handle produced by [`Handle::to_bits`].
    ///
    /// Returns `None` if the generation half is zero, which no real handle ever
    /// has.
    #[inline]
    pub const fn from_bits(bits: u64) -> Option<Self> {
        match NonZeroU32::new((bits >> 32) as u32) {
            Some(generation) => Some(Self::new(bits as u32, generation)),
            None => None,
        }
    }

    /// Reinterprets the handle as naming a different type.
    ///
    /// Useful only at seams that erase types (e.g. a generic resource table);
    /// the result is meaningless in any other pool.
    #[inline]
    pub const fn cast<U>(self) -> Handle<U> {
        Handle::new(self.index, self.generation)
    }
}

impl<T> Clone for Handle<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Handle<T> {}

impl<T> Hash for Handle<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_bits().hash(state);
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Handle<{}>({}, gen {})",
            core::any::type_name::<T>(),
            self.index,
            self.generation
        )
    }
}

/// A slot in a [`Pool`].
enum Slot<T> {
    /// Holds a live value; `generation` is the one issued to its handle.
    Occupied { generation: NonZeroU32, value: T },
    /// Free and on the free list; `generation` is the one the *next* insert
    /// into this slot will issue.
    Vacant {
        generation: NonZeroU32,
        next_free: Option<u32>,
    },
    /// Permanently withdrawn: its generation counter is exhausted. Never
    /// reused, so no stale handle can ever be revived.
    Retired,
}

/// A slotmap-style arena: values in, [`Handle`]s out.
///
/// Insertion recycles free slots through a free list, so live values stay
/// densely packed near the front of the backing `Vec` under steady-state
/// churn. Every removal bumps the slot's generation, so handles issued before
/// the removal stop resolving.
///
/// # Generation exhaustion
///
/// Generations are 32-bit and *never wrap*. A slot that has been recycled
/// `u32::MAX - 1` times is **retired**: its storage is dropped, the slot is
/// removed from the free list, and it is never handed out again. Wrapping would
/// silently resurrect a >4-billion-removals-old handle as a valid alias for an
/// unrelated value — a bug that would surface as impossible-looking aliasing
/// weeks into a server's uptime. Retiring instead costs one dead `Vec` entry
/// (a few bytes) per exhausted slot, and only after ~4.3e9 reuses of that one
/// slot: at 60 Hz, recycling the same slot every tick, that is over two years
/// of continuous churn. See [`Pool::retired_slots`] to observe it.
///
/// # Safety
///
/// This module contains no `unsafe`. The `Vec<Slot<T>>` + enum payload costs
/// one discriminant per slot versus a hand-rolled `MaybeUninit` arena; clarity
/// wins at this layer, and the pool is not in any inner loop that measures.
pub struct Pool<T> {
    slots: Vec<Slot<T>>,
    /// Index of the first free slot, if any (singly linked through `Vacant`).
    free_head: Option<u32>,
    len: usize,
    retired: usize,
}

impl<T> Pool<T> {
    /// Creates an empty pool.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_head: None,
            len: 0,
            retired: 0,
        }
    }

    /// Creates an empty pool with room for `capacity` slots.
    #[inline]
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: Vec::with_capacity(capacity),
            free_head: None,
            len: 0,
            retired: 0,
        }
    }

    /// Number of live values.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the pool holds no live values.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of slots permanently withdrawn by generation exhaustion.
    ///
    /// Expected to be zero forever; a non-zero value in a long-running server
    /// is worth a metric, not an alarm.
    #[inline]
    #[must_use]
    pub const fn retired_slots(&self) -> usize {
        self.retired
    }

    /// Inserts `value` and returns a handle to it.
    ///
    /// # Panics
    ///
    /// If the pool would exceed `u32::MAX + 1` slots — the index is 32-bit by
    /// design, and a pool that large is a leak, not a workload.
    pub fn insert(&mut self, value: T) -> Handle<T> {
        if let Some(index) = self.free_head {
            let slot = &mut self.slots[index as usize];
            let Slot::Vacant {
                generation,
                next_free,
            } = *slot
            else {
                unreachable!("free list only links Vacant slots");
            };
            *slot = Slot::Occupied { generation, value };
            self.free_head = next_free;
            self.len += 1;
            Handle::new(index, generation)
        } else {
            let index = u32::try_from(self.slots.len()).expect("Pool exceeded 2^32 slots");
            self.slots.push(Slot::Occupied {
                generation: FIRST_GENERATION,
                value,
            });
            self.len += 1;
            Handle::new(index, FIRST_GENERATION)
        }
    }

    /// Borrows the value `handle` names, or `None` if the handle is stale.
    #[inline]
    #[must_use]
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        match self.slots.get(handle.index as usize) {
            Some(Slot::Occupied { generation, value }) if *generation == handle.generation => {
                Some(value)
            }
            _ => None,
        }
    }

    /// Mutably borrows the value `handle` names, or `None` if the handle is
    /// stale.
    #[inline]
    #[must_use]
    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        match self.slots.get_mut(handle.index as usize) {
            Some(Slot::Occupied { generation, value }) if *generation == handle.generation => {
                Some(value)
            }
            _ => None,
        }
    }

    /// Whether `handle` still resolves.
    #[inline]
    #[must_use]
    pub fn contains(&self, handle: Handle<T>) -> bool {
        self.get(handle).is_some()
    }

    /// Removes and returns the value `handle` names, invalidating every handle
    /// to that slot.
    ///
    /// Returns `None` (and changes nothing) if the handle is already stale.
    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        let slot = self.slots.get_mut(handle.index as usize)?;
        match slot {
            Slot::Occupied { generation, .. } if *generation == handle.generation => {}
            _ => return None,
        }
        // Retire first, then decide: the value has to move out before the slot
        // can be rewritten, and `Retired` is the correct state if the
        // generation turns out to be exhausted.
        let Slot::Occupied { generation, value } = mem::replace(slot, Slot::Retired) else {
            unreachable!("checked above");
        };
        self.len -= 1;
        match generation.checked_add(1) {
            Some(next) => {
                *slot = Slot::Vacant {
                    generation: next,
                    next_free: self.free_head,
                };
                self.free_head = Some(handle.index);
            }
            // Generation exhausted: the slot stays `Retired` and off the free
            // list, forever. See the type-level docs.
            None => self.retired += 1,
        }
        Some(value)
    }

    /// Removes every value, invalidating all outstanding handles.
    ///
    /// Slot storage is kept for reuse; generations continue from where they
    /// were, so handles issued before the clear stay dead.
    pub fn clear(&mut self) {
        self.free_head = None;
        for index in 0..self.slots.len() {
            let slot = &mut self.slots[index];
            let generation = match slot {
                Slot::Occupied { generation, .. } => match generation.checked_add(1) {
                    Some(next) => next,
                    None => {
                        *slot = Slot::Retired;
                        self.retired += 1;
                        continue;
                    }
                },
                Slot::Vacant { generation, .. } => *generation,
                Slot::Retired => continue,
            };
            *slot = Slot::Vacant {
                generation,
                next_free: self.free_head,
            };
            // `index < slots.len() <= u32::MAX + 1` and slots were created via
            // `insert`, which caps the index at `u32::MAX`.
            self.free_head = Some(index as u32);
        }
        self.len = 0;
    }

    /// Iterates live `(handle, &value)` pairs in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| match slot {
                Slot::Occupied { generation, value } => {
                    Some((Handle::new(index as u32, *generation), value))
                }
                _ => None,
            })
    }

    /// Iterates live `(handle, &mut value)` pairs in slot order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Handle<T>, &mut T)> {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| match slot {
                Slot::Occupied { generation, value } => {
                    Some((Handle::new(index as u32, *generation), value))
                }
                _ => None,
            })
    }
}

impl<T> Default for Pool<T> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Extend<T> for Pool<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for value in iter {
            self.insert(value);
        }
    }
}

impl<T> FromIterator<T> for Pool<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut pool = Self::new();
        pool.extend(iter);
        pool
    }
}

/// Deliberately does not print the values: a pool is usually large, and this
/// impl must exist for *every* `T`, not just `T: Debug`.
impl<T> fmt::Debug for Pool<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pool")
            .field("type", &core::any::type_name::<T>())
            .field("len", &self.len)
            .field("slots", &self.slots.len())
            .field("retired", &self.retired)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashMap;

    #[test]
    fn handle_is_eight_bytes_and_option_is_free() {
        assert_eq!(mem::size_of::<Handle<String>>(), 8);
        assert_eq!(mem::size_of::<Option<Handle<String>>>(), 8);
    }

    #[test]
    fn handle_traits_do_not_depend_on_t() {
        // `*mut ()` is neither `Send`, `Sync`, `Copy`, nor `Eq`; the handle is
        // all of them anyway. This is the whole point of the manual impls.
        fn assert_traits<H: Copy + Eq + Hash + fmt::Debug + Send + Sync>() {}
        assert_traits::<Handle<*mut ()>>();
    }

    #[test]
    fn a_handle_survives_to_bits_and_back_and_a_zero_generation_is_not_a_handle() {
        let handle = Handle::<u8>::new(0xDEAD_BEEF, NonZeroU32::new(0x0BAD_F00D).unwrap());
        assert_eq!(Handle::from_bits(handle.to_bits()), Some(handle));
        assert_ne!(handle.to_bits(), 0);
        assert_eq!(Handle::<u8>::from_bits(0), None);
        // Index-only bits still have a zero generation, hence no handle.
        assert_eq!(Handle::<u8>::from_bits(0xFFFF_FFFF), None);
    }

    #[test]
    fn a_pool_hands_back_what_was_inserted_and_a_removed_handle_never_resurrects() {
        let mut pool = Pool::new();
        let a = pool.insert("a".to_string());
        let b = pool.insert("b".to_string());
        assert_eq!(pool.len(), 2);
        assert_eq!(pool.get(a).map(String::as_str), Some("a"));
        assert_eq!(pool.get(b).map(String::as_str), Some("b"));

        assert_eq!(pool.remove(a), Some("a".to_string()));
        assert_eq!(pool.remove(a), None, "double remove must not resurrect");
        assert!(!pool.contains(a));
        assert!(pool.contains(b));
        assert_eq!(pool.len(), 1);

        pool.get_mut(b).unwrap().push('!');
        assert_eq!(pool.get(b).map(String::as_str), Some("b!"));
    }

    #[test]
    fn removed_slot_is_recycled_with_a_new_generation() {
        let mut pool = Pool::new();
        let old = pool.insert(1u32);
        pool.remove(old);
        let new = pool.insert(2u32);
        assert_eq!(new.index(), old.index(), "free list should reuse the slot");
        assert_ne!(new.generation(), old.generation());
        assert_eq!(
            pool.get(old),
            None,
            "stale handle must not see the new value"
        );
        assert_eq!(pool.get(new), Some(&2));
    }

    #[test]
    fn clear_invalidates_everything_but_keeps_slots() {
        let mut pool = Pool::new();
        let handles: Vec<_> = (0..8).map(|i| pool.insert(i)).collect();
        pool.clear();
        assert!(pool.is_empty());
        for handle in &handles {
            assert_eq!(pool.get(*handle), None);
        }
        // Slots come back from the free list, so indices are reused.
        let fresh = pool.insert(99);
        assert!(handles.iter().any(|h| h.index() == fresh.index()));
        assert_eq!(pool.get(fresh), Some(&99));
    }

    #[test]
    fn iteration_yields_live_pairs_only() {
        let mut pool = Pool::new();
        let a = pool.insert(10);
        let b = pool.insert(20);
        let c = pool.insert(30);
        pool.remove(b);

        let mut seen: Vec<_> = pool.iter().map(|(h, v)| (h, *v)).collect();
        seen.sort_by_key(|(h, _)| h.index());
        assert_eq!(seen, vec![(a, 10), (c, 30)]);

        for (_, value) in pool.iter_mut() {
            *value += 1;
        }
        assert_eq!(pool.get(a), Some(&11));
        assert_eq!(pool.get(c), Some(&31));
    }

    #[test]
    fn exhausted_generation_retires_the_slot() {
        let mut pool = Pool::new();
        let handle = pool.insert(1u8);
        // Fast-forward the slot to its last generation rather than churning
        // 4.3 billion times.
        let last = NonZeroU32::new(u32::MAX).unwrap();
        pool.slots[0] = Slot::Occupied {
            generation: last,
            value: 1,
        };
        let live = Handle::<u8>::new(0, last);
        assert_eq!(pool.get(handle), None);
        assert_eq!(pool.get(live), Some(&1));

        assert_eq!(pool.remove(live), Some(1));
        assert_eq!(pool.retired_slots(), 1);
        assert_eq!(pool.get(live), None);

        // The retired slot is off the free list: the next insert takes a new
        // one and can never collide with `live`.
        let next = pool.insert(2u8);
        assert_ne!(next.index(), live.index());
        assert_eq!(pool.get(live), None);
    }

    #[test]
    fn collected_and_debug_render() {
        let pool: Pool<i32> = (0..3).collect();
        assert_eq!(pool.len(), 3);
        let rendered = format!("{pool:?}");
        assert!(rendered.contains("len: 3"), "{rendered}");
    }

    #[test]
    fn len_equals_iter_count_after_any_operation() {
        // If `insert` increments `len` before `Vec::push` and the push
        // panics (OOM), this invariant would break.  The OOM path cannot
        // be forced in a normal test, but the invariant itself is what
        // the `len` move-after-push protects.
        let mut pool = Pool::new();
        assert_eq!(pool.len(), pool.iter().count());

        let mut handles = Vec::new();
        for i in 0..20 {
            handles.push(pool.insert(i));
            assert_eq!(pool.len(), pool.iter().count());
        }

        for h in &handles[..10] {
            pool.remove(*h);
            assert_eq!(pool.len(), pool.iter().count());
        }

        // Insert again — exercises the free-list path.
        for i in 30..40 {
            pool.insert(i);
            assert_eq!(pool.len(), pool.iter().count());
        }

        pool.clear();
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.iter().count(), 0);
    }

    /// Reference-model comparison: a `HashMap` of every handle ever issued and
    /// whether it should still be live.
    #[test]
    fn model_matches_under_scripted_churn() {
        let mut pool = Pool::new();
        let mut model: HashMap<Handle<u64>, u64> = HashMap::new();
        let mut dead: Vec<Handle<u64>> = Vec::new();
        // Deterministic pseudo-random script; the proptest version is
        // `prop_pool_matches_hashmap_model` below.
        let mut state = 0x1234_5678_9ABC_DEF0u64;
        for step in 0..2_000u64 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            if state.is_multiple_of(3) && !model.is_empty() {
                let victim = *model.keys().next().unwrap();
                assert_eq!(pool.remove(victim), model.remove(&victim));
                dead.push(victim);
            } else {
                let handle = pool.insert(step);
                assert!(
                    model.insert(handle, step).is_none(),
                    "handle reissued while live"
                );
            }
            assert_eq!(pool.len(), model.len());
        }
        for (handle, value) in &model {
            assert_eq!(pool.get(*handle), Some(value));
        }
        for handle in &dead {
            assert!(!pool.contains(*handle));
        }
    }

    // --- property tests -------------------------------------------------

    /// One step of the pool script.
    #[derive(Clone, Debug)]
    enum Op {
        Insert(u64),
        /// Remove the nth live handle (modulo the number live).
        RemoveLive(usize),
        /// Re-remove the nth already-dead handle — must be a no-op.
        RemoveDead(usize),
        Clear,
    }

    fn any_op() -> impl Strategy<Value = Op> {
        prop_oneof![
            8 => any::<u64>().prop_map(Op::Insert),
            6 => any::<usize>().prop_map(Op::RemoveLive),
            2 => any::<usize>().prop_map(Op::RemoveDead),
            1 => Just(Op::Clear),
        ]
    }

    proptest! {
        /// The anchor test from `docs/plan/12-testing.md`: after an arbitrary
        /// insert/remove/clear sequence, the pool agrees with a `HashMap`
        /// reference model on length and contents, every live handle still
        /// resolves to the value it was inserted with, and every handle that
        /// was ever removed stays dead forever.
        #[test]
        fn prop_pool_matches_hashmap_model(ops in proptest::collection::vec(any_op(), 1..120)) {
            let mut pool: Pool<u64> = Pool::new();
            let mut live: HashMap<Handle<u64>, u64> = HashMap::new();
            let mut dead: Vec<Handle<u64>> = Vec::new();

            for op in &ops {
                match *op {
                    Op::Insert(value) => {
                        let handle = pool.insert(value);
                        prop_assert!(
                            !dead.contains(&handle),
                            "a dead handle was reissued: {handle:?}"
                        );
                        prop_assert!(live.insert(handle, value).is_none());
                    }
                    Op::RemoveLive(nth) => {
                        if live.is_empty() {
                            continue;
                        }
                        // Sort for determinism: `HashMap` iteration order is
                        // not reproducible, and a property test that is not
                        // reproducible is a coin flip.
                        let mut handles: Vec<_> = live.keys().copied().collect();
                        handles.sort_by_key(|handle| handle.to_bits());
                        let victim = handles[nth % handles.len()];
                        prop_assert_eq!(pool.remove(victim), live.remove(&victim));
                        dead.push(victim);
                    }
                    Op::RemoveDead(nth) => {
                        if dead.is_empty() {
                            continue;
                        }
                        let zombie = dead[nth % dead.len()];
                        prop_assert_eq!(pool.remove(zombie), None);
                    }
                    Op::Clear => {
                        pool.clear();
                        dead.extend(live.keys().copied());
                        live.clear();
                    }
                }

                prop_assert_eq!(pool.len(), live.len());
                prop_assert_eq!(pool.is_empty(), live.is_empty());
                prop_assert_eq!(pool.iter().count(), live.len());
                for handle in &dead {
                    prop_assert!(!pool.contains(*handle), "stale handle resolved: {handle:?}");
                }
            }

            for (handle, value) in &live {
                prop_assert_eq!(pool.get(*handle), Some(value));
            }
            // Iteration must yield exactly the model, handles included.
            let mut iterated: Vec<_> = pool.iter().map(|(h, v)| (h.to_bits(), *v)).collect();
            let mut expected: Vec<_> = live.iter().map(|(h, v)| (h.to_bits(), *v)).collect();
            iterated.sort_unstable();
            expected.sort_unstable();
            prop_assert_eq!(iterated, expected);
        }
    }
}
