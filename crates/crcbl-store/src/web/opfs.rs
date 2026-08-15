//! [`OpfsStorage`] — saves and settings in the Origin Private File System.
//!
//! The design and the reasoning are in the [parent module's docs](super). This
//! module is the mechanism, what happens to
//! [`write_atomic`](crate::write_atomic)'s guarantee, and the exact JS↔wasm
//! contract.
//!
//! # What `write_atomic`'s guarantee becomes
//!
//! Natively, [`write_atomic`](crate::write_atomic) is: write a temp sibling,
//! `fsync` it, `fsync` the parent, `rename` over the target, `fsync` again. Two
//! things come out of that — **a reader never sees a torn value**, and **when
//! the call returns, the value is on the disk**.
//!
//! OPFS has neither `rename` nor `fsync`. So one of those survives and the other
//! does not, and this is the honest split:
//!
//! | Guarantee | In a browser | How |
//! | --- | --- | --- |
//! | A reader never sees a torn or half-written value | **Kept** | generation ping-pong + checksum, below |
//! | A reader never sees an *older* value after a newer one committed | **Kept**, within a session | reads come from the resident map, not from OPFS |
//! | The value is durable when `write` returns | **Not kept** | `write` returns when the record is *queued*; the shim writes it later |
//! | A failed write leaves nothing behind | **Kept** | a failed record is dropped; the previous generation is untouched |
//!
//! **The one a caller must not assume is durability at return.** A tab closed
//! between [`write`](OpfsStorage) and the shim's acknowledgement loses that
//! write, and nothing on the wasm side can prevent it — there is no synchronous
//! way to reach the disk from a page. What the engine *can* do is know: the
//! record is counted in [`OpfsStats::queued`] until the shim takes it and in
//! [`OpfsStats::in_flight`] until it acknowledges, so "is my save on disk yet"
//! is an answerable question. A game that wants a save to be durable before it
//! navigates away must wait for that count to reach zero, and a `beforeunload`
//! handler is the shim's place to drain it.
//!
//! ## Generation ping-pong (what replaces `rename`)
//!
//! Each logical key is stored as **two** OPFS files, `key~0` and `key~1`. A
//! write goes to the slot the previous generation is *not* in, so an interrupted
//! write can only damage the copy nobody is reading. Each file is a framed
//! record carrying a monotonic sequence number and a SHA-256 of its contents;
//! restore decodes both, discards anything that does not verify, and takes the
//! valid one with the higher sequence.
//!
//! That is the classic no-rename atomic-update scheme, and it holds for a crash
//! at any point: mid-write, the other slot still verifies and is one generation
//! behind — which is exactly what a native crash before the `rename` leaves.
//!
//! `~` is not a legal byte in a key (see [`canonical_key`]),
//! so no logical key can ever collide with a physical name, and the split is
//! unambiguous.
//!
//! Browsers do stage `createWritable()` output and apply it at `close()`, which
//! in practice gives per-file replacement; the specification promises nothing
//! about a crash, so the scheme above does not rely on it.
//!
//! ## The frame
//!
//! Little-endian, 52-byte header, and the same SHA-256 the save container uses
//! ([`crcbl_shaders::sha256`]):
//!
//! | Offset | Size | Field |
//! | --- | --- | --- |
//! | 0 | 4 | magic `CRWB` |
//! | 4 | 2 | format version (`1`) |
//! | 6 | 2 | flags, reserved, `0` |
//! | 8 | 8 | generation sequence, `u64` |
//! | 16 | 4 | payload length, `u32` |
//! | 20 | 32 | SHA-256 of bytes `0..20` followed by the payload |
//! | 52 | n | payload |
//!
//! The checksum covers the header fields as well as the body, so a corrupted
//! sequence number cannot promote a stale generation over a good one.
//!
//! # Exports
//!
//! Symbols are `#[unsafe(no_mangle)]` **only** on `wasm32`.
//!
//! ## Restore (JS → wasm), once at boot
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_web_opfs_key_ptr`](shim::__crcbl_web_opfs_key_ptr) | `() -> i32` | Address of the name scratch buffer, or `0`. Stable for the source's life. |
//! | [`__crcbl_web_opfs_key_capacity`](shim::__crcbl_web_opfs_key_capacity) | `() -> i32` | How many bytes may be written there ([`MAX_KEY_LEN`](super::MAX_KEY_LEN)). |
//! | [`__crcbl_web_opfs_restore_begin`](shim::__crcbl_web_opfs_restore_begin) | `(i32) -> i32` | Open a restore slot for the `len`-byte **physical** name in the scratch buffer. Slot id, or `0` if the name is not one this module wrote. |
//! | [`__crcbl_web_opfs_restore_buffer`](shim::__crcbl_web_opfs_restore_buffer) | `(i32, i32) -> i32` | Size that slot's buffer to `len` and return its address, or `0`. **May grow wasm memory.** |
//! | [`__crcbl_web_opfs_restore_commit`](shim::__crcbl_web_opfs_restore_commit) | `(i32, i32) -> i32` | The first `len` bytes are the file. `1`/`0`. A frame that does not verify is counted, not fatal. |
//! | [`__crcbl_web_opfs_restore_abort`](shim::__crcbl_web_opfs_restore_abort) | `(i32) -> i32` | The file could not be read. Closes the slot. `1`/`0`. |
//! | [`__crcbl_web_opfs_ready`](shim::__crcbl_web_opfs_ready) | `() -> i32` | Every file has been delivered. **Until this is called every read is [`StorageError::Pending`]** — that is what stops a game from seeing an empty store and writing a fresh one over a real save. |
//!
//! ## Flush (wasm → JS), every frame
//!
//! | Symbol | Signature (wasm) | Meaning |
//! | --- | --- | --- |
//! | [`__crcbl_web_opfs_take`](shim::__crcbl_web_opfs_take) | `() -> i32` | Dequeue the next record. Returns its sequence, or `0` when the queue is empty. |
//! | [`__crcbl_web_opfs_record_kind`](shim::__crcbl_web_opfs_record_kind) | `(i32) -> i32` | [`RECORD_WRITE`] (`1`) or [`RECORD_DELETE`] (`2`), `0` for no such record. |
//! | [`__crcbl_web_opfs_record_name_ptr`](shim::__crcbl_web_opfs_record_name_ptr) | `(i32) -> i32` | Address of the record's physical file name (UTF-8, not NUL-terminated), or `0`. |
//! | [`__crcbl_web_opfs_record_name_len`](shim::__crcbl_web_opfs_record_name_len) | `(i32) -> i32` | Its length in bytes. |
//! | [`__crcbl_web_opfs_record_data_ptr`](shim::__crcbl_web_opfs_record_data_ptr) | `(i32) -> i32` | Address of the framed bytes to write, or `0` (always `0` for a delete). |
//! | [`__crcbl_web_opfs_record_data_len`](shim::__crcbl_web_opfs_record_data_len) | `(i32) -> i32` | Their length. |
//! | [`__crcbl_web_opfs_ack`](shim::__crcbl_web_opfs_ack) | `(i32, i32) -> i32` | The record is done: `ok` non-zero for success. Frees it. `1`/`0`. |
//! | [`__crcbl_web_opfs_pending`](shim::__crcbl_web_opfs_pending) | `() -> i32` | Records queued and not yet taken. |
//! | [`__crcbl_web_opfs_inflight`](shim::__crcbl_web_opfs_inflight) | `() -> i32` | Records taken and not yet acknowledged. `pending + inflight == 0` is "everything is on disk". |
//! | [`__crcbl_web_opfs_environment`](shim::__crcbl_web_opfs_environment) | `(i32) -> i32` | Declare where the shim runs; see [`OpfsEnvironment`]. `1`/`0`. |
//! | [`__crcbl_web_opfs_physical_name`](shim::__crcbl_web_opfs_physical_name) | `(i32, i32) -> i32` | Turn the key in the scratch buffer into generation `slot`'s file name, **written back into the same buffer**. Returns the new length, or `0`. Nothing in the normal flow needs it — it is for a shim that wants to report per-key disk usage or sweep files this store no longer owns. |
//!
//! ## Buffer ownership
//!
//! Every buffer belongs to wasm, in both directions.
//!
//! - The **name scratch** is written by JS, read by wasm, and its address is
//!   stable for the source's life.
//! - A **restore buffer** is allocated by wasm and written by JS between
//!   `restore_buffer` and `restore_commit` for the same slot. `restore_buffer`
//!   can grow wasm memory, so a `Uint8Array` must be built after it and must not
//!   be cached across a call for another slot.
//! - A **record's name and data** are wasm's, and JS only reads them. They are
//!   valid from the `take` that produced the sequence until that sequence's
//!   `ack`, and **must be copied or consumed before the `ack`** — an
//!   `ArrayBuffer` view held past it points at freed wasm memory. `createWritable`
//!   is asynchronous, so the natural mistake is to `await write(view)` and then
//!   `ack`; copy into a fresh `Uint8Array` (or a `Blob`) first, or `ack` only
//!   after the write has fully resolved and never touch the view again.
//!
//! ## Call ordering
//!
//! 1. The app constructs an [`OpfsStorage`] and calls [`install`]. Until then
//!    every export answers `0`.
//! 2. `__crcbl_web_opfs_environment(flags)` — optional, but the first thing a
//!    debug HUD wants.
//! 3. For every file in the OPFS directory: `key_ptr` → write the name →
//!    `restore_begin` → read the file → `restore_buffer` → copy →
//!    `restore_commit`. A file that cannot be read gets `restore_abort`.
//! 4. `__crcbl_web_opfs_ready()` — **once**, after the last delivery. Reads work
//!    from here on.
//! 5. Boot the app.
//! 6. Every frame (or on a timer, or from `visibilitychange`/`beforeunload`):
//!    `take()` until it answers `0`; for each sequence, read `kind`, `name` and
//!    `data`, perform the OPFS operation, then `ack(seq, ok)`.
//!
//! **Ordering obligation.** Records for the same key must be applied in the
//! order they were taken. A shim that runs them concurrently can land an older
//! generation last, and the wasm side cannot detect it. Draining strictly
//! sequentially — `await` each record before taking the next — satisfies this
//! and is what the sample shim does; per-key serialisation is enough if
//! throughput ever matters.
//!
//! ## Failure behaviour
//!
//! | Situation | What the shim does | What the engine sees |
//! | --- | --- | --- |
//! | file unreadable during restore | `restore_abort(id)` | the key is simply absent, or falls back to its other generation |
//! | file present but corrupt | `restore_commit` anyway | the frame fails to verify; counted in [`OpfsStats::skipped`], other generation used |
//! | OPFS write throws (quota, permissions) | `ack(seq, 0)` | counted in [`OpfsStats::failed`]; the value stays resident, so the session is consistent |
//! | delete of a missing file | `ack(seq, 1)` — it is already gone | nothing |
//! | shim absent or stalled | — | the queue fills and `write` returns [`StorageError::LimitExceeded`] |
//!
//! **There is no automatic retry.** A failed record is dropped. The value is
//! still resident and still readable this session, and the next
//! [`write`](OpfsStorage) of that key queues a fresh generation — but a failure
//! that is never followed by another write is a save that stayed in memory. A
//! game that cares should watch [`OpfsStats::failed`] and tell the player, which
//! is the honest thing a page can do about a full disk.
//!
//! ## Worked shim
//!
//! ```js
//! const root = await navigator.storage.getDirectory();
//! const enc = new TextEncoder();
//!
//! ex.__crcbl_web_opfs_environment(
//!   (self.constructor.name !== 'Window' ? 1 : 0) |          // WORKER
//!   (typeof FileSystemFileHandle !== 'undefined' &&
//!    'createSyncAccessHandle' in FileSystemFileHandle.prototype ? 2 : 0) |
//!   (await navigator.storage.persisted() ? 4 : 0));
//!
//! // Restore, before the app boots.
//! for await (const [name, handle] of root.entries()) {
//!   if (handle.kind !== 'file') continue;
//!   const bytes = enc.encode(name);
//!   const kp = ex.__crcbl_web_opfs_key_ptr();
//!   if (bytes.length > ex.__crcbl_web_opfs_key_capacity()) continue;
//!   new Uint8Array(memory.buffer, kp, bytes.length).set(bytes);
//!   const id = ex.__crcbl_web_opfs_restore_begin(bytes.length);
//!   if (id === 0) continue;                       // not one of ours
//!   try {
//!     const file = new Uint8Array(await (await handle.getFile()).arrayBuffer());
//!     const p = ex.__crcbl_web_opfs_restore_buffer(id, file.length);
//!     if (p === 0) { ex.__crcbl_web_opfs_restore_abort(id); continue; }
//!     new Uint8Array(memory.buffer, p, file.length).set(file);
//!     ex.__crcbl_web_opfs_restore_commit(id, file.length);
//!   } catch (e) { ex.__crcbl_web_opfs_restore_abort(id); }
//! }
//! ex.__crcbl_web_opfs_ready();
//!
//! // Flush, strictly sequential. Call from rAF and from beforeunload.
//! let draining = false;
//! async function flush() {
//!   if (draining) return;
//!   draining = true;
//!   try {
//!     for (let seq; (seq = ex.__crcbl_web_opfs_take()) !== 0; ) {
//!       const kind = ex.__crcbl_web_opfs_record_kind(seq);
//!       const np = ex.__crcbl_web_opfs_record_name_ptr(seq);
//!       const nn = ex.__crcbl_web_opfs_record_name_len(seq);
//!       const name = new TextDecoder().decode(new Uint8Array(memory.buffer, np, nn));
//!       let ok = 0;
//!       try {
//!         if (kind === 1) {
//!           const dp = ex.__crcbl_web_opfs_record_data_ptr(seq);
//!           const dn = ex.__crcbl_web_opfs_record_data_len(seq);
//!           // Copy: the view dies at `ack`, and `close()` resolves after it.
//!           const body = new Uint8Array(memory.buffer, dp, dn).slice();
//!           const h = await root.getFileHandle(name, { create: true });
//!           const w = await h.createWritable();
//!           await w.write(body);
//!           await w.close();
//!         } else {
//!           await root.removeEntry(name).catch(() => {});
//!         }
//!         ok = 1;
//!       } catch (e) { ok = 0; }
//!       ex.__crcbl_web_opfs_ack(seq, ok);
//!     }
//!   } finally { draining = false; }
//! }
//! ```

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use super::{Inbox, KeyScratch, canonical_dir, canonical_key, direct_children};
use crate::{StorageError, StorageSource};

