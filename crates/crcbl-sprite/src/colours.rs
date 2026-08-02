//! The colour names an XPM may legitimately use.
//!
//! Generated from `magick -list color`, keeping every entry whose compliance
//! column names **XPM** — which is precisely "a name a real `.xpm` might
//! contain" rather than a set chosen by taste. 234 of them, and the same list
//! X11's `rgb.txt` carries in its canonical spellings.
//!
//! Names were refused when this format was first written, on the argument that
//! "a misspelled name is a colour that silently is not the one you meant". That
//! argument was simply wrong: a name not in this table is refused with an
//! error, loudly, which is the opposite of silent. The real cost is the three
//! kilobytes below, and it buys a palette block pasted out of a GIMP or
//! ImageMagick export working as-is.
//!
//! Matching is **case-insensitive and ignores spaces**, as X11's own lookup is:
//! `AliceBlue`, `aliceblue` and `alice blue` are one colour.

/// `(name, rgb)`, sorted by name so the lookup can bisect.
///
/// Sorted-ness is asserted by a test rather than trusted, because a hand-added
/// entry in the wrong place would make the binary search miss it — and miss it
/// *silently*, for that one colour.
static NAMES: &[(&str, [u8; 3])] = &[
    ("aliceblue", [240, 248, 255]),
    ("antiquewhite", [250, 235, 215]),
    ("aquamarine", [127, 255, 212]),
    ("azure", [240, 255, 255]),
    ("beige", [245, 245, 220]),
    ("bisque", [255, 228, 196]),
    ("black", [0, 0, 0]),
    ("blanchedalmond", [255, 235, 205]),
    ("blue", [0, 0, 255]),
    ("blueviolet", [138, 43, 226]),
    ("brown", [165, 42, 42]),
    ("burlywood", [222, 184, 135]),
    ("cadetblue", [95, 158, 160]),
    ("chartreuse", [127, 255, 0]),
    ("chocolate", [210, 105, 30]),
    ("coral", [255, 127, 80]),
    ("cornflowerblue", [100, 149, 237]),
    ("cornsilk", [255, 248, 220]),
    ("cyan", [0, 255, 255]),
    ("darkgoldenrod", [184, 134, 11]),
    ("darkgreen", [0, 100, 0]),
    ("darkkhaki", [189, 183, 107]),
    ("darkolivegreen", [85, 107, 47]),
    ("darkorange", [255, 140, 0]),
    ("darkorchid", [153, 50, 204]),
    ("darksalmon", [233, 150, 122]),
    ("darkseagreen", [143, 188, 143]),
    ("darkslateblue", [72, 61, 139]),
    ("darkslategray", [47, 79, 79]),
    ("darkturquoise", [0, 206, 209]),
    ("darkviolet", [148, 0, 211]),
    ("deeppink", [255, 20, 147]),
    ("deepskyblue", [0, 191, 255]),
    ("dimgray", [105, 105, 105]),
    ("dodgerblue", [30, 144, 255]),
    ("firebrick", [178, 34, 34]),
    ("floralwhite", [255, 250, 240]),
    ("forestgreen", [34, 139, 34]),
    ("gainsboro", [220, 220, 220]),
    ("ghostwhite", [248, 248, 255]),
    ("gold", [255, 215, 0]),
    ("goldenrod", [218, 165, 32]),
    ("gray", [190, 190, 190]),
    ("gray0", [0, 0, 0]),
    ("gray1", [3, 3, 3]),
    ("gray10", [26, 26, 26]),
    ("gray100", [255, 255, 255]),
    ("gray11", [28, 28, 28]),
    ("gray12", [31, 31, 31]),
    ("gray13", [33, 33, 33]),
    ("gray14", [36, 36, 36]),
    ("gray15", [38, 38, 38]),
    ("gray16", [41, 41, 41]),
    ("gray17", [43, 43, 43]),
    ("gray18", [46, 46, 46]),
    ("gray19", [48, 48, 48]),
    ("gray2", [5, 5, 5]),
    ("gray20", [51, 51, 51]),
    ("gray21", [54, 54, 54]),
    ("gray22", [56, 56, 56]),
    ("gray23", [59, 59, 59]),
    ("gray24", [61, 61, 61]),
    ("gray25", [64, 64, 64]),
    ("gray26", [66, 66, 66]),
    ("gray27", [69, 69, 69]),
    ("gray28", [71, 71, 71]),
    ("gray29", [74, 74, 74]),
    ("gray3", [8, 8, 8]),
    ("gray30", [77, 77, 77]),
    ("gray31", [79, 79, 79]),
    ("gray32", [82, 82, 82]),
    ("gray33", [84, 84, 84]),
    ("gray34", [87, 87, 87]),
    ("gray35", [89, 89, 89]),
    ("gray36", [92, 92, 92]),
    ("gray37", [94, 94, 94]),
    ("gray38", [97, 97, 97]),
    ("gray39", [99, 99, 99]),
    ("gray4", [10, 10, 10]),
    ("gray40", [102, 102, 102]),
    ("gray41", [105, 105, 105]),
    ("gray42", [107, 107, 107]),
    ("gray43", [110, 110, 110]),
    ("gray44", [112, 112, 112]),
    ("gray45", [115, 115, 115]),
    ("gray46", [117, 117, 117]),
    ("gray47", [120, 120, 120]),
    ("gray48", [122, 122, 122]),
    ("gray49", [125, 125, 125]),
    ("gray5", [13, 13, 13]),
    ("gray50", [127, 127, 127]),
    ("gray51", [130, 130, 130]),
    ("gray52", [133, 133, 133]),
    ("gray53", [135, 135, 135]),
    ("gray54", [138, 138, 138]),
    ("gray55", [140, 140, 140]),
    ("gray56", [143, 143, 143]),
    ("gray57", [145, 145, 145]),
    ("gray58", [148, 148, 148]),
    ("gray59", [150, 150, 150]),
    ("gray6", [15, 15, 15]),
    ("gray60", [153, 153, 153]),
    ("gray61", [156, 156, 156]),
    ("gray62", [158, 158, 158]),
    ("gray63", [161, 161, 161]),
    ("gray64", [163, 163, 163]),
    ("gray65", [166, 166, 166]),
    ("gray66", [168, 168, 168]),
    ("gray67", [171, 171, 171]),
    ("gray68", [173, 173, 173]),
    ("gray69", [176, 176, 176]),
    ("gray7", [18, 18, 18]),
    ("gray70", [179, 179, 179]),
    ("gray71", [181, 181, 181]),
    ("gray72", [184, 184, 184]),
    ("gray73", [186, 186, 186]),
    ("gray74", [189, 189, 189]),
    ("gray75", [191, 191, 191]),
    ("gray76", [194, 194, 194]),
    ("gray77", [196, 196, 196]),
    ("gray78", [199, 199, 199]),
    ("gray79", [201, 201, 201]),
    ("gray8", [20, 20, 20]),
    ("gray80", [204, 204, 204]),
    ("gray81", [207, 207, 207]),
    ("gray82", [209, 209, 209]),
    ("gray83", [212, 212, 212]),
    ("gray84", [214, 214, 214]),
    ("gray85", [217, 217, 217]),
    ("gray86", [219, 219, 219]),
    ("gray87", [222, 222, 222]),
    ("gray88", [224, 224, 224]),
    ("gray89", [227, 227, 227]),
    ("gray9", [23, 23, 23]),
    ("gray90", [229, 229, 229]),
    ("gray91", [232, 232, 232]),
    ("gray92", [235, 235, 235]),
    ("gray93", [237, 237, 237]),
    ("gray94", [240, 240, 240]),
    ("gray95", [242, 242, 242]),
    ("gray96", [245, 245, 245]),
    ("gray97", [247, 247, 247]),
    ("gray98", [250, 250, 250]),
    ("gray99", [252, 252, 252]),
    ("green", [0, 255, 0]),
    ("greenyellow", [173, 255, 47]),
    ("honeydew", [240, 255, 240]),
    ("hotpink", [255, 105, 180]),
    ("indianred", [205, 92, 92]),
    ("ivory", [255, 255, 240]),
    ("khaki", [240, 230, 140]),
    ("lavender", [230, 230, 250]),
    ("lavenderblush", [255, 240, 245]),
    ("lawngreen", [124, 252, 0]),
    ("lemonchiffon", [255, 250, 205]),
    ("lightblue", [173, 216, 230]),
    ("lightcoral", [240, 128, 128]),
    ("lightcyan", [224, 255, 255]),
    ("lightgoldenrod", [238, 221, 130]),
    ("lightgoldenrodyellow", [250, 250, 210]),
    ("lightgray", [211, 211, 211]),
    ("lightpink", [255, 182, 193]),
    ("lightsalmon", [255, 160, 122]),
    ("lightseagreen", [32, 178, 170]),
    ("lightskyblue", [135, 206, 250]),
    ("lightslateblue", [132, 112, 255]),
    ("lightslategray", [119, 136, 153]),
    ("lightsteelblue", [176, 196, 222]),
    ("lightyellow", [255, 255, 224]),
    ("limegreen", [50, 205, 50]),
    ("linen", [250, 240, 230]),
    ("magenta", [255, 0, 255]),
    ("maroon", [176, 48, 96]),
    ("mediumaquamarine", [102, 205, 170]),
    ("mediumblue", [0, 0, 205]),
    ("mediumforestgreen", [50, 129, 75]),
    ("mediumgoldenrod", [209, 193, 102]),
    ("mediumorchid", [186, 85, 211]),
    ("mediumpurple", [147, 112, 219]),
    ("mediumseagreen", [60, 179, 113]),
    ("mediumslateblue", [123, 104, 238]),
    ("mediumspringgreen", [0, 250, 154]),
    ("mediumturquoise", [72, 209, 204]),
    ("mediumvioletred", [199, 21, 133]),
    ("midnightblue", [25, 25, 112]),
    ("mintcream", [245, 255, 250]),
    ("mistyrose", [255, 228, 225]),
    ("moccasin", [255, 228, 181]),
    ("navajowhite", [255, 222, 173]),
    ("navy", [0, 0, 128]),
    ("navyblue", [0, 0, 128]),
    ("none", [0, 0, 0]),
    ("oldlace", [253, 245, 230]),
    ("olivedrab", [107, 142, 35]),
    ("orange", [255, 165, 0]),
    ("orangered", [255, 69, 0]),
    ("orchid", [218, 112, 214]),
    ("palegoldenrod", [238, 232, 170]),
    ("palegreen", [152, 251, 152]),
    ("paleturquoise", [175, 238, 238]),
    ("palevioletred", [219, 112, 147]),
    ("papayawhip", [255, 239, 213]),
    ("peachpuff", [255, 218, 185]),
    ("peru", [205, 133, 63]),
    ("pink", [255, 192, 203]),
    ("plum", [221, 160, 221]),
    ("powderblue", [176, 224, 230]),
    ("purple", [160, 32, 240]),
    ("red", [255, 0, 0]),
    ("rosybrown", [188, 143, 143]),
    ("royalblue", [65, 105, 225]),
    ("saddlebrown", [139, 69, 19]),
    ("salmon", [250, 128, 114]),
    ("sandybrown", [244, 164, 96]),
    ("seagreen", [46, 139, 87]),
    ("seashell", [255, 245, 238]),
    ("sienna", [160, 82, 45]),
    ("skyblue", [135, 206, 235]),
    ("slateblue", [106, 90, 205]),
    ("slategray", [112, 128, 144]),
    ("snow", [255, 250, 250]),
    ("springgreen", [0, 255, 127]),
    ("steelblue", [70, 130, 180]),
    ("tan", [210, 180, 140]),
    ("thistle", [216, 191, 216]),
    ("tomato", [255, 99, 71]),
    ("turquoise", [64, 224, 208]),
    ("violet", [238, 130, 238]),
    ("violetred", [208, 32, 144]),
    ("wheat", [245, 222, 179]),
    ("white", [255, 255, 255]),
    ("whitesmoke", [245, 245, 245]),
    ("yellow", [255, 255, 0]),
    ("yellowgreen", [154, 205, 50]),
];

