//! Which device issued a handle, and how a handle says so.
//!
//! # The bug this module exists because of
//!
//! **Every object table in this backend is per-device**, and every insert stamps
//! the creating device's id — which on its own makes
//! [`HalError::ForeignObject`] unproducible rather than satisfied. Two devices
//! allocating in step reach the same slot index at the same generation
//! immediately, so device A's first buffer handle resolves inside device B's
//! pool to *B's* first buffer, whose owner is B: the check passes and B writes
//! into the wrong object with no error anywhere. That is `crcbl-hal`'s
//! obligation 3 met by accident, and only while the two pools happen to differ.
//!
//! So the handle carries the issuing device. The top byte of the index half is
//! the device's tag; the rest is the pool's own index, restored before any
//! lookup. The generation half is untouched, so [`Pool`]'s generation-exhaustion
//! rule is unaffected. `crcbl-vk` and `crcbl-mtl` use the same scheme, arrived at
//! from the same bug.
//!
//! The residual hole is stated rather than hidden: tags repeat every
//! [`DEVICE_TAG_COUNT`] devices, so a process that opens that many falls back to
//! the owner-id check for the ones that collide, which cannot separate them. A
//! process that opens hundreds of D3D12 devices has a different problem.
//!
//! # Three outcomes, kept apart
//!
//! A handle offered to a device is one of three things, and the seam gives each
//! its own error because the fixes differ:
//!
//! * **This device's, and live** — resolved.
//! * **This device's, and dead** — [`HalError::InvalidHandle`]: "you kept this
//!   too long". A handle carrying *no* tag is here too, because no device ever
//!   issued it.
//! * **Another device's** — [`HalError::ForeignObject`]: "you crossed two
//!   objects that never met".

use crcbl_core::{Handle, Pool};
use crcbl_hal::{HalError, QueueHandle, QueueKind};

/// Bits of a handle's index half given over to the owning device's tag.
const DEVICE_TAG_SHIFT: u32 = 24;
/// The part of a handle's index half that is the pool's own index.
pub(crate) const POOL_INDEX_MASK: u32 = (1 << DEVICE_TAG_SHIFT) - 1;
/// How many distinct device tags exist. Tag `0` is reserved for "nobody", so a
/// hand-made or un-stamped handle is foreign to every device.
const DEVICE_TAG_COUNT: u64 = (u32::MAX >> DEVICE_TAG_SHIFT) as u64;

/// Anything an object table holds, so one lookup helper serves them all.
pub(crate) trait Owned {
    /// Id of the device that created it, per `crcbl-hal`'s obligation 3.
    fn owner(&self) -> u64;
}

/// The device a handle belongs to: its process-wide id, and the tag it stamps.
///
/// Both, because they answer different halves of the question. The **tag** is
/// what a handle can carry — one byte, so it repeats — and the **id** is what
/// the table entry records, which never repeats. A handle is this device's only
/// if both agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    /// Process-wide device id, from `crcbl_dx12::instance`'s counter.
    pub(crate) id: u64,
    /// The tag this device stamps into every handle it issues. Never zero.
    pub(crate) tag: u32,
}

impl Owner {
    /// The owner a device with this id stamps with.
    pub(crate) fn new(id: u64) -> Self {
        #[allow(clippy::cast_possible_truncation)]
        let tag = 1 + (id % DEVICE_TAG_COUNT) as u32;
        Self { id, tag }
    }
}

/// The device tag a handle carries, or `0` if it carries none.
pub(crate) const fn tag_of<M>(handle: Handle<M>) -> u32 {
    handle.index() >> DEVICE_TAG_SHIFT
}

/// Strips the device tag, recovering the pool's own handle.
fn untag<A, B>(handle: Handle<A>) -> Handle<B> {
    Handle::from_bits(
        (u64::from(handle.generation()) << 32) | u64::from(handle.index() & POOL_INDEX_MASK),
    )
    .unwrap_or_else(|| unreachable!("a handle's generation is never zero"))
}

/// Stamps a device's tag into a handle its pools just issued.
///
/// Every handle that crosses the seam goes through here; every handle that comes
/// back goes through [`local`]. A pool index too large to carry the tag gets tag
/// `0`, which resolves nowhere — the object leaks until the device is dropped,
/// which is far better than a handle that might resolve to another device's
/// object. It takes more live objects of one kind than [`POOL_INDEX_MASK`]
/// admits to reach.
pub(crate) fn stamp<A, B>(owner: Owner, handle: Handle<A>) -> Handle<B> {
    let index = handle.index();
    let tag = if index > POOL_INDEX_MASK {
        crcbl_core::log::error!(
            "crcbl-dx12: pool index {index} is too large to carry a device tag; issuing a handle \
             that resolves nowhere rather than one that might resolve to another device's object"
        );
        0
    } else {
        owner.tag
    };
    Handle::from_bits(
        (u64::from(handle.generation()) << 32) | u64::from((tag << DEVICE_TAG_SHIFT) | index),
    )
    .unwrap_or_else(|| unreachable!("a handle's generation is never zero"))
}

