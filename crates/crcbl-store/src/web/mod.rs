//! Browser storage (P5.7): [`FetchSource`] for assets, [`OpfsStorage`] for
//! saves and settings.
//!
//! [`StorageSource`](crate::StorageSource) is synchronous. The browser has no
//! blocking IO of any kind — `fetch()` is a promise, every Origin Private File
//! System entry point that matters is a promise, and there is no thread to park
//! on the main thread of a page. This module resolves that mismatch **without**
//! an async runtime and without a blocking-executor hack, in the same shape the
//! rest of the P5 work settled on: a request/deliver queue that a JS shim
//! drains, with plain Rust state behind it that tests drive directly.
//!
//! # The shape, and why this one
//!
//! Both backends are a **resident cache with an out-of-band fill**:
//!
//! ```text
//!     engine (sync)                    shim (async)
//!     ─────────────                    ────────────
//!     read(key) ──► resident? ──► yes ──► Ok(bytes)
//!                       │
//!                       no ──► enqueue ──►  take() ─► fetch()/OPFS ─┐
//!                       │                                          │
//!                   Err(Pending)  ◄── poll next frame               │
//!                       ▲                                           │
//!                       └──────────── buffer()+commit() ◄───────────┘
//! ```
//!
//! Nothing in the engine's call path ever waits. A read of something not yet
//! resident answers [`StorageError::Pending`], which is a state the caller polls
//! rather than a stall, and a write returns as soon as the record is *queued*.
//!
//! ## Read: pre-load, with request/poll underneath
//!
//! The roadmap's P5 row names a "`FetchSource`-lite", and the codebase already
//! set two precedents for the async→sync problem: `Instance::request_device` →
//! `PendingDevice::poll`, and `request_readback`/`poll_readback`. Both are
//! request/poll. This module keeps request/poll as the *mechanism* but makes
//! **pre-load the intended mode**, because of how the demo actually boots:
//!
//! 1. the page fetches and compiles `crcbl.wasm`;
//! 2. the shim fetches every asset named in a small manifest — the same network
//!    round trips the engine would make, only in parallel and while the wasm is
//!    still compiling;
//! 3. each response is pushed into the instance
//!    ([`__crcbl_web_fetch_begin`](fetch::shim::__crcbl_web_fetch_begin) →
//!    `buffer` → `commit`);
//! 4. *then* the app's boot export runs and the first `tick(dt)` happens.
//!
//! By the time any engine code calls `read`, the answer is resident and the call
//! is a hash lookup. That matters because the P4 sample's startup is not written
//! as a state machine that tolerates a missing asset for forty frames — it loads
//! what it needs and draws. Making the *demo's* path synchronous-in-effect is
//! what keeps the P5 gate ("breakout playable in browser") reachable without
//! rewriting the sample's boot.
//!
//! Request/poll is still there and still first-class, because pre-load alone is
//! a lie for anything discovered at runtime (a level's assets, a texture chosen
//! by a settings value). A `read` miss enqueues the fetch itself, so the caller
//! that can tolerate latency gets it by retrying, and the caller that cannot
//! pre-loaded.
//!
//! ## Write: queue + acknowledgement
//!
//! OPFS is the same problem with the additional constraint that a write must
//! survive a reload. [`OpfsStorage::write`](OpfsStorage) updates the resident map
//! immediately — so read-after-write within a session is exact — and appends a
//! record to a bounded queue the shim drains and acknowledges. See that type's
//! docs for what happens to [`write_atomic`](crate::write_atomic)'s guarantee,
//! which is the part a caller must not assume.
//!
//! # Where the engine runs (the assumption, made checkable)
//!
//! OPFS has two write APIs: `createWritable()`, available on the main thread and
//! in workers, asynchronous; and `createSyncAccessHandle()`, whose read/write
//! calls are synchronous but which **only exists inside a Worker**.
//!
//! This module assumes **neither**. Every OPFS call is the shim's, is allowed to
//! be asynchronous, and is acknowledged out of band — so the engine may run on
//! the main thread (which is what the P5 canvas/rAF shell does today) or in a
//! Worker, and the same ABI serves both. A shim in a Worker that wants sync
//! access handles for lower latency can use them; nothing here changes.
//!
//! The assumption is *declared* rather than left implicit:
//! [`__crcbl_web_opfs_environment`](opfs::shim::__crcbl_web_opfs_environment)
//! lets the shim state where it is running and what it has
//! ([`OpfsEnvironment`]), and [`OpfsStorage::environment`] reads it back for a
//! debug HUD. A shim that never calls it reads as `unknown`, not as "worker".
//!
//! And it is *checkable*: if the assumption is wrong — no shim, a shim that
//! never drains, a Worker that died — the queue fills and
//! [`write`](OpfsStorage) starts returning [`StorageError::LimitExceeded`]
//! instead of silently accepting writes that will never land.
//! [`OpfsStorage::stats`] shows the backlog before that happens.
//!
//! # Path containment: an origin is a security boundary
//!
//! `NativeStorage::resolve` rejects `..` and absolute paths because a caller-
//! supplied key must not escape the storage root. A URL assembled from a key has
//! the same problem with a longer list of ways to go wrong: `..` climbs out of
//! the asset directory, a leading `/` reaches the site root, `//host/x` is
//! **another origin**, `foo:` can be read as a scheme, `?` starts a query, `#`
//! truncates the path, `%2e%2e` is `..` after the server decodes it, and a
//! backslash is a separator to some servers.
//!
//! So every key — from the engine *and* from the shim's manifest — goes through
//! [`canonical_key`], which requires:
//!
//! - the same containment rule as `NativeStorage`: relative, no prefix, no root,
//!   no `..` (bare `.` components are dropped, as they are natively);
//! - at least one component, and no empty component;
//! - every component drawn from `[A-Za-z0-9._-]` only — which excludes `/` `\`
//!   `:` `?` `#` `%` `@` whitespace and every control character, so the key needs
//!   no percent-encoding and cannot contain a URL metacharacter;
//! - a total length of at most [`MAX_KEY_LEN`] bytes.
//!
//! The URL is then built **in Rust** as `base + key` and handed to the shim
//! whole ([`__crcbl_web_fetch_url_ptr`](fetch::shim::__crcbl_web_fetch_url_ptr)).
//! The shim fetches exactly the string it is given and never concatenates one
//! itself, so there is one place where a key becomes a URL and it is the place
//! that validated the key. The base is validated too, and must end in `/`, so a
//! first component containing `:` could not have been read as a scheme even if
//! the charset allowed it.
//!
//! The same rule guards the OPFS side, where the failure mode is different but
//! real: OPFS directory handles are reached by name, `getFileHandle("..")`
//! throws, and a key with a `/` in a component would silently address a
//! different directory than the one [`list`](crate::StorageSource::list)
//! reported.
//!
//! # The JS↔wasm ABI
//!
//! Every symbol, its signature, who owns which buffer, the call order and the
//! failure behaviour are specified in [`fetch`] and [`opfs`]. Read those before
//! writing the shim. Three rules hold across both, and each is a real bug when
//! missed:
//!
//! 1. **`#[unsafe(no_mangle)]` is applied only on `wasm32`.** A native binary
//!    that links the engine exports no `__crcbl_web_*`. The functions still
//!    compile natively, which is how the tests in this module drive them.
//! 2. **Every buffer belongs to wasm.** JS never passes a pointer in; it writes
//!    into a pointer wasm handed out, or reads from one. There is no `malloc`
//!    export and no JS-owned memory in the contract.
//! 3. **A `Uint8Array` over `memory.buffer` is detached when wasm memory
//!    grows** — the same trap `crcbl-audio`'s worklet documents for
//!    `Float32Array`. Both `*_buffer` calls here may grow memory, so the view
//!    must be constructed *after* the call that returns the pointer and must not
//!    be cached across another one. Rebuilding per delivery costs nothing
//!    against a network round trip.
//!
//! # What is not closed
//!
//! - **No IndexedDB fallback.** `docs/plan/14-persistence.md` names IndexedDB as
//!   the fallback for browsers without OPFS. Nothing here needs it — the wasm
//!   side is a queue of `(name, bytes)` records and a shim could satisfy them
//!   from IndexedDB without this crate knowing — but no shim does yet, and the
//!   choice is not represented in [`OpfsEnvironment`].
//! - **No quota negotiation.** `navigator.storage.estimate()` is not surfaced;
//!   the engine learns about a full disk from a failed acknowledgement, after
//!   the fact.
//! - **No range requests or streaming.** A fetched asset is delivered whole. An
//!   asset larger than [`fetch::MAX_ASSET_BYTES`] is refused rather than
//!   streamed.
//! - **No cross-session ordering audit.** The generation scheme in [`opfs`]
//!   assumes the shim applies a key's records in the order it took them; a shim
//!   that runs them concurrently can land an older generation last. The wasm
//!   side cannot detect that, so it is a stated shim obligation and not an
//!   enforced one.

