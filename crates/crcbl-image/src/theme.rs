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
//! directories that hold the name, as GTK's `icon-theme.cache` lets it do.
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
    /// Each directory, with where it is in every copy of the theme, in
    /// search-path order.
    dirs: Vec<(ThemeDir, Vec<PathBuf>)>,
    inherits: Vec<String>,
}

/// Where a theme holds its icons: each icon's name, without its extension, to
/// the copies of it, and when the directories were read.
#[derive(Debug)]
struct Index {
    icons: HashMap<OsString, Vec<Spot>>,
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
    /// Reads every directory of `theme`; a directory that cannot be read
    /// holds nothing.
    fn read(theme: &Theme, now: Instant) -> Self {
        // Papirus lists each `@2x` directory as a symlink to its 1x one: read
        // every real directory once, for each place it is listed.
        let mut places: Vec<(PathBuf, Vec<(usize, usize)>)> = Vec::new();
        let mut seen: HashMap<PathBuf, usize> = HashMap::new();
        for (dir, (_, directories)) in theme.dirs.iter().enumerate() {
            for (root, directory) in directories.iter().enumerate() {
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
        Self { icons, at: now }
    }
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
                Arc::new(Theme { dirs, inherits })
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
    let spot = index
        .icons
        .get(OsStr::new(name))?
        .iter()
        .min_by_key(|spot| {
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
}
