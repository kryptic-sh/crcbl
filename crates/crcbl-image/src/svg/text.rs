//! Text laid out as SVG 2 lays out a line: the characters a `<text>` element
//! draws once white space is processed, the positions its `x`, `y`, `dx`
//! and `dy` lists give them, and each chunk placed by its `text-anchor`.
//!
//! # Shaping
//!
//! A chunk starts at each character with an absolute `x` or `y`, except a
//! mark or format character that continues the cluster before it, whose
//! coordinates are ignored. Each chunk is shaped on its own by `harfrust`
//! (the submodule `shape`): bidirectional runs resolved and put in visual
//! order, each cluster in the first face that draws it, with its GSUB and
//! GPOS — kerning, ligatures, contextual forms, marks on their bases. The
//! pen then walks the chunk's glyphs from the left, each advanced by its
//! shaped advance, a cluster's last glyph with letter spacing added, and
//! word spacing too where the cluster is a space.
//!
//! **A cluster takes the positions of its first character**: its `dx` and
//! `dy` shift the pen before the cluster's first glyph in visual order.
//! The positions of the other characters of a cluster — a ligature's later
//! components, a mark on its base, a character shaping removed — are
//! ignored, as browsers ignore them; a chunk's `x` and `y` place the pen
//! before its visually first glyph. A character no face maps draws the
//! `.notdef` glyph of its cluster's face, as browsers draw it.
//!
//! # Fonts
//!
//! Faces come from the system through `crcbl-font`, when this crate is built
//! with its `svg-text` feature ([`faces`]). Without it, or with no face for
//! a text's `font-family` list nor for `serif`, text that would draw a
//! character refuses the file, as anything else this renderer cannot draw
//! does; text that draws none (empty, or only collapsible white space) needs
//! no font and draws nothing.

#[cfg(feature = "svg-text")]
pub(super) mod faces;
#[cfg(feature = "svg-text")]
pub(super) mod shape;

#[cfg(feature = "svg-text")]
use super::style::Anchor;

/// Where text finds its faces: the system's, or a test's own.
#[cfg(feature = "svg-text")]
pub(super) type Fonts = &'static dyn faces::FontSource;

/// Where text finds its faces: nowhere, in a build without `svg-text`.
#[cfg(not(feature = "svg-text"))]
#[derive(Clone, Copy)]
pub(super) struct Fonts {}

/// The faces one document has read.
#[cfg(feature = "svg-text")]
pub(super) type FaceCache = faces::Faces;

/// The faces one document has read: none, in a build without `svg-text`.
#[cfg(not(feature = "svg-text"))]
#[derive(Default)]
pub(super) struct FaceCache {}

/// The fonts [`decode`](super::decode) draws text in: the system's.
#[cfg(feature = "svg-text")]
pub(super) fn system_fonts() -> Fonts {
    faces::system()
}

/// The fonts [`decode`](super::decode) draws text in: none.
#[cfg(not(feature = "svg-text"))]
pub(super) fn system_fonts() -> Fonts {
    Fonts {}
}

/// One character a text element draws, after white space is processed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Character {
    pub ch: char,
    /// The element whose text it is: its index in the document.
    pub owner: usize,
    /// Whether it is a space white-space processing may collapse, as the
    /// last space of the text is.
    collapsible: bool,
}

/// Appends the characters of `text`, which the element `owner` holds, to
/// `out` as SVG 2's white-space processing has them (CSS Text's for one
/// line): a line break or a tab is a space; then, unless `preserve`
/// (`xml:space="preserve"`), a space is dropped at the start of the text
/// and after another space that could collapse, across element boundaries
/// as the text is one line. [`finish`] drops a space left at the end.
pub(super) fn push_characters(out: &mut Vec<Character>, text: &str, owner: usize, preserve: bool) {
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        let ch = match ch {
            // XML reads a carriage return and line feed as one line break.
            '\r' if chars.peek() == Some(&'\n') => continue,
            '\r' | '\n' | '\t' => ' ',
            other => other,
        };
        let collapsible = ch == ' ' && !preserve;
        if collapsible && out.last().is_none_or(|last| last.collapsible) {
            continue;
        }
        out.push(Character {
            ch,
            owner,
            collapsible,
        });
    }
}

