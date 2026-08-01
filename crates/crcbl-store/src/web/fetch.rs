//! [`FetchSource`] — assets over `fetch()`, read through a resident cache.
//!
//! The design and the reasoning behind pre-load-plus-request/poll are in the
//! [parent module's docs](super). This module is the mechanism and the exact
//! JS↔wasm contract.
//!
//! # Exports
//!
//! Symbols are `#[unsafe(no_mangle)]` **only** on `wasm32`; a native build
//! exports none of them.
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_web_fetch_key_ptr`](shim::__crcbl_web_fetch_key_ptr) | `() -> i32` | Address of the key scratch buffer, or `0` when no source is installed. Stable for the source's life. |
//! | [`__crcbl_web_fetch_key_capacity`](shim::__crcbl_web_fetch_key_capacity) | `() -> i32` | How many bytes may be written there ([`MAX_KEY_LEN`](super::MAX_KEY_LEN)). |
//! | [`__crcbl_web_fetch_begin`](shim::__crcbl_web_fetch_begin) | `(i32) -> i32` | Open a delivery for the `len`-byte key in the scratch buffer. Returns a slot id, or `0` if the key was refused. The **pre-load** entry point. |
//! | [`__crcbl_web_fetch_take`](shim::__crcbl_web_fetch_take) | `() -> i32` | Dequeue one engine-requested fetch. Returns a slot id, or `0` when the queue is empty. The **request/poll** entry point. |
//! | [`__crcbl_web_fetch_url_ptr`](shim::__crcbl_web_fetch_url_ptr) | `(i32) -> i32` | Address of slot `id`'s URL (UTF-8, not NUL-terminated), or `0`. Only slots from `take` have one. |
//! | [`__crcbl_web_fetch_url_len`](shim::__crcbl_web_fetch_url_len) | `(i32) -> i32` | Length of that URL in bytes, or `0`. |
//! | [`__crcbl_web_fetch_buffer`](shim::__crcbl_web_fetch_buffer) | `(i32, i32) -> i32` | Size slot `id`'s body buffer to `len` bytes and return its address, or `0`. **May grow wasm memory.** |
//! | [`__crcbl_web_fetch_commit`](shim::__crcbl_web_fetch_commit) | `(i32, i32) -> i32` | The first `len` bytes of that buffer are the body. Makes the key resident and closes the slot. `1`/`0`. |
//! | [`__crcbl_web_fetch_fail`](shim::__crcbl_web_fetch_fail) | `(i32, i32) -> i32` | The fetch failed with HTTP `status` (`0` for a network error). Closes the slot. `1`/`0`. |
//! | [`__crcbl_web_fetch_pending`](shim::__crcbl_web_fetch_pending) | `() -> i32` | Queued requests not yet taken. A shim may poll this once per frame. |
//! | [`__crcbl_web_fetch_inflight`](shim::__crcbl_web_fetch_inflight) | `() -> i32` | Open slots. Zero after a pre-load means every response landed. |
//!
//! ## Buffer ownership
//!
//! Every buffer belongs to wasm; JS never passes a pointer in.
//!
//! - The **key scratch** is written by JS and read by wasm. Its address is
//!   stable from [`install`] until the source is dropped, and its capacity is
//!   [`MAX_KEY_LEN`](super::MAX_KEY_LEN) — a longer key must not be written, and
//!   is refused by length if it is.
//! - A **URL** is written by wasm and read by JS. It is valid from the `take`
//!   that produced the slot until that slot's `commit` or `fail`, and must be
//!   decoded (`TextDecoder`) before either.
//! - A **body buffer** is allocated by wasm and written by JS, between
//!   `__crcbl_web_fetch_buffer` and `__crcbl_web_fetch_commit` for the same
//!   slot. Nothing else may touch it in that window, and no call for another
//!   slot may be interleaved into it, because `buffer` for the other slot can
//!   grow wasm memory and detach the view.
//!
//! **The detached-view trap.** A `Uint8Array` over `memory.buffer` is detached
//! when wasm memory grows, and `__crcbl_web_fetch_buffer` allocates, so it can.
//! Build the view *after* the call that returned the pointer:
//!
//! ```js
//! const ptr = ex.__crcbl_web_fetch_buffer(id, bytes.length);   // may grow memory
//! if (ptr === 0) { ex.__crcbl_web_fetch_fail(id, 0); return; }
//! new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes); // view built after
//! ex.__crcbl_web_fetch_commit(id, bytes.length);
//! ```
//!
//! ## Call ordering
//!
//! 1. The app constructs a [`FetchSource`] and calls [`install`] — before the
//!    shim's first call. Until then **every export answers `0`**, which is a
//!    shim that started before the engine did rather than a failure to report.
//! 2. Pre-load, for each manifest entry: `key_ptr` → write the key →
//!    `begin(len)` → `fetch()` → `buffer` → copy → `commit`. `begin` may be
//!    called for every entry up front; slots are independent and responses may
//!    land in any order.
//! 3. Boot the app and start the `requestAnimationFrame` loop.
//! 4. Per frame, or on a timer: `take()` until it answers `0`; for each id,
//!    `url_ptr`/`url_len` → `fetch(url)` → `buffer` → copy → `commit`.
//! 5. On any failure — network error, non-`ok` response, a `0` from `buffer` —
//!    call `fail(id, status)`. **A slot that is neither committed nor failed
//!    leaks**: the key stays `InFlight` forever and the engine polls it forever.
//!    `fail` is the obligation, not the optional path.
//!
//! ## Failure behaviour
//!
//! | Situation | What the shim does | What the engine sees |
//! | --- | --- | --- |
//! | key refused by `begin` (returns `0`) | log and skip; do not retry with the same key | the key is `Unknown`; a `read` of it enqueues a request |
//! | HTTP 404 | `fail(id, 404)` | [`StorageError::NotFound`] |
//! | HTTP 401/403 | `fail(id, status)` | [`StorageError::PermissionDenied`] |
//! | other HTTP status | `fail(id, status)` | [`StorageError::Other`] naming the status |
//! | network error / CORS / abort | `fail(id, 0)` | [`StorageError::Other`] |
//! | body larger than [`MAX_ASSET_BYTES`] | `buffer` answers `0`; call `fail(id, 0)` | as above |
//!
//! A failure is remembered, so a `read` after it reports the failure rather than
//! re-queueing forever. [`FetchSource::request`] on a failed key clears the
//! failure and retries — that is the only retry, and it is the engine's
//! decision, not the shim's.
//!
//! ## Worked shim
//!
//! ```js
//! const KEYS = ['sprites/ball.png', 'sound/bounce.wav'];   // the manifest
//! const enc = new TextEncoder();
//!
//! async function deliver(id, url) {
//!   try {
//!     const res = await fetch(url);
//!     if (!res.ok) { ex.__crcbl_web_fetch_fail(id, res.status); return; }
//!     const bytes = new Uint8Array(await res.arrayBuffer());
//!     const ptr = ex.__crcbl_web_fetch_buffer(id, bytes.length);
//!     if (ptr === 0) { ex.__crcbl_web_fetch_fail(id, 0); return; }
//!     new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
//!     ex.__crcbl_web_fetch_commit(id, bytes.length);
//!   } catch (e) {
//!     ex.__crcbl_web_fetch_fail(id, 0);                    // never leak a slot
//!   }
//! }
//!
//! // Pre-load: the shim knows the key, so it builds the URL from its own base.
//! await Promise.all(KEYS.map((key) => {
//!   const bytes = enc.encode(key);
//!   const ptr = ex.__crcbl_web_fetch_key_ptr();
//!   if (ptr === 0 || bytes.length > ex.__crcbl_web_fetch_key_capacity()) return;
//!   new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
//!   const id = ex.__crcbl_web_fetch_begin(bytes.length);
//!   return id === 0 ? undefined : deliver(id, 'assets/' + key);
//! }));
//!
//! // Runtime: the engine asked, so wasm supplies the whole URL. Do not build
//! // one from the key here — see the parent module on containment.
//! function pump() {
//!   for (let id; (id = ex.__crcbl_web_fetch_take()) !== 0; ) {
//!     const p = ex.__crcbl_web_fetch_url_ptr(id);
//!     const n = ex.__crcbl_web_fetch_url_len(id);
//!     const url = new TextDecoder().decode(new Uint8Array(memory.buffer, p, n));
//!     deliver(id, url);
//!   }
//! }
//! ```

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use super::{Inbox, KeyScratch, canonical_dir, canonical_key, direct_children, is_valid_base};
use crate::{StorageError, StorageSource};

