//! Asset identity, load state, and the IO seam underneath both.
//!
//! This is step 2 of `docs/plan/06-assets-scenes.md`: names for assets
//! ([`AssetId`]), a place to put them while they load ([`AssetRegistry`]), and
//! the one trait every byte an asset is made of arrives through
//! ([`AssetSource`], with [`DirSource`] as the native implementation). Nothing
//! here knows what an asset *is* — bytes in, bytes out. Decoding (glTF, PNG,
//! WAV) is step 3 and lands in the crates that own those formats.
//!
//! # Why a crate of its own
//!
//! `crcbl-store` is persistence: data the *player* produces — saves, settings,
//! profiles — written by the game and read back later. Assets are the opposite
//! direction: read-only content shipped *with* the game. The two share an IO
//! layer and nothing else, and folding assets into `crcbl-store` would put the
//! asset registry in front of every consumer that only wanted a settings file.
//!
//! `crcbl-scene` was the other candidate and is the wrong one for a plainer
//! reason: assets are not scenes. A scene *references* assets, so the
//! dependency runs `crcbl-scene` → `crcbl-assets`, and putting the registry in
//! the crate that will hold the scene graph would invert it.
//!
//! # What this crate reuses rather than re-deriving
//!
//! All of the IO. [`DirSource`] is a read-only view of
//! [`crcbl_store::NativeStorage`], the error type is
//! [`StorageError`], and the key rule is
//! [`crcbl_store::web::canonical_key`] — the same function the browser backend
//! validates fetch keys with. That last one is the load-bearing reuse: it means
//! a key that names an asset in a directory names the same asset over HTTP, so
//! an asset tree that loads natively is one that can ship to the web without a
//! second set of rules to discover it does not satisfy.
//!
//! # Async from day one
//!
//! The browser has no blocking filesystem, so [`AssetSource::read`] is defined
//! to never block: a source that does not have the bytes yet answers
//! [`StorageError::Pending`] and the caller
//! polls. That is not a new invention here — it is the shape
//! [`crcbl_store::web::FetchSource`] already implements, and the same
//! request/poll shape `crcbl-hal` uses for `request_device` →
//! `PendingDevice::poll`. See [`source`] for what a browser `AssetSource` costs
//! (a delegation, and no change to any caller).

pub mod registry;
pub mod source;

use core::fmt;

pub use registry::{Asset, AssetHandle, AssetRegistry, AssetState};
pub use source::{AssetSource, DirSource, MemorySource};

/// The error every fallible call in this crate returns, re-exported from
/// [`crcbl_store`].
///
/// Not a new type and not an alias: it is `crcbl-store`'s own error, and it is
/// here because [`AssetSource::read`] hands one back, so a crate that
/// implements or calls the trait has to be able to name it. Without the
/// re-export that crate would depend on `crcbl-store` — the persistence layer —
/// for a type it only sees because it loads assets.
pub use crcbl_store::StorageError;

use std::path::Path;

use crcbl_store::web::canonical_key;

/// A stable, machine-independent name for one asset.
///
/// 128 bits, printed and parsed as 32 lower-case hex digits. Values are opaque:
/// two ids compare equal or they do not, and nothing else about the number
/// means anything.
///
/// # Where the bits come from, and where they will come from
///
/// Today: [`AssetId::from_path`], the leading 16 bytes of the SHA-256 of the
/// canonical key. That is the "path hashing" `docs/plan/06-assets-scenes.md`'s
/// corrections section keeps as a lookup convenience — deliberately *not* the
/// long-term identity scheme, because renaming a file changes its id and
/// silently orphans every reference to it.
///
/// The corrected model in that document gives every asset a sidecar meta file
/// carrying a random 128-bit GUID, created on first import and committed. That
/// import step does not exist yet (it is step 3), so nothing can produce such a
/// GUID here. What this type does is make it a drop-in when it arrives: the id
/// is 128 bits wide *because* a GUID is, and [`AssetId::from_bits`] is how one
/// read out of a sidecar becomes an `AssetId` without the type changing shape.
///
/// [`AssetId::to_bits`] is the other half: an id has to survive being written
/// into a scene chunk and read back, which is what step 4 does with it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetId(u128);

impl AssetId {
    /// The id of the asset at `path`.
    ///
    /// `path` is canonicalised first — `./meshes/crate.glb` and
    /// `meshes/crate.glb` are one asset, and get one id, on every platform.
    ///
    /// # Errors
    ///
    /// [`StorageError::InvalidPath`] if the key is not one an asset may have:
    /// absolute, escaping via `..`, empty, over-long, or containing anything
    /// outside `[A-Za-z0-9._-]` and `/`. See [`crcbl_store::web::canonical_key`]
    /// for the whole rule and the argument for it.
    ///
    /// ```
    /// use crcbl_assets::AssetId;
    /// use std::path::Path;
    ///
    /// let id = AssetId::from_path(Path::new("meshes/crate.glb")).unwrap();
    /// assert_eq!(id, AssetId::from_path(Path::new("./meshes/crate.glb")).unwrap());
    /// assert!(AssetId::from_path(Path::new("../secrets")).is_err());
    /// ```
    pub fn from_path(path: &Path) -> Result<Self, StorageError> {
        let key = canonical_key(path)?;
        Ok(Self::of_key(&key))
    }

