//! A face's bytes as [`FontMatch::map`](crate::FontMatch::map) hands them
//! over: the font file mapped into memory when [`may_map`] admits it, a copy
//! on the heap everywhere else.
//!
//! **Why map at all**: a face read with `std::fs::read` is private memory for
//! as long as the font lives, every page of it, read or not. A mapping is the
//! page cache's own pages, clean and shared with every other process that has
//! the file open, and resident only where a glyph was read; the kernel drops
//! and refetches them under pressure rather than swapping them.

use std::fs::{File, Metadata};
use std::io::{self, Read};
use std::path::Path;

/// One face's bytes, as a font file of its own: a mapping of the font file
/// for a single font [`FontMatch::map`](crate::FontMatch::map) maps, else a
/// copy on the heap.
/// Dereferences to the bytes, and is what `crcbl-ui`'s `Font::from_system`
/// parses in place.
pub struct FaceBytes(Held);

enum Held {
    /// Read into memory this process owns.
    Read(Vec<u8>),
    /// The file, mapped read-only; see [`may_map`] for when.
    Mapped(Mapping),
}

/// A read-only mapping of a file, on the targets that have system fonts.
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
))]
type Mapping = memmap2::Mmap;

/// No mapping, on a target with no system fonts: nothing is ever mapped.
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
)))]
type Mapping = NoMapping;

/// Uninhabited: a [`Held::Mapped`] that cannot be made.
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
)))]
#[derive(Debug)]
enum NoMapping {}

#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
)))]
impl core::ops::Deref for NoMapping {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match *self {}
    }
}

impl FaceBytes {
    /// Whether the bytes are a mapping of the file rather than a copy on the
    /// heap: what says whether they cost this process private memory.
    #[must_use]
    pub fn is_mapped(&self) -> bool {
        matches!(self.0, Held::Mapped(_))
    }
}

impl From<Vec<u8>> for FaceBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self(Held::Read(bytes))
    }
}

impl core::ops::Deref for FaceBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match &self.0 {
            Held::Read(bytes) => bytes,
            Held::Mapped(map) => map,
        }
    }
}

impl AsRef<[u8]> for FaceBytes {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl std::fmt::Debug for FaceBytes {
    /// The length and where the bytes live; not the bytes.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaceBytes")
            .field("len", &self.len())
            .field("mapped", &self.is_mapped())
            .finish()
    }
}

/// The whole font file at `path`: mapped when [`may_map`] admits it, read
/// otherwise — and read too when the mapping is refused, as it is on a
/// filesystem that cannot map.
///
/// # Errors
///
/// The file cannot be opened, or, when it is not mapped, read.
pub(crate) fn whole_file(path: &Path) -> io::Result<FaceBytes> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    let mapped = may_map(&file, &metadata).then(|| map(&file));
    map_or_read(file, &metadata, mapped)
}

/// `file`, mapped read-only. Called only for a file [`may_map`] allows.
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
))]
fn map(file: &File) -> io::Result<Mapping> {
    // SAFETY: not provable, and accepted. `Mmap::map` is unsafe because the
    // `&[u8]` it hands back is memory another process can change or shrink
    // underneath it, and a byte slice whose contents change while it is
    // borrowed is undefined behaviour in Rust, whatever the code reading it
    // does. That `skrifa`, `read-fonts` and `harfrust` are
    // `forbid(unsafe_code)` and bounds-check every read lowers what a changed
    // byte can do in practice — a wrong glyph, a parse error — but does not
    // make it defined. A truncation is worse: a read past the new end raises
    // SIGBUS (Unix) or `EXCEPTION_IN_PAGE_ERROR` (Windows) and kills the
    // process. This is the risk fontconfig, FreeType and the browsers on
    // these systems accept: they map font files too.
    //
    // What `may_map`, this function's only gate, narrows it to:
    //
    // * The mapping is read-only, so nothing this process does writes it.
    // * Unix: a file root owns and neither its group nor others may write,
    //   wherever it is. A package manager replaces such a font by unlinking
    //   or renaming over it, which leaves the mapped inode whole. What it does
    //   not cover: root writing in place — `sudo cp newfont.ttf
    //   /usr/share/fonts/…` truncates and rewrites the file, and a running
    //   program that reads it can die of SIGBUS; a network or FUSE filesystem
    //   whose server or another client changes the file whatever its mode
    //   says; a rootless container whose "root" is an unprivileged user; and
    //   an I/O error paging the file in, which raises the same signal.
    // * Windows: a file in the Windows directory's `Fonts`, by the open
    //   handle's final path — a local directory only administrators and the
    //   installer write. A file with a mapped view cannot be truncated there
    //   (`SetEndOfFile` fails with `ERROR_USER_MAPPED_FILE`), so what is left
    //   is an administrator writing it in place, and an I/O error paging it.
    //
    // `memmap2`'s safe `map_if_safe` maps only a file with fs-verity or a
    // sealed memfd, which no system font is, so it would never map.
    unsafe { memmap2::Mmap::map(file) }
}

