//! Icon-theme lookup by name, size and scale: the freedesktop.org Icon Theme
//! Specification's algorithm, with the caching a launcher or a tray needs.
//!
//! # The lookup
//!
//! [`IconThemes::lookup`] follows the specification's `FindIcon`: the chosen
//! theme, then each theme it `Inherits`, depth first, then the caller's
//! fallback themes, then `hicolor`, and last the icon as a bare file in a
//! base directory (`/usr/share/pixmaps`). Within a theme, a directory whose
//! size and scale match wins over one that does not, and among those that do,
//! the one whose nominal size is nearest the request: a threshold `22x22`
//! directory matches 24, but a `24x24` one beside it is the sharper answer, so
//! this departs from the specification's first-match rule as GTK does.
//! Otherwise the file in the directory nearest by the specification's
//! `DirectorySizeDistance`, the nominal size again breaking a tie. Each
//! directory is tried for PNG, then SVG, then XPM. A name that misses
//! everywhere is tried again as each of the caller's generic fallback names, so
//! an application with no icon still gets the generic one. An absolute path is
//! returned as it is, if the file exists, since `Icon=` and a notification's
//! `app_icon` may name one.
//!
//! # Where it looks
//!
//! [`SearchPath::from_environment`] builds the specification's base
//! directories: `$XDG_DATA_HOME/icons` (`~/.local/share/icons`),
//! `$HOME/.icons`, each `$XDG_DATA_DIRS` entry's `icons` (`/usr/local/share`
//! and `/usr/share` when unset), and `/usr/share/pixmaps` for bare files. A
//! tray icon's `IconThemePath` goes in front with [`SearchPath::prepend`].
//! These are the Linux and BSD desktops' locations; elsewhere the directories
//! do not exist and every name misses, so a Windows or macOS caller builds its
//! own [`SearchPath`] or does not use this module.
//!
//! # Caching
//!
//! A found icon is remembered for the life of the [`IconThemes`]. A miss is
//! remembered for the caller's `miss_retry` and looked up afresh after it, so a
//! theme installed while the program runs is found without a restart. Each
//! theme's directories are read once into an index of the icons they hold,
//! rebuilt on the same clock, because the specification's algorithm would
//! otherwise test hundreds of paths per icon; a lookup then ranks only the
//! directories that hold the name. A copy of a theme with a current
//! `icon-theme.cache` — GTK's, which `gtk-update-icon-cache` writes and
//! distributions regenerate on install — is not listed at all: its cache
//! already says which directories hold each name, and a lookup reads the few
//! records that name needs, so a launcher's first icon no longer waits on a
//! listing of every directory a large theme has.
//! What a found file holds is the caller's to cache:
//! [`FileVersion`] is the key that says when it changed.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// The theme every theme falls back to, by the specification.
pub const HICOLOR: &str = "hicolor";

/// The file extensions an icon may have, in the order they are preferred.
const EXTENSIONS: [&str; 3] = ["png", "svg", "xpm"];

/// The `Threshold` a directory has when its `index.theme` gives none, by the
/// specification.
const DEFAULT_THRESHOLD: u32 = 2;

/// How deep `Inherits` may chain before the lookup stops following it: deeper
/// than any real theme, and a stop for a theme that inherits itself.
const MAX_INHERITANCE_DEPTH: usize = 16;

/// The directories an icon is looked for in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchPath {
    /// Where themes live, each holding one directory per theme, searched in
    /// order.
    pub theme_roots: Vec<PathBuf>,
    /// Where an icon may sit as a bare file, `name.png` and so on, searched
    /// last of all.
    pub bare_roots: Vec<PathBuf>,
}

impl SearchPath {
    /// The specification's base directories, from this process's environment.
    #[must_use]
    pub fn from_environment() -> Self {
        Self::from_variables(|name| std::env::var_os(name))
    }

    /// The specification's base directories, with `var` reading the
    /// environment.
    fn from_variables(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let set = |name: &str| var(name).filter(|value| !value.is_empty());
        let home = set("HOME").map(PathBuf::from);
        let mut theme_roots = Vec::new();
        match (set("XDG_DATA_HOME"), &home) {
            (Some(data_home), _) => theme_roots.push(PathBuf::from(data_home).join("icons")),
            (None, Some(home)) => theme_roots.push(home.join(".local/share/icons")),
            (None, None) => {}
        }
        if let Some(home) = &home {
            theme_roots.push(home.join(".icons"));
        }
        // Colon-separated by the Base Directory specification on every
        // system, which is why this is not `std::env::split_paths`.
        let data_dirs =
            set("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        theme_roots.extend(
            data_dirs
                .to_string_lossy()
                .split(':')
                .filter(|dir| !dir.is_empty())
                .map(|dir| PathBuf::from(dir).join("icons")),
        );
        Self {
            theme_roots,
            bare_roots: vec![PathBuf::from("/usr/share/pixmaps")],
        }
    }

    /// Puts `roots` ahead of every other theme root: a tray icon's
    /// `IconThemePath`, searched first.
    #[must_use]
    pub fn prepend(mut self, roots: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut first: Vec<PathBuf> = roots.into_iter().collect();
        first.append(&mut self.theme_roots);
        self.theme_roots = first;
        self
    }
}

/// When a file last changed, as a key for whatever the caller made of it: a
/// cached decode is stale once the file's version differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileVersion {
    /// Its modification time, where the filesystem keeps one.
    pub modified: Option<SystemTime>,
    /// Its length in bytes.
    pub len: u64,
}

impl FileVersion {
    /// `path`'s version now; `None` if it cannot be read.
    #[must_use]
    pub fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        metadata.is_file().then(|| Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

/// How a theme directory's icons are sized, from its `index.theme` section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SizeKind {
    /// Exactly `size`.
    Fixed,
    /// Anything from `min` to `max`.
    Scaled { min: u32, max: u32 },
    /// Within `threshold` of `size`.
    Threshold(u32),
}

/// One directory of a theme, as its `index.theme` describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ThemeDir {
    /// Its path under the theme's directory, `48x48/apps`.
    path: String,
    size: u32,
    scale: u32,
    kind: SizeKind,
}

impl ThemeDir {
    /// The specification's `DirectoryMatchesSize`.
    fn matches(&self, size: u32, scale: u32) -> bool {
        if self.scale != scale {
            return false;
        }
        match self.kind {
            SizeKind::Fixed => self.size == size,
            SizeKind::Scaled { min, max } => (min..=max).contains(&size),
            SizeKind::Threshold(threshold) => (self.size.saturating_sub(threshold)
                ..=self.size.saturating_add(threshold))
                .contains(&size),
        }
    }

    /// How far the directory's nominal size is from the request, in device
    /// pixels: which of several matching directories is the sharper answer.
    fn nominal_gap(&self, size: u32, scale: u32) -> u64 {
        (u64::from(self.size) * u64::from(self.scale)).abs_diff(u64::from(size) * u64::from(scale))
    }

