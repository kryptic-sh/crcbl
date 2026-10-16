//! System font lookup: a family name, a weight and a style to the font file
//! and face that draw them, and the system's fallback order for the same
//! query.
//!
//! ```no_run
//! use crcbl_font::{FontQuery, FontStyle, SystemFonts};
//!
//! let fonts = SystemFonts::new();
//! let query = FontQuery::new("sans-serif").weight(700).style(FontStyle::Italic);
//! if let Some(face) = fonts.find(&query) {
//!     let bytes = face.read()?; // one face, as a font file of its own
//!     println!("{} is {} bytes of {}", face.family, bytes.len(), face.path.display());
//! }
//! for fallback in fonts.fallbacks(&query) {
//!     println!("then {} ({})", fallback.family, fallback.path.display());
//! }
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! **A miss is `None`, never an error**, and a font file that cannot be read
//! is skipped as though it were not installed: what a caller does about a
//! missing font is fall back, which needs no reason.
//!
//! # Where each platform looks
//!
//! [`SystemFonts::source`] says which of these a lookup goes to.
//!
//! ## Windows
//!
//! [`Source::Registry`]. The registry's `Fonts` lists, machine-wide and per
//! user, narrow which files a lookup opens, and each file's name table
//! decides, so a lookup reads a few small tables rather than walking the
//! Fonts directory. Windows has no setting for the generic families, so each
//! is the first installed of a list: `sans-serif` Segoe UI then Arial,
//! `serif` Times New Roman then Cambria, `monospace` Consolas then Courier
//! New, `emoji` Segoe UI Emoji — the platform's own look rather than a
//! browser's, decided 2026-10-16. Fallbacks are the faces Windows links to
//! the family (`FontLink\SystemLink`), then Segoe UI, Segoe UI Emoji, Segoe
//! UI Symbol, Microsoft YaHei, Yu Gothic, Malgun Gothic and Arial Unicode MS,
//! each where installed.
//!
//! ## Linux and FreeBSD
//!
//! [`Source::Fontconfig`]: `libfontconfig.so.1`, loaded at run time, answers
//! as `fc-match` does, the system's and the user's configuration included,
//! and generic families are fontconfig's. `find` keeps fontconfig's face only
//! when it is of the family asked for or of one configuration put in that
//! family's place with a strong binding (the metric-compatible aliases, Arial
//! to Liberation Sans, are), since fontconfig matches its default face for a
//! family nothing installs. Fallbacks are `FcFontSort`'s order, trimmed to
//! the faces that add coverage, as `fc-match --sort` lists them.
//!
//! Without fontconfig, [`Source::Scan`]: `$XDG_DATA_HOME/fonts`, `~/.fonts`,
//! `/usr/share/fonts` and `/usr/local/share/fonts` are walked by name table,
//! **ignoring every fontconfig rule**; generic families are the first
//! installed of DejaVu, Noto and Liberation's, `emoji` Noto Color Emoji, and
//! fallbacks DejaVu Sans, Noto Sans, Noto Sans CJK SC, Noto Sans Symbols and
//! Symbols 2, and Noto Color Emoji, each where installed.
//!
//! ## macOS
//!
//! [`Source::Scan`]: `/System/Library/Fonts` and its `Supplemental`,
//! `/Library/Fonts` and `~/Library/Fonts`, walked by name table. Fonts macOS
//! downloads on demand into its asset store are not there to find; CoreText
//! is the better source, and the future one. Generic families are the first
//! installed of: `sans-serif` Helvetica then Helvetica Neue, `serif` Times
//! then Times New Roman, `monospace` Menlo then Courier, `emoji` Apple Color
//! Emoji. Fallbacks are the system interface font (as `System Font` or
//! `.SF NS`, else Helvetica Neue), Apple Color Emoji, PingFang SC, Hiragino
//! Sans, Apple SD Gothic Neo and Apple Symbols, each where installed.
//!
//! ## Every other target, `wasm32` included
//!
//! [`Source::None`]: no system fonts this crate can reach — a browser shows a
//! page none of its fonts' files — so every lookup misses. Decided
//! 2026-10-16 over leaving the crate out of those builds, so a program that
//! looks fonts up where it can compiles everywhere unchanged.
//!
//! # Matching
//!
//! Where this crate picks the face itself rather than asking the system, it
//! picks it by CSS Fonts Level 4's font matching (§5.2): width first, always
//! asked for as normal since a query names none; then style, italic falling
//! back to oblique then normal, oblique to italic then normal, normal to
//! oblique then italic; then weight, by CSS's rule for the weight asked for.
//! A family is every face whose name table gives it as its typographic family
//! (name ID 16) or its family (name ID 1), in any language, compared without
//! case. A generic family's [`FontMatch::family`] names the family it
//! resolved to.
//!
//! # Cost
//!
//! [`SystemFonts::new`] reads nothing. The first lookup does the platform's
//! one-off work and keeps what it read in the value, so keep one
//! `SystemFonts` for the process rather than one per lookup.

