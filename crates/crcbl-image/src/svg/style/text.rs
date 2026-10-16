//! The properties that style text: the font asked for, its size, the
//! anchor and the spacing, and the text properties this renderer does not
//! lay out, which are kept so that text using them can be refused.
//!
//! Every one is inherited. The ones that change text in ways this renderer
//! does not draw ([`UNDRAWN`]) are recorded rather than refused where they
//! are given: on a shape they mean nothing, and Inkscape writes
//! text properties on shapes and groups as well, so only text that would
//! be drawn under one of their other values refuses the file.

use super::{Length, length, number_or_percent};
use crate::svg::path::number_prefix;

/// The font size text takes when nothing sets one, in user units: resvg's
/// default, since CSS leaves `medium` to the renderer (browsers take 16).
pub(in crate::svg) const DEFAULT_FONT_SIZE: f64 = 12.0;

/// Between adjacent font size keywords, the scale CSS 2 suggested and
/// resvg takes.
const KEYWORD_SCALE: f64 = 1.2;

/// A text property this renderer reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::svg) enum TextProperty {
    FontFamily,
    FontSize,
    FontStyle,
    FontWeight,
    LetterSpacing,
    TextAnchor,
    WordSpacing,
    /// One of [`UNDRAWN`], by name.
    Undrawn(&'static str),
}

const PROPERTIES: &[(&str, TextProperty)] = &[
    ("font-family", TextProperty::FontFamily),
    ("font-size", TextProperty::FontSize),
    ("font-style", TextProperty::FontStyle),
    ("font-weight", TextProperty::FontWeight),
    ("letter-spacing", TextProperty::LetterSpacing),
    ("text-anchor", TextProperty::TextAnchor),
    ("word-spacing", TextProperty::WordSpacing),
];

/// Text properties whose other values change text in ways this renderer
/// does not draw, each with the values that change nothing here: vertical
/// and right-to-left text, decorations, baselines, case and small caps,
/// widths and font variations, the `font` shorthand (any value), and SVG 2's
/// wrapped text.
const UNDRAWN: &[(&str, &[&str])] = &[
    ("alignment-baseline", &["auto", "baseline", "alphabetic"]),
    ("baseline-shift", &["baseline", "0"]),
    ("direction", &["ltr"]),
    ("dominant-baseline", &["auto", "alphabetic"]),
    ("font", &[]),
    ("font-size-adjust", &["none"]),
    ("font-stretch", &["normal", "100%"]),
    ("font-variant", &["normal"]),
    ("font-variant-caps", &["normal"]),
    ("font-variation-settings", &["normal"]),
    ("inline-size", &["0", "auto"]),
    ("shape-inside", &["none"]),
    ("text-decoration", &["none"]),
    ("text-decoration-line", &["none"]),
    ("text-transform", &["none"]),
    ("unicode-bidi", &["normal"]),
    ("white-space", &["normal", "nowrap"]),
    ("writing-mode", &["lr", "lr-tb", "horizontal-tb"]),
];

impl TextProperty {
    /// The text property called `name`, if it is one.
    pub(in crate::svg) fn named(name: &str) -> Option<Self> {
        PROPERTIES
            .iter()
            .find(|(known, _)| *known == name)
            .map(|&(_, property)| property)
            .or_else(|| {
                UNDRAWN
                    .iter()
                    .find(|(known, _)| *known == name)
                    .map(|&(known, _)| Self::Undrawn(known))
            })
    }

    /// Its name.
    pub(in crate::svg) fn name(self) -> &'static str {
        match self {
            Self::Undrawn(name) => name,
            _ => PROPERTIES
                .iter()
                .find(|(_, known)| *known == self)
                .map_or("?", |(name, _)| name),
        }
    }
}

/// One family of a `font-family` list.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::svg) enum Family {
    /// A family by name, as written (a quoted name or identifiers).
    Named(String),
    /// A generic family, an unquoted keyword: `serif`, `sans-serif`,
    /// `monospace` and the rest of CSS's.
    Generic(&'static str),
}

/// CSS's generic family keywords.
const GENERIC_FAMILIES: &[&str] = &[
    "cursive",
    "emoji",
    "fangsong",
    "fantasy",
    "math",
    "monospace",
    "sans-serif",
    "serif",
    "system-ui",
    "ui-monospace",
    "ui-rounded",
    "ui-sans-serif",
    "ui-serif",
];

/// Whether a face is upright or slanted, as `font-style` asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::svg) enum Slant {
    Normal,
    Italic,
    Oblique,
}

/// Where `text-anchor` puts a chunk of text against its position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::svg) enum Anchor {
    Start,
    Middle,
    End,
}