/// The largest single asset body accepted, in bytes.
///
/// A bound rather than a limit anyone should reach: it stops a nonsense
/// `Content-Length` — or a hostile response to a request the page did not
/// intend — from asking wasm to allocate arbitrarily much. 64 MiB is far above
/// any asset the samples have.
pub const MAX_ASSET_BYTES: usize = 64 << 20;

/// The most fetches that may be queued but not yet taken.
///
/// Reached only when nothing is draining the queue — no shim, or a shim that
/// stopped. Further [`FetchSource::request`] calls then fail loudly instead of
/// growing a queue for a consumer that is not there.
pub const MAX_QUEUED_REQUESTS: usize = 256;

/// Where a key stands with the browser.
///
/// The whole of the async story as the engine sees it. `Queued` and `InFlight`
/// are both "ask again next frame"; the pair is distinguished because a key
/// stuck in `Queued` means the shim is not pumping, and a key stuck in
/// `InFlight` means it took the slot and never settled it — different bugs, in
/// different files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchState {
    /// Never requested, or evicted.
    Unknown,
    /// Requested; the shim has not taken it yet.
    Queued,
    /// The shim took it and has not committed or failed it.
    InFlight,
    /// Resident: [`read`](StorageSource::read) answers from memory.
    Ready,
    /// The shim reported a failure with this HTTP status (`0` for a network
    /// error). Remembered until [`FetchSource::request`] retries the key.
    Failed {
        /// The HTTP status, or `0` when the request never got one.
        status: u16,
    },
}

/// A snapshot of the cache, for a debug overlay.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FetchStats {
    /// Keys resident in memory.
    pub resident: usize,
    /// Bytes those keys occupy.
    pub bytes: usize,
    /// Requests queued and not yet taken by the shim.
    pub queued: usize,
    /// Slots the shim has taken or begun and not yet settled.
    pub in_flight: usize,
    /// Keys whose last attempt failed.
    pub failed: usize,
}

/// Everything behind the [`RefCell`].
#[derive(Debug, Default)]
struct Inner {
    /// Resident bodies, keyed by canonical key. `BTreeMap` so
    /// [`list`](StorageSource::list) is ordered without a sort of the whole map.
    resident: BTreeMap<String, Vec<u8>>,
    /// The last failure per key, until the key is requested again.
    failed: HashMap<String, u16>,
    /// Requested, not yet taken by the shim.
    queued: VecDeque<String>,
    /// Deliveries in progress, from `take` or `begin`.
    inbox: Inbox,
    /// URLs for slots that came from `take`, valid until the slot settles.
    urls: HashMap<u32, String>,
    /// Where the shim writes keys.
    scratch: KeyScratch,
}

