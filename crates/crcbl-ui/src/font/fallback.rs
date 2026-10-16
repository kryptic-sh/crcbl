//! Per-glyph font fallback: which face of a [`Font`] and its fallback faces
//! draws each char.
//!
//! # The rule
//!
//! A font handle carries an ordered list of fallback faces
//! ([`Font::with_fallback`]; a tree's come from
//! [`Ui::set_fallback_fonts`](crate::tree::Ui::set_fallback_fonts)). A char is
//! drawn by **the first face whose cmap maps it** — the handle's own face
//! first, then each fallback face in order — and by the handle's own face as
//! `.notdef` when none does. Layout, measurement, caret stops and drawing all
//! ask [`map_char`], so they cannot disagree about which face a glyph came
//! from or how wide it is.
//!
//! # Presentation selectors
//!
//! A char followed by U+FE0F (VARIATION SELECTOR-16) asks for **emoji
//! presentation**: the first face whose glyph for it is a colour glyph
//! ([`Font::has_color_glyph`]) draws it, and only when no face has one the
//! first face that maps it at all. U+FE0E (VARIATION SELECTOR-15) asks for
//! **text presentation** the same way round: the first face whose glyph is not
//! a colour glyph. Neither selector draws anything or takes any width, and
//! neither does any other variation selector or a zero-width joiner, which
//! [`is_invisible`] names.
//!
//! # Clusters
//!
//! There is no shaping: a ZWJ sequence (`👩‍💻`), a skin-tone modifier after its
//! base and a flag's two regional indicators are not joined by the font's
//! GSUB ligatures, so each component draws as its own glyph — the joiner
//! itself drawing nothing.

use super::{Font, GlyphId};

/// U+FE0E VARIATION SELECTOR-15: text presentation.
pub const TEXT_PRESENTATION: char = '\u{FE0E}';

/// U+FE0F VARIATION SELECTOR-16: emoji presentation.
pub const EMOJI_PRESENTATION: char = '\u{FE0F}';

/// U+200D ZERO WIDTH JOINER, which joins emoji into a ZWJ sequence.
pub const ZERO_WIDTH_JOINER: char = '\u{200D}';

/// The most fallback faces a handle carries: a face's index has to fit the
/// `u8` a laid-out glyph records it in, and index zero is the handle's own.
pub const MAX_FALLBACK_FACES: usize = u8::MAX as usize;

/// One char as a font and its fallback faces draw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapped {
    /// Which face: zero for the handle's own, `n` for its `n`th fallback face
    /// — see [`Font::face`].
    pub face: u8,
    /// The glyph, in that face.
    pub glyph: GlyphId,
}

/// Whether `c` draws nothing and takes no width whatever font it is in: a
/// variation selector (U+FE00 to U+FE0F) or a zero-width joiner.
#[must_use]
pub fn is_invisible(c: char) -> bool {
    matches!(c, '\u{FE00}'..='\u{FE0F}' | ZERO_WIDTH_JOINER)
}

/// What draws `c` in `font` and its fallback faces when `next` follows it —
/// see the module docs — or `None` for a char that draws nothing.
#[must_use]
pub fn map_char(font: &Font, c: char, next: Option<char>) -> Option<Mapped> {
    if is_invisible(c) {
        return None;
    }
    let faces = core::iter::once(font).chain(font.fallback());
    let mut first = None;
    for (index, face) in faces.enumerate() {
        let glyph = face.glyph_id(c);
        if glyph == GlyphId::NOTDEF {
            continue;
        }
        // At most `MAX_FALLBACK_FACES` fallback faces, so the index fits.
        let mapped = Mapped {
            face: index as u8,
            glyph,
        };
        let wanted = match next {
            Some(EMOJI_PRESENTATION) => face.has_color_glyph(glyph),
            Some(TEXT_PRESENTATION) => !face.has_color_glyph(glyph),
            _ => return Some(mapped),
        };
        if wanted {
            return Some(mapped);
        }
        first.get_or_insert(mapped);
    }
    Some(first.unwrap_or(Mapped {
        face: 0,
        glyph: GlyphId::NOTDEF,
    }))
}

/// Every char of `text` with what draws it: its byte offset, the char, and
/// [`map_char`]'s answer given the char after it.
pub fn map_text<'a>(
    font: &'a Font,
    text: &'a str,
) -> impl Iterator<Item = (usize, char, Option<Mapped>)> + 'a {
    let mut chars = text.char_indices().peekable();
    core::iter::from_fn(move || {
        let (byte, c) = chars.next()?;
        let next = chars.peek().map(|&(_, next)| next);
        Some((byte, c, map_char(font, c, next)))
    })
}

