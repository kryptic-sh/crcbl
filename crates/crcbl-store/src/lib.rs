//! Persistence: saves, settings, profiles.
//!
//! This crate provides the storage abstraction and concrete implementations for
//! engine and game persistence. It is organised in layers:
//!
//! - [`StorageSource`] — the abstract seam (sync for native, see notes below)
//! - [`NativeStorage`] — platform-directory-backed storage
//! - [`write_atomic`] — write-then-rename for crash-safe file writes
//! - Settings (TOML layers, typed access) — module `settings`
//! - Saves (binary save/load container) — module `save`
//! - Browser storage ([`FetchSource`](web::FetchSource) for assets,
//!   [`OpfsStorage`](web::OpfsStorage) for saves) — module [`web`]
//!
//! # Wasm / async design note
//!
//! The trait is a **sync** `fn` interface, and the browser has neither blocking
//! file IO nor blocking network IO. Module [`web`] resolves that without an
//! async runtime and without a blocking-executor hack: both browser backends are
//! **resident caches with an out-of-band fill**, so every `StorageSource` call
//! is an ordinary map lookup and the asynchrony lives in a request/deliver queue
//! the JS shim drains. A read of something not yet resident is
//! [`StorageError::Pending`] — a state the caller polls, not a stall. See the
//! [`web`] module docs for the whole contract, including what
//! [`write_atomic`]'s guarantee becomes when there is no `rename`.

pub mod crash_ring;
pub mod record;
pub mod replay;
pub mod save;
pub mod settings;
pub mod web;

use std::io;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

// ── Error type ─────────────────────────────────────────────────────────────

/// Errors produced by storage operations.
///
/// `#[non_exhaustive]`: the browser backends in [`web`] added three variants
/// after the native ones shipped, and an IndexedDB fallback is still to come
/// (`docs/plan/14-persistence.md`). Match with a `_` arm.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StorageError {
    /// An underlying I/O error.
    #[error("I/O error: {0}")]
    Io(io::Error),

    /// The path does not exist.
    #[error("path not found: {0}")]
    NotFound(PathBuf),

    /// Permission denied.
    #[error("permission denied: {0}")]
    PermissionDenied(PathBuf),

    /// The path is a directory when a file was expected (or vice versa).
    #[error("unexpected type at path: {0}")]
    WrongType(PathBuf),

    /// The path escapes the storage root, or is absolute when it must be
    /// relative to it.
    #[error("path escapes the storage root: {0}")]
    InvalidPath(PathBuf),

    /// The backend has been asked for the data but does not have it yet.
    ///
    /// Only the browser backends produce this: a `fetch()` is in flight, or the
    /// shim has not finished restoring the Origin Private File System into the
    /// resident map. It is a *state*, not a failure — poll again next frame. See
    /// [`web`].
    #[error("not resident yet, still pending: {0}")]
    Pending(PathBuf),

    /// The operation does not exist on this backend at all.
    ///
    /// [`FetchSource`](web::FetchSource) is read-only: HTTP `GET` has no write
    /// or delete, and pretending otherwise by succeeding silently would lose a
    /// save.
    #[error("unsupported operation: {0}")]
    Unsupported(&'static str),

    /// A size or backlog limit refused the write.
    ///
    /// The browser's storage quota, a single value larger than the backend
    /// accepts, or — the case worth naming — the bounded queue of writes waiting
    /// for a JS shim that is not draining it. See
    /// [`OpfsStorage`](web::OpfsStorage).
    #[error("storage limit reached: {0}")]
    LimitExceeded(PathBuf),

    /// A generic application-level error.
    #[error("{0}")]
    Other(String),
}

impl StorageError {
    /// Classify an [`io::Error`] that happened while touching `path`.
    ///
    /// The path must be supplied by the caller: `io::Error` does not carry it,
    /// and the `Display` text is an errno message, not a path.
    fn from_io(path: &Path, e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound(path.to_path_buf()),
            io::ErrorKind::PermissionDenied => StorageError::PermissionDenied(path.to_path_buf()),
            _ => StorageError::Io(e),
        }
    }
}

impl From<io::Error> for StorageError {
    /// Path-less conversion. Prefer `StorageError::from_io` wherever the path
    /// is known — this variant cannot classify not-found or permission errors
    /// without inventing a path for them.
    fn from(e: io::Error) -> Self {
        StorageError::Io(e)
    }
}