    /// The specification's `DirectorySizeDistance`, in device pixels. Its
    /// `Threshold` branch, as published, reads `MinSize` and `MaxSize`, which a
    /// threshold directory does not have; this uses the threshold's own
    /// bounds, as GTK does.
    fn distance(&self, size: u32, scale: u32) -> u64 {
        let wanted = u64::from(size) * u64::from(scale);
        let scaled = |value: u32| u64::from(value) * u64::from(self.scale);
        let (low, high) = match self.kind {
            SizeKind::Fixed => (scaled(self.size), scaled(self.size)),
            SizeKind::Scaled { min, max } => (scaled(min), scaled(max)),
            SizeKind::Threshold(threshold) => (
                scaled(self.size.saturating_sub(threshold)),
                scaled(self.size.saturating_add(threshold)),
            ),
        };
        if wanted < low {
            low - wanted
        } else {
            wanted.saturating_sub(high)
        }
    }
}

/// A theme as its `index.theme` describes it.
#[derive(Debug)]
struct Theme {
    /// Every copy of the theme, `root/name`, in search-path order.
    copies: Vec<PathBuf>,
    /// Each directory, with where it is in every copy of the theme, in
    /// search-path order.
    dirs: Vec<(ThemeDir, Vec<PathBuf>)>,
    inherits: Vec<String>,
}

/// Where a theme holds its icons, and when that was read: each copy with a
/// current GTK cache through the cache, and the rest as each icon's name,
/// without its extension, to the copies of it their directories hold.
#[derive(Debug)]
struct Index {
    icons: HashMap<OsString, Vec<Spot>>,
    /// Each copy read through its cache, by its position in
    /// [`Theme::copies`].
    caches: Vec<(usize, GtkCache)>,
    at: Instant,
}

/// One copy of an icon: positions in [`Theme::dirs`], in that directory's
/// copies and in [`EXTENSIONS`].
#[derive(Clone, Copy, Debug)]
struct Spot {
    dir: usize,
    root: usize,
    extension: usize,
}

impl Index {
    /// Opens each copy of `theme`'s cache, and reads every directory of the
    /// copies without a current one; a directory that cannot be read holds
    /// nothing.
    fn read(theme: &Theme, now: Instant) -> Self {
        let caches = theme
            .copies
            .iter()
            .enumerate()
            .filter_map(|(root, copy)| Some((root, GtkCache::open(copy, &theme.dirs)?)))
            .collect();
        Self::with_caches(theme, caches, now)
    }

    /// `theme`'s index with `caches` for the copies they belong to, every
    /// other copy listed.
    fn with_caches(theme: &Theme, caches: Vec<(usize, GtkCache)>, now: Instant) -> Self {
        // Papirus lists each `@2x` directory as a symlink to its 1x one: read
        // every real directory once, for each place it is listed.
        let mut places: Vec<(PathBuf, Vec<(usize, usize)>)> = Vec::new();
        let mut seen: HashMap<PathBuf, usize> = HashMap::new();
        for (dir, (_, directories)) in theme.dirs.iter().enumerate() {
            for (root, directory) in directories.iter().enumerate() {
                if caches.iter().any(|(cached, _)| *cached == root) {
                    continue;
                }
                let Ok(real) = std::fs::canonicalize(directory) else {
                    continue;
                };
                match seen.get(&real) {
                    Some(&at) => places[at].1.push((dir, root)),
                    None => {
                        seen.insert(real.clone(), places.len());
                        places.push((real, vec![(dir, root)]));
                    }
                }
            }
        }
        let mut icons: HashMap<OsString, Vec<Spot>> = HashMap::new();
        for (real, listed) in &places {
            let Ok(entries) = std::fs::read_dir(real) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name();
                let file = Path::new(&name);
                let (Some(stem), Some(extension)) = (file.file_stem(), file.extension()) else {
                    continue;
                };
                let Some(extension) = EXTENSIONS.iter().position(|known| *known == extension)
                else {
                    continue;
                };
                let spots = listed.iter().map(|&(dir, root)| Spot {
                    dir,
                    root,
                    extension,
                });
                // Most names recur in every size's directory: own the key
                // only the first time.
                match icons.get_mut(stem) {
                    Some(known) => known.extend(spots),
                    None => {
                        icons.insert(stem.to_owned(), spots.collect());
                    }
                }
            }
        }
        Self {
            icons,
            caches,
            at: now,
        }
    }

    /// Every copy of `name` the theme holds, from the listed directories and
    /// each cache.
    fn spots(&self, name: &str) -> Vec<Spot> {
        let mut spots = self
            .icons
            .get(OsStr::new(name))
            .cloned()
            .unwrap_or_default();
        for (root, cache) in &self.caches {
            for (dir, flags) in cache.images(name) {
                spots.extend(
                    EXTENSIONS
                        .iter()
                        .enumerate()
                        .filter(|&(extension, _)| flags & GTK_CACHE_FLAGS[extension] != 0)
                        .map(|(extension, _)| Spot {
                            dir,
                            root: *root,
                            extension,
                        }),
                );
            }
        }
        spots
    }
}

/// The flag GTK's cache records each of [`EXTENSIONS`] with, in that order:
/// `HAS_SUFFIX_PNG`, `HAS_SUFFIX_SVG` and `HAS_SUFFIX_XPM` in GTK's
/// `updateiconcache.c`.
const GTK_CACHE_FLAGS: [u16; 3] = [1 << 2, 1 << 1, 1 << 0];

/// The cache format's major version this reads: GTK's writer has written 1
/// since the format began.
const GTK_CACHE_MAJOR: u16 = 1;

/// What ends a chain of a bucket in GTK's cache.
const GTK_CACHE_END: u32 = 0xffff_ffff;

/// A name longer than this is not looked for in a cache: no icon has one, and
/// it bounds the read that compares it.
const GTK_CACHE_MAX_NAME: usize = 1024;

/// One copy of a theme's `icon-theme.cache`: GTK's index of which directories
/// hold each icon name, in which formats.
///
/// Transcribed from GTK 3's `gtkiconcache.c` (the reader) and
/// `updateiconcache.c` (the writer). Big-endian throughout: a header of the
/// major and minor version (two `u16`), the hash table's offset and the
/// directory list's (two `u32`); the directory list is a count and an offset
/// per directory to its NUL-terminated name, relative to the theme; the hash
/// table is a bucket count and an offset per bucket to a chain of entries,
/// each the next entry's offset ([`GTK_CACHE_END`] for none), its name's and
/// its image list's; an image list is a count and, per image, the directory's
/// index (`u16`), its flags (`u16`) and an offset to cached pixel data, which
/// this does not read. A name's bucket is `icon_name_hash` of it modulo the
/// bucket count; see [`gtk_icon_name_hash`].
///
/// **Read a record at a time rather than whole**: a lookup reads the bucket,
/// each chain entry and its name until one matches, and that entry's image
/// list — a handful of short reads from the page cache — so Papirus's three
/// megabytes are never held. The file is someone else's, so every offset is
/// checked against its length and a chain is cut off at the most entries the
/// file could hold; a cache that does not read answers no images for the name.
#[derive(Debug)]
struct GtkCache {
    file: std::fs::File,
    len: u64,
    /// Where the hash table is, and how many buckets it has.
    hash: u32,
    buckets: u32,
    /// Each of the cache's directories, as the position in [`Theme::dirs`] of
    /// the theme directory of that name; `None` for one the theme's
    /// `index.theme` does not list, which the lookup never searches.
    dirs: Vec<Option<usize>>,
}

