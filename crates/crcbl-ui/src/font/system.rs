//! Faces looked up on the system by family name, through `crcbl-font`, and
//! parsed: behind the `system-fonts` feature, which turns `parsed-font` on.
//!
//! ```no_run
//! use crcbl_ui::font::system::crcbl_font::{FontQuery, SystemFonts};
//! use crcbl_ui::font::Font;
//! use crcbl_ui::tree::Ui;
//!
//! let fonts = SystemFonts::new();
//! let mut ui = Ui::new();
//! // `font-family: body` now draws in the system's sans-serif.
//! ui.register_system_font("body", &fonts, &FontQuery::new("sans-serif"))?;
//! // Its fallbacks, for the chars that face has no glyph for: each read
//! // only when a char first misses every face before it.
//! let query = FontQuery::new("sans-serif");
//! ui.set_fallback_fonts(fonts.fallbacks(&query).into_iter().map(Font::from_system_lazy));
//! # Ok::<(), crcbl_ui::font::system::SystemFontError>(())
//! ```

use core::fmt;
use std::io;

pub use crcbl_font;
use std::sync::Arc;

use crcbl_font::{FaceBytes, FontMatch, FontQuery, SystemFonts};

use super::{Bytes, Font, FontData, FontError, ReservedFamilyName, is_reserved_family};
use crate::tree::Ui;

/// Why [`Font::from_system`] or [`Ui::register_system_font`] gave no font.
#[derive(Debug)]
pub enum SystemFontError {
    /// The system has no face of the family asked for, which it names.
    NotFound(String),
    /// The face's file could not be read: it went, or changed, after it was
    /// found.
    Read(io::Error),
    /// The face's bytes are not a font [`Font::parse`] reads.
    Parse(FontError),
    /// The name it was to be registered under is reserved; see
    /// [`Ui::register_font`].
    Reserved(ReservedFamilyName),
}

impl fmt::Display for SystemFontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(family) => write!(f, "the system has no font of the family `{family}`"),
            Self::Read(error) => write!(f, "the system font cannot be read: {error}"),
            Self::Parse(error) => write!(f, "the system font does not parse: {error}"),
            Self::Reserved(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SystemFontError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotFound(_) => None,
            Self::Read(error) => Some(error),
            Self::Parse(error) => Some(error),
            Self::Reserved(error) => Some(error),
        }
    }
}

/// A face `crcbl-font` mapped or read, parsed in place: the font and its
/// shaper share it, and a mapping stays mapped for as long as either lives.
impl From<FaceBytes> for FontData {
    fn from(bytes: FaceBytes) -> Self {
        Self(Bytes::Shared(Arc::new(bytes)))
    }
}

impl Font {
    /// The face `face` names, mapped from its file by [`FontMatch::map`] and
    /// parsed in place: a single font of the system's own is the page
    /// cache's pages, not a copy, and a face of a collection is cut out of
    /// it first.
    ///
    /// # Errors
    ///
    /// [`SystemFontError::Read`] when the file cannot be read or holds no
    /// such face, [`SystemFontError::Parse`] when the face does not parse.
    pub fn from_system(face: &FontMatch) -> Result<Self, SystemFontError> {
        let bytes = face.map().map_err(SystemFontError::Read)?;
        Self::parse(bytes).map_err(SystemFontError::Parse)
    }

    /// The face `face` names, mapped and parsed the first time it is asked a
    /// question rather than now: [`Font::lazy`] over [`FontMatch::map`].
    /// What a list of system fallback faces is for, which a launcher would
    /// otherwise read whole before its first frame; see [`Font::lazy`] for
    /// what a face that fails to read answers. Its warning names the file.
    ///
    /// **Asked whether it maps a char, it reads only its `cmap`**
    /// ([`FontMatch::read_cmap`]), and reads the whole face only for a char
    /// the `cmap` maps: a fallback list that a char misses throughout costs
    /// each face's `cmap`, not its file.
    #[must_use]
    pub fn from_system_lazy(face: FontMatch) -> Self {
        let what = format!("the font face {} (#{})", face.path.display(), face.index);
        let cmap = face.clone();
        Self::lazy_with_cmap(
            what,
            move || face.map(),
            move || cmap.read_cmap().map(FaceBytes::from),
        )
    }
}