    /// The id of an already-canonical key.
    ///
    /// Crate-private because "already canonical" is not something a caller can
    /// be trusted to have established — [`AssetRegistry`] calls it having just
    /// produced the key itself.
    pub(crate) fn of_key(key: &str) -> Self {
        let digest = crcbl_shaders::sha256::sha256(key.as_bytes());
        let mut bits = [0u8; 16];
        bits.copy_from_slice(&digest[..16]);
        Self(u128::from_be_bytes(bits))
    }

    /// The id's bits, for serialization.
    #[inline]
    #[must_use]
    pub const fn to_bits(self) -> u128 {
        self.0
    }

    /// An id from bits produced by [`AssetId::to_bits`], or read out of a
    /// sidecar meta file once those exist.
    #[inline]
    #[must_use]
    pub const fn from_bits(bits: u128) -> Self {
        Self(bits)
    }
}

/// 32 lower-case hex digits, zero-padded — the form a log line, a CLI argument
/// or a scene file writes.
impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

/// Hex, like [`Display`](fmt::Display). The derived form would print a 39-digit
/// decimal that no other rendering of the same id matches.
impl fmt::Debug for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AssetId({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_spellings_of_one_asset_path_produce_one_asset_id() {
        let plain = AssetId::from_path(Path::new("meshes/crate.glb")).unwrap();
        for spelling in [
            "./meshes/crate.glb",
            "meshes/./crate.glb",
            "./meshes/./crate.glb",
        ] {
            assert_eq!(
                AssetId::from_path(Path::new(spelling)).unwrap(),
                plain,
                "{spelling} names the same asset"
            );
        }
    }

    #[test]
    fn different_asset_paths_produce_different_asset_ids() {
        let ids: Vec<_> = [
            "a.glb",
            "b.glb",
            "meshes/a.glb",
            "meshes/b.glb",
            "meshes/a.glb.meta",
        ]
        .into_iter()
        .map(|p| AssetId::from_path(Path::new(p)).unwrap())
        .collect();
        for (i, left) in ids.iter().enumerate() {
            for right in &ids[i + 1..] {
                assert_ne!(left, right);
            }
        }
    }

    /// The id has to be the *same* number on every machine and every run, or a
    /// scene file written on one is unreadable on another. Pinned against a
    /// value computed from the documented rule — SHA-256 of the canonical key,
    /// leading 16 bytes, big-endian — so a change to either half fails here
    /// rather than in a content pipeline.
    #[test]
    fn an_asset_id_is_the_leading_half_of_the_keys_sha256_and_never_moves() {
        let digest = crcbl_shaders::sha256::sha256_hex(b"meshes/crate.glb");
        let id = AssetId::from_path(Path::new("meshes/crate.glb")).unwrap();
        assert_eq!(id.to_string(), digest[..32]);
        // Cross-checked against coreutils:
        // `printf 'meshes/crate.glb' | sha256sum` starts with these digits.
        assert_eq!(id.to_string(), "737a40d22c0ad6789abe1d7bceb40232");
    }

    #[test]
    fn an_asset_id_survives_to_bits_and_back_and_renders_as_padded_hex() {
        let id = AssetId::from_path(Path::new("textures/albedo.png")).unwrap();
        assert_eq!(AssetId::from_bits(id.to_bits()), id);

        let small = AssetId::from_bits(1);
        assert_eq!(small.to_string(), "00000000000000000000000000000001");
        assert_eq!(format!("{small:?}"), format!("AssetId({small})"));
        assert_eq!(
            AssetId::from_bits(u128::MAX).to_string(),
            "ffffffffffffffffffffffffffffffff"
        );
    }

    /// Every key rule `canonical_key` enforces is a key that cannot become an
    /// `AssetId`. The refusals are listed here rather than assumed, because
    /// this is the first place a caller-supplied name is turned into anything.
    #[test]
    fn a_name_that_is_not_a_legal_asset_key_has_no_asset_id() {
        for name in [
            "",
            "..",
            "../secrets",
            "meshes/../../etc/passwd",
            "/etc/passwd",
            "//evil.example/x",
            "C:\\Windows\\win.ini",
            "meshes\\crate.glb",
            "crate.glb?v=1",
            "%2e%2e/up",
        ] {
            assert!(
                matches!(
                    AssetId::from_path(Path::new(name)),
                    Err(StorageError::InvalidPath(_))
                ),
                "{name:?} must not have an id"
            );
        }
    }
}
