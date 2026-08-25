//! Reading a pool's whole state, for the suites that compare two of them.

use crcbl_vfx::ParticleSystem;

/// The pool's arrays, in the order [`bits`] writes them.
///
/// Named so a failure says which attribute drifted rather than which flat
/// offset did.
const FIELDS: &[&str] = &[
    "position.x",
    "position.y",
    "position.z",
    "velocity.x",
    "velocity.y",
    "velocity.z",
    "age",
    "lifetime",
    "size",
    "rotation",
    "color.r",
    "color.g",
    "color.b",
    "color.a",
    "index",
];

/// Every slot of every array in a pool, as IEEE bit patterns.
///
/// # Why bits and not floats
///
/// The claim being checked is that two runs produce the *same state*, and `==`
/// on `f32` answers a different question: it calls `+0.0` and `-0.0` equal, and
/// calls a `NaN` unequal to itself, so a run that had gone quietly wrong in
/// either direction could compare equal or unequal for reasons that have
/// nothing to do with the simulation. `to_bits` is the value's defined
/// encoding, taken field by field — not a read of whatever memory the pool
/// happens to occupy, which would fold in padding and capacity.
pub fn bits(vfx: &ParticleSystem) -> Vec<u32> {
    let pool = vfx.pool();
    let mut out = Vec::with_capacity(FIELDS.len() * pool.capacity() as usize);
    for axis in 0..3 {
        out.extend(pool.positions().iter().map(|v| v[axis].to_bits()));
    }
    for axis in 0..3 {
        out.extend(pool.velocities().iter().map(|v| v[axis].to_bits()));
    }
    out.extend(pool.ages().iter().map(|v| v.to_bits()));
    out.extend(pool.lifetimes().iter().map(|v| v.to_bits()));
    out.extend(pool.sizes().iter().map(|v| v.to_bits()));
    out.extend(pool.rotations().iter().map(|v| v.to_bits()));
    for channel in 0..4 {
        out.extend(pool.colors().iter().map(|v| v[channel].to_bits()));
    }
    out.extend(pool.indices().iter().copied());
    out
}

/// Assert two pools are bit-for-bit the same, naming the first attribute and
/// slot that differ.
///
/// # Panics
///
/// If the two differ anywhere, or if either is empty — an empty digest would
/// make this a check that cannot fail.
pub fn assert_same(what: &str, left: &[u32], right: &[u32]) {
    assert!(
        !left.is_empty() && left.len() == right.len(),
        "{what}: the two digests are {} and {} words long, so nothing was compared",
        left.len(),
        right.len()
    );
    let slots = left.len() / FIELDS.len();
    for (at, (a, b)) in left.iter().zip(right).enumerate() {
        assert!(
            a == b,
            "{what}: {} of slot {} differs — {a:#010x} against {b:#010x}",
            FIELDS[at / slots],
            at % slots,
        );
    }
}

/// Assert two pools are *not* the same, so a check that they are cannot be
/// passing because both sides are empty or frozen.
///
/// # Panics
///
/// If the two are identical.
pub fn assert_differs(what: &str, left: &[u32], right: &[u32]) {
    assert!(
        !left.is_empty(),
        "{what}: the digest is empty, so nothing was compared"
    );
    assert!(
        left != right,
        "{what}: the two pools are identical, so the comparison proves nothing"
    );
}