// ---------------------------------------------------------------------------
// Limits and constants
// ---------------------------------------------------------------------------

/// The largest single value [`OpfsStorage::write`](OpfsStorage) accepts, in
/// bytes.
///
/// Saves and settings, not assets. The whole value is held resident *and*
/// framed into a queued record, so it costs about twice its size in wasm memory
/// until the shim acknowledges it.
pub const MAX_VALUE_BYTES: usize = 4 << 20;

/// The most records that may be waiting for the shim at once.
pub const MAX_PENDING_RECORDS: usize = 64;

/// The most bytes those records may hold at once.
pub const MAX_QUEUED_BYTES: usize = 16 << 20;

/// [`__crcbl_web_opfs_record_kind`](shim::__crcbl_web_opfs_record_kind): write
/// the record's data to the record's file, creating it.
pub const RECORD_WRITE: u32 = 1;

/// [`__crcbl_web_opfs_record_kind`](shim::__crcbl_web_opfs_record_kind): remove
/// the record's file if it exists.
pub const RECORD_DELETE: u32 = 2;

/// Bytes of frame header before the payload.
const FRAME_HEADER: usize = 52;

/// Frame magic: **CR**ucible **W**eb **B**lob.
const FRAME_MAGIC: [u8; 4] = *b"CRWB";

/// Frame format version.
const FRAME_VERSION: u16 = 1;

// ---------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------

