//! Vulkan drivers by their ICD manifests: the files the loader finds them
//! through, read and loaded so one instance can open exactly one driver.
//!
//! # Why
//!
//! A `vkCreateInstance` loads **every** installed driver, whatever adapter the
//! program goes on to use: on a laptop with an Intel GPU and an NVIDIA one,
//! a program that renders on the Intel GPU still loads NVIDIA's driver and
//! holds its device files open, which costs start-up time and wakes the
//! discrete GPU. [`VkInstance::open_driver`](crate::VkInstance::open_driver)
//! opens an instance through one driver alone, with the loader's
//! `VK_LUNARG_direct_driver_loading` in its exclusive mode: the loader then
//! loads that driver and none of the others, and no environment variable is
//! written to get there.
//!
//! # Where the manifests are
//!
//! [`manifests`] walks the directories the Vulkan loader's own documentation
//! lists for Linux and the BSDs, in its order: `VK_DRIVER_FILES` (or the older
//! `VK_ICD_FILENAMES`) when set, and otherwise `$XDG_CONFIG_HOME`,
//! `$XDG_CONFIG_DIRS`, `/etc`, `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`, each with
//! `vulkan/icd.d` appended. Windows finds its drivers through the registry,
//! which this does not read: there [`manifests`] is empty, and Windows' saved
//! GPU choice is D3D12's rather than Vulkan's.
//!
//! # A loaded driver stays loaded
//!
//! A driver's library is `dlopen`ed and never closed: the instance calls into
//! it for as long as it lives, and drivers register state the loader itself
//! never unloads either. A probe that opens several drivers one after another
//! therefore leaves each loaded, which is why a probe belongs off a program's
//! start-up path.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ash::vk;

/// Why a driver could not be read or loaded.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    /// Its manifest could not be read.
    #[error("cannot read the driver manifest {}: {source}", path.display())]
    Read {
        /// The manifest.
        path: PathBuf,
        /// What reading it said.
        source: std::io::Error,
    },
    /// Its manifest is not an ICD manifest: not JSON, or no `ICD.library_path`.
    #[error("{} is not a Vulkan ICD manifest: {detail}", path.display())]
    Manifest {
        /// The manifest.
        path: PathBuf,
        /// What was wrong with it.
        detail: String,
    },
    /// Its library would not load.
    #[error("cannot load the driver library {}: {detail}", library.display())]
    Load {
        /// The library, as the manifest named it.
        library: PathBuf,
        /// What `dlopen` said.
        detail: String,
    },
    /// Its library loaded but does not export `vk_icdGetInstanceProcAddr`,
    /// which loading a driver directly needs.
    #[error("{} exports no vk_icdGetInstanceProcAddr", library.display())]
    NoEntryPoint {
        /// The library.
        library: PathBuf,
    },
    /// This platform does not load drivers by their manifests.
    #[error("loading a Vulkan driver directly is not supported on this platform")]
    Unsupported,
}

/// A driver as its ICD manifest describes it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Driver {
    /// The manifest it was read from.
    pub manifest: PathBuf,
    /// Its library: a path, resolved against the manifest's directory when the
    /// manifest gave a relative one, or a bare file name for the dynamic
    /// linker to search for, as the loader does.
    pub library: PathBuf,
}

/// The manifest fields this reads.
#[derive(serde::Deserialize)]
struct Manifest {
    #[serde(rename = "ICD")]
    icd: Icd,
}

/// A manifest's `ICD` object.
#[derive(serde::Deserialize)]
struct Icd {
    library_path: String,
}

