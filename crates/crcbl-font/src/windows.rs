//! Windows: the registry's font lists narrow the files, each file's name
//! table decides — [`Source::Registry`].
//!
//! # Finding a family
//!
//! The `Fonts` keys under `HKEY_LOCAL_MACHINE` (fonts installed for every
//! user, in the Windows directory's `Fonts`) and `HKEY_CURRENT_USER` (fonts
//! one user installed, under `%LOCALAPPDATA%\Microsoft\Windows\Fonts`) list
//! every installed font file under a display name. A lookup opens only the
//! files whose display name could hold the family (see
//! [`registry::may_hold`]), reads each face's name and `OS/2` tables, and
//! keeps the faces whose family is the one asked for — so a few small reads
//! per lookup rather than a walk of the Fonts directory. The registry is read
//! once per [`SystemFonts`](crate::SystemFonts), on its first lookup, and
//! every file's faces once, on the first lookup that opens it.
//!
//! A font whose display name shares nothing with its family name is not
//! found. Windows lists the fonts it ships under their full names, and the
//! per-user installs on the machine this was written on under their
//! PostScript names; the narrowing admits both.
//!
//! What the generic families and the fallback list resolve to is in the
//! crate docs; [`DEFAULTS`] holds the names.
//!
//! # FFI
//!
//! `advapi32`'s registry functions and `kernel32`'s `GetWindowsDirectoryW`,
//! declared here with `#[link]` as `crcbl-shell`'s `win32::ffi` declares its
//! own: they are part of every Windows, so there is no absence for a runtime
//! probe to find.

use core::ffi::c_void;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::face::{self, Face};
use crate::{Defaults, FontMatch, FontQuery, Source, registry};

/// `HKEY`.
type Hkey = *mut c_void;

/// `HKEY_LOCAL_MACHINE`: `(HKEY)(ULONG_PTR)(LONG)0x80000002`, the predefined
/// handle sign-extended to the pointer's width as `winreg.h` defines it.
const HKEY_LOCAL_MACHINE: Hkey = 0x8000_0002_u32 as i32 as isize as Hkey;
/// `HKEY_CURRENT_USER`, likewise `(LONG)0x80000001`.
const HKEY_CURRENT_USER: Hkey = 0x8000_0001_u32 as i32 as isize as Hkey;
/// `KEY_READ`.
const KEY_READ: u32 = 0x2_0019;
/// `ERROR_SUCCESS`.
const ERROR_SUCCESS: i32 = 0;
/// `REG_SZ`.
const REG_SZ: u32 = 1;
/// `REG_MULTI_SZ`.
const REG_MULTI_SZ: u32 = 7;
/// `MAX_PATH`, the buffer `GetWindowsDirectoryW` documents as enough.
const MAX_PATH: usize = 260;

/// The installed-fonts list, under both roots.
const FONTS_KEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Fonts";
/// The fonts Windows links to each family, under `HKEY_LOCAL_MACHINE`.
const SYSTEM_LINK_KEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\FontLink\SystemLink";

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegOpenKeyExW(
        key: Hkey,
        sub_key: *const u16,
        options: u32,
        desired: u32,
        result: *mut Hkey,
    ) -> i32;
    fn RegCloseKey(key: Hkey) -> i32;
    fn RegQueryInfoKeyW(
        key: Hkey,
        class: *mut u16,
        class_len: *mut u32,
        reserved: *mut u32,
        sub_keys: *mut u32,
        max_sub_key_len: *mut u32,
        max_class_len: *mut u32,
        values: *mut u32,
        max_value_name_len: *mut u32,
        max_value_len: *mut u32,
        security_descriptor_len: *mut u32,
        last_write_time: *mut c_void,
    ) -> i32;
    fn RegEnumValueW(
        key: Hkey,
        index: u32,
        value_name: *mut u16,
        value_name_len: *mut u32,
        reserved: *mut u32,
        kind: *mut u32,
        data: *mut u8,
        data_len: *mut u32,
    ) -> i32;
    fn RegQueryValueExW(
        key: Hkey,
        value_name: *const u16,
        reserved: *mut u32,
        kind: *mut u32,
        data: *mut u8,
        data_len: *mut u32,
    ) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetWindowsDirectoryW(buffer: *mut u16, size: u32) -> u32;
}

const DEFAULTS: Defaults = Defaults {
    sans_serif: &["Segoe UI", "Arial"],
    serif: &["Times New Roman", "Cambria"],
    monospace: &["Consolas", "Courier New"],
    emoji: &["Segoe UI Emoji"],
    cascade: &[
        "Segoe UI",
        "Segoe UI Emoji",
        "Segoe UI Symbol",
        "Microsoft YaHei",
        "Yu Gothic",
        "Malgun Gothic",
        "Arial Unicode MS",
    ],
};

