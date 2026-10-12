//! Pairs of bodies told never to collide with each other:
//! [`crate::PhysicsSystem::ignore_contacts`].
//!
//! A joint already keeps its own two bodies apart (`joint/mod.rs`); this is
//! the same rule for a pair no joint joins — a ragdoll's two thighs, which
//! meet at the pelvis without touching each other's joint. The broadphase asks
//! both before it makes a pair, so an ignored pair never gets a contact.

use std::collections::HashSet;
use std::hash::Hasher;

use crcbl_core::Pool;

use super::joint::pair_key;
use crate::system::{BodyId, BodyRecord};

/// The ignored pairs, each keyed by its two body ids, lower first.
#[derive(Debug, Default)]
pub(crate) struct IgnoredPairs {
    pairs: HashSet<(u64, u64)>,
}

impl IgnoredPairs {
    /// Ignores the pair; `false` if it already was.
    pub(crate) fn insert(&mut self, a: BodyId, b: BodyId) -> bool {
        self.pairs.insert(pair_key(a, b))
    }

    /// Stops ignoring the pair; `false` if it was not.
    pub(crate) fn remove(&mut self, a: BodyId, b: BodyId) -> bool {
        self.pairs.remove(&pair_key(a, b))
    }

    /// Whether the pair is ignored.
    pub(crate) fn contains(&self, a: BodyId, b: BodyId) -> bool {
        !self.pairs.is_empty() && self.pairs.contains(&pair_key(a, b))
    }

    /// Forgets every pair `id` is in: what removing its entity does, so a
    /// body that later takes its slot starts with none.
    pub(crate) fn forget_body(&mut self, id: BodyId) {
        if self.pairs.is_empty() {
            return;
        }
        let bits = id.to_bits();
        self.pairs.retain(|&(a, b)| a != bits && b != bits);
    }

    /// Feeds the pairs into a determinism hash, by their entities, sorted.
    /// Nothing is written when there are none, so a system that ignores no
    /// pair hashes as it did before pairs could be ignored.
    pub(crate) fn hash_state(&self, records: &Pool<BodyRecord>, hasher: &mut dyn Hasher) {
        if self.pairs.is_empty() {
            return;
        }
        let entity = |bits: u64| {
            crcbl_core::Handle::from_bits(bits)
                .and_then(|id: BodyId| records.get(id))
                .map_or(0, |record| record.entity.to_bits())
        };
        let mut pairs: Vec<(u64, u64)> = self
            .pairs
            .iter()
            .map(|&(a, b)| {
                let (a, b) = (entity(a), entity(b));
                (a.min(b), a.max(b))
            })
            .collect();
        pairs.sort_unstable();
        hasher.write(&[6]);
        hasher.write(&(pairs.len() as u64).to_le_bytes());
        for (a, b) in pairs {
            hasher.write(&a.to_le_bytes());
            hasher.write(&b.to_le_bytes());
        }
    }
}
