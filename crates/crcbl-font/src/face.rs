//! One face of a font file as matching sees it: where it is, the family
//! names it answers to, and its weight, style and width.

use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use skrifa::attribute::Style;
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};

use crate::matching::Attributes;
use crate::{FontMatch, FontStyle, sfnt};

/// A face of a font file on disk.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Face {
    pub path: PathBuf,
    /// Its index in a collection; 0 in a single font.
    pub index: u32,
    /// Every family name its name table gives, typographic (ID 16) and
    /// legacy (ID 1), in every language: what a query's family is compared
    /// with.
    pub families: Vec<String>,
    /// The name it reports: the typographic family, else the legacy one,
    /// each in English where the table has English.
    pub family: String,
    pub attributes: Attributes,
}

impl Face {
    /// Whether `family` is one of its names, by [`same_family`].
    pub fn answers_to(&self, family: &str) -> bool {
        self.families.iter().any(|name| same_family(name, family))
    }

    pub fn to_match(&self) -> FontMatch {
        FontMatch {
            path: self.path.clone(),
            index: self.index,
            family: self.family.clone(),
            weight: self.attributes.weight,
            style: self.attributes.style,
        }
    }
}

/// Whether two family names name one family: equal but for case and the
/// space around them, as CSS compares the names a `font-family` list gives.
pub(crate) fn same_family(a: &str, b: &str) -> bool {
    a.trim()
        .chars()
        .flat_map(char::to_lowercase)
        .eq(b.trim().chars().flat_map(char::to_lowercase))
}

/// Every face of the font file at `path`, read through only the tables
/// [`describe`] needs. A face those tables do not describe is left out.
///
/// # Errors
///
/// The file cannot be opened, or is not a font file.
pub(crate) fn read_faces(path: &Path) -> io::Result<Vec<Face>> {
    let mut file = BufReader::new(File::open(path)?);
    let faces = sfnt::describe_tables(&mut file)?;
    Ok(faces
        .into_iter()
        .zip(0..)
        .filter_map(|(tables, index)| {
            let tables = tables?;
            let font = FontRef::new(&tables).ok()?;
            let (families, family, attributes) = describe(&font)?;
            Some(Face {
                path: path.to_owned(),
                index,
                families,
                family,
                attributes,
            })
        })
        .collect())
}

/// What a face's tables say about it: its family names, the one it reports,
/// and its attributes. `None` for a face with no family name.
pub(crate) fn describe(font: &FontRef<'_>) -> Option<(Vec<String>, String, Attributes)> {
    let primary = [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
        .into_iter()
        .find_map(|id| {
            let name = font.localized_strings(id).english_or_first()?.to_string();
            (!name.trim().is_empty()).then_some(name)
        })?;
    let mut families = vec![primary.clone()];
    for id in [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME] {
        for name in font.localized_strings(id) {
            let name = name.to_string();
            if !name.trim().is_empty() && !families.iter().any(|known| same_family(known, &name)) {
                families.push(name);
            }
        }
    }
    let attributes = font.attributes();
    let style = match attributes.style {
        Style::Normal => FontStyle::Normal,
        Style::Italic => FontStyle::Italic,
        Style::Oblique(_) => FontStyle::Oblique,
    };
    Some((
        families,
        primary,
        Attributes {
            // `skrifa` reads `usWeightClass`, a `u16`, as a float, so the
            // cast only undoes that.
            weight: attributes.weight.value() as u16,
            style,
            stretch: attributes.stretch.ratio(),
        },
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::matching::NORMAL_STRETCH;
    use crate::sfnt::tests::{EMOJI_TEXT, SANS, collection};

    /// `font` with its `OS/2` weight class and italic bit replaced: a face
    /// of another weight or style without a font file of its own.
    pub(crate) fn restyled(font: &[u8], weight: u16, italic: bool) -> Vec<u8> {
        let mut out = font.to_vec();
        let count = usize::from(u16::from_be_bytes([font[4], font[5]]));
        let record = (0..count)
            .map(|record| 12 + 16 * record)
            .find(|&at| &font[at..at + 4] == b"OS/2")
            .expect("an OS/2 table");
        let offset = u32::from_be_bytes(font[record + 8..record + 12].try_into().unwrap()) as usize;
        // `usWeightClass` at 4, `fsSelection` at 62; its bit 0 is italic and
        // bit 6 regular.
        out[offset + 4..offset + 6].copy_from_slice(&weight.to_be_bytes());
        let selection = u16::from_be_bytes([out[offset + 62], out[offset + 63]]);
        let selection = if italic {
            (selection | 1) & !(1 << 6)
        } else {
            selection & !1
        };
        out[offset + 62..offset + 64].copy_from_slice(&selection.to_be_bytes());
        out
    }

    #[test]
    fn the_committed_font_describes_as_atkinson_hyperlegible_regular() {
        let (families, family, attributes) = describe(&FontRef::new(SANS).unwrap()).unwrap();
        assert_eq!(family, "Atkinson Hyperlegible");
        assert!(families.iter().any(|name| name == "Atkinson Hyperlegible"));
        assert_eq!(
            attributes,
            Attributes {
                weight: 400,
                style: FontStyle::Normal,
                stretch: NORMAL_STRETCH,
            }
        );
    }

    #[test]
    fn a_restyled_face_reads_its_new_weight_and_style() {
        let bold_italic = restyled(SANS, 700, true);
        let (_, family, attributes) = describe(&FontRef::new(&bold_italic).unwrap()).unwrap();
        assert_eq!(family, "Atkinson Hyperlegible");
        assert_eq!(attributes.weight, 700);
        assert_eq!(attributes.style, FontStyle::Italic);
    }

    #[test]
    fn faces_read_from_disk_match_what_the_whole_font_says() {
        let dir = crate::tests::temp_dir("faces");
        let ttc = dir.join("pair.ttc");
        std::fs::write(&ttc, collection(&[EMOJI_TEXT, SANS])).unwrap();
        let faces = read_faces(&ttc).unwrap();
        assert_eq!(faces.len(), 2);
        for (face, original) in faces.iter().zip([EMOJI_TEXT, SANS]) {
            let whole = describe(&FontRef::new(original).unwrap()).unwrap();
            assert_eq!(
                (&face.families, &face.family, face.attributes),
                (&whole.0, &whole.1, whole.2)
            );
            assert_eq!(face.path, ttc);
        }
        assert_eq!(faces[0].index, 0);
        assert_eq!(faces[1].index, 1);
        assert_eq!(faces[1].family, "Atkinson Hyperlegible");

        let junk = dir.join("junk.ttf");
        std::fs::write(&junk, b"not a font at all").unwrap();
        assert!(read_faces(&junk).is_err());
    }

    #[test]
    fn family_names_compare_without_case_or_surrounding_space() {
        assert!(same_family("DejaVu Sans", "dejavu sans"));
        assert!(same_family(" Segoe UI", "SEGOE UI "));
        assert!(same_family("Ärial", "ärial"));
        assert!(!same_family("DejaVu Sans", "DejaVu Sans Mono"));
        assert!(!same_family("DejaVu Sans", "DejaVuSans"));
    }
}