/// A registry key opened for reading, closed on drop.
///
/// Only [`Key::open`] makes one, from a handle `RegOpenKeyExW` returned, so
/// every `Key` holds a live handle this value alone closes.
struct Key(Hkey);

impl Key {
    /// `path` under `root`, or `None` when it cannot be opened.
    fn open(root: Hkey, path: &str) -> Option<Self> {
        let path = wide_nul(path);
        let mut key = ptr::null_mut();
        // SAFETY: `root` is a predefined key, `path` is NUL-terminated and
        // outlives the call, and `key` is a valid place for the handle.
        let status = unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_READ, &mut key) };
        (status == ERROR_SUCCESS).then_some(Self(key))
    }

    /// Every value's name, type and data, in the order the registry
    /// enumerates them. A value that grew between sizing the buffers and
    /// reading it is left out: a font being installed at that moment.
    fn values(&self) -> Vec<(String, u32, Vec<u16>)> {
        let (mut count, mut max_name, mut max_data) = (0, 0, 0);
        // SAFETY: `self.0` is a live key (see `Key`); the three outputs are
        // valid places, and every other pointer is null, which the function
        // documents as "not wanted".
        let status = unsafe {
            RegQueryInfoKeyW(
                self.0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut count,
                &mut max_name,
                &mut max_data,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            return Vec::new();
        }
        // The name's maximum leaves out its NUL; the data's is in bytes.
        let mut name = vec![0u16; max_name as usize + 1];
        let mut data = vec![0u16; (max_data as usize).div_ceil(2)];
        let mut values = Vec::new();
        for index in 0..count {
            let mut name_len = u32::try_from(name.len()).unwrap_or(u32::MAX);
            let mut data_len = u32::try_from(data.len() * 2).unwrap_or(u32::MAX);
            let mut kind = 0;
            // SAFETY: `self.0` is a live key; `name` holds `name_len` units
            // and `data` `data_len` bytes, both lengths taken from the
            // buffers themselves, and the call writes no further. A `u16`
            // buffer is aligned for the UTF-16 the string values hold.
            let status = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &mut name_len,
                    ptr::null_mut(),
                    &mut kind,
                    data.as_mut_ptr().cast(),
                    &mut data_len,
                )
            };
            if status == ERROR_SUCCESS {
                values.push((
                    String::from_utf16_lossy(&name[..name_len as usize]),
                    kind,
                    data[..(data_len as usize).div_ceil(2)].to_vec(),
                ));
            }
        }
        values
    }

    /// The value `name`'s type and data, or `None` when there is none.
    fn value(&self, name: &str) -> Option<(u32, Vec<u16>)> {
        let name = wide_nul(name);
        let mut kind = 0;
        let mut len = 0;
        // SAFETY: `self.0` is a live key and `name` is NUL-terminated; a
        // null data pointer asks only for the size, into `len`.
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null_mut(),
                &mut kind,
                ptr::null_mut(),
                &mut len,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let mut data = vec![0u16; (len as usize).div_ceil(2)];
        let mut len = u32::try_from(data.len() * 2).ok()?;
        // SAFETY: as above, with `data` holding `len` bytes, the length
        // taken from the buffer itself.
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null_mut(),
                &mut kind,
                data.as_mut_ptr().cast(),
                &mut len,
            )
        };
        (status == ERROR_SUCCESS).then(|| {
            data.truncate((len as usize).div_ceil(2));
            (kind, data)
        })
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from `RegOpenKeyExW` and nothing else closes
        // it (see `Key`). A failed close leaks a handle and nothing more.
        unsafe { RegCloseKey(self.0) };
    }
}

/// `text` as NUL-terminated UTF-16.
fn wide_nul(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// The Windows directory's `Fonts`, where `HKEY_LOCAL_MACHINE`'s bare file
/// names are.
fn system_fonts_dir() -> Option<PathBuf> {
    let mut buffer = [0u16; MAX_PATH];
    // SAFETY: `buffer` holds the `MAX_PATH` units the length says, and the
    // call writes no further; it returns the length written, or the length
    // needed when that is more, or 0 on failure.
    let len = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), MAX_PATH as u32) } as usize;
    (len > 0 && len < MAX_PATH)
        .then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..len])).join("Fonts"))
}

