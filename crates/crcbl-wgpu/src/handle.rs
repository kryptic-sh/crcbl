//! Which device issued a handle, and how a handle says so.
//!
//! # The obligation this module exists for
//!
//! `crcbl-hal`'s [obligation 3](crcbl_hal::device) — handles never cross
//! instances or devices — needs a mechanism, because a [`Handle`] has no room
//! for one: its index and generation halves are fully spoken for, and two pools
//! genuinely do issue identical bits. A backend must therefore stamp an owner
//! identity into its own side table and compare it on every lookup, so a
//! foreign handle is [`HalError::ForeignObject`] rather than whatever happens
//! to live at that index.
//!
//! This backend had neither half. Every pool here is per-device already, which
//! makes it *look* covered — device A's handle usually misses device B's pool.
//! Usually is not a guarantee: two devices allocating in step reach the same
//! slot at the same generation immediately, and from that moment B silently
//! resolves A's handle to B's own unrelated object and writes into it, or
//! destroys it.
//!
//! So there are two halves, and they answer different questions:
//!
//! * **The handle carries the owner's tag.** The top byte of the index half is
//!   the tag; the rest is the pool's own index, restored before any lookup. The
//!   generation half is untouched, so [`Pool`]'s generation-exhaustion rule is
//!   unaffected. This is what separates two handles whose bits are otherwise
//!   identical.
//! * **The slot records the owner's id.** The `u64` per tracked object the seam
//!   asks for by name. It is what would catch a pool holding more than one
//!   owner's entries, and it is what keeps a `remove` from taking a row this
//!   owner does not own; it does *not* substitute for the tag, because every
//!   pool here is per-owner and so its rows all carry the same id.
//!
//! `crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` arrived at the same scheme from the
//! same bug. The shape here follows `crcbl-dx12`'s `handle` module most
//! closely, with one difference forced by this backend's storage: its pools
//! hold bare `wgpu` objects — a `wgpu::Texture`, a `wgpu::Sampler` — which
//! cannot grow an `owner` field, so the id rides in an [`Owned`] wrapper the
//! pools store instead of in a per-kind slot struct.
//!
//! The residual hole is stated rather than hidden: tags repeat every
//! [`OWNER_TAG_COUNT`] owners, so a process that opens that many falls back to
//! the id check for the ones that collide — which, per the paragraph above,
//! cannot separate them. A process that opens hundreds of wgpu devices has a
//! different problem.
//!
//! # Three outcomes, kept apart
//!
//! A handle offered to a device is one of three things, and the seam gives each
//! its own error because the fixes differ:
//!
//! * **This owner's, and live** — resolved.
//! * **This owner's, and dead** — [`HalError::InvalidHandle`]: "you kept this
//!   too long". A handle carrying *no* tag is here too, because no owner ever
//!   issued it.
//! * **Another owner's** — [`HalError::ForeignObject`]: "you crossed two
//!   objects that never met".

use std::sync::atomic::{AtomicU64, Ordering};

use crcbl_core::{Handle, Pool};
use crcbl_hal::HalError;

/// Bits of a handle's index half given over to the owning object's tag.
const OWNER_TAG_SHIFT: u32 = 24;
/// The part of a handle's index half that is the pool's own index.
pub(crate) const POOL_INDEX_MASK: u32 = (1 << OWNER_TAG_SHIFT) - 1;
/// How many distinct owner tags exist. Tag `0` is reserved for "nobody", so a
/// hand-made or un-stamped handle is foreign to every owner.
const OWNER_TAG_COUNT: u64 = (u32::MAX >> OWNER_TAG_SHIFT) as u64;

/// Process-wide source of owner ids.
///
/// Starts at `1` and never yields `0`, so no owner shares an id with the
/// "nobody" the zero tag stands for.
static NEXT_OWNER_ID: AtomicU64 = AtomicU64::new(1);