// ── StorageSource trait ────────────────────────────────────────────────────

/// A storage backend that maps string keys (paths) to binary blobs.
///
/// Implementations provide the actual filesystem, in-memory, or network-backed
/// storage. All paths are relative to the storage root — the implementation
/// decides where on disk (or elsewhere) data lives.
///
/// This trait is deliberately **sync** for now (see the module-level note about
/// async / wasm). Methods return [`Result`] so every failure mode — disk full,
/// permission denied, quota exceeded — is surfaced rather than panicked on.
pub trait StorageSource: Send + std::fmt::Debug {
    /// Read the full contents of a file at `path`.
    fn read(&self, path: &Path) -> Result<Vec<u8>, StorageError>;

    /// Write `data` to `path`, replacing any existing content atomically if the
    /// backend supports it.
    fn write(&self, path: &Path, data: &[u8]) -> Result<(), StorageError>;

    /// Delete the file at `path`.
    fn delete(&self, path: &Path) -> Result<(), StorageError>;

    /// Return `true` if `path` exists as a file or directory.
    fn exists(&self, path: &Path) -> bool;

    /// List the entries in a directory `path`, returning their full (relative)
    /// paths within the storage root.
    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, StorageError>;
}

// ── NativeStorage ──────────────────────────────────────────────────────────

/// A [`StorageSource`] backed by the platform's standard directories.
///
/// | Use          | Platform directory            | Example                         |
/// | ------------ | ----------------------------- | ------------------------------- |
/// | Config       | [`dirs::config_dir`]          | `~/.config/<app_name>/`         |
/// | Data         | [`dirs::data_dir`]            | `~/.local/share/<app_name>/`    |
///
/// `app_name` is set at construction and becomes a subdirectory under each
/// platform dir, so settings and saves for different games never collide.
#[derive(Debug)]
pub struct NativeStorage {
    root: PathBuf,
}

impl NativeStorage {
    /// Where `app_name`'s config directory **would** be, creating nothing.
    ///
    /// [`config`](Self::config) is the same answer plus the `mkdir`, and a
    /// caller that is only going to *read* wants this one: a start-up that
    /// looks for a settings file the player never wrote must not leave a
    /// directory behind on every machine that has never had one. Split out so
    /// that the two callers share the one statement of where the platform keeps
    /// this — a second `dirs::config_dir().join(..)` is a second place for the
    /// answer to drift.
    ///
    /// `None` where the platform will not name a config directory at all, which
    /// includes `wasm32`: [`dirs`] has no browser to ask.
    #[must_use]
    pub fn config_root(app_name: &str) -> Option<PathBuf> {
        Some(dirs::config_dir()?.join(app_name))
    }

    /// Create a storage root under a platform-standard config directory.
    ///
    /// `app_name` is used as a subdirectory name, e.g. `~/.config/<app_name>/`.
    /// The directory is created (including parents) on construction.
    pub fn config(app_name: &str) -> Result<Self, StorageError> {
        let root = Self::config_root(app_name)
            .ok_or_else(|| StorageError::Other("no config directory found".into()))?;
        std::fs::create_dir_all(&root).map_err(|e| StorageError::from_io(&root, e))?;
        Ok(Self { root })
    }

    /// Create a storage root under a platform-standard data directory
    /// (preferred for saves and large data).
    pub fn data(app_name: &str) -> Result<Self, StorageError> {
        let base = dirs::data_dir()
            .ok_or_else(|| StorageError::Other("no data directory found".into()))?;
        let root = base.join(app_name);
        std::fs::create_dir_all(&root).map_err(|e| StorageError::from_io(&root, e))?;
        Ok(Self { root })
    }

    /// Create a storage root at an explicit path (useful for tests and
    /// dedicated server deployments).
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    /// The absolute root path of this storage.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a storage-relative path to an absolute one.
    ///
    /// The argument must stay inside the root: absolute paths, Windows path
    /// prefixes, and `..` components are rejected with
    /// [`StorageError::InvalidPath`]. Without this a caller-supplied `../..`
    /// (or `/etc`, which `Path::join` would silently substitute for the whole
    /// root) would let [`delete`](StorageSource::delete) `remove_dir_all` an
    /// arbitrary directory.
    fn resolve(&self, path: &Path) -> Result<PathBuf, StorageError> {
        if !is_contained(path) {
            return Err(StorageError::InvalidPath(path.to_path_buf()));
        }
        Ok(self.root.join(path))
    }
}