impl Driver {
    /// Reads the manifest at `path`.
    ///
    /// # Errors
    ///
    /// [`DriverError::Read`] or [`DriverError::Manifest`].
    pub fn read(path: &Path) -> Result<Self, DriverError> {
        let text = std::fs::read_to_string(path).map_err(|source| DriverError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::parse(path, &text)
    }

    /// The driver the manifest text `text`, read from `path`, describes.
    fn parse(path: &Path, text: &str) -> Result<Self, DriverError> {
        let manifest: Manifest =
            serde_json::from_str(text).map_err(|error| DriverError::Manifest {
                path: path.to_owned(),
                detail: error.to_string(),
            })?;
        let named = PathBuf::from(manifest.icd.library_path);
        // The loader's rule: a bare file name goes to the dynamic linker's
        // search, and a relative path with a directory in it is relative to
        // the manifest's own directory.
        let library = if named.is_relative() && named.components().count() > 1 {
            path.parent().unwrap_or(Path::new("")).join(named)
        } else {
            named
        };
        Ok(Self {
            manifest: path.to_owned(),
            library,
        })
    }

    /// Loads the library and finds its `vk_icdGetInstanceProcAddr`.
    ///
    /// # Errors
    ///
    /// [`DriverError::Load`] or [`DriverError::NoEntryPoint`], and
    /// [`DriverError::Unsupported`] off Linux, the BSDs and macOS.
    pub(crate) fn load(&self) -> Result<vk::PFN_vkGetInstanceProcAddrLUNARG, DriverError> {
        load(&self.library)
    }

    /// The PCI vendor this driver serves, where its library's file name says:
    /// a vendor's own driver (`libGLX_nvidia`, `amdvlk64`) or a Mesa driver
    /// named for its hardware (`libvulkan_intel`, `libvulkan_radeon`,
    /// `libvulkan_nouveau`). `None` for any other name, which may serve
    /// anything: llvmpipe, a virtual GPU, a vendor this does not list.
    #[must_use]
    pub fn vendor_hint(&self) -> Option<u32> {
        let name = self
            .library
            .file_name()?
            .to_string_lossy()
            .to_ascii_lowercase();
        LIBRARY_VENDORS
            .iter()
            .find(|(part, _)| name.contains(part))
            .map(|&(_, vendor)| vendor)
    }
}

/// NVIDIA's PCI vendor id.
pub const PCI_VENDOR_NVIDIA: u32 = 0x10de;
/// Intel's PCI vendor id.
pub const PCI_VENDOR_INTEL: u32 = 0x8086;
/// AMD's PCI vendor id.
pub const PCI_VENDOR_AMD: u32 = 0x1002;

/// The parts of a driver library's file name that give its vendor; see
/// [`Driver::vendor_hint`].
const LIBRARY_VENDORS: [(&str, u32); 5] = [
    ("nvidia", PCI_VENDOR_NVIDIA),
    ("nouveau", PCI_VENDOR_NVIDIA),
    ("intel", PCI_VENDOR_INTEL),
    ("radeon", PCI_VENDOR_AMD),
    ("amdvlk", PCI_VENDOR_AMD),
];

/// The installed drivers that could serve a device of PCI vendor `vendor`,
/// most likely first: [`manifests`] read, then [`by_vendor`]. A manifest that
/// does not read is left out.
#[must_use]
pub fn for_vendor(vendor: u32) -> Vec<Driver> {
    let drivers = manifests()
        .iter()
        .filter_map(|manifest| Driver::read(manifest).ok())
        .collect();
    by_vendor(drivers, vendor)
}

/// `drivers` that could serve a device of PCI vendor `vendor`: those whose
/// [`Driver::vendor_hint`] is `vendor`, then those with none, each in the
/// order given. One whose hint names another vendor is left out, which is
/// the point: opening it would load that vendor's driver, and on a laptop
/// wake that vendor's GPU, for a device it cannot serve.
#[must_use]
pub fn by_vendor(drivers: Vec<Driver>, vendor: u32) -> Vec<Driver> {
    let (named, unnamed): (Vec<Driver>, Vec<Driver>) = drivers
        .into_iter()
        .filter(|driver| driver.vendor_hint().is_none_or(|hint| hint == vendor))
        .partition(|driver| driver.vendor_hint().is_some());
    named.into_iter().chain(unnamed).collect()
}

/// The PCI vendor of the device behind the DRM device number `device` — a
/// compositor's dmabuf `main_device`, an X server's DRI3 device — as sysfs
/// gives it. Linux only: `None` elsewhere, and when sysfs has no such device
/// or it is not a PCI one.
#[must_use]
pub fn drm_vendor(device: u64) -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        let (major, minor) = crate::adapter::major_minor(device);
        let text =
            std::fs::read_to_string(format!("/sys/dev/char/{major}:{minor}/device/vendor")).ok()?;
        u32::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = device;
        None
    }
}

