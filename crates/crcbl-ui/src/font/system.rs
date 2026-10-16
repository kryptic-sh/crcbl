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
//! // Its fallbacks, for the chars that face has no glyph for.
//! let query = FontQuery::new("sans-serif");
//! ui.set_fallback_fonts(
//!     fonts.fallbacks(&query).iter().filter_map(|face| Font::from_system(face).ok()),
//! );
//! # Ok::<(), crcbl_ui::font::system::SystemFontError>(())
//! ```

use core::fmt;
use std::io;

pub use crcbl_font;
use crcbl_font::{FontMatch, FontQuery, SystemFonts};

use super::{Font, FontError, ReservedFamilyName, is_reserved_family};
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

impl Font {
    /// The face `face` names, read from its file and parsed: a face of a
    /// collection is cut out of it first, by [`FontMatch::read`].
    ///
    /// # Errors
    ///
    /// [`SystemFontError::Read`] when the file cannot be read or holds no
    /// such face, [`SystemFontError::Parse`] when the face does not parse.
    pub fn from_system(face: &FontMatch) -> Result<Self, SystemFontError> {
        let bytes = face.read().map_err(SystemFontError::Read)?;
        Self::parse(bytes).map_err(SystemFontError::Parse)
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
