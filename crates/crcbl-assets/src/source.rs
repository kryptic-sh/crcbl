//! The IO seam: [`AssetSource`], and [`DirSource`] over a directory.
//!
//! # One method, and why it is not two
//!
//! An asset source answers exactly one question — *the bytes of this key* — and
//! answers it without blocking. There is no separate `request` call: a source
//! that has to go and get the bytes starts doing so as a side effect of the
//! read that missed, and says [`StorageError::Pending`] meanwhile. That is not
//! a shape invented here; it is
//! [`crcbl_store::web::FetchSource`]'s
//! documented contract, and a second, differently-shaped answer to "an API that
//! must work in a browser too" is exactly the drift worth not adding.
//!
//! # What a browser source costs
//!
//! Nothing in [`AssetRegistry`](crate::AssetRegistry) or any other caller. The
//! browser implementation is:
//!
//! ```
//! use crcbl_assets::AssetSource;
//! use crcbl_store::{StorageError, StorageSource};
//! use std::path::Path;
//!
//! #[derive(Debug)]
//! struct FetchAssets(crcbl_store::web::FetchSource);
//!
//! impl AssetSource for FetchAssets {
//!     fn read(&self, key: &Path) -> Result<Vec<u8>, StorageError> {
//!         self.0.read(key)
//!     }
//! }
//! ```
//!
//! — because `FetchSource::read` already canonicalises the key, already
//! enqueues on a miss, and already answers `Pending`. It is not written here
//! because stage 10 owns the browser asset path and a wrapper with no consumer
//! is a wrapper nobody has checked; the point is that the trait does not stand
//! in its way.
//!
//! # Why the trait exists at all, given `StorageSource`
//!
//! [`crcbl_store::StorageSource`] has `read`, and a blanket
//! `impl<S: StorageSource> AssetSource for S` would have made every storage
//! backend an asset source for free. It is not done, for two reasons. The
//! narrow one: an asset source must not be writable, and `write`/`delete`
//! returning [`StorageError::Unsupported`] at run time is a weaker statement
//! than a trait that has no such method. The structural one: a blanket impl
//! claims the trait for every present and future `StorageSource`, so
//! `PackSource` — a baked blob, which is not directory-shaped storage — could
//! not implement `AssetSource` on its own terms without coherence forbidding
//! it. Reuse the implementations, not the trait: [`DirSource`] below delegates
//! to `NativeStorage` and adds nothing but the key check.

use std::fmt;
use std::path::{Path, PathBuf};

use crcbl_store::web::{canonical_dir, canonical_key};
use crcbl_store::{NativeStorage, StorageError, StorageSource};

/// A read-only source of asset bytes that never blocks.
///
/// The only IO seam engine code loads content through. Implementations decide
/// where bytes come from — a directory ([`DirSource`]), a server, a baked blob
/// — but not whether the call may stall, which is fixed at "it may not".
pub trait AssetSource: fmt::Debug {
    /// The bytes of the asset named by `key`, or why they are not here.
    ///
    /// **Never blocks, on any implementation.** A source that must perform IO
    /// it cannot complete immediately starts it and answers
    /// [`StorageError::Pending`]; the caller polls again next frame. A source
    /// that can answer at once (a directory, a resident cache) does.
    ///
    /// `key` is relative to whatever the source's root is, and must satisfy
    /// [`crcbl_store::web::canonical_key`] — the implementation is required to
    /// enforce that rather than assume it, because the key reaches it from
    /// caller-supplied names.
    ///
    /// # Errors
    ///
    /// - [`StorageError::Pending`] — in flight; not a failure, a state.
    /// - [`StorageError::NotFound`] — the source is sure it does not have it.
    /// - [`StorageError::InvalidPath`] — `key` is not a legal asset key.
    /// - anything else the backing store produces, unchanged.
    fn read(&self, key: &Path) -> Result<Vec<u8>, StorageError>;

