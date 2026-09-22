//! Finding and opening the Steam library at runtime.
//!
//! **Always by absolute path, never by bare name.** A bare
//! `dlopen("libsteam_api.so")` searches `LD_LIBRARY_PATH` and the system and
//! never the executable's directory, and a bare `LoadLibraryW` searches the
//! current directory, where a planted DLL would win. The search order is fixed
//! and reported in full on failure:
//!
//! 1. the directory of `std::env::current_exe()`, where a shipped build puts
//!    the redistributable (on macOS, also `../Frameworks/`, for a bundle);
//! 2. `$CRCBL_STEAM_SDK/redistributable_bin/<platform>/`, for development.
//!
//! **The module is never unloaded**, so a load that fails after the module
//! opened (a missing symbol) leaks it too: it cannot be holding anything, and
//! a leaked mapping is the price of never having to reason about it.
//!
//! Windows opens with `LoadLibraryExW(…, LOAD_WITH_ALTERED_SEARCH_PATH)`, so
//! the DLL's own dependencies resolve from its directory; Linux and macOS with
//! `dlopen(…, RTLD_NOW | RTLD_LOCAL)`. `aarch64` Linux has a path
//! (`linuxarm64`, SDK 1.63 and later) that no machine or CI job has run.

use std::{
    ffi::c_void,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use super::{Fns, Lib};
use crate::error::InitError;

/// The library's file name.
#[cfg(target_os = "windows")]
const LIBRARY: &str = "steam_api64.dll";
/// The library's file name.
#[cfg(target_os = "linux")]
const LIBRARY: &str = "libsteam_api.so";
/// The library's file name.
#[cfg(target_os = "macos")]
const LIBRARY: &str = "libsteam_api.dylib";

/// The SDK's `redistributable_bin` subdirectory for this target, if Valve
/// ships one.
#[cfg(target_os = "windows")]
const SDK_SUBDIR: Option<&str> = Some("win64");
/// The SDK's `redistributable_bin` subdirectory for this target, if Valve
/// ships one.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const SDK_SUBDIR: Option<&str> = Some("linux64");
/// The SDK's `redistributable_bin` subdirectory for this target, if Valve
/// ships one.
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const SDK_SUBDIR: Option<&str> = Some("linuxarm64");
/// The SDK's `redistributable_bin` subdirectory for this target: none for a
/// 64-bit Linux architecture Valve does not build for, so only the executable's
/// directory is searched.
#[cfg(all(
    target_os = "linux",
    not(any(target_arch = "x86_64", target_arch = "aarch64"))
))]
const SDK_SUBDIR: Option<&str> = None;
/// The SDK's `redistributable_bin` subdirectory for this target: one universal
/// x86-64 + arm64 dylib.
#[cfg(target_os = "macos")]
const SDK_SUBDIR: Option<&str> = Some("osx");

/// The environment variable naming an unzipped SDK, for development.
pub(crate) const SDK_ENV: &str = "CRCBL_STEAM_SDK";

/// The paths to try, in order, given the executable's directory and the SDK
/// root. Pure, so the order is testable without either existing.
pub(crate) fn candidates(exe_dir: Option<&Path>, sdk_root: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(dir) = exe_dir {
        out.push(dir.join(LIBRARY));
        #[cfg(target_os = "macos")]
        out.push(dir.join("..").join("Frameworks").join(LIBRARY));
    }
    if let (Some(root), Some(subdir)) = (sdk_root, SDK_SUBDIR) {
        out.push(root.join("redistributable_bin").join(subdir).join(LIBRARY));
    }
    out
}

/// The real search: the executable's directory and `$CRCBL_STEAM_SDK`, made
/// absolute. Anything that could not be determined is reported in the
/// returned notes rather than dropped, so `NoLibrary` says why a location was
/// never tried.
fn search_paths() -> (Vec<PathBuf>, Vec<String>) {
    let mut notes = Vec::new();
    let exe_dir = match std::env::current_exe() {
        Ok(exe) => exe.parent().map(Path::to_path_buf),
        Err(err) => {
            notes.push(format!("current_exe: {err}"));
            None
        }
    };
    let sdk_root = match std::env::var_os(SDK_ENV) {
        None => {
            notes.push(format!("{SDK_ENV} is not set"));
            None
        }
        Some(root) => match std::path::absolute(&root) {
            Ok(root) => Some(root),
            Err(err) => {
                notes.push(format!("{SDK_ENV}={}: {err}", Path::new(&root).display()));
                None
            }
        },
    };
    (candidates(exe_dir.as_deref(), sdk_root.as_deref()), notes)
}

/// The real library, loaded once per process and never unloaded. A failure
/// is cached too: the search looks at the same places every time.
pub(crate) fn real() -> Result<&'static Lib, InitError> {
    static REAL: OnceLock<Result<&'static Lib, InitError>> = OnceLock::new();
    REAL.get_or_init(|| {
        let (paths, notes) = search_paths();
        load_from(&paths, &notes)
    })
    .clone()
}

/// Opens the first path that opens and resolves every symbol from it.
///
/// # Errors
///
/// `NoLibrary` listing every path and each one's loader message (plus
/// `notes`) if none opened; `NoSymbol` if one opened and lacked a symbol.
pub(crate) fn load_from(paths: &[PathBuf], notes: &[String]) -> Result<&'static Lib, InitError> {
    let mut reasons: Vec<String> = notes.to_vec();
    for path in paths {
        match Module::open(path) {
            Ok(module) => {
                let fns = Fns::resolve(&mut |name| module.symbol(name))?;
                return Ok(Box::leak(Box::new(Lib::new(fns))));
            }
            Err(reason) => reasons.push(format!("{}: {reason}", path.display())),
        }
    }
    Err(InitError::NoLibrary {
        tried: paths.to_vec(),
        loader: reasons.join("; "),
    })
}