/// Frame `payload` as generation `seq`. See the module docs for the layout.
fn frame(seq: u64, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(FRAME_HEADER + payload.len());
    bytes.extend_from_slice(&FRAME_MAGIC);
    bytes.extend_from_slice(&FRAME_VERSION.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&seq.to_le_bytes());
    // A payload longer than `u32::MAX` cannot reach here: `write` refuses
    // anything over `MAX_VALUE_BYTES`.
    bytes.extend_from_slice(&(u32::try_from(payload.len()).unwrap_or(u32::MAX)).to_le_bytes());

    let mut digest_input = Vec::with_capacity(20 + payload.len());
    digest_input.extend_from_slice(&bytes[..20]);
    digest_input.extend_from_slice(payload);
    bytes.extend_from_slice(&crcbl_shaders::sha256::sha256(&digest_input));

    bytes.extend_from_slice(payload);
    bytes
}

/// Decode a frame, or `None` if it is not one, is truncated, or does not verify.
///
/// Every rejection is silent by design: these bytes come off a disk the user can
/// edit, and there is no repair to attempt — the other generation is the answer.
fn unframe(bytes: &[u8]) -> Option<(u64, &[u8])> {
    if bytes.len() < FRAME_HEADER || bytes[..4] != FRAME_MAGIC {
        return None;
    }
    let version = u16::from_le_bytes(bytes[4..6].try_into().ok()?);
    if version != FRAME_VERSION {
        return None;
    }
    let seq = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let len = u32::from_le_bytes(bytes[16..20].try_into().ok()?) as usize;
    let payload = bytes.get(FRAME_HEADER..FRAME_HEADER.checked_add(len)?)?;

    let mut digest_input = Vec::with_capacity(20 + payload.len());
    digest_input.extend_from_slice(&bytes[..20]);
    digest_input.extend_from_slice(payload);
    if crcbl_shaders::sha256::sha256(&digest_input) != bytes[20..52] {
        return None;
    }
    Some((seq, payload))
}

/// The OPFS file name for generation slot `slot` of `key`.
fn physical_name(key: &str, slot: u8) -> String {
    format!("{key}~{slot}")
}

/// Split a physical file name back into its key and slot.
///
/// `None` for any name this module did not write — which is how a shim that
/// hands over every file in the origin's directory, including another
/// library's, is safe to write.
fn split_physical(name: &str) -> Option<(String, u8)> {
    let (key, slot) = name.rsplit_once('~')?;
    let slot = match slot {
        "0" => 0,
        "1" => 1,
        _ => return None,
    };
    // The key half must satisfy every rule a key would have to satisfy to be
    // written in the first place: a delivered name is as untrusted as a fetch
    // manifest entry.
    Some((canonical_key(Path::new(key)).ok()?, slot))
}

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

/// What the shim says about where it is running.
///
/// The parent module's assumption — that the engine may be on the main thread
/// *or* in a Worker, because nothing here needs a synchronous access handle — is
/// made checkable by having the shim state the facts rather than by having this
/// crate guess them. Undeclared is a distinct answer from "main thread".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpfsEnvironment(u32);

impl OpfsEnvironment {
    /// The shim is running in a Worker (`DedicatedWorkerGlobalScope` or
    /// similar) rather than on a `Window`.
    pub const WORKER: u32 = 1 << 0;

    /// `FileSystemFileHandle.createSyncAccessHandle` is available — which
    /// implies a Worker, and which this module does not require.
    pub const SYNC_ACCESS_HANDLES: u32 = 1 << 1;

    /// `navigator.storage.persisted()` answered true: the origin's data is not
    /// eligible for eviction under storage pressure.
    pub const PERSISTED: u32 = 1 << 2;

    /// Set by the entry point, so an undeclared environment (all zero) is
    /// distinguishable from one declared as "none of the above".
    pub const DECLARED: u32 = 1 << 31;

    /// Whether the shim declared anything at all.
    pub fn is_declared(self) -> bool {
        self.0 & Self::DECLARED != 0
    }

    /// Whether the shim declared itself to be in a Worker.
    pub fn in_worker(self) -> bool {
        self.0 & Self::WORKER != 0
    }

    /// Whether synchronous access handles are available to it.
    pub fn has_sync_access_handles(self) -> bool {
        self.0 & Self::SYNC_ACCESS_HANDLES != 0
    }

    /// Whether the origin's storage is persistent.
    pub fn is_persisted(self) -> bool {
        self.0 & Self::PERSISTED != 0
    }

