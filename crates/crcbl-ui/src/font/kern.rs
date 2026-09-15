//! Pair kerning from the GPOS table's `kern` feature.
//!
//! # What is read
//!
//! The `kern` feature of the `latn` script's default language system — or
//! `DFLT`'s, for a font with no `latn` — and every pair-adjustment lookup
//! (type 2, directly or behind an extension) that feature names. For each
//! lookup, subtables are tried in order and **the first that covers a pair
//! decides it**, which is OpenType's rule: a format 1 subtable covers a pair
//! when its pair set holds the second glyph, and a format 2 subtable covers
//! every pair whose first glyph is in its coverage, class zero included. The
//! adjustments of different lookups add.
//!
//! Only `valueRecord1.xAdvance` is read — the first glyph's advance, which is
//! how left-to-right kerning is written. Placement and the second glyph's
//! record are not applied; the committed font writes neither, which a test
//! below holds it to.
//!
//! No `kern` table fallback: the committed font has GPOS, and a font with only
//! the legacy table draws unkerned.

use std::collections::{HashMap, HashSet};

use skrifa::FontRef;
use skrifa::raw::tables::gpos::{PairPos, PositionLookup, PositionSubtables};
use skrifa::raw::tables::layout::{FeatureList, ScriptList};
use skrifa::raw::types::{GlyphId16, Tag};
use skrifa::raw::{ReadError, TableProvider};

const KERN: Tag = Tag::new(b"kern");

/// The scripts whose default language system is read, in order of preference.
const SCRIPTS: [Tag; 2] = [Tag::new(b"latn"), Tag::new(b"DFLT")];

/// Every non-zero pair adjustment between two of `glyphs`, in font units.
///
/// A font with no GPOS table has no kerning and is not an error.
///
/// # Errors
///
/// When a table on the way to a pair adjustment cannot be read.
pub(super) fn pair_kerning(
    font: &FontRef<'_>,
    glyphs: &[u32],
) -> Result<HashMap<(u32, u32), f32>, ReadError> {
    let Ok(gpos) = font.gpos() else {
        return Ok(HashMap::new());
    };
    let lookups = kern_lookups(&gpos.script_list()?, &gpos.feature_list()?)?;
    let list = gpos.lookup_list()?;
    let glyphs: Vec<GlyphId16> = glyphs
        .iter()
        .filter_map(|&glyph| u16::try_from(glyph).ok().map(GlyphId16::new))
        .collect();

    let mut kerning: HashMap<(u32, u32), f32> = HashMap::new();
    for index in lookups {
        let lookup = list.lookups().get(usize::from(index))?;
        add_lookup(&lookup, &glyphs, &mut kerning)?;
    }
    kerning.retain(|_, adjustment| *adjustment != 0.0);
    Ok(kerning)
}

/// The lookup indices the preferred script's `kern` feature names, in lookup
/// list order.
fn kern_lookups(
    scripts: &ScriptList<'_>,
    features: &FeatureList<'_>,
) -> Result<Vec<u16>, ReadError> {
    let Some(record) = SCRIPTS.iter().find_map(|tag| {
        scripts
            .script_records()
            .iter()
            .find(|record| record.script_tag() == *tag)
    }) else {
        return Ok(Vec::new());
    };
    let script = record.script(scripts.offset_data())?;
    let Some(language) = script.default_lang_sys().transpose()? else {
        return Ok(Vec::new());
    };

    let mut lookups = Vec::new();
    for feature_index in language.feature_indices() {
        let Some(record) = features
            .feature_records()
            .get(usize::from(feature_index.get()))
        else {
            continue;
        };
        if record.feature_tag() != KERN {
            continue;
        }
        let feature = record.feature(features.offset_data())?;
        lookups.extend(
            feature
                .lookup_list_indices()
                .iter()
                .map(|index| index.get()),
        );
    }
    lookups.sort_unstable();
    lookups.dedup();
    Ok(lookups)
}

