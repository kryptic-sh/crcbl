//! Per-cluster font fallback: which face of a [`Font`] and its fallback faces
//! draws each cluster.
//!
//! # The rule
//!
//! A font handle carries an ordered list of fallback faces
//! ([`Font::with_fallback`]; a tree's come from
//! [`Ui::set_fallback_fonts`](crate::tree::Ui::set_fallback_fonts)). Text is
//! cut into clusters by [`cluster`](super::cluster)'s rule, and a cluster is
//! drawn by **the first face whose cmap maps every char of it** — the
//! handle's own face first, then each fallback face in order — the chars
//! [`is_invisible`] names left out of the count. When no face maps all of it,
//! the first face that maps its base char draws it, and when none maps even
//! that, the handle's own face draws it, as `.notdef`. A whole cluster goes to
//! one face so that face's GSUB can join it: a ZWJ sequence, a flag's two
//! regional indicators, a keycap. Layout, measurement, caret stops and
//! drawing all ask [`cluster_face`], through [`shape`], so they
//! cannot disagree about which face a glyph came from or how wide it is.
//!
//! # Presentation selectors
//!
//! A cluster whose second char is U+FE0F (VARIATION SELECTOR-16) asks for
//! **emoji presentation**: of the faces that map it, the first that draws a
//! colour glyph ([`Font::has_color_glyph`]) for the cluster shaped in it
//! alone draws it, and only when none does the first that maps it at all.
//! Shaped, not the base char's own glyph: Segoe UI Emoji's digit one is a
//! plain outline, and only its GSUB ligature of `1`, VS16 and the keycap is
//! in colour. U+FE0E (VARIATION SELECTOR-15) asks for **text presentation**
//! the same way round: the first face that draws no colour glyph for it.
//! Only a cluster with a selector is shaped to choose. Neither selector draws
//! anything or takes any width, and neither does any other variation selector
//! or a zero-width joiner: the shaper removes them.

use super::{Font, GlyphId, shape};

/// U+FE0E VARIATION SELECTOR-15: text presentation.
pub const TEXT_PRESENTATION: char = '\u{FE0E}';

/// U+FE0F VARIATION SELECTOR-16: emoji presentation.
pub const EMOJI_PRESENTATION: char = '\u{FE0F}';

/// U+200D ZERO WIDTH JOINER, which joins emoji into a ZWJ sequence.
pub const ZERO_WIDTH_JOINER: char = '\u{200D}';

/// The most fallback faces a handle carries: a face's index has to fit the
/// `u8` a laid-out glyph records it in, and index zero is the handle's own.
pub const MAX_FALLBACK_FACES: usize = u8::MAX as usize;

/// Whether `c` draws nothing and takes no width whatever font it is in: a
/// variation selector (U+FE00 to U+FE0F) or a zero-width joiner.
#[must_use]
pub fn is_invisible(c: char) -> bool {
    matches!(c, '\u{FE00}'..='\u{FE0F}' | ZERO_WIDTH_JOINER)
}