    /// The entries directly inside the directory `dir` names, in key order —
    /// what an asset browser or an in-app file picker walks.
    ///
    /// `dir` is a key as [`read`](Self::read) takes one, and the empty path (or
    /// `.`) is the source's root. Each entry's key is the full key from the
    /// root, so it can be handed straight back to `read` or to `list`. Only
    /// entries whose names are legal keys are listed: a file called
    /// `my asset.png` could never be read, and listing it would offer a thing
    /// that fails when chosen.
    ///
    /// # Errors
    ///
    /// - [`StorageError::Unsupported`] — **the default**, for a source that
    ///   cannot enumerate what it holds: a browser fetching by URL has no
    ///   directory to ask. Said rather than answered with an empty list, which
    ///   would read as a directory with nothing in it.
    /// - [`StorageError::NotFound`] — no directory by that key.
    /// - [`StorageError::InvalidPath`] — `dir` is not a legal key.
    fn list(&self, _dir: &Path) -> Result<Vec<AssetEntry>, StorageError> {
        Err(StorageError::Unsupported("listing assets"))
    }
}

/// One entry [`AssetSource::list`] found.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AssetEntry {
    /// The full key from the source's root, `/`-separated.
    pub key: String,
    /// Whether the key names a directory, which lists, rather than an asset,
    /// which reads.
    pub is_dir: bool,
}

/// The native [`AssetSource`]: a directory on disk.
///
/// A read-only view of [`NativeStorage`] rooted at the asset directory, so the
/// containment argument, the error classification and the atomic-write
/// machinery all stay in the one crate that owns them.
///
/// # Containment
///
/// Two layers, and both are wanted:
///
/// 1. [`crcbl_store::web::canonical_key`] refuses anything that is not a legal
///    asset key — including names `NativeStorage` alone would happily accept,
///    like `my asset.png` or `café/x.png`. That is deliberate: those load here
///    and 404 over HTTP, and an asset tree that only works natively is one that
///    fails at the point it is hardest to fix.
/// 2. `NativeStorage::resolve` refuses `..` and absolute paths before joining.
///    Layer 1 already refuses both; layer 2 is the store's own invariant and
///    stays enforced by the store, so `DirSource` is not the thing standing
///    between a name and `std::fs`.
#[derive(Debug)]
pub struct DirSource {
    inner: NativeStorage,
}

impl DirSource {
    /// A source reading from `root`.
    ///
    /// The directory is not created and not required to exist: a missing root
    /// makes every read a [`StorageError::NotFound`], which is what a missing
    /// asset is anyway.
    #[must_use]
    pub fn at(root: PathBuf) -> Self {
        Self {
            inner: NativeStorage::at(root),
        }
    }

    /// The directory this source reads from.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.inner.root()
    }
}

impl AssetSource for DirSource {
    /// Reads the file at `key` under [`root`](DirSource::root).
    ///
    /// Never [`StorageError::Pending`]: a directory read either completes or
    /// fails. That is the whole difference between this and the browser source,
    /// and it is why a caller written against the trait works on both.
    fn read(&self, key: &Path) -> Result<Vec<u8>, StorageError> {
        let key = canonical_key(key)?;
        self.inner.read(Path::new(&key))
    }

    /// Lists the directory `dir` names under [`root`](DirSource::root),
    /// through [`NativeStorage`]'s own listing and containment.
    fn list(&self, dir: &Path) -> Result<Vec<AssetEntry>, StorageError> {
        let dir = canonical_dir(dir)?;
        let mut entries: Vec<AssetEntry> = self
            .inner
            .list(Path::new(&dir))?
            .into_iter()
            .filter_map(|path| {
                // Joined with `/` here: the store's listing joins with the
                // host's separator, and a key is a URL path on every host.
                let spelled: Vec<&str> = path
                    .components()
                    .map(|part| part.as_os_str().to_str())
                    .collect::<Option<_>>()?;
                let key = canonical_key(Path::new(&spelled.join("/"))).ok()?;
                let is_dir = self.inner.root().join(&path).is_dir();
                Some(AssetEntry { key, is_dir })
            })
            .collect();
        entries.sort();
        Ok(entries)
    }
}