/// The colour `name` denotes, if it is one.
///
/// Case and internal spaces are ignored. Returns opaque RGBA: XPM has no alpha
/// beyond `None`, and neither do these.
#[must_use]
pub fn lookup(name: &str) -> Option<[u8; 4]> {
    // Normalised into a stack buffer rather than a `String`: this runs once per
    // palette entry at build time, and the longest name here is 20 characters.
    let mut needle = [0u8; 32];
    let mut len = 0;
    for byte in name.bytes() {
        if byte == b' ' || byte == b'_' {
            continue;
        }
        if len == needle.len() {
            return None;
        }
        needle[len] = byte.to_ascii_lowercase();
        len += 1;
    }
    let needle = core::str::from_utf8(&needle[..len]).ok()?;
    NAMES
        .binary_search_by_key(&needle, |(name, _)| *name)
        .ok()
        .map(|index| {
            let [r, g, b] = NAMES[index].1;
            [r, g, b, 255]
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table is searched by bisection, so an entry out of order is one the
    /// lookup silently cannot find.
    #[test]
    fn the_table_is_sorted_and_has_no_duplicates() {
        for pair in NAMES.windows(2) {
            assert!(
                pair[0].0 < pair[1].0,
                "`{}` is not before `{}`",
                pair[0].0,
                pair[1].0
            );
        }
    }

    /// Spot-checked against `magick -list color`, which is where the table came
    /// from. Values, not just presence: a table that resolved every name to
    /// black would pass any test that only asked whether the name was known.
    #[test]
    fn the_names_resolve_to_the_colours_x11_gives_them() {
        assert_eq!(lookup("white"), Some([255, 255, 255, 255]));
        assert_eq!(lookup("black"), Some([0, 0, 0, 255]));
        assert_eq!(lookup("red"), Some([255, 0, 0, 255]));
        assert_eq!(lookup("cornflowerblue"), Some([100, 149, 237, 255]));
        assert_eq!(lookup("gold"), Some([255, 215, 0, 255]));
        assert_eq!(lookup("navy"), Some([0, 0, 128, 255]));
    }

    /// X11 looks names up without regard to case or spacing, and a palette
    /// pasted from one file writes them differently from another.
    #[test]
    fn lookup_ignores_case_and_spacing() {
        let expected = Some([100, 149, 237, 255]);
        assert_eq!(lookup("CornflowerBlue"), expected);
        assert_eq!(lookup("cornflower blue"), expected);
        assert_eq!(lookup("CORNFLOWER_BLUE"), expected);
    }

    /// A name that is not a colour is refused rather than guessed at.
    #[test]
    fn an_unknown_name_is_none() {
        assert_eq!(lookup("cornflowerblu"), None);
        assert_eq!(lookup(""), None);
        assert_eq!(
            lookup(&"x".repeat(64)),
            None,
            "and cannot overrun the buffer"
        );
    }

    /// Every name in the table is findable through the public lookup — the
    /// property the sortedness test exists to protect, checked directly.
    #[test]
    fn every_name_in_the_table_can_be_found() {
        for (name, [r, g, b]) in NAMES {
            assert_eq!(lookup(name), Some([*r, *g, *b, 255]), "{name}");
        }
    }
}