/// Which face of `font` draws `cluster` — zero for the handle's own, `n` for
/// its `n`th fallback face, as [`Font::face`] numbers them. See the module
/// docs for the rule.
#[must_use]
pub fn cluster_face(font: &Font, cluster: &str) -> u8 {
    let mut chars = cluster.chars();
    let Some(base) = chars.next() else {
        return 0;
    };
    let presentation = chars
        .next()
        .filter(|&c| c == EMOJI_PRESENTATION || c == TEXT_PRESENTATION);
    let faces = core::iter::once(font).chain(font.fallback());
    // The first face that maps every char, and the first that maps the base.
    let (mut first, mut base_face) = (None, None);
    for (index, face) in faces.enumerate() {
        // At most `MAX_FALLBACK_FACES` fallback faces, so the index fits.
        let index = index as u8;
        if base_face.is_none() && face.glyph_id(base) != GlyphId::NOTDEF {
            base_face = Some(index);
        }
        let maps_all = cluster
            .chars()
            .filter(|&c| !is_invisible(c))
            .all(|c| face.glyph_id(c) != GlyphId::NOTDEF);
        if !maps_all {
            continue;
        }
        let wanted = match presentation {
            Some(EMOJI_PRESENTATION) => shape::shapes_to_colour(face, cluster),
            Some(_) => !shape::shapes_to_colour(face, cluster),
            None => return index,
        };
        if wanted {
            return index;
        }
        first.get_or_insert(index);
    }
    first.or(base_face).unwrap_or(0)
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

    /// The emoji fixtures' bytes, for faces made lazily from them.
    const CBDT: &[u8] = include_bytes!("../../tests/fixtures/emoji/cbdt.ttf");
    const COLRV0: &[u8] = include_bytes!("../../tests/fixtures/emoji/colrv0.ttf");

    /// **A lazy face is read only when a cluster misses every face before
    /// it**, once whichever handle asks, and then answers as the face its
    /// bytes hold.
    #[test]
    fn a_lazy_face_is_read_only_when_every_face_before_it_misses() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let reads = Arc::new(AtomicUsize::new(0));
        let lazy = |bytes: &'static [u8]| {
            let reads = Arc::clone(&reads);
            Font::lazy(move || {
                reads.fetch_add(1, Ordering::Relaxed);
                Ok::<_, String>(bytes)
            })
        };
        let (cbdt, colr) = (lazy(CBDT), lazy(COLRV0));
        let font = latin1().with_fallback(&[cbdt.clone(), colr.clone()]);
        assert_ne!(cbdt.id(), colr.id(), "each has its identity unread");
        assert_eq!(cluster_face(&font, "a"), 0);
        assert!(!cbdt.is_loaded() && !colr.is_loaded());
        // The first fallback face maps U+1F600: the second is not reached.
        assert_eq!(cluster_face(&font, "\u{1F600}"), 1);
        assert!(cbdt.is_loaded() && !colr.is_loaded());
        // Only the second maps U+1F601.
        assert_eq!(cluster_face(&font, "\u{1F601}"), 2);
        assert!(colr.is_loaded());
        assert_eq!(reads.load(Ordering::Relaxed), 2);
        assert_eq!(
            font.face(1).glyph_id('\u{1F600}'),
            fixtures::cbdt().glyph_id('\u{1F600}')
        );
    }

    /// **A lazy face that cannot be read maps no char**, so fallback goes
    /// past it to the next face, as it does past a face lacking the glyph.
    #[test]
    fn a_lazy_face_that_cannot_be_read_is_passed_over() {
        let gone = Font::lazy(|| Err::<&'static [u8], _>("the file went"));
        let garbage = Font::lazy(|| Ok::<_, String>(b"not a font".as_slice()));
        let font = latin1().with_fallback(&[gone.clone(), garbage.clone(), fixtures::cbdt()]);
        assert_eq!(cluster_face(&font, "\u{1F600}"), 3);
        for face in [&gone, &garbage] {
            assert!(face.is_loaded());
            assert_eq!(face.glyph_id('\u{1F600}'), GlyphId::NOTDEF);
            assert_eq!(face.glyph_id('a'), GlyphId::NOTDEF);
        }
    }

    /// **A lone char goes to the first face that maps it**: the handle's own
    /// face for what it has, then the fallback faces in order, and the
    /// handle's own face when none maps it.
    #[test]
    fn a_char_is_drawn_by_the_first_face_that_maps_it() {
        let (cbdt, colr) = (fixtures::cbdt(), fixtures::colrv0());
        let font = latin1().with_fallback(&[cbdt.clone(), colr.clone()]);
        assert_eq!(cluster_face(&font, "a"), 0);
        // Both fallback faces map U+1F600; the first listed wins.
        assert_eq!(cluster_face(&font, "\u{1F600}"), 1);
        assert_eq!(font.face(1), &cbdt);
        // Only the second maps U+1F601.
        assert_eq!(cluster_face(&font, "\u{1F601}"), 2);
        assert_eq!(font.face(2), &colr);
        // Nobody maps U+4E2D.
        assert_eq!(cluster_face(&font, "\u{4E2D}"), 0);
        // Order is the caller's: listed the other way, the COLR face wins.
        let swapped = latin1().with_fallback(&[colr.clone(), cbdt]);
        assert_eq!(cluster_face(&swapped, "\u{1F600}"), 1);
        assert_eq!(swapped.face(1), &colr);
    }

    /// **A cluster goes wholly to the first face that maps every char of
    /// it**, past an earlier face that maps only its base; with no face
    /// mapping all of it, to the first that maps its base. The joiner is not
    /// counted.
    #[test]
    fn a_cluster_goes_to_the_first_face_that_maps_all_of_it() {
        let (cbdt, shaped) = (fixtures::cbdt(), fixtures::shaped());
        let font = latin1().with_fallback(&[cbdt.clone(), shaped.clone()]);
        // CBDT maps the heart and not the fire; the shaped face maps both.
        assert_ne!(cbdt.glyph_id('\u{2764}'), GlyphId::NOTDEF);
        assert_eq!(cbdt.glyph_id('\u{1F525}'), GlyphId::NOTDEF);
        assert_eq!(cluster_face(&font, "\u{2764}"), 1);
        assert_eq!(cluster_face(&font, "\u{2764}\u{200D}\u{1F525}"), 2);
        // VS16 asks for colour, which neither face's heart has: the face
        // that maps every char still wins over the one that maps the heart.
        assert_eq!(cluster_face(&font, "\u{2764}\u{FE0F}\u{200D}\u{1F525}"), 2);
        // Nobody maps the heart with an Han char after it: the base's face.
        assert_eq!(cluster_face(&font, "\u{2764}\u{4E2D}"), 1);
        assert_eq!(cluster_face(&font, ""), 0);
        // A face that lacks the joiner itself still takes the cluster.
        let without = latin1().with_fallback(&[cbdt, fixtures::colrv0()]);
        assert_eq!(cluster_face(&without, "\u{1F600}\u{200D}\u{1F601}"), 2);
    }

    /// **VS16 prefers a colour glyph and VS15 a text one**, past a face that
    /// maps the char the other way; with no face of the kind asked for, the
    /// first face that maps it still draws it.
    #[test]
    fn presentation_selectors_choose_colour_or_text() {
        let (text, cbdt) = (fixtures::text(), fixtures::cbdt());
        let colour = "\u{2764}\u{FE0F}";
        let plain = "\u{2764}\u{FE0E}";
        // The text face first: it draws the heart unless VS16 asks for colour.
        let font = text.with_fallback(core::slice::from_ref(&cbdt));
        assert_eq!(cluster_face(&font, "\u{2764}"), 0);
        assert_eq!(cluster_face(&font, plain), 0);
        let face = cluster_face(&font, colour);
        assert_eq!(face, 1);
        assert!(
            font.face(face)
                .has_color_glyph(font.face(face).glyph_id('\u{2764}'))
        );
        // The colour face first: VS15 goes past it to the text face.
        let font = cbdt.with_fallback(&[text]);
        assert_eq!(cluster_face(&font, "\u{2764}"), 0);
        assert_eq!(cluster_face(&font, plain), 1);
        // No text face at all: VS15 still draws the colour glyph.
        assert_eq!(cluster_face(&cbdt, plain), 0);
        for invisible in [EMOJI_PRESENTATION, TEXT_PRESENTATION, ZERO_WIDTH_JOINER] {
            assert!(is_invisible(invisible), "{invisible:?}");
        }
        assert!(!is_invisible('a'));
    }

    /// **VS16 counts a colour glyph the cluster shapes into, not only the
    /// base's own**: the text face maps the digit and the keycap as plain
    /// outlines, and the shaped face's digit is plain too, but its GSUB
    /// ligature of the three is in colour, so VS16 goes past the text face.
    /// Without a selector the first face that maps it all keeps it, and VS15
    /// goes past the ligature's face.
    #[test]
    fn presentation_counts_a_colour_ligature() {
        let (text, shaped) = (fixtures::text(), fixtures::shaped());
        assert!(!shaped.has_color_glyph(shaped.glyph_id('1')));
        let keycap = "1\u{FE0F}\u{20E3}";
        let font = text.clone().with_fallback(core::slice::from_ref(&shaped));
        assert_eq!(cluster_face(&font, keycap), 1);
        assert_eq!(cluster_face(&font, "1\u{20E3}"), 0);
        let mut glyphs = Vec::new();
        shape::shape(&font, 16.0, keycap, 0..keycap.len(), false, &mut glyphs);
        assert_eq!(glyphs.len(), 1, "one ligature: {glyphs:?}");
        assert_eq!(glyphs[0].face, 1);
        assert!(shaped.has_color_glyph(glyphs[0].glyph));
        let font = shaped.with_fallback(&[text]);
        assert_eq!(cluster_face(&font, keycap), 0);
        assert_eq!(cluster_face(&font, "1\u{FE0E}\u{20E3}"), 1);
    }
}
