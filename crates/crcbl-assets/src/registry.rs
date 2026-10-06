//! Handles, load states, and the table that owns both.
//!
//! # The states, and the one that is not here
//!
//! Stage 6's asset model listed `Unloaded → Loading → Ready | Failed`.
//! Three of those four exist here, and a fourth the model did not name:
//! `Reloading`. `Unloaded` does not, because nothing can observe it: an asset
//! nobody has requested has no entry, and an entry whose last reference is
//! released is removed. "Not in the registry" is `None` from
//! [`AssetRegistry::get`], not a state a handle can be in — and a state that no
//! value ever holds is a match arm every caller writes and no test can reach.
//!
//! **A reload does not bring it back either.** The model expected a hot reload
//! to turn a `Ready` entry back into one with no bytes. It keeps them instead:
//! a [`Reloading`](AssetState::Reloading) entry still answers
//! [`Asset::bytes`] with what it had, and holds the new bytes beside them
//! ([`Asset::reloaded`]) until the consumer that decodes and uploads them says
//! it has ([`AssetRegistry::commit_reload`]) or could not
//! ([`AssetRegistry::refuse_reload`]). The frame goes on drawing the old asset
//! for as long as the new one is on its way, and a file caught half written or
//! refused by its decoder leaves the old one in place rather than a hole.
//!
//! The retiring entry the GPU deletion queue might have wanted is not here
//! either: what is retired is the consumer's device copy, and the renderer
//! retires that itself once no frame in flight names it — see
//! `crcbl_render::forward::ForwardRenderer::replace_page`.
//!
//! # The transitions
//!
//! ```text
//!                   source answers bytes
//!     request ────────────────────────────► Ready ◄───────────────┐
//!        │                                    ▲  │                 │ commit_reload: the new bytes
//!        │ source answers Pending             │  │ reload          │ refuse_reload: the old bytes
//!        ├────────────────► Loading ──────────┘  ▼                 │ a source error: the old bytes
//!        │                     │   poll: bytes  Reloading ─────────┘
//!        │ source answers      │
//!        │ an error            │ poll: an error
//!        └────────────────► Failed ◄
//! ```
//!
//! `Failed` is terminal, and `Ready` is terminal until somebody asks for a
//! reload. Re-requesting an asset in either state adds a reference and hands
//! back the same handle; it does not retry, because a retry policy nothing has
//! asked for is a policy nobody has checked. A caller that wants one releases
//! and requests again. A reload asked of a `Loading` or `Failed` entry is
//! refused: there is no old asset to keep drawing, so it is a first load, and
//! a first load is a request.
//!
//! # A reload keeps the handle
//!
//! The entry is the same entry throughout: the same [`AssetHandle`], the same
//! [`AssetId`], the same refcount. Whatever holds the handle — a material, a
//! scene chunk naming the asset by id — goes on naming the same asset, and sees
//! the new bytes once [`Asset::revision`] moves. Nothing that references an
//! asset has to be told it was reloaded in order to stay valid.
//!
//! # Refcounts
//!
//! [`request`](AssetRegistry::request) deduplicates by [`AssetId`], so two
//! callers naming one asset get one entry, one load and one handle.
//! [`release`](AssetRegistry::release) decrements, and the entry is dropped at
//! zero — reloading or not. That is the plan's "refcounted release"; the GPU
//! half of it is the consumer's, because the device copy is the consumer's and
//! this crate decodes and uploads nothing.

use core::fmt;
use std::collections::HashMap;
use std::path::Path;

use crcbl_core::{Handle, Pool};
use crcbl_store::StorageError;
use crcbl_store::web::canonical_key;

use crate::{AssetId, AssetSource};

/// A reference to an entry in an [`AssetRegistry`].
///
/// [`crcbl_core::Handle`] rather than a second handle type: an asset table is a
/// generational slot arena, which is what [`Pool`] is, and its stale-handle
/// behaviour is exactly the behaviour wanted here — a handle to an asset whose
/// last reference was released stops resolving instead of aliasing whatever
/// asset lands in the slot next.
///
/// There is no type parameter yet. The plan writes `AssetHandle<T>`, and `T`
/// will be `Mesh`, `Texture`, `AudioClip` — types that arrive with the
/// importers in step 3. A phantom parameter with exactly one instantiation
/// would be a type-safety claim nothing checks.
pub type AssetHandle = Handle<Asset>;

/// Where an asset is in its load.
///
/// See the [module docs](self) for the transitions and for why there is no
/// `Unloaded`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetState {
    /// The source has been asked and has not answered yet. Poll again.
    Loading,
    /// The bytes are here: [`Asset::bytes`] returns them.
    Ready,
    /// The bytes are here and newer ones are on their way: [`Asset::bytes`]
    /// still returns the old, and [`Asset::reloaded`] returns the new once the
    /// source has answered — see [`AssetRegistry::reload`].
    Reloading,
    /// The load failed: [`Asset::error`] says how. Terminal.
    Failed,
}

