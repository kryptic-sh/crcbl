//! `ISteamRemoteStorage` as a [`crcbl_store::StorageSource`]: Steam Cloud,
//! file by file.
//!
//! Conflicts are not this module's business: Steam keeps one version of each
//! file, and settles a clash between devices before the game starts, in its
//! own dialog. [`crcbl_store::synced::SyncedFile`] over this storage is what
//! notices a clash and hands it to the game (`docs/plan/42-steam.md`,
//! "Cloud").

use std::{
    ffi::{CStr, CString},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use crcbl_store::{StorageError, StorageSource};

use crate::{Steam, client::Client};

/// The longest cloud file name, in bytes: `k_cchFilenameMax`
/// (`isteamremotestorage.h`) is the size of the `char` arrays Steam's
/// callbacks carry a file name in, the NUL included.
pub const MAX_CLOUD_PATH_BYTES: usize = 259;

/// The largest file one write may carry: `k_unMaxCloudFileChunkSize`, 100 MiB
/// (`isteamremotestorage.h`).
pub const MAX_CLOUD_FILE_BYTES: usize = 100 * 1024 * 1024;

/// What every call answers off the pump thread.
const OFF_THREAD: &str =
    "SteamCloudStorage used off the thread Steam was initialised on; Steam was not called";

/// Steam Cloud's quota for this app and account (`GetQuota`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloudQuota {
    /// Bytes this app may keep in the cloud.
    pub total: u64,
    /// Bytes still free.
    pub available: u64,
}

/// Steam Cloud through `ISteamRemoteStorage`, as a
/// [`StorageSource`].
///
/// Paths are relative, `/`-joined into Steam's file names, at most
/// [`MAX_CLOUD_PATH_BYTES`]; anything else is
/// [`StorageError::InvalidPath`] before Steam is called. Steam's names are
/// case-insensitive and stored lowercase (the header says so), so
/// [`list`](StorageSource::list) answers lowercase paths. A write is one
/// `FileWrite` inside `BeginFileWriteBatch`/`EndFileWriteBatch`, and one
/// Steam refuses is an error, never `Ok`.
///
/// **`Send`, because `StorageSource` requires it — but Steam is only called
/// on the pump thread.** Off it, every call is [`StorageError::Unsupported`]
/// (and [`exists`](StorageSource::exists) is `false`) with no Steam call made.
#[derive(Debug)]
pub struct SteamCloudStorage {
    client: Arc<Client>,
}

impl SteamCloudStorage {
    /// Steam Cloud for this app.
    ///
    /// # Errors
    ///
    /// [`StorageError::Unsupported`] when the player turned Steam Cloud off
    /// (`IsCloudEnabledForAccount`) or the app has none
    /// (`IsCloudEnabledForApp`) — the game then keeps its files locally.
    pub fn new(steam: &Steam) -> Result<Self, StorageError> {
        let client = &steam.client;
        let storage = &client.lib.fns.remote_storage;
        // SAFETY: `client.remote_storage` is the non-null interface init
        // resolved; `Steam` is `!Send`, so this is the pump thread.
        let (account, app) = unsafe {
            (
                (storage.is_cloud_enabled_for_account)(client.remote_storage),
                (storage.is_cloud_enabled_for_app)(client.remote_storage),
            )
        };
        if !(account && app) {
            return Err(StorageError::Unsupported(
                "Steam Cloud is off for this account or this app",
            ));
        }
        Ok(Self {
            client: Arc::clone(client),
        })
    }

    /// The app's cloud quota, or `None` when Steam does not say or this is
    /// not the pump thread.
    #[must_use]
    pub fn quota(&self) -> Option<CloudQuota> {
        self.pump_thread().ok()?;
        let mut total = 0_u64;
        let mut available = 0_u64;
        // SAFETY: the interface is non-null, this is the pump thread, and
        // both out-parameters are writable for the call.
        let answered = unsafe {
            (self.client.lib.fns.remote_storage.get_quota)(
                self.client.remote_storage,
                &raw mut total,
                &raw mut available,
            )
        };
        answered.then_some(CloudQuota { total, available })
    }

    /// `Unsupported` off the pump thread.
    fn pump_thread(&self) -> Result<(), StorageError> {
        if self.client.on_pump_thread() {
            Ok(())
        } else {
            Err(StorageError::Unsupported(OFF_THREAD))
        }
    }

    /// `FileExists`, for a name already validated, on the pump thread.
    fn file_exists(&self, name: &CStr) -> bool {
        // SAFETY: the interface is non-null, `name` is NUL-terminated and
        // outlives the call, and the caller is on the pump thread.
        unsafe {
            (self.client.lib.fns.remote_storage.file_exists)(
                self.client.remote_storage,
                name.as_ptr(),
            )
        }
    }
}

/// `path` as a Steam Cloud file name: its components joined with `/`.
fn cloud_name(path: &Path) -> Result<CString, StorageError> {
    let name = joined(path)?;
    if name.is_empty() || name.len() > MAX_CLOUD_PATH_BYTES {
        return Err(StorageError::InvalidPath(path.to_path_buf()));
    }
    CString::new(name).map_err(|_| StorageError::InvalidPath(path.to_path_buf()))
}