/// A spacing as given: in user units, or in ems of the font size of the
/// text it spaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::svg) enum Spacing {
    Px(f64),
    Em(f64),
}

impl Spacing {
    /// In user units, for text of `font_size`.
    #[cfg(any(feature = "svg-text", test))]
    pub(in crate::svg) fn resolve(self, font_size: f64) -> f64 {
        match self {
            Self::Px(value) => value,
            Self::Em(value) => value * font_size,
        }
    }
}

/// The computed text properties of one element.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::svg) struct TextStyle {
    /// The `font-family` list, in order; empty for none given.
    pub families: Vec<Family>,
    /// In user units.
    pub size: f64,
    /// From 1 to 1000, as CSS's `font-weight`.
    pub weight: u16,
    pub slant: Slant,
    pub anchor: Anchor,
    pub letter_spacing: Spacing,
    pub word_spacing: Spacing,
    /// Each [`UNDRAWN`] property in effect at a value that changes text,
    /// with that value: refused where text is drawn.
    pub undrawn: Vec<(&'static str, String)>,
}

impl TextStyle {
    pub(in crate::svg) fn initial() -> Self {
        Self {
            families: Vec::new(),
            size: DEFAULT_FONT_SIZE,
            weight: 400,
            slant: Slant::Normal,
            anchor: Anchor::Start,
            letter_spacing: Spacing::Px(0.0),
            word_spacing: Spacing::Px(0.0),
            undrawn: Vec::new(),
        }
    }

    /// Sets `property` from `value`, its parent's text style being `parent`:
    /// `Some(true)` if it applied, `Some(false)` if it is no value of the
    /// property, to be ignored, and `None` for a value in a form this
    /// renderer does not evaluate, to be refused.
    pub(in crate::svg) fn apply(
        &mut self,
        property: TextProperty,
        value: &str,
        parent: &Self,
    ) -> Option<bool> {
        match property {
            TextProperty::FontFamily => match families(value) {
                Some(families) => self.families = families,
                None => return Some(false),
            },
            TextProperty::FontSize => match font_size(value, parent.size) {
                Ok(size) => self.size = size,
                Err(undrawn) => return (!undrawn).then_some(false),
            },
            TextProperty::FontStyle => {
                self.slant = match value.split_ascii_whitespace().next() {
                    Some("normal") => Slant::Normal,
                    Some("italic") => Slant::Italic,
                    Some("oblique") => Slant::Oblique,
                    _ => return Some(false),
                };
            }
            TextProperty::FontWeight => match font_weight(value, parent.weight) {
                Some(weight) => self.weight = weight,
                None => return Some(false),
            },
            TextProperty::TextAnchor => {
                self.anchor = match value {
                    "start" => Anchor::Start,
                    "middle" => Anchor::Middle,
                    "end" => Anchor::End,
                    _ => return Some(false),
                };
            }
            TextProperty::LetterSpacing => match spacing(value) {
                Ok(spacing) => self.letter_spacing = spacing,
                Err(undrawn) => return (!undrawn).then_some(false),
            },
            TextProperty::WordSpacing => match spacing(value) {
                Ok(spacing) => self.word_spacing = spacing,
                Err(undrawn) => return (!undrawn).then_some(false),
            },
            TextProperty::Undrawn(name) => {
                self.undrawn.retain(|(known, _)| *known != name);
                let drawn = UNDRAWN
                    .iter()
                    .find(|(known, _)| *known == name)
                    .is_some_and(|(_, values)| {
                        values.contains(&value)
                            || values.contains(&"0") && length(value) == Some(Length::Px(0.0))
                    });
                if !drawn {
                    self.undrawn.push((name, value.to_owned()));
                }
            }
        }
        Some(true)
    }

    /// Takes `property`'s value from `parent`, for `inherit`.
    pub(in crate::svg) fn inherit(&mut self, property: TextProperty, parent: &Self) {
        match property {
            TextProperty::FontFamily => self.families = parent.families.clone(),
            TextProperty::FontSize => self.size = parent.size,
            TextProperty::FontStyle => self.slant = parent.slant,
            TextProperty::FontWeight => self.weight = parent.weight,
            TextProperty::LetterSpacing => self.letter_spacing = parent.letter_spacing,
            TextProperty::TextAnchor => self.anchor = parent.anchor,
            TextProperty::WordSpacing => self.word_spacing = parent.word_spacing,
            TextProperty::Undrawn(name) => {
                self.undrawn.retain(|(known, _)| *known != name);
                self.undrawn.extend(
                    parent
                        .undrawn
                        .iter()
                        .filter(|(known, _)| *known == name)
                        .cloned(),
                );
            }
        }
    }
}