/// Deterministic queue handles.
///
/// Queues are not pooled: there is exactly one for a device's lifetime and it
/// carries no state a caller can hold, so
/// [`Device::queue`](crcbl_hal::Device::queue) stays a pure function. The
/// device's tag rides in the same place it does on a pooled handle, because
/// obligation 3 covers queues too — a [`QueueHandle`] synthesised from the kind
/// alone would carry no device identity at all, and every device would accept
/// every other device's.
pub(crate) fn queue(owner: Owner, kind: QueueKind) -> QueueHandle {
    Handle::from_bits((1u64 << 32) | u64::from((owner.tag << DEVICE_TAG_SHIFT) | queue_index(kind)))
        .unwrap_or_else(|| unreachable!("generation 1 is non-zero"))
}

const fn queue_index(kind: QueueKind) -> u32 {
    match kind {
        QueueKind::Graphics => 0,
        QueueKind::Compute => 1,
        QueueKind::Transfer => 2,
    }
}

/// Decodes a handle for `owner`'s pools, or says why it is not one.
///
/// # Errors
///
/// [`HalError::InvalidHandle`] for a handle no device issued, or
/// [`HalError::ForeignObject`] for one another device did.
pub(crate) fn local<E, M>(
    kind: &'static str,
    handle: Handle<M>,
    owner: Owner,
) -> Result<Handle<E>, HalError> {
    let tag = tag_of(handle);
    if tag == owner.tag {
        return Ok(untag(handle));
    }
    // Tag zero was never issued by any device — a hand-made handle, or one whose
    // pool index overflowed the tagged range. Anything else is a real handle
    // belonging to a real, different device, which is the case obligation 3
    // exists for.
    Err(if tag == 0 {
        HalError::invalid_handle(kind, handle)
    } else {
        HalError::ForeignObject {
            kind,
            bits: handle.to_bits(),
        }
    })
}

/// Resolves a handle against a pool and its owning device.
///
/// # Errors
///
/// As [`local`], plus [`HalError::InvalidHandle`] for a handle whose slot is
/// empty or whose generation has moved on.
pub(crate) fn lookup<'p, E: Owned, M>(
    pool: &'p Pool<E>,
    kind: &'static str,
    handle: Handle<M>,
    owner: Owner,
) -> Result<&'p E, HalError> {
    let index = local(kind, handle, owner)?;
    match pool.get(index) {
        Some(entry) if entry.owner() == owner.id => Ok(entry),
        Some(_) => Err(HalError::ForeignObject {
            kind,
            bits: handle.to_bits(),
        }),
        None => Err(HalError::invalid_handle(kind, handle)),
    }
}

/// [`lookup`], for an entry the caller has to change.
///
/// Separate rather than `lookup` returning a `&mut`, because every other table
/// in this backend is read through the shared form and a swapchain is the one
/// object with per-frame state on it — the acquired index and the present
/// ledger.
///
/// # Errors
///
/// As [`lookup`].
pub(crate) fn lookup_mut<'p, E: Owned, M>(
    pool: &'p mut Pool<E>,
    kind: &'static str,
    handle: Handle<M>,
    owner: Owner,
) -> Result<&'p mut E, HalError> {
    let index = local(kind, handle, owner)?;
    // The owner check happens through an immutable borrow that ends before the
    // mutable one begins, which is what stops a foreign handle from handing
    // back a mutable reference to this device's own object.
    match pool.get(index).map(Owned::owner) {
        Some(entry) if entry == owner.id => Ok(pool
            .get_mut(index)
            .unwrap_or_else(|| unreachable!("the slot resolved a moment ago"))),
        Some(_) => Err(HalError::ForeignObject {
            kind,
            bits: handle.to_bits(),
        }),
        None => Err(HalError::invalid_handle(kind, handle)),
    }
}