/// Every ICD manifest the loader would find, in the order it would look,
/// without duplicates; see the module docs. Empty where the loader does not
/// use manifest directories.
#[must_use]
pub fn manifests() -> Vec<PathBuf> {
    if cfg!(windows) {
        return Vec::new();
    }
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let mut found = Vec::new();
    if let Some(listed) = var("VK_DRIVER_FILES").or_else(|| var("VK_ICD_FILENAMES")) {
        for entry in std::env::split_paths(&listed) {
            push_manifests(&entry, &mut found);
        }
        return found;
    }
    let home = var("HOME").map(PathBuf::from);
    let under_home = |rest: &str| home.as_ref().map(|home| home.join(rest));
    let list = |name: &str, default: &str| -> Vec<PathBuf> {
        std::env::split_paths(&var(name).unwrap_or_else(|| OsString::from(default))).collect()
    };
    let mut roots = Vec::new();
    roots.extend(
        var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| under_home(".config")),
    );
    roots.extend(list("XDG_CONFIG_DIRS", "/etc/xdg"));
    roots.push(PathBuf::from("/etc"));
    roots.extend(
        var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| under_home(".local/share")),
    );
    roots.extend(list("XDG_DATA_DIRS", "/usr/local/share:/usr/share"));
    for root in roots {
        push_manifests(&root.join("vulkan").join("icd.d"), &mut found);
    }
    found
}

/// Adds `entry`'s manifests to `found`: the file itself, or a directory's
/// `.json` files by name. One already listed — the same file reached twice,
/// as `/etc/xdg` and `/etc` can — is left out.
fn push_manifests(entry: &Path, found: &mut Vec<PathBuf>) {
    let mut add = |path: PathBuf| {
        let same = |known: &PathBuf| {
            known == &path || std::fs::canonicalize(known).ok() == std::fs::canonicalize(&path).ok()
        };
        if !found.iter().any(same) {
            found.push(path);
        }
    };
    if entry.is_file() {
        add(entry.to_owned());
        return;
    }
    let Ok(listing) = std::fs::read_dir(entry) else {
        return;
    };
    let mut files: Vec<PathBuf> = listing
        .filter_map(Result::ok)
        .map(|item| item.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    files.sort();
    for file in files {
        add(file);
    }
}

#[cfg(unix)]
fn load(library: &Path) -> Result<vk::PFN_vkGetInstanceProcAddrLUNARG, DriverError> {
    use core::ffi::{c_char, c_int, c_void};
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;

    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlerror() -> *mut c_char;
    }
    const RTLD_NOW: c_int = 2;

    let failed = |detail: String| DriverError::Load {
        library: library.to_owned(),
        detail,
    };
    let name = CString::new(library.as_os_str().as_bytes())
        .map_err(|_| failed("the path holds a NUL byte".to_owned()))?;
    // SAFETY: `name` is a NUL-terminated path. `dlopen` returns a handle or
    // null, and the handle is never closed — see the module docs.
    let handle = unsafe { dlopen(name.as_ptr(), RTLD_NOW) };
    if handle.is_null() {
        // SAFETY: `dlerror` returns null or a NUL-terminated message owned by
        // the C library, read before any other `dl` call can replace it.
        let message = unsafe { dlerror() };
        let detail = if message.is_null() {
            "dlopen failed".to_owned()
        } else {
            // SAFETY: as above.
            unsafe { CStr::from_ptr(message) }
                .to_string_lossy()
                .into_owned()
        };
        return Err(failed(detail));
    }
    // SAFETY: `handle` is live and the name is NUL-terminated.
    let symbol = unsafe { dlsym(handle, c"vk_icdGetInstanceProcAddr".as_ptr()) };
    if symbol.is_null() {
        return Err(DriverError::NoEntryPoint {
            library: library.to_owned(),
        });
    }
    // SAFETY: a driver exports `vk_icdGetInstanceProcAddr` with
    // `vkGetInstanceProcAddr`'s signature, which is the one the extension's
    // `pfnGetInstanceProcAddr` takes; the library is never unloaded, so the
    // pointer stays valid.
    Ok(Some(unsafe {
        core::mem::transmute::<
            *mut c_void,
            unsafe extern "system" fn(vk::Instance, *const c_char) -> vk::PFN_vkVoidFunction,
        >(symbol)
    }))
}