#[cfg(all(test, feature = "parsed-font"))]
mod tests {
    use super::*;
    use crate::font::fixtures;

    /// A font mapping every Latin-1 codepoint and nothing past it, so
    /// anything past Latin-1 falls back.
    fn latin1() -> Font {
        Font::sans().clone()
    }

    /// **A char goes to the first face that maps it**: the handle's own face
    /// for what it has, then the fallback faces in order, and `.notdef` in the
    /// handle's own face when none maps it.
    #[test]
    fn a_char_is_drawn_by_the_first_face_that_maps_it() {
        let (cbdt, colr) = (fixtures::cbdt(), fixtures::colrv0());
        let font = latin1().with_fallback(&[cbdt.clone(), colr.clone()]);
        assert_eq!(
            map_char(&font, 'a', None),
            Some(Mapped {
                face: 0,
                glyph: font.glyph_id('a')
            })
        );
        // Both fallback faces map U+1F600; the first listed wins.
        let smile = map_char(&font, '\u{1F600}', None).expect("drawn");
        assert_eq!(smile.face, 1);
        assert_eq!(smile.glyph, cbdt.glyph_id('\u{1F600}'));
        assert_eq!(font.face(smile.face), &cbdt);
        // Only the second maps U+1F601.
        let many = map_char(&font, '\u{1F601}', None).expect("drawn");
        assert_eq!((many.face, many.glyph), (2, colr.glyph_id('\u{1F601}')));
        assert_eq!(font.face(2), &colr);
        // Nobody maps U+4E2D.
        assert_eq!(
            map_char(&font, '\u{4E2D}', None),
            Some(Mapped {
                face: 0,
                glyph: GlyphId::NOTDEF
            })
        );
        // Order is the caller's: listed the other way, the COLR face wins.
        let swapped = latin1().with_fallback(&[colr.clone(), cbdt]);
        assert_eq!(
            map_char(&swapped, '\u{1F600}', None).map(|m| m.face),
            Some(1)
        );
        assert_eq!(swapped.face(1), &colr);
    }

    /// **VS16 prefers a colour glyph and VS15 a text one**, past a face that
    /// maps the char the other way; with no face of the kind asked for, the
    /// first face that maps it still draws it. Neither selector draws.
    #[test]
    fn presentation_selectors_choose_colour_or_text() {
        let (text, cbdt) = (fixtures::text(), fixtures::cbdt());
        let heart = '\u{2764}';
        // The text face first: it draws the heart unless VS16 asks for colour.
        let font = text.with_fallback(core::slice::from_ref(&cbdt));
        assert_eq!(map_char(&font, heart, None).map(|m| m.face), Some(0));
        assert_eq!(
            map_char(&font, heart, Some(TEXT_PRESENTATION)).map(|m| m.face),
            Some(0)
        );
        let colour = map_char(&font, heart, Some(EMOJI_PRESENTATION)).expect("drawn");
        assert_eq!(colour.face, 1);
        assert!(font.face(colour.face).has_color_glyph(colour.glyph));
        // The colour face first: VS15 goes past it to the text face.
        let font = cbdt.with_fallback(&[text]);
        assert_eq!(map_char(&font, heart, None).map(|m| m.face), Some(0));
        assert_eq!(
            map_char(&font, heart, Some(TEXT_PRESENTATION)).map(|m| m.face),
            Some(1)
        );
        // No text face at all: VS15 still draws the colour glyph.
        assert_eq!(
            map_char(&cbdt, heart, Some(TEXT_PRESENTATION)).map(|m| m.face),
            Some(0)
        );
        for invisible in [EMOJI_PRESENTATION, TEXT_PRESENTATION, ZERO_WIDTH_JOINER] {
            assert_eq!(map_char(&font, invisible, None), None, "{invisible:?}");
        }
        assert!(!is_invisible('a'));
    }

    /// `map_text` reads each char's successor, so a selector after a char
    /// steers it and is itself skipped.
    #[test]
    fn mapping_text_reads_the_selector_after_each_char() {
        let (text, cbdt) = (fixtures::text(), fixtures::cbdt());
        let font = text.with_fallback(&[cbdt]);
        let mapped: Vec<_> = map_text(&font, "\u{2764}\u{FE0F}\u{2764}")
            .map(|(byte, c, mapped)| (byte, c, mapped.map(|m| m.face)))
            .collect();
        assert_eq!(
            mapped,
            [
                (0, '\u{2764}', Some(1)),
                (3, '\u{FE0F}', None),
                (6, '\u{2764}', Some(0)),
            ]
        );
    }
}