/// An open module handle, never closed.
struct Module(*mut c_void);

#[cfg(unix)]
mod os {
    use std::ffi::{c_char, c_int, c_void};

    // The dynamic loader, from libc — which `std` already links on every Unix
    // target — declared the way `crcbl-shell`'s X11 backend declares it.
    unsafe extern "C" {
        pub(super) fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        pub(super) fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        pub(super) fn dlerror() -> *const c_char;
    }

    /// `RTLD_NOW`: resolve every symbol at open, so a broken library fails
    /// here and not at the first call.
    pub(super) const RTLD_NOW: c_int = 2;
    /// `RTLD_LOCAL`: keep the library's symbols out of the global namespace.
    #[cfg(target_os = "linux")]
    pub(super) const RTLD_LOCAL: c_int = 0;
    /// `RTLD_LOCAL`: keep the library's symbols out of the global namespace.
    #[cfg(target_os = "macos")]
    pub(super) const RTLD_LOCAL: c_int = 4;
}

#[cfg(unix)]
impl Module {
    fn open(path: &Path) -> Result<Self, String> {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};

        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| "the path contains a NUL byte".to_owned())?;
        // SAFETY: `path` is NUL-terminated and outlives the call, and the flags
        // are valid. `dlopen` returns a handle or null and keeps no reference
        // to the string. Loading runs the library's initialisers, which is
        // the trust every `dlopen` extends.
        let handle = unsafe { os::dlopen(path.as_ptr(), os::RTLD_NOW | os::RTLD_LOCAL) };
        if handle.is_null() {
            return Err(last_dl_error());
        }
        Ok(Self(handle))
    }

    fn symbol(&self, name: &str) -> *mut c_void {
        let Ok(name) = std::ffi::CString::new(name) else {
            return core::ptr::null_mut();
        };
        // SAFETY: the handle is live (never closed) and `name` is
        // NUL-terminated.
        unsafe { os::dlsym(self.0, name.as_ptr()) }
    }
}

/// The loader's last error, copied out of its thread-local buffer.
#[cfg(unix)]
fn last_dl_error() -> String {
    // SAFETY: `dlerror` returns null or a pointer to a NUL-terminated buffer
    // owned by the loader, valid until the next loader call on this thread;
    // it is copied out before anything else runs.
    unsafe {
        let message = os::dlerror();
        if message.is_null() {
            "dlopen failed without a message".to_owned()
        } else {
            std::ffi::CStr::from_ptr(message).to_string_lossy().into_owned()
        }
    }
}

#[cfg(windows)]
mod os {
    use std::ffi::{c_char, c_void};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub(super) fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        pub(super) fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
        pub(super) fn GetLastError() -> u32;
    }

    /// `LOAD_WITH_ALTERED_SEARCH_PATH`: resolve the DLL's own dependencies
    /// from the directory it was loaded from, not the process's.
    pub(super) const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x0000_0008;
}

#[cfg(windows)]
impl Module {
    fn open(path: &Path) -> Result<Self, String> {
        use std::os::windows::ffi::OsStrExt;

        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err("the path contains a NUL character".to_owned());
        }
        let mut wide = wide;
        wide.push(0);
        // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the
        // call; the reserved `hFile` must be null; the flag is valid. The call
        // returns a module handle or null.
        let handle = unsafe {
            os::LoadLibraryExW(
                wide.as_ptr(),
                core::ptr::null_mut(),
                os::LOAD_WITH_ALTERED_SEARCH_PATH,
            )
        };
        if handle.is_null() {
            // SAFETY: no other call has run on this thread since the failure.
            let code = unsafe { os::GetLastError() };
            return Err(format!("LoadLibraryExW failed with error {code}"));
        }
        Ok(Self(handle))
    }

    fn symbol(&self, name: &str) -> *mut c_void {
        let Ok(name) = std::ffi::CString::new(name) else {
            return core::ptr::null_mut();
        };
        // SAFETY: the module is live (never freed) and `name` is
        // NUL-terminated ASCII.
        unsafe { os::GetProcAddress(self.0, name.as_ptr()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory under the system temp dir that exists and is empty, unique
    /// to this test process and name.
    fn empty_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("crcbl-steam-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "{}", dir.display());
        dir
    }

    #[test]
    fn the_executable_directory_is_searched_before_the_sdk() {
        let exe = Path::new("/game");
        let sdk = Path::new("/sdk");
        let paths = candidates(Some(exe), Some(sdk));
        assert_eq!(paths.first(), Some(&exe.join(LIBRARY)));
        if let Some(subdir) = SDK_SUBDIR {
            assert_eq!(
                paths.last(),
                Some(&sdk.join("redistributable_bin").join(subdir).join(LIBRARY))
            );
        }
        assert!(candidates(None, None).is_empty());
    }

    #[test]
    #[cfg_attr(miri, ignore = "opens files through the OS loader")]
    fn no_library_lists_every_path_tried_in_order() {
        let exe = empty_dir("exe");
        let sdk = empty_dir("sdk");
        let paths = candidates(Some(&exe), Some(&sdk));
        assert!(!paths.is_empty());
        let note = "a note".to_owned();
        match load_from(&paths, std::slice::from_ref(&note)) {
            Err(InitError::NoLibrary { tried, loader }) => {
                assert_eq!(tried, paths);
                assert!(loader.starts_with("a note; "), "{loader}");
                for path in &paths {
                    assert!(loader.contains(&path.display().to_string()), "{loader}");
                }
            }
            other => panic!("expected NoLibrary, got {other:?}"),
        }
    }
}