/// A `font-family` list: comma-separated families, each a quoted string or
/// identifiers separated by white space (one space between them in the
/// name), an unquoted generic keyword being the generic family. `None` for
/// a list with an empty or malformed entry.
pub(in crate::svg) fn families(text: &str) -> Option<Vec<Family>> {
    let mut out = Vec::new();
    for entry in text.split(',') {
        let entry = entry.trim();
        let quote = entry.chars().next().filter(|c| matches!(c, '"' | '\''));
        let family = match quote {
            Some(quote) => {
                let name = entry.strip_prefix(quote)?.strip_suffix(quote)?;
                if name.contains(quote) {
                    return None;
                }
                Family::Named(name.to_owned())
            }
            None => {
                let words: Vec<&str> = entry.split_ascii_whitespace().collect();
                if words.is_empty() || words.iter().any(|word| word.contains(['"', '\''])) {
                    return None;
                }
                match GENERIC_FAMILIES
                    .iter()
                    .find(|generic| words.len() == 1 && generic.eq_ignore_ascii_case(words[0]))
                {
                    Some(generic) => Family::Generic(generic),
                    None => Family::Named(words.join(" ")),
                }
            }
        };
        out.push(family);
    }
    Some(out)
}

/// A `font-size` given a parent's of `parent`: a length, a percentage or
/// `em` of the parent's (`ex` as half an `em`, as resvg reads it), or a
/// keyword. `Err(true)` for a form this renderer does not evaluate (a unit
/// such as `rem` or `vw`), `Err(false)` for no font size at all.
fn font_size(value: &str, parent: f64) -> Result<f64, bool> {
    const ABSOLUTE: &[(&str, i32)] = &[
        ("xx-small", -3),
        ("x-small", -2),
        ("small", -1),
        ("medium", 0),
        ("large", 1),
        ("x-large", 2),
        ("xx-large", 3),
        ("xxx-large", 4),
    ];
    if let Some(&(_, step)) = ABSOLUTE.iter().find(|(name, _)| *name == value) {
        return Ok(DEFAULT_FONT_SIZE * KEYWORD_SCALE.powi(step));
    }
    match value {
        "smaller" => return Ok(parent / KEYWORD_SCALE),
        "larger" => return Ok(parent * KEYWORD_SCALE),
        _ => {}
    }
    let size = match length(value) {
        Some(Length::Px(size)) => size,
        Some(Length::Percent(percent)) => percent / 100.0 * parent,
        None => match number_prefix(value.trim()) {
            Some((em, "em")) => em * parent,
            Some((ex, "ex")) => ex * parent / 2.0,
            Some((_, unit)) => return Err(is_unit(unit)),
            None => return Err(false),
        },
    };
    if size < 0.0 || !size.is_finite() {
        return Err(false);
    }
    Ok(size)
}

/// Whether `text` is a unit's name, letters only.
fn is_unit(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_alphabetic())
}

/// A `font-weight` given a parent's of `parent`: a keyword, or a number
/// from 1 to 1000; `bolder` and `lighter` by CSS Fonts 4's table of
/// relative weights.
fn font_weight(value: &str, parent: u16) -> Option<u16> {
    Some(match value {
        "normal" => 400,
        "bold" => 700,
        "bolder" => match parent {
            0..350 => 400,
            350..550 => 700,
            _ => 900,
        },
        "lighter" => match parent {
            0..100 => parent,
            100..550 => 100,
            550..750 => 400,
            _ => 700,
        },
        _ => {
            let weight = number_or_percent(value).filter(|_| !value.contains('%'))?;
            if !(1.0..=1000.0).contains(&weight) {
                return None;
            }
            weight.round() as u16
        }
    })
}