#[cfg(not(unix))]
fn load(_library: &Path) -> Result<vk::PFN_vkGetInstanceProcAddrLUNARG, DriverError> {
    Err(DriverError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A manifest's library is found as the loader finds it**: a bare name
    /// left for the dynamic linker, a relative path with a directory taken
    /// from the manifest's own directory, an absolute path as it is; and a
    /// file that is not an ICD manifest is refused by name.
    #[test]
    fn a_manifest_names_its_library_as_the_loader_reads_it() {
        let manifest = Path::new("/usr/share/vulkan/icd.d/intel_icd.json");
        let read = |library_path: &str| {
            Driver::parse(
                manifest,
                &format!(
                    r#"{{"file_format_version": "1.0.1",
                        "ICD": {{"api_version": "1.4.354", "library_path": "{library_path}"}}}}"#
                ),
            )
            .expect("a manifest")
            .library
        };
        assert_eq!(read("libvulkan_intel.so"), Path::new("libvulkan_intel.so"));
        assert_eq!(
            read("../../../lib/libvulkan_intel.so"),
            Path::new("/usr/share/vulkan/icd.d/../../../lib/libvulkan_intel.so")
        );
        assert_eq!(
            read("/usr/lib/libvulkan_intel.so"),
            Path::new("/usr/lib/libvulkan_intel.so")
        );
        assert!(matches!(
            Driver::parse(manifest, r#"{"layer": {"name": "x"}}"#),
            Err(DriverError::Manifest { .. })
        ));
        assert!(matches!(
            Driver::parse(manifest, "not json"),
            Err(DriverError::Manifest { .. })
        ));
    }

    /// A driver whose manifest names `library`.
    fn named(library: &str) -> Driver {
        Driver {
            manifest: PathBuf::from(format!("/usr/share/vulkan/icd.d/{library}.json")),
            library: PathBuf::from(library),
        }
    }

    /// **A library's file name gives its vendor where it names one**, in
    /// whatever case and at whatever path, and nothing for one that does not.
    #[test]
    fn a_driver_library_names_its_vendor() {
        for (library, vendor) in [
            ("/usr/lib/libvulkan_intel.so", Some(PCI_VENDOR_INTEL)),
            ("libvulkan_intel_hasvk.so", Some(PCI_VENDOR_INTEL)),
            ("libGLX_nvidia.so.0", Some(PCI_VENDOR_NVIDIA)),
            ("libvulkan_nouveau.so", Some(PCI_VENDOR_NVIDIA)),
            ("libvulkan_radeon.so", Some(PCI_VENDOR_AMD)),
            ("/opt/amdvlk/AMDVLK64.SO", Some(PCI_VENDOR_AMD)),
            ("libvulkan_lvp.so", None),
            ("libvulkan_virtio.so", None),
        ] {
            assert_eq!(named(library).vendor_hint(), vendor, "{library}");
        }
    }

    /// **The display's vendor's drivers go first and another vendor's are
    /// left out**: an Intel display tries Intel's two Mesa drivers, then
    /// llvmpipe, and never NVIDIA's.
    #[test]
    fn drivers_for_a_vendor_leave_out_every_other_vendors() {
        let installed = vec![
            named("libGLX_nvidia.so.0"),
            named("libvulkan_lvp.so"),
            named("libvulkan_intel_hasvk.so"),
            named("libvulkan_nouveau.so"),
            named("libvulkan_intel.so"),
        ];
        let libraries = |vendor| -> Vec<PathBuf> {
            by_vendor(installed.clone(), vendor)
                .into_iter()
                .map(|driver| driver.library)
                .collect()
        };
        assert_eq!(
            libraries(PCI_VENDOR_INTEL),
            [
                "libvulkan_intel_hasvk.so",
                "libvulkan_intel.so",
                "libvulkan_lvp.so"
            ]
            .map(PathBuf::from)
        );
        assert_eq!(
            libraries(PCI_VENDOR_NVIDIA),
            [
                "libGLX_nvidia.so.0",
                "libvulkan_nouveau.so",
                "libvulkan_lvp.so"
            ]
            .map(PathBuf::from)
        );
        assert_eq!(libraries(0x1af4), [PathBuf::from("libvulkan_lvp.so")]);
    }

    /// **A library that is not there is refused, naming it**, rather than
    /// opening an instance with no driver behind it.
    #[cfg(unix)]
    #[test]
    fn a_missing_library_is_a_load_error() {
        let driver = Driver {
            manifest: PathBuf::from("/nowhere/missing_icd.json"),
            library: PathBuf::from("/nowhere/libvulkan_missing.so"),
        };
        match driver.load() {
            Err(DriverError::Load { library, .. }) => {
                assert_eq!(library, Path::new("/nowhere/libvulkan_missing.so"));
            }
            other => panic!("a missing library loaded: {other:?}"),
        }
    }
}