    /// The raw bits, for a debug overlay.
    pub fn bits(self) -> u32 {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// A snapshot of the store, for a debug overlay — and for the one question a
/// browser save has that a native one does not: *is it on the disk yet?*
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpfsStats {
    /// Whether the shim has finished restoring. Reads fail with
    /// [`StorageError::Pending`] until it has.
    pub restored: bool,
    /// Logical keys held.
    pub keys: usize,
    /// Bytes those keys occupy, payload only.
    pub bytes: usize,
    /// Records queued and not yet taken by the shim.
    pub queued: usize,
    /// Records taken and not yet acknowledged.
    pub in_flight: usize,
    /// Records the shim acknowledged as written.
    pub committed: u64,
    /// Records the shim acknowledged as failed. Non-zero means a save is in
    /// memory and not on the disk.
    pub failed: u64,
    /// Files delivered at restore that did not verify, and were ignored.
    pub skipped: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// One logical key's resident value and which generation it is.
#[derive(Debug)]
struct Entry {
    data: Vec<u8>,
    seq: u64,
    slot: u8,
}

/// One queued OPFS operation.
#[derive(Debug)]
struct Record {
    kind: u32,
    /// The physical file name, `key~slot`.
    name: String,
    /// The framed bytes for a write; empty for a delete.
    data: Vec<u8>,
}

#[derive(Debug, Default)]
struct Inner {
    resident: BTreeMap<String, Entry>,
    restored: bool,
    /// Restore deliveries in progress.
    inbox: Inbox,
    scratch: KeyScratch,
    /// Record sequences queued and not yet taken, in order.
    queue: VecDeque<u32>,
    /// Every unsettled record, queued or in flight.
    records: BTreeMap<u32, Record>,
    next_seq: u32,
    queued_bytes: usize,
    environment: OpfsEnvironment,
    committed: u64,
    failed: u64,
    skipped: u64,
}

impl Inner {
    /// Queue `record`, or refuse it because nothing is draining the queue.
    fn push(&mut self, path: &Path, record: Record) -> Result<(), StorageError> {
        if self.records.len() >= MAX_PENDING_RECORDS
            || self.queued_bytes + record.data.len() > MAX_QUEUED_BYTES
        {
            return Err(StorageError::LimitExceeded(path.to_path_buf()));
        }
        self.next_seq = self.next_seq.wrapping_add(1);
        if self.next_seq == 0 {
            self.next_seq = 1;
        }
        let seq = self.next_seq;
        self.queued_bytes += record.data.len();
        self.records.insert(seq, record);
        self.queue.push_back(seq);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// OpfsStorage
// ---------------------------------------------------------------------------

/// A [`StorageSource`] over the Origin Private File System: resident in wasm,
/// flushed by the shim.
///
/// Read the module docs before using it, and in particular the table of which
/// half of [`write_atomic`](crate::write_atomic)'s guarantee survives. In one
/// line: a reader never sees a torn value, and a write is *not* durable when
/// `write` returns.
///
/// ```
/// use crcbl_store::{StorageSource, StorageError};
/// use crcbl_store::web::OpfsStorage;
/// use std::path::Path;
///
/// let store = OpfsStorage::new();
/// let key = Path::new("high_score.bin");
///
/// // Nothing may be read until the shim has finished restoring: an empty store
/// // that looks empty is how a real save gets overwritten with a fresh one.
/// assert!(matches!(store.read(key), Err(StorageError::Pending(_))));
/// store.mark_restored();
/// assert!(matches!(store.read(key), Err(StorageError::NotFound(_))));
///
/// // A write is visible immediately and queued for the shim.
/// store.write(key, &1234u32.to_le_bytes()).unwrap();
/// assert_eq!(store.read(key).unwrap(), 1234u32.to_le_bytes());
/// assert_eq!(store.stats().queued, 1, "not on the disk yet");
/// ```
#[derive(Debug, Default)]
pub struct OpfsStorage {
    inner: RefCell<Inner>,
}

impl OpfsStorage {
    /// A store with nothing in it, waiting for the shim to restore.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare the restore phase complete.
    ///
    /// [`read`](StorageSource::read) and [`list`](StorageSource::list) answer
    /// [`StorageError::Pending`] until this is called — see the type's example
    /// for why that matters more than it looks. The shim calls it through
    /// [`__crcbl_web_opfs_ready`](shim::__crcbl_web_opfs_ready); a native test
    /// or a page with no persistence calls it directly.
    ///
    /// Idempotent, and one-way: there is no un-restoring.
    pub fn mark_restored(&self) {
        self.inner.borrow_mut().restored = true;
    }

    /// Whether the restore phase has completed.
    pub fn is_restored(&self) -> bool {
        self.inner.borrow().restored
    }

    /// What the shim declared about where it runs.
    pub fn environment(&self) -> OpfsEnvironment {
        self.inner.borrow().environment
    }

    /// A snapshot for a debug overlay.
    ///
    /// `queued + in_flight == 0` after a write means the shim has acknowledged
    /// it — the closest a browser gets to "the save is on the disk".
    pub fn stats(&self) -> OpfsStats {
        let inner = self.inner.borrow();
        OpfsStats {
            restored: inner.restored,
            keys: inner.resident.len(),
            bytes: inner.resident.values().map(|e| e.data.len()).sum(),
            queued: inner.queue.len(),
            in_flight: inner.records.len() - inner.queue.len(),
            committed: inner.committed,
            failed: inner.failed,
            skipped: inner.skipped,
        }
    }

    /// Adopt a restored file: the bytes of `name`, a physical `key~slot` name.
    ///
    /// The safe twin of the restore entry points, and what the tests drive.
    /// Returns whether the file was adopted; `false` means the name was not one
    /// this module writes, the frame did not verify, or a newer generation of
    /// the same key had already been delivered. A `false` for a bad frame is
    /// counted in [`OpfsStats::skipped`].
    pub fn restore(&self, name: &str, bytes: &[u8]) -> bool {
        let mut inner = self.inner.borrow_mut();
        inner.restore(name, bytes)
    }
}

impl Inner {
    /// See [`OpfsStorage::restore`].
    fn restore(&mut self, name: &str, bytes: &[u8]) -> bool {
        let Some((key, slot)) = split_physical(name) else {
            return false;
        };
        let Some((seq, payload)) = unframe(bytes) else {
            self.skipped = self.skipped.saturating_add(1);
            return false;
        };
        // Both generations are delivered, in whichever order the directory
        // iterated. The newer one wins; the older is left on the disk, which is
        // what makes the *next* write's ping-pong safe.
        if self.resident.get(&key).is_some_and(|old| old.seq >= seq) {
            return false;
        }
        self.resident.insert(
            key,
            Entry {
                data: payload.to_vec(),
                seq,
                slot,
            },
        );
        true
    }
}

impl StorageSource for OpfsStorage {
    /// Read `path` from the resident map.
    ///
    /// Never waits.
    ///
    /// # Errors
    ///
    /// - [`StorageError::Pending`] — the shim has not finished restoring.
    /// - [`StorageError::NotFound`] — restore finished and the key is not there.
    /// - [`StorageError::InvalidPath`] — the key escapes the storage root.
    fn read(&self, path: &Path) -> Result<Vec<u8>, StorageError> {
        let key = canonical_key(path)?;
        let inner = self.inner.borrow();
        if !inner.restored {
            return Err(StorageError::Pending(path.to_path_buf()));
        }
        inner
            .resident
            .get(&key)
            .map(|entry| entry.data.clone())
            .ok_or_else(|| StorageError::NotFound(path.to_path_buf()))
    }

    /// Store `data` at `path` and queue it for the disk.
    ///
    /// The value is resident — and readable — the instant this returns. It is
    /// **not** on the disk: see the module docs' table. The write goes to the
    /// generation slot the previous value is not in, so a crash during the
    /// shim's write cannot damage the value that is currently readable.
    ///
    /// Unlike the native backends this does not need the restore phase to have
    /// finished; a game that writes before it has read is doing something odd,
    /// but the write is still ordered correctly by generation.
    ///
    /// # Errors
    ///
    /// - [`StorageError::InvalidPath`] — the key escapes the storage root.
    /// - [`StorageError::LimitExceeded`] — `data` is over
    ///   [`MAX_VALUE_BYTES`], or the flush queue is full, which means the shim
    ///   is not draining it.
    fn write(&self, path: &Path, data: &[u8]) -> Result<(), StorageError> {
        let key = canonical_key(path)?;
        if data.len() > MAX_VALUE_BYTES {
            return Err(StorageError::LimitExceeded(path.to_path_buf()));
        }
        let mut inner = self.inner.borrow_mut();
        // Ping-pong: the new generation goes to *the other slot*, so the file
        // holding the readable value is never the one being overwritten. Which
        // slot that is comes from the entry rather than from the generation's
        // parity, because a value restored from disk may sit in either — a file
        // written by an older build, or one whose sibling failed to write, would
        // otherwise be the one clobbered.
        let (seq, slot) = match inner.resident.get(&key) {
            Some(entry) => (entry.seq.saturating_add(1), 1 - entry.slot),
            None => (1, 1),
        };
        let record = Record {
            kind: RECORD_WRITE,
            name: physical_name(&key, slot),
            data: frame(seq, data),
        };
        inner.push(path, record)?;
        inner.resident.insert(
            key,
            Entry {
                data: data.to_vec(),
                seq,
                slot,
            },
        );
        Ok(())
    }

    /// Remove `path`, queueing the removal of **both** generation files.
    ///
    /// # Errors
    ///
    /// - [`StorageError::Pending`] — the shim has not finished restoring, so
    ///   "not there" is not yet a fact.
    /// - [`StorageError::NotFound`] — the key is not resident.
    /// - [`StorageError::InvalidPath`], [`StorageError::LimitExceeded`] — as
    ///   [`write`](Self::write).
    fn delete(&self, path: &Path) -> Result<(), StorageError> {
        let key = canonical_key(path)?;
        let mut inner = self.inner.borrow_mut();
        if !inner.restored {
            return Err(StorageError::Pending(path.to_path_buf()));
        }
        if !inner.resident.contains_key(&key) {
            return Err(StorageError::NotFound(path.to_path_buf()));
        }
        // Room for *both* records before anything is mutated: a delete that
        // queued one generation's removal and refused the other would leave the
        // key deleted in memory and half-deleted on the disk.
        if inner.records.len() + 2 > MAX_PENDING_RECORDS {
            return Err(StorageError::LimitExceeded(path.to_path_buf()));
        }
        inner.resident.remove(&key);
        // Both slots: the one holding the current generation, and the one
        // holding the previous one, which is still on the disk by design.
        for slot in [0u8, 1] {
            inner.push(
                path,
                Record {
                    kind: RECORD_DELETE,
                    name: physical_name(&key, slot),
                    data: Vec::new(),
                },
            )?;
        }
        Ok(())
    }

    /// Whether `path` is resident.
    ///
    /// Always `false` before the restore phase completes, when the honest answer
    /// is "not known yet" and this signature has nowhere to put it. Use
    /// [`read`](Self::read), which distinguishes them.
    fn exists(&self, path: &Path) -> bool {
        let inner = self.inner.borrow();
        inner.restored && canonical_key(path).is_ok_and(|key| inner.resident.contains_key(&key))
    }

    /// The keys directly under `dir`, as full relative paths.
    ///
    /// The same contract as the native and in-memory backends: full paths,
    /// sorted, direct children only. Physical generation names never appear —
    /// `key~0` is an implementation detail of the disk, not a key.
    ///
    /// # Errors
    ///
    /// [`StorageError::Pending`] before the restore phase completes — a partial
    /// listing presented as a complete one is how a save-slot menu loses a slot.
    /// [`StorageError::InvalidPath`] for a directory that escapes the root.
    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, StorageError> {
        let key = canonical_dir(dir)?;
        let inner = self.inner.borrow();
        if !inner.restored {
            return Err(StorageError::Pending(dir.to_path_buf()));
        }
        Ok(direct_children(
            inner.resident.keys().map(String::as_str),
            &key,
        ))
    }
}

// ---------------------------------------------------------------------------
// The installed store
// ---------------------------------------------------------------------------

thread_local! {
    /// The store the `extern "C"` entry points reach. A [`Weak`], for the same
    /// reasons as [`fetch`](super::fetch)'s.
    static STORE: RefCell<Weak<OpfsStorage>> = const { RefCell::new(Weak::new()) };
}

/// Make `store` the one the `__crcbl_web_opfs_*` entry points reach.
///
/// Returns `false` if a live store is already installed — replacing one would
/// strand every record the shim is holding a sequence for. Call it before the
/// shim's first call; until then every export answers `0`.
pub fn install(store: &Rc<OpfsStorage>) -> bool {
    STORE.with(|slot| {
        let Ok(mut slot) = slot.try_borrow_mut() else {
            return false;
        };
        if slot.strong_count() > 0 {
            return false;
        }
        *slot = Rc::downgrade(store);
        true
    })
}

/// The installed store, if one is live on this thread.
///
/// The slot holds a [`Weak`], so this is `None` both before [`install`] and
/// after whoever owned the `Rc` dropped it — the same two states in which every
/// `__crcbl_web_opfs_*` export answers `0`. A caller that gets `Some` holds the
/// store alive for as long as it keeps the `Rc`.
///
/// This is what lets [`Backing::platform`](crate::record::Backing::platform)
/// answer for a browser without the game handing its store in: the entry points
/// and the record now read the same slot rather than two paths agreeing by
/// convention.
#[must_use]
pub fn installed() -> Option<Rc<OpfsStorage>> {
    STORE.with(|slot| slot.try_borrow().ok().and_then(|slot| slot.upgrade()))
}

/// Forget the installed store, returning whether there was a live one.
pub fn uninstall() -> bool {
    STORE.with(|slot| match slot.try_borrow_mut() {
        Ok(mut slot) => {
            let had = slot.strong_count() > 0;
            *slot = Weak::new();
            had
        }
        Err(_) => false,
    })
}

/// Whether a live store is installed on this thread.
pub fn is_installed() -> bool {
    STORE.with(|slot| slot.try_borrow().is_ok_and(|slot| slot.strong_count() > 0))
}

/// Run `f` against the installed store, or answer `absent`.
fn with_store<R>(absent: R, f: impl FnOnce(&OpfsStorage) -> R) -> R {
    STORE.with(|slot| match slot.try_borrow() {
        Ok(slot) => match slot.upgrade() {
            Some(store) => f(&store),
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
/// none dereferences a pointer the caller supplied.
pub mod shim {
    use super::{MAX_VALUE_BYTES, OpfsEnvironment, RECORD_WRITE, physical_name, with_store};

    /// The address of the name scratch buffer, or `0` when no store is
    /// installed. Stable for the store's life.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_key_ptr() -> *mut u8 {
        with_store(std::ptr::null_mut(), |store| {
            store.inner.borrow_mut().scratch.ptr()
        })
    }

    /// How many bytes may be written at that address.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_key_capacity() -> u32 {
        with_store(0, |store| {
            u32::try_from(store.inner.borrow().scratch.capacity()).unwrap_or(0)
        })
    }

    /// Declare where the shim is running; see [`OpfsEnvironment`].
    ///
    /// Optional, informational, and idempotent — a later call replaces the
    /// earlier one. Returns `1` if there was a store to tell.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_environment(flags: u32) -> u32 {
        with_store(0, |store| {
            store.inner.borrow_mut().environment =
                OpfsEnvironment(flags | OpfsEnvironment::DECLARED);
            1
        })
    }

    /// Open a restore slot for the `name_len`-byte physical name in the scratch
    /// buffer.
    ///
    /// Returns a slot id, or `0` when the name is not one this module writes —
    /// which is the expected answer for any other file in the origin's
    /// directory, and not an error.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_restore_begin(name_len: u32) -> u32 {
        with_store(0, |store| {
            let mut inner = store.inner.borrow_mut();
            let Ok(name) = inner.scratch.raw(name_len as usize) else {
                return 0;
            };
            let name = name.to_owned();
            if super::split_physical(&name).is_none() {
                return 0;
            }
            inner.inbox.open(name)
        })
    }

    /// Size restore slot `id`'s buffer to `len` bytes and return its address.
    ///
    /// `0` when the slot does not exist or `len` exceeds
    /// [`MAX_VALUE_BYTES`] plus a frame header — a file bigger than anything
    /// this module could have written is not one of ours however it is named.
    /// The shim must then call [`__crcbl_web_opfs_restore_abort`].
    ///
    /// **May grow wasm memory**; build the `Uint8Array` after the call.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_restore_buffer(id: u32, len: u32) -> *mut u8 {
        with_store(std::ptr::null_mut(), |store| {
            store
                .inner
                .borrow_mut()
                .inbox
                .buffer(id, len as usize, MAX_VALUE_BYTES + super::FRAME_HEADER)
                .unwrap_or(std::ptr::null_mut())
        })
    }

    /// Commit the first `len` bytes of restore slot `id` as the file's contents.
    ///
    /// `1` when the frame verified and was adopted; `0` when the slot did not
    /// exist, `len` was longer than the buffer handed out, the frame did not
    /// verify, or a newer generation of the same key had already arrived. Only
    /// the first of those leaves the slot open; the rest close it, because a
    /// file that does not verify has nothing more to say.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_restore_commit(id: u32, len: u32) -> u32 {
        with_store(0, |store| {
            let mut inner = store.inner.borrow_mut();
            let Some((name, bytes)) = inner.inbox.commit(id, len as usize) else {
                return 0;
            };
            u32::from(inner.restore(&name, &bytes))
        })
    }

    /// Close restore slot `id` without adopting anything — the file could not be
    /// read. `1` if the slot existed.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_restore_abort(id: u32) -> u32 {
        with_store(0, |store| {
            u32::from(store.inner.borrow_mut().inbox.discard(id).is_some())
        })
    }

    /// Declare the restore phase complete. `1` if there was a store.
    ///
    /// Until this is called every read is [`StorageError::Pending`](crate::StorageError::Pending).
    /// Call it exactly once, after the last delivery, and call it even when the
    /// directory was empty — an empty store that never becomes readable is a
    /// game stuck on its loading screen.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_ready() -> u32 {
        with_store(0, |store| {
            store.mark_restored();
            1
        })
    }

    /// Dequeue the next record, or `0` when the queue is empty.
    ///
    /// The returned value is the record's sequence, and the handle for every
    /// other call about it until [`__crcbl_web_opfs_ack`].
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_take() -> u32 {
        with_store(0, |store| {
            store.inner.borrow_mut().queue.pop_front().unwrap_or(0)
        })
    }

    /// [`RECORD_WRITE`],
    /// [`RECORD_DELETE`](super::RECORD_DELETE), or `0` for no such record.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_record_kind(seq: u32) -> u32 {
        with_store(0, |store| {
            store
                .inner
                .borrow()
                .records
                .get(&seq)
                .map_or(0, |record| record.kind)
        })
    }