/// A read-only [`StorageSource`] over HTTP, backed by a resident cache the JS
/// shim fills.
///
/// See the [module docs](self) for the ABI and the [parent module](super) for
/// why the seam has this shape. In one line: `read` never waits; it answers from
/// the cache, or enqueues a fetch and reports [`StorageError::Pending`].
///
/// ```
/// use crcbl_store::{StorageSource, StorageError};
/// use crcbl_store::web::{FetchSource, FetchState};
/// use std::path::Path;
///
/// let source = FetchSource::new("assets/").unwrap();
/// let key = Path::new("sprites/ball.png");
///
/// // Nothing is resident, so the read enqueues the fetch and says so.
/// assert!(matches!(source.read(key), Err(StorageError::Pending(_))));
/// assert_eq!(source.state(key), FetchState::Queued);
///
/// // What the shim would fetch. The engine's key never becomes a URL anywhere
/// // else, and a key that could escape the asset root is refused outright.
/// assert_eq!(source.url_for(key).unwrap(), "assets/sprites/ball.png");
/// assert!(source.url_for(Path::new("../../etc/passwd")).is_err());
///
/// // Delivery (the shim does this through the `extern "C"` entry points).
/// source.insert(key, b"PNG...".to_vec()).unwrap();
/// assert_eq!(source.state(key), FetchState::Ready);
/// assert_eq!(source.read(key).unwrap(), b"PNG...");
/// ```
#[derive(Debug)]
pub struct FetchSource {
    /// The asset root, always ending in `/`.
    base: String,
    inner: RefCell<Inner>,
}

impl FetchSource {
    /// Create a source rooted at `base`, a URL prefix ending in `/`.
    ///
    /// Relative (`"assets/"`), absolute-path (`"/assets/"`) and absolute-URL
    /// (`"https://example.test/assets/"`) bases all work; the browser resolves
    /// the result against the document as usual.
    ///
    /// # Errors
    ///
    /// [`StorageError::InvalidPath`] when `base` is empty, does not end in `/`,
    /// is not ASCII, or contains `..`, `?`, `#`, `\`, a quote, a space or a
    /// control character — anything that would undo the key rules the module
    /// docs rely on.
    pub fn new(base: &str) -> Result<Self, StorageError> {
        if !is_valid_base(base) {
            return Err(StorageError::InvalidPath(PathBuf::from(base)));
        }
        Ok(Self {
            base: base.to_owned(),
            inner: RefCell::new(Inner::default()),
        })
    }

    /// The asset root this source was built with.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// The URL the shim would be given for `path`.
    ///
    /// The only place a key becomes a URL. Exposed because "does this key stay
    /// inside the asset root" is a question worth being able to ask — and to
    /// test — without a browser.
    ///
    /// # Errors
    ///
    /// [`StorageError::InvalidPath`] for any key [`canonical_key`] refuses.
    pub fn url_for(&self, path: &Path) -> Result<String, StorageError> {
        Ok(format!("{}{}", self.base, canonical_key(path)?))
    }

    /// Ask the shim to fetch `path`.
    ///
    /// Idempotent and cheap to call every frame: a key that is resident, queued
    /// or in flight is left alone. A key whose last attempt **failed** is
    /// retried, and the remembered failure cleared — this is the only retry in
    /// the system, and it is deliberately the engine's decision.
    ///
    /// # Errors
    ///
    /// [`StorageError::InvalidPath`] for a key that escapes the asset root, and
    /// [`StorageError::LimitExceeded`] when more than [`MAX_QUEUED_REQUESTS`]
    /// requests are already waiting — which means nothing is draining them.
    pub fn request(&self, path: &Path) -> Result<(), StorageError> {
        let key = canonical_key(path)?;
        let mut inner = self.inner.borrow_mut();
        if inner.resident.contains_key(&key)
            || inner.queued.contains(&key)
            || inner.inbox.find(&key).is_some()
        {
            return Ok(());
        }
        if inner.queued.len() >= MAX_QUEUED_REQUESTS {
            return Err(StorageError::LimitExceeded(path.to_path_buf()));
        }
        inner.failed.remove(&key);
        inner.queued.push_back(key);
        Ok(())
    }

    /// Where `path` stands. [`FetchState::Unknown`] for a key that could never
    /// be valid, which is also what it is: nothing will ever fetch it.
    pub fn state(&self, path: &Path) -> FetchState {
        let Ok(key) = canonical_key(path) else {
            return FetchState::Unknown;
        };
        let inner = self.inner.borrow();
        if inner.resident.contains_key(&key) {
            FetchState::Ready
        } else if let Some(&status) = inner.failed.get(&key) {
            FetchState::Failed { status }
        } else if inner.queued.contains(&key) {
            FetchState::Queued
        } else if inner.inbox.find(&key).is_some() {
            FetchState::InFlight
        } else {
            FetchState::Unknown
        }
    }

    /// Make `bytes` resident for `path` without a fetch.
    ///
    /// For an asset the page already has — one embedded in the wasm, one
    /// delivered by a mechanism this crate does not model — and for tests. Any
    /// queued request or remembered failure for the key is cleared.
    ///
    /// # Errors
    ///
    /// [`StorageError::InvalidPath`] for a key that escapes the asset root, and
    /// [`StorageError::LimitExceeded`] for a body over [`MAX_ASSET_BYTES`].
    pub fn insert(&self, path: &Path, bytes: Vec<u8>) -> Result<(), StorageError> {
        let key = canonical_key(path)?;
        if bytes.len() > MAX_ASSET_BYTES {
            return Err(StorageError::LimitExceeded(path.to_path_buf()));
        }
        let mut inner = self.inner.borrow_mut();
        inner.settle(&key);
        inner.resident.insert(key, bytes);
        Ok(())
    }

