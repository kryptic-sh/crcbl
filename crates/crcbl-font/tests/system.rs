//! Lookups against the fonts of the machine the tests run on: each platform
//! asks for families its CI image certainly has, so a lookup that breaks
//! fails here rather than reading as a font that happens to be missing.

use crcbl_font::{FontQuery, SystemFonts};

/// The file name of `found`'s path, lower-cased: what a test compares,
/// since Windows does not keep one case for its font files.
#[cfg(windows)]
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
#[cfg(windows)]
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