/// The per-user font directory, where `HKEY_CURRENT_USER`'s bare file names
/// would be: per-user installs write full paths, so this is seldom read.
fn user_fonts_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(Path::new(&local).join(r"Microsoft\Windows\Fonts"))
}

/// One installed font file and the names the registry lists it under.
#[derive(Debug)]
struct Installed {
    names: Vec<String>,
    path: PathBuf,
}

/// Every font file the two `Fonts` keys list.
fn installed() -> Vec<Installed> {
    let roots = [
        (HKEY_LOCAL_MACHINE, system_fonts_dir()),
        (HKEY_CURRENT_USER, user_fonts_dir()),
    ];
    let mut out = Vec::new();
    for (root, dir) in roots {
        let (Some(key), Some(dir)) = (Key::open(root, FONTS_KEY), dir) else {
            continue;
        };
        for (name, kind, data) in key.values() {
            if kind != REG_SZ {
                continue;
            }
            if let Some(path) = registry::font_path(&registry::wide_string(&data), &dir) {
                out.push(Installed {
                    names: registry::value_names(&name),
                    path,
                });
            }
        }
    }
    out
}

/// What `FontLink\SystemLink` lists for `family`: each entry's file and the
/// face it names.
fn system_link(family: &str) -> Vec<(String, Option<String>)> {
    let Some((REG_MULTI_SZ, data)) =
        Key::open(HKEY_LOCAL_MACHINE, SYSTEM_LINK_KEY).and_then(|key| key.value(family))
    else {
        return Vec::new();
    };
    registry::multi_string(&data)
        .iter()
        .filter_map(|entry| registry::link_entry(entry))
        .collect()
}

#[derive(Debug)]
pub(crate) struct Platform {
    installed: OnceLock<Vec<Installed>>,
    /// Each file opened so far, and its faces: empty for one that could not
    /// be read, so it is not tried again.
    files: Mutex<HashMap<PathBuf, Vec<Face>>>,
}

impl Platform {
    pub fn new() -> Self {
        Self {
            installed: OnceLock::new(),
            files: Mutex::new(HashMap::new()),
        }
    }

    pub fn source(&self) -> Source {
        Source::Registry
    }

    pub fn find(&self, query: &FontQuery) -> Option<FontMatch> {
        DEFAULTS.find(query, |family| self.faces_of(family))
    }

    pub fn fallbacks(&self, query: &FontQuery) -> Vec<FontMatch> {
        let family = self
            .find(query)
            .map_or_else(|| query.family.clone(), |primary| primary.family);
        let linked = system_link(&family)
            .into_iter()
            .filter_map(|(file, face)| self.linked(query, &file, face.as_deref()))
            .collect();
        DEFAULTS.fallbacks(query, linked, |query| self.find(query))
    }

    /// One `SystemLink` entry as a face: the face it names, looked up at
    /// `query`'s weight and style; else, when that finds nothing, the face
    /// of that name in `file`, or its first face when it names none.
    fn linked(&self, query: &FontQuery, file: &str, face: Option<&str>) -> Option<FontMatch> {
        if let Some(face) = face {
            let named = FontQuery {
                family: face.to_owned(),
                ..query.clone()
            };
            if let Some(found) = self.find(&named) {
                return Some(found);
            }
        }
        let path = registry::font_path(file, &system_fonts_dir()?)?;
        let faces = self.faces_in(&path);
        let chosen = match face {
            Some(face) => faces.iter().find(|candidate| candidate.answers_to(face)),
            None => faces.first(),
        };
        chosen.map(Face::to_match)
    }

    /// Every installed face of `family`, opening only the files whose
    /// registry names could hold it.
    fn faces_of(&self, family: &str) -> Vec<Face> {
        let installed = self.installed.get_or_init(installed);
        let mut out: Vec<Face> = Vec::new();
        for font in installed
            .iter()
            .filter(|font| registry::may_hold(&font.names, family))
        {
            for face in self.faces_in(&font.path) {
                let listed = out
                    .iter()
                    .any(|known| known.path == face.path && known.index == face.index);
                if face.answers_to(family) && !listed {
                    out.push(face);
                }
            }
        }
        out
    }

    /// The faces of the file at `path`, read on first asking.
    fn faces_in(&self, path: &Path) -> Vec<Face> {
        // A panic mid-insert leaves at worst a file missing from the cache,
        // which the next lookup reads again.
        let lock = || self.files.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(faces) = lock().get(path) {
            return faces.clone();
        }
        let faces = face::read_faces(path).unwrap_or_default();
        lock().insert(path.to_owned(), faces.clone());
        faces
    }
}