/// A source over bytes that are already in memory.
///
/// **What it is for**: content that never lived in a directory. A `.glb` a
/// `build.rs` generated and `include_bytes!` compiled into the module, and — the
/// case that drove it — a document a browser handed over, where there is no
/// filesystem to point a [`DirSource`] at and no URL to fetch. `apps/viewer`
/// opens its web demo's model through one.
///
/// **Never [`StorageError::Pending`]**, for the same reason [`DirSource`] is
/// not: the bytes are here, so the read either finds the key or does not.
///
/// The key is canonicalised on the way in *and* on the way out, so a document
/// filed as `model.glb` is found by `./model.glb` — the spelling-independence
/// [`DirSource`] gives, given by the same function rather than by a second rule
/// that could disagree with it.
#[derive(Debug, Default)]
pub struct MemorySource {
    /// Canonical key to bytes. A map rather than a single entry because a glTF
    /// document may name buffers and images beside it, and
    /// `crcbl_scene::import_gltf` reads each of those through this same source.
    entries: std::collections::BTreeMap<String, Vec<u8>>,
}

impl MemorySource {
    /// An empty source. Every read is [`StorageError::NotFound`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Files `bytes` under `key`, replacing anything already there.
    ///
    /// # Errors
    ///
    /// [`StorageError::InvalidPath`] if `key` is not a legal asset key —
    /// refused here rather than at the read, because a key that can never be
    /// found is a caller's bug at the moment it is written, and a source that
    /// accepted it would answer `NotFound` for ever and look like a missing
    /// file.
    pub fn insert(&mut self, key: &Path, bytes: Vec<u8>) -> Result<(), StorageError> {
        self.entries.insert(canonical_key(key)?, bytes);
        Ok(())
    }
}

impl AssetSource for MemorySource {
    fn read(&self, key: &Path) -> Result<Vec<u8>, StorageError> {
        let key = canonical_key(key)?;
        self.entries
            .get(&key)
            .cloned()
            .ok_or(StorageError::NotFound(PathBuf::from(key)))
    }

    /// Lists what is filed under `dir`: a key with nothing further is an asset,
    /// and the first segment of a longer one is a directory — a memory source
    /// has no directories of its own, only the keys that imply them.
    fn list(&self, dir: &Path) -> Result<Vec<AssetEntry>, StorageError> {
        let dir = canonical_dir(dir)?;
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut entries: Vec<AssetEntry> = Vec::new();
        for key in self.entries.keys().filter(|key| key.starts_with(&prefix)) {
            let rest = &key[prefix.len()..];
            let entry = match rest.split_once('/') {
                Some((child, _)) => AssetEntry {
                    key: format!("{prefix}{child}"),
                    is_dir: true,
                },
                None => AssetEntry {
                    key: key.clone(),
                    is_dir: false,
                },
            };
            if entries.last() != Some(&entry) {
                entries.push(entry);
            }
        }
        if entries.is_empty() && !dir.is_empty() {
            return Err(StorageError::NotFound(PathBuf::from(dir)));
        }
        entries.sort();
        entries.dedup();
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A memory source holding one document under a nested key.
    fn resident() -> MemorySource {
        let mut source = MemorySource::new();
        source
            .insert(Path::new("meshes/crate.glb"), b"glTF fake".to_vec())
            .expect("a nested key is a legal asset key");
        source
    }

    #[test]
    fn a_memory_source_reads_back_what_was_put_in_it() {
        let source = resident();
        assert_eq!(
            source.read(Path::new("meshes/crate.glb")).unwrap(),
            b"glTF fake"
        );
        // The other spelling of the same key, because both ends canonicalise.
        assert_eq!(
            source.read(Path::new("./meshes/crate.glb")).unwrap(),
            b"glTF fake"
        );
    }

    #[test]
    fn a_memory_source_reports_a_key_it_has_not_got_as_not_found() {
        let source = resident();
        assert!(
            matches!(
                source.read(Path::new("meshes/barrel.glb")),
                Err(StorageError::NotFound(_))
            ),
            "a key nothing was filed under is missing, not an error about the key"
        );
    }

    /// **Refused when it is written, not when it is read.** A key that cannot
    /// canonicalise can never be found again, so storing it would turn a
    /// caller's bug into a permanent `NotFound` that reads like a missing file.
    #[test]
    fn a_memory_source_refuses_an_illegal_key_at_the_insert() {
        let mut source = MemorySource::new();
        let error = source
            .insert(Path::new("../secrets.txt"), b"do not read me".to_vec())
            .expect_err("an escaping key is not a legal asset key");
        assert!(matches!(error, StorageError::InvalidPath(_)), "{error:?}");
        assert!(
            matches!(
                source.read(Path::new("../secrets.txt")),
                Err(StorageError::InvalidPath(_))
            ),
            "and it was not stored under some other spelling either"
        );
    }

    /// A root with one file in it, plus a sibling directory outside the root
    /// holding a file an escape would reach.
    fn fixture() -> (tempfile::TempDir, DirSource) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("assets");
        std::fs::create_dir_all(root.join("meshes")).unwrap();
        std::fs::write(root.join("meshes/crate.glb"), b"glTF fake").unwrap();
        std::fs::write(dir.path().join("secrets.txt"), b"do not read me").unwrap();
        let source = DirSource::at(root);
        (dir, source)
    }