/// Why a reload left an asset's bytes as they were.
///
/// Kept on the entry ([`Asset::reload_failure`]) until the next reload
/// commits, so a tool can show it beside the asset as well as log it.
#[derive(Debug)]
pub enum ReloadFailure {
    /// The source could not produce the new bytes: the file went missing, or
    /// stopped being readable, between the change being noticed and the read.
    Source(StorageError),
    /// The consumer could not use them — a decoder refused the file, or a
    /// device refused the upload — and said why through
    /// [`AssetRegistry::refuse_reload`].
    Refused(String),
}

impl fmt::Display for ReloadFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => write!(f, "the source could not read it: {error}"),
            Self::Refused(why) => f.write_str(why),
        }
    }
}

/// One asset's entry: its identity, its key, and its bytes or the reason there
/// are none.
#[derive(Debug)]
pub struct Asset {
    id: AssetId,
    key: String,
    refs: u32,
    load: Load,
    /// Reloads committed since the first load — see [`Asset::revision`].
    revision: u32,
    /// Why the last reload left the bytes as they were, until one commits.
    reload_failure: Option<ReloadFailure>,
}

/// The state and its payload. Private because `Failed`'s
/// [`StorageError`] is not `Clone` and `Ready`'s bytes should be borrowed, not
/// matched out; [`Asset::state`], [`Asset::bytes`], [`Asset::reloaded`] and
/// [`Asset::error`] are the questions a caller actually asks.
#[derive(Debug)]
enum Load {
    Loading,
    Ready(Vec<u8>),
    /// The bytes in force, and the reload's read beside them once the source
    /// has answered.
    Reloading {
        current: Vec<u8>,
        next: Option<Vec<u8>>,
    },
    Failed(StorageError),
}

impl Asset {
    /// The asset's stable id.
    #[inline]
    #[must_use]
    pub const fn id(&self) -> AssetId {
        self.id
    }

    /// The canonical key it was requested by.
    #[inline]
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// How many outstanding references there are.
    #[inline]
    #[must_use]
    pub const fn refs(&self) -> u32 {
        self.refs
    }

    /// Where the load has got to.
    #[inline]
    #[must_use]
    pub const fn state(&self) -> AssetState {
        match self.load {
            Load::Loading => AssetState::Loading,
            Load::Ready(_) => AssetState::Ready,
            Load::Reloading { .. } => AssetState::Reloading,
            Load::Failed(_) => AssetState::Failed,
        }
    }

    /// The bytes in force, or `None` unless the state is
    /// [`Ready`](AssetState::Ready) or [`Reloading`](AssetState::Reloading).
    ///
    /// While reloading these are still the **old** bytes — the ones the
    /// consumer's device copy was made from — until
    /// [`AssetRegistry::commit_reload`] swaps the new ones in.
    #[inline]
    #[must_use]
    pub fn bytes(&self) -> Option<&[u8]> {
        match &self.load {
            Load::Ready(bytes) | Load::Reloading { current: bytes, .. } => Some(bytes),
            _ => None,
        }
    }

    /// The bytes a reload read, waiting for the consumer to commit or refuse
    /// them — or `None` when no reload is under way or its source has not
    /// answered yet.
    #[inline]
    #[must_use]
    pub fn reloaded(&self) -> Option<&[u8]> {
        match &self.load {
            Load::Reloading {
                next: Some(bytes), ..
            } => Some(bytes),
            _ => None,
        }
    }

    /// How many reloads have been committed since the first load.
    ///
    /// The handle does not change across a reload — see the
    /// [module docs](self) — so this is what a holder compares to learn that
    /// the bytes behind it did.
    #[inline]
    #[must_use]
    pub const fn revision(&self) -> u32 {
        self.revision
    }

    /// Why the last reload left the bytes as they were, or `None` if none has
    /// failed since one last committed.
    #[inline]
    #[must_use]
    pub const fn reload_failure(&self) -> Option<&ReloadFailure> {
        self.reload_failure.as_ref()
    }

    /// Why the load failed, or `None` unless the state is
    /// [`Failed`](AssetState::Failed).
    #[inline]
    #[must_use]
    pub fn error(&self) -> Option<&StorageError> {
        match &self.load {
            Load::Failed(error) => Some(error),
            _ => None,
        }
    }
}

/// The table of assets being loaded and loaded, over one [`AssetSource`].
///
/// Not thread-safe and not shared: one registry per source, owned by whatever
/// drives the frame. Loads advance when [`poll`](AssetRegistry::poll) is
/// called, never on a background thread — which is what lets the same code run
/// in a browser, where there is no thread to put them on.
#[derive(Debug)]
pub struct AssetRegistry<S: AssetSource> {
    source: S,
    assets: Pool<Asset>,
    by_id: HashMap<AssetId, AssetHandle>,
}

impl<S: AssetSource> AssetRegistry<S> {
    /// A registry loading through `source`.
    #[must_use]
    pub fn new(source: S) -> Self {
        Self {
            source,
            assets: Pool::new(),
            by_id: HashMap::new(),
        }
    }

