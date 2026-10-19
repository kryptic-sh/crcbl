//! Shaping: a text chunk's characters to the glyphs that draw them, through
//! `harfrust`, in visual order.
//!
//! # Clusters before shaping
//!
//! What picks a face ([`Faces::cluster_face`]), and where a chunk may not
//! start, is a cluster: a character and every nonspacing mark (bidi class
//! NSM, which takes in the variation selectors) and format character (class
//! BN, the joiners among them) after it in the same font. That is UAX #29's
//! grapheme cluster approximated from the bidi classes alone: a spacing mark
//! (an Indic vowel sign) starts a cluster of its own, so it can be drawn in
//! another face than its base's when its base's face lacks it.
//!
//! # Bidirectional text
//!
//! Each chunk is a paragraph of UAX #9 on its own, as SVG reorders each
//! chunk on its own, at level 0, since `direction` is drawn only at its
//! initial `ltr`. The levels come from `unicode-bidi`, with rule L1 applied
//! across the chunk, so a right-to-left run's trailing space goes back to
//! level 0; the runs of one level and one face are shaped each in its
//! direction, and put in visual order by rule L2.
//!
//! # What a run is shaped with
//!
//! The face's own GSUB and GPOS at the face's default features (kerning,
//! ligatures, contextual forms, mark positioning, complex scripts'
//! reordering), at the `wght` the run's `font-weight` asks of a variable
//! face, with the rest of the chunk as context. The script is `harfrust`'s
//! guess from the run (`Buffer::guess_segment_properties`) and the language
//! is never set, so `locl` forms never apply. Default-ignorable characters
//! are removed once GSUB has seen them. Clusters are `harfrust`'s monotone
//! graphemes, so a ligature or a mark attached to its base is one cluster,
//! named by its first character.

use core::ops::Range;

use skrifa::GlyphId;
use unicode_bidi::{BidiClass, Level, ParagraphBidiInfo, bidi_class, level};

use super::faces::{Faces, FontSource, Glyph};
use crate::ImageError;
use crate::svg::style::TextStyle;
use crate::svg::unsupported;

/// One glyph a chunk shapes into, in its face's font units, `y` up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::svg) struct Shaped {
    pub glyph: Glyph,
    /// The first character of its cluster, by index in the chunk.
    pub cluster: usize,
    /// How far it moves the pen.
    pub advance: f64,
    /// Where it is drawn from the pen.
    pub offset: (f64, f64),
}

/// Whether the character at `index`, styled as `styles` say, continues the
/// cluster of the one before it: a nonspacing mark or a format character in
/// the same font.
pub(in crate::svg) fn continues(characters: &[char], styles: &[&TextStyle], index: usize) -> bool {
    index > 0
        && matches!(
            bidi_class(characters[index]),
            BidiClass::NSM | BidiClass::BN
        )
        && same_font(styles[index - 1], styles[index])
}

/// Whether two styles choose their faces alike.
fn same_font(a: &TextStyle, b: &TextStyle) -> bool {
    a.families == b.families && a.weight == b.weight && a.slant == b.slant
}

/// Whether `ch` needs a glyph from the face that draws its cluster: a format
/// character is removed in shaping, and a variation selector only chooses
/// among its base's glyphs.
fn draws(ch: char) -> bool {
    bidi_class(ch) != BidiClass::BN
        && !matches!(ch, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

/// The glyphs `characters`, one chunk, each styled as `styles` say, shape
/// into, in visual order from the left; see the module docs.
///
/// # Errors
///
/// [`ImageError::Unsupported`] for a chunk no face can draw, past a cap of
/// [`Faces`], or past `u32::MAX` characters, which `harfrust` numbers its
/// clusters in.
pub(in crate::svg) fn shape_chunk(
    faces: &mut Faces,
    source: &dyn FontSource,
    characters: &[char],
    styles: &[&TextStyle],
) -> Result<Vec<Shaped>, ImageError> {
    let count = characters.len();
    if u32::try_from(count).is_err() {
        return Err(unsupported("a text chunk of more than u32::MAX characters"));
    }
    let text: String = characters.iter().collect();
    let starts: Vec<usize> = text
        .char_indices()
        .map(|(at, _)| at)
        .chain([text.len()])
        .collect();
    let mut chosen = Vec::with_capacity(count);
    let mut start = 0;
    while start < count {
        let mut end = start + 1;
        while end < count && continues(characters, styles, end) {
            end += 1;
        }
        let cluster = &text[starts[start]..starts[end]];
        let face = faces.cluster_face(source, styles[start], cluster, draws)?;
        chosen.resize(end, face);
        start = end;
    }
    let levels = levels(&text, &starts[..count]);
    let mut runs: Vec<(Range<usize>, (Level, usize, u16))> = Vec::new();
    for index in 0..count {
        let key = (levels[index], chosen[index], styles[index].weight);
        match runs.last_mut() {
            Some((run, run_key)) if *run_key == key => run.end = index + 1,
            _ => runs.push((index..index + 1, key)),
        }
    }
    let run_levels: Vec<Level> = runs.iter().map(|(_, (level, _, _))| *level).collect();
    let mut out = Vec::with_capacity(count);
    for visual in ParagraphBidiInfo::reorder_visual(&run_levels) {
        let (run, (level, face, weight)) = &runs[visual];
        let shaper = faces.shaper(*face, *weight);
        let buffer = shape_run(
            &shaper,
            characters,
            &text,
            &starts,
            run.clone(),
            level.is_rtl(),
        );
        for (info, position) in buffer.glyph_infos().iter().zip(buffer.glyph_positions()) {
            out.push(Shaped {
                glyph: Glyph {
                    face: *face,
                    id: GlyphId::new(info.glyph_id),
                },
                cluster: info.cluster as usize,
                advance: f64::from(position.x_advance),
                offset: (f64::from(position.x_offset), f64::from(position.y_offset)),
            });
        }
    }
    Ok(out)
}

/// Each character's level, `starts` being their byte offsets in `text`:
/// UAX #9's for `text` as one left-to-right paragraph, rule L1 applied.
fn levels(text: &str, starts: &[usize]) -> Vec<Level> {
    if !text.is_ascii() {
        let info = ParagraphBidiInfo::new(text, Some(Level::ltr()));
        if level::has_rtl(&info.levels) {
            let levels = info.reordered_levels(0..text.len());
            return starts.iter().map(|&at| levels[at]).collect();
        }
    }
    vec![Level::ltr(); starts.len()]
}

/// The characters `run` of a chunk, whose text is `text` and whose
/// characters start at the byte offsets `starts`, shaped by `shaper`, right
/// to left when `rtl` says so: its glyphs in visual order, each glyph's
/// cluster the index of its first character in the chunk.
fn shape_run(
    shaper: &harfrust::Font,
    characters: &[char],
    text: &str,
    starts: &[usize],
    run: Range<usize>,
    rtl: bool,
) -> harfrust::Buffer {
    let mut buffer = harfrust::Buffer::new();
    for index in run.clone() {
        let cluster =
            u32::try_from(index).expect("a chunk of at most `u32::MAX` characters, checked first");
        buffer.push(u32::from(characters[index]), cluster);
    }
    buffer.set_pre_context(&text[..starts[run.start]]);
    buffer.set_post_context(&text[starts[run.end]..]);
    buffer.set_direction(if rtl {
        harfrust::Direction::RightToLeft
    } else {
        harfrust::Direction::LeftToRight
    });
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