pub mod fetch;
pub mod opfs;

use std::collections::BTreeMap;
use std::path::Path;

use crate::StorageError;

pub use fetch::{FetchSource, FetchState, FetchStats};
pub use opfs::{OpfsEnvironment, OpfsStats, OpfsStorage};

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// The longest key either browser backend accepts, in bytes.
///
/// Bounds the scratch buffer the shim writes keys into, and keeps a
/// pathological manifest entry from becoming a multi-kilobyte URL. Well above
/// anything an asset tree needs.
pub const MAX_KEY_LEN: usize = 255;

/// Canonicalise a storage key for browser use, or reject it.
///
/// The result is the key's components joined with `/`, with `.` components
/// dropped — the same normalisation `NativeStorage` applies before joining onto
/// its root, so `./saves/a.crb` and `saves/a.crb` are one key here as they are
/// one path there.
///
/// See the module docs for the rule and the reasoning. Briefly: relative, no
/// `..`, no empty component, `[A-Za-z0-9._-]` only, at most [`MAX_KEY_LEN`]
/// bytes.
///
/// # Errors
///
/// [`StorageError::InvalidPath`] for anything that fails those rules. There is
/// deliberately no "sanitising" path that strips the offending part: a key that
/// does not mean what the caller wrote is worse than a key that is refused.
///
/// ```
/// use crcbl_store::web::canonical_key;
/// use std::path::Path;
///
/// assert_eq!(canonical_key(Path::new("./sprites/ball.png")).unwrap(), "sprites/ball.png");
/// assert!(canonical_key(Path::new("../secrets")).is_err());
/// assert!(canonical_key(Path::new("/etc/passwd")).is_err());
/// assert!(canonical_key(Path::new("a/../b")).is_err());
/// assert!(canonical_key(Path::new("save.bin?x=1")).is_err());
/// assert!(canonical_key(Path::new("%2e%2e/up")).is_err());
/// ```
pub fn canonical_key(path: &Path) -> Result<String, StorageError> {
    let invalid = || StorageError::InvalidPath(path.to_path_buf());

    // Split the string on `/` rather than walking `Path::components()`.
    //
    // A key here is a **URL path** — what `FetchSource` appends to its base and
    // what an OPFS name is looked up by — and `Path`'s idea of a separator is
    // the *host's*. On Windows `a\b` parses as two components, so this function
    // refused that key on Linux and quietly rewrote it to `a/b` on Windows: the
    // one function whose job is to say what a key may contain gave two answers
    // depending on which machine asked. The Windows CI leg is where that
    // surfaced, and a manifest built on Windows is where it would have bitten.
    //
    // With the split done here, `\` is simply not a key byte on any platform,
    // and `..` is rejected by name rather than by `Component::ParentDir`.
    let raw = path.to_str().ok_or_else(invalid)?;
    // One trailing slash is allowed and dropped, which is what `Path` did with
    // `saves/` and what a caller naming a directory writes.
    let raw = raw.strip_suffix('/').unwrap_or(raw);

    let mut key = String::with_capacity(raw.len());
    for part in raw.split('/') {
        if part == "." {
            continue;
        }
        if part.is_empty() || part == ".." || !part.bytes().all(is_key_byte) {
            return Err(invalid());
        }
        if !key.is_empty() {
            key.push('/');
        }
        key.push_str(part);
    }

    if key.is_empty() || key.len() > MAX_KEY_LEN {
        return Err(invalid());
    }
    Ok(key)
}