/// `path`'s components joined with `/` — empty for the root — or
/// `InvalidPath` for one that is absolute, climbs out with `..`, or is not
/// UTF-8.
fn joined(path: &Path) -> Result<String, StorageError> {
    let invalid = || StorageError::InvalidPath(path.to_path_buf());
    let mut name = String::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                if !name.is_empty() {
                    name.push('/');
                }
                name.push_str(part.to_str().ok_or_else(invalid)?);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(invalid());
            }
        }
    }
    Ok(name)
}

/// A Steam file name as a relative path, one component per `/`-separated
/// part.
fn path_of(name: &str) -> PathBuf {
    name.split('/').collect()
}

impl StorageSource for SteamCloudStorage {
    fn read(&self, path: &Path) -> Result<Vec<u8>, StorageError> {
        self.pump_thread()?;
        let name = cloud_name(path)?;
        if !self.file_exists(&name) {
            return Err(StorageError::NotFound(path.to_path_buf()));
        }
        let storage = &self.client.lib.fns.remote_storage;
        // SAFETY: as in `file_exists`.
        let size = unsafe { (storage.get_file_size)(self.client.remote_storage, name.as_ptr()) };
        let Ok(len) = usize::try_from(size) else {
            return Err(StorageError::Other(format!(
                "GetFileSize answered {size} for {}",
                path.display()
            )));
        };
        let mut data = vec![0_u8; len];
        // SAFETY: as above; `data` is `size` writable bytes.
        let read = unsafe {
            (storage.file_read)(
                self.client.remote_storage,
                name.as_ptr(),
                data.as_mut_ptr().cast(),
                size,
            )
        };
        if read != size {
            return Err(StorageError::Other(format!(
                "FileRead gave {read} of {size} bytes of {}",
                path.display()
            )));
        }
        Ok(data)
    }

    fn write(&self, path: &Path, data: &[u8]) -> Result<(), StorageError> {
        self.pump_thread()?;
        let name = cloud_name(path)?;
        if data.len() > MAX_CLOUD_FILE_BYTES {
            return Err(StorageError::LimitExceeded(path.to_path_buf()));
        }
        let size = i32::try_from(data.len())
            .map_err(|_| StorageError::LimitExceeded(path.to_path_buf()))?;
        let storage = &self.client.lib.fns.remote_storage;
        let iface = self.client.remote_storage;
        // SAFETY: as in `file_exists`; `data` is `size` readable bytes.
        let written = unsafe {
            let batch = (storage.begin_file_write_batch)(iface);
            let written = (storage.file_write)(iface, name.as_ptr(), data.as_ptr().cast(), size);
            if batch {
                (storage.end_file_write_batch)(iface);
            }
            written
        };
        if written {
            Ok(())
        } else {
            Err(StorageError::Other(format!(
                "Steam refused FileWrite of {size} bytes to {}",
                path.display()
            )))
        }
    }

    fn delete(&self, path: &Path) -> Result<(), StorageError> {
        self.pump_thread()?;
        let name = cloud_name(path)?;
        // SAFETY: as in `file_exists`.
        let deleted = unsafe {
            (self.client.lib.fns.remote_storage.file_delete)(
                self.client.remote_storage,
                name.as_ptr(),
            )
        };
        match (deleted, self.file_exists(&name)) {
            (true, _) => Ok(()),
            (false, false) => Err(StorageError::NotFound(path.to_path_buf())),
            (false, true) => Err(StorageError::Other(format!(
                "Steam refused FileDelete of {}",
                path.display()
            ))),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        self.pump_thread().is_ok() && cloud_name(path).is_ok_and(|name| self.file_exists(&name))
    }

    /// Every cloud file directly under `dir`. A name Steam hands back that
    /// is not UTF-8 is listed lossily, and reads back as not found.
    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, StorageError> {
        self.pump_thread()?;
        let dir = joined(dir)?;
        let dir = if dir.is_empty() {
            PathBuf::new()
        } else {
            path_of(&dir)
        };
        let storage = &self.client.lib.fns.remote_storage;
        let iface = self.client.remote_storage;
        // SAFETY: as in `file_exists`.
        let count = unsafe { (storage.get_file_count)(iface) };
        let mut entries = Vec::new();
        for index in 0..count {
            let mut size = 0_i32;
            // SAFETY: as above; `index` is below the count Steam just gave,
            // and `size` is writable.
            let name = unsafe { (storage.get_file_name_and_size)(iface, index, &raw mut size) };
            if name.is_null() {
                return Err(StorageError::Other(format!(
                    "GetFileNameAndSize answered null for file {index} of {count}"
                )));
            }
            // SAFETY: a NUL-terminated string straight out of the call,
            // copied before any other Steam call.
            let name = unsafe { CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned();
            let path = path_of(&name);
            if path.parent() == Some(dir.as_path()) {
                entries.push(path);
            }
        }
        entries.sort();
        Ok(entries)
    }
}

#[cfg(test)]
mod tests;