use std::io;
use std::path::PathBuf;

mod face;
mod matching;
#[cfg(any(windows, test))]
mod registry;
#[cfg(any(not(windows), test))]
mod scan;
mod sfnt;

#[cfg(windows)]
#[path = "windows.rs"]
mod platform;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[path = "fontconfig.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
)))]
#[path = "none.rs"]
mod platform;

use face::{Face, same_family};

/// `font-weight: normal`.
pub const NORMAL_WEIGHT: u16 = 400;

/// `font-weight: bold`.
pub const BOLD_WEIGHT: u16 = 700;

/// Whether a face is upright or slanted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontStyle {
    /// Upright.
    #[default]
    Normal,
    /// A true italic: a face drawn slanted, with its own letterforms.
    Italic,
    /// An upright face's letterforms, slanted.
    Oblique,
}

/// What to look up: a family, and the weight and style wanted of it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontQuery {
    /// A family name, such as `DejaVu Sans` or `Segoe UI`, or one of the
    /// generic families `sans-serif`, `serif`, `monospace` and `emoji`.
    pub family: String,
    /// From 1 to 1000, as CSS's `font-weight`: [`NORMAL_WEIGHT`] by default.
    pub weight: u16,
    pub style: FontStyle,
}

impl FontQuery {
    /// The upright face of `family` at [`NORMAL_WEIGHT`].
    #[must_use]
    pub fn new(family: impl Into<String>) -> Self {
        Self {
            family: family.into(),
            weight: NORMAL_WEIGHT,
            style: FontStyle::Normal,
        }
    }

    /// This query at `weight`.
    #[must_use]
    pub fn weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }

    /// This query in `style`.
    #[must_use]
    pub fn style(mut self, style: FontStyle) -> Self {
        self.style = style;
        self
    }
}

/// A face found: the file it is in, which face of that file, and what it is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontMatch {
    pub path: PathBuf,
    /// The face's index in a collection (`.ttc`, `.otc`); 0 in a single font.
    pub index: u32,
    /// The family the face belongs to, as the system names it: for a generic
    /// family, the one it resolved to.
    pub family: String,
    /// Its weight class, from 1 to 1000.
    pub weight: u16,
    pub style: FontStyle,
}

impl FontMatch {
    /// The face's bytes as a font file of its own: the file itself for a
    /// single font, the face's tables under a directory of their own for a
    /// face of a collection. What a parser that reads one font per file
    /// takes.
    ///
    /// # Errors
    ///
    /// The file cannot be read, or holds no such face: it changed since it
    /// was found, or [`FontMatch::index`] was edited.
    pub fn read(&self) -> io::Result<Vec<u8>> {
        sfnt::standalone(std::fs::read(&self.path)?, self.index)
    }
}

