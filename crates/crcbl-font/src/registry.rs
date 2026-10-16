//! Reading the Windows registry's font values, as pure functions over what
//! the registry returned: compiled on every target for its tests, used on
//! Windows by [`crate::platform`].
//!
//! Two keys are read. `Fonts` maps a display name to a file:
//!
//! ```text
//! Arial Bold (TrueType)                 REG_SZ  arialbd.ttf
//! Cambria & Cambria Math (TrueType)     REG_SZ  cambria.ttc
//! HackNerdFont-Bold (TrueType)          REG_SZ  C:\Users\…\Fonts\HackNerdFont-Bold.ttf
//! ```
//!
//! The name is a hint, not a family: it may carry the style, list the faces
//! of a collection, or be a PostScript name, depending on what installed the
//! font. So it only narrows which files are opened, and each file's name
//! table decides. `FontLink\SystemLink` maps a family to the faces Windows
//! links to it for the chars it lacks, one `file,face[,scale,scale]` string
//! per face.

use std::path::{Path, PathBuf};

use crate::face;

/// The names a `Fonts` value's name lists: the text before its
/// parenthesised format tag, split at each `&`.
///
/// `Cambria & Cambria Math (TrueType)` lists `Cambria` and `Cambria Math`.
pub(crate) fn value_names(value: &str) -> Vec<String> {
    let names = value.rfind('(').map_or(value, |tag| &value[..tag]);
    names
        .split('&')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether a `Fonts` value listing `names` may hold a face of `family`: one
/// of the names, reduced as [`reduced`] does, starts with the family reduced
/// the same way. Generous on purpose — `Arial` admits `Arial Black` and
/// `Arial Bold`, `Hack Nerd Font` admits `HackNerdFont-Bold` — since the
/// name table, not this, decides.
pub(crate) fn may_hold(names: &[String], family: &str) -> bool {
    let family = reduced(family);
    !family.is_empty() && names.iter().any(|name| reduced(name).starts_with(&family))
}

/// `name` with everything but letters and digits dropped and case folded:
/// what a display name, a family name and a PostScript name of one family
/// have in common.
fn reduced(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The file a `Fonts` value's data names, or `None` when it is not a font
/// file this crate reads (a bitmap `.fon`, say). A bare file name is in `fonts_dir`; a full path,
/// as per-user installs write, stays as it is.
pub(crate) fn font_path(data: &str, fonts_dir: &Path) -> Option<PathBuf> {
    let data = data.trim();
    face::is_font_file(Path::new(data)).then(|| fonts_dir.join(data))
}

/// One `SystemLink` string: its file, and the face it names if it names
/// one. The scale factors some entries end with are not read.
pub(crate) fn link_entry(entry: &str) -> Option<(String, Option<String>)> {
    let mut fields = entry.split(',').map(str::trim);
    let file = fields.next().filter(|file| !file.is_empty())?.to_owned();
    let face = fields
        .next()
        .filter(|face| !face.is_empty())
        .map(str::to_owned);
    Some((file, face))
}

/// A `REG_SZ` value's text: the UTF-16 up to its first NUL, or all of it
/// when the writer left the NUL off, as the registry allows.
pub(crate) fn wide_string(data: &[u16]) -> String {
    let end = data
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(data.len());
    String::from_utf16_lossy(&data[..end])
}

/// A `REG_MULTI_SZ` value's strings: NUL-separated, ending at the first
/// empty one.
pub(crate) fn multi_string(data: &[u16]) -> Vec<String> {
    data.split(|&unit| unit == 0)
        .take_while(|string| !string.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn value_names_drop_the_format_tag_and_split_collections() {
        assert_eq!(value_names("Cascadia Mono (TrueType)"), ["Cascadia Mono"]);
        assert_eq!(
            value_names("Cambria & Cambria Math (TrueType)"),
            ["Cambria", "Cambria Math"]
        );
        assert_eq!(
            value_names("MS Gothic & MS UI Gothic & MS PGothic (TrueType)"),
            ["MS Gothic", "MS UI Gothic", "MS PGothic"]
        );
        // A stray character after the tag, as one installer leaves.
        assert_eq!(
            value_names("BIZ UDGothic & BIZ UDPGothic (TrueType);"),
            ["BIZ UDGothic", "BIZ UDPGothic"]
        );
        assert_eq!(value_names("Untagged Family"), ["Untagged Family"]);
        assert_eq!(value_names(" (TrueType)"), Vec::<String>::new());
    }

    #[test]
    fn may_hold_admits_every_spelling_of_a_family_and_nothing_else() {
        let names = |value| value_names(value);
        assert!(may_hold(&names("Arial Bold Italic (TrueType)"), "Arial"));
        assert!(may_hold(&names("Arial Black (TrueType)"), "arial"));
        assert!(may_hold(
            &names("Cambria & Cambria Math (TrueType)"),
            "Cambria Math"
        ));
        assert!(may_hold(
            &names("HackNerdFontMono-Bold (TrueType)"),
            "Hack Nerd Font Mono"
        ));
        assert!(may_hold(
            &names("Yu Gothic Medium & Yu Gothic UI Regular (TrueType)"),
            "Yu Gothic UI"
        ));
        assert!(!may_hold(&names("Calibri (TrueType)"), "Arial"));
        assert!(!may_hold(&names("Segoe UI (TrueType)"), "Segoe UI Emoji"));
        // An empty family, or one of punctuation only, admits nothing.
        assert!(!may_hold(&names("Arial (TrueType)"), ""));
        assert!(!may_hold(&names("Arial (TrueType)"), " - "));
    }

    #[test]
    fn font_paths_join_bare_names_and_skip_what_is_not_a_font() {
        let dir = Path::new("fonts-dir");
        assert_eq!(font_path("arial.ttf", dir), Some(dir.join("arial.ttf")));
        assert_eq!(font_path("YuGothM.TTC", dir), Some(dir.join("YuGothM.TTC")));
        assert_eq!(font_path("font.otf ", dir), Some(dir.join("font.otf")));
        assert_eq!(font_path("COURE.FON", dir), None);
        assert_eq!(font_path("noextension", dir), None);
        #[cfg(windows)]
        {
            let full = r"C:\Users\someone\AppData\Local\Microsoft\Windows\Fonts\Hack.ttf";
            assert_eq!(font_path(full, dir), Some(PathBuf::from(full)));
        }
    }

    #[test]
    fn link_entries_give_the_file_and_the_face() {
        assert_eq!(
            link_entry("MSYH.TTC,Microsoft YaHei UI,128,96"),
            Some(("MSYH.TTC".to_owned(), Some("Microsoft YaHei UI".to_owned())))
        );
        assert_eq!(
            link_entry("TAHOMA.TTF,Tahoma"),
            Some(("TAHOMA.TTF".to_owned(), Some("Tahoma".to_owned())))
        );
        assert_eq!(
            link_entry("SEGUISYM.TTF"),
            Some(("SEGUISYM.TTF".to_owned(), None))
        );
        assert_eq!(link_entry(""), None);
        assert_eq!(link_entry(",Tahoma"), None);
    }

    #[test]
    fn registry_strings_decode_with_and_without_their_nuls() {
        let mut data = wide("arial.ttf");
        assert_eq!(wide_string(&data), "arial.ttf");
        data.extend([0, 0x41]);
        assert_eq!(wide_string(&data), "arial.ttf");

        let mut multi = wide("TAHOMA.TTF,Tahoma");
        multi.push(0);
        multi.extend(wide("SEGUISYM.TTF,Segoe UI Symbol"));
        multi.extend([0, 0]);
        multi.extend(wide("after the end"));
        assert_eq!(
            multi_string(&multi),
            ["TAHOMA.TTF,Tahoma", "SEGUISYM.TTF,Segoe UI Symbol"]
        );
        assert_eq!(multi_string(&wide("unterminated")), ["unterminated"]);
        assert_eq!(multi_string(&[]), Vec::<String>::new());
    }
}
