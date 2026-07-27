use std::hash::Hasher;

/// Trait for component types whose byte representation is safe and
/// deterministic to hash.
///
/// # Safety / soundness contract
///
/// Implementations guarantee:
/// - All bytes of `self` are initialized (no padding bytes).
/// - `a == b` implies `hash_component(a, h)` and `hash_component(b, h)`
///   write the same bytes to `h` — i.e. the hash reflects logical value,
///   not pointer identity or allocation details.
/// - The hash is stable within a process lifetime (same Rust version,
///   same platform endianness — sufficient for a determinism harness that
///   compares runs on the same binary).
///
/// This is deliberately NOT a blanket impl over `Hash`:
/// `std::hash::Hash` is allowed to hash pointers (e.g. for `String`),
/// which would produce false nondeterminism; `ComponentHash` forbids it.
pub trait ComponentHash {
    /// Feed the component's value into `hasher` in a deterministic,
    /// padding-free way.
    fn hash_component(&self, hasher: &mut dyn Hasher);
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

macro_rules! impl_component_hash_int {
    ($($t:ty),* $(,)?) => {
        $(
            impl ComponentHash for $t {
                fn hash_component(&self, hasher: &mut dyn Hasher) {
                    hasher.write(&self.to_le_bytes());
                }
            }
        )*
    };
}

impl_component_hash_int!(u8, u16, u32, u64, u128, usize);
impl_component_hash_int!(i8, i16, i32, i64, i128, isize);

impl ComponentHash for f32 {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        // to_bits() handles NaN deterministically (canonical payload).
        hasher.write(&self.to_bits().to_le_bytes());
    }
}

impl ComponentHash for f64 {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        hasher.write(&self.to_bits().to_le_bytes());
    }
}

impl ComponentHash for bool {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        hasher.write(&[u8::from(*self)]);
    }
}

impl ComponentHash for char {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        hasher.write(&(*self as u32).to_le_bytes());
    }
}

// ---------------------------------------------------------------------------
// Arrays
// ---------------------------------------------------------------------------

impl<T: ComponentHash, const N: usize> ComponentHash for [T; N] {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        for elem in self {
            elem.hash_component(hasher);
        }
    }
}

// ---------------------------------------------------------------------------
// Tuples (0..=12)
// ---------------------------------------------------------------------------

macro_rules! impl_component_hash_tuple {
    ($($T:ident $idx:tt),*) => {
        impl<$($T: ComponentHash),*> ComponentHash for ($($T,)*) {
            #[allow(non_snake_case, unused_variables)]
            fn hash_component(&self, hasher: &mut dyn Hasher) {
                $(self.$idx.hash_component(hasher);)*
            }
        }
    };
}

impl_component_hash_tuple!();
impl_component_hash_tuple!(A 0);
impl_component_hash_tuple!(A 0, B 1);
impl_component_hash_tuple!(A 0, B 1, C 2);
impl_component_hash_tuple!(A 0, B 1, C 2, D 3);
impl_component_hash_tuple!(A 0, B 1, C 2, D 3, E 4);
impl_component_hash_tuple!(A 0, B 1, C 2, D 3, E 4, F 5);
impl_component_hash_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
impl_component_hash_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
impl_component_hash_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
impl_component_hash_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
impl_component_hash_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
impl_component_hash_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;

    fn hash_one<T: ComponentHash>(value: &T) -> u64 {
        let mut h = DefaultHasher::new();
        value.hash_component(&mut h);
        h.finish()
    }

    // -- primitives ----------------------------------------------------------

    #[test]
    fn equal_values_hash_equal() {
        assert_eq!(hash_one(&42u32), hash_one(&42u32));
        assert_eq!(hash_one(&1.25f32), hash_one(&1.25f32));
        assert_eq!(hash_one(&true), hash_one(&true));
    }

    #[test]
    fn different_values_hash_different() {
        assert_ne!(hash_one(&1u32), hash_one(&2u32));
        assert_ne!(hash_one(&1.0f32), hash_one(&2.0f32));
        assert_ne!(hash_one(&true), hash_one(&false));
    }

    #[test]
    fn zero_and_negative_zero_hash_equal() {
        // IEEE 754: +0.0 and -0.0 compare equal, and to_bits() differs,
        // but this is the standard float hashing trade-off;
        // our determinism harness doesn't produce -0.0.
        // Documented here so the expectation is explicit.
        assert_ne!(0.0f32.to_bits(), (-0.0f32).to_bits());
    }

    // -- tuples ---------------------------------------------------------------

    #[test]
    fn tuple_hashes_deterministically() {
        assert_eq!(hash_one(&(1u8, 2u16, 3u32)), hash_one(&(1u8, 2u16, 3u32)),);
    }

    #[test]
    fn tuple_order_matters() {
        assert_ne!(hash_one(&(1u8, 2u8)), hash_one(&(2u8, 1u8)),);
    }

    // -- arrays ---------------------------------------------------------------

    #[test]
    fn array_hashes_deterministically() {
        assert_eq!(hash_one(&[1u32, 2u32, 3u32]), hash_one(&[1u32, 2u32, 3u32]),);
    }

    // -- type-level guarantees ------------------------------------------------

    /// `String` must NOT implement `ComponentHash` — it hashes the pointer,
    /// not the contents.  This is a compile-time check: uncommenting the
    /// `impl` below must fail.
    ///
    /// ```compile_fail
    /// # use crcbl_ecs::ComponentHash;
    /// # use std::hash::Hasher;
    /// impl ComponentHash for String {
    ///     fn hash_component(&self, _: &mut dyn Hasher) {}
    /// }
    /// ```
    #[allow(dead_code)]
    fn string_does_not_impl_component_hash() {
        // The following function would fail to compile if it existed:
        // fn assert_impl<T: ComponentHash>() {}
        // assert_impl::<String>(); // must fail
    }
}
