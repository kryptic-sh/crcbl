//! Shaping: text to positioned glyphs, through `harfrust`.
//!
//! [`shape`] cuts a stretch of text into [`cluster`]s, asks
//! [`fallback::cluster_face`] which face draws each, and shapes every
//! maximal run of clusters one face draws with `harfrust` — HarfBuzz's Rust
//! port — in that face. GSUB and GPOS apply as the face writes them: kerning,
//! mark positioning, ligatures, contextual forms, complex scripts' reordering,
//! emoji ZWJ sequences and flags. What comes back is one [`ShapedGlyph`] per
//! glyph, in pixels at the size asked for, each naming the face that draws
//! it and the cluster of the text it came from.
//!
//! # What a run is shaped with
//!
//! * **Left to right, always.** The script is `harfrust`'s guess from the
//!   run's chars (`Buffer::guess_segment_properties`), and the language is
//!   unset. A right-to-left run is shaped — Arabic joins — but laid out in
//!   logical order, left to right: there is no bidi.
//! * **The text around the run is its context**, so a contextual lookup sees
//!   past the run's edges; nothing outside the run is shaped.
//! * **Default-ignorable chars are removed** — the zero-width joiner and the
//!   variation selectors among them — once GSUB has seen them, so they draw
//!   nothing and take no width.
//! * **Clusters are monotone graphemes** (`harfrust`'s default), so a glyph's
//!   cluster never comes before the one of the glyph before it.
//! * **A synthesised bold** ([`face::Synthesis`](super::face::Synthesis))
//!   widens each glyph that advances at all by
//!   [`face::SYNTHETIC_BOLD_EM`](super::face::SYNTHETIC_BOLD_EM); a mark with
//!   no advance stays where the font put it.

use core::ops::Range;

use glam::Vec2;

use super::{Font, GlyphId, cluster, fallback};

/// One glyph as [`shape`] placed it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Which face draws it, as [`Font::face`] numbers them.
    pub face: u8,
    /// The glyph, in that face.
    pub glyph: GlyphId,
    /// The byte offset in the text of the first char of its cluster.
    pub cluster: usize,
    /// How far the pen moves after it, in pixels.
    pub advance: f32,
    /// Where it is drawn from the pen, in pixels: x right and y **down**, as
    /// the screen runs.
    pub offset: Vec2,
}

/// Appends to `out` the glyphs `range` of `text` shapes into in `font` — and
/// the fallback faces it carries — at `size` pixels per em, in order. The
/// text outside `range` is context only; every glyph's
/// [`cluster`](ShapedGlyph::cluster) is a byte offset in `text`. See the
/// module docs.
///
/// # Panics
///
/// If `range` is not a char-boundary range of `text`, or `text` is longer
/// than `u32::MAX` bytes, which `harfrust` numbers clusters in.
pub fn shape(font: &Font, size: f32, text: &str, range: Range<usize>, out: &mut Vec<ShapedGlyph>) {
    let mut run: Option<(u8, Range<usize>)> = None;
    for cluster in cluster::clusters(&text[range.clone()]) {
        let bytes = range.start + cluster.start..range.start + cluster.end;
        let face = fallback::cluster_face(font, &text[bytes.clone()]);
        match &mut run {
            Some((current, run)) if *current == face => run.end = bytes.end,
            _ => {
                if let Some((face, run)) = run.take() {
                    shape_run(font, face, size, text, run, out);
                }
                run = Some((face, bytes));
            }
        }
    }
    if let Some((face, run)) = run {
        shape_run(font, face, size, text, run, out);
    }
}

/// Without the `parsed-font` feature there is no face to shape in.
#[cfg(not(feature = "parsed-font"))]
fn shape_run(
    font: &Font,
    face: u8,
    _size: f32,
    _text: &str,
    _range: Range<usize>,
    _out: &mut Vec<ShapedGlyph>,
) {
    match font.face(face).parsed.never {}
}

/// Appends `range` of `text` shaped in `font`'s face `face` at `size`.
#[cfg(feature = "parsed-font")]
fn shape_run(
    font: &Font,
    face: u8,
    size: f32,
    text: &str,
    range: Range<usize>,
    out: &mut Vec<ShapedGlyph>,
) {
    let drawn = font.face(face);
    let metrics = drawn.metrics();
    let scale = metrics.scale(size);
    let bold = if drawn.synthesis().bold {
        f32::from(metrics.units_per_em) * super::face::SYNTHETIC_BOLD_EM
    } else {
        0.0
    };
    let Some(shaper) = &drawn.parsed.shaper else {
        nominal(drawn, face, scale, text, range, out);
        return;
    };
    let buffer = run_shaper(shaper, text, range);
    for (info, position) in buffer.glyph_infos().iter().zip(buffer.glyph_positions()) {
        let mut advance = position.x_advance as f32;
        if position.x_advance != 0 {
            advance += bold;
        }
        out.push(ShapedGlyph {
            face,
            glyph: GlyphId(info.glyph_id),
            cluster: info.cluster as usize,
            advance: advance * scale,
            offset: Vec2::new(
                position.x_offset as f32 * scale,
                -(position.y_offset as f32) * scale,
            ),
        });
    }
}