    /// The source this registry loads through.
    #[inline]
    #[must_use]
    pub const fn source(&self) -> &S {
        &self.source
    }

    /// How many assets have entries.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.assets.len()
    }

    /// Whether nothing is loaded or loading.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// Request the asset at `path`, adding a reference to it.
    ///
    /// The first request for a key asks the source at once, so a source that
    /// can answer immediately — [`DirSource`](crate::DirSource) — produces a
    /// handle that is already [`Ready`](AssetState::Ready). A later request for
    /// the same key returns the same handle and does not ask again, whatever
    /// state it is in.
    ///
    /// # Errors
    ///
    /// [`StorageError::InvalidPath`] if `path` is not a legal asset key. That
    /// is the *only* error: a source that could not produce the bytes yields a
    /// handle in the [`Failed`](AssetState::Failed) state rather than an error
    /// here, because the caller needs a handle to hold onto either way and a
    /// failed asset is a thing that has to be observable per-asset rather than
    /// at the request that happened to be first.
    pub fn request(&mut self, path: &Path) -> Result<AssetHandle, StorageError> {
        let key = canonical_key(path)?;
        let id = AssetId::of_key(&key);
        if let Some(&handle) = self.by_id.get(&id) {
            // The pool is the owner; the map cannot hold a handle the pool has
            // dropped, because `release` removes both together.
            let asset = self
                .assets
                .get_mut(handle)
                .expect("by_id holds only live handles");
            asset.refs += 1;
            return Ok(handle);
        }

        let load = classify(self.source.read(Path::new(&key)));
        let handle = self.assets.insert(Asset {
            id,
            key,
            refs: 1,
            load,
            revision: 0,
            reload_failure: None,
        });
        self.by_id.insert(id, handle);
        Ok(handle)
    }

    /// Advance every [`Loading`](AssetState::Loading) asset and every reload
    /// still waiting on its source, and return how many are still waiting.
    ///
    /// Call once a frame. Asking the source again is the whole mechanism: a
    /// browser source answers [`Pending`](StorageError::Pending) until its shim
    /// delivers the bytes, then answers the bytes. Poll for the count to reach
    /// zero with a deadline; never sleep on it.
    pub fn poll(&mut self) -> usize {
        let mut pending = 0;
        // Two passes: the source is borrowed for the read and the pool for the
        // write, and they are both fields of `self`.
        let waiting: Vec<AssetHandle> = self
            .assets
            .iter()
            .filter(|(_, asset)| asset.waits_on_source())
            .map(|(handle, _)| handle)
            .collect();
        for handle in waiting {
            let key = self.assets.get(handle).map(|asset| asset.key.clone());
            let Some(key) = key else { continue };
            let answer = self.source.read(Path::new(&key));
            let Some(asset) = self.assets.get_mut(handle) else {
                continue;
            };
            let answered = if matches!(asset.load, Load::Loading) {
                asset.load = classify(answer);
                !matches!(asset.load, Load::Loading)
            } else {
                asset.take_reload_answer(answer)
            };
            if !answered {
                pending += 1;
            }
        }
        pending
    }

    /// Read the asset `handle` names again, keeping the bytes it has until the
    /// consumer commits the new ones, and return whether a reload is now under
    /// way.
    ///
    /// The hot-reload entry point: a watch saw the file change, and the
    /// consumer — the code that decodes the bytes and uploads them — asks for
    /// them here. On `true` the entry is [`Reloading`](AssetState::Reloading):
    /// [`Asset::bytes`] is still the old bytes, and [`Asset::reloaded`] is the
    /// new ones as soon as the source has answered — at once for
    /// [`DirSource`](crate::DirSource), at a later [`poll`](Self::poll) for a
    /// source that answers [`Pending`](StorageError::Pending). The consumer
    /// then calls [`commit_reload`](Self::commit_reload) once its device copy
    /// is made from them, or [`refuse_reload`](Self::refuse_reload) if it could
    /// not make one.
    ///
    /// **A reload of an entry already reloading starts again**: the file
    /// changed a second time, and the newer read supersedes whatever the first
    /// brought back that nobody committed.
    ///
    /// `false` for a released handle; for an entry still
    /// [`Loading`](AssetState::Loading) or [`Failed`](AssetState::Failed),
    /// which has no old asset to keep, so that is a first load and a first load
    /// is a [`request`](Self::request); and for a source that refuses the read
    /// outright, which leaves the entry [`Ready`](AssetState::Ready) on its old
    /// bytes with the error at [`Asset::reload_failure`].
    pub fn reload(&mut self, handle: AssetHandle) -> bool {
        let Some(asset) = self.assets.get_mut(handle) else {
            return false;
        };
        let current = match std::mem::replace(&mut asset.load, Load::Loading) {
            Load::Ready(current) | Load::Reloading { current, .. } => current,
            other => {
                asset.load = other;
                return false;
            }
        };
        asset.load = Load::Reloading {
            current,
            next: None,
        };
        let answer = self.source.read(Path::new(&asset.key));
        asset.take_reload_answer(answer);
        asset.state() == AssetState::Reloading
    }

    /// Swap a reload's bytes in, and return whether there were any to swap.
    ///
    /// Called by the consumer once its device copy is made from
    /// [`Asset::reloaded`]: the entry goes back to [`Ready`](AssetState::Ready)
    /// on the new bytes, [`Asset::revision`] moves, and any earlier
    /// [`Asset::reload_failure`] is cleared. The handle, the id and the
    /// refcount are untouched.
    ///
    /// `false`, changing nothing, unless the entry is reloading and its source
    /// has answered.
    pub fn commit_reload(&mut self, handle: AssetHandle) -> bool {
        let Some(asset) = self.assets.get_mut(handle) else {
            return false;
        };
        let Load::Reloading {
            next: Some(next), ..
        } = &mut asset.load
        else {
            return false;
        };
        asset.load = Load::Ready(std::mem::take(next));
        asset.revision += 1;
        asset.reload_failure = None;
        true
    }

    /// Give a reload up, keeping the bytes in force, and record `why`.
    ///
    /// Called by the consumer when the new bytes are no use to it — a decoder
    /// refused them, or a device refused the upload. The entry goes back to
    /// [`Ready`](AssetState::Ready) on its **old** bytes, which is what the
    /// consumer's device copy still holds, and `why` is
    /// [`Asset::reload_failure`] until a later reload commits.
    ///
    /// Returns whether a reload was given up; `false`, changing nothing, when
    /// none was under way.
    pub fn refuse_reload(&mut self, handle: AssetHandle, why: String) -> bool {
        let Some(asset) = self.assets.get_mut(handle) else {
            return false;
        };
        if asset.state() != AssetState::Reloading {
            return false;
        }
        asset.give_up_reload(ReloadFailure::Refused(why));
        true
    }

    /// The asset `handle` names, or `None` if it has been released.
    #[inline]
    #[must_use]
    pub fn get(&self, handle: AssetHandle) -> Option<&Asset> {
        self.assets.get(handle)
    }

    /// The handle for `id`, if the asset has an entry.
    ///
    /// The lookup the plan keeps path hashing for: a scene chunk naming an
    /// asset by id finds the entry without knowing the path it was loaded from.
    #[inline]
    #[must_use]
    pub fn find(&self, id: AssetId) -> Option<AssetHandle> {
        self.by_id.get(&id).copied()
    }

    /// Drop one reference to `handle`, and the asset with the last one.
    ///
    /// Returns whether the asset was dropped. A stale handle returns `false`
    /// and changes nothing, so a double release cannot free somebody else's
    /// asset — the generation in the handle is what makes that true.
    pub fn release(&mut self, handle: AssetHandle) -> bool {
        let Some(asset) = self.assets.get_mut(handle) else {
            return false;
        };
        asset.refs -= 1;
        if asset.refs > 0 {
            return false;
        }
        let id = asset.id;
        self.assets.remove(handle);
        self.by_id.remove(&id);
        true
    }
}

