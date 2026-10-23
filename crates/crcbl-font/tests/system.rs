//! Lookups against the fonts of the machine the tests run on, so a lookup
//! that breaks fails here rather than reading as a font that happens to be
//! missing. Windows and macOS ask for families every install has; fontconfig
//! is held to `fc-match`'s answer, since a Linux desktop's fonts are its own.

use crcbl_font::{FontQuery, SystemFonts};

/// The file name of `found`'s path, lower-cased: what a test compares,
/// since Windows does not keep one case for its font files.
#[cfg(any(windows, target_os = "macos"))]
fn file_name(found: &crcbl_font::FontMatch) -> String {
    found
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a file name")
        .to_lowercase()
}

/// `found`'s face, read as a font of its own, describes itself as `family`:
/// the path and index point at the face that was reported.
#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos"
))]
fn reads_as(found: &crcbl_font::FontMatch, family: &str) {
    use skrifa::MetadataProvider;
    use skrifa::string::StringId;

    let bytes = found.read().expect("the face reads");
    let font = skrifa::FontRef::new(&bytes).expect("the face parses on its own");
    let names: Vec<String> = [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
        .into_iter()
        .flat_map(|id| font.localized_strings(id).map(|name| name.to_string()))
        .collect();
    assert!(
        names.iter().any(|name| name.eq_ignore_ascii_case(family)),
        "{} face {} names {names:?}, not {family}",
        found.path.display(),
        found.index
    );
}

#[test]
fn a_family_nothing_installs_misses() {
    let fonts = SystemFonts::new();
    assert_eq!(fonts.find(&FontQuery::new("No Such Family Zq7")), None);
}

#[cfg(windows)]
mod windows {
    use crcbl_font::{BOLD_WEIGHT, FontStyle, Source};

    use super::*;

    #[test]
    fn arial_is_found_in_each_of_its_faces() {
        let fonts = SystemFonts::new();
        assert_eq!(fonts.source(), Source::Registry);
        let face = |weight, style| {
            fonts
                .find(&FontQuery::new("Arial").weight(weight).style(style))
                .expect("Arial is installed")
        };
        let regular = face(400, FontStyle::Normal);
        assert_eq!(
            (regular.family.as_str(), regular.weight, regular.style),
            ("Arial", 400, FontStyle::Normal)
        );
        assert_eq!(file_name(&regular), "arial.ttf");
        assert_eq!(
            file_name(&face(BOLD_WEIGHT, FontStyle::Normal)),
            "arialbd.ttf"
        );
        assert_eq!(file_name(&face(400, FontStyle::Italic)), "ariali.ttf");
        let bold_italic = face(800, FontStyle::Italic);
        assert_eq!(file_name(&bold_italic), "arialbi.ttf");
        assert_eq!(
            (bold_italic.weight, bold_italic.style),
            (700, FontStyle::Italic)
        );
        reads_as(&regular, "Arial");
    }

    #[test]
    fn segoe_ui_is_found_and_is_the_sans_serif() {
        let fonts = SystemFonts::new();
        let segoe = fonts
            .find(&FontQuery::new("segoe ui"))
            .expect("Segoe UI is installed");
        assert_eq!(segoe.family, "Segoe UI");
        assert_eq!(file_name(&segoe), "segoeui.ttf");
        assert_eq!(fonts.find(&FontQuery::new("sans-serif")), Some(segoe));
        let emoji = fonts.find(&FontQuery::new("emoji")).expect("an emoji face");
        assert_eq!(emoji.family, "Segoe UI Emoji");
    }

    #[test]
    fn a_collection_face_is_found_at_its_index() {
        let fonts = SystemFonts::new();
        let math = fonts
            .find(&FontQuery::new("Cambria Math"))
            .expect("Cambria Math is installed");
        assert_eq!(file_name(&math), "cambria.ttc");
        assert_eq!(math.index, 1);
        reads_as(&math, "Cambria Math");
    }

    #[test]
    fn segoe_ui_falls_back_to_its_system_link_then_the_defaults() {
        let fonts = SystemFonts::new();
        let list = fonts.fallbacks(&FontQuery::new("Segoe UI"));
        let families: Vec<&str> = list.iter().map(|m| m.family.as_str()).collect();
        let at = |family: &str| {
            families
                .iter()
                .position(|known| *known == family)
                .unwrap_or_else(|| panic!("{family} is not in {families:?}"))
        };
        // Tahoma is linked to Segoe UI; Segoe UI Emoji is not, and comes
        // from the default list after every linked face.
        assert!(at("Tahoma") < at("Segoe UI Emoji"));
        at("Segoe UI Symbol");
        assert!(!families.contains(&"Segoe UI"), "{families:?}");
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod fontconfig {
    use std::path::PathBuf;

    use crcbl_font::{BOLD_WEIGHT, FontStyle, Source};

    use super::*;

    /// A `SystemFonts` that reached fontconfig, or a failure saying why not:
    /// the walk it falls back to would answer these lookups too, and say
    /// nothing about fontconfig.
    fn fontconfig() -> SystemFonts {
        let fonts = SystemFonts::new();
        assert_eq!(
            fonts.source(),
            Source::Fontconfig,
            "libfontconfig.so.1 did not load: install fontconfig (CI's image has it)"
        );
        fonts
    }

    /// What `fc-match` answers for `pattern`: the family, file and face
    /// index fontconfig picks. The lookups below are held to it rather than
    /// to one machine's fonts, so they say the same thing on any desktop.
    fn fc_match(pattern: &str) -> (String, PathBuf, u32) {
        let output = crcbl_spawn::command("fc-match")
            .args(["-f", "%{family[0]}\n%{file}\n%{index}", pattern])
            .output()
            .expect("fc-match runs: install fontconfig (CI's image has it)");
        assert!(output.status.success(), "fc-match {pattern}: {output:?}");
        let text = String::from_utf8(output.stdout).expect("UTF-8");
        let mut lines = text.lines();
        let mut next = || lines.next().expect("three lines").to_owned();
        let (family, file, index) = (next(), next(), next());
        (
            family,
            PathBuf::from(file),
            index.parse().expect("an index"),
        )
    }

    /// `found` is the face `fc-match` picks for `pattern`.
    fn is_fc_match(found: &crcbl_font::FontMatch, pattern: &str) {
        let (family, file, index) = fc_match(pattern);
        assert_eq!(
            (found.family.as_str(), &found.path, found.index),
            (family.as_str(), &file, index),
            "{pattern}"
        );
    }

    #[test]
    fn the_default_sans_family_is_found_in_its_regular_and_bold_faces() {
        let fonts = fontconfig();
        let (family, _, _) = fc_match("sans-serif");
        let regular = fonts
            .find(&FontQuery::new(&family))
            .unwrap_or_else(|| panic!("{family} is installed"));
        is_fc_match(&regular, &family);
        assert_eq!(
            (regular.weight, regular.style),
            (400, FontStyle::Normal),
            "{regular:?}"
        );
        reads_as(&regular, &family);
        // Family names match without regard to case, as fontconfig's do.
        let bold = fonts
            .find(&FontQuery::new(family.to_lowercase()).weight(BOLD_WEIGHT))
            .unwrap_or_else(|| panic!("{family} has a bold face"));
        is_fc_match(&bold, &format!("{family}:weight=bold"));
    }

    #[test]
    fn monospace_resolves_to_fontconfigs_choice() {
        let fonts = fontconfig();
        let mono = fonts
            .find(&FontQuery::new("monospace"))
            .expect("a monospace face");
        is_fc_match(&mono, "monospace");
        let bold = fonts
            .find(&FontQuery::new("monospace").weight(BOLD_WEIGHT))
            .expect("a bold monospace face");
        is_fc_match(&bold, "monospace:weight=bold");
    }

    #[test]
    fn emoji_is_an_emoji_family_or_nothing() {
        // fontconfig answers its default text face for `emoji` on a system
        // with no emoji font, as WSL's Ubuntu is; that is a miss.
        let fonts = fontconfig();
        if let Some(emoji) = fonts.find(&FontQuery::new("emoji")) {
            assert!(emoji.family.contains("Emoji"), "{emoji:?}");
        }
    }

    #[test]
    fn fallbacks_are_fontconfigs_sort_without_the_family_itself() {
        let fonts = fontconfig();
        let (family, _, _) = fc_match("sans-serif");
        let list = fonts.fallbacks(&FontQuery::new(&family));
        assert!(!list.is_empty(), "fontconfig sorted no other face");
        assert!(list.iter().all(|m| m.family != family), "{list:?}");
        assert!(list.iter().all(|m| m.path.is_file()), "{list:?}");
        for (at, m) in list.iter().enumerate() {
            let repeated = list[..at].iter().any(|known| known.family == m.family);
            assert!(!repeated, "{} is listed twice: {list:?}", m.family);
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use crcbl_font::{BOLD_WEIGHT, Source};

    use super::*;

    #[test]
    fn helvetica_is_found_in_its_collection() {
        let fonts = SystemFonts::new();
        assert_eq!(fonts.source(), Source::Scan);
        let regular = fonts
            .find(&FontQuery::new("Helvetica"))
            .expect("Helvetica is installed");
        assert_eq!(regular.family, "Helvetica");
        assert_eq!(file_name(&regular), "helvetica.ttc");
        reads_as(&regular, "Helvetica");
        let bold = fonts
            .find(&FontQuery::new("helvetica").weight(BOLD_WEIGHT))
            .expect("Helvetica Bold is installed");
        assert_eq!(bold.path, regular.path);
        assert_ne!(bold.index, regular.index);
        assert!(bold.weight >= 600, "{bold:?}");
        reads_as(&bold, "Helvetica");
        assert_eq!(fonts.find(&FontQuery::new("sans-serif")), Some(regular));
    }

    #[test]
    fn apple_color_emoji_is_the_emoji_and_a_fallback() {
        let fonts = SystemFonts::new();
        let emoji = fonts.find(&FontQuery::new("emoji")).expect("an emoji face");
        assert_eq!(emoji.family, "Apple Color Emoji");
        let list = fonts.fallbacks(&FontQuery::new("Helvetica"));
        let families: Vec<&str> = list.iter().map(|m| m.family.as_str()).collect();
        assert!(families.contains(&"Apple Color Emoji"), "{families:?}");
        assert!(!families.contains(&"Helvetica"), "{families:?}");
    }
}