/// A `letter-spacing` or `word-spacing`: `normal`, a length or `em`.
/// `Err(true)` for a unit this renderer does not evaluate, `Err(false)` for
/// no spacing at all (a percentage included, which neither takes in SVG).
fn spacing(value: &str) -> Result<Spacing, bool> {
    if value == "normal" {
        return Ok(Spacing::Px(0.0));
    }
    match length(value) {
        Some(Length::Px(value)) => Ok(Spacing::Px(value)),
        Some(Length::Percent(_)) => Err(false),
        None => match number_prefix(value.trim()) {
            Some((em, "em")) => Ok(Spacing::Em(em)),
            Some((ex, "ex")) => Ok(Spacing::Em(ex / 2.0)),
            Some((_, unit)) => Err(is_unit(unit)),
            None => Err(false),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A family list parses as CSS writes it**: quoted names keep their
    /// spaces, unquoted identifiers join with one, and only an unquoted
    /// keyword is a generic family.
    #[test]
    fn family_lists_parse() {
        assert_eq!(
            families(
                r#"'Noto Sans', "DejaVu  Serif", Bitstream   Vera Sans, sans-serif, 'serif', MONOSPACE"#
            ),
            Some(vec![
                Family::Named("Noto Sans".to_owned()),
                Family::Named("DejaVu  Serif".to_owned()),
                Family::Named("Bitstream Vera Sans".to_owned()),
                Family::Generic("sans-serif"),
                Family::Named("serif".to_owned()),
                Family::Generic("monospace"),
            ])
        );
        for bad in ["", "a,,b", "'unclosed", "a 'b'", "'a'b'"] {
            assert_eq!(families(bad), None, "{bad}");
        }
    }

    /// **Font sizes resolve against the parent's**, keywords against the
    /// default, and a unit this renderer does not resolve is told apart from
    /// no size at all.
    #[test]
    fn font_sizes_resolve() {
        assert_eq!(font_size("10", 20.0), Ok(10.0));
        assert_eq!(font_size("12pt", 20.0), Ok(16.0));
        assert_eq!(font_size("50%", 20.0), Ok(10.0));
        assert_eq!(font_size("1.5em", 20.0), Ok(30.0));
        assert_eq!(font_size("1ex", 20.0), Ok(10.0));
        assert_eq!(font_size("medium", 20.0), Ok(DEFAULT_FONT_SIZE));
        assert_eq!(font_size("large", 20.0), Ok(DEFAULT_FONT_SIZE * 1.2));
        assert_eq!(font_size("larger", 20.0), Ok(24.0));
        assert_eq!(font_size("smaller", 24.0), Ok(20.0));
        assert_eq!(font_size("2rem", 20.0), Err(true));
        assert_eq!(font_size("-1", 20.0), Err(false));
        assert_eq!(font_size("big", 20.0), Err(false));
    }

    /// **Weights take CSS Fonts 4's relative table**, and a number must lie
    /// from 1 to 1000.
    #[test]
    fn font_weights_resolve() {
        assert_eq!(font_weight("bold", 400), Some(700));
        assert_eq!(font_weight("normal", 700), Some(400));
        assert_eq!(font_weight("650", 400), Some(650));
        assert_eq!(font_weight("1001", 400), None);
        assert_eq!(font_weight("0", 400), None);
        assert_eq!(font_weight("50%", 400), None);
        for (parent, bolder, lighter) in [
            (50, 400, 50),
            (300, 400, 100),
            (400, 700, 100),
            (600, 900, 400),
            (800, 900, 700),
        ] {
            assert_eq!(font_weight("bolder", parent), Some(bolder), "{parent}");
            assert_eq!(font_weight("lighter", parent), Some(lighter), "{parent}");
        }
    }

    /// **Spacings are lengths or ems**; a percentage is no spacing.
    #[test]
    fn spacings_resolve() {
        assert_eq!(spacing("normal"), Ok(Spacing::Px(0.0)));
        assert_eq!(spacing("2px"), Ok(Spacing::Px(2.0)));
        assert_eq!(spacing("0.5em").map(|s| s.resolve(10.0)), Ok(5.0));
        assert_eq!(spacing("1ex").map(|s| s.resolve(10.0)), Ok(5.0));
        assert_eq!(spacing("10%"), Err(false));
        assert_eq!(spacing("1rem"), Err(true));
    }

    /// **An undrawn property is recorded at a value that changes text, and
    /// forgotten at one that does not**, so a child can set it back.
    #[test]
    fn undrawn_values_are_recorded() {
        let parent = TextStyle::initial();
        let mut style = TextStyle::initial();
        let writing = TextProperty::named("writing-mode").expect("known");
        assert_eq!(style.apply(writing, "tb-rl", &parent), Some(true));
        assert_eq!(style.undrawn, [("writing-mode", "tb-rl".to_owned())]);
        let child_parent = style.clone();
        assert_eq!(style.apply(writing, "lr-tb", &child_parent), Some(true));
        assert!(style.undrawn.is_empty());
        let shift = TextProperty::named("baseline-shift").expect("known");
        assert_eq!(style.apply(shift, "0px", &parent), Some(true));
        assert!(style.undrawn.is_empty());
        assert_eq!(style.apply(shift, "super", &parent), Some(true));
        assert_eq!(style.undrawn, [("baseline-shift", "super".to_owned())]);
        style.inherit(shift, &parent);
        assert!(style.undrawn.is_empty());
        let shorthand = TextProperty::named("font").expect("known");
        assert_eq!(style.apply(shorthand, "12px serif", &parent), Some(true));
        assert_eq!(style.undrawn, [("font", "12px serif".to_owned())]);
    }
}