impl Ui {
    /// Looks `query` up in `fonts` and registers the face found as a face of
    /// the family `name`, as [`Ui::register_font`] does, returning it.
    ///
    /// `name` is what a stylesheet's `font-family` will say, and need not be
    /// the family looked up: a generic family such as `sans-serif` cannot be
    /// registered under its own name, and a stylesheet naming `body` keeps
    /// working whichever face the system chose.
    ///
    /// # Errors
    ///
    /// [`SystemFontError::Reserved`] for a name [`Ui::register_font`]
    /// refuses, before anything is looked up; [`SystemFontError::NotFound`]
    /// when the system has no such family; and [`Font::from_system`]'s.
    pub fn register_system_font(
        &mut self,
        name: &str,
        fonts: &SystemFonts,
        query: &FontQuery,
    ) -> Result<Font, SystemFontError> {
        if is_reserved_family(name) {
            return Err(SystemFontError::Reserved(ReservedFamilyName(
                name.to_owned(),
            )));
        }
        let face = fonts
            .find(query)
            .ok_or_else(|| SystemFontError::NotFound(query.family.clone()))?;
        let font = Font::from_system(&face)?;
        self.register_font(name, &font)
            .map_err(SystemFontError::Reserved)?;
        Ok(font)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// The committed font, as a lookup would report it.
    fn committed(index: u32) -> FontMatch {
        FontMatch {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("fonts")
                .join("AtkinsonHyperlegible-Regular.ttf"),
            index,
            family: "Atkinson Hyperlegible".to_owned(),
            weight: 400,
            style: crcbl_font::FontStyle::Normal,
        }
    }

    #[test]
    fn a_found_face_parses_as_the_font_its_file_holds() {
        let font = Font::from_system(&committed(0)).expect("the committed font parses");
        let compiled = Font::parse(crate::font::SANS_TTF).expect("parses");
        assert_eq!(font.metrics(), compiled.metrics());
        assert_eq!(font.weight(), 400.0);
        assert!(matches!(
            Font::from_system(&committed(1)),
            Err(SystemFontError::Read(_))
        ));
        let gone = FontMatch {
            path: PathBuf::from("no such font file.ttf"),
            ..committed(0)
        };
        assert!(matches!(
            Font::from_system(&gone),
            Err(SystemFontError::Read(_))
        ));
    }

    /// **A system face made lazily is read on first use**, as the face its
    /// file holds; one whose file went maps no char, and the warning it logs
    /// names the file, since the caller no longer sees the failure itself.
    #[test]
    fn a_lazy_system_face_reads_on_first_use_and_names_its_file() {
        let font = Font::from_system_lazy(committed(0));
        assert!(!font.is_loaded());
        let compiled = Font::parse(crate::font::SANS_TTF).expect("parses");
        assert_eq!(font.glyph_id('a'), compiled.glyph_id('a'));
        assert!(font.is_loaded());

        let gone = Font::from_system_lazy(FontMatch {
            path: PathBuf::from("no such font file.ttf"),
            ..committed(0)
        });
        assert!(gone.source.what.contains("no such font file.ttf"));
        assert_eq!(gone.glyph_id('a'), crate::font::GlyphId::NOTDEF);
    }

    /// **A char the face's `cmap` does not map is answered without reading
    /// the face**, as a fallback face missed by a whole run of text is: in
    /// Latin-1 and past it, and for a char it does map the face is read and
    /// answers as itself.
    #[test]
    fn a_lazy_system_face_misses_from_its_cmap_alone() {
        let font = Font::from_system_lazy(committed(0));
        let compiled = Font::parse(crate::font::SANS_TTF).expect("parses");
        for missing in ['\u{1}', '\u{4E00}', '\u{F0001}'] {
            assert_eq!(compiled.glyph_id(missing), crate::font::GlyphId::NOTDEF);
            assert_eq!(font.glyph_id(missing), crate::font::GlyphId::NOTDEF);
        }
        assert!(!font.is_loaded(), "a miss read the whole face");
        assert_ne!(compiled.glyph_id('\u{2014}'), crate::font::GlyphId::NOTDEF);
        assert_eq!(font.glyph_id('\u{2014}'), compiled.glyph_id('\u{2014}'));
        assert!(font.is_loaded());

        // A face whose `cmap` cannot be read is read whole to answer.
        let unreadable = Font::lazy_with_cmap(
            "a face with an unreadable cmap".to_owned(),
            || Ok::<_, String>(crate::font::SANS_TTF),
            || Err("no cmap".to_owned()),
        );
        assert_eq!(
            unreadable.glyph_id('\u{4E00}'),
            crate::font::GlyphId::NOTDEF
        );
        assert!(unreadable.is_loaded());
    }

    #[test]
    fn a_reserved_name_is_refused_before_the_lookup_and_a_miss_is_not_found() {
        let fonts = SystemFonts::new();
        let mut ui = Ui::new();
        let query = FontQuery::new("sans-serif");
        assert!(matches!(
            ui.register_system_font("serif", &fonts, &query),
            Err(SystemFontError::Reserved(ReservedFamilyName(name))) if name == "serif"
        ));
        let missing = FontQuery::new("No Such Family Zq7");
        assert!(matches!(
            ui.register_system_font("body", &fonts, &missing),
            Err(SystemFontError::NotFound(family)) if family == "No Such Family Zq7"
        ));
    }

    /// **A system face mapped by [`Font::from_system`] draws as the face
    /// read whole**: the same bytes, metrics, glyphs, advances and masks.
    /// On Linux a mapped face's bytes are shown to be the file's own pages,
    /// by the process's memory map, and a single font of the system's font
    /// directory is shown to be mapped rather than read.
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_mapped_system_face_draws_as_the_face_read_whole() {
        use crate::font::atlas::GlyphAtlas;

        /// What `font` rasterises `c` to at 32px, mask and placement.
        fn mask(font: &Font, c: char) -> (Vec<u8>, i32, i32) {
            let mut atlas = GlyphAtlas::new(256, 1, 100);
            atlas.begin_frame();
            let at = atlas.glyph(font, font.glyph_id(c), 32.0, 0).expect("room");
            let rect = crate::image::TexelRect {
                x: at.x,
                y: at.y,
                width: at.width,
                height: at.height,
            };
            (atlas.region(at.page as usize, rect), at.left, at.top)
        }

        let found = SystemFonts::new()
            .find(&FontQuery::new("sans-serif"))
            .expect("the system has a sans-serif");
        let mapped = Font::from_system(&found).expect("the face maps and parses");
        let read = Font::parse(found.read().expect("the face reads")).expect("parses");
        assert_eq!(mapped.data(), read.data());
        assert_eq!(mapped.metrics(), read.metrics());
        assert_eq!(mapped.glyph_count(), read.glyph_count());
        let mut inked = 0;
        for c in ['A', 'g', 'W', '&', '7'] {
            let glyph = read.glyph_id(c);
            assert_eq!(mapped.glyph_id(c), glyph, "{c:?}");
            assert_eq!(mapped.advance(glyph), read.advance(glyph), "{c:?}");
            let drawn = mask(&mapped, c);
            assert_eq!(drawn, mask(&read, c), "{c:?}");
            inked += usize::from(drawn.0.iter().any(|&texel| texel > 0));
        }
        assert!(inked > 0, "no glyph drew any ink, so no mask was compared");

        #[cfg(target_os = "linux")]
        {
            let single = found.read().unwrap() == std::fs::read(&found.path).unwrap();
            if single {
                // A single face maps exactly when root alone may write its
                // file. Not "every file under /usr/share/fonts": the GitHub
                // Ubuntu image ships DejaVu Sans world-writable (mode 0777),
                // and that one must be read.
                use std::os::unix::fs::MetadataExt;
                let metadata = std::fs::metadata(&found.path).unwrap();
                let root_alone_writes = metadata.uid() == 0 && metadata.mode() & 0o022 == 0;
                assert_eq!(
                    found.map().unwrap().is_mapped(),
                    root_alone_writes,
                    "{found:?}: uid {} mode {:o}",
                    metadata.uid(),
                    metadata.mode(),
                );
            }
            if !found.map().unwrap().is_mapped() {
                eprintln!(
                    "skipped: {} is read, not mapped, so the memory map is not checked",
                    found.path.display()
                );
            } else {
                let file = std::fs::canonicalize(&found.path).unwrap();
                let at = mapped.data().as_ptr() as usize;
                let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
                let of_file = maps.lines().any(|line| {
                    let (range, path) = line.split_at(line.find('/').unwrap_or(line.len()));
                    let (start, end) = range
                        .split_whitespace()
                        .next()
                        .and_then(|range| range.split_once('-'))
                        .unwrap();
                    let start = usize::from_str_radix(start, 16).unwrap();
                    let end = usize::from_str_radix(end, 16).unwrap();
                    std::path::Path::new(path) == file && (start..end).contains(&at)
                });
                assert!(of_file, "{} is not mapped at {at:#x}", file.display());
            }
        }
    }

    /// The platforms whose CI image has a sans-serif this crate can read:
    /// the lookup, the read, the parse and the registration all run for
    /// real. Drawing a registered font is `tree::font_tests`' business.
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_systems_sans_serif_registers_under_a_name_of_its_own() {
        let fonts = SystemFonts::new();
        let query = FontQuery::new("sans-serif");
        let found = fonts.find(&query).expect("the system has a sans-serif");
        let mut ui = Ui::new();
        let font = ui
            .register_system_font("body", &fonts, &query)
            .expect("the system's sans-serif registers");
        assert_eq!(font.weight(), f32::from(found.weight));
        assert_eq!(
            font.is_italic(),
            found.style != crcbl_font::FontStyle::Normal
        );
    }
}