/// Canonicalise a directory key for [`list`](crate::StorageSource::list).
///
/// The same rules as [`canonical_key`], except that the empty key is the
/// storage root rather than an error — `list(".")` and `list("")` both mean
/// "the top level", as they do for `NativeStorage` and `MemoryStorage`.
///
/// # Errors
///
/// [`StorageError::InvalidPath`], as [`canonical_key`].
fn canonical_dir(path: &Path) -> Result<String, StorageError> {
    // String-first, for the reason `canonical_key` gives at length.
    let raw = path.to_str().unwrap_or("\0");
    let trimmed = raw.strip_suffix('/').unwrap_or(raw);
    if trimmed.is_empty() || trimmed.split('/').all(|part| part == ".") {
        return Ok(String::new());
    }
    canonical_key(path)
}

/// The direct children of `dir` among `keys`, as full relative paths.
///
/// The [`StorageSource::list`](crate::StorageSource::list) contract, factored
/// out: both browser backends list a resident map rather than a directory, and
/// the review finding that `NativeStorage` and `MemoryStorage` disagreed about
/// whether entries are full paths or bare file names is not one to reproduce a
/// third and fourth time.
fn direct_children<'a>(keys: impl Iterator<Item = &'a str>, dir: &str) -> Vec<std::path::PathBuf> {
    let mut entries: Vec<std::path::PathBuf> = keys
        .filter(|key| {
            let rest = if dir.is_empty() {
                Some(*key)
            } else {
                key.strip_prefix(dir)
                    .and_then(|rest| rest.strip_prefix('/'))
            };
            rest.is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
        })
        .map(std::path::PathBuf::from)
        .collect();
    entries.sort();
    entries
}

