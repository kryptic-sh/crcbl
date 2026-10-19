//! Line break opportunities: where a line must end, and where it may.
//!
//! # The rule
//!
//! * **A hard break ends a paragraph**: a char of Unicode Standard Annex
//!   #14's classes BK, CR, LF or NL — a line feed, a carriage return (with
//!   the line feed after it, as one break), a vertical tab, a form feed,
//!   U+0085 NEXT LINE, U+2028 LINE SEPARATOR or U+2029 PARAGRAPH SEPARATOR.
//!   The break belongs to no paragraph.
//! * **A soft break is UAX #14's**, as `unicode-linebreak` finds them — after
//!   the spaces following a word, after a hyphen, between two ideographs, and
//!   never before closing punctuation, among the rest of its pair table — and
//!   only where a [`cluster`] starts, so no break is ever inside one.
//!
//! A soft break after spaces falls where a word starts; one after a hyphen,
//! an ideograph or an emoji falls inside a word, which the line breakers
//! shape whole and cut only when a line has to break there.

use core::ops::Range;

use unicode_linebreak::{BreakClass, break_property, linebreaks};

use super::cluster;

/// Whether `c` ends a paragraph: UAX #14's BK, CR, LF and NL classes.
fn is_hard_break(c: char) -> bool {
    matches!(
        break_property(u32::from(c)),
        BreakClass::Mandatory
            | BreakClass::CarriageReturn
            | BreakClass::LineFeed
            | BreakClass::NextLine
    )
}

/// Each paragraph of `text`, in order, as the bytes between its hard breaks,
/// the breaks left out. See the module docs. Text that ends in a hard break
/// ends with an empty paragraph, and empty text is one empty paragraph.
pub(crate) fn paragraphs(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut chars = text.char_indices().peekable();
    let mut start = Some(0);
    core::iter::from_fn(move || {
        let from = start?;
        while let Some((at, c)) = chars.next() {
            if is_hard_break(c) {
                let mut next = at + c.len_utf8();
                if c == '\r' && chars.next_if(|&(_, c)| c == '\n').is_some() {
                    next += '\n'.len_utf8();
                }
                start = Some(next);
                return Some(from..at);
            }
        }
        start = None;
        Some(from..text.len())
    })
}

/// Every soft break in `paragraph` — text with no hard break in it — as the
/// byte it falls before, in order: never its start or its end. See the
/// module docs.
pub(crate) fn soft_breaks(paragraph: &str) -> Vec<usize> {
    let mut starts = cluster::clusters(paragraph)
        .map(|cluster| cluster.start)
        .peekable();
    linebreaks(paragraph)
        .map(|(at, _)| at)
        .filter(|&at| {
            while starts.next_if(|&cluster| cluster < at).is_some() {}
            at < paragraph.len() && starts.peek() == Some(&at)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `text`'s paragraphs as strings.
    fn split(text: &str) -> Vec<&str> {
        paragraphs(text).map(|range| &text[range]).collect()
    }

    /// `paragraph` cut at its soft breaks.
    fn cut(paragraph: &str) -> Vec<&str> {
        let mut pieces = Vec::new();
        let mut start = 0;
        for at in soft_breaks(paragraph).into_iter().chain([paragraph.len()]) {
            pieces.push(&paragraph[start..at]);
            start = at;
        }
        pieces
    }

    /// **Every hard break ends a paragraph and belongs to none**, a carriage
    /// return and line feed together being one, and a break at the end
    /// leaving an empty paragraph after it.
    #[test]
    fn hard_breaks_end_paragraphs() {
        assert_eq!(split(""), [""]);
        assert_eq!(split("ab"), ["ab"]);
        assert_eq!(split("ab\n"), ["ab", ""]);
        assert_eq!(split("a\n\nb"), ["a", "", "b"]);
        assert_eq!(split("a\r\nb\rc"), ["a", "b", "c"]);
        assert_eq!(
            split("a\u{B}b\u{C}c\u{85}d\u{2028}e\u{2029}f"),
            ["a", "b", "c", "d", "e", "f"]
        );
        // A tab and a no-break space are not breaks.
        assert_eq!(split("a\tb\u{A0}c"), ["a\tb\u{A0}c"]);
    }

    /// **Soft breaks follow UAX #14's classes**: after a run of spaces (SP,
    /// rule LB18), after a hyphen (HY, LB21), between two ideographs (ID,
    /// LB31), never before closing punctuation (CL and CP, LB13) or a no-break
    /// space (GL, LB12a), and never at either end.
    #[test]
    fn soft_breaks_follow_the_line_break_classes() {
        assert_eq!(cut("The  quick"), ["The  ", "quick"]);
        assert_eq!(cut("  lead"), ["  ", "lead"]);
        assert_eq!(cut("well-known"), ["well-", "known"]);
        assert_eq!(
            cut("\u{65E5}\u{672C}\u{8A9E}"),
            ["\u{65E5}", "\u{672C}", "\u{8A9E}"]
        );
        assert_eq!(cut("(a) b"), ["(a) ", "b"]);
        assert_eq!(cut("end )"), ["end )"]);
        assert_eq!(cut("10\u{A0}kg"), ["10\u{A0}kg"]);
        assert_eq!(cut("tail "), ["tail "]);
        assert_eq!(cut(""), [""]);
    }

    /// **No soft break is inside a [`cluster`]**: UAX #14 breaks after a
    /// space a mark follows (LB10 reads the mark as a letter), and UAX #29
    /// makes the space and the mark one cluster.
    #[test]
    fn clusters_stay_whole() {
        assert_eq!(cut("a \u{301}b"), ["a \u{301}b"]);
        assert_eq!(cut("a b"), ["a ", "b"]);
    }
}
