//! Clusters: the runs of chars that draw, fall back, break and take the caret
//! as one.
//!
//! # The rule
//!
//! A cluster is a base char and every char after it that continues it:
//!
//! * a **combining mark** from the combining diacritical mark blocks (U+0300
//!   to U+036F, U+1AB0 to U+1AFF, U+1DC0 to U+1DFF, U+20D0 to U+20FF — which
//!   holds the keycap, U+20E3 — and U+FE20 to U+FE2F);
//! * a **variation selector** (U+FE00 to U+FE0F, U+E0100 to U+E01EF);
//! * a **zero-width joiner**, and the char right after it, which it joins —
//!   an emoji ZWJ sequence such as `👩‍💻`;
//! * an **emoji skin-tone modifier** (U+1F3FB to U+1F3FF);
//! * a **tag** (U+E0020 to U+E007F), which spells a subdivision flag;
//! * the **second regional indicator** of a pair — a flag — so `🇺🇸🇬🇧` is two
//!   clusters, not one or four.
//!
//! **A space (U+0020), a line break or any other control char is always a
//! cluster of its own**: nothing continues it and it continues nothing, so a
//! line breaker that breaks at spaces never breaks inside a cluster.
//!
//! This is a small subset of UAX #29's extended grapheme clusters, chosen to
//! keep emoji sequences and Latin marks whole; it has no Unicode tables. A
//! mark outside those blocks — Indic vowel signs, Hebrew points, Arabic
//! harakat — starts a cluster of its own here, although the shaper still
//! shapes it with its base when one face draws both.
//!
//! [`fallback::cluster_face`](super::fallback::cluster_face) picks one face
//! per cluster, layout never breaks a word inside one, and
//! [`crate::edit::LineEdit`]'s caret never stops inside one.

use core::ops::Range;

use super::fallback::ZERO_WIDTH_JOINER;

/// Whether `c` continues the cluster before it whatever that cluster is: a
/// combining mark, a variation selector, a zero-width joiner, a skin-tone
/// modifier or a tag. See the module docs.
fn extends(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{E0100}'..='\u{E01EF}'
            | ZERO_WIDTH_JOINER
            | '\u{1F3FB}'..='\u{1F3FF}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

/// Whether `c` is a regional indicator, half of a flag.
fn is_regional_indicator(c: char) -> bool {
    matches!(c, '\u{1F1E6}'..='\u{1F1FF}')
}

/// Whether `c` is always a cluster of its own: a space or a control char.
fn stands_alone(c: char) -> bool {
    c == ' ' || c.is_control()
}

/// Every cluster of `text`, in order, as the bytes it spans. See the module
/// docs for the rule. Empty text has none.
pub fn clusters(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut chars = text.char_indices().peekable();
    core::iter::from_fn(move || {
        let (start, base) = chars.next()?;
        let mut end = start + base.len_utf8();
        if stands_alone(base) {
            return Some(start..end);
        }
        // A regional indicator still waiting for the second of its pair.
        let mut open_flag = is_regional_indicator(base);
        let mut previous = base;
        while let Some(&(at, c)) = chars.peek() {
            let continues = !stands_alone(c)
                && (previous == ZERO_WIDTH_JOINER
                    || extends(c)
                    || (open_flag && is_regional_indicator(c)));
            if !continues {
                break;
            }
            open_flag = false;
            previous = c;
            end = at + c.len_utf8();
            chars.next();
        }
        Some(start..end)
    })
}

/// The `char` positions in `text` a caret may stop at: where each cluster
/// starts, and the text's end. Always starts with zero.
#[must_use]
pub fn caret_stops(text: &str) -> Vec<usize> {
    let mut stops = vec![0];
    let mut chars = 0;
    for cluster in clusters(text) {
        chars += text[cluster].chars().count();
        stops.push(chars);
    }
    stops
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `text` cut into its clusters.
    fn cut(text: &str) -> Vec<&str> {
        clusters(text).map(|range| &text[range]).collect()
    }

    /// **Each continuation joins its base, and nothing else does**: marks,
    /// selectors, ZWJ sequences, skin tones, keycaps, tag flags and
    /// regional-indicator pairs, beside plain chars each their own.
    #[test]
    fn the_cluster_rule_keeps_sequences_whole() {
        assert_eq!(cut("abc"), ["a", "b", "c"]);
        assert_eq!(cut("e\u{301}x"), ["e\u{301}", "x"]);
        assert_eq!(cut("q\u{301}\u{328}"), ["q\u{301}\u{328}"]);
        assert_eq!(cut("\u{2764}\u{FE0F}a"), ["\u{2764}\u{FE0F}", "a"]);
        // Woman, ZWJ, laptop: one.
        assert_eq!(
            cut("\u{1F469}\u{200D}\u{1F4BB}!"),
            ["\u{1F469}\u{200D}\u{1F4BB}", "!"]
        );
        // A thumbs-up with a skin tone, and a keycap.
        assert_eq!(cut("\u{1F44D}\u{1F3FD}"), ["\u{1F44D}\u{1F3FD}"]);
        assert_eq!(cut("1\u{FE0F}\u{20E3}2"), ["1\u{FE0F}\u{20E3}", "2"]);
        // England's flag: a black flag and its tags.
        let england = "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}";
        assert_eq!(cut(england), [england]);
        // Regional indicators pair up, left to right.
        let (us, gb) = ("\u{1F1FA}\u{1F1F8}", "\u{1F1EC}\u{1F1E7}");
        assert_eq!(cut(&format!("{us}{gb}")), [us, gb]);
        assert_eq!(
            cut("\u{1F1FA}\u{1F1F8}\u{1F1EC}"),
            [us, "\u{1F1EC}"],
            "an odd one out is alone"
        );
        assert_eq!(cut(""), Vec::<&str>::new());
    }

    /// **A space or a control char is always alone**: a mark after one does
    /// not join it, and a joiner before one does not reach it.
    #[test]
    fn spaces_and_controls_stand_alone() {
        assert_eq!(cut("a \u{301}"), ["a", " ", "\u{301}"]);
        assert_eq!(cut("a\u{200D} b"), ["a\u{200D}", " ", "b"]);
        assert_eq!(cut("a\u{200D}\nb"), ["a\u{200D}", "\n", "b"]);
        assert_eq!(cut("\u{301}a"), ["\u{301}", "a"], "a leading mark is alone");
    }

    /// **Caret stops are cluster starts in chars**, with the end.
    #[test]
    fn caret_stops_are_cluster_starts_in_chars() {
        assert_eq!(caret_stops(""), [0]);
        assert_eq!(caret_stops("ab"), [0, 1, 2]);
        assert_eq!(
            caret_stops("a\u{1F469}\u{200D}\u{1F4BB}e\u{301}"),
            [0, 1, 4, 6]
        );
    }
}