/// Whether `byte` may appear in a key component.
///
/// The allow-list is the whole of the containment argument on the URL side: it
/// admits no separator, no URL metacharacter, no percent sign and no control
/// character, so a validated key is safe to concatenate onto a base URL
/// verbatim.
const fn is_key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}

/// Whether `base` is a usable asset-root URL prefix.
///
/// The base is the page's, not a caller's — but it is still the other half of
/// every URL this module builds, so a base that undoes the key rules (a `?`
/// that turns the key into a query, a missing `/` that pastes the key onto the
/// last path segment) is refused at construction rather than discovered as a
/// 404.
fn is_valid_base(base: &str) -> bool {
    !base.is_empty()
        && base.len() <= 512
        && base.ends_with('/')
        && base.is_ascii()
        && !base.contains("..")
        && !base.contains(['?', '#', '\\', ' ', '"', '\''])
        && !base.bytes().any(|b| b.is_ascii_control())
}

// ---------------------------------------------------------------------------
// Inbox: the JS→wasm delivery slots both backends use
// ---------------------------------------------------------------------------

/// A delivery in progress: a key the shim is bringing bytes for, and the buffer
/// it writes them into.
#[derive(Debug)]
struct InboxSlot {
    /// The canonical key (fetch) or physical file name (OPFS) being delivered.
    name: String,
    /// Wasm-owned bytes. Sized by `buffer`, filled by JS, taken by `commit`.
    buffer: Vec<u8>,
}

/// The JS→wasm half of both backends: numbered slots, each a name plus a buffer.
///
/// Shared because the fetch and OPFS deliveries are the same three calls with
/// different names — open a slot, ask for a buffer of `n` bytes, commit — and a
/// second copy of the id bookkeeping is a second place for an id to be reused
/// while a delivery is outstanding.
///
/// `BTreeMap` rather than `HashMap` so [`Inbox::names`] is ordered, which makes
/// the tests' expectations stable.
#[derive(Debug, Default)]
struct Inbox {
    slots: BTreeMap<u32, InboxSlot>,
    /// The next id to hand out. Never `0` — `0` is the ABI's "no slot".
    next_id: u32,
}

impl Inbox {
    /// Open a slot for `name` and return its id.
    ///
    /// Ids are monotonic and skip `0` on wrap, so an id the shim is holding is
    /// never confused with "none". Wrapping requires four billion deliveries in
    /// one session, and even then only collides if a slot from the previous lap
    /// is still open.
    fn open(&mut self, name: String) -> u32 {
        self.next_id = self.next_id.wrapping_add(1);
        if self.next_id == 0 {
            self.next_id = 1;
        }
        let id = self.next_id;
        self.slots.insert(
            id,
            InboxSlot {
                name,
                buffer: Vec::new(),
            },
        );
        id
    }