impl GtkCache {
    /// `copy`'s cache, if it has one GTK would use: no older, to the second,
    /// than the theme's directory — GTK's rule, and the writer sets the
    /// directory's time to the cache's for it — and of a version this reads.
    fn open(copy: &Path, dirs: &[(ThemeDir, Vec<PathBuf>)]) -> Option<Self> {
        let seconds = |path: &Path| -> Option<u64> {
            let modified = std::fs::metadata(path).ok()?.modified().ok()?;
            Some(
                modified
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .ok()?
                    .as_secs(),
            )
        };
        let file = std::fs::File::open(copy.join("icon-theme.cache")).ok()?;
        if seconds(&copy.join("icon-theme.cache"))? < seconds(copy)? {
            return None;
        }
        let len = file.metadata().ok()?.len();
        let mut cache = Self {
            file,
            len,
            hash: 0,
            buckets: 0,
            dirs: Vec::new(),
        };
        if cache.u16_at(0)? != GTK_CACHE_MAJOR {
            return None;
        }
        cache.hash = cache.u32_at(4)?;
        cache.buckets = cache.u32_at(u64::from(cache.hash))?;
        if cache.buckets == 0 {
            return None;
        }
        let list = u64::from(cache.u32_at(8)?);
        let count = cache.u32_at(list)?;
        // Each directory takes at least its four-byte offset.
        if u64::from(count) * 4 > len {
            return None;
        }
        for at in 0..count {
            let name = cache.string_at(u64::from(cache.u32_at(list + 4 + 4 * u64::from(at))?))?;
            cache
                .dirs
                .push(dirs.iter().position(|(dir, _)| dir.path.as_bytes() == name));
        }
        Some(cache)
    }

    /// Each directory the theme lists that holds `name`, with the flags of
    /// the formats it holds it in.
    fn images(&self, name: &str) -> Vec<(usize, u16)> {
        self.find(name).unwrap_or_default()
    }

    /// [`images`](Self::images), or `None` where the cache does not read.
    fn find(&self, name: &str) -> Option<Vec<(usize, u16)>> {
        if name.len() > GTK_CACHE_MAX_NAME {
            return Some(Vec::new());
        }
        let bucket = gtk_icon_name_hash(name) % self.buckets;
        let mut entry = self.u32_at(u64::from(self.hash) + 4 + 4 * u64::from(bucket))?;
        // Twelve bytes an entry: a chain longer than the file could hold has
        // a loop in it.
        for _ in 0..=self.len / 12 {
            if entry == GTK_CACHE_END {
                return Some(Vec::new());
            }
            let here = u64::from(entry);
            if self.string_at(u64::from(self.u32_at(here + 4)?))? == name.as_bytes() {
                let list = u64::from(self.u32_at(here + 8)?);
                let count = self.u32_at(list)?;
                if u64::from(count) * 8 > self.len {
                    return None;
                }
                let mut images = Vec::new();
                for at in 0..count {
                    let image = list + 4 + 8 * u64::from(at);
                    let dir = usize::from(self.u16_at(image)?);
                    if let Some(&Some(dir)) = self.dirs.get(dir) {
                        images.push((dir, self.u16_at(image + 2)?));
                    }
                }
                return Some(images);
            }
            entry = self.u32_at(here)?;
        }
        None
    }

    /// `N` bytes at `offset`, or `None` past the end. Offsets are `u64` so
    /// arithmetic on the file's `u32` offsets cannot overflow: a corrupt
    /// offset lands past the end instead.
    fn bytes_at<const N: usize>(&self, offset: u64) -> Option<[u8; N]> {
        use std::io::{Read, Seek, SeekFrom};
        if offset.saturating_add(N as u64) > self.len {
            return None;
        }
        let mut bytes = [0; N];
        let mut file = &self.file;
        file.seek(SeekFrom::Start(offset)).ok()?;
        file.read_exact(&mut bytes).ok()?;
        Some(bytes)
    }

    fn u16_at(&self, offset: u64) -> Option<u16> {
        self.bytes_at(offset).map(u16::from_be_bytes)
    }

    fn u32_at(&self, offset: u64) -> Option<u32> {
        self.bytes_at(offset).map(u32::from_be_bytes)
    }

    /// The NUL-terminated string at `offset`, without its NUL; `None` when it
    /// runs past the end or past [`GTK_CACHE_MAX_NAME`].
    fn string_at(&self, offset: u64) -> Option<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let room = self.len.checked_sub(offset)?;
        let wanted = room.min(GTK_CACHE_MAX_NAME as u64 + 1);
        let mut bytes = vec![0; usize::try_from(wanted).ok()?];
        let mut file = &self.file;
        file.seek(SeekFrom::Start(offset)).ok()?;
        file.read_exact(&mut bytes).ok()?;
        let end = bytes.iter().position(|&byte| byte == 0)?;
        bytes.truncate(end);
        Some(bytes)
    }
}

/// GTK's `icon_name_hash`, which places a name in the cache's buckets: the
/// first byte, then `h * 31 + byte` for each after it, with the bytes read as
/// **signed** chars — a byte past ASCII is negative, as GTK's `signed char`
/// pointer makes it — and the arithmetic wrapping in 32 bits.
fn gtk_icon_name_hash(name: &str) -> u32 {
    let mut bytes = name.bytes().map(|byte| i32::from(byte as i8) as u32);
    let Some(first) = bytes.next() else {
        return 0;
    };
    bytes.fold(first, |hash, byte| {
        (hash << 5).wrapping_sub(hash).wrapping_add(byte)
    })
}

/// Reads a theme's `index.theme`: the desktop-entry format, groups of
/// `key=value` lines, `#` comments. Only the keys the lookup needs are read,
/// and a localised key (`Name[de]`) is a different key and ignored.
fn parse_index(text: &str) -> (Vec<ThemeDir>, Vec<String>) {
    let mut groups: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
    let mut current = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = Some(name);
            groups.entry(name).or_default();
        } else if let (Some(group), Some((key, value))) = (current, line.split_once('=')) {
            groups
                .entry(group)
                .or_default()
                .insert(key.trim(), value.trim());
        }
    }
    let theme = groups.get("Icon Theme");
    let list = |key: &str| -> Vec<String> {
        theme
            .and_then(|keys| keys.get(key))
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let inherits = list("Inherits");
    let mut names = list("Directories");
    names.extend(list("ScaledDirectories"));
    let mut seen = HashSet::new();
    let dirs = names
        .into_iter()
        .filter(|name| seen.insert(name.clone()))
        .filter_map(|name| {
            let keys = groups.get(name.as_str())?;
            let number = |key: &str| keys.get(key).and_then(|v| v.parse::<u32>().ok());
            let size = number("Size")?;
            let kind = match keys.get("Type").copied().unwrap_or("Threshold") {
                "Fixed" => SizeKind::Fixed,
                // The specification's value; `Scaled` is no `Type` it defines.
                "Scalable" => SizeKind::Scaled {
                    min: number("MinSize").unwrap_or(size),
                    max: number("MaxSize").unwrap_or(size),
                },
                _ => SizeKind::Threshold(number("Threshold").unwrap_or(DEFAULT_THRESHOLD)),
            };
            Some(ThemeDir {
                path: name,
                size,
                scale: number("Scale").unwrap_or(1).max(1),
                kind,
            })
        })
        .collect();
    (dirs, inherits)
}