/// `range` of `text` shaped by `shaper`, as the module docs say a run is.
#[cfg(feature = "parsed-font")]
fn run_shaper(shaper: &harfrust::Font, text: &str, range: Range<usize>) -> harfrust::Buffer {
    let mut buffer = harfrust::Buffer::new();
    for (at, c) in text[range.clone()].char_indices() {
        let cluster = u32::try_from(range.start + at)
            .expect("a text no longer than `u32::MAX` bytes, as `shape` documents");
        buffer.push(u32::from(c), cluster);
    }
    buffer.set_pre_context(&text[..range.start]);
    buffer.set_post_context(&text[range.end..]);
    buffer.set_direction(harfrust::Direction::LeftToRight);
    buffer.guess_segment_properties();
    buffer.set_flags(harfrust::BufferFlags::REMOVE_DEFAULT_IGNORABLES);
    harfrust::shape(
        &harfrust::ShaperFont::new(shaper),
        &mut buffer,
        harfrust::ShapeOptions::new(),
    )
    .expect("a fresh buffer with its direction set is everything shaping asks for");
    buffer
}

/// Whether `face` draws any of `cluster` as a colour glyph once the cluster
/// is shaped in it alone — so a keycap or a flag whose parts are plain
/// outlines, and which only a GSUB ligature colours, counts as colour.
#[cfg(feature = "parsed-font")]
pub(super) fn shapes_to_colour(face: &Font, cluster: &str) -> bool {
    match &face.parsed.shaper {
        Some(shaper) => run_shaper(shaper, cluster, 0..cluster.len())
            .glyph_infos()
            .iter()
            .any(|info| face.has_color_glyph(GlyphId(info.glyph_id))),
        None => cluster
            .chars()
            .any(|c| face.has_color_glyph(face.glyph_id(c))),
    }
}

/// Without the `parsed-font` feature there is no face to shape in.
#[cfg(not(feature = "parsed-font"))]
pub(super) fn shapes_to_colour(face: &Font, _cluster: &str) -> bool {
    match face.parsed.never {}
}

/// `range` of `text` in `face` with no tables to shape with: each char that
/// draws through the cmap, advanced by its advance. Only a test's
/// fixed-pitch face, which has no bytes, takes this path.
#[cfg(feature = "parsed-font")]
fn nominal(
    face: &Font,
    index: u8,
    scale: f32,
    text: &str,
    range: Range<usize>,
    out: &mut Vec<ShapedGlyph>,
) {
    for (at, c) in text[range.clone()].char_indices() {
        if fallback::is_invisible(c) {
            continue;
        }
        let glyph = face.glyph_id(c);
        out.push(ShapedGlyph {
            face: index,
            glyph,
            cluster: range.start + at,
            advance: face.advance(glyph) * scale,
            offset: Vec2::ZERO,
        });
    }
}

#[cfg(all(test, feature = "parsed-font"))]
mod tests {
    use super::*;
    use crate::font::fixtures;

    const SIZE: f32 = 16.0;

    /// `text` shaped whole in `font`.
    fn shaped(font: &Font, text: &str) -> Vec<ShapedGlyph> {
        let mut out = Vec::new();
        shape(font, SIZE, text, 0..text.len(), &mut out);
        out
    }

    /// `text` shaped straight through `harfrust` in the committed font, as
    /// `(glyph, cluster, x advance, x offset, y offset)` in font units.
    fn reference(text: &str) -> Vec<(u32, u32, i32, i32, i32)> {
        let font = harfrust::Font::new(crate::font::SANS_TTF, 0).expect("reads");
        let mut buffer = harfrust::Buffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        harfrust::shape(
            &harfrust::ShaperFont::new(&font),
            &mut buffer,
            harfrust::ShapeOptions::new(),
        )
        .expect("shapes");
        buffer
            .glyph_infos()
            .iter()
            .zip(buffer.glyph_positions())
            .map(|(info, at)| {
                (
                    info.glyph_id,
                    info.cluster,
                    at.x_advance,
                    at.x_offset,
                    at.y_offset,
                )
            })
            .collect()
    }