/// No target without system fonts maps.
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
)))]
fn map(_: &File) -> io::Result<Mapping> {
    Err(io::ErrorKind::Unsupported.into())
}

/// `mapped` when the file was mapped, otherwise `file` read whole: when it
/// was not to be mapped (`None`) and when mapping it failed alike.
fn map_or_read(
    mut file: File,
    metadata: &Metadata,
    mapped: Option<io::Result<Mapping>>,
) -> io::Result<FaceBytes> {
    if let Some(Ok(map)) = mapped {
        return Ok(FaceBytes(Held::Mapped(map)));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.read_to_end(&mut bytes)?;
    Ok(FaceBytes(Held::Read(bytes)))
}

/// Whether `file`, of `metadata`, may be mapped; see the `SAFETY` comment in
/// [`map`] for what each platform's rule does and does not rule out. On Unix:
/// root owns it and neither its group nor others may write it, wherever it
/// is.
#[cfg(unix)]
fn may_map(_: &File, metadata: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    only_root_writes(metadata.uid(), metadata.mode())
}

/// On Windows: the open handle's final path is in the Windows directory's
/// `Fonts`. Not a per-user font, which its user can write in place and whose
/// uninstall a mapping would block, and not a file on a share, which another
/// client can truncate.
#[cfg(windows)]
fn may_map(file: &File, _: &Metadata) -> bool {
    let fonts = crate::platform::system_fonts_dir().and_then(|dir| std::fs::canonicalize(dir).ok());
    in_system_fonts(
        crate::platform::final_path(file).as_deref(),
        fonts.as_deref(),
    )
}

/// On any other target: never, as it has no system fonts.
#[cfg(not(any(unix, windows)))]
fn may_map(_: &File, _: &Metadata) -> bool {
    false
}

/// Whether a Unix file of owner `uid` and permission bits `mode` is writable
/// by root alone: root owns it, and neither its group nor others may write.
#[cfg(unix)]
fn only_root_writes(uid: u32, mode: u32) -> bool {
    /// The group- and other-write permission bits, `S_IWGRP | S_IWOTH`.
    const GROUP_OR_OTHER_WRITE: u32 = 0o022;
    uid == 0 && mode & GROUP_OR_OTHER_WRITE == 0
}

/// Whether a file at the final path `file` is in the Windows `Fonts`
/// directory `fonts`, each in the `\\?\` form both are resolved to; no when
/// either is unknown.
#[cfg(windows)]
fn in_system_fonts(file: Option<&Path>, fonts: Option<&Path>) -> bool {
    file.zip(fonts)
        .is_some_and(|(file, fonts)| file.starts_with(fonts))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "freebsd",
        target_os = "macos"
    ))]
    use crate::sfnt::tests::SANS;
    use crate::tests::temp_dir;

    /// A file `name` holding `bytes` in `dir`, opened, with its metadata.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "freebsd",
        target_os = "macos"
    ))]
    fn opened(dir: &Path, name: &str, bytes: &[u8]) -> (File, Metadata) {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let file = File::open(&path).unwrap();
        let metadata = file.metadata().unwrap();
        (file, metadata)
    }

    /// **A mapped file and a read one are the same bytes**, and only the
    /// mapping says it is one.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "freebsd",
        target_os = "macos"
    ))]
    #[test]
    fn a_mapped_file_is_the_bytes_a_read_gives() {
        let dir = temp_dir("bytes-mapped");
        let (file, metadata) = opened(&dir, "mapped.ttf", SANS);
        // SAFETY: the file is this test's own, in a directory of its own, and
        // nothing writes it while it is mapped.
        let map = unsafe { memmap2::Mmap::map(&file) };
        let mapped = map_or_read(file, &metadata, Some(map)).unwrap();
        assert!(mapped.is_mapped());
        assert_eq!(&*mapped, SANS);

        let (file, metadata) = opened(&dir, "read.ttf", SANS);
        let read = map_or_read(file, &metadata, None).unwrap();
        assert!(!read.is_mapped());
        assert_eq!(&*read, SANS);

        // An empty file maps as no bytes, for the parse to refuse.
        let (file, metadata) = opened(&dir, "empty.ttf", b"");
        // SAFETY: as above.
        let map = unsafe { memmap2::Mmap::map(&file) };
        let empty = map_or_read(file, &metadata, Some(map)).unwrap();
        assert!(empty.is_mapped());
        assert!(empty.is_empty());
    }

    /// **A mapping that fails falls back to reading the file**, as on a
    /// filesystem that cannot map.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "freebsd",
        target_os = "macos"
    ))]
    #[test]
    fn a_refused_mapping_reads_the_file_instead() {
        let dir = temp_dir("bytes-refused");
        let (file, metadata) = opened(&dir, "refused.ttf", SANS);
        let refused = Some(Err(io::Error::other("this filesystem cannot map")));
        let read = map_or_read(file, &metadata, refused).unwrap();
        assert!(!read.is_mapped());
        assert_eq!(&*read, SANS);
    }

    /// **An empty file comes back empty**, for the parse to refuse, and a
    /// file that is not there is an error.
    #[test]
    fn an_empty_file_is_no_bytes() {
        let dir = temp_dir("bytes-empty");
        let path = dir.join("empty.ttf");
        std::fs::write(&path, b"").unwrap();
        assert!(whole_file(&path).unwrap().is_empty());
        assert!(whole_file(&dir.join("gone.ttf")).is_err());
    }

    /// **Only a file root alone can write is mapped**: one its group or
    /// anyone else may write, or a user owns, is read.
    #[cfg(unix)]
    #[test]
    fn only_a_file_root_alone_writes_is_mapped() {
        assert!(only_root_writes(0, 0o100_644));
        assert!(only_root_writes(0, 0o100_444));
        assert!(!only_root_writes(0, 0o100_664));
        assert!(!only_root_writes(0, 0o100_646));
        assert!(!only_root_writes(1000, 0o100_644));
        // The root directory is root's on every Unix, and no one else's to
        // write.
        let root = File::open("/").unwrap();
        assert!(may_map(&root, &root.metadata().unwrap()));
    }

    /// **On Windows only a file under the Windows `Fonts` directory is
    /// mapped**: not a per-user font, not a neighbouring directory whose
    /// name merely starts the same, and nothing whose path is unknown.
    #[cfg(windows)]
    #[test]
    fn only_a_file_in_the_windows_fonts_directory_is_mapped() {
        let fonts = Path::new(r"\\?\C:\Windows\Fonts");
        let at = |path: &str| in_system_fonts(Some(Path::new(path)), Some(fonts));
        assert!(at(r"\\?\C:\Windows\Fonts\arial.ttf"));
        assert!(!at(
            r"\\?\C:\Users\me\AppData\Local\Microsoft\Windows\Fonts\hack.ttf"
        ));
        assert!(!at(r"\\?\C:\Windows\FontsCache\arial.ttf"));
        assert!(!at(r"\\?\UNC\server\share\Windows\Fonts\arial.ttf"));
        assert!(!in_system_fonts(None, Some(fonts)));
        assert!(!in_system_fonts(
            Some(Path::new(r"\\?\C:\Windows\Fonts\arial.ttf")),
            None
        ));
        // The real directory, resolved as `may_map` resolves it, maps.
        let real = crate::platform::system_fonts_dir()
            .unwrap()
            .join("arial.ttf");
        let file = File::open(&real).unwrap();
        assert!(may_map(&file, &file.metadata().unwrap()));
        let dir = temp_dir("bytes-windows");
        let (file, metadata) = opened(&dir, "user.ttf", SANS);
        assert!(!may_map(&file, &metadata));
    }
}