/// Adds one lookup's pair adjustments to `kerning`. See the module docs for
/// which subtable decides a pair.
fn add_lookup(
    lookup: &PositionLookup<'_>,
    glyphs: &[GlyphId16],
    kerning: &mut HashMap<(u32, u32), f32>,
) -> Result<(), ReadError> {
    let PositionSubtables::Pair(subtables) = lookup.subtables()? else {
        return Ok(());
    };
    let wanted: HashSet<GlyphId16> = glyphs.iter().copied().collect();
    let mut decided: HashSet<(GlyphId16, GlyphId16)> = HashSet::new();
    for subtable in subtables.iter() {
        match subtable? {
            PairPos::Format1(table) => {
                let coverage = table.coverage()?;
                let sets = table.pair_sets();
                for &first in glyphs {
                    let Some(at) = coverage.get(first) else {
                        continue;
                    };
                    let set = sets.get(usize::from(at))?;
                    for record in set.pair_value_records().iter() {
                        let record = record?;
                        let second = record.second_glyph();
                        if !wanted.contains(&second) || !decided.insert((first, second)) {
                            continue;
                        }
                        let adjustment = record.value_record1().x_advance().unwrap_or(0);
                        add(kerning, first, second, adjustment);
                    }
                }
            }
            PairPos::Format2(table) => {
                let coverage = table.coverage()?;
                let class_def1 = table.class_def1()?;
                let class_def2 = table.class_def2()?;
                let records = table.class_value_records();
                for &first in glyphs {
                    if coverage.get(first).is_none() {
                        continue;
                    }
                    let class1 = class_def1.get(first);
                    for &second in glyphs {
                        // A class past the table's counts does not cover the
                        // pair, and leaves it to a later subtable.
                        let Some([value, _]) = records.get(class1, class_def2.get(second)) else {
                            continue;
                        };
                        if decided.insert((first, second)) {
                            add(kerning, first, second, value.x_advance().unwrap_or(0));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn add(kerning: &mut HashMap<(u32, u32), f32>, first: GlyphId16, second: GlyphId16, value: i16) {
    if value != 0 {
        *kerning
            .entry((u32::from(first.to_u16()), u32::from(second.to_u16())))
            .or_insert(0.0) += f32::from(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{Font, SANS_TTF};
    use skrifa::raw::tables::gpos::ValueFormat;

    /// **Every pair adjustment the committed font's `kern` lookups hold is a
    /// first-glyph x-advance and nothing else**, which is what makes reading
    /// only that field exact for it.
    #[test]
    fn the_committed_fonts_kerning_is_first_glyph_advance_only() {
        let font = FontRef::new(SANS_TTF).expect("reads");
        let gpos = font.gpos().expect("the committed font has GPOS");
        let lookups = kern_lookups(
            &gpos.script_list().expect("reads"),
            &gpos.feature_list().expect("reads"),
        )
        .expect("reads");
        assert!(!lookups.is_empty(), "no kern lookup was found");
        let list = gpos.lookup_list().expect("reads");
        let mut subtables_seen = 0;
        for index in lookups {
            let lookup = list.lookups().get(usize::from(index)).expect("reads");
            let PositionSubtables::Pair(subtables) = lookup.subtables().expect("reads") else {
                panic!("kern lookup {index} is not a pair adjustment");
            };
            for subtable in subtables.iter() {
                let (first, second) = match subtable.expect("reads") {
                    PairPos::Format1(table) => (table.value_format1(), table.value_format2()),
                    PairPos::Format2(table) => (table.value_format1(), table.value_format2()),
                };
                assert_eq!(first, ValueFormat::X_ADVANCE);
                assert_eq!(second, ValueFormat::empty());
                subtables_seen += 1;
            }
        }
        assert!(subtables_seen > 0);
    }

    /// The adjustment of `(first, second)` read straight from the font's `kern`
    /// lookups: for each, the first pair subtable that covers the pair —
    /// written out for one pair at a time, independently of the bulk read.
    fn read_pair(
        list: &skrifa::raw::tables::gpos::PositionLookupList<'_>,
        lookups: &[u16],
        first: GlyphId16,
        second: GlyphId16,
    ) -> f32 {
        let mut total = 0.0;
        for &index in lookups {
            let lookup = list.lookups().get(usize::from(index)).expect("reads");
            let PositionSubtables::Pair(subtables) = lookup.subtables().expect("reads") else {
                continue;
            };
            for subtable in subtables.iter() {
                let decided = match subtable.expect("reads") {
                    PairPos::Format1(table) => table
                        .coverage()
                        .expect("reads")
                        .get(first)
                        .and_then(|at| {
                            table
                                .pair_sets()
                                .get(usize::from(at))
                                .expect("reads")
                                .find_pair_value_record(second)
                        })
                        .map(|record| record.value_record1().x_advance().unwrap_or(0)),
                    PairPos::Format2(table) => {
                        table.coverage().expect("reads").get(first).and_then(|_| {
                            let class1 = table.class_def1().expect("reads").get(first);
                            let class2 = table.class_def2().expect("reads").get(second);
                            table
                                .value_record_refs(class1, class2)
                                .map(|[value, _]| value.x_advance().unwrap_or(0))
                        })
                    }
                };
                if let Some(value) = decided {
                    total += f32::from(value);
                    break;
                }
            }
        }
        total
    }

    /// **The kerning table holds what the font says for every pair of printable
    /// Latin-1 glyphs**, pair by pair, and the pairs a type designer always
    /// kerns are kerned.
    #[test]
    fn every_latin1_pair_matches_the_font() {
        let font = Font::sans();
        let reference = FontRef::new(SANS_TTF).expect("reads");
        let gpos = reference.gpos().expect("GPOS");
        let list = gpos.lookup_list().expect("reads");
        let lookups = kern_lookups(
            &gpos.script_list().expect("reads"),
            &gpos.feature_list().expect("reads"),
        )
        .expect("reads");
        let glyph16 =
            |c: char| GlyphId16::new(u16::try_from(font.glyph_id(c).0).expect("glyf ids fit"));
        let printable: Vec<char> = (0x20u32..0x7f)
            .chain(0xa0..0x100)
            .filter_map(char::from_u32)
            .collect();
        let mut kerned = 0;
        for &left in &printable {
            for &right in &printable {
                let want = read_pair(&list, &lookups, glyph16(left), glyph16(right));
                let got = font.kerning(font.glyph_id(left), font.glyph_id(right));
                assert_eq!(got, want, "{left:?}{right:?}");
                kerned += usize::from(want != 0.0);
            }
        }
        assert!(kerned > 0);
        for (left, right) in [('A', 'V'), ('T', 'o')] {
            let adjustment = read_pair(&list, &lookups, glyph16(left), glyph16(right));
            assert!(adjustment < 0.0, "{left}{right} is not kerned together");
        }
    }
}
