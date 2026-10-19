//! Clusters: the runs of chars that draw, fall back, break and take the caret
//! as one.
//!
//! # The rule
//!
//! A cluster is an **extended grapheme cluster** of Unicode Standard Annex
//! #29, as `unicode-segmentation` finds them: a base char and the combining
//! marks, spacing marks, variation selectors and skin-tone modifiers after
//! it, an emoji ZWJ sequence such as `👩‍💻`, a flag's pair of regional
//! indicators — so `🇺🇸🇬🇧` is two clusters, not one or four — a tag
//! sequence, a Hangul syllable written in jamo, and a carriage return with
//! the line feed after it. A control char other than that pair is always a
//! cluster of its own. A space is not: a mark after one continues it, which
//! is how a lone diacritic is written.
//!
//! [`fallback::cluster_face`](super::fallback::cluster_face) picks one face
//! per cluster, layout never breaks a word inside one, and
//! [`crate::edit::LineEdit`]'s caret never stops inside one.

use core::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// Every cluster of `text`, in order, as the bytes it spans. See the module
/// docs for the rule. Empty text has none.
///
/// ASCII is cut without the tables: in it every char is a cluster of its own
/// but a carriage return and the line feed after it (rules GB3 to GB5 and
/// GB999), which is what most text a layout measures is.
pub fn clusters(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut graphemes = (!text.is_ascii()).then(|| text.grapheme_indices(true));
    let mut at = 0;
    core::iter::from_fn(move || {
        if let Some(graphemes) = &mut graphemes {
            return graphemes
                .next()
                .map(|(start, cluster)| start..start + cluster.len());
        }
        let start = at;
        let bytes = text.as_bytes();
        if start == bytes.len() {
            return None;
        }
        at += if bytes[start..].starts_with(b"\r\n") {
            2
        } else {
            1
        };
        Some(start..at)
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

    /// `text` written as UAX #29's test files write a case: each char's
    /// code point in hex, `×` between two chars of one cluster and `÷` at
    /// every boundary, the text's ends included.
    fn notation(text: &str) -> String {
        let mut out = String::from("÷");
        for cluster in cut(text) {
            let points: Vec<String> = cluster
                .chars()
                .map(|c| format!("{:04X}", c as u32))
                .collect();
            out.push(' ');
            out.push_str(&points.join(" × "));
            out.push_str(" ÷");
        }
        out
    }

    /// **The cut is UAX #29's**: cases from Unicode 17.0's
    /// `GraphemeBreakTest.txt` (checked against `unicode-segmentation`
    /// 1.13.3's transcription of it), each with the rule that decides it.
    #[test]
    fn the_cut_matches_the_grapheme_break_tests() {
        let cases = [
            // GB3: a carriage return and a line feed are one cluster.
            ("\u{D}\u{A}", "÷ 000D × 000A ÷"),
            // GB4: any other control stands alone.
            ("\u{0}\u{903}", "÷ 0000 ÷ 0903 ÷"),
            // GB6: Hangul L before V.
            ("\u{1100}\u{1160}", "÷ 1100 × 1160 ÷"),
            // GB9: an extending mark continues even a space.
            ("\u{20}\u{308}\u{20}", "÷ 0020 × 0308 ÷ 0020 ÷"),
            // GB9a and GB9b: a spacing mark continues what is before it, and
            // a prepended mark joins what is after it.
            ("\u{1F1E6}\u{903}", "÷ 1F1E6 × 0903 ÷"),
            ("\u{6DD}\u{1F1E6}", "÷ 06DD × 1F1E6 ÷"),
            // GB11: a joiner joins two pictographs, and nothing else.
            ("\u{1F6D1}\u{200D}\u{1F6D1}", "÷ 1F6D1 × 200D × 1F6D1 ÷"),
            ("\u{61}\u{200D}\u{1F6D1}", "÷ 0061 × 200D ÷ 1F6D1 ÷"),
            // GB12 and GB13: regional indicators pair up from the left.
            (
                "\u{61}\u{1F1E6}\u{1F1E7}\u{1F1E8}\u{62}",
                "÷ 0061 ÷ 1F1E6 × 1F1E7 ÷ 1F1E8 ÷ 0062 ÷",
            ),
        ];
        for (text, want) in cases {
            assert_eq!(notation(text), want, "{text:?}");
        }
    }

    /// **ASCII's cut without the tables is UAX #29's**: every pair of ASCII
    /// chars, and each with a carriage return and line feed around it, cuts
    /// as `unicode-segmentation` cuts it.
    #[test]
    fn the_ascii_cut_matches_the_tables() {
        let tables = |text: &str| -> Vec<Range<usize>> {
            text.grapheme_indices(true)
                .map(|(start, cluster)| start..start + cluster.len())
                .collect()
        };
        for a in 0..128u8 {
            for b in 0..128u8 {
                let pair = String::from_utf8(vec![a, b]).expect("ASCII");
                for text in [pair.clone(), format!("\r\n{pair}\r\n")] {
                    assert_eq!(
                        clusters(&text).collect::<Vec<_>>(),
                        tables(&text),
                        "{text:?}"
                    );
                }
            }
        }
        assert_eq!(cut("a\r\nb"), ["a", "\r\n", "b"]);
    }

    /// **The sequences a caret must not split are one cluster each**: a
    /// mark, a selector, a ZWJ family, a skin tone, a keycap and a tag flag,
    /// beside plain chars each their own.
    #[test]
    fn the_cluster_rule_keeps_sequences_whole() {
        assert_eq!(cut("abc"), ["a", "b", "c"]);
        assert_eq!(cut("e\u{301}x"), ["e\u{301}", "x"]);
        assert_eq!(cut("q\u{301}\u{328}"), ["q\u{301}\u{328}"]);
        assert_eq!(cut("\u{2764}\u{FE0F}a"), ["\u{2764}\u{FE0F}", "a"]);
        // Man, woman, girl and boy, joined: one.
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
        assert_eq!(cut(&format!("{family}!")), [family, "!"]);
        // A thumbs-up with a skin tone, and a keycap.
        assert_eq!(cut("\u{1F44D}\u{1F3FD}"), ["\u{1F44D}\u{1F3FD}"]);
        assert_eq!(cut("1\u{FE0F}\u{20E3}2"), ["1\u{FE0F}\u{20E3}", "2"]);
        // England's flag: a black flag and its tags.
        let england = "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}";
        assert_eq!(cut(england), [england]);
        // A Hebrew letter with its points, and Arabic with its harakat.
        assert_eq!(cut("\u{5E9}\u{5C1}\u{5B8}"), ["\u{5E9}\u{5C1}\u{5B8}"]);
        assert_eq!(cut("\u{628}\u{650}\u{633}"), ["\u{628}\u{650}", "\u{633}"]);
        assert_eq!(cut(""), Vec::<&str>::new());
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
