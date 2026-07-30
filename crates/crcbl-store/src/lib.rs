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
//!
//! # Wasm / async design note
//!
//! The trait is defined as a **sync** `fn` interface because no async runtime
//! exists in the engine yet. The wasm backend (P5) will add an async path —
//! either by wrapping this trait with an adapter or by introducing a second
//! trait. The `Result<Vec<u8>>` return is forward-compatible with both.

pub mod crash_ring;
pub mod replay;
pub mod save;
pub mod settings;

use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

// ── Error type ─────────────────────────────────────────────────────────────

/// Errors produced by storage operations.
#[derive(Debug, thiserror::Error)]
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

    /// A generic application-level error.
    #[error("{0}")]
    Other(String),
}

impl From<io::Error> for StorageError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::NotFound => {
                // Extract the path from the error message when possible;
                // the io::Error itself stores the path on some platforms.
                StorageError::NotFound(PathBuf::from(e.to_string()))
            }
            io::ErrorKind::PermissionDenied => {
                StorageError::PermissionDenied(PathBuf::from(e.to_string()))
            }
            _ => StorageError::Io(e),
        }
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
    /// Create a storage root under a platform-standard config directory.
    ///
    /// `app_name` is used as a subdirectory name, e.g. `~/.config/<app_name>/`.
    /// The directory is created (including parents) on construction.
    pub fn config(app_name: &str) -> Result<Self, StorageError> {
        let base = dirs::config_dir()
            .ok_or_else(|| StorageError::Other("no config directory found".into()))?;
        let root = base.join(app_name);
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Create a storage root under a platform-standard data directory
    /// (preferred for saves and large data).
    pub fn data(app_name: &str) -> Result<Self, StorageError> {
        let base = dirs::data_dir()
            .ok_or_else(|| StorageError::Other("no data directory found".into()))?;
        let root = base.join(app_name);
        std::fs::create_dir_all(&root)?;
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

    /// Resolve a relative path to an absolute one.
    fn resolve(&self, path: &Path) -> PathBuf {
        self.root.join(path)
    }
}

impl StorageSource for NativeStorage {
    fn read(&self, path: &Path) -> Result<Vec<u8>, StorageError> {
        let abs = self.resolve(path);
        Ok(std::fs::read(&abs)?)
    }

    fn write(&self, path: &Path, data: &[u8]) -> Result<(), StorageError> {
        let abs = self.resolve(path);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_atomic(&abs, data)
    }

    fn delete(&self, path: &Path) -> Result<(), StorageError> {
        let abs = self.resolve(path);
        if abs.is_dir() {
            std::fs::remove_dir_all(&abs)?;
        } else {
            std::fs::remove_file(&abs)?;
        }
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.resolve(path).exists()
    }

    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, StorageError> {
        let abs = self.resolve(dir);
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&abs)? {
            let entry = entry?;
            entries.push(entry.file_name());
        }
        entries.sort();
        Ok(entries.into_iter().map(PathBuf::from).collect())
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
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), StorageError> {
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;

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

    {
        let mut f = std::fs::File::create(&tmp_name)?;
        f.write_all(data)?;
        f.sync_all()?;
    }

    // fsync the parent directory so the link is durable.
    if let Ok(parent_fd) = std::fs::File::open(parent) {
        parent_fd.sync_all().ok();
    }

    std::fs::rename(&tmp_name, path)?;

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
        let mut entries: Vec<PathBuf> = self
            .data
            .borrow()
            .keys()
            .filter(|p| p.parent() == Some(dir))
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
    fn native_list_entries() {
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
    fn atomic_write_creates_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/dir/target.txt");

        write_atomic(&path, b"deep").unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), b"deep");
    }
}