/// What one lookup asks for, carried down the search.
#[derive(Clone, Copy, Debug)]
struct Query<'a> {
    name: &'a str,
    size: u32,
    scale: u32,
    now: Instant,
}

/// One cached answer, and when it was found.
#[derive(Clone, Debug)]
struct Cached {
    path: Option<PathBuf>,
    at: Instant,
}

/// The file names in one directory, and when they were read.
#[derive(Clone, Debug)]
struct Listing {
    names: HashSet<OsString>,
    at: Instant,
}

/// The icon themes on a search path, and what has been looked up in them.
#[derive(Debug)]
pub struct IconThemes {
    search: SearchPath,
    theme: String,
    fallback_themes: Vec<String>,
    generic_names: Vec<String>,
    miss_retry: Duration,
    themes: HashMap<String, Option<Arc<Theme>>>,
    indexes: HashMap<String, Index>,
    listings: HashMap<PathBuf, Listing>,
    cache: HashMap<(String, u32, u32), Cached>,
}

impl IconThemes {
    /// Lookups in `theme` on `search`, a miss retried after `miss_retry`.
    #[must_use]
    pub fn new(search: SearchPath, theme: impl Into<String>, miss_retry: Duration) -> Self {
        Self {
            search,
            theme: theme.into(),
            fallback_themes: Vec::new(),
            generic_names: Vec::new(),
            miss_retry,
            themes: HashMap::new(),
            indexes: HashMap::new(),
            listings: HashMap::new(),
            cache: HashMap::new(),
        }
    }

    /// Themes to try, in order, after the chosen one and everything it
    /// inherits and before `hicolor`: what a minimal desktop with only
    /// `hicolor` installed by default needs to find most icons.
    #[must_use]
    pub fn with_fallback_themes(mut self, themes: impl IntoIterator<Item = String>) -> Self {
        self.fallback_themes = themes.into_iter().collect();
        self
    }

    /// Names to try, in order, when a name misses everywhere:
    /// `application-x-executable` for an application with no icon of its own.
    #[must_use]
    pub fn with_generic_names(mut self, names: impl IntoIterator<Item = String>) -> Self {
        self.generic_names = names.into_iter().collect();
        self
    }

    /// The file for `name` at `size` pixels and `scale`, or `None`; see the
    /// [module docs](self) for the order and the caching.
    pub fn lookup(&mut self, name: &str, size: u32, scale: u32) -> Option<PathBuf> {
        if Path::new(name).is_absolute() {
            return FileVersion::of(Path::new(name)).map(|_| PathBuf::from(name));
        }
        let key = (name.to_owned(), size, scale.max(1));
        let now = Instant::now();
        if let Some(cached) = self.cache.get(&key)
            && (cached.path.is_some() || now.duration_since(cached.at) < self.miss_retry)
        {
            return cached.path.clone();
        }
        let path = self.find_with_generics(name, size, scale.max(1), now);
        self.cache.insert(
            key,
            Cached {
                path: path.clone(),
                at: now,
            },
        );
        path
    }

    /// `name`, then each generic name in turn.
    fn find_with_generics(
        &mut self,
        name: &str,
        size: u32,
        scale: u32,
        now: Instant,
    ) -> Option<PathBuf> {
        let generics = self.generic_names.clone();
        std::iter::once(name.to_owned())
            .chain(generics)
            .find_map(|candidate| {
                self.find(Query {
                    name: &candidate,
                    size,
                    scale,
                    now,
                })
            })
    }

    /// The specification's `FindIcon`, with the caller's fallback themes
    /// before `hicolor`.
    fn find(&mut self, query: Query<'_>) -> Option<PathBuf> {
        let name = query.name;
        // A name is never a path; one with a separator would climb out of the
        // theme directories.
        if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
            return None;
        }
        let mut order = vec![self.theme.clone()];
        order.extend(self.fallback_themes.iter().cloned());
        order.push(HICOLOR.to_owned());
        let mut visited = HashSet::new();
        for theme in order {
            if let Some(path) = self.find_in_theme(query, &theme, 0, &mut visited) {
                return Some(path);
            }
        }
        let bare_roots = self.search.bare_roots.clone();
        bare_roots.iter().find_map(|root| {
            EXTENSIONS
                .iter()
                .map(|extension| format!("{name}.{extension}"))
                .find(|file| self.listed(root, file, query.now))
                .map(|file| root.join(file))
        })
    }

    /// The specification's `FindIconHelper`: `theme`, then what it inherits.
    fn find_in_theme(
        &mut self,
        query: Query<'_>,
        theme: &str,
        depth: usize,
        visited: &mut HashSet<String>,
    ) -> Option<PathBuf> {
        if depth > MAX_INHERITANCE_DEPTH || !visited.insert(theme.to_owned()) {
            return None;
        }
        let parsed = self.theme(theme)?;
        if let Some(path) = lookup_in(query, &parsed, self.index(theme, &parsed, query.now)) {
            return Some(path);
        }
        parsed
            .inherits
            .iter()
            .find_map(|parent| self.find_in_theme(query, parent, depth + 1, visited))
    }

    /// `theme`'s description, read once; `None` for a theme with no
    /// `index.theme` on the search path.
    fn theme(&mut self, theme: &str) -> Option<Arc<Theme>> {
        if let Some(known) = self.themes.get(theme) {
            return known.clone();
        }
        let roots: Vec<PathBuf> = self
            .search
            .theme_roots
            .iter()
            .filter(|root| root.join(theme).is_dir())
            .cloned()
            .collect();
        // The first copy with an index describes the theme; the others only
        // add files, as the specification has it.
        let parsed = roots
            .iter()
            .find_map(|root| std::fs::read_to_string(root.join(theme).join("index.theme")).ok())
            .map(|text| {
                let (dirs, inherits) = parse_index(&text);
                let dirs = dirs
                    .into_iter()
                    .map(|dir| {
                        let directories = roots
                            .iter()
                            .map(|root| root.join(theme).join(&dir.path))
                            .collect();
                        (dir, directories)
                    })
                    .collect();
                let copies = roots.iter().map(|root| root.join(theme)).collect();
                Arc::new(Theme {
                    copies,
                    dirs,
                    inherits,
                })
            });
        self.themes.insert(theme.to_owned(), parsed.clone());
        parsed
    }

    /// `theme`'s index, read again once it is older than the miss retry.
    fn index(&mut self, name: &str, theme: &Theme, now: Instant) -> &Index {
        let stale = self
            .indexes
            .get(name)
            .is_none_or(|index| now.duration_since(index.at) >= self.miss_retry);
        if stale {
            self.indexes
                .insert(name.to_owned(), Index::read(theme, now));
        }
        self.indexes
            .get(name)
            .expect("indexed just above when missing or stale")
    }

    /// Whether `directory` holds `file`, from its file names, read again once
    /// they are older than the miss retry.
    fn listed(&mut self, directory: &Path, file: &str, now: Instant) -> bool {
        let stale = self
            .listings
            .get(directory)
            .is_none_or(|listing| now.duration_since(listing.at) >= self.miss_retry);
        if stale {
            let names = std::fs::read_dir(directory)
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .map(|entry| entry.file_name())
                        .collect()
                })
                .unwrap_or_default();
            self.listings
                .insert(directory.to_owned(), Listing { names, at: now });
        }
        self.listings
            .get(directory)
            .expect("listed just above when missing or stale")
            .names
            .contains(OsStr::new(file))
    }
}

