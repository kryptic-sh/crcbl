//! CSS colours as SVG writes them: the named colours, hex notation, and the
//! `rgb()` and `rgba()` functions.

/// A straight-alpha sRGB colour, each channel in `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Self = Self::rgb8(0, 0, 0);

    pub const fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    /// The same colour with its alpha multiplied by `opacity`.
    pub fn with_opacity(self, opacity: f32) -> Self {
        Self {
            a: self.a * opacity,
            ..self
        }
    }
}

/// A colour value, or `currentColor`, which the caller resolves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum ColorValue {
    Color(Color),
    CurrentColor,
}

/// Parses a CSS colour. `None` for anything that is not one this module
/// reads, including the `hsl()` family and system colours.
pub(super) fn parse(text: &str) -> Option<ColorValue> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("currentcolor") {
        return Some(ColorValue::CurrentColor);
    }
    if let Some(hex) = text.strip_prefix('#') {
        return hex_color(hex).map(ColorValue::Color);
    }
    let lower = text.to_ascii_lowercase();
    if let Some(arguments) = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))
    {
        return rgb_function(arguments.strip_suffix(')')?).map(ColorValue::Color);
    }
    if lower == "transparent" {
        return Some(ColorValue::Color(Color {
            a: 0.0,
            ..Color::BLACK
        }));
    }
    NAMED
        .binary_search_by(|(name, _)| name.cmp(&lower.as_str()))
        .ok()
        .map(|index| {
            let [r, g, b] = NAMED[index].1;
            ColorValue::Color(Color::rgb8(r, g, b))
        })
}

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`, without the `#`.
fn hex_color(hex: &str) -> Option<Color> {
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |index: usize| u8::from_str_radix(&hex[index..=index], 16).ok();
    let pair = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok();
    let (r, g, b, a) = match hex.len() {
        3 | 4 => {
            let short = |index| digit(index).map(|d| d * 17);
            let a = if hex.len() == 4 { short(3)? } else { 255 };
            (short(0)?, short(1)?, short(2)?, a)
        }
        6 | 8 => {
            let a = if hex.len() == 8 { pair(6)? } else { 255 };
            (pair(0)?, pair(2)?, pair(4)?, a)
        }
        _ => return None,
    };
    Some(Color {
        a: f32::from(a) / 255.0,
        ..Color::rgb8(r, g, b)
    })
}

/// The arguments of `rgb(…)` or `rgba(…)`: three channels as numbers out of
/// 255 or as percentages, comma- or space-separated, and an optional alpha
/// after a comma or a slash.
fn rgb_function(arguments: &str) -> Option<Color> {
    let (channels, alpha) = match arguments.split_once('/') {
        Some((channels, alpha)) => (channels, Some(alpha)),
        None => (arguments, None),
    };
    let mut parts: Vec<&str> = if channels.contains(',') {
        channels.split(',').map(str::trim).collect()
    } else {
        channels.split_whitespace().collect()
    };
    let alpha = match (alpha, parts.len()) {
        (Some(alpha), 3) => Some(alpha.trim()),
        (None, 4) => parts.pop(),
        (None, 3) => None,
        _ => return None,
    };
    let channel = |text: &str| -> Option<f32> {
        let value = if let Some(percent) = text.strip_suffix('%') {
            percent.trim().parse::<f32>().ok()? / 100.0
        } else {
            text.parse::<f32>().ok()? / 255.0
        };
        value.is_finite().then(|| value.clamp(0.0, 1.0))
    };
    let a = match alpha {
        None => 1.0,
        Some(text) => super::style::number_or_percent(text)?.clamp(0.0, 1.0),
    };
    Some(Color {
        r: channel(parts[0])?,
        g: channel(parts[1])?,
        b: channel(parts[2])?,
        a,
    })
}

