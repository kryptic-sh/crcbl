//! Bidirectional text: the levels Unicode Standard Annex #9 gives each char,
//! through `unicode-bidi`, and the visual order a line's pieces take.
//!
//! # The rule
//!
//! * **A paragraph's direction is its first strong char's** (rules P2 and
//!   P3): right to left when that is a Hebrew or Arabic letter, or opens a
//!   right-to-left embedding, override or isolate; left to right otherwise,
//!   and when it has no strong char at all. Nothing else sets it: there is
//!   no `direction` property.
//! * **Each char takes UAX #9's level**, explicit embeddings, overrides and
//!   isolates included; an odd level runs right to left. A line's levels have
//!   rule L1 applied — a tab, and the whitespace before it, take the
//!   paragraph's level — and its pieces are put in visual order by rule L2:
//!   from the highest level down to the lowest odd one, every run of pieces at
//!   that level or above is reversed.
//! * **Text with no right-to-left level is left alone**: ASCII is never
//!   analysed, and other text whose levels are all even is laid out exactly
//!   as text with no bidi at all.

use core::ops::Range;

use unicode_bidi::{Level, ParagraphBidiInfo, level};

/// One paragraph's levels: only ever made for one with a right-to-left
/// level in it.
#[derive(Debug)]
pub(crate) struct Bidi<'t> {
    info: ParagraphBidiInfo<'t>,
}

impl<'t> Bidi<'t> {
    /// `paragraph`'s levels — text with no hard break in it — or `None` when
    /// every char of it runs left to right. See the module docs.
    pub(crate) fn new(paragraph: &'t str) -> Option<Self> {
        if paragraph.is_ascii() {
            return None;
        }
        let info = ParagraphBidiInfo::new(paragraph, None);
        level::has_rtl(&info.levels).then_some(Self { info })
    }

    /// Whether the paragraph runs right to left.
    pub(crate) fn rtl(&self) -> bool {
        self.info.paragraph_level.is_rtl()
    }

    /// `range` of the paragraph cut where its level changes: each stretch,
    /// and whether it runs right to left.
    pub(crate) fn runs(&self, range: Range<usize>) -> Vec<(Range<usize>, bool)> {
        let levels = &self.info.levels;
        let mut runs = Vec::new();
        let mut start = range.start;
        for (at, _) in self.info.text[range.clone()].char_indices().skip(1) {
            let at = range.start + at;
            if levels[at] != levels[start] {
                runs.push((start..at, levels[start].is_rtl()));
                start = at;
            }
        }
        if start < range.end {
            runs.push((start..range.end, levels[start].is_rtl()));
        }
        runs
    }

    /// The level of each byte of the paragraph, with rule L1 applied to the
    /// bytes `line`.
    pub(crate) fn line_levels(&self, line: Range<usize>) -> Vec<Level> {
        self.info.reordered_levels(line)
    }
}

/// The visual order of pieces at `levels`, by rule L2: the index of the
/// piece shown first from the left, then the next, and so on.
pub(crate) fn visual_order(levels: &[Level]) -> Vec<usize> {
    ParagraphBidiInfo::reorder_visual(levels)
}

/// `text` — one line — cut into its stretches of one level, rule L1
/// applied, as ranges of chars: one stretch for text with no right-to-left
/// level, and none for empty text.
pub(crate) fn char_runs(text: &str) -> Vec<Range<usize>> {
    let Some(bidi) = Bidi::new(text) else {
        let chars = text.chars().count();
        return (chars > 0).then_some(0..chars).into_iter().collect();
    };
    let levels = bidi.line_levels(0..text.len());
    let mut runs: Vec<Range<usize>> = Vec::new();
    let mut previous = None;
    for (index, (at, _)) in text.char_indices().enumerate() {
        match runs.last_mut() {
            Some(run) if previous == Some(levels[at]) => run.end = index + 1,
            _ => runs.push(index..index + 1),
        }
        previous = Some(levels[at]);
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each char's level in `text`, rule L1 applied to the whole of it, or
    /// `None` when it has no right-to-left level.
    fn levels(text: &str) -> Option<Vec<u8>> {
        let bidi = Bidi::new(text)?;
        let levels = bidi.line_levels(0..text.len());
        Some(
            text.char_indices()
                .map(|(at, _)| levels[at].number())
                .collect(),
        )
    }

    /// **Levels are UAX #9's**, worked by hand from its rules: Hebrew in a
    /// Latin paragraph is level one (I1) and the paragraph stays left to
    /// right (P2); Latin in a Hebrew paragraph is level two (I2), and a space
    /// between Hebrew and Latin takes the paragraph's direction (N2); a
    /// European number after Hebrew is level two (W rules, I2); an override
    /// makes Latin level one (X4); the trailing space of a line goes back to
    /// the paragraph's level (L1).
    #[test]
    fn levels_follow_the_bidi_rules() {
        assert_eq!(levels("ab"), None, "ASCII is never analysed");
        assert_eq!(levels("caf\u{E9}"), None, "no right-to-left level");
        assert_eq!(levels("ab \u{5D0}\u{5D1}"), Some(vec![0, 0, 0, 1, 1]));
        assert_eq!(
            levels("\u{5D0} ab"),
            Some(vec![1, 1, 2, 2]),
            "Latin in a Hebrew paragraph"
        );
        assert_eq!(levels("\u{5D0} 12"), Some(vec![1, 1, 2, 2]));
        assert_eq!(
            levels("\u{202E}ab\u{202C}"),
            Some(vec![0, 1, 1, 0]),
            "an override"
        );
        assert_eq!(levels("\u{5D0}b "), Some(vec![1, 2, 1]));
        let bidi = Bidi::new("\u{5D0}b").expect("right to left");
        assert!(bidi.rtl(), "the first strong char is Hebrew");
        assert!(!Bidi::new("b\u{5D0}").expect("mixed").rtl());
    }

    /// **Direction runs and visual order**: a paragraph cuts where its level
    /// changes, and L2 reverses each run at or above every odd level, from
    /// the highest down.
    #[test]
    fn runs_cut_at_levels_and_reorder_by_l2() {
        let text = "ab \u{5D0}\u{5D1} cd";
        let bidi = Bidi::new(text).expect("right to left");
        let runs: Vec<(&str, bool)> = bidi
            .runs(0..text.len())
            .into_iter()
            .map(|(run, rtl)| (&text[run], rtl))
            .collect();
        assert_eq!(
            runs,
            [("ab ", false), ("\u{5D0}\u{5D1}", true), (" cd", false)]
        );
        let at = |levels: &[u8]| visual_order(&Level::vec(levels));
        assert_eq!(at(&[0, 1, 1, 0]), [0, 2, 1, 3]);
        assert_eq!(at(&[1, 2, 2, 1]), [3, 1, 2, 0]);
        assert_eq!(at(&[0, 0]), [0, 1]);
        assert_eq!(char_runs("ab \u{5D0}\u{5D1}"), [0..3, 3..5]);
        let whole = char_runs("abc");
        assert_eq!((whole.len(), whole.first()), (1, Some(&(0..3))));
        assert!(char_runs("").is_empty());
    }
}