    /// The address of record `seq`'s physical file name, or `0`.
    ///
    /// UTF-8, not NUL-terminated; pair it with
    /// [`__crcbl_web_opfs_record_name_len`]. Valid until the record is
    /// acknowledged.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_record_name_ptr(seq: u32) -> *const u8 {
        with_store(std::ptr::null(), |store| {
            store
                .inner
                .borrow()
                .records
                .get(&seq)
                .map_or(std::ptr::null(), |record| record.name.as_ptr())
        })
    }

    /// The length in bytes of record `seq`'s file name, or `0`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_record_name_len(seq: u32) -> u32 {
        with_store(0, |store| {
            store
                .inner
                .borrow()
                .records
                .get(&seq)
                .and_then(|record| u32::try_from(record.name.len()).ok())
                .unwrap_or(0)
        })
    }

    /// The address of record `seq`'s bytes, or `0` — which a delete always
    /// answers, because it has none.
    ///
    /// Valid until the record is acknowledged. **Copy them before the `ack`**;
    /// see the module docs' buffer-ownership section.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_record_data_ptr(seq: u32) -> *const u8 {
        with_store(std::ptr::null(), |store| {
            store
                .inner
                .borrow()
                .records
                .get(&seq)
                .filter(|record| record.kind == RECORD_WRITE && !record.data.is_empty())
                .map_or(std::ptr::null(), |record| record.data.as_ptr())
        })
    }

    /// The length in bytes of record `seq`'s data, or `0`.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_record_data_len(seq: u32) -> u32 {
        with_store(0, |store| {
            store
                .inner
                .borrow()
                .records
                .get(&seq)
                .and_then(|record| u32::try_from(record.data.len()).ok())
                .unwrap_or(0)
        })
    }

    /// Acknowledge record `seq`: `ok` non-zero for success.
    ///
    /// Frees the record — every pointer for it is dangling afterwards. `1` if
    /// the record existed. A failure is counted in
    /// [`OpfsStats::failed`](super::OpfsStats::failed) and **not retried**; the
    /// value stays resident, so the session remains consistent.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_ack(seq: u32, ok: u32) -> u32 {
        with_store(0, |store| {
            let mut inner = store.inner.borrow_mut();
            let Some(record) = inner.records.remove(&seq) else {
                return 0;
            };
            inner.queued_bytes = inner.queued_bytes.saturating_sub(record.data.len());
            // A record acknowledged without having been taken is a shim bug,
            // but dropping it from the queue is still the right repair.
            inner.queue.retain(|queued| *queued != seq);
            if ok == 0 {
                inner.failed = inner.failed.saturating_add(1);
            } else {
                inner.committed = inner.committed.saturating_add(1);
            }
            1
        })
    }

    /// How many records are queued and not yet taken.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_pending() -> u32 {
        with_store(0, |store| {
            u32::try_from(store.inner.borrow().queue.len()).unwrap_or(u32::MAX)
        })
    }

    /// How many records are taken and not yet acknowledged.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_inflight() -> u32 {
        with_store(0, |store| {
            let inner = store.inner.borrow();
            u32::try_from(inner.records.len() - inner.queue.len()).unwrap_or(u32::MAX)
        })
    }

    /// The physical file name for generation `slot` of the key currently in the
    /// scratch buffer, written back into the same buffer. `0`, or the new
    /// length.
    ///
    /// A convenience for a shim that wants to pre-create files or report disk
    /// usage per key; nothing in the normal flow needs it, because every record
    /// carries its own name.
    #[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
    pub extern "C" fn __crcbl_web_opfs_physical_name(key_len: u32, slot: u32) -> u32 {
        with_store(0, |store| {
            let mut inner = store.inner.borrow_mut();
            let Ok(key) = inner.scratch.key(key_len as usize) else {
                return 0;
            };
            if slot > 1 {
                return 0;
            }
            let name = physical_name(&key, u8::try_from(slot).unwrap_or(0));
            match inner.scratch.write_back(name.as_bytes()) {
                Some(len) => u32::try_from(len).unwrap_or(0),
                None => 0,
            }
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

    fn restored() -> OpfsStorage {
        let store = OpfsStorage::new();
        store.mark_restored();
        store
    }

    /// Drain the queue as a shim would, applying each record to `disk` and
    /// acknowledging it. Returns how many records were applied.
    fn drain(store: &OpfsStorage, disk: &mut BTreeMap<String, Vec<u8>>) -> usize {
        let mut applied = 0;
        loop {
            let seq = {
                let mut inner = store.inner.borrow_mut();
                match inner.queue.pop_front() {
                    Some(seq) => seq,
                    None => break,
                }
            };
            {
                let mut inner = store.inner.borrow_mut();
                let record = inner.records.remove(&seq).expect("a taken record");
                inner.queued_bytes = inner.queued_bytes.saturating_sub(record.data.len());
                inner.committed += 1;
                match record.kind {
                    RECORD_WRITE => {
                        disk.insert(record.name, record.data);
                    }
                    _ => {
                        disk.remove(&record.name);
                    }
                }
            }
            applied += 1;
        }
        applied
    }

    /// Restore a fresh store from `disk`, as the shim does at boot.
    fn reload(disk: &BTreeMap<String, Vec<u8>>) -> OpfsStorage {
        let store = OpfsStorage::new();
        for (name, bytes) in disk {
            store.restore(name, bytes);
        }
        store.mark_restored();
        store
    }

    // -- framing ------------------------------------------------------------

    #[test]
    fn a_frame_round_trips_its_generation_and_payload() {
        let bytes = frame(7, b"payload");
        assert_eq!(bytes.len(), FRAME_HEADER + 7);
        assert_eq!(unframe(&bytes), Some((7, &b"payload"[..])));

        let empty = frame(1, b"");
        assert_eq!(unframe(&empty), Some((1, &b""[..])));
    }

    #[test]
    fn a_damaged_frame_never_verifies() {
        let good = frame(42, b"the save");

        // Every single-byte corruption, anywhere in the frame, must be caught —
        // a torn write is exactly a frame that is partly one generation and
        // partly another.
        for i in 0..good.len() {
            let mut bad = good.clone();
            bad[i] ^= 0x01;
            assert_eq!(unframe(&bad), None, "byte {i} was not caught");
        }
        // Truncation at every length, which is what an interrupted write leaves.
        for len in 0..good.len() {
            assert_eq!(unframe(&good[..len]), None, "truncation to {len}");
        }
        // Trailing garbage does not change the payload.
        let mut extended = good.clone();
        extended.extend_from_slice(b"junk");
        assert_eq!(unframe(&extended), Some((42, &b"the save"[..])));
        // A length field claiming more than is there.
        let mut lying = good.clone();
        lying[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(unframe(&lying), None);
        // Something else entirely.
        assert_eq!(unframe(b"not a frame at all, but long enough to try"), None);
    }

    #[test]
    fn a_physical_name_round_trips_and_rejects_anything_else() {
        assert_eq!(physical_name("saves/slot1.crb", 1), "saves/slot1.crb~1");
        assert_eq!(
            split_physical("saves/slot1.crb~1"),
            Some(("saves/slot1.crb".to_owned(), 1))
        );
        assert_eq!(
            split_physical("./saves/slot1.crb~0"),
            Some(("saves/slot1.crb".to_owned(), 0))
        );
        for name in [
            "no-suffix",
            "two~slots~0", // `~` cannot appear in a key, so this is not ours
            "key~2",       // only two generations exist
            "key~",
            "~0",
            "../escape~0", // the containment rule applies to a restored name
            "/abs~1",
            "a b~0",
            "some-other-library-file.json",
        ] {
            assert_eq!(split_physical(name), None, "{name} was accepted");
        }
    }

    // -- the store ----------------------------------------------------------

    #[test]
    fn nothing_may_be_read_before_the_shim_finishes_restoring() {
        let store = OpfsStorage::new();
        let key = Path::new("high_score.bin");
        assert!(!store.is_restored());
        assert!(matches!(store.read(key), Err(StorageError::Pending(_))));
        assert!(matches!(
            store.list(Path::new(".")),
            Err(StorageError::Pending(_))
        ));
        assert!(matches!(store.delete(key), Err(StorageError::Pending(_))));
        assert!(!store.exists(key));

        store.mark_restored();
        assert!(store.is_restored());
        assert!(matches!(store.read(key), Err(StorageError::NotFound(_))));
        assert_eq!(store.list(Path::new(".")).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_write_is_readable_immediately_and_queued_for_the_disk() {
        let store = restored();
        let key = Path::new("high_score.bin");
        store.write(key, b"1234").unwrap();

        assert_eq!(store.read(key).unwrap(), b"1234");
        assert!(store.exists(key));
        let stats = store.stats();
        assert_eq!((stats.queued, stats.in_flight, stats.committed), (1, 0, 0));
        assert_eq!(stats.keys, 1);
    }

    /// The whole point: a value survives a reload, which is what the native
    /// build gets from the filesystem and the browser gets from this.
    #[test]
    fn a_value_survives_a_reload() {
        let mut disk = BTreeMap::new();
        let store = restored();
        store.write(Path::new("high_score.bin"), b"1234").unwrap();
        assert_eq!(drain(&store, &mut disk), 1);
        assert_eq!(store.stats().queued, 0, "flushed");

        let reloaded = reload(&disk);
        assert_eq!(reloaded.read(Path::new("high_score.bin")).unwrap(), b"1234");
    }

    /// Generation ping-pong: a write goes to the slot the readable value is not
    /// in, so a crash mid-write cannot damage it.
    #[test]
    fn successive_writes_alternate_slots_and_the_newest_wins_on_reload() {
        let mut disk = BTreeMap::new();
        let store = restored();
        let key = Path::new("save.bin");

        for generation in 1..=5u8 {
            store.write(key, &[generation]).unwrap();
            drain(&store, &mut disk);
        }
        // Both slots exist on the disk; one is the current generation and one is
        // the previous. That is the whole safety property.
        assert_eq!(disk.len(), 2);
        assert!(disk.contains_key("save.bin~0"));
        assert!(disk.contains_key("save.bin~1"));

        assert_eq!(reload(&disk).read(key).unwrap(), vec![5]);
        assert_eq!(unframe(&disk["save.bin~1"]).unwrap().0, 5);
        assert_eq!(unframe(&disk["save.bin~0"]).unwrap().0, 4);
    }

    /// The crash `write_atomic` exists for, in the shape a browser gets it: the
    /// write to one slot is interrupted, and the reader still sees a value.
    #[test]
    fn a_torn_write_leaves_the_previous_generation_readable() {
        let mut disk = BTreeMap::new();
        let store = restored();
        let key = Path::new("save.bin");

        store.write(key, b"first").unwrap();
        drain(&store, &mut disk);
        store.write(key, b"second").unwrap();
        drain(&store, &mut disk);

        // Generation 3 is written to slot 1, and the tab dies halfway through.
        store.write(key, b"third").unwrap();
        let (name, bytes) = {
            let mut inner = store.inner.borrow_mut();
            let seq = inner.queue.pop_front().expect("queued");
            let record = inner.records.remove(&seq).expect("taken");
            (record.name, record.data)
        };
        assert_eq!(name, "save.bin~1");
        disk.insert(name, bytes[..bytes.len() / 2].to_vec());

        // The half-written file does not verify, so the reader falls back to the
        // generation that is intact — exactly what a crash before `rename`
        // leaves natively.
        let reloaded = reload(&disk);
        assert_eq!(reloaded.read(key).unwrap(), b"second");
        assert_eq!(reloaded.stats().skipped, 1);
    }

    /// The slot a write goes to comes from the entry, not from the generation's
    /// parity: a value restored from the "wrong" slot must not be overwritten by
    /// the very next write.
    #[test]
    fn a_write_after_a_restore_never_clobbers_the_slot_it_restored_from() {
        for (slot, expected) in [(0u8, "save.bin~1"), (1, "save.bin~0")] {
            let mut disk = BTreeMap::new();
            // Only one generation on the disk, at an odd generation number, so
            // parity and slot disagree for one of the two cases.
            disk.insert(physical_name("save.bin", slot), frame(5, b"restored"));

            let store = reload(&disk);
            assert_eq!(store.read(Path::new("save.bin")).unwrap(), b"restored");

            store.write(Path::new("save.bin"), b"next").unwrap();
            let name = {
                let inner = store.inner.borrow();
                let seq = inner.queue.front().copied().expect("queued");
                inner.records[&seq].name.clone()
            };
            assert_eq!(name, expected);

            // And the restored generation is still readable after the crash
            // that interrupts that very write.
            drain(&store, &mut disk);
            assert_eq!(disk.len(), 2);
            assert_eq!(reload(&disk).read(Path::new("save.bin")).unwrap(), b"next");
        }
    }

    #[test]
    fn a_delete_removes_both_generations() {
        let mut disk = BTreeMap::new();
        let store = restored();
        let key = Path::new("save.bin");
        store.write(key, b"a").unwrap();
        drain(&store, &mut disk);
        store.write(key, b"b").unwrap();
        drain(&store, &mut disk);
        assert_eq!(disk.len(), 2);

        store.delete(key).unwrap();
        assert!(matches!(store.read(key), Err(StorageError::NotFound(_))));
        assert_eq!(store.stats().queued, 2, "one record per generation");
        drain(&store, &mut disk);
        assert!(disk.is_empty());
        assert!(matches!(
            reload(&disk).read(key),
            Err(StorageError::NotFound(_))
        ));

        // Deleting what is not there is `NotFound`, as it is natively.
        assert!(matches!(store.delete(key), Err(StorageError::NotFound(_))));
    }

    #[test]
    fn a_failed_flush_leaves_the_value_readable_and_counted() {
        let store = Rc::new(restored());
        let _guard = shim_guard();
        assert!(install(&store));
        let key = Path::new("save.bin");
        store.write(key, b"value").unwrap();

        let seq = shim::__crcbl_web_opfs_take();
        assert_ne!(seq, 0);
        assert_eq!(shim::__crcbl_web_opfs_ack(seq, 0), 1);

        let stats = store.stats();
        assert_eq!((stats.failed, stats.committed), (1, 0));
        assert_eq!((stats.queued, stats.in_flight), (0, 0));
        // The session is still consistent — the value is simply not on the disk,
        // which is the one thing the engine can be told and cannot fix.
        assert_eq!(store.read(key).unwrap(), b"value");
    }

    #[test]
    fn the_queue_is_bounded_so_a_stalled_shim_is_an_error_not_a_leak() {
        let store = restored();
        for i in 0..MAX_PENDING_RECORDS {
            store.write(Path::new(&format!("k{i}.bin")), b"x").unwrap();
        }
        assert!(matches!(
            store.write(Path::new("one-too-many.bin"), b"x"),
            Err(StorageError::LimitExceeded(_))
        ));
        assert_eq!(store.stats().queued, MAX_PENDING_RECORDS);

        // And the refused write did not become resident: a `write` that reports
        // an error must not leave a value that a later `read` presents as saved.
        assert!(matches!(
            store.read(Path::new("one-too-many.bin")),
            Err(StorageError::NotFound(_))
        ));
    }

    #[test]
    fn an_oversized_value_is_refused() {
        let store = restored();
        assert!(matches!(
            store.write(Path::new("big.bin"), &vec![0; MAX_VALUE_BYTES + 1]),
            Err(StorageError::LimitExceeded(_))
        ));
    }

    #[test]
    fn keys_that_escape_the_storage_root_are_refused_everywhere() {
        let store = restored();
        for escape in ["../escape", "/etc/passwd", "a/../b", "key~0", "a b"] {
            let path = Path::new(escape);
            assert!(matches!(
                store.write(path, b"x"),
                Err(StorageError::InvalidPath(_))
            ));
            assert!(matches!(
                store.read(path),
                Err(StorageError::InvalidPath(_))
            ));
            assert!(matches!(
                store.delete(path),
                Err(StorageError::InvalidPath(_))
            ));
            assert!(!store.exists(path));
        }
        assert_eq!(store.stats().keys, 0);
    }

    /// The `list` contract, checked against the backend that defines it.
    #[test]
    fn list_matches_the_memory_backend_entry_for_entry() {
        let store = restored();
        let memory = MemoryStorage::new();
        for key in [
            "saves/a.crb",
            "saves/b.crb",
            "settings.toml",
            "saves/x/y.crb",
        ] {
            store.write(Path::new(key), b"x").unwrap();
            memory.write(Path::new(key), b"x").unwrap();
        }
        for dir in ["saves", ".", "", "saves/x", "nothing"] {
            assert_eq!(
                store.list(Path::new(dir)).unwrap(),
                memory.list(Path::new(dir)).unwrap(),
                "list({dir:?})"
            );
        }
        // Physical names are never keys.
        assert!(
            !store
                .list(Path::new("saves"))
                .unwrap()
                .iter()
                .any(|entry| entry.to_string_lossy().contains('~'))
        );
    }

    #[test]
    fn a_file_from_another_library_is_ignored_rather_than_adopted() {
        let store = OpfsStorage::new();
        assert!(!store.restore("some-other-library.json", b"{}"));
        // A name that is ours but a body that is not: counted, not adopted.
        assert!(!store.restore("save.bin~0", b"garbage"));
        store.mark_restored();
        assert_eq!(store.stats().keys, 0);
        assert_eq!(store.stats().skipped, 1);
    }

    // -- the thread-local and the entry points ------------------------------

    static SHIM: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn shim_guard() -> std::sync::MutexGuard<'static, ()> {
        let guard = SHIM.lock().unwrap_or_else(|e| e.into_inner());
        uninstall();
        guard
    }

    /// Write `name` into the scratch buffer exactly as the shim does.
    fn write_name(name: &str) -> u32 {
        let ptr = shim::__crcbl_web_opfs_key_ptr();
        assert!(!ptr.is_null(), "no store installed");
        assert!(name.len() <= shim::__crcbl_web_opfs_key_capacity() as usize);
        // SAFETY: `ptr` is the installed store's name scratch, whose capacity
        // was just checked against `name.len()`; nothing else borrows it until
        // the entry point that reads it back. This is the shim's
        // `new Uint8Array(memory.buffer, ptr, n).set(bytes)`.
        unsafe { std::ptr::copy_nonoverlapping(name.as_ptr(), ptr, name.len()) };
        u32::try_from(name.len()).expect("a short name")
    }

    #[test]
    fn the_entry_points_answer_zero_until_a_store_is_installed() {
        let _guard = shim_guard();
        assert!(!is_installed());
        assert!(shim::__crcbl_web_opfs_key_ptr().is_null());
        assert_eq!(shim::__crcbl_web_opfs_key_capacity(), 0);
        assert_eq!(shim::__crcbl_web_opfs_environment(1), 0);
        assert_eq!(shim::__crcbl_web_opfs_restore_begin(4), 0);
        assert!(shim::__crcbl_web_opfs_restore_buffer(1, 4).is_null());
        assert_eq!(shim::__crcbl_web_opfs_restore_commit(1, 4), 0);
        assert_eq!(shim::__crcbl_web_opfs_restore_abort(1), 0);
        assert_eq!(shim::__crcbl_web_opfs_ready(), 0);
        assert_eq!(shim::__crcbl_web_opfs_take(), 0);
        assert_eq!(shim::__crcbl_web_opfs_record_kind(1), 0);
        assert!(shim::__crcbl_web_opfs_record_name_ptr(1).is_null());
        assert_eq!(shim::__crcbl_web_opfs_record_name_len(1), 0);
        assert!(shim::__crcbl_web_opfs_record_data_ptr(1).is_null());
        assert_eq!(shim::__crcbl_web_opfs_record_data_len(1), 0);
        assert_eq!(shim::__crcbl_web_opfs_ack(1, 1), 0);
        assert_eq!(shim::__crcbl_web_opfs_pending(), 0);
        assert_eq!(shim::__crcbl_web_opfs_inflight(), 0);
        assert_eq!(shim::__crcbl_web_opfs_physical_name(3, 0), 0);
    }

    /// The boot sequence and the flush loop, end to end through the ABI, with
    /// the reload the whole slice exists for.
    #[test]
    fn the_shims_whole_sequence_persists_a_high_score_across_a_reload() {
        let _guard = shim_guard();
        let store = Rc::new(OpfsStorage::new());
        assert!(install(&store));
        assert!(!install(&store), "a second install must not replace it");

        // The shim declares where it is. Nothing depends on the answer — the
        // point is that the assumption is on the record.
        assert_eq!(
            shim::__crcbl_web_opfs_environment(OpfsEnvironment::WORKER),
            1
        );
        assert!(store.environment().is_declared());
        assert!(store.environment().in_worker());
        assert!(!store.environment().has_sync_access_handles());

        // An empty OPFS directory: nothing to restore, but `ready` still fires.
        assert_eq!(shim::__crcbl_web_opfs_ready(), 1);
        assert!(store.is_restored());

        let key = Path::new("high_score.bin");
        store.write(key, &4321u32.to_le_bytes()).unwrap();
        assert_eq!(shim::__crcbl_web_opfs_pending(), 1);

        // The flush loop.
        let seq = shim::__crcbl_web_opfs_take();
        assert_ne!(seq, 0);
        assert_eq!(shim::__crcbl_web_opfs_pending(), 0);
        assert_eq!(shim::__crcbl_web_opfs_inflight(), 1);
        assert_eq!(shim::__crcbl_web_opfs_record_kind(seq), RECORD_WRITE);

        let name_ptr = shim::__crcbl_web_opfs_record_name_ptr(seq);
        let name_len = shim::__crcbl_web_opfs_record_name_len(seq) as usize;
        let data_ptr = shim::__crcbl_web_opfs_record_data_ptr(seq);
        let data_len = shim::__crcbl_web_opfs_record_data_len(seq) as usize;
        // SAFETY: both pairs describe record `seq`'s own buffers, which live
        // until the `ack` below; the copies are taken first, exactly as the
        // module docs require of the shim.
        let (name, body) = unsafe {
            (
                String::from_utf8(std::slice::from_raw_parts(name_ptr, name_len).to_vec())
                    .expect("UTF-8"),
                std::slice::from_raw_parts(data_ptr, data_len).to_vec(),
            )
        };
        assert_eq!(name, "high_score.bin~1");
        assert_eq!(shim::__crcbl_web_opfs_ack(seq, 1), 1);
        assert_eq!(shim::__crcbl_web_opfs_ack(seq, 1), 0, "already settled");
        assert_eq!(shim::__crcbl_web_opfs_inflight(), 0);
        assert_eq!(store.stats().committed, 1);

        // The reload: a fresh instance, the same file delivered back.
        drop(store);
        uninstall();
        let store = Rc::new(OpfsStorage::new());
        assert!(install(&store));

        let len = write_name(&name);
        let id = shim::__crcbl_web_opfs_restore_begin(len);
        assert_ne!(id, 0);
        let ptr = shim::__crcbl_web_opfs_restore_buffer(id, data_len as u32);
        assert!(!ptr.is_null());
        // SAFETY: `restore_buffer` sized slot `id`'s buffer to `body.len()` and
        // returned its address; nothing else touches it before the commit.
        unsafe { std::ptr::copy_nonoverlapping(body.as_ptr(), ptr, body.len()) };
        assert_eq!(
            shim::__crcbl_web_opfs_restore_commit(id, data_len as u32),
            1
        );
        assert_eq!(shim::__crcbl_web_opfs_ready(), 1);

        assert_eq!(store.read(key).unwrap(), 4321u32.to_le_bytes());
    }

    #[test]
    fn a_shim_that_misuses_the_restore_path_gets_zero_and_changes_nothing() {
        let _guard = shim_guard();
        let store = Rc::new(OpfsStorage::new());
        assert!(install(&store));

        // Names that are not ours, including one that tries to climb out.
        for name in ["other.json", "../escape~0", "key~7"] {
            let len = write_name(name);
            assert_eq!(shim::__crcbl_web_opfs_restore_begin(len), 0, "{name}");
        }

        let len = write_name("save.bin~0");
        let id = shim::__crcbl_web_opfs_restore_begin(len);
        assert_ne!(id, 0);
        assert!(
            shim::__crcbl_web_opfs_restore_buffer(id, u32::MAX).is_null(),
            "an absurd length must not allocate"
        );
        assert!(!shim::__crcbl_web_opfs_restore_buffer(id, 8).is_null());
        assert_eq!(
            shim::__crcbl_web_opfs_restore_commit(id, 9),
            0,
            "committed more than it asked for"
        );
        // The slot is still open, so the shim can abort it properly.
        assert_eq!(shim::__crcbl_web_opfs_restore_abort(id), 1);
        assert_eq!(shim::__crcbl_web_opfs_restore_abort(id), 0);

        shim::__crcbl_web_opfs_ready();
        assert_eq!(store.stats().keys, 0);
    }

    #[test]
    fn the_physical_name_helper_answers_through_the_scratch_buffer() {
        let _guard = shim_guard();
        let store = Rc::new(restored());
        assert!(install(&store));

        let len = write_name("saves/slot1.crb");
        let out = shim::__crcbl_web_opfs_physical_name(len, 1);
        assert_eq!(out as usize, "saves/slot1.crb~1".len());
        let name = store
            .inner
            .borrow()
            .scratch
            .raw(out as usize)
            .unwrap()
            .to_owned();
        assert_eq!(name, "saves/slot1.crb~1");

        // Only two generations, and only valid keys.
        let len = write_name("saves/slot1.crb");
        assert_eq!(shim::__crcbl_web_opfs_physical_name(len, 2), 0);
        let len = write_name("../escape");
        assert_eq!(shim::__crcbl_web_opfs_physical_name(len, 0), 0);
    }
}