/// Where a [`SystemFonts`] finds its faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// fontconfig, `dlopen`ed: what `fc-match` answers, user configuration
    /// included.
    Fontconfig,
    /// The Windows registry's font lists, confirmed by each file's name table.
    Registry,
    /// A walk of the platform's font directories, matched by name table.
    Scan,
    /// No source: this target has no system fonts this crate can reach, and
    /// every lookup misses.
    None,
}

/// The system's fonts: what [`SystemFonts::find`] and
/// [`SystemFonts::fallbacks`] look up in. See the crate docs for where each
/// platform looks and what the first lookup costs.
pub struct SystemFonts {
    platform: platform::Platform,
}

impl std::fmt::Debug for SystemFonts {
    /// Not the index: printing it would not say anything a lookup does not.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemFonts").finish_non_exhaustive()
    }
}

impl Default for SystemFonts {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemFonts {
    /// The system's fonts, read nothing of yet: construction is free, and the
    /// first lookup pays for what it reads.
    #[must_use]
    pub fn new() -> Self {
        Self {
            platform: platform::Platform::new(),
        }
    }

    /// Where lookups go on this system. On Linux and FreeBSD this loads
    /// fontconfig if no lookup has yet, since whether it loads is the answer.
    #[must_use]
    pub fn source(&self) -> Source {
        self.platform.source()
    }

    /// The face of `query`'s family closest to its weight and style, or `None`
    /// when the system has no such family.
    #[must_use]
    pub fn find(&self, query: &FontQuery) -> Option<FontMatch> {
        self.platform.find(query)
    }

    /// The faces to try, in order, for a char `query`'s face has no glyph for:
    /// what `crcbl-ui`'s `Ui::set_fallback_fonts` takes, for CJK, symbols and
    /// emoji. The query's own family is left out, and every other family
    /// appears once, where it first would; empty when the system offers
    /// none.
    #[must_use]
    pub fn fallbacks(&self, query: &FontQuery) -> Vec<FontMatch> {
        self.platform.fallbacks(query)
    }
}

/// CSS's generic families this crate resolves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Generic {
    SansSerif,
    Serif,
    Monospace,
    Emoji,
}

impl Generic {
    /// The generic family `family` names, if it names one.
    fn parse(family: &str) -> Option<Self> {
        [
            ("sans-serif", Self::SansSerif),
            ("serif", Self::Serif),
            ("monospace", Self::Monospace),
            ("emoji", Self::Emoji),
        ]
        .into_iter()
        .find(|(name, _)| family.trim().eq_ignore_ascii_case(name))
        .map(|(_, generic)| generic)
    }
}

/// What a platform with no system matcher answers for each generic family
/// and for fallback: family names, each tried in order and skipped when the
/// system does not have it.
#[derive(Debug)]
struct Defaults {
    sans_serif: &'static [&'static str],
    serif: &'static [&'static str],
    monospace: &'static [&'static str],
    emoji: &'static [&'static str],
    /// The families [`SystemFonts::fallbacks`] lists after any the platform
    /// links to the query's family itself.
    cascade: &'static [&'static str],
}

impl Defaults {
    fn generic(&self, generic: Generic) -> &'static [&'static str] {
        match generic {
            Generic::SansSerif => self.sans_serif,
            Generic::Serif => self.serif,
            Generic::Monospace => self.monospace,
            Generic::Emoji => self.emoji,
        }
    }

    /// [`SystemFonts::find`] for a platform that finds a family's faces with
    /// `faces_of` and picks among them by [`matching`].
    fn find(&self, query: &FontQuery, faces_of: impl Fn(&str) -> Vec<Face>) -> Option<FontMatch> {
        let best = |family: &str| {
            let faces = faces_of(family);
            let attributes: Vec<_> = faces.iter().map(|face| face.attributes).collect();
            matching::best(&attributes, query.weight, query.style).map(|at| faces[at].to_match())
        };
        match Generic::parse(&query.family) {
            Some(generic) => self.generic(generic).iter().find_map(|family| best(family)),
            None => best(&query.family),
        }
    }

    /// [`SystemFonts::fallbacks`] for such a platform: `linked`, what it
    /// links to the query's family, then [`Defaults::cascade`], each at the
    /// query's weight and style.
    fn fallbacks(
        &self,
        query: &FontQuery,
        linked: Vec<FontMatch>,
        find: impl Fn(&FontQuery) -> Option<FontMatch>,
    ) -> Vec<FontMatch> {
        let primary = find(query);
        let cascade = self.cascade.iter().filter_map(|family| {
            find(&FontQuery {
                family: (*family).to_owned(),
                ..query.clone()
            })
        });
        ordered(primary.as_ref(), linked.into_iter().chain(cascade))
    }
}