/// Drops the collapsible space a text's characters may end with.
pub(super) fn finish(out: &mut Vec<Character>) {
    if out.last().is_some_and(|last| last.collapsible) {
        out.pop();
    }
}

/// A character's positions from the `x`, `y`, `dx` and `dy` lists: an
/// absolute coordinate, if a list gave one, starts a new chunk there.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Position {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub dx: Option<f64>,
    pub dy: Option<f64>,
}

/// Which of a [`Position`]'s lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Axis {
    X,
    Y,
    Dx,
    Dy,
}

/// Gives `values`, one element's list for `axis`, to that element's
/// characters, `positions` (its own and its descendants'), the first value
/// to the first character, as SVG 2 assigns them. A list longer than the
/// characters has its rest ignored; an element nested inside assigns after
/// its ancestors, so its values win for its characters.
pub(super) fn assign(positions: &mut [Position], axis: Axis, values: &[f64]) {
    for (position, &value) in positions.iter_mut().zip(values) {
        let slot = match axis {
            Axis::X => &mut position.x,
            Axis::Y => &mut position.y,
            Axis::Dx => &mut position.dx,
            Axis::Dy => &mut position.dy,
        };
        *slot = Some(value);
    }
}

/// What placing a glyph needs of it: the positions that move the pen before
/// it, how far it advances the pen (its shaped advance with letter and word
/// spacing added), and the anchor of the chunk it is in.
#[cfg(feature = "svg-text")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Advance {
    pub position: Position,
    pub advance: f64,
    pub anchor: Anchor,
}

/// The pen position of each glyph in the text's user space, `glyphs` being
/// each chunk's in visual order.
///
/// The pen starts at the origin. A glyph with an absolute `x` or `y` moves
/// the pen there and starts a chunk; its `dx` and `dy` then shift the pen,
/// and the glyph sits at the pen, which its advance moves on. A chunk is
/// then shifted by its first glyph's anchor: back by its advances'
/// sum for `end`, by half of it for `middle`, as resvg measures a chunk
/// (SVG 2 measures from the glyphs placed, `dx` included, which differs only
/// where a `dx` falls inside a chunk). The pen goes on from where it left
/// the chunk before that shift.
#[cfg(feature = "svg-text")]
pub(super) fn place(glyphs: &[Advance]) -> Vec<(f64, f64)> {
    let mut origins = Vec::with_capacity(glyphs.len());
    let (mut x, mut y) = (0.0, 0.0);
    let mut chunk_start = 0;
    let mut chunk_advance = 0.0;
    for (index, glyph) in glyphs.iter().enumerate() {
        let position = glyph.position;
        if index > 0 && (position.x.is_some() || position.y.is_some()) {
            anchor(
                &mut origins[chunk_start..],
                glyphs[chunk_start].anchor,
                chunk_advance,
            );
            chunk_start = index;
            chunk_advance = 0.0;
        }
        x = position.x.unwrap_or(x) + position.dx.unwrap_or(0.0);
        y = position.y.unwrap_or(y) + position.dy.unwrap_or(0.0);
        origins.push((x, y));
        x += glyph.advance;
        chunk_advance += glyph.advance;
    }
    if let Some(first) = glyphs.get(chunk_start) {
        anchor(&mut origins[chunk_start..], first.anchor, chunk_advance);
    }
    origins
}