/// The instance or device a handle belongs to: its process-wide id, and the tag
/// it stamps.
///
/// Both, because they answer different halves of the question. The **tag** is
/// what a handle can carry — one byte, so it repeats — and the **id** is what
/// the slot records, which never repeats. A handle is this owner's only if both
/// agree.
///
/// Instances and devices are both owners: the seam checks surfaces against the
/// *instance* id, so any device from that instance may use them, and everything
/// else against the *device* id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    /// Process-wide id, recorded in every slot this owner fills.
    id: u64,
    /// The tag this owner stamps into every handle it issues. Never zero.
    tag: u32,
}

impl Owner {
    /// The owner an instance or device with this id stamps with.
    fn new(id: u64) -> Self {
        #[allow(clippy::cast_possible_truncation)]
        let tag = 1 + (id % OWNER_TAG_COUNT) as u32;
        Self { id, tag }
    }

    /// A fresh owner, distinct from every other one this process has issued.
    pub(crate) fn next() -> Self {
        Self::new(NEXT_OWNER_ID.fetch_add(1, Ordering::Relaxed))
    }
}

/// A pool entry and the id of the owner that created it.
///
/// The `u64` per tracked object the seam's obligation 3 asks for. Generic
/// because most of this backend's pools hold `wgpu` types, which cannot carry
/// the field themselves.
pub(crate) struct Owned<T> {
    owner: u64,
    value: T,
}

/// The owner tag a handle carries, or `0` if it carries none.
const fn tag_of<M>(handle: Handle<M>) -> u32 {
    handle.index() >> OWNER_TAG_SHIFT
}

/// Strips the owner tag, recovering the pool's own handle.
fn untag<A, B>(handle: Handle<A>) -> Handle<B> {
    Handle::from_bits(
        (u64::from(handle.generation()) << 32) | u64::from(handle.index() & POOL_INDEX_MASK),
    )
    .unwrap_or_else(|| unreachable!("a handle's generation is never zero"))
}

/// Stamps an owner's tag into a handle its pool just issued.
///
/// A pool index too large to carry the tag gets tag `0`, which resolves
/// nowhere — the object leaks until the owner is dropped, which is far better
/// than a handle that might resolve to another owner's object. It takes more
/// live objects of one kind than [`POOL_INDEX_MASK`] admits to reach.
pub(crate) fn stamp<A, B>(owner: Owner, handle: Handle<A>) -> Handle<B> {
    let index = handle.index();
    let tag = if index > POOL_INDEX_MASK {
        crcbl_core::log::error!(
            "crcbl-wgpu: pool index {index} is too large to carry an owner tag; issuing a handle \
             that resolves nowhere rather than one that might resolve to another owner's object"
        );
        0
    } else {
        owner.tag
    };
    Handle::from_bits(
        (u64::from(handle.generation()) << 32) | u64::from((tag << OWNER_TAG_SHIFT) | index),
    )
    .unwrap_or_else(|| unreachable!("a handle's generation is never zero"))
}

/// Puts `value` in `owner`'s pool and hands back the stamped handle.
///
/// **The only way a handle leaves this backend**, so there is no creation path
/// that can forget the owner id or the tag.
pub(crate) fn insert<T, M>(pool: &mut Pool<Owned<T>>, owner: Owner, value: T) -> Handle<M> {
    let raw = pool.insert(Owned {
        owner: owner.id,
        value,
    });
    stamp(owner, raw)
}

/// The seam's error for a handle `owner` will not accept.
///
/// **The one place the three outcomes are told apart.** A tag some *other*
/// owner stamped is obligation 3's case; no tag at all means nobody issued it,
/// and this owner's own tag on a handle that still did not resolve means it is
/// stale — both of which are [`HalError::InvalidHandle`].
pub(crate) fn not_ours<M>(kind: &'static str, handle: Handle<M>, owner: Owner) -> HalError {
    if tag_of(handle) == 0 || tag_of(handle) == owner.tag {
        HalError::invalid_handle(kind, handle)
    } else {
        foreign(kind, handle)
    }
}

fn foreign<M>(kind: &'static str, handle: Handle<M>) -> HalError {
    HalError::ForeignObject {
        kind,
        bits: handle.to_bits(),
    }
}

