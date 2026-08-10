//! Handles, load states, and the table that owns both.
//!
//! # The states, and the ones that are not here
//!
//! `docs/plan/06-assets-scenes.md` lists `Unloaded → Loading → Ready | Failed`.
//! Three of those four exist here. `Unloaded` does not, because nothing can
//! observe it: an asset nobody has requested has no entry, and an entry whose
//! last reference is released is removed. "Not in the registry" is `None` from
//! [`AssetRegistry::get`], not a state a handle can be in — and a state that no
//! value ever holds is a match arm every caller writes and no test can reach.
//!
//! It comes back when something can produce it. Hot reload (step 5) turns a
//! `Ready` entry back into one with no bytes; the GPU deletion queue the plan's
//! asset model names for refcounted release will want an entry that is retiring
//! rather than gone. Neither exists, and neither is guessed at here.
//!
//! # The transitions
//!
//! ```text
//!                  source answers bytes
//!     request ──────────────────────────► Ready
//!        │                                  ▲
//!        │ source answers Pending           │ poll: source answers bytes
//!        ├──────────────────► Loading ──────┤
//!        │                       │          │
//!        │ source answers an     │ poll: source answers an error
//!        │ error                 ▼          │
//!        └──────────────────► Failed ◄──────┘
//! ```
//!
//! `Ready` and `Failed` are terminal. Re-requesting an asset in either state
//! adds a reference and hands back the same handle; it does not retry, because
//! a retry policy nothing has asked for is a policy nobody has checked. A
//! caller that wants one releases and requests again.
//!
//! # Refcounts
//!
//! [`request`](AssetRegistry::request) deduplicates by [`AssetId`], so two
//! callers naming one asset get one entry, one load and one handle.
//! [`release`](AssetRegistry::release) decrements, and the entry is dropped at
//! zero. That is the plan's "refcounted release" minus its other half: the GPU
//! retire calls, which need the stage 2 deletion queue and a GPU-resident asset
//! to retire, and this crate has neither.

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
    /// The load failed: [`Asset::error`] says how. Terminal.
    Failed,
}

/// One asset's entry: its identity, its key, and its bytes or the reason there
/// are none.
#[derive(Debug)]
pub struct Asset {
    id: AssetId,
    key: String,
    refs: u32,
    load: Load,
}

/// The state and its payload. Private because `Failed`'s
/// [`StorageError`] is not `Clone` and `Ready`'s bytes should be borrowed, not
/// matched out; [`Asset::state`], [`Asset::bytes`] and [`Asset::error`] are the
/// three questions a caller actually asks.
#[derive(Debug)]
enum Load {
    Loading,
    Ready(Vec<u8>),
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
            Load::Failed(_) => AssetState::Failed,
        }
    }

    /// The loaded bytes, or `None` unless the state is
    /// [`Ready`](AssetState::Ready).
    #[inline]
    #[must_use]
    pub fn bytes(&self) -> Option<&[u8]> {
        match &self.load {
            Load::Ready(bytes) => Some(bytes),
            _ => None,
        }
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
        });
        self.by_id.insert(id, handle);
        Ok(handle)
    }

    /// Advance every [`Loading`](AssetState::Loading) asset, and return how
    /// many are still loading.
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
            .filter(|(_, asset)| asset.state() == AssetState::Loading)
            .map(|(handle, _)| handle)
            .collect();
        for handle in waiting {
            let key = self.assets.get(handle).map(|asset| asset.key.clone());
            let Some(key) = key else { continue };
            let load = classify(self.source.read(Path::new(&key)));
            if matches!(load, Load::Loading) {
                pending += 1;
            }
            if let Some(asset) = self.assets.get_mut(handle) {
                asset.load = load;
            }
        }
        pending
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