/// Shifts a chunk's glyph origins by its `anchor` for a chunk `advance`
/// long.
#[cfg(feature = "svg-text")]
fn anchor(origins: &mut [(f64, f64)], anchor: Anchor, advance: f64) {
    let shift = match anchor {
        Anchor::Start => return,
        Anchor::Middle => -advance / 2.0,
        Anchor::End => -advance,
    };
    for origin in origins {
        origin.0 += shift;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn processed(pieces: &[(&str, bool)]) -> String {
        let mut out = Vec::new();
        for (owner, (text, preserve)) in pieces.iter().enumerate() {
            push_characters(&mut out, text, owner, *preserve);
        }
        finish(&mut out);
        out.iter().map(|character| character.ch).collect()
    }

    /// **White space collapses as one line of CSS text**: breaks and tabs
    /// are spaces, a run of spaces is one even across elements, and the
    /// text's ends lose theirs; `preserve` keeps every space it holds.
    #[test]
    fn white_space_collapses_across_elements() {
        assert_eq!(processed(&[("  a \n\t b  ", false)]), "a b");
        assert_eq!(processed(&[("a ", false), (" b", false)]), "a b");
        assert_eq!(
            processed(&[("a ", false), ("  ", false), ("b ", false)]),
            "a b"
        );
        assert_eq!(processed(&[(" \n ", false)]), "");
        assert_eq!(processed(&[(" a\tb\r\nc  ", true)]), " a b c  ");
        // A preserved space does not swallow a collapsible one after it, and
        // a collapsible space after a preserved character stays.
        assert_eq!(processed(&[("a ", true), (" b ", false)]), "a  b");
        assert_eq!(processed(&[("a", true), (" b", false)]), "a b");
        // Each character keeps the element it came from.
        let mut out = Vec::new();
        push_characters(&mut out, "a ", 3, false);
        push_characters(&mut out, " b", 7, false);
        finish(&mut out);
        let owners: Vec<_> = out.iter().map(|character| character.owner).collect();
        assert_eq!(owners, [3, 3, 7]);
    }

    /// **Lists assign from an element's first character**, a longer list's
    /// rest ignored, and a later assignment wins.
    #[test]
    fn position_lists_assign_in_order() {
        let mut positions = vec![Position::default(); 3];
        assign(&mut positions, Axis::X, &[1.0, 2.0, 3.0, 4.0]);
        assign(&mut positions[1..2], Axis::X, &[9.0]);
        assign(&mut positions, Axis::Dy, &[5.0]);
        let xs: Vec<_> = positions.iter().map(|position| position.x).collect();
        assert_eq!(xs, [Some(1.0), Some(9.0), Some(3.0)]);
        let dys: Vec<_> = positions.iter().map(|position| position.dy).collect();
        assert_eq!(dys, [Some(5.0), None, None]);
        assert!(positions.iter().all(|position| position.y.is_none()));
    }

    #[cfg(feature = "svg-text")]
    fn advance(x: Option<f64>, y: Option<f64>, dx: f64, dy: f64, anchor: Anchor) -> Advance {
        Advance {
            position: Position {
                x,
                y,
                dx: Some(dx),
                dy: Some(dy),
            },
            advance: 10.0,
            anchor,
        }
    }

    /// **The pen advances glyph by glyph**, shifted by `dx` and `dy` that
    /// carry on, and moved by absolute coordinates.
    #[test]
    #[cfg(feature = "svg-text")]
    fn the_pen_advances_and_shifts() {
        let origins = place(&[
            advance(Some(5.0), Some(20.0), 0.0, 0.0, Anchor::Start),
            advance(None, None, 2.0, -3.0, Anchor::Start),
            advance(None, None, 0.0, 0.0, Anchor::Start),
            advance(None, Some(40.0), 0.0, 0.0, Anchor::Start),
        ]);
        assert_eq!(
            origins,
            [(5.0, 20.0), (17.0, 17.0), (27.0, 17.0), (37.0, 40.0)]
        );
    }

    /// **Each chunk is anchored on its own**, by the anchor of its first
    /// character and the sum of its advances, and the next chunk's pen is
    /// not shifted with it.
    #[test]
    #[cfg(feature = "svg-text")]
    fn chunks_anchor_separately() {
        let origins = place(&[
            advance(Some(100.0), Some(0.0), 0.0, 0.0, Anchor::End),
            advance(None, None, 0.0, 0.0, Anchor::Start),
            advance(None, Some(10.0), 0.0, 0.0, Anchor::Middle),
            advance(None, None, 0.0, 0.0, Anchor::End),
            advance(Some(0.0), None, 0.0, 0.0, Anchor::Start),
        ]);
        assert_eq!(
            origins,
            [
                (80.0, 0.0),
                (90.0, 0.0),
                (110.0, 10.0),
                (120.0, 10.0),
                (0.0, 10.0)
            ]
        );
    }
}