    #[test]
    fn a_dir_source_reads_a_file_that_is_really_on_disk() {
        let (_dir, source) = fixture();
        assert_eq!(
            source.read(Path::new("meshes/crate.glb")).unwrap(),
            b"glTF fake"
        );
        // The same file under its other spelling, because the key is
        // canonicalised before it reaches the filesystem.
        assert_eq!(
            source.read(Path::new("./meshes/crate.glb")).unwrap(),
            b"glTF fake"
        );
    }

    #[test]
    fn a_dir_source_reports_a_missing_file_as_not_found_rather_than_panicking() {
        let (_dir, source) = fixture();
        assert!(matches!(
            source.read(Path::new("meshes/absent.glb")),
            Err(StorageError::NotFound(_))
        ));
        assert!(matches!(
            source.read(Path::new("nothing.glb")),
            Err(StorageError::NotFound(_))
        ));
    }

    #[test]
    fn a_dir_source_with_no_directory_behind_it_reports_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let source = DirSource::at(dir.path().join("never-created"));
        assert!(matches!(
            source.read(Path::new("a.glb")),
            Err(StorageError::NotFound(_))
        ));
    }

    /// Every shape of name that could make the join address something outside
    /// the root, enumerated. The file `secrets.txt` really exists one level
    /// above the root, so a traversal that got through would return its bytes
    /// rather than an error — which is what makes these assertions able to
    /// fail.
    #[test]
    fn a_dir_source_refuses_every_key_that_would_escape_its_root() {
        let (dir, source) = fixture();
        let absolute = dir.path().join("secrets.txt");
        let absolute = absolute.to_str().unwrap();

        for key in [
            // Parent traversal, at every position a joiner might miss.
            "../secrets.txt",
            "./../secrets.txt",
            "meshes/../../secrets.txt",
            "meshes/..",
            "..",
            // Absolute: `Path::join` would discard the root entirely.
            absolute,
            "/etc/passwd",
            // Root-relative and authority-relative, which are absolute over
            // HTTP even where they are not on this filesystem.
            "/",
            "//evil.example/x",
            // Windows spellings: a prefix, and a separator that is a separator
            // on one host and a filename byte on another.
            "C:\\Windows\\win.ini",
            "\\\\host\\share\\x",
            "meshes\\..\\..\\secrets.txt",
            // Percent-encoded traversal, which a server decodes and this does
            // not.
            "%2e%2e/secrets.txt",
            // Empty and dot-only: no asset is named by them.
            "",
            ".",
            "./",
        ] {
            let refused = source.read(Path::new(key));
            assert!(
                matches!(refused, Err(StorageError::InvalidPath(_))),
                "{key:?} should be refused as an invalid path, got {refused:?}"
            );
        }

        assert!(
            std::fs::read(dir.path().join("secrets.txt")).is_ok(),
            "the file an escape would have reached is still there to be read"
        );
    }

    /// Not an escape, but still refused: a name that works on this filesystem
    /// and would not work over HTTP. Refusing it here is what keeps a native
    /// asset tree shippable to the web.
    #[test]
    fn a_dir_source_refuses_a_key_that_could_not_be_served_over_http() {
        let (_dir, source) = fixture();
        let root = source.root().to_path_buf();
        // These files really exist, so the only thing refusing them is the key
        // rule.
        std::fs::write(root.join("my asset.png"), b"spaces").unwrap();
        std::fs::write(root.join("caf\u{e9}.png"), b"non-ascii").unwrap();
        std::fs::write(root.join("q.png"), b"query-ish").unwrap();

        for key in ["my asset.png", "caf\u{e9}.png", "q.png?v=1", "q.png#frag"] {
            assert!(
                matches!(
                    source.read(Path::new(key)),
                    Err(StorageError::InvalidPath(_))
                ),
                "{key:?} should be refused"
            );
        }
        // The legal spelling of the third one does read.
        assert_eq!(source.read(Path::new("q.png")).unwrap(), b"query-ish");
    }

    /// An entry as a test spells one.
    fn entry(key: &str, is_dir: bool) -> AssetEntry {
        AssetEntry {
            key: key.to_owned(),
            is_dir,
        }
    }

    /// **A memory source lists the keys directly under a directory, and the
    /// directories its deeper keys imply** — each once, in key order.
    #[test]
    fn a_memory_source_lists_assets_and_the_directories_keys_imply() {
        let mut source = resident();
        for key in ["meshes/barrel.glb", "meshes/props/lamp.glb", "readme.txt"] {
            source
                .insert(Path::new(key), Vec::new())
                .expect("a legal key");
        }
        assert_eq!(
            source.list(Path::new("")).expect("the root lists"),
            [entry("meshes", true), entry("readme.txt", false)],
        );
        assert_eq!(
            source.list(Path::new(".")).expect("the root"),
            source.list(Path::new("")).unwrap()
        );
        assert_eq!(
            source.list(Path::new("meshes/")).expect("a directory"),
            [
                entry("meshes/barrel.glb", false),
                entry("meshes/crate.glb", false),
                entry("meshes/props", true),
            ],
        );
        assert!(matches!(
            source.list(Path::new("textures")),
            Err(StorageError::NotFound(_))
        ));
        assert!(matches!(
            source.list(Path::new("../x")),
            Err(StorageError::InvalidPath(_))
        ));
        assert_eq!(
            MemorySource::new().list(Path::new("")).expect("empty root"),
            []
        );
    }

    /// **A directory source lists what is on disk**, directories told apart
    /// from files, and leaves out a name no key could spell.
    #[test]
    fn a_dir_source_lists_a_directory_and_skips_illegal_names() {
        let (_dir, source) = fixture();
        let root = source.root().to_path_buf();
        std::fs::write(root.join("meshes/my asset.png"), b"unreadable").unwrap();
        std::fs::create_dir_all(root.join("meshes/props")).unwrap();
        std::fs::write(root.join("index.txt"), b"").unwrap();

        assert_eq!(
            source.list(Path::new("")).expect("the root lists"),
            [entry("index.txt", false), entry("meshes", true)],
        );
        assert_eq!(
            source.list(Path::new("meshes")).expect("a directory"),
            [
                entry("meshes/crate.glb", false),
                entry("meshes/props", true)
            ],
        );
        for entry in source.list(Path::new("meshes")).unwrap() {
            if !entry.is_dir {
                source
                    .read(Path::new(&entry.key))
                    .expect("every listed asset reads");
            }
        }
        assert!(matches!(
            source.list(Path::new("absent")),
            Err(StorageError::NotFound(_))
        ));
        assert!(matches!(
            source.list(Path::new("..")),
            Err(StorageError::InvalidPath(_))
        ));
    }

    /// A source that cannot enumerate says so, rather than answering with an
    /// empty directory.
    #[test]
    fn a_source_that_cannot_list_says_so() {
        #[derive(Debug)]
        struct ReadOnly;
        impl AssetSource for ReadOnly {
            fn read(&self, key: &Path) -> Result<Vec<u8>, StorageError> {
                Err(StorageError::NotFound(key.to_path_buf()))
            }
        }
        assert!(matches!(
            ReadOnly.list(Path::new("")),
            Err(StorageError::Unsupported(_))
        ));
    }
}