impl Asset {
    /// Whether [`AssetRegistry::poll`] has a read to make for this entry: a
    /// first load, or a reload whose source has not answered.
    const fn waits_on_source(&self) -> bool {
        matches!(
            self.load,
            Load::Loading | Load::Reloading { next: None, .. }
        )
    }

    /// Files a reload's read: the bytes become [`Asset::reloaded`], `Pending`
    /// leaves the reload waiting, and an error gives it up on the old bytes.
    /// Returns whether the source has answered — anything but `Pending`.
    fn take_reload_answer(&mut self, answer: Result<Vec<u8>, StorageError>) -> bool {
        match answer {
            Ok(bytes) => {
                if let Load::Reloading { next, .. } = &mut self.load {
                    *next = Some(bytes);
                }
                true
            }
            Err(StorageError::Pending(_)) => false,
            Err(error) => {
                self.give_up_reload(ReloadFailure::Source(error));
                true
            }
        }
    }

    /// Back to [`Ready`](AssetState::Ready) on the bytes in force, recording
    /// why.
    fn give_up_reload(&mut self, failure: ReloadFailure) {
        if let Load::Reloading { current, .. } = &mut self.load {
            self.load = Load::Ready(std::mem::take(current));
        }
        self.reload_failure = Some(failure);
    }
}

