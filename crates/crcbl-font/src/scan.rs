//! Finding faces by walking font directories: the source where the system
//! keeps no index of its own that this crate reads.
//!
//! The walk reads only the tables a face is matched by (see
//! [`crate::sfnt`]), once per [`Catalog`], on its first lookup; the index it
//! builds is kept for every lookup after. A file that cannot be read, or is
//! not a font, is skipped.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::face::{self, Face};

/// The extensions of the font files a walk reads: TrueType and OpenType,
/// single and collected. Bitmap formats and WOFF are left out, since no
/// font parser this crate serves reads them.
const FONT_EXTENSIONS: [&str; 4] = ["ttf", "otf", "ttc", "otc"];

/// The faces under a set of directories, indexed on first use.
#[derive(Debug)]
pub(crate) struct Catalog {
    dirs: Vec<PathBuf>,
    faces: OnceLock<Vec<Face>>,
}

impl Catalog {
    /// A catalog of `dirs`, read nothing of yet.
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self {
            dirs,
            faces: OnceLock::new(),
        }
    }

    /// Every face of the family `family`, walking the directories first if
    /// nothing has.
    pub fn faces_of(&self, family: &str) -> Vec<Face> {
        self.faces
            .get_or_init(|| scan(&self.dirs))
            .iter()
            .filter(|face| face.answers_to(family))
            .cloned()
            .collect()
    }
}

/// Every face of every font file under `dirs`, recursively, in path order.
/// A directory reached twice — through a link, or named twice — is walked
/// once.
fn scan(dirs: &[PathBuf]) -> Vec<Face> {
    let mut visited = HashSet::new();
    let mut pending: Vec<PathBuf> = dirs.to_vec();
    let mut files = Vec::new();
    while let Some(dir) = pending.pop() {
        let Ok(canonical) = fs::canonicalize(&dir) else {
            continue;
        };
        if !visited.insert(canonical) {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // `metadata` follows links, so a linked directory is walked and
            // a linked font read.
            match fs::metadata(&path) {
                Ok(meta) if meta.is_dir() => pending.push(path),
                Ok(meta) if meta.is_file() && is_font_file(&path) => files.push(path),
                _ => {}
            }
        }
    }
    files.sort();
    files.dedup();
    files
        .iter()
        .filter_map(|path| face::read_faces(path).ok())
        .flatten()
        .collect()
}

fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            FONT_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face::tests::restyled;
    use crate::sfnt::tests::{EMOJI_TEXT, SANS, collection};
    use crate::{FontStyle, matching};

    #[test]
    fn a_walk_finds_every_face_under_nested_directories_and_skips_the_rest() {
        let root = crate::tests::temp_dir("scan-walk");
        let nested = root.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("Sans.TTF"), SANS).unwrap();
        fs::write(nested.join("pair.ttc"), collection(&[EMOJI_TEXT, SANS])).unwrap();
        fs::write(nested.join("broken.otf"), b"OTTO and nothing else").unwrap();
        fs::write(nested.join("notes.txt"), SANS).unwrap();

        // The root named twice, and its child as well: still one walk each.
        let catalog = Catalog::new(vec![root.to_path_buf(), root.to_path_buf(), root.join("a")]);
        let faces = catalog.faces_of("atkinson hyperlegible");
        let places: Vec<_> = faces
            .iter()
            .map(|face| {
                (
                    face.path.strip_prefix(&*root).unwrap().to_owned(),
                    face.index,
                )
            })
            .collect();
        assert_eq!(
            places,
            [
                (Path::new("Sans.TTF").to_owned(), 0),
                (Path::new("a").join("b").join("pair.ttc"), 1),
            ]
        );
        assert_eq!(catalog.faces_of("no such family"), []);
        assert_eq!(
            Catalog::new(vec![root.join("missing")]).faces_of("Atkinson Hyperlegible"),
            []
        );
    }

    #[test]
    fn a_walked_family_is_matched_by_weight_and_style() {
        let root = crate::tests::temp_dir("scan-match");
        fs::write(root.join("regular.ttf"), SANS).unwrap();
        fs::write(root.join("bold.ttf"), restyled(SANS, 700, false)).unwrap();
        fs::write(root.join("light-italic.ttf"), restyled(SANS, 300, true)).unwrap();
        let faces = Catalog::new(vec![root.to_path_buf()]).faces_of("Atkinson Hyperlegible");
        let attributes: Vec<_> = faces.iter().map(|face| face.attributes).collect();
        let pick = |weight, style| {
            let at = matching::best(&attributes, weight, style).unwrap();
            faces[at]
                .path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned()
        };
        assert_eq!(pick(700, FontStyle::Normal), "bold.ttf");
        assert_eq!(pick(600, FontStyle::Normal), "bold.ttf");
        assert_eq!(pick(400, FontStyle::Normal), "regular.ttf");
        assert_eq!(pick(700, FontStyle::Italic), "light-italic.ttf");
    }
}