/// CSS Color Module Level 4's named colours, sorted by name for the binary
/// search.
const NAMED: &[(&str, [u8; 3])] = &[
    ("aliceblue", [240, 248, 255]),
    ("antiquewhite", [250, 235, 215]),
    ("aqua", [0, 255, 255]),
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
    ("crimson", [220, 20, 60]),
    ("cyan", [0, 255, 255]),
    ("darkblue", [0, 0, 139]),
    ("darkcyan", [0, 139, 139]),
    ("darkgoldenrod", [184, 134, 11]),
    ("darkgray", [169, 169, 169]),
    ("darkgreen", [0, 100, 0]),
    ("darkgrey", [169, 169, 169]),
    ("darkkhaki", [189, 183, 107]),
    ("darkmagenta", [139, 0, 139]),
    ("darkolivegreen", [85, 107, 47]),
    ("darkorange", [255, 140, 0]),
    ("darkorchid", [153, 50, 204]),
    ("darkred", [139, 0, 0]),
    ("darksalmon", [233, 150, 122]),
    ("darkseagreen", [143, 188, 143]),
    ("darkslateblue", [72, 61, 139]),
    ("darkslategray", [47, 79, 79]),
    ("darkslategrey", [47, 79, 79]),
    ("darkturquoise", [0, 206, 209]),
    ("darkviolet", [148, 0, 211]),
    ("deeppink", [255, 20, 147]),
    ("deepskyblue", [0, 191, 255]),
    ("dimgray", [105, 105, 105]),
    ("dimgrey", [105, 105, 105]),
    ("dodgerblue", [30, 144, 255]),
    ("firebrick", [178, 34, 34]),
    ("floralwhite", [255, 250, 240]),
    ("forestgreen", [34, 139, 34]),
    ("fuchsia", [255, 0, 255]),
    ("gainsboro", [220, 220, 220]),
    ("ghostwhite", [248, 248, 255]),
    ("gold", [255, 215, 0]),
    ("goldenrod", [218, 165, 32]),
    ("gray", [128, 128, 128]),
    ("green", [0, 128, 0]),
    ("greenyellow", [173, 255, 47]),
    ("grey", [128, 128, 128]),
    ("honeydew", [240, 255, 240]),
    ("hotpink", [255, 105, 180]),
    ("indianred", [205, 92, 92]),
    ("indigo", [75, 0, 130]),
    ("ivory", [255, 255, 240]),
    ("khaki", [240, 230, 140]),
    ("lavender", [230, 230, 250]),
    ("lavenderblush", [255, 240, 245]),
    ("lawngreen", [124, 252, 0]),
    ("lemonchiffon", [255, 250, 205]),
    ("lightblue", [173, 216, 230]),
    ("lightcoral", [240, 128, 128]),
    ("lightcyan", [224, 255, 255]),
    ("lightgoldenrodyellow", [250, 250, 210]),
    ("lightgray", [211, 211, 211]),
    ("lightgreen", [144, 238, 144]),
    ("lightgrey", [211, 211, 211]),
    ("lightpink", [255, 182, 193]),
    ("lightsalmon", [255, 160, 122]),
    ("lightseagreen", [32, 178, 170]),
    ("lightskyblue", [135, 206, 250]),
    ("lightslategray", [119, 136, 153]),
    ("lightslategrey", [119, 136, 153]),
    ("lightsteelblue", [176, 196, 222]),
    ("lightyellow", [255, 255, 224]),
    ("lime", [0, 255, 0]),
    ("limegreen", [50, 205, 50]),
    ("linen", [250, 240, 230]),
    ("magenta", [255, 0, 255]),
    ("maroon", [128, 0, 0]),
    ("mediumaquamarine", [102, 205, 170]),
    ("mediumblue", [0, 0, 205]),
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
    ("oldlace", [253, 245, 230]),
    ("olive", [128, 128, 0]),
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
    ("purple", [128, 0, 128]),
    ("rebeccapurple", [102, 51, 153]),
    ("red", [255, 0, 0]),
    ("rosybrown", [188, 143, 143]),
    ("royalblue", [65, 105, 225]),
    ("saddlebrown", [139, 69, 19]),
    ("salmon", [250, 128, 114]),
    ("sandybrown", [244, 164, 96]),
    ("seagreen", [46, 139, 87]),
    ("seashell", [255, 245, 238]),
    ("sienna", [160, 82, 45]),
    ("silver", [192, 192, 192]),
    ("skyblue", [135, 206, 235]),
    ("slateblue", [106, 90, 205]),
    ("slategray", [112, 128, 144]),
    ("slategrey", [112, 128, 144]),
    ("snow", [255, 250, 250]),
    ("springgreen", [0, 255, 127]),
    ("steelblue", [70, 130, 180]),
    ("tan", [210, 180, 140]),
    ("teal", [0, 128, 128]),
    ("thistle", [216, 191, 216]),
    ("tomato", [255, 99, 71]),
    ("turquoise", [64, 224, 208]),
    ("violet", [238, 130, 238]),
    ("wheat", [245, 222, 179]),
    ("white", [255, 255, 255]),
    ("whitesmoke", [245, 245, 245]),
    ("yellow", [255, 255, 0]),
    ("yellowgreen", [154, 205, 50]),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn color(text: &str) -> Color {
        match parse(text) {
            Some(ColorValue::Color(color)) => color,
            other => panic!("{text}: {other:?}"),
        }
    }

    fn bytes(color: Color) -> [u8; 4] {
        [color.r, color.g, color.b, color.a].map(|c| (c * 255.0).round() as u8)
    }

    /// **Every notation gives the colour it names**: hex in each length,
    /// both functions in each syntax, and names in any case.
    #[test]
    fn every_notation_reads_its_colour() {
        for (text, want) in [
            ("#f80", [255, 136, 0, 255]),
            ("#f808", [255, 136, 0, 136]),
            ("#FF8000", [255, 128, 0, 255]),
            ("#ff800080", [255, 128, 0, 128]),
            ("rgb(255, 128, 0)", [255, 128, 0, 255]),
            ("rgb(100%,50%,0%)", [255, 128, 0, 255]),
            ("RGBA(255,128,0,0.5)", [255, 128, 0, 128]),
            ("rgb(255 128 0 / 50%)", [255, 128, 0, 128]),
            ("rgb(300, -5, 0)", [255, 0, 0, 255]),
            ("CornflowerBlue", [100, 149, 237, 255]),
            ("rebeccapurple", [102, 51, 153, 255]),
            ("transparent", [0, 0, 0, 0]),
        ] {
            assert_eq!(bytes(color(text)), want, "{text}");
        }
        assert_eq!(parse("currentColor"), Some(ColorValue::CurrentColor));
        assert_eq!(parse("currentcolor"), Some(ColorValue::CurrentColor));
    }

    /// **What is not a colour this reads is `None`**, never a guess.
    #[test]
    fn what_is_not_a_colour_is_refused() {
        for text in [
            "#12",
            "#12345",
            "#ggg",
            "rgb(1,2)",
            "rgb(1,2,3,4,5)",
            "hsl(0, 100%, 50%)",
            "ButtonFace",
            "notacolour",
            "",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }

    /// **The name table is sorted**, which its binary search depends on.
    #[test]
    fn the_name_table_is_sorted() {
        assert!(NAMED.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }
}