/// Turn a source's answer into a state: `Pending` is the one error that is not
/// a failure.
fn classify(answer: Result<Vec<u8>, StorageError>) -> Load {
    match answer {
        Ok(bytes) => Load::Ready(bytes),
        Err(StorageError::Pending(_)) => Load::Loading,
        Err(error) => Load::Failed(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DirSource;
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::path::PathBuf;

    /// A source whose answers a test writes: how many `Pending`s a key owes
    /// before its bytes appear, or an error to give instead.
    ///
    /// The registry's `Loading` state needs a source that answers
    /// [`StorageError::Pending`], and the one that exists —
    /// `crcbl_store::web::FetchSource` — needs a browser to drive it. This is
    /// the same contract with the browser replaced by a counter.
    #[derive(Debug, Default)]
    struct ScriptedSource {
        entries: RefCell<HashMap<String, Entry>>,
        reads: RefCell<Vec<String>>,
    }

    #[derive(Debug)]
    enum Entry {
        /// Answers `Pending` this many more times, then the bytes.
        After(u32, Vec<u8>),
        Fails,
    }

    impl ScriptedSource {
        fn ready(&self, key: &str, bytes: &[u8]) {
            self.after(key, 0, bytes);
        }

        fn after(&self, key: &str, polls: u32, bytes: &[u8]) {
            self.entries
                .borrow_mut()
                .insert(key.to_string(), Entry::After(polls, bytes.to_vec()));
        }

        fn fails(&self, key: &str) {
            self.entries
                .borrow_mut()
                .insert(key.to_string(), Entry::Fails);
        }

        fn reads(&self) -> Vec<String> {
            self.reads.borrow().clone()
        }
    }

    impl AssetSource for ScriptedSource {
        fn read(&self, key: &Path) -> Result<Vec<u8>, StorageError> {
            let key = canonical_key(key)?;
            self.reads.borrow_mut().push(key.clone());
            let mut entries = self.entries.borrow_mut();
            match entries.get_mut(&key) {
                None => Err(StorageError::NotFound(PathBuf::from(key))),
                Some(Entry::Fails) => Err(StorageError::Other(format!("{key} is broken"))),
                Some(Entry::After(0, bytes)) => Ok(bytes.clone()),
                Some(Entry::After(remaining, _)) => {
                    *remaining -= 1;
                    Err(StorageError::Pending(PathBuf::from(key)))
                }
            }
        }
    }

    fn dir_fixture() -> (tempfile::TempDir, AssetRegistry<DirSource>) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("meshes")).unwrap();
        std::fs::write(dir.path().join("meshes/crate.glb"), b"glTF fake").unwrap();
        let registry = AssetRegistry::new(DirSource::at(dir.path().to_path_buf()));
        (dir, registry)
    }

    #[test]
    fn a_source_that_answers_at_once_produces_a_ready_asset_with_its_bytes() {
        let (_dir, mut registry) = dir_fixture();
        let handle = registry.request(Path::new("meshes/crate.glb")).unwrap();

        let asset = registry.get(handle).expect("just requested");
        assert_eq!(asset.state(), AssetState::Ready);
        assert_eq!(asset.bytes(), Some(&b"glTF fake"[..]));
        assert!(asset.error().is_none());
        assert_eq!(asset.key(), "meshes/crate.glb");
        assert_eq!(asset.refs(), 1);
        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());
        // Already resolved, so polling has nothing to advance.
        assert_eq!(registry.poll(), 0);
        assert_eq!(
            registry.get(handle).map(Asset::state),
            Some(AssetState::Ready)
        );
    }

    #[test]
    fn a_source_that_answers_pending_leaves_the_asset_loading_until_it_does_not() {
        let source = ScriptedSource::default();
        source.after("meshes/crate.glb", 2, b"arrived");
        let mut registry = AssetRegistry::new(source);

        let handle = registry.request(Path::new("meshes/crate.glb")).unwrap();
        assert_eq!(
            registry.get(handle).map(Asset::state),
            Some(AssetState::Loading)
        );
        assert_eq!(registry.get(handle).and_then(Asset::bytes), None);

        assert_eq!(registry.poll(), 1, "second Pending");
        assert_eq!(
            registry.get(handle).map(Asset::state),
            Some(AssetState::Loading)
        );

        assert_eq!(registry.poll(), 0, "the bytes arrive on this one");
        let asset = registry.get(handle).expect("still held");
        assert_eq!(asset.state(), AssetState::Ready);
        assert_eq!(asset.bytes(), Some(&b"arrived"[..]));

        // Terminal: a further poll neither re-reads nor changes the state.
        let reads_so_far = registry.source().reads().len();
        assert_eq!(registry.poll(), 0);
        assert_eq!(registry.source().reads().len(), reads_so_far);
        assert_eq!(
            registry.get(handle).map(Asset::state),
            Some(AssetState::Ready)
        );
    }

    #[test]
    fn a_missing_asset_becomes_a_failed_handle_rather_than_a_request_error() {
        let (_dir, mut registry) = dir_fixture();
        let handle = registry
            .request(Path::new("meshes/absent.glb"))
            .expect("a legal key, so the request itself succeeds");

        let asset = registry
            .get(handle)
            .expect("failed assets keep their entry");
        assert_eq!(asset.state(), AssetState::Failed);
        assert!(matches!(asset.error(), Some(StorageError::NotFound(_))));
        assert_eq!(asset.bytes(), None);
    }

    #[test]
    fn an_asset_that_fails_while_loading_ends_up_failed_and_stays_there() {
        let source = ScriptedSource::default();
        source.after("late.glb", 1, b"never gets here");
        let mut registry = AssetRegistry::new(source);

        let handle = registry.request(Path::new("late.glb")).unwrap();
        assert_eq!(
            registry.get(handle).map(Asset::state),
            Some(AssetState::Loading)
        );

        // The source changes its mind before the next poll.
        registry.source().fails("late.glb");
        assert_eq!(registry.poll(), 0);
        let asset = registry.get(handle).expect("still held");
        assert_eq!(asset.state(), AssetState::Failed);
        assert!(matches!(asset.error(), Some(StorageError::Other(_))));

        // Terminal, even though the source would now answer bytes.
        registry.source().ready("late.glb", b"too late");
        assert_eq!(registry.poll(), 0);
        assert_eq!(
            registry.get(handle).map(Asset::state),
            Some(AssetState::Failed)
        );
    }

    #[test]
    fn a_key_that_is_not_a_legal_asset_key_is_refused_before_any_source_sees_it() {
        let source = ScriptedSource::default();
        let mut registry = AssetRegistry::new(source);
        for key in ["../secrets", "/etc/passwd", "", "a\\b", "a.png?v=1"] {
            assert!(
                matches!(
                    registry.request(Path::new(key)),
                    Err(StorageError::InvalidPath(_))
                ),
                "{key:?} should be refused"
            );
        }
        assert!(registry.is_empty());
        assert!(
            registry.source().reads().is_empty(),
            "the source must never be handed a key that was not validated"
        );
    }

    #[test]
    fn two_requests_for_one_asset_share_a_handle_a_load_and_a_refcount() {
        let source = ScriptedSource::default();
        source.ready("meshes/crate.glb", b"once");
        let mut registry = AssetRegistry::new(source);

        let first = registry.request(Path::new("meshes/crate.glb")).unwrap();
        let second = registry.request(Path::new("./meshes/crate.glb")).unwrap();
        assert_eq!(first, second, "one asset, one handle, either spelling");
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.get(first).map(Asset::refs), Some(2));
        assert_eq!(
            registry.source().reads(),
            vec!["meshes/crate.glb".to_string()],
            "the second request must not re-read"
        );
    }

    #[test]
    fn an_asset_is_dropped_by_its_last_release_and_not_before() {
        let source = ScriptedSource::default();
        source.ready("a.glb", b"a");
        let mut registry = AssetRegistry::new(source);

        let handle = registry.request(Path::new("a.glb")).unwrap();
        registry.request(Path::new("a.glb")).unwrap();

        assert!(!registry.release(handle), "one reference left");
        assert_eq!(registry.get(handle).map(Asset::refs), Some(1));
        assert_eq!(registry.len(), 1);

        assert!(registry.release(handle), "that was the last one");
        assert!(registry.get(handle).is_none());
        assert!(registry.is_empty());
    }

    #[test]
    fn a_released_handle_never_resolves_again_even_when_its_slot_is_reused() {
        let source = ScriptedSource::default();
        source.ready("a.glb", b"a");
        source.ready("b.glb", b"b");
        let mut registry = AssetRegistry::new(source);

        let stale = registry.request(Path::new("a.glb")).unwrap();
        assert!(registry.release(stale));
        assert!(
            !registry.release(stale),
            "a double release must not free somebody else's asset"
        );

        let fresh = registry.request(Path::new("b.glb")).unwrap();
        assert_eq!(fresh.index(), stale.index(), "the pool reused the slot");
        assert_ne!(fresh, stale);
        assert!(registry.get(stale).is_none());
        assert_eq!(registry.get(fresh).and_then(Asset::bytes), Some(&b"b"[..]));
    }

    #[test]
    fn a_released_asset_is_forgotten_by_id_too_and_reloads_on_the_next_request() {
        let source = ScriptedSource::default();
        source.ready("a.glb", b"a");
        let mut registry = AssetRegistry::new(source);

        let id = AssetId::from_path(Path::new("a.glb")).unwrap();
        let first = registry.request(Path::new("a.glb")).unwrap();
        assert_eq!(registry.find(id), Some(first));
        assert_eq!(registry.get(first).map(Asset::id), Some(id));

        assert!(registry.release(first));
        assert_eq!(registry.find(id), None, "the id lookup goes with the entry");

        let second = registry.request(Path::new("a.glb")).unwrap();
        assert_eq!(registry.find(id), Some(second));
        assert_eq!(
            registry.source().reads().len(),
            2,
            "a re-request after release really re-reads"
        );
    }

    /// **A reload keeps the handle and swaps the bytes** — but only at the
    /// commit. Until then the old bytes are what [`Asset::bytes`] answers,
    /// because they are what the consumer's device copy was made from.
    #[test]
    fn a_reload_keeps_the_handle_and_swaps_the_bytes_at_the_commit() {
        let source = ScriptedSource::default();
        source.ready("tex/brick.png", b"old texels");
        let mut registry = AssetRegistry::new(source);
        let id = AssetId::from_path(Path::new("tex/brick.png")).unwrap();
        let handle = registry.request(Path::new("tex/brick.png")).unwrap();

        registry.source().ready("tex/brick.png", b"new texels");
        assert!(registry.reload(handle), "a ready asset reloads");
        let asset = registry.get(handle).expect("the handle still resolves");
        assert_eq!(asset.state(), AssetState::Reloading);
        assert_eq!(
            asset.bytes(),
            Some(&b"old texels"[..]),
            "nothing swapped yet"
        );
        assert_eq!(asset.reloaded(), Some(&b"new texels"[..]));
        assert_eq!(asset.revision(), 0);

        assert!(registry.commit_reload(handle));
        let asset = registry.get(handle).expect("the handle still resolves");
        assert_eq!(asset.state(), AssetState::Ready);
        assert_eq!(asset.bytes(), Some(&b"new texels"[..]));
        assert_eq!(asset.reloaded(), None);
        assert_eq!(asset.revision(), 1, "the commit is what a holder can see");
        assert_eq!(registry.find(id), Some(handle), "one id, one handle, still");
        assert_eq!(registry.len(), 1, "a reload is not a second entry");
        assert!(
            !registry.commit_reload(handle),
            "a second commit has nothing to swap"
        );
        assert_eq!(registry.get(handle).map(Asset::revision), Some(1));
    }

    /// **A consumer that cannot use the new bytes keeps the old ones**, and the
    /// reason stays on the entry until a later reload commits.
    #[test]
    fn a_refused_reload_keeps_the_old_bytes_and_says_why() {
        let source = ScriptedSource::default();
        source.ready("tex/brick.png", b"old texels");
        let mut registry = AssetRegistry::new(source);
        let handle = registry.request(Path::new("tex/brick.png")).unwrap();

        registry.source().ready("tex/brick.png", b"half a png");
        assert!(registry.reload(handle));
        assert!(registry.refuse_reload(handle, "the PNG decoder refused it".to_owned()));
        let asset = registry.get(handle).expect("the handle still resolves");
        assert_eq!(asset.state(), AssetState::Ready);
        assert_eq!(asset.bytes(), Some(&b"old texels"[..]));
        assert_eq!(asset.revision(), 0, "a refusal is not a revision");
        assert!(
            matches!(
                asset.reload_failure(),
                Some(ReloadFailure::Refused(why)) if why == "the PNG decoder refused it"
            ),
            "the refusal is reported: {:?}",
            asset.reload_failure()
        );
        assert!(
            !registry.refuse_reload(handle, "again".to_owned()),
            "no reload is under way to refuse"
        );
        assert!(!registry.commit_reload(handle), "nor one to commit");

        // The next good save clears the report.
        registry.source().ready("tex/brick.png", b"whole png");
        assert!(registry.reload(handle));
        assert!(registry.commit_reload(handle));
        let asset = registry.get(handle).expect("the handle still resolves");
        assert_eq!(asset.bytes(), Some(&b"whole png"[..]));
        assert!(asset.reload_failure().is_none());
    }

    /// **A file that cannot be read when the reload asks keeps the old bytes**:
    /// a source error is a refusal the source makes, not a failed asset.
    #[test]
    fn a_reload_the_source_refuses_keeps_the_old_bytes() {
        let source = ScriptedSource::default();
        source.ready("tex/brick.png", b"old texels");
        let mut registry = AssetRegistry::new(source);
        let handle = registry.request(Path::new("tex/brick.png")).unwrap();

        registry.source().fails("tex/brick.png");
        assert!(!registry.reload(handle), "no reload is under way");
        let asset = registry.get(handle).expect("the handle still resolves");
        assert_eq!(asset.state(), AssetState::Ready, "not Failed");
        assert_eq!(asset.bytes(), Some(&b"old texels"[..]));
        assert!(matches!(
            asset.reload_failure(),
            Some(ReloadFailure::Source(StorageError::Other(_)))
        ));
    }

    /// **A source that answers `Pending` leaves the reload waiting on the old
    /// bytes**, and `poll` brings the new ones in — the browser's shape.
    #[test]
    fn a_pending_reload_waits_on_the_old_bytes_until_a_poll_brings_the_new() {
        let source = ScriptedSource::default();
        source.ready("tex/brick.png", b"old texels");
        let mut registry = AssetRegistry::new(source);
        let handle = registry.request(Path::new("tex/brick.png")).unwrap();

        registry.source().after("tex/brick.png", 2, b"new texels");
        assert!(registry.reload(handle));
        let asset = registry.get(handle).expect("still held");
        assert_eq!(asset.state(), AssetState::Reloading);
        assert_eq!(asset.reloaded(), None, "the source has not answered");
        assert_eq!(asset.bytes(), Some(&b"old texels"[..]));
        assert!(
            !registry.commit_reload(handle),
            "nothing to commit before the bytes arrive"
        );

        assert_eq!(registry.poll(), 1, "the second Pending");
        assert_eq!(registry.poll(), 0, "the bytes arrive");
        assert_eq!(
            registry.get(handle).and_then(Asset::reloaded),
            Some(&b"new texels"[..])
        );
        let reads = registry.source().reads().len();
        assert_eq!(registry.poll(), 0);
        assert_eq!(
            registry.source().reads().len(),
            reads,
            "a reload that has its bytes is not read again"
        );
        assert!(registry.commit_reload(handle));
        assert_eq!(
            registry.get(handle).and_then(Asset::bytes),
            Some(&b"new texels"[..])
        );
    }

    /// **A reference held across a reload stays valid**, and the refcount is
    /// the entry's, not the reload's: a release while reloading drops a
    /// reference and leaves the reload to finish.
    #[test]
    fn a_reference_held_across_a_reload_stays_valid() {
        let source = ScriptedSource::default();
        source.ready("tex/brick.png", b"old texels");
        let mut registry = AssetRegistry::new(source);
        let first = registry.request(Path::new("tex/brick.png")).unwrap();
        let second = registry.request(Path::new("tex/brick.png")).unwrap();
        assert_eq!(first, second);

        registry.source().ready("tex/brick.png", b"new texels");
        assert!(registry.reload(first));
        assert_eq!(registry.get(first).map(Asset::refs), Some(2));
        assert!(!registry.release(first), "one reference left");
        assert!(registry.commit_reload(second));
        let asset = registry.get(second).expect("the held reference resolves");
        assert_eq!(asset.refs(), 1);
        assert_eq!(asset.bytes(), Some(&b"new texels"[..]));

        assert!(registry.release(second), "that was the last one");
        assert!(registry.get(first).is_none());
        assert!(registry.is_empty());
    }

    /// **A second change while reloading supersedes the first**, and a reload
    /// is refused where there is no old asset to keep.
    #[test]
    fn a_second_reload_supersedes_and_a_first_load_cannot_reload() {
        let source = ScriptedSource::default();
        source.ready("tex/brick.png", b"one");
        source.after("tex/late.png", 3, b"late");
        source.fails("tex/broken.png");
        let mut registry = AssetRegistry::new(source);
        let handle = registry.request(Path::new("tex/brick.png")).unwrap();
        let loading = registry.request(Path::new("tex/late.png")).unwrap();
        let failed = registry.request(Path::new("tex/broken.png")).unwrap();

        registry.source().ready("tex/brick.png", b"two");
        assert!(registry.reload(handle));
        registry.source().ready("tex/brick.png", b"three");
        assert!(registry.reload(handle), "the file changed again");
        let asset = registry.get(handle).expect("still held");
        assert_eq!(asset.bytes(), Some(&b"one"[..]), "still the bytes in force");
        assert_eq!(asset.reloaded(), Some(&b"three"[..]), "the newer read");

        assert!(!registry.reload(loading), "a first load is a request");
        assert_eq!(
            registry.get(loading).map(Asset::state),
            Some(AssetState::Loading)
        );
        assert!(
            !registry.reload(failed),
            "a failed asset has nothing to keep"
        );
        assert_eq!(
            registry.get(failed).map(Asset::state),
            Some(AssetState::Failed)
        );
        assert!(registry.release(failed));
        assert!(
            !registry.reload(failed),
            "a released handle reloads nothing"
        );
    }

    #[test]
    fn many_assets_load_independently_and_poll_reports_only_the_ones_still_waiting() {
        let source = ScriptedSource::default();
        source.ready("now.glb", b"now");
        source.after("soon.glb", 2, b"soon");
        source.after("later.glb", 4, b"later");
        source.fails("broken.glb");
        let mut registry = AssetRegistry::new(source);

        let mut handles = HashMap::new();
        for key in ["now.glb", "soon.glb", "later.glb", "broken.glb"] {
            handles.insert(key, registry.request(Path::new(key)).unwrap());
        }
        assert_eq!(registry.len(), 4);
        let distinct: HashSet<_> = handles.values().copied().collect();
        assert_eq!(distinct.len(), 4, "four assets, four handles");

        let state = |registry: &AssetRegistry<ScriptedSource>, key: &str| {
            registry.get(handles[key]).map(Asset::state)
        };
        assert_eq!(state(&registry, "now.glb"), Some(AssetState::Ready));
        assert_eq!(state(&registry, "broken.glb"), Some(AssetState::Failed));

        assert_eq!(registry.poll(), 2, "soon and later both answered Pending");
        assert_eq!(
            registry.poll(),
            1,
            "soon arrived; later has one Pending left"
        );
        assert_eq!(registry.poll(), 1, "later spent its last Pending");
        assert_eq!(registry.poll(), 0, "later arrived");

        assert_eq!(state(&registry, "soon.glb"), Some(AssetState::Ready));
        assert_eq!(
            registry.get(handles["later.glb"]).and_then(Asset::bytes),
            Some(&b"later"[..])
        );
        assert_eq!(state(&registry, "broken.glb"), Some(AssetState::Failed));
    }
}