    /// Size slot `id`'s buffer to `len` bytes and return a pointer to it.
    ///
    /// `None` when the slot does not exist or `len` exceeds `limit`. The
    /// pointer is invalidated by the next call for the same slot and by
    /// `commit`/`discard`; growing the buffer may grow wasm memory, which is the
    /// detached-view trap in the module docs.
    fn buffer(&mut self, id: u32, len: usize, limit: usize) -> Option<*mut u8> {
        if len > limit {
            return None;
        }
        let slot = self.slots.get_mut(&id)?;
        slot.buffer.clear();
        slot.buffer.resize(len, 0);
        Some(slot.buffer.as_mut_ptr())
    }

    /// Close slot `id` and return its name and the first `len` bytes written.
    ///
    /// `None` when the slot does not exist or `len` is longer than the buffer
    /// that was handed out — a shim committing more than it asked for is a bug
    /// that must not become an out-of-bounds read.
    fn commit(&mut self, id: u32, len: usize) -> Option<(String, Vec<u8>)> {
        let slot = self.slots.get(&id)?;
        if len > slot.buffer.len() {
            return None;
        }
        let mut slot = self.slots.remove(&id)?;
        slot.buffer.truncate(len);
        Some((slot.name, slot.buffer))
    }

    /// Close slot `id` without taking its bytes, returning its name.
    fn discard(&mut self, id: u32) -> Option<String> {
        self.slots.remove(&id).map(|slot| slot.name)
    }

    /// How many deliveries are open.
    fn len(&self) -> usize {
        self.slots.len()
    }

    /// The id of the open slot for `name`, if any.
    fn find(&self, name: &str) -> Option<u32> {
        self.slots
            .iter()
            .find(|(_, slot)| slot.name == name)
            .map(|(id, _)| *id)
    }
}

// ---------------------------------------------------------------------------
// Key scratch: the one buffer JS writes strings into
// ---------------------------------------------------------------------------

/// A fixed-capacity buffer the shim writes a key into before naming it.
///
/// JS cannot pass a string to wasm; it can only write bytes into wasm memory.
/// Rather than export an allocator, each backend owns one scratch buffer of
/// [`MAX_KEY_LEN`] bytes: the shim encodes the key with `TextEncoder`, copies it
/// in, and passes the length. The buffer is allocated once and never resized, so
/// its address is stable for the life of the source — but wasm memory can still
/// grow for other reasons, so the caveat about rebuilding the `Uint8Array` view
/// applies here too.
#[derive(Debug)]
struct KeyScratch {
    bytes: Vec<u8>,
}

impl Default for KeyScratch {
    fn default() -> Self {
        Self {
            bytes: vec![0; MAX_KEY_LEN],
        }
    }
}

impl KeyScratch {
    /// The address JS writes to. Stable for the life of this scratch.
    fn ptr(&mut self) -> *mut u8 {
        self.bytes.as_mut_ptr()
    }

    /// How many bytes may be written there: [`MAX_KEY_LEN`].
    fn capacity(&self) -> usize {
        self.bytes.len()
    }

    /// Read back the first `len` bytes as UTF-8, without interpreting them.
    ///
    /// The OPFS restore path needs this: the names it delivers are *physical*
    /// file names, which carry a generation suffix that [`canonical_key`]
    /// rightly refuses.
    ///
    /// # Errors
    ///
    /// [`StorageError::InvalidPath`] when `len` is past the buffer or the bytes
    /// are not UTF-8.
    fn raw(&self, len: usize) -> Result<&str, StorageError> {
        let unnamed = || StorageError::InvalidPath(std::path::PathBuf::new());
        let bytes = self.bytes.get(..len).ok_or_else(unnamed)?;
        std::str::from_utf8(bytes).map_err(|_| unnamed())
    }

    /// Replace the scratch's contents with `bytes`, returning the new length.
    ///
    /// The one wasm→JS use of this buffer: an entry point that answers with a
    /// string writes it back where the shim already has a view.
    ///
    /// `None` when `bytes` does not fit.
    fn write_back(&mut self, bytes: &[u8]) -> Option<usize> {
        self.bytes.get_mut(..bytes.len())?.copy_from_slice(bytes);
        Some(bytes.len())
    }