    /// Drop `path` from the cache, returning whether it was resident.
    ///
    /// The next [`read`](StorageSource::read) re-requests it.
    pub fn evict(&self, path: &Path) -> bool {
        let Ok(key) = canonical_key(path) else {
            return false;
        };
        self.inner.borrow_mut().resident.remove(&key).is_some()
    }

    /// A snapshot for a debug overlay.
    pub fn stats(&self) -> FetchStats {
        let inner = self.inner.borrow();
        FetchStats {
            resident: inner.resident.len(),
            bytes: inner.resident.values().map(Vec::len).sum(),
            queued: inner.queued.len(),
            in_flight: inner.inbox.len(),
            failed: inner.failed.len(),
        }
    }
}

impl Inner {
    /// Forget any queued request, in-flight slot or remembered failure for
    /// `key` — everything a delivery supersedes.
    fn settle(&mut self, key: &str) {
        self.failed.remove(key);
        self.queued.retain(|queued| queued != key);
        if let Some(id) = self.inbox.find(key) {
            self.inbox.discard(id);
            self.urls.remove(&id);
        }
    }
}

impl StorageSource for FetchSource {
    /// Read `path` from the cache, requesting it if it is not there.
    ///
    /// Never waits. The error *is* the state:
    ///
    /// - [`StorageError::Pending`] — queued or in flight; poll again.
    /// - [`StorageError::NotFound`] — the server answered 404 or 410.
    /// - [`StorageError::PermissionDenied`] — 401 or 403.
    /// - [`StorageError::Other`] — any other status, or a network error.
    /// - [`StorageError::InvalidPath`] — the key escapes the asset root.
    ///
    /// A miss on an unknown key enqueues the fetch as a side effect, so a caller
    /// that can tolerate latency needs nothing but a retry. A caller that cannot
    /// should have pre-loaded; see the [parent module](super).
    fn read(&self, path: &Path) -> Result<Vec<u8>, StorageError> {
        let key = canonical_key(path)?;
        {
            let inner = self.inner.borrow();
            if let Some(bytes) = inner.resident.get(&key) {
                return Ok(bytes.clone());
            }
            if let Some(&status) = inner.failed.get(&key) {
                return Err(match status {
                    404 | 410 => StorageError::NotFound(path.to_path_buf()),
                    401 | 403 => StorageError::PermissionDenied(path.to_path_buf()),
                    0 => StorageError::Other(format!("fetch of {key} failed (network error)")),
                    status => StorageError::Other(format!("fetch of {key} failed (HTTP {status})")),
                });
            }
        }
        self.request(path)?;
        Err(StorageError::Pending(path.to_path_buf()))
    }

    /// Always [`StorageError::Unsupported`]: HTTP `GET` has no write.
    ///
    /// Saves and settings go to [`OpfsStorage`](super::OpfsStorage). Succeeding
    /// here — writing into the cache and calling it persistence — would lose the
    /// data at the next reload with no error anywhere.
    fn write(&self, _path: &Path, _data: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported(
            "FetchSource is read-only; use OpfsStorage to persist",
        ))
    }

    /// Always [`StorageError::Unsupported`]. Use [`FetchSource::evict`] to drop
    /// a cache entry — deleting an asset from the server is not a thing a game
    /// does.
    fn delete(&self, _path: &Path) -> Result<(), StorageError> {
        Err(StorageError::Unsupported(
            "FetchSource is read-only; use FetchSource::evict to drop a cached asset",
        ))
    }

    /// Whether `path` is **resident**.
    ///
    /// Not whether the server has it: that question cannot be answered without a
    /// round trip, and this call has nowhere to put one. Unlike
    /// [`read`](Self::read) it does not enqueue anything.
    fn exists(&self, path: &Path) -> bool {
        canonical_key(path).is_ok_and(|key| self.inner.borrow().resident.contains_key(&key))
    }

    /// The resident keys directly under `dir`, as full relative paths.
    ///
    /// The same contract as the native and in-memory backends — full paths, not
    /// bare file names, sorted, direct children only — over what has been
    /// fetched. HTTP has no directory listing, so a key the server holds and the
    /// page has never fetched cannot appear here.
    ///
    /// # Errors
    ///
    /// [`StorageError::InvalidPath`] for a directory key that escapes the asset
    /// root. An unknown directory is empty, not an error, because "no entries"
    /// and "nothing fetched from there" are the same fact here.
    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, StorageError> {
        let dir = canonical_dir(dir)?;
        let inner = self.inner.borrow();
        Ok(direct_children(
            inner.resident.keys().map(String::as_str),
            &dir,
        ))
    }
}

// ---------------------------------------------------------------------------
// The installed source
// ---------------------------------------------------------------------------

thread_local! {
    /// The source the `extern "C"` entry points reach.
    ///
    /// A [`Weak`] rather than an owning handle: the app holds the [`Rc`] and
    /// uses the source as a [`StorageSource`], and the entry points only need to
    /// find it. When the app drops it the exports answer `0` again by
    /// themselves, with no `uninstall` to forget.
    ///
    /// Thread-local for the same reason `crcbl-shell`'s bridge is: whichever
    /// thread the engine runs on is the one the shim calls, and a second one
    /// must not see the first's source.
    static SOURCE: RefCell<Weak<FetchSource>> = const { RefCell::new(Weak::new()) };
}