/// Removes a handle from `pool`, but **only** if `owner` owns it.
///
/// The order is the point: removing first and checking the owner afterwards
/// would already have dropped the entry, so a foreign handle that happened to
/// resolve would destroy this device's own unrelated object.
pub(crate) fn take_owned<E: Owned, M>(
    pool: &mut Pool<E>,
    handle: Handle<M>,
    owner: Owner,
) -> Option<E> {
    let index = local::<E, M>("object", handle, owner).ok()?;
    if !pool
        .get(index)
        .is_some_and(|entry| entry.owner() == owner.id)
    {
        return None;
    }
    pool.remove(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::Buffer;

    /// A stand-in table entry, so the lookup rules are testable without a
    /// device — which is the whole point of this module being separate from
    /// one.
    #[derive(Debug)]
    struct Entry {
        owner: u64,
    }

    impl Owned for Entry {
        fn owner(&self) -> u64 {
            self.owner
        }
    }

    /// A tag is never zero, because zero is what a handle nobody issued carries.
    ///
    /// The wrap is asserted rather than assumed: it is the residual hole the
    /// module docs describe, and a `%` that produced a zero tag would make the
    /// wrapping device accept every hand-made handle.
    #[test]
    fn every_d3d12_device_id_gets_a_non_zero_tag_and_ids_wrap_rather_than_collide_at_zero() {
        for id in [1u64, 2, 254, 255, 256, 1_000_000] {
            let owner = Owner::new(id);
            assert_ne!(owner.tag, 0, "device {id} would accept an untagged handle");
            assert_eq!(owner.id, id);
        }
        // Neighbouring ids must differ, or two devices opened back to back would
        // share a tag and fall through to the id check.
        assert_ne!(Owner::new(1).tag, Owner::new(2).tag);
    }

    /// A stamped handle round-trips: the tag comes back out and the pool index
    /// and generation survive untouched.
    ///
    /// The generation is the half that must not move — `Pool` keys its
    /// staleness on it, so a stamp that disturbed it would make every handle
    /// stale on its first use.
    #[test]
    fn stamping_a_d3d12_handle_preserves_the_pool_index_and_the_generation() {
        let owner = Owner::new(7);
        let mut pool: Pool<Entry> = Pool::new();
        let raw = pool.insert(Entry { owner: owner.id });

        let stamped: Handle<Buffer> = stamp(owner, raw);
        assert_eq!(tag_of(stamped), owner.tag, "the tag did not survive");
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

        let back: Handle<Entry> = local("entry", stamped, owner).expect("this device's own handle");
        assert_eq!(
            back, raw,
            "the round trip did not recover the pool's handle"
        );
        assert_eq!(
            lookup(&pool, "entry", stamped, owner)
                .expect("the entry is live")
                .owner,
            owner.id
        );
    }

    /// **The three outcomes are three errors.** This is the property the whole
    /// module exists for, and it is checked on handles whose *bits are
    /// identical* — two fresh pools issue the same index and generation, so the
    /// tag is the only thing that can tell them apart.
    #[test]
    fn a_foreign_d3d12_handle_is_foreign_a_stale_one_is_stale_and_an_untagged_one_is_neither() {
        let a = Owner::new(1);
        let b = Owner::new(2);
        let mut pool_a: Pool<Entry> = Pool::new();
        let mut pool_b: Pool<Entry> = Pool::new();
        let raw_a = pool_a.insert(Entry { owner: a.id });
        let raw_b = pool_b.insert(Entry { owner: b.id });
        assert_eq!(
            raw_a, raw_b,
            "two fresh pools must issue identical bits, or this test proves nothing"
        );

        let on_a: Handle<Buffer> = stamp(a, raw_a);
        let on_b: Handle<Buffer> = stamp(b, raw_b);
        assert_ne!(on_a, on_b, "the tag is the only difference and it vanished");

        let error = lookup(&pool_b, "entry", on_a, b).expect_err("A's handle is not B's");
        assert!(
            matches!(error, HalError::ForeignObject { kind, .. } if kind == "entry"),
            "{error:?}"
        );
        // B's own still resolves, so the check is not simply refusing
        // everything.
        lookup(&pool_b, "entry", on_b, b).expect("B's own handle");

        // A destroy with a foreign handle must not take the local object that
        // shares its bits.
        assert!(
            take_owned(&mut pool_b, on_a, b).is_none(),
            "a foreign handle removed a local entry"
        );
        lookup(&pool_b, "entry", on_b, b).expect("B's entry survived a foreign destroy");

        // Destroyed, then stale — a different error from foreign.
        assert!(take_owned(&mut pool_b, on_b, b).is_some());
        let error = lookup(&pool_b, "entry", on_b, b).expect_err("the entry was removed");
        assert!(matches!(error, HalError::InvalidHandle { .. }), "{error:?}");

        // A hand-made handle carries no tag at all, so no device ever issued it.
        let untagged: Handle<Buffer> = Handle::from_bits(1 << 32).expect("generation 1");
        assert_eq!(tag_of(untagged), 0);
        let error = local::<Entry, _>("entry", untagged, a).expect_err("nobody issued that");
        assert!(matches!(error, HalError::InvalidHandle { .. }), "{error:?}");
    }

    /// Queue handles are per device and per kind, and carry the tag so a queue
    /// from another device is detectable.
    #[test]
    fn d3d12_queue_handles_differ_by_device_and_by_kind() {
        let a = Owner::new(1);
        let b = Owner::new(2);
        let kinds = [QueueKind::Graphics, QueueKind::Compute, QueueKind::Transfer];
        let mut seen = Vec::new();
        for owner in [a, b] {
            for kind in kinds {
                let handle = queue(owner, kind);
                assert_eq!(tag_of(handle), owner.tag, "{kind:?}");
                assert!(
                    !seen.contains(&handle),
                    "{kind:?} on device {} duplicates an earlier queue handle",
                    owner.id
                );
                seen.push(handle);
            }
        }
        assert_eq!(seen.len(), kinds.len() * 2);
    }
}
