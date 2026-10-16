//! macOS: a walk of the system's font directories, matched by name table —
//! [`Source::Scan`].
//!
//! `/System/Library/Fonts` (with its `Supplemental`), `/Library/Fonts` and
//! `~/Library/Fonts` are walked once per
//! [`SystemFonts`](crate::SystemFonts), on its first lookup.
//!
//! CoreText is the better source and the future one: `CTFontDescriptor`
//! matching knows the fonts macOS downloads on demand into its asset store,
//! which no directory here holds, and
//! `CTFontCopyDefaultCascadeListForLanguages` is the system's own fallback
//! order where [`DEFAULTS`] is a fixed one.

use std::path::PathBuf;

use crate::scan::Catalog;
use crate::{Defaults, FontMatch, FontQuery, Source};

const DEFAULTS: Defaults = Defaults {
    sans_serif: &["Helvetica", "Helvetica Neue"],
    serif: &["Times", "Times New Roman"],
    monospace: &["Menlo", "Courier"],
    emoji: &["Apple Color Emoji"],
    cascade: &[
        "System Font",
        ".SF NS",
        "Helvetica Neue",
        "Apple Color Emoji",
        "PingFang SC",
        "Hiragino Sans",
        "Apple SD Gothic Neo",
        "Apple Symbols",
    ],
};

/// The directories walked; see the module docs.
fn dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/System/Library/Fonts"),
        PathBuf::from("/System/Library/Fonts/Supplemental"),
        PathBuf::from("/Library/Fonts"),
    ];
    dirs.extend(std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Fonts")));
    dirs
}

#[derive(Debug)]
pub(crate) struct Platform {
    catalog: Catalog,
}

impl Platform {
    pub fn new() -> Self {
        Self {
            catalog: Catalog::new(dirs()),
        }
    }

    pub fn source(&self) -> Source {
        Source::Scan
    }

    pub fn find(&self, query: &FontQuery) -> Option<FontMatch> {
        DEFAULTS.find(query, |family| self.catalog.faces_of(family))
    }

    pub fn fallbacks(&self, query: &FontQuery) -> Vec<FontMatch> {
        DEFAULTS.fallbacks(query, Vec::new(), |query| self.find(query))
    }
}