/// Decodes a handle for `owner`'s pools, or says why it is not one.
///
/// **The one place a tag is compared.** Every resolve in this crate reaches it
/// through [`resolve`], and the two queue-handle checks — a queue is not
/// pooled, so there is no slot to compare — call [`not_ours`] directly.
fn local<E, M>(kind: &'static str, handle: Handle<M>, owner: Owner) -> Result<Handle<E>, HalError> {
    if tag_of(handle) == owner.tag {
        Ok(untag(handle))
    } else {
        Err(not_ours(kind, handle, owner))
    }
}

/// The pool index a handle names, once both halves of the owner check pass.
///
/// **The one place a slot's owner id is compared.** Splitting the resolve from
/// the borrow is what lets [`lookup_mut`] and [`remove`] reuse it: the check
/// runs through a shared borrow that ends before the mutable one begins, so a
/// foreign handle can never hand back a mutable reference to — or remove —
/// this owner's own object.
fn resolve<T, M>(
    pool: &Pool<Owned<T>>,
    kind: &'static str,
    handle: Handle<M>,
    owner: Owner,
) -> Result<Handle<Owned<T>>, HalError> {
    let index = local(kind, handle, owner)?;
    match pool.get(index) {
        Some(slot) if slot.owner == owner.id => Ok(index),
        Some(_) => Err(foreign(kind, handle)),
        None => Err(HalError::invalid_handle(kind, handle)),
    }
}

/// Resolves a handle against a pool and its owner.
///
/// # Errors
///
/// [`HalError::ForeignObject`] for another owner's handle, and
/// [`HalError::InvalidHandle`] for one nobody issued or one whose slot is empty
/// or has moved on.
pub(crate) fn lookup<'p, T, M>(
    pool: &'p Pool<Owned<T>>,
    kind: &'static str,
    handle: Handle<M>,
    owner: Owner,
) -> Result<&'p T, HalError> {
    let index = resolve(pool, kind, handle, owner)?;
    Ok(&pool
        .get(index)
        .unwrap_or_else(|| unreachable!("the slot resolved a moment ago"))
        .value)
}

/// [`lookup`], for an entry the caller has to change.
///
/// # Errors
///
/// As [`lookup`].
pub(crate) fn lookup_mut<'p, T, M>(
    pool: &'p mut Pool<Owned<T>>,
    kind: &'static str,
    handle: Handle<M>,
    owner: Owner,
) -> Result<&'p mut T, HalError> {
    let index = resolve(pool, kind, handle, owner)?;
    Ok(&mut pool
        .get_mut(index)
        .unwrap_or_else(|| unreachable!("the slot resolved a moment ago"))
        .value)
}

/// Every live value in a pool, for the sweeps that visit all of them.
///
/// No handle is involved and so nothing is checked: a pool only ever holds
/// entries its own owner inserted, which is the invariant [`insert`] keeps.
pub(crate) fn values<T>(pool: &Pool<Owned<T>>) -> impl Iterator<Item = &T> {
    pool.iter().map(|(_, slot)| &slot.value)
}

/// [`values`], for a sweep that updates what it visits.
pub(crate) fn values_mut<T>(pool: &mut Pool<Owned<T>>) -> impl Iterator<Item = &mut T> {
    pool.iter_mut().map(|(_, slot)| &mut slot.value)
}