    /// Read back the first `len` bytes as a canonical key.
    ///
    /// # Errors
    ///
    /// [`StorageError::InvalidPath`] when `len` is out of range, when the bytes
    /// are not UTF-8, or when [`canonical_key`] refuses them.
    fn key(&self, len: usize) -> Result<String, StorageError> {
        canonical_key(Path::new(self.raw(len)?))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_that_stay_inside_the_root_are_canonicalised() {
        for (input, expected) in [
            ("a.txt", "a.txt"),
            ("./a.txt", "a.txt"),
            ("saves/slot1.crb", "saves/slot1.crb"),
            ("./saves/./campaign/slot1.crb", "saves/campaign/slot1.crb"),
            ("sprites/ball-2.png", "sprites/ball-2.png"),
            ("a_b/c.d.e", "a_b/c.d.e"),
        ] {
            assert_eq!(
                canonical_key(Path::new(input)).expect(input),
                expected,
                "{input}"
            );
        }
    }

    /// The `NativeStorage::resolve` finding, restated for a URL: every one of
    /// these must be refused, because the concatenation happens without further
    /// checks.
    #[test]
    fn keys_that_could_escape_the_asset_root_are_refused() {
        for input in [
            "",
            ".",
            "./",
            "..",
            "../escape",
            "a/../b",
            "a/..",
            "/etc/passwd",
            "//evil.example/x",
            "/",
            // URL metacharacters: query, fragment, scheme, encoded traversal,
            // credentials, separator confusion, whitespace, control bytes.
            "asset.png?x=1",
            "asset.png#frag",
            "http:/evil.example",
            "%2e%2e/up",
            "user@host/x",
            "a\\b",
            "a b.png",
            "a\nb",
            "a\tb",
            "a;b",
            "a&b",
            "a|b",
            "a*b",
            "a\u{7f}b",
            // Non-ASCII is refused rather than encoded: the shim would have to
            // percent-encode it, and then one side's idea of the key differs
            // from the other's.
            "café/x.png",
        ] {
            assert!(
                matches!(
                    canonical_key(Path::new(input)),
                    Err(StorageError::InvalidPath(_))
                ),
                "{input:?} should be refused"
            );
        }
    }

    /// `/` is the only separator, on every host.
    ///
    /// This used to walk `Path::components()`, whose separators are the
    /// *host's*: `a\b` was one component on Linux (refused, `\` is not a key
    /// byte) and two on Windows (accepted, and silently rewritten to `a/b`).
    /// The Windows CI leg is what failed, and a key that means one thing on the
    /// machine that built the manifest and another on the machine that reads it
    /// is the bug behind the failure.
    ///
    /// The refusals below hold everywhere now because nothing in
    /// [`canonical_key`] consults platform path semantics any more.
    #[test]
    fn a_key_is_split_on_slashes_and_nothing_else() {
        assert_eq!(canonical_key(Path::new("a/b/c.png")).unwrap(), "a/b/c.png");
        // A trailing slash names the same thing, as it did through `Path`.
        assert_eq!(canonical_key(Path::new("saves/")).unwrap(), "saves");
        assert_eq!(canonical_key(Path::new("./a/./b")).unwrap(), "a/b");

        for host_separator in ["a\\b", "a\\\\b", "C:\\x", "\\\\host\\share"] {
            assert!(
                canonical_key(Path::new(host_separator)).is_err(),
                "{host_separator:?} must be refused on every platform",
            );
        }
    }

    #[test]
    fn an_absurdly_long_key_is_refused() {
        let long = format!("a/{}", "b".repeat(MAX_KEY_LEN));
        assert!(canonical_key(Path::new(&long)).is_err());
        let just_fits = "c".repeat(MAX_KEY_LEN);
        assert_eq!(canonical_key(Path::new(&just_fits)).unwrap(), just_fits);
    }

    #[test]
    fn a_base_url_that_would_undo_the_key_rules_is_refused() {
        for base in ["assets/", "/assets/", "https://example.test/a/", "./"] {
            assert!(is_valid_base(base), "{base} should be accepted");
        }
        for base in [
            "",
            "assets",
            "assets/../",
            "assets/?v=1/",
            "assets/#/",
            "assets\\",
            "assets /",
            "ässets/",
        ] {
            assert!(!is_valid_base(base), "{base} should be refused");
        }
    }

    #[test]
    fn inbox_ids_are_never_zero_and_never_alias_a_live_slot() {
        let mut inbox = Inbox {
            next_id: u32::MAX - 1,
            ..Inbox::default()
        };
        let a = inbox.open("a".into());
        let b = inbox.open("b".into());
        let c = inbox.open("c".into());
        assert_eq!((a, b, c), (u32::MAX, 1, 2));
        assert_eq!(inbox.len(), 3);
        assert_eq!(inbox.find("a"), Some(a));
        assert_eq!(inbox.find("b"), Some(b));
        assert_eq!(inbox.find("nope"), None);
    }

    #[test]
    fn a_slot_hands_out_a_buffer_and_gives_back_what_was_written() {
        let mut inbox = Inbox::default();
        let id = inbox.open("k".into());
        let ptr = inbox.buffer(id, 4, 16).expect("within the limit");
        // SAFETY: `buffer` sized the slot's `Vec` to 4 bytes and returned its
        // pointer; nothing else borrows it until `commit` takes it, and the
        // write below stays inside those 4 bytes. This is exactly what the shim
        // does through the wasm heap.
        unsafe { std::ptr::copy_nonoverlapping(b"data".as_ptr(), ptr, 4) };
        let (name, bytes) = inbox.commit(id, 4).expect("open");
        assert_eq!(name, "k");
        assert_eq!(bytes, b"data");
        assert_eq!(inbox.len(), 0);
        assert_eq!(inbox.commit(id, 4), None, "the slot is closed");
    }

    #[test]
    fn a_shim_that_lies_about_the_length_gets_nothing() {
        let mut inbox = Inbox::default();
        let id = inbox.open("k".into());
        assert!(inbox.buffer(id, 8, 4).is_none(), "over the limit");
        assert!(inbox.buffer(999, 4, 8).is_none(), "no such slot");
        assert!(inbox.buffer(id, 4, 8).is_some());
        assert_eq!(inbox.commit(id, 5), None, "committed more than it asked");
        assert_eq!(inbox.len(), 1, "and the slot is still open");
        assert_eq!(inbox.discard(id).as_deref(), Some("k"));
        assert_eq!(inbox.len(), 0);
    }

    #[test]
    fn the_key_scratch_round_trips_a_key_and_refuses_a_bad_one() {
        let mut scratch = KeyScratch::default();
        assert_eq!(scratch.capacity(), MAX_KEY_LEN);
        let ptr = scratch.ptr();
        let key = b"saves/slot1.crb";
        // SAFETY: `ptr` is the scratch's own buffer, `MAX_KEY_LEN` bytes long
        // and larger than `key`; the scratch is not otherwise borrowed here.
        unsafe { std::ptr::copy_nonoverlapping(key.as_ptr(), ptr, key.len()) };
        assert_eq!(scratch.key(key.len()).unwrap(), "saves/slot1.crb");

        // A length past what was written is still in range, and re-reads stale
        // bytes rather than reading out of bounds — but only if they are a valid
        // key, which trailing zeros are not.
        assert!(scratch.key(key.len() + 1).is_err());
        assert!(scratch.key(MAX_KEY_LEN + 1).is_err(), "past the buffer");

        let bad = b"../escape";
        // SAFETY: as above; `bad` is shorter than the scratch.
        unsafe { std::ptr::copy_nonoverlapping(bad.as_ptr(), scratch.ptr(), bad.len()) };
        assert!(matches!(
            scratch.key(bad.len()),
            Err(StorageError::InvalidPath(_))
        ));
    }

    #[test]
    fn invalid_utf8_from_the_shim_is_refused() {
        let mut scratch = KeyScratch::default();
        let bytes = [0xffu8, 0xfe, 0x41];
        // SAFETY: as above; three bytes into a `MAX_KEY_LEN` buffer.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), scratch.ptr(), bytes.len()) };
        assert!(scratch.key(bytes.len()).is_err());
    }
}