    /// **Advances, offsets and clusters are `harfrust`'s, scaled**: a
    /// kerned pair, a mark the font positions over a base with no
    /// precomposed form, and a plain word, glyph for glyph.
    #[test]
    fn advances_and_offsets_are_harfrusts_scaled_to_the_size() {
        let font = Font::sans();
        let scale = font.metrics().scale(SIZE);
        for text in ["AVATAR", "h\u{301}x", "The quick"] {
            let got = shaped(font, text);
            let want = reference(text);
            assert_eq!(got.len(), want.len(), "{text:?}");
            for (got, (glyph, cluster, advance, x, y)) in got.iter().zip(want) {
                assert_eq!(got.face, 0);
                assert_eq!(got.glyph, GlyphId(glyph), "{text:?}");
                assert_eq!(got.cluster, cluster as usize, "{text:?}");
                assert_eq!(got.advance, advance as f32 * scale, "{text:?}");
                assert_eq!(got.offset, Vec2::new(x as f32, -y as f32) * scale);
            }
        }
        // The pair kerns, and the mark sits on its base with no advance.
        let pair = shaped(font, "AV");
        let a = font.glyph_id('A');
        assert!(
            pair[0].advance < font.advance(a) * scale,
            "AV is not kerned"
        );
        let mark = shaped(font, "h\u{301}");
        assert_eq!(mark.len(), 2);
        assert_eq!(mark[1].cluster, 0, "the mark is in its base's cluster");
        assert_eq!(mark[1].advance, 0.0);
        assert_ne!(mark[1].offset, Vec2::ZERO, "the mark is not positioned");
    }

    /// **A synthesised bold widens each glyph that advances**, by its share
    /// of an em at the size, and leaves a mark's zero advance alone.
    #[test]
    fn a_synthesised_bold_widens_what_advances() {
        let font = Font::sans();
        let bold = font.synthesized(crate::font::face::Synthesis {
            bold: true,
            italic: false,
        });
        let extra = SIZE * crate::font::face::SYNTHETIC_BOLD_EM;
        let (plain, wide) = (shaped(font, "xh\u{301}"), shaped(&bold, "xh\u{301}"));
        for (plain, wide) in plain.iter().zip(&wide).take(2) {
            assert!((wide.advance - plain.advance - extra).abs() < 1e-4);
        }
        assert_eq!(wide[2].advance, 0.0, "the mark was widened");
    }

    /// **The committed font's GSUB applies**: its `ccmp` swaps `i` for a
    /// dotless `ı` under a mark above, which a cmap lookup never would.
    #[test]
    fn the_committed_fonts_gsub_applies() {
        let font = Font::sans();
        let glyphs = shaped(font, "i\u{307}");
        assert_eq!(glyphs[0].glyph, font.glyph_id('\u{131}'));
        assert_ne!(glyphs[0].glyph, font.glyph_id('i'));
    }

    /// **A ZWJ sequence and a flag become one ligature each**, in the face
    /// that maps all of them, and each ligature is a colour glyph, while its
    /// components are not.
    #[test]
    fn sequences_and_flags_ligate_into_colour_glyphs() {
        let face = fixtures::shaped();
        let font = Font::sans().with_fallback(&[fixtures::cbdt(), face.clone()]);
        let text = "a\u{2764}\u{200D}\u{1F525}\u{1F1FA}\u{1F1F8}b";
        let glyphs = shaped(&font, text);
        let faces: Vec<u8> = glyphs.iter().map(|glyph| glyph.face).collect();
        assert_eq!(faces, [0, 2, 2, 0]);
        let clusters: Vec<usize> = glyphs.iter().map(|glyph| glyph.cluster).collect();
        assert_eq!(clusters, [0, 1, 11, 19]);
        for ligature in &glyphs[1..3] {
            assert!(face.has_color_glyph(ligature.glyph));
            assert_eq!(ligature.advance, SIZE, "an em wide, as every glyph is");
        }
        // A selector draws nothing, not even an empty glyph, in a face
        // that has a space glyph to hide it as.
        let hearts = shaped(&face, "\u{2764}\u{FE0F}\u{2764}");
        assert_eq!(hearts.len(), 2, "the selector left a glyph");
        for component in ['\u{2764}', '\u{1F525}', '\u{1F1FA}', '\u{1F1F8}'] {
            assert!(!face.has_color_glyph(face.glyph_id(component)));
        }
    }

    /// **A run is shaped only where asked**: shaping part of a text appends
    /// that part's glyphs, clusters counted in the whole text.
    #[test]
    fn only_the_range_is_shaped_with_clusters_in_the_whole_text() {
        let font = Font::sans();
        let mut out = Vec::new();
        shape(font, SIZE, "ab cd", 3..5, &mut out);
        let clusters: Vec<usize> = out.iter().map(|glyph| glyph.cluster).collect();
        assert_eq!(clusters, [3, 4]);
        assert_eq!(out[0].glyph, font.glyph_id('c'));
    }
}