/// `candidates` in order, without the faces of `primary`'s family, and with
/// only the first face of every other: a second face of a family listed
/// already adds a weight or style, not a script.
fn ordered(
    primary: Option<&FontMatch>,
    candidates: impl IntoIterator<Item = FontMatch>,
) -> Vec<FontMatch> {
    let mut out: Vec<FontMatch> = Vec::new();
    for candidate in candidates {
        let primary_family =
            primary.is_some_and(|primary| same_family(&primary.family, &candidate.family));
        let listed = out
            .iter()
            .any(|known| same_family(&known.family, &candidate.family));
        if !primary_family && !listed {
            out.push(candidate);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;
    use std::path::Path;

    use super::*;
    use crate::sfnt::tests::SANS;

    /// A fresh directory of this test's own under the system temp directory,
    /// removed when dropped.
    pub(crate) struct TempDir(PathBuf);

    impl Deref for TempDir {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// # Panics
    ///
    /// If the directory is already there: a leftover would decide the
    /// assertions, so it is refused rather than cleared.
    pub(crate) fn temp_dir(name: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!("crcbl-font-{name}-{}", std::process::id()));
        std::fs::create_dir(&path).expect("a fresh temp directory for this test");
        TempDir(path)
    }

    fn found(family: &str, path: &str, index: u32) -> FontMatch {
        FontMatch {
            path: PathBuf::from(path),
            index,
            family: family.to_owned(),
            weight: NORMAL_WEIGHT,
            style: FontStyle::Normal,
        }
    }

    #[test]
    fn generic_families_parse_without_case_and_nothing_else_does() {
        assert_eq!(Generic::parse("sans-serif"), Some(Generic::SansSerif));
        assert_eq!(Generic::parse(" Serif "), Some(Generic::Serif));
        assert_eq!(Generic::parse("MONOSPACE"), Some(Generic::Monospace));
        assert_eq!(Generic::parse("emoji"), Some(Generic::Emoji));
        assert_eq!(Generic::parse("sans serif"), None);
        assert_eq!(Generic::parse("Segoe UI Emoji"), None);
    }

    #[test]
    fn fallbacks_keep_their_order_and_drop_the_primary_family_and_repeats() {
        let primary = found("Segoe UI", "segoeui.ttf", 0);
        let list = ordered(
            Some(&primary),
            [
                found("Tahoma", "tahoma.ttf", 0),
                found("segoe ui", "segoeuib.ttf", 0),
                found("Meiryo UI", "meiryo.ttc", 1),
                found("Tahoma", "tahoma.ttf", 0),
                found("Meiryo", "meiryo.ttc", 0),
                found("Meiryo UI", "meiryo.ttc", 1),
                // Another face of a family already listed: its bold.
                found("TAHOMA", "tahomabd.ttf", 0),
            ],
        );
        let names: Vec<_> = list
            .iter()
            .map(|m| (m.family.as_str(), m.path.to_str().unwrap()))
            .collect();
        assert_eq!(
            names,
            [
                ("Tahoma", "tahoma.ttf"),
                ("Meiryo UI", "meiryo.ttc"),
                ("Meiryo", "meiryo.ttc")
            ]
        );
        // With no primary, only repeats go.
        let list = ordered(None, [primary.clone(), primary.clone()]);
        assert_eq!(list, [primary]);
    }

    #[test]
    fn catalog_find_resolves_generics_in_order_and_misses_as_none() {
        let dir = temp_dir("defaults-find");
        std::fs::write(dir.join("sans.ttf"), SANS).unwrap();
        let second = face::tests::renamed(SANS, &[(1, 0x409, "Second Family")]);
        std::fs::write(dir.join("second.ttf"), second).unwrap();
        let catalog = scan::Catalog::new(vec![dir.to_path_buf()]);
        let defaults = Defaults {
            sans_serif: &["Not Installed", "Atkinson Hyperlegible", "Second Family"],
            serif: &["Not Installed"],
            monospace: &[],
            emoji: &[],
            cascade: &[],
        };
        let find = |family: &str| defaults.find(&FontQuery::new(family), |f| catalog.faces_of(f));
        let sans = find("Sans-Serif").expect("the second sans-serif family");
        assert_eq!(sans.family, "Atkinson Hyperlegible");
        assert_eq!(sans.path, dir.join("sans.ttf"));
        assert_eq!(find("atkinson HYPERLEGIBLE"), Some(sans));
        assert_eq!(find("serif"), None);
        assert_eq!(find("monospace"), None);
        assert_eq!(find("Not Installed"), None);
    }

    #[test]
    fn catalog_fallbacks_are_linked_then_cascade_at_the_query_weight() {
        let defaults = Defaults {
            sans_serif: &[],
            serif: &[],
            monospace: &[],
            emoji: &[],
            cascade: &["Primary", "Cascade One", "Missing", "Cascade Two"],
        };
        let query = FontQuery::new("Primary").weight(BOLD_WEIGHT);
        let find = |query: &FontQuery| {
            // Every family but "Missing" is installed, at whatever weight it
            // was asked for.
            (query.family != "Missing").then(|| FontMatch {
                weight: query.weight,
                ..found(&query.family, &format!("{}.ttf", query.family), 0)
            })
        };
        let list = defaults.fallbacks(&query, vec![found("Linked", "linked.ttf", 0)], find);
        let names: Vec<_> = list.iter().map(|m| m.family.as_str()).collect();
        assert_eq!(names, ["Linked", "Cascade One", "Cascade Two"]);
        assert!(list[1..].iter().all(|m| m.weight == BOLD_WEIGHT));
    }

    #[test]
    fn system_fonts_can_be_shared_between_threads() {
        fn shared<T: Send + Sync>() {}
        shared::<SystemFonts>();
    }

    #[test]
    fn a_query_builds_as_its_methods_say() {
        let query = FontQuery::new("Arial")
            .weight(BOLD_WEIGHT)
            .style(FontStyle::Oblique);
        assert_eq!(
            query,
            FontQuery {
                family: "Arial".to_owned(),
                weight: BOLD_WEIGHT,
                style: FontStyle::Oblique,
            }
        );
        assert_eq!(FontQuery::new("Arial").weight, NORMAL_WEIGHT);
    }

    #[test]
    fn a_match_reads_its_face_as_a_font_of_its_own() {
        let dir = temp_dir("match-read");
        let ttc = dir.join("pair.ttc");
        std::fs::write(
            &ttc,
            sfnt::tests::collection(&[sfnt::tests::EMOJI_TEXT, SANS]),
        )
        .unwrap();
        let face = FontMatch {
            path: ttc,
            index: 1,
            ..found("Atkinson Hyperlegible", "", 0)
        };
        let bytes = face.read().unwrap();
        let font = skrifa::FontRef::new(&bytes).expect("a standalone face");
        assert_eq!(face::describe(&font).unwrap().1, "Atkinson Hyperlegible");
        assert!(
            FontMatch {
                index: 2,
                ..face.clone()
            }
            .read()
            .is_err()
        );
        assert!(
            FontMatch {
                path: dir.join("gone.ttf"),
                ..face
            }
            .read()
            .is_err()
        );
    }
}