/// The specification's `LookupIcon` in one theme, ranking the directories that
/// hold the icon by nominal size as the [module docs](self) say.
fn lookup_in(query: Query<'_>, theme: &Theme, index: &Index) -> Option<PathBuf> {
    let Query {
        name, size, scale, ..
    } = query;
    // Lower is better: a match before a miss, then the specification's
    // distance (zero for a match), then the nominal size's gap. The first
    // directory, copy and extension keep a tie.
    let spot = index.spots(name).into_iter().min_by_key(|spot| {
        let (dir, _) = &theme.dirs[spot.dir];
        (
            !dir.matches(size, scale),
            dir.distance(size, scale),
            dir.nominal_gap(size, scale),
            spot.dir,
            spot.root,
            spot.extension,
        )
    })?;
    let (_, directories) = &theme.dirs[spot.dir];
    Some(directories[spot.root].join(format!("{name}.{}", EXTENSIONS[spot.extension])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A fresh directory under the system's temporary directory, removed when
    /// dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "crcbl-image-theme-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).expect("scratch directory");
            Self(dir)
        }

        fn write(&self, path: &str, text: &str) -> PathBuf {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("directories");
            std::fs::write(&path, text).expect("file");
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const ADWAITA_INDEX: &str = "\
[Icon Theme]
Name=Adwaita
Name[de]=Adwaita auf Deutsch
Inherits=base
Directories=16x16/apps,48x48/apps,scalable/apps
ScaledDirectories=48x48@2/apps

# A fixed size, a threshold size and a scalable one.
[16x16/apps]
Size=16
Type=Fixed

[48x48/apps]
Size=48

[48x48@2/apps]
Size=48
Scale=2
Type=Fixed

[scalable/apps]
Size=64
Type=Scalable
MinSize=8
MaxSize=512
";

    fn search(scratch: &Scratch) -> SearchPath {
        SearchPath {
            theme_roots: vec![scratch.0.join("icons")],
            bare_roots: vec![scratch.0.join("pixmaps")],
        }
    }

    fn themes(scratch: &Scratch) -> IconThemes {
        IconThemes::new(search(scratch), "Adwaita", Duration::from_secs(3600))
    }

    #[test]
    fn an_index_is_read_for_its_directories_and_parents() {
        let (dirs, inherits) = parse_index(ADWAITA_INDEX);
        assert_eq!(inherits, ["base"]);
        assert_eq!(
            dirs,
            [
                ThemeDir {
                    path: "16x16/apps".into(),
                    size: 16,
                    scale: 1,
                    kind: SizeKind::Fixed
                },
                ThemeDir {
                    path: "48x48/apps".into(),
                    size: 48,
                    scale: 1,
                    kind: SizeKind::Threshold(DEFAULT_THRESHOLD)
                },
                ThemeDir {
                    path: "scalable/apps".into(),
                    size: 64,
                    scale: 1,
                    kind: SizeKind::Scaled { min: 8, max: 512 }
                },
                ThemeDir {
                    path: "48x48@2/apps".into(),
                    size: 48,
                    scale: 2,
                    kind: SizeKind::Fixed
                },
            ]
        );
    }

    #[test]
    fn sizes_match_and_measure_as_the_specification_says() {
        let fixed = ThemeDir {
            path: String::new(),
            size: 16,
            scale: 1,
            kind: SizeKind::Fixed,
        };
        let threshold = ThemeDir {
            kind: SizeKind::Threshold(2),
            size: 48,
            ..fixed.clone()
        };
        let scaled = ThemeDir {
            kind: SizeKind::Scaled { min: 8, max: 512 },
            size: 64,
            ..fixed.clone()
        };
        assert!(fixed.matches(16, 1) && !fixed.matches(17, 1) && !fixed.matches(16, 2));
        assert!(threshold.matches(46, 1) && threshold.matches(50, 1) && !threshold.matches(51, 1));
        assert!(scaled.matches(8, 1) && scaled.matches(512, 1) && !scaled.matches(513, 1));
        assert_eq!(fixed.distance(24, 1), 8);
        assert_eq!(fixed.distance(8, 2), 0);
        assert_eq!(threshold.distance(40, 1), 6);
        assert_eq!(threshold.distance(60, 1), 10);
        assert_eq!(threshold.distance(48, 1), 0);
        assert_eq!(scaled.distance(4, 1), 4);
    }

    #[test]
    fn an_exact_directory_wins_and_otherwise_the_nearest_does() {
        let scratch = Scratch::new();
        scratch.write("icons/Adwaita/index.theme", ADWAITA_INDEX);
        let small = scratch.write("icons/Adwaita/16x16/apps/firefox.png", "");
        let big = scratch.write("icons/Adwaita/48x48/apps/firefox.png", "");
        let double = scratch.write("icons/Adwaita/48x48@2/apps/firefox.png", "");
        let svg = scratch.write("icons/Adwaita/scalable/apps/firefox.svg", "");
        let term = scratch.write("icons/Adwaita/16x16/apps/term.png", "");
        scratch.write("icons/Adwaita/48x48/apps/term.png", "");
        let mut themes = themes(&scratch);
        assert_eq!(themes.lookup("firefox", 16, 1), Some(small));
        assert_eq!(themes.lookup("firefox", 48, 1), Some(big));
        assert_eq!(themes.lookup("firefox", 48, 2), Some(double));
        assert_eq!(themes.lookup("firefox", 128, 1), Some(svg));
        // No directory matches 30: 16 is 14 away, and 48's threshold reaches
        // down to 46, 16 away. (The scalable directory holds no `term`.)
        assert_eq!(themes.lookup("term", 30, 1), Some(term));
    }

    /// Adwaita's own sections: a scalable directory whose nominal size is far
    /// from the request still holds every size it ranges over, so it beats a
    /// small fixed PNG at either scale. Read as a threshold directory around
    /// 128 it lost to the 16 px PNG, and an icon drew stretched.
    #[test]
    fn a_scalable_directory_beats_a_small_png_it_ranges_over() {
        let scratch = Scratch::new();
        scratch.write(
            "icons/Adwaita/index.theme",
            "[Icon Theme]
Directories=16x16/mimetypes,scalable/mimetypes
             [16x16/mimetypes]
Size=16
Type=Fixed
             [scalable/mimetypes]
Size=128
MinSize=8
MaxSize=512
Type=Scalable
",
        );
        scratch.write(
            "icons/Adwaita/16x16/mimetypes/application-x-executable.png",
            "",
        );
        let svg = scratch.write(
            "icons/Adwaita/scalable/mimetypes/application-x-executable.svg",
            "",
        );
        let mut themes = themes(&scratch);
        assert_eq!(
            themes.lookup("application-x-executable", 24, 1),
            Some(svg.clone())
        );
        assert_eq!(themes.lookup("application-x-executable", 24, 2), Some(svg));
    }

    /// hicolor lists `22x22/apps`, whose threshold takes 24, before
    /// `24x24/apps`: the 24-pixel file is the answer for 24, and among two
    /// scalable directories that both range over 96, the one whose nominal
    /// size is nearer. By the specification's first match, each was the
    /// other file.
    #[test]
    fn the_nearest_nominal_size_wins_among_matching_directories() {
        let scratch = Scratch::new();
        scratch.write(
            "icons/hicolor/index.theme",
            "[Icon Theme]
Directories=22x22/apps,24x24/apps,256x256/apps,scalable/apps
             [22x22/apps]
Size=22
Type=Threshold
             [24x24/apps]
Size=24
Type=Threshold
             [256x256/apps]
Size=256
MinSize=64
MaxSize=256
Type=Scalable
             [scalable/apps]
Size=128
MinSize=1
MaxSize=256
Type=Scalable
",
        );
        scratch.write("icons/hicolor/22x22/apps/firefox.png", "");
        let exact = scratch.write("icons/hicolor/24x24/apps/firefox.png", "");
        scratch.write("icons/hicolor/256x256/apps/firefox.png", "");
        let svg = scratch.write("icons/hicolor/scalable/apps/firefox.svg", "");
        let mut themes = IconThemes::new(search(&scratch), "hicolor", Duration::ZERO);
        assert_eq!(themes.lookup("firefox", 24, 1), Some(exact));
        assert_eq!(themes.lookup("firefox", 96, 1), Some(svg.clone()));
        // No directory is at scale 2, so none matches; both scalable ones are
        // at distance 0 from 96 pixels, and the nominal size breaks the tie.
        assert_eq!(themes.lookup("firefox", 48, 2), Some(svg));
    }

    /// The same device pixels at the wrong scale are no match: the
    /// specification's exact pass comes before its nearest one.
    #[test]
    fn the_right_scale_beats_the_same_pixels_at_another() {
        let scratch = Scratch::new();
        scratch.write(
            "icons/hicolor/index.theme",
            "[Icon Theme]
Directories=96x96/apps,48x48@2/apps
             [96x96/apps]
Size=96
Type=Fixed
             [48x48@2/apps]
Size=48
Scale=2
Type=Fixed
",
        );
        scratch.write("icons/hicolor/96x96/apps/app.png", "");
        let double = scratch.write("icons/hicolor/48x48@2/apps/app.png", "");
        let mut themes = IconThemes::new(search(&scratch), "hicolor", Duration::ZERO);
        assert_eq!(themes.lookup("app", 48, 2), Some(double));
    }

    #[test]
    fn a_png_is_preferred_to_an_svg_in_the_same_directory() {
        let scratch = Scratch::new();
        scratch.write("icons/Adwaita/index.theme", ADWAITA_INDEX);
        scratch.write("icons/Adwaita/48x48/apps/mail.svg", "");
        let png = scratch.write("icons/Adwaita/48x48/apps/mail.png", "");
        assert_eq!(themes(&scratch).lookup("mail", 48, 1), Some(png));
    }

    #[test]
    fn the_first_listed_of_two_equal_directories_wins() {
        let scratch = Scratch::new();
        scratch.write(
            "icons/hicolor/index.theme",
            "[Icon Theme]
Directories=48x48/devices,48x48/apps
             [48x48/devices]
Size=48
             [48x48/apps]
Size=48
",
        );
        scratch.write("icons/hicolor/48x48/apps/phone.png", "");
        let devices = scratch.write("icons/hicolor/48x48/devices/phone.png", "");
        assert_eq!(themes(&scratch).lookup("phone", 48, 1), Some(devices));
    }

    /// Unix only: creating a symlink on Windows needs a privilege a test runner
    /// may not have.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_answers_under_its_own_name_and_scale() {
        let scratch = Scratch::new();
        scratch.write(
            "icons/Papirus/index.theme",
            "[Icon Theme]
Directories=16x16/apps,16x16@2x/apps
             [16x16/apps]
Size=16
Type=Fixed
             [16x16@2x/apps]
Size=16
Scale=2
Type=Fixed
",
        );
        scratch.write("icons/Papirus/16x16/apps/mail.png", "");
        std::os::unix::fs::symlink("16x16", scratch.0.join("icons/Papirus/16x16@2x"))
            .expect("symlink");
        let mut themes = IconThemes::new(search(&scratch), "Papirus", Duration::ZERO);
        assert_eq!(
            themes.lookup("mail", 16, 2),
            Some(scratch.0.join("icons/Papirus/16x16@2x/apps/mail.png"))
        );
        assert_eq!(
            themes.lookup("mail", 16, 1),
            Some(scratch.0.join("icons/Papirus/16x16/apps/mail.png"))
        );
    }

    #[test]
    fn parents_then_fallbacks_then_hicolor_then_bare_files() {
        let scratch = Scratch::new();
        scratch.write("icons/Adwaita/index.theme", ADWAITA_INDEX);
        scratch.write(
            "icons/base/index.theme",
            "[Icon Theme]\nInherits=Adwaita\nDirectories=apps\n[apps]\nSize=48\n",
        );
        scratch.write(
            "icons/Papirus/index.theme",
            "[Icon Theme]\nDirectories=apps\n[apps]\nSize=48\n",
        );
        scratch.write(
            "icons/hicolor/index.theme",
            "[Icon Theme]\nDirectories=apps\n[apps]\nSize=48\n",
        );
        let inherited = scratch.write("icons/base/apps/a.png", "");
        let fallback = scratch.write("icons/Papirus/apps/b.png", "");
        let hicolor = scratch.write("icons/hicolor/apps/c.png", "");
        let bare = scratch.write("pixmaps/d.xpm", "");
        scratch.write("icons/hicolor/apps/b.png", "");
        let mut themes = themes(&scratch).with_fallback_themes(["Papirus".to_owned()]);
        assert_eq!(themes.lookup("a", 48, 1), Some(inherited));
        assert_eq!(themes.lookup("b", 48, 1), Some(fallback));
        assert_eq!(themes.lookup("c", 48, 1), Some(hicolor));
        assert_eq!(themes.lookup("d", 48, 1), Some(bare));
        assert_eq!(themes.lookup("e", 48, 1), None, "and the cycle ends");
    }

    #[test]
    fn a_name_that_misses_tries_the_generic_names() {
        let scratch = Scratch::new();
        scratch.write(
            "icons/hicolor/index.theme",
            "[Icon Theme]\nDirectories=apps\n[apps]\nSize=48\n",
        );
        let generic = scratch.write("icons/hicolor/apps/application-x-executable.png", "");
        let mut themes = themes(&scratch).with_generic_names([
            "no-such-generic".to_owned(),
            "application-x-executable".to_owned(),
        ]);
        assert_eq!(themes.lookup("unheard-of", 48, 1), Some(generic));
    }

    #[test]
    fn an_extra_root_is_searched_first() {
        let scratch = Scratch::new();
        let index = "[Icon Theme]\nDirectories=apps\n[apps]\nSize=22\n";
        scratch.write("icons/hicolor/index.theme", index);
        scratch.write("icons/hicolor/apps/tray.png", "");
        scratch.write("tray-theme/hicolor/index.theme", index);
        let own = scratch.write("tray-theme/hicolor/apps/tray.png", "");
        let search = search(&scratch).prepend([scratch.0.join("tray-theme")]);
        let mut themes = IconThemes::new(search, "hicolor", Duration::ZERO);
        assert_eq!(themes.lookup("tray", 22, 1), Some(own));
    }

    #[test]
    fn an_absolute_path_is_its_own_answer_and_a_name_is_never_a_path() {
        let scratch = Scratch::new();
        let file = scratch.write("art/cover.png", "");
        scratch.write(
            "icons/hicolor/index.theme",
            "[Icon Theme]\nDirectories=apps\n[apps]\nSize=48\n",
        );
        scratch.write("icons/hicolor/secret.png", "");
        let mut themes = themes(&scratch);
        let absolute = file.to_str().expect("utf-8 path");
        assert_eq!(themes.lookup(absolute, 48, 1), Some(file.clone()));
        assert_eq!(themes.lookup(&format!("{absolute}.missing"), 48, 1), None);
        assert_eq!(themes.lookup("../secret", 48, 1), None);
    }

    #[test]
    fn a_miss_is_retried_only_once_its_time_is_up() {
        let scratch = Scratch::new();
        let index = "[Icon Theme]\nDirectories=apps\n[apps]\nSize=48\n";
        scratch.write("icons/hicolor/index.theme", index);
        let mut patient = IconThemes::new(search(&scratch), "hicolor", Duration::from_secs(3600));
        let mut eager = IconThemes::new(search(&scratch), "hicolor", Duration::ZERO);
        assert_eq!(patient.lookup("late", 48, 1), None);
        assert_eq!(eager.lookup("late", 48, 1), None);
        let late = scratch.write("icons/hicolor/apps/late.png", "");
        assert_eq!(
            patient.lookup("late", 48, 1),
            None,
            "the miss is still fresh"
        );
        assert_eq!(eager.lookup("late", 48, 1), Some(late.clone()));
        std::fs::remove_file(&late).expect("remove");
        assert_eq!(eager.lookup("late", 48, 1), Some(late), "a hit is kept");
    }

    #[test]
    fn a_file_version_changes_with_the_file() {
        let scratch = Scratch::new();
        let path = scratch.write("icon.png", "one");
        let before = FileVersion::of(&path).expect("version");
        std::fs::write(&path, "longer").expect("rewrite");
        assert_ne!(FileVersion::of(&path), Some(before));
        assert_eq!(FileVersion::of(&scratch.0.join("absent")), None);
    }

    #[test]
    fn the_environment_gives_the_specifications_directories() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| OsString::from(value))
            }
        };
        let path = SearchPath::from_variables(env(&[("HOME", "/home/u")]));
        assert_eq!(
            path.theme_roots,
            [
                PathBuf::from("/home/u/.local/share/icons"),
                PathBuf::from("/home/u/.icons"),
                PathBuf::from("/usr/local/share/icons"),
                PathBuf::from("/usr/share/icons"),
            ]
        );
        assert_eq!(path.bare_roots, [PathBuf::from("/usr/share/pixmaps")]);
        let custom = SearchPath::from_variables(env(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", "/data"),
            ("XDG_DATA_DIRS", ""),
        ]));
        assert_eq!(custom.theme_roots[0], PathBuf::from("/data/icons"));
        assert_eq!(
            custom.theme_roots[2],
            PathBuf::from("/usr/local/share/icons")
        );
    }

    /// GTK's `icon_name_hash`'s answers, from its C compiled verbatim:
    /// ASCII, a name past ASCII whose bytes are negative as signed chars,
    /// and the empty name.
    #[test]
    fn the_name_hash_is_gtks() {
        for (name, hash) in [
            ("", 0),
            ("a", 97),
            ("firefox", 3_445_514_969),
            ("application-x-executable", 213_956_124),
            ("org.gnome.Nautilus", 2_873_866_283),
            ("caf\u{e9}", 94_414_350),
            ("\u{65e5}\u{672c}", 3_450_900_514),
        ] {
            assert_eq!(gtk_icon_name_hash(name), hash, "{name:?}");
        }
    }

    /// A GTK cache over `dirs` holding `icons` — each name with the
    /// directories (by position in `dirs`) and flags it is in — hashed into
    /// `buckets` buckets, laid out as `updateiconcache.c` lays one out.
    fn gtk_cache(dirs: &[&str], icons: &[(&str, &[(u16, u16)])], buckets: u32) -> Vec<u8> {
        fn put(out: &mut [u8], at: usize, value: u32) {
            out[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        let offset = |out: &Vec<u8>| u32::try_from(out.len()).expect("small");
        let mut out = Vec::new();
        out.extend(GTK_CACHE_MAJOR.to_be_bytes());
        out.extend(0_u16.to_be_bytes());
        out.extend([0; 8]);
        let hash = offset(&out);
        put(&mut out, 4, hash);
        out.extend(buckets.to_be_bytes());
        for _ in 0..buckets {
            out.extend(GTK_CACHE_END.to_be_bytes());
        }
        for (name, images) in icons {
            let entry = offset(&out);
            out.extend([0; 12]);
            let bucket = (hash + 4 + 4 * (gtk_icon_name_hash(name) % buckets)) as usize;
            // Prepended to its bucket's chain, as the writer does.
            let next = u32::from_be_bytes(out[bucket..bucket + 4].try_into().expect("four"));
            put(&mut out, entry as usize, next);
            put(&mut out, bucket, entry);
            let list = offset(&out);
            put(&mut out, entry as usize + 8, list);
            out.extend(u32::try_from(images.len()).expect("few").to_be_bytes());
            for (dir, flags) in *images {
                out.extend(dir.to_be_bytes());
                out.extend(flags.to_be_bytes());
                out.extend(0_u32.to_be_bytes());
            }
            let text = offset(&out);
            put(&mut out, entry as usize + 4, text);
            out.extend(name.as_bytes());
            out.push(0);
        }
        let list = offset(&out);
        put(&mut out, 8, list);
        out.extend(u32::try_from(dirs.len()).expect("few").to_be_bytes());
        let slots = out.len();
        out.extend(vec![0; 4 * dirs.len()]);
        for (at, dir) in dirs.iter().enumerate() {
            let text = offset(&out);
            put(&mut out, slots + 4 * at, text);
            out.extend(dir.as_bytes());
            out.push(0);
        }
        out
    }

    const CACHED_INDEX: &str = "\
[Icon Theme]
Name=Cached
Directories=16x16/apps,48x48/apps

[16x16/apps]
Size=16
Type=Fixed

[48x48/apps]
Size=48
";

    /// The PNG, SVG and XPM flags, as GTK writes them.
    const PNG: u16 = GTK_CACHE_FLAGS[0];
    const SVG: u16 = GTK_CACHE_FLAGS[1];

    /// **A theme with a current cache is looked up through it, not listed**:
    /// the cache names an icon no file holds, and the lookup returns it in
    /// the directory and format the cache gives — the best size, PNG over
    /// SVG — past a directory the theme does not list and down a chain of one
    /// bucket. A name the cache does not hold misses.
    #[test]
    fn a_current_cache_answers_without_listing_the_theme() {
        let scratch = Scratch::new();
        scratch.write("icons/cached/index.theme", CACHED_INDEX);
        let cache = gtk_cache(
            &["unlisted/apps", "16x16/apps", "48x48/apps"],
            &[
                ("other", &[(2, PNG)]),
                ("ghost", &[(0, PNG), (1, SVG), (2, PNG | SVG)]),
                ("more", &[(1, PNG)]),
            ],
            1,
        );
        std::fs::write(scratch.0.join("icons/cached/icon-theme.cache"), cache).expect("cache");
        let mut themes = IconThemes::new(search(&scratch), "cached", Duration::from_secs(3600));
        let theme = scratch.0.join("icons/cached");
        assert_eq!(
            themes.lookup("ghost", 48, 1),
            Some(theme.join("48x48/apps/ghost.png"))
        );
        assert_eq!(
            themes.lookup("ghost", 16, 1),
            Some(theme.join("16x16/apps/ghost.svg"))
        );
        assert_eq!(themes.lookup("missing", 48, 1), None);
    }

    /// **A cache older than its theme is not trusted**, by GTK's rule, and
    /// the theme is listed instead: the icon only the cache names misses,
    /// and the one only the directory holds is found.
    #[test]
    fn a_stale_cache_is_passed_over_for_the_listing() {
        let scratch = Scratch::new();
        scratch.write("icons/cached/index.theme", CACHED_INDEX);
        scratch.write("icons/cached/48x48/apps/real.png", "png");
        let cache = gtk_cache(&["48x48/apps"], &[("ghost", &[(0, PNG)])], 4);
        let path = scratch.0.join("icons/cached/icon-theme.cache");
        std::fs::write(&path, cache).expect("cache");
        let written = std::fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .expect("a modification time");
        open_directory(&scratch.0.join("icons/cached"))
            .and_then(|dir| dir.set_modified(written + Duration::from_secs(10)))
            .expect("the theme directory's time moved on");
        let mut themes = IconThemes::new(search(&scratch), "cached", Duration::from_secs(3600));
        assert_eq!(themes.lookup("ghost", 48, 1), None);
        assert_eq!(
            themes.lookup("real", 48, 1),
            Some(scratch.0.join("icons/cached/48x48/apps/real.png"))
        );
    }

    /// A handle on the directory `path`, to set its time through.
    #[cfg(not(windows))]
    fn open_directory(path: &Path) -> std::io::Result<std::fs::File> {
        std::fs::File::open(path)
    }

    /// A handle on the directory `path`, to set its time through: Windows
    /// opens a directory only with `FILE_FLAG_BACKUP_SEMANTICS`, and writes
    /// its times only through a handle opened for writing.
    #[cfg(windows)]
    fn open_directory(path: &Path) -> std::io::Result<std::fs::File> {
        use std::os::windows::fs::OpenOptionsExt;
        /// `FILE_FLAG_BACKUP_SEMANTICS`, from `winbase.h`.
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
    }

    /// **A cache that does not read answers nothing, and ends**: a chain
    /// that loops back on itself, and a file cut short.
    #[test]
    fn a_broken_cache_answers_nothing_and_ends() {
        let dirs = [(
            ThemeDir {
                path: "48x48/apps".to_owned(),
                size: 48,
                scale: 1,
                kind: SizeKind::Threshold(DEFAULT_THRESHOLD),
            },
            Vec::new(),
        )];
        let scratch = Scratch::new();
        let open = |bytes: &[u8]| {
            std::fs::write(scratch.0.join("icon-theme.cache"), bytes).expect("cache");
            GtkCache::open(&scratch.0, &dirs)
        };
        let mut looped = gtk_cache(&["48x48/apps"], &[("ghost", &[(0, PNG)])], 1);
        // The one entry, at the hash table's end, made its own next.
        let entry = 12 + 4 + 4;
        looped[entry..entry + 4].copy_from_slice(&(entry as u32).to_be_bytes());
        let cache = open(&looped).expect("the header reads");
        assert_eq!(cache.images("absent"), Vec::new());
        let whole = gtk_cache(&["48x48/apps"], &[("ghost", &[(0, PNG)])], 1);
        let cut = open(&whole[..whole.len() - 8]);
        assert!(cut.is_none_or(|cache| cache.images("ghost").is_empty()));
        assert_eq!(open(&whole).expect("whole").images("ghost"), vec![(0, PNG)]);
    }

    /// **An offset the file gives that would overflow 32 bits answers
    /// nothing**, as one past the end does: a bucket count so large that the
    /// name's bucket slot lies past four gigabytes, and a chain entry at the
    /// very top of the offset range.
    #[test]
    fn an_offset_past_32_bits_answers_nothing() {
        let dirs = [(
            ThemeDir {
                path: "48x48/apps".to_owned(),
                size: 48,
                scale: 1,
                kind: SizeKind::Threshold(DEFAULT_THRESHOLD),
            },
            Vec::new(),
        )];
        let scratch = Scratch::new();
        let open = |bytes: &[u8]| {
            std::fs::write(scratch.0.join("icon-theme.cache"), bytes).expect("cache");
            GtkCache::open(&scratch.0, &dirs).expect("the header reads")
        };
        // The bucket count sits at the hash table's start.
        let count = 12;
        let mut huge = gtk_cache(&["48x48/apps"], &[("ghost", &[(0, PNG)])], 1);
        huge[count..count + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        // The name lands in a bucket whose slot is past 32 bits.
        assert!(gtk_icon_name_hash("firefox") >= 1 << 30);
        assert_eq!(open(&huge).find("firefox"), None);
        // The one bucket's slot, after the count, made to name an entry
        // whose fields are past 32 bits.
        let slot = count + 4;
        let mut high = gtk_cache(&["48x48/apps"], &[("ghost", &[(0, PNG)])], 1);
        high[slot..slot + 4].copy_from_slice(&(GTK_CACHE_END - 2).to_be_bytes());
        assert_eq!(open(&high).find("ghost"), None);
    }

    /// **Every installed theme's cache agrees with listing it**: for each icon
    /// a listing finds in a theme with a current GTK cache, the cache gives
    /// the same directories and formats. What tests the reader against
    /// caches GTK itself wrote, Papirus's three megabytes among them.
    /// Ignored: it reads `/usr/share/icons`, which a CI image may not have.
    #[test]
    #[ignore = "reads the installed icon themes"]
    fn every_installed_cache_agrees_with_a_listing() {
        let roots = SearchPath::from_environment();
        let mut themes = IconThemes::new(roots.clone(), HICOLOR, Duration::from_secs(3600));
        let names: Vec<String> = roots
            .theme_roots
            .iter()
            .filter_map(|root| std::fs::read_dir(root).ok())
            .flatten()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().join("icon-theme.cache").is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        let mut checked = 0;
        for name in names {
            let Some(theme) = themes.theme(&name) else {
                continue;
            };
            let now = Instant::now();
            let cached = Index::read(&theme, now);
            if cached.caches.is_empty() {
                eprintln!("{name}: no current cache");
                continue;
            }
            let listed = Index::with_caches(&theme, Vec::new(), now);
            for icon in listed.icons.keys() {
                let icon = icon.to_str().expect("a UTF-8 name");
                let key = |spot: &Spot| (spot.dir, spot.root, spot.extension);
                let mut want: Vec<_> = listed.spots(icon).iter().map(key).collect();
                let mut got: Vec<_> = cached.spots(icon).iter().map(key).collect();
                want.sort_unstable();
                got.sort_unstable();
                assert_eq!(got, want, "{name}: {icon}");
                checked += 1;
            }
            eprintln!("{name}: {} icons agree", listed.icons.len());
        }
        assert!(checked > 0, "no installed theme has a cache to check");
    }
}