/// Whether `path` is relative and stays within its root — no prefix, no root
/// directory, no `..`. Bare `.` components are allowed and ignored.
fn is_contained(path: &Path) -> bool {
    path.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Strip `.` components so `list(".")` and `list("")` produce the same
/// key prefix that [`MemoryStorage`] stores its keys under.
fn normalise_dir(dir: &Path) -> PathBuf {
    dir.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect()
}

impl StorageSource for NativeStorage {
    fn read(&self, path: &Path) -> Result<Vec<u8>, StorageError> {
        let abs = self.resolve(path)?;
        std::fs::read(&abs).map_err(|e| StorageError::from_io(&abs, e))
    }

    fn write(&self, path: &Path, data: &[u8]) -> Result<(), StorageError> {
        let abs = self.resolve(path)?;
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StorageError::from_io(parent, e))?;
        }
        write_atomic(&abs, data)
    }

    fn delete(&self, path: &Path) -> Result<(), StorageError> {
        let abs = self.resolve(path)?;
        if abs.is_dir() {
            std::fs::remove_dir_all(&abs).map_err(|e| StorageError::from_io(&abs, e))?;
        } else {
            std::fs::remove_file(&abs).map_err(|e| StorageError::from_io(&abs, e))?;
        }
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.resolve(path).is_ok_and(|abs| abs.exists())
    }

    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, StorageError> {
        let abs = self.resolve(dir)?;
        // The trait promises full storage-relative paths, not bare file names.
        let prefix = normalise_dir(dir);
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&abs).map_err(|e| StorageError::from_io(&abs, e))? {
            let entry = entry.map_err(|e| StorageError::from_io(&abs, e))?;
            entries.push(prefix.join(entry.file_name()));
        }
        entries.sort();
        Ok(entries)
    }
}

// ── Atomic write ───────────────────────────────────────────────────────────

/// Write `data` to `path` atomically: write a temporary sibling file, fsync
/// both the file and its parent directory, then rename over the target.
///
/// This is the safe write pattern that the project uses everywhere — saves,
/// settings, golden-image output. A crash during the write leaves the original
/// file intact, and a crash after the rename leaves a complete file at the
/// target (the rename is atomic on the same filesystem).
///
/// Parent directories are created if they do not exist.
///
/// The temporary file is created with `create_new` (`O_EXCL`), so a pre-planted
/// symlink or file at the guessable temporary path makes the write fail instead
/// of being followed. It is removed again on every failure path, so a failed
/// write leaves nothing behind.
///
/// # Not in a browser
///
/// This function is `std::fs` all the way down. It compiles for
/// `wasm32-unknown-unknown` and fails at runtime on the first call, because that
/// target's `std::fs` is a stub with no filesystem behind it — and the Origin
/// Private File System has no `rename` to build this shape on even when reached
/// through JS. The browser's persistence path is
/// [`OpfsStorage`](web::OpfsStorage), whose docs state exactly which half of the
/// guarantee above survives (torn reads: prevented; durability at return:
/// **not** provided) rather than leaving a caller to assume this one's.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), StorageError> {
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| StorageError::from_io(parent, e))?;

    let tmp_name = {
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let ext = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        // A random-ish suffix so concurrent writers (e.g. two autosave timers)
        // do not collide. This is not a cryptographic concern — just collision
        // avoidance.
        let suffix: u64 = rand_suffix();
        parent.join(format!(".{stem}.{suffix:x}{ext}.tmp"))
    };

    // The handle's whole life is this `match`: the file must be closed before
    // the rename below, because Windows refuses to rename an open file. An
    // explicit `drop(f)` used to say so, until `wasm32-unknown-unknown` — whose
    // `std::fs::File` is an unsupported stub with no `Drop` impl at all — turned
    // that line into a `clippy::drop_non_drop` error in the wasm32 CI job. A
    // scope closes it on every target and cannot be read as a no-op.
    let written = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_name)
    {
        Ok(mut f) => f.write_all(data).and_then(|()| f.sync_all()),
        Err(e) => return Err(StorageError::from_io(&tmp_name, e)),
    };
    if let Err(e) = written {
        std::fs::remove_file(&tmp_name).ok();
        return Err(StorageError::from_io(&tmp_name, e));
    }

    // fsync the parent directory so the link is durable.
    if let Ok(parent_fd) = std::fs::File::open(parent) {
        parent_fd.sync_all().ok();
    }

    if let Err(e) = std::fs::rename(&tmp_name, path) {
        std::fs::remove_file(&tmp_name).ok();
        return Err(StorageError::from_io(path, e));
    }

    // fsync again after rename so the directory entry is durable on systems
    // that require it.
    if let Ok(parent_fd) = std::fs::File::open(parent) {
        parent_fd.sync_all().ok();
    }

    Ok(())
}