/// Make `source` the one the `__crcbl_web_fetch_*` entry points reach.
///
/// Returns `false` if a live source is already installed — replacing one would
/// strand every slot the shim is holding an id for. Call it before the shim's
/// first call; until then every export answers `0`.
pub fn install(source: &Rc<FetchSource>) -> bool {
    SOURCE.with(|slot| {
        let Ok(mut slot) = slot.try_borrow_mut() else {
            return false;
        };
        if slot.strong_count() > 0 {
            return false;
        }
        *slot = Rc::downgrade(source);
        true
    })
}

/// Forget the installed source, returning whether there was a live one.
///
/// Rarely needed — dropping the app's [`Rc`] has the same effect — but a test
/// that installs a source must not leak the installation into the next one.
pub fn uninstall() -> bool {
    SOURCE.with(|slot| match slot.try_borrow_mut() {
        Ok(mut slot) => {
            let had = slot.strong_count() > 0;
            *slot = Weak::new();
            had
        }
        Err(_) => false,
    })
}

/// Whether a live source is installed on this thread.
pub fn is_installed() -> bool {
    SOURCE.with(|slot| slot.try_borrow().is_ok_and(|slot| slot.strong_count() > 0))
}

/// Run `f` against the installed source, or answer `absent`.
///
/// `try_borrow` rather than `borrow`: an entry point reached re-entrantly — a
/// shim calling back in from something the engine triggered — answers "no
/// source", which the caller already handles, rather than panicking inside a
/// browser callback where a panic is an aborted page.
fn with_source<R>(absent: R, f: impl FnOnce(&FetchSource) -> R) -> R {
    SOURCE.with(|slot| match slot.try_borrow() {
        Ok(slot) => match slot.upgrade() {
            Some(source) => f(&source),
            None => absent,
        },
        Err(_) => absent,
    })
}

// ---------------------------------------------------------------------------
// extern "C" entry points — called from JS
// ---------------------------------------------------------------------------

/// The JS→wasm ABI. See the [module docs](self) for the whole contract.
///
/// `#[unsafe(no_mangle)]` only on `wasm32`. None of these functions is `unsafe`:
/// none dereferences a pointer the caller supplied. They *return* addresses, and
/// what JS may do with each one is the module docs' buffer-ownership section.
pub mod shim {
    use super::{MAX_ASSET_BYTES, with_source};