/// Removes a handle from `pool`, but **only** if `owner` owns it.
///
/// The order is the point: removing first and checking afterwards would already
/// have dropped the entry, so a foreign handle that happened to resolve would
/// destroy this owner's own unrelated object.
pub(crate) fn remove<T, M>(
    pool: &mut Pool<Owned<T>>,
    handle: Handle<M>,
    owner: Owner,
) -> Option<T> {
    let index = resolve(pool, "object", handle, owner).ok()?;
    pool.remove(index).map(|slot| slot.value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::Buffer;

    /// An owner with a chosen id, so the wrap can be exercised at ids no test
    /// run would otherwise reach.
    fn owner(id: u64) -> Owner {
        Owner::new(id)
    }

    /// A tag is never zero, because zero is what a handle nobody issued carries.
    ///
    /// The wrap is asserted rather than assumed: it is the residual hole the
    /// module docs describe, and a `%` that produced a zero tag would make the
    /// wrapping owner accept every hand-made handle.
    #[test]
    fn every_wgpu_owner_id_gets_a_non_zero_tag_and_ids_wrap_rather_than_collide_at_zero() {
        for id in [1u64, 2, 254, 255, 256, 1_000_000] {
            let owner = owner(id);
            assert_ne!(owner.tag, 0, "owner {id} would accept an untagged handle");
            assert_eq!(owner.id, id);
        }
        // Neighbouring ids must differ, or two devices opened back to back would
        // share a tag and fall through to the id check.
        assert_ne!(owner(1).tag, owner(2).tag);
        // And the counter itself never hands out the id whose slot-less tag is
        // reserved, nor the same one twice.
        let first = Owner::next();
        let second = Owner::next();
        assert_ne!(first.id, 0);
        assert_ne!(first.id, second.id);
    }

    /// A stamped handle round-trips: the tag comes back out and the pool index
    /// and generation survive untouched.
    ///
    /// The generation is the half that must not move — `Pool` keys its
    /// staleness on it, so a stamp that disturbed it would make every handle
    /// stale on its first use.
    #[test]
    fn stamping_a_wgpu_handle_preserves_the_pool_index_and_the_generation() {
        let owner = owner(7);
        let mut pool: Pool<Owned<u32>> = Pool::new();
        let stamped: Handle<Buffer> = insert(&mut pool, owner, 11);

        assert_eq!(tag_of(stamped), owner.tag, "the tag did not survive");
        let raw: Handle<Owned<u32>> = local("entry", stamped, owner).expect("this owner's handle");
        assert_eq!(
            stamped.generation(),
            raw.generation(),
            "the generation moved, which would make the handle stale at once"
        );
        assert_eq!(
            stamped.index() & POOL_INDEX_MASK,
            raw.index(),
            "the pool index did not survive"
        );
        assert_eq!(
            *lookup(&pool, "entry", stamped, owner).expect("the entry is live"),
            11
        );
    }

    /// **The three outcomes are three errors.** This is the property the whole
    /// module exists for, and it is checked on handles whose *bits before
    /// stamping are identical* — two fresh pools issue the same index and
    /// generation, so the tag is the only thing that can tell them apart.
    #[test]
    fn a_foreign_wgpu_handle_is_foreign_a_stale_one_is_stale_and_an_untagged_one_is_neither() {
        let a = owner(1);
        let b = owner(2);
        let mut pool_a: Pool<Owned<u32>> = Pool::new();
        let mut pool_b: Pool<Owned<u32>> = Pool::new();
        let on_a: Handle<Buffer> = insert(&mut pool_a, a, 1);
        let on_b: Handle<Buffer> = insert(&mut pool_b, b, 2);
        assert_eq!(
            on_a.index() & POOL_INDEX_MASK,
            on_b.index() & POOL_INDEX_MASK,
            "two fresh pools must issue the same slot, or this test proves nothing"
        );
        assert_eq!(on_a.generation(), on_b.generation());
        assert_ne!(on_a, on_b, "the tag is the only difference and it vanished");

        let error = lookup(&pool_b, "entry", on_a, b).expect_err("A's handle is not B's");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "entry"),
            "{error:?}"
        );
        // B's own still resolves, so the check is not simply refusing
        // everything.
        lookup(&pool_b, "entry", on_b, b).expect("B's own handle");
        // The mutable path answers the same three ways.
        let error = lookup_mut(&mut pool_b, "entry", on_a, b).expect_err("A's handle is not B's");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "entry"),
            "{error:?}"
        );
        *lookup_mut(&mut pool_b, "entry", on_b, b).expect("B's own handle") = 3;

        // A destroy with a foreign handle must not take the local object that
        // shares its bits.
        assert!(
            remove(&mut pool_b, on_a, b).is_none(),
            "a foreign handle removed a local entry"
        );
        assert_eq!(
            *lookup(&pool_b, "entry", on_b, b).expect("B's entry survived a foreign destroy"),
            3
        );

        // Destroyed, then stale — a different error from foreign.
        assert_eq!(remove(&mut pool_b, on_b, b), Some(3));
        let error = lookup(&pool_b, "entry", on_b, b).expect_err("the entry was removed");
        assert!(matches!(error, HalError::InvalidHandle { .. }), "{error:?}");

        // A hand-made handle carries no tag at all, so no owner ever issued it.
        let untagged: Handle<Buffer> = Handle::from_bits(1 << 32).expect("generation 1");
        assert_eq!(tag_of(untagged), 0);
        let error = lookup(&pool_a, "entry", untagged, a).expect_err("nobody issued that");
        assert!(matches!(error, HalError::InvalidHandle { .. }), "{error:?}");
        // …and the unpooled path — a queue handle — classifies the same three
        // ways, which is the only reason `not_ours` is reachable on its own.
        assert!(
            matches!(
                not_ours("queue", untagged, a),
                HalError::InvalidHandle { .. }
            ),
            "an untagged queue handle is nobody's, not another device's"
        );
        assert!(
            matches!(not_ours("queue", on_b, a), HalError::ForeignObject { .. }),
            "B's queue handle offered to A is foreign"
        );
    }

    /// **The slot's `u64` is a real check, not decoration**: a row belongs to
    /// the owner that filled it, and a second owner cannot read or remove it
    /// even when the tag lets it through.
    ///
    /// The two owners here are chosen to *collide* — ids
    /// [`OWNER_TAG_COUNT`] apart stamp the same tag — because that is the only
    /// arrangement in which the tag is no help and the id is the whole answer.
    ///
    /// What this deliberately does **not** claim is that colliding owners are
    /// separable in general. They are not: with a pool per owner, a foreign
    /// handle lands on a row the *looking-up* owner filled, so the id agrees and
    /// the lookup succeeds. That is the residual hole the module docs state, and
    /// asserting it away here would be asserting a bug.
    #[test]
    fn a_wgpu_slot_belongs_to_the_owner_that_filled_it_even_when_two_tags_collide() {
        let a = owner(1);
        let b = owner(1 + OWNER_TAG_COUNT);
        assert_eq!(
            a.tag, b.tag,
            "this test needs a tag collision to be about it"
        );
        assert_ne!(a.id, b.id);

        let mut pool: Pool<Owned<u32>> = Pool::new();
        let filled_by_a: Handle<Buffer> = insert(&mut pool, a, 1);
        assert_eq!(
            tag_of(filled_by_a),
            b.tag,
            "the tag cannot refuse this handle, which is the point"
        );

        let error = lookup(&pool, "entry", filled_by_a, b).expect_err("A filled that row, not B");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "entry"),
            "{error:?}"
        );
        assert!(
            remove(&mut pool, filled_by_a, b).is_none(),
            "B removed a row A owns"
        );
        assert_eq!(*lookup(&pool, "entry", filled_by_a, a).expect("A's own"), 1);
    }

    /// A recycled slot must not resurrect the handle that used to name it —
    /// the generation half has to survive the stamp for that to hold.
    #[test]
    fn a_destroyed_wgpu_handle_does_not_alias_the_entry_that_replaces_it() {
        let owner = owner(3);
        let mut pool: Pool<Owned<u32>> = Pool::new();
        let first: Handle<Buffer> = insert(&mut pool, owner, 1);
        assert_eq!(remove(&mut pool, first, owner), Some(1));
        let second: Handle<Buffer> = insert(&mut pool, owner, 2);

        assert_eq!(
            first.index(),
            second.index(),
            "the free list should have handed back the same slot; if not, this test is not \
             exercising recycling at all"
        );
        assert_ne!(
            first, second,
            "the pool reissued the identical handle, so the generation never moved"
        );
        assert_eq!(*lookup(&pool, "entry", second, owner).expect("live"), 2);
        let error = lookup(&pool, "entry", first, owner).expect_err("the dead handle");
        assert!(matches!(error, HalError::InvalidHandle { .. }), "{error:?}");
    }
}