/// A cheap random u64 for temporary-file suffix generation.
fn rand_suffix() -> u64 {
    // Simple Lehmer generator seeded from system time and process id.
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let pid = std::process::id() as u64;
    let mut state = t.wrapping_add(pid).wrapping_mul(6364136223846793005);
    state = state.wrapping_add(1);
    state.wrapping_mul(6364136223846793005).wrapping_add(1)
}

// ── In-memory storage (for testing) ────────────────────────────────────────

/// A [`StorageSource`] that keeps all data in memory, backed by a
/// `HashMap<PathBuf, Vec<u8>>`.
///
/// Useful for unit tests that must not touch the filesystem.
#[derive(Debug, Default)]
pub struct MemoryStorage {
    data: std::cell::RefCell<std::collections::HashMap<PathBuf, Vec<u8>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl StorageSource for MemoryStorage {
    fn read(&self, path: &Path) -> Result<Vec<u8>, StorageError> {
        self.data
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(path.to_path_buf()))
    }

    fn write(&self, path: &Path, data: &[u8]) -> Result<(), StorageError> {
        self.data
            .borrow_mut()
            .insert(path.to_path_buf(), data.to_vec());
        Ok(())
    }

    fn delete(&self, path: &Path) -> Result<(), StorageError> {
        self.data.borrow_mut().remove(path);
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.data.borrow().contains_key(path)
    }

    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, StorageError> {
        let dir = normalise_dir(dir);
        let mut entries: Vec<PathBuf> = self
            .data
            .borrow()
            .keys()
            .filter(|p| p.parent() == Some(dir.as_path()))
            .cloned()
            .collect();
        entries.sort();
        Ok(entries)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── MemoryStorage tests ──────────────────────────────────────────────

    #[test]
    fn memory_read_write_roundtrip() {
        let store = MemoryStorage::new();
        let path = Path::new("test.txt");
        store.write(path, b"hello world").unwrap();
        let data = store.read(path).unwrap();
        assert_eq!(data, b"hello world");
    }

    #[test]
    fn memory_read_missing_returns_not_found() {
        let store = MemoryStorage::new();
        let err = store.read(Path::new("nope")).unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[test]
    fn memory_delete_removes_entry() {
        let store = MemoryStorage::new();
        let path = Path::new("test.txt");
        store.write(path, b"data").unwrap();
        assert!(store.exists(path));
        store.delete(path).unwrap();
        assert!(!store.exists(path));
    }

    #[test]
    fn memory_list_entries_in_dir() {
        let store = MemoryStorage::new();
        store.write(Path::new("dir/a.txt"), b"a").unwrap();
        store.write(Path::new("dir/b.txt"), b"b").unwrap();
        store.write(Path::new("other/c.txt"), b"c").unwrap();

        let entries = store.list(Path::new("dir")).unwrap();
        let names: Vec<_> = entries
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["a.txt", "b.txt"]);
    }

    // ── NativeStorage tests (filesystem, tempdir-isolated) ───────────────

    #[test]
    fn native_write_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = NativeStorage::at(dir.path().to_path_buf());
        let path = Path::new("hello.txt");

        store.write(path, b"world").unwrap();
        assert!(store.exists(path));

        let data = store.read(path).unwrap();
        assert_eq!(data, b"world");
    }

    #[test]
    fn native_write_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let store = NativeStorage::at(dir.path().to_path_buf());
        let path = Path::new("a/deeply/nested/file.txt");