    /// The address of the key scratch buffer, or `0` when no source is
    /// installed. Stable for the source's life.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_fetch_key_ptr() -> *mut u8 {
        with_source(std::ptr::null_mut(), |source| {
            source.inner.borrow_mut().scratch.ptr()
        })
    }

    /// How many bytes may be written at that address.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_fetch_key_capacity() -> u32 {
        with_source(0, |source| {
            u32::try_from(source.inner.borrow().scratch.capacity()).unwrap_or(0)
        })
    }

    /// Open a delivery for the `key_len`-byte key in the scratch buffer.
    ///
    /// The pre-load path: the shim has (or is about to have) the bytes for a key
    /// the engine has not asked for. Returns a slot id, or `0` if the key was
    /// not valid UTF-8, escaped the asset root, or there is no source.
    ///
    /// A key that is already resident may be re-delivered; the new body wins.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_fetch_begin(key_len: u32) -> u32 {
        with_source(0, |source| {
            let mut inner = source.inner.borrow_mut();
            let Ok(key) = inner.scratch.key(key_len as usize) else {
                return 0;
            };
            // A pre-load for a key the engine also requested supersedes the
            // request: the shim is already fetching it, and a second fetch would
            // be a wasted round trip on a demo's critical path.
            inner.settle(&key);
            inner.inbox.open(key)
        })
    }

    /// Dequeue one engine-requested fetch, or `0` when there is none.
    ///
    /// The returned slot has a URL; see [`__crcbl_web_fetch_url_ptr`].
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_fetch_take() -> u32 {
        with_source(0, |source| {
            let base = source.base.clone();
            let mut inner = source.inner.borrow_mut();
            let Some(key) = inner.queued.pop_front() else {
                return 0;
            };
            let url = format!("{base}{key}");
            let id = inner.inbox.open(key);
            inner.urls.insert(id, url);
            id
        })
    }

    /// The address of slot `id`'s URL, or `0`.
    ///
    /// UTF-8, **not** NUL-terminated — pair it with
    /// [`__crcbl_web_fetch_url_len`]. Only slots from
    /// [`__crcbl_web_fetch_take`] have one; a slot from
    /// [`__crcbl_web_fetch_begin`] answers `0` because the shim named that key
    /// itself. Valid until the slot is committed or failed.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_fetch_url_ptr(id: u32) -> *const u8 {
        with_source(std::ptr::null(), |source| {
            source
                .inner
                .borrow()
                .urls
                .get(&id)
                .map_or(std::ptr::null(), |url| url.as_ptr())
        })
    }

    /// The length in bytes of slot `id`'s URL, or `0`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_fetch_url_len(id: u32) -> u32 {
        with_source(0, |source| {
            source
                .inner
                .borrow()
                .urls
                .get(&id)
                .and_then(|url| u32::try_from(url.len()).ok())
                .unwrap_or(0)
        })
    }

    /// Size slot `id`'s body buffer to `len` bytes and return its address.
    ///
    /// `0` when the slot does not exist, when `len` exceeds
    /// [`MAX_ASSET_BYTES`], or when there is no source — in every case the shim
    /// must then call [`__crcbl_web_fetch_fail`], because a slot that is neither
    /// committed nor failed never settles.
    ///
    /// **May grow wasm memory**, detaching any `Uint8Array` built before the
    /// call. Build the view afterwards.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_fetch_buffer(id: u32, len: u32) -> *mut u8 {
        with_source(std::ptr::null_mut(), |source| {
            source
                .inner
                .borrow_mut()
                .inbox
                .buffer(id, len as usize, MAX_ASSET_BYTES)
                .unwrap_or(std::ptr::null_mut())
        })
    }

    /// Commit the first `len` bytes of slot `id`'s buffer as the body.
    ///
    /// Makes the key resident and closes the slot. `1` on success; `0` when the
    /// slot does not exist or `len` is longer than the buffer that was handed
    /// out — a shim that commits more than it asked for gets nothing rather than
    /// an out-of-bounds read, and the slot stays open for it to fail properly.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_fetch_commit(id: u32, len: u32) -> u32 {
        with_source(0, |source| {
            let mut inner = source.inner.borrow_mut();
            let Some((key, bytes)) = inner.inbox.commit(id, len as usize) else {
                return 0;
            };
            inner.urls.remove(&id);
            inner.failed.remove(&key);
            inner.resident.insert(key, bytes);
            1
        })
    }

    /// Report that slot `id` failed with HTTP `status` (`0` for a network
    /// error).
    ///
    /// Closes the slot and remembers the failure, so a subsequent
    /// [`read`](crate::StorageSource::read) reports it instead of re-queueing
    /// forever. `1` if the slot existed.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_fetch_fail(id: u32, status: u32) -> u32 {
        with_source(0, |source| {
            let mut inner = source.inner.borrow_mut();
            let Some(key) = inner.inbox.discard(id) else {
                return 0;
            };
            inner.urls.remove(&id);
            inner.failed.insert(key, u16::try_from(status).unwrap_or(0));
            1
        })
    }

    /// How many requests are queued and not yet taken.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_fetch_pending() -> u32 {
        with_source(0, |source| {
            u32::try_from(source.inner.borrow().queued.len()).unwrap_or(u32::MAX)
        })
    }

    /// How many slots are open — taken or begun, and not yet settled.
    ///
    /// Zero after a pre-load means every response landed. A number that never
    /// falls is a shim leaking slots.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_fetch_inflight() -> u32 {
        with_source(0, |source| {
            u32::try_from(source.inner.borrow().inbox.len()).unwrap_or(u32::MAX)
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    fn source() -> FetchSource {
        FetchSource::new("assets/").expect("a valid base")
    }

    #[test]
    fn a_base_that_could_undo_the_key_rules_is_refused_at_construction() {
        for base in ["", "assets", "assets/../", "assets/?v=1/", "aßets/"] {
            assert!(
                matches!(FetchSource::new(base), Err(StorageError::InvalidPath(_))),
                "{base:?} should be refused"
            );
        }
        assert_eq!(source().base(), "assets/");
    }

    #[test]
    fn a_key_becomes_a_url_in_exactly_one_place_and_cannot_leave_the_root() {
        let source = source();
        assert_eq!(
            source.url_for(Path::new("./sprites/ball.png")).unwrap(),
            "assets/sprites/ball.png"
        );
        // Every one of these would reach outside the asset directory, or off the
        // origin entirely, if it were concatenated.
        for escape in [
            "../secret",
            "/etc/passwd",
            "//evil.example/x",
            "a/../../b",
            "x.png?redirect=//evil.example",
            "x.png#/../..",
            "%2e%2e/up",
        ] {
            assert!(
                source.url_for(Path::new(escape)).is_err(),
                "{escape} produced a URL"
            );
            // And the same key cannot be requested, read or listed either.
            assert!(source.request(Path::new(escape)).is_err());
            assert!(source.read(Path::new(escape)).is_err());
            assert!(!source.exists(Path::new(escape)));
            assert_eq!(source.state(Path::new(escape)), FetchState::Unknown);
        }
    }

    #[test]
    fn a_read_of_an_unknown_key_enqueues_it_and_reports_pending() {
        let source = source();
        let key = Path::new("sprites/ball.png");
        assert_eq!(source.state(key), FetchState::Unknown);

        assert!(matches!(source.read(key), Err(StorageError::Pending(_))));
        assert_eq!(source.state(key), FetchState::Queued);
        assert_eq!(source.stats().queued, 1);

        // Polling every frame must not enqueue it again.
        for _ in 0..8 {
            assert!(matches!(source.read(key), Err(StorageError::Pending(_))));
        }
        assert_eq!(source.stats().queued, 1);
    }

    #[test]
    fn the_preload_path_makes_a_read_a_hash_lookup() {
        let source = source();
        let key = Path::new("sprites/ball.png");
        source.insert(key, b"body".to_vec()).unwrap();

        assert_eq!(source.state(key), FetchState::Ready);
        assert_eq!(source.read(key).unwrap(), b"body");
        assert!(source.exists(key));
        assert_eq!(source.stats().queued, 0, "a hit enqueues nothing");
        assert_eq!(source.stats().bytes, 4);
    }

    #[test]
    fn write_and_delete_are_refused_rather_than_silently_lost() {
        let source = source();
        let key = Path::new("save.bin");
        assert!(matches!(
            source.write(key, b"x"),
            Err(StorageError::Unsupported(_))
        ));
        assert!(matches!(
            source.delete(key),
            Err(StorageError::Unsupported(_))
        ));
        assert!(!source.exists(key));
    }

    #[test]
    fn eviction_puts_a_key_back_to_unknown() {
        let source = source();
        let key = Path::new("a.png");
        source.insert(key, b"x".to_vec()).unwrap();
        assert!(source.evict(key));
        assert!(!source.evict(key));
        assert_eq!(source.state(key), FetchState::Unknown);
    }

    /// The `list` contract the review found the two native backends disagreeing
    /// about: full relative paths, sorted, direct children only.
    #[test]
    fn list_matches_the_memory_backend_entry_for_entry() {
        let fetch = source();
        let memory = MemoryStorage::new();
        for key in ["sprites/a.png", "sprites/b.png", "sound/c.wav", "top.txt"] {
            fetch.insert(Path::new(key), b"x".to_vec()).unwrap();
            memory.write(Path::new(key), b"x").unwrap();
        }
        // A nested key must not surface as an entry of the parent directory.
        fetch.insert(Path::new("sprites/ui/d.png"), vec![]).unwrap();
        memory.write(Path::new("sprites/ui/d.png"), b"").unwrap();

        for dir in ["sprites", "sound", ".", "", "sprites/ui", "nothing/here"] {
            assert_eq!(
                fetch.list(Path::new(dir)).unwrap(),
                memory.list(Path::new(dir)).unwrap(),
                "list({dir:?})"
            );
        }
        assert_eq!(
            fetch.list(Path::new("sprites")).unwrap(),
            vec![
                PathBuf::from("sprites/a.png"),
                PathBuf::from("sprites/b.png")
            ]
        );
        assert!(fetch.list(Path::new("../..")).is_err());
    }

    #[test]
    fn the_queue_is_bounded_so_a_missing_shim_is_an_error_not_a_leak() {
        let source = source();
        for i in 0..MAX_QUEUED_REQUESTS {
            source.request(Path::new(&format!("a{i}.bin"))).unwrap();
        }
        assert!(matches!(
            source.request(Path::new("one-too-many.bin")),
            Err(StorageError::LimitExceeded(_))
        ));
        assert_eq!(source.stats().queued, MAX_QUEUED_REQUESTS);
    }

    // -- the thread-local and the entry points ------------------------------

    /// The entry points share one thread-local, so the tests that touch it run
    /// under a mutex and clean up after themselves. `nextest` runs each test in
    /// its own process, but `cargo test` does not.
    static SHIM: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn shim_guard() -> std::sync::MutexGuard<'static, ()> {
        let guard = SHIM.lock().unwrap_or_else(|e| e.into_inner());
        uninstall();
        guard
    }

    /// Write `key` into the scratch buffer exactly as the shim does, and return
    /// its length.
    fn write_key(key: &str) -> u32 {
        let ptr = shim::__crcbl_web_fetch_key_ptr();
        assert!(!ptr.is_null(), "no source installed");
        assert!(key.len() <= shim::__crcbl_web_fetch_key_capacity() as usize);
        // SAFETY: `ptr` is the installed source's key scratch, whose capacity
        // was just checked against `key.len()`; nothing else borrows it between
        // the call above and `__crcbl_web_fetch_begin` below. This is the shim's
        // `new Uint8Array(memory.buffer, ptr, n).set(bytes)`.
        unsafe { std::ptr::copy_nonoverlapping(key.as_ptr(), ptr, key.len()) };
        u32::try_from(key.len()).expect("a short key")
    }

    /// Deliver `body` into slot `id`, exactly as the shim does.
    fn deliver(id: u32, body: &[u8]) -> u32 {
        let len = u32::try_from(body.len()).expect("a short body");
        let ptr = shim::__crcbl_web_fetch_buffer(id, len);
        assert!(!ptr.is_null(), "buffer refused");
        // SAFETY: `__crcbl_web_fetch_buffer` sized slot `id`'s buffer to
        // `body.len()` bytes and returned its address; the slot is not settled
        // until `commit` below, and nothing else touches it in between.
        unsafe { std::ptr::copy_nonoverlapping(body.as_ptr(), ptr, body.len()) };
        shim::__crcbl_web_fetch_commit(id, len)
    }

    #[test]
    fn the_entry_points_answer_zero_until_a_source_is_installed() {
        let _guard = shim_guard();
        assert!(!is_installed());
        assert!(shim::__crcbl_web_fetch_key_ptr().is_null());
        assert_eq!(shim::__crcbl_web_fetch_key_capacity(), 0);
        assert_eq!(shim::__crcbl_web_fetch_begin(4), 0);
        assert_eq!(shim::__crcbl_web_fetch_take(), 0);
        assert!(shim::__crcbl_web_fetch_url_ptr(1).is_null());
        assert_eq!(shim::__crcbl_web_fetch_url_len(1), 0);
        assert!(shim::__crcbl_web_fetch_buffer(1, 4).is_null());
        assert_eq!(shim::__crcbl_web_fetch_commit(1, 4), 0);
        assert_eq!(shim::__crcbl_web_fetch_fail(1, 404), 0);
        assert_eq!(shim::__crcbl_web_fetch_pending(), 0);
        assert_eq!(shim::__crcbl_web_fetch_inflight(), 0);
    }

    #[test]
    fn the_preload_sequence_makes_a_key_resident() {
        let _guard = shim_guard();
        let source = Rc::new(source());
        assert!(install(&source));
        assert!(!install(&source), "a second install must not replace it");
        assert!(is_installed());

        let len = write_key("sprites/ball.png");
        let id = shim::__crcbl_web_fetch_begin(len);
        assert_ne!(id, 0);
        assert_eq!(shim::__crcbl_web_fetch_inflight(), 1);
        // A begun slot has no URL: the shim named the key itself.
        assert!(shim::__crcbl_web_fetch_url_ptr(id).is_null());
        assert_eq!(
            source.state(Path::new("sprites/ball.png")),
            FetchState::InFlight
        );

        assert_eq!(deliver(id, b"PNG"), 1);
        assert_eq!(shim::__crcbl_web_fetch_inflight(), 0);
        assert_eq!(source.read(Path::new("sprites/ball.png")).unwrap(), b"PNG");
        assert_eq!(source.stats().queued, 0, "the pre-load asked for nothing");
    }

    #[test]
    fn the_request_poll_sequence_hands_the_shim_a_whole_url() {
        let _guard = shim_guard();
        let source = Rc::new(source());
        assert!(install(&source));
        let key = Path::new("sound/bounce.wav");

        assert!(matches!(source.read(key), Err(StorageError::Pending(_))));
        assert_eq!(shim::__crcbl_web_fetch_pending(), 1);

        let id = shim::__crcbl_web_fetch_take();
        assert_ne!(id, 0);
        assert_eq!(shim::__crcbl_web_fetch_take(), 0, "the queue is drained");
        assert_eq!(source.state(key), FetchState::InFlight);

        let ptr = shim::__crcbl_web_fetch_url_ptr(id);
        let len = shim::__crcbl_web_fetch_url_len(id) as usize;
        assert!(!ptr.is_null());
        // SAFETY: `url_ptr`/`url_len` describe slot `id`'s URL string, which
        // lives until the slot is committed or failed — neither has happened.
        // This is the shim's `TextDecoder().decode(...)`.
        let url = unsafe { std::slice::from_raw_parts(ptr, len) };
        assert_eq!(url, b"assets/sound/bounce.wav");

        assert_eq!(deliver(id, b"RIFF"), 1);
        assert_eq!(source.read(key).unwrap(), b"RIFF");
        assert!(
            shim::__crcbl_web_fetch_url_ptr(id).is_null(),
            "a settled slot has no URL"
        );
    }

    #[test]
    fn a_failure_is_remembered_and_reported_as_the_status_means() {
        let _guard = shim_guard();
        let source = Rc::new(source());
        assert!(install(&source));

        for (status, check) in [(404u32, 0u8), (403, 1), (500, 2), (0, 2)] {
            let key = format!("a{status}.bin");
            let path = PathBuf::from(&key);
            assert!(matches!(source.read(&path), Err(StorageError::Pending(_))));
            let id = shim::__crcbl_web_fetch_take();
            assert_ne!(id, 0);
            assert_eq!(shim::__crcbl_web_fetch_fail(id, status), 1);

            assert_eq!(
                source.state(&path),
                FetchState::Failed {
                    status: u16::try_from(status).unwrap()
                }
            );
            let err = source.read(&path).unwrap_err();
            match check {
                0 => assert!(matches!(err, StorageError::NotFound(_)), "{err:?}"),
                1 => assert!(matches!(err, StorageError::PermissionDenied(_)), "{err:?}"),
                _ => assert!(matches!(err, StorageError::Other(_)), "{err:?}"),
            }
            // A failed read does not re-queue: the loop would be forever.
            assert_eq!(shim::__crcbl_web_fetch_pending(), 0);

            // The engine's own retry is the only one.
            source.request(&path).unwrap();
            assert_eq!(source.state(&path), FetchState::Queued);
            let id = shim::__crcbl_web_fetch_take();
            assert_eq!(deliver(id, b"ok"), 1);
            assert_eq!(source.read(&path).unwrap(), b"ok");
        }
    }

    #[test]
    fn a_shim_that_misuses_a_slot_id_gets_zero_and_changes_nothing() {
        let _guard = shim_guard();
        let source = Rc::new(source());
        assert!(install(&source));

        // A key the ABI must refuse rather than canonicalise into something
        // else: the manifest is as untrusted as any other input.
        let len = write_key("../../etc/passwd");
        assert_eq!(shim::__crcbl_web_fetch_begin(len), 0);
        assert_eq!(shim::__crcbl_web_fetch_inflight(), 0);

        let len = write_key("ok.bin");
        let id = shim::__crcbl_web_fetch_begin(len);
        assert_ne!(id, 0);
        // Committing more than was asked for must not read past the buffer.
        assert!(!shim::__crcbl_web_fetch_buffer(id, 4).is_null());
        assert_eq!(shim::__crcbl_web_fetch_commit(id, 5), 0);
        assert_eq!(shim::__crcbl_web_fetch_inflight(), 1, "still open to fail");
        // A body over the bound is refused, and the shim must then fail it.
        assert!(
            shim::__crcbl_web_fetch_buffer(id, u32::MAX).is_null(),
            "an absurd length must not allocate"
        );
        assert_eq!(shim::__crcbl_web_fetch_fail(id, 0), 1);
        assert_eq!(shim::__crcbl_web_fetch_fail(id, 0), 0, "already settled");
        assert!(!source.exists(Path::new("ok.bin")));

        // Ids from a dead source answer zero rather than aliasing a new one.
        drop(source);
        assert!(!is_installed());
        assert_eq!(shim::__crcbl_web_fetch_take(), 0);
    }

    #[test]
    fn a_preload_supersedes_a_request_for_the_same_key() {
        let _guard = shim_guard();
        let source = Rc::new(source());
        assert!(install(&source));
        let key = Path::new("shared.bin");

        source.request(key).unwrap();
        assert_eq!(shim::__crcbl_web_fetch_pending(), 1);

        // The shim pre-loads the same key. The queued request must go away, or
        // the shim fetches it a second time on the demo's critical path.
        let len = write_key("shared.bin");
        let id = shim::__crcbl_web_fetch_begin(len);
        assert_ne!(id, 0);
        assert_eq!(shim::__crcbl_web_fetch_pending(), 0);
        assert_eq!(shim::__crcbl_web_fetch_take(), 0);

        assert_eq!(deliver(id, b"once"), 1);
        assert_eq!(source.read(key).unwrap(), b"once");
        assert_eq!(shim::__crcbl_web_fetch_inflight(), 0);
    }
}