        store.write(path, b"content").unwrap();
        assert!(store.exists(path));
    }

    #[test]
    fn native_read_missing_returns_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = NativeStorage::at(dir.path().to_path_buf());
        let err = store.read(Path::new("nope")).unwrap_err();
        assert!(matches!(err, StorageError::NotFound(_)));
    }

    #[test]
    fn native_delete_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = NativeStorage::at(dir.path().to_path_buf());
        let path = Path::new("file.txt");

        store.write(path, b"data").unwrap();
        assert!(store.exists(path));

        store.delete(path).unwrap();
        assert!(!store.exists(path));
    }

    #[test]
    fn the_native_backend_lists_the_files_that_were_written_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = NativeStorage::at(dir.path().to_path_buf());

        store.write(Path::new("a.txt"), b"").unwrap();
        store.write(Path::new("b.txt"), b"").unwrap();

        let mut entries = store.list(Path::new(".")).unwrap();
        entries.sort();
        let names: Vec<_> = entries
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn native_list_returns_relative_paths_like_memory() {
        let dir = tempfile::tempdir().unwrap();
        let native = NativeStorage::at(dir.path().to_path_buf());
        let memory = MemoryStorage::new();

        for store in [&native as &dyn StorageSource, &memory] {
            store.write(Path::new("saves/a.crb"), b"a").unwrap();
            store.write(Path::new("saves/b.crb"), b"b").unwrap();
        }

        let expected = vec![PathBuf::from("saves/a.crb"), PathBuf::from("saves/b.crb")];
        assert_eq!(native.list(Path::new("saves")).unwrap(), expected);
        assert_eq!(memory.list(Path::new("saves")).unwrap(), expected);
    }

    // ── Path containment ─────────────────────────────────────────────────

    #[test]
    fn native_rejects_parent_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let store = NativeStorage::at(dir.path().join("root"));

        let escape = Path::new("../escape");
        assert!(matches!(
            store.write(escape, b"x").unwrap_err(),
            StorageError::InvalidPath(_)
        ));
        assert!(matches!(
            store.read(escape).unwrap_err(),
            StorageError::InvalidPath(_)
        ));
        assert!(matches!(
            store.delete(escape).unwrap_err(),
            StorageError::InvalidPath(_)
        ));
        assert!(!store.exists(escape));
        assert!(!dir.path().join("escape").exists());
    }

    #[test]
    fn native_rejects_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        std::fs::create_dir_all(victim.join("nested")).unwrap();

        let store = NativeStorage::at(dir.path().join("root"));
        // `Path::join` would discard the root entirely for an absolute
        // argument, so `delete` would `remove_dir_all` the victim directory.
        assert!(matches!(
            store.delete(&victim).unwrap_err(),
            StorageError::InvalidPath(_)
        ));
        assert!(victim.join("nested").exists());
    }

    #[test]
    fn native_accepts_nested_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = NativeStorage::at(dir.path().to_path_buf());
        let path = Path::new("./saves/campaign/slot1.crb");

        store.write(path, b"inside").unwrap();
        assert!(store.exists(path));
        assert_eq!(store.read(path).unwrap(), b"inside");
        assert!(dir.path().join("saves/campaign/slot1.crb").exists());
    }

    #[test]
    fn native_missing_file_error_carries_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = NativeStorage::at(dir.path().to_path_buf());
        match store.read(Path::new("nope.txt")).unwrap_err() {
            StorageError::NotFound(p) => assert_eq!(p, dir.path().join("nope.txt")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    // ── Atomic write tests ───────────────────────────────────────────────

    #[test]
    fn atomic_write_creates_file_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("target.txt");

        write_atomic(&path, b"atomic data").unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), b"atomic data");
    }

    #[test]
    fn atomic_write_does_not_leave_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("target.txt");

        write_atomic(&path, b"data").unwrap();

        // No .tmp files should remain.
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert!(!entries.iter().any(|n| n.ends_with(".tmp")));
        assert!(entries.contains(&"target.txt".to_string()));
    }

    #[test]
    fn atomic_write_overwrites_existing_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("target.txt");

        write_atomic(&path, b"old").unwrap();
        write_atomic(&path, b"new").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn atomic_write_removes_temp_file_when_rename_fails() {
        let dir = tempfile::tempdir().unwrap();
        // A non-empty directory cannot be replaced by a rename, so the write
        // fails after the temp file has been created and synced.
        let path = dir.path().join("target.txt");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("occupant"), b"x").unwrap();

        assert!(write_atomic(&path, b"data").is_err());

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "leaked temp files: {leftovers:?}");
    }

    #[test]
    fn atomic_write_creates_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/dir/target.txt");

        write_atomic(&path, b"deep").unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), b"deep");
    }
}
