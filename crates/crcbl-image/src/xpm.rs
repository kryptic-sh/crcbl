//! XPM, written here: X PixMaps, the C source many X11 programs and Debian's
//! menu entries keep their icons in, decoded to RGBA8.
//!
//! # What it reads
//!
//! XPM 3, the C array form —
//! `/* XPM */ static char *name[] = { "values", "colour"..., "row"... };` —
//! and XPM 2, the same strings one a line after `! XPM2`. The values string
//! is `width height colours chars-per-pixel`, then optionally a hotspot and
//! `XPMEXT`, which are ignored. In XPM 3 the strings may sit anywhere among
//! the C and `/* */` comments, several to a line or one; in XPM 2 a line
//! starting `!` is a comment. Whatever follows the last pixel row — the
//! extensions `XPMEXT` announces, the closing brace — is never read. A pixel
//! may take any number of characters up to [`MAX_CHARS_PER_PIXEL`], spaces
//! included, and a file may define up to [`MAX_COLOURS`]. XPM 1, the
//! `#define` form, is [`ImageError::Unsupported`].
//!
//! # Colours
//!
//! A colour gives values for several kinds of display, each after its key:
//! `c` for colour, `g` for grey, `g4` for four-level grey and `m` for mono.
//! The `c` value is used, failing it the `g`, `g4` and `m` ones in that
//! order; a symbolic name (`s`), which lets a program substitute its own
//! colour, is ignored. A value is `None` (transparent), `#` with three, six,
//! nine or twelve hex digits, or an X11 colour name.
//!
//! Hex digits are scaled to eight bits and rounded, so `#fff` is white and
//! `#00ff00ff00ff` is (1, 1, 1), as netpbm's `xpmtoppm` reads them. Xlib's
//! `XParseColor` instead takes short digits as a value's high bits, which
//! makes `#fff` (240, 240, 240): a difference only where a channel has other
//! than two digits.
//!
//! Colour names are X11's, from [`X11_COLOURS`], and match as X's colour
//! database matches them: whatever their case, and with spaces ignored, so
//! `Light Blue` is `LightBlue`.
//!
//! # Hostile files
//!
//! The image's size is checked against the caller's [`Limits`] as soon as
//! the values string is read, before the colours or pixels are, and the
//! caps above bound the colour table. A pixel whose characters no colour
//! defines, and a colour this module cannot read, are
//! [`ImageError::Malformed`]: nothing is guessed.

use std::collections::HashMap;

use crate::{ImageError, Limits, Rgba8Image};

/// The most characters one pixel may take. Real files use one to three; the
/// cap keeps each key small.
pub const MAX_CHARS_PER_PIXEL: usize = 31;

/// The most colours a file may define: more than any icon has, and a bound
/// on the colour table a file of that many lines builds.
pub const MAX_COLOURS: usize = 1 << 20;

fn malformed(why: impl Into<String>) -> ImageError {
    ImageError::Malformed(format!("XPM: {}", why.into()))
}

fn unsupported(why: impl Into<String>) -> ImageError {
    ImageError::Unsupported(format!("XPM: {}", why.into()))
}

/// Decodes an `.xpm` file to straight RGBA8.
///
/// # Errors
///
/// [`ImageError::Empty`] or [`ImageError::TooLarge`] if its size breaks
/// `limits`, [`ImageError::Unsupported`] for XPM 1 or a file past
/// [`MAX_CHARS_PER_PIXEL`] or [`MAX_COLOURS`], and [`ImageError::Malformed`]
/// otherwise.
pub fn decode(bytes: &[u8], limits: &Limits) -> Result<Rgba8Image, ImageError> {
    let mut strings = Strings::new(bytes)?;
    let values = strings
        .next()?
        .ok_or_else(|| malformed("there is no values string"))?;
    let header = Header::read(values)?;
    let rgba_bytes = limits.rgba_bytes(header.width, header.height)?;

    let mut colours: HashMap<&[u8], [u8; 4]> = HashMap::new();
    for _ in 0..header.colours {
        let (key, spec) = strings
            .next()?
            .and_then(|line| line.split_at_checked(header.chars_per_pixel))
            .ok_or_else(|| malformed("a colour is missing or shorter than its key"))?;
        let colour = colour_line(spec)?;
        // The first definition of a key stands.
        colours.entry(key).or_insert(colour);
    }

    let row_bytes = (header.width as usize)
        .checked_mul(header.chars_per_pixel)
        .ok_or_else(|| malformed("a pixel row is too long"))?;
    let mut pixels = Vec::with_capacity(rgba_bytes);
    for _ in 0..header.height {
        let row = strings
            .next()?
            .and_then(|row| row.get(..row_bytes))
            .ok_or_else(|| malformed("the pixel rows are cut short"))?;
        for key in row.chunks_exact(header.chars_per_pixel) {
            let colour = colours.get(key).ok_or_else(|| {
                malformed(format!(
                    "the pixel {:?} has no colour",
                    String::from_utf8_lossy(key)
                ))
            })?;
            pixels.extend_from_slice(colour);
        }
    }
    Ok(Rgba8Image::from_sized(header.width, header.height, pixels))
}

/// The strings an image is written in, in order.
enum Strings<'a> {
    /// XPM 3: the contents of C string literals, with C and comments around
    /// them.
    C(&'a [u8]),
    /// XPM 2: lines, a `!` starting a comment.
    Lines(&'a [u8]),
}

impl<'a> Strings<'a> {
    /// The strings of `bytes`, by the header it starts with.
    fn new(bytes: &'a [u8]) -> Result<Self, ImageError> {
        let text = bytes.trim_ascii_start();
        let xpm3 = text
            .strip_prefix(b"/*")
            .and_then(|rest| rest.trim_ascii_start().strip_prefix(b"XPM"))
            .and_then(|rest| rest.trim_ascii_start().strip_prefix(b"*/"));
        if let Some(rest) = xpm3 {
            return Ok(Self::C(rest));
        }
        if let Some(rest) = text.strip_prefix(b"!")
            && rest.trim_ascii_start().starts_with(b"XPM2")
        {
            // The header line is a comment line like any other.
            return Ok(Self::Lines(text));
        }
        if text.starts_with(b"#define") {
            return Err(unsupported("XPM 1, the #define form"));
        }
        Err(malformed("no XPM header"))
    }

    /// The next string, or `None` at the end.
    fn next(&mut self) -> Result<Option<&'a [u8]>, ImageError> {
        match self {
            Self::C(rest) => loop {
                let Some(at) = rest.iter().position(|&byte| matches!(byte, b'"' | b'/')) else {
                    *rest = &[];
                    return Ok(None);
                };
                let after = &rest[at + 1..];
                if rest[at] == b'/' {
                    // A comment that never ends runs to the end of the file,
                    // which leaves the strings it hid cut short.
                    *rest = match after.strip_prefix(b"*") {
                        Some(comment) => comment
                            .windows(2)
                            .position(|pair| pair == b"*/")
                            .map_or(&[][..], |end| &comment[end + 2..]),
                        None => after,
                    };
                    continue;
                }
                let end = after
                    .iter()
                    .position(|&byte| byte == b'"')
                    .ok_or_else(|| malformed("a string never ends"))?;
                *rest = &after[end + 1..];
                return Ok(Some(&after[..end]));
            },
            Self::Lines(rest) => loop {
                if rest.is_empty() {
                    return Ok(None);
                }
                let end = rest
                    .iter()
                    .position(|&byte| byte == b'\n')
                    .unwrap_or(rest.len());
                // A CR before the LF is whitespace to the colours' reader,
                // and past the last pixel to the rows'.
                let line = &rest[..end];
                *rest = rest.get(end + 1..).unwrap_or(&[]);
                if !line.starts_with(b"!") {
                    return Ok(Some(line));
                }
            },
        }
    }
}

/// What the values string says.
#[derive(Debug, PartialEq, Eq)]
struct Header {
    width: u32,
    height: u32,
    colours: usize,
    chars_per_pixel: usize,
}

impl Header {
    /// Reads the values string; what follows its first four numbers (a
    /// hotspot, `XPMEXT`) changes nothing drawn.
    fn read(values: &[u8]) -> Result<Self, ImageError> {
        let mut words = values
            .split(u8::is_ascii_whitespace)
            .filter(|word| !word.is_empty());
        let mut number = |what: &str| {
            words
                .next()
                .and_then(|word| core::str::from_utf8(word).ok()?.parse::<u32>().ok())
                .ok_or_else(|| malformed(format!("the values string has no {what}")))
        };
        let width = number("width")?;
        let height = number("height")?;
        let colours = number("colour count")? as usize;
        let chars_per_pixel = number("characters per pixel")? as usize;
        if chars_per_pixel == 0 {
            return Err(malformed("an XPM of no characters a pixel"));
        }
        if chars_per_pixel > MAX_CHARS_PER_PIXEL {
            return Err(unsupported(format!(
                "{chars_per_pixel} characters a pixel, past {MAX_CHARS_PER_PIXEL}"
            )));
        }
        if colours > MAX_COLOURS {
            return Err(unsupported(format!(
                "{colours} colours, past {MAX_COLOURS}"
            )));
        }
        Ok(Self {
            width,
            height,
            colours,
            chars_per_pixel,
        })
    }
}

/// The colour a colour string defines, from what follows its key: the first
/// of the `c`, `g`, `g4` and `m` values it gives.
fn colour_line(spec: &[u8]) -> Result<[u8; 4], ImageError> {
    const KEYS: [&[u8]; 4] = [b"c", b"g", b"g4", b"m"];
    // Each key's value, its words in order; `s` and its words are dropped.
    let mut values: [Vec<&[u8]>; 4] = Default::default();
    let mut current: Option<Option<usize>> = None;
    for word in spec
        .split(u8::is_ascii_whitespace)
        .filter(|word| !word.is_empty())
    {
        if word == b"s" {
            current = Some(None);
        } else if let Some(key) = KEYS.iter().position(|key| *key == word) {
            current = Some(Some(key));
        } else {
            match current {
                Some(Some(key)) => values[key].push(word),
                Some(None) => {}
                None => {
                    return Err(malformed(format!(
                        "the colour {:?} has no key",
                        String::from_utf8_lossy(spec)
                    )));
                }
            }
        }
    }
    let words = values
        .iter()
        .find(|words| !words.is_empty())
        .ok_or_else(|| {
            malformed(format!(
                "the colour {:?} has no value",
                String::from_utf8_lossy(spec)
            ))
        })?;
    // Names match with their spaces removed, and nothing else has any.
    let value = words.concat();
    colour(&value).ok_or_else(|| {
        malformed(format!(
            "the colour {:?} is not one this decoder knows",
            String::from_utf8_lossy(&value)
        ))
    })
}

/// A colour value as RGBA: `None`, hex digits or an X11 name, any spaces in
/// it removed.
fn colour(value: &[u8]) -> Option<[u8; 4]> {
    if value.eq_ignore_ascii_case(b"none") {
        return Some([0; 4]);
    }
    if let Some(digits) = value.strip_prefix(b"#") {
        if digits.is_empty() || digits.len() % 3 != 0 || digits.len() > 12 {
            return None;
        }
        let width = digits.len() / 3;
        let max = (1u32 << (4 * width)) - 1;
        let mut rgba = [0, 0, 0, 255];
        for (channel, digits) in rgba.iter_mut().zip(digits.chunks_exact(width)) {
            let digits = core::str::from_utf8(digits).ok()?;
            // `from_str_radix` takes a sign, which a colour has none of.
            if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return None;
            }
            let value = u32::from_str_radix(digits, 16).ok()?;
            *channel = ((value * 255 + max / 2) / max) as u8;
        }
        return Some(rgba);
    }
    let name = value.to_ascii_lowercase();
    let index = X11_COLOURS
        .binary_search_by(|(known, _)| known.as_bytes().cmp(&name))
        .ok()?;
    let [r, g, b] = X11_COLOURS[index].1;
    Some([r, g, b, 255])
}

/// X11's colour names, lower-cased with their spaces removed and sorted by
/// byte, each with its RGB.
///
/// Generated from X.Org's `rgb.txt` (its `app/rgb` module, under the MIT
/// licence), the copy Ubuntu 26.04's `x11-common` installs at
/// `/etc/X11/rgb.txt` (`$Xorg: rgb.txt,v 1.3 2000/08/17`). That file lists
/// most colours twice, as `alice blue` and `AliceBlue`; both come to one
/// name here, and every such pair agrees.
pub const X11_COLOURS: &[(&str, [u8; 3])] = &[
    ("aliceblue", [240, 248, 255]),
    ("antiquewhite", [250, 235, 215]),
    ("antiquewhite1", [255, 239, 219]),
    ("antiquewhite2", [238, 223, 204]),
    ("antiquewhite3", [205, 192, 176]),
    ("antiquewhite4", [139, 131, 120]),
    ("aquamarine", [127, 255, 212]),
    ("aquamarine1", [127, 255, 212]),
    ("aquamarine2", [118, 238, 198]),
    ("aquamarine3", [102, 205, 170]),
    ("aquamarine4", [69, 139, 116]),
    ("azure", [240, 255, 255]),
    ("azure1", [240, 255, 255]),
    ("azure2", [224, 238, 238]),
    ("azure3", [193, 205, 205]),
    ("azure4", [131, 139, 139]),
    ("beige", [245, 245, 220]),
    ("bisque", [255, 228, 196]),
    ("bisque1", [255, 228, 196]),
    ("bisque2", [238, 213, 183]),
    ("bisque3", [205, 183, 158]),
    ("bisque4", [139, 125, 107]),
    ("black", [0, 0, 0]),
    ("blanchedalmond", [255, 235, 205]),
    ("blue", [0, 0, 255]),
    ("blue1", [0, 0, 255]),
    ("blue2", [0, 0, 238]),
    ("blue3", [0, 0, 205]),
    ("blue4", [0, 0, 139]),
    ("blueviolet", [138, 43, 226]),
    ("brown", [165, 42, 42]),
    ("brown1", [255, 64, 64]),
    ("brown2", [238, 59, 59]),
    ("brown3", [205, 51, 51]),
    ("brown4", [139, 35, 35]),
    ("burlywood", [222, 184, 135]),
    ("burlywood1", [255, 211, 155]),
    ("burlywood2", [238, 197, 145]),
    ("burlywood3", [205, 170, 125]),
    ("burlywood4", [139, 115, 85]),
    ("cadetblue", [95, 158, 160]),
    ("cadetblue1", [152, 245, 255]),
    ("cadetblue2", [142, 229, 238]),
    ("cadetblue3", [122, 197, 205]),
    ("cadetblue4", [83, 134, 139]),
    ("chartreuse", [127, 255, 0]),
    ("chartreuse1", [127, 255, 0]),
    ("chartreuse2", [118, 238, 0]),
    ("chartreuse3", [102, 205, 0]),
    ("chartreuse4", [69, 139, 0]),
    ("chocolate", [210, 105, 30]),
    ("chocolate1", [255, 127, 36]),
    ("chocolate2", [238, 118, 33]),
    ("chocolate3", [205, 102, 29]),
    ("chocolate4", [139, 69, 19]),
    ("coral", [255, 127, 80]),
    ("coral1", [255, 114, 86]),
    ("coral2", [238, 106, 80]),
    ("coral3", [205, 91, 69]),
    ("coral4", [139, 62, 47]),
    ("cornflowerblue", [100, 149, 237]),
    ("cornsilk", [255, 248, 220]),
    ("cornsilk1", [255, 248, 220]),
    ("cornsilk2", [238, 232, 205]),
    ("cornsilk3", [205, 200, 177]),
    ("cornsilk4", [139, 136, 120]),
    ("cyan", [0, 255, 255]),
    ("cyan1", [0, 255, 255]),
    ("cyan2", [0, 238, 238]),
    ("cyan3", [0, 205, 205]),
    ("cyan4", [0, 139, 139]),
    ("darkblue", [0, 0, 139]),
    ("darkcyan", [0, 139, 139]),
    ("darkgoldenrod", [184, 134, 11]),
    ("darkgoldenrod1", [255, 185, 15]),
    ("darkgoldenrod2", [238, 173, 14]),
    ("darkgoldenrod3", [205, 149, 12]),
    ("darkgoldenrod4", [139, 101, 8]),
    ("darkgray", [169, 169, 169]),
    ("darkgreen", [0, 100, 0]),
    ("darkgrey", [169, 169, 169]),
    ("darkkhaki", [189, 183, 107]),
    ("darkmagenta", [139, 0, 139]),
    ("darkolivegreen", [85, 107, 47]),
    ("darkolivegreen1", [202, 255, 112]),
    ("darkolivegreen2", [188, 238, 104]),
    ("darkolivegreen3", [162, 205, 90]),
    ("darkolivegreen4", [110, 139, 61]),
    ("darkorange", [255, 140, 0]),
    ("darkorange1", [255, 127, 0]),
    ("darkorange2", [238, 118, 0]),
    ("darkorange3", [205, 102, 0]),
    ("darkorange4", [139, 69, 0]),
    ("darkorchid", [153, 50, 204]),
    ("darkorchid1", [191, 62, 255]),
    ("darkorchid2", [178, 58, 238]),
    ("darkorchid3", [154, 50, 205]),
    ("darkorchid4", [104, 34, 139]),
    ("darkred", [139, 0, 0]),
    ("darksalmon", [233, 150, 122]),
    ("darkseagreen", [143, 188, 143]),
    ("darkseagreen1", [193, 255, 193]),
    ("darkseagreen2", [180, 238, 180]),
    ("darkseagreen3", [155, 205, 155]),
    ("darkseagreen4", [105, 139, 105]),
    ("darkslateblue", [72, 61, 139]),
    ("darkslategray", [47, 79, 79]),
    ("darkslategray1", [151, 255, 255]),
    ("darkslategray2", [141, 238, 238]),
    ("darkslategray3", [121, 205, 205]),
    ("darkslategray4", [82, 139, 139]),
    ("darkslategrey", [47, 79, 79]),
    ("darkturquoise", [0, 206, 209]),
    ("darkviolet", [148, 0, 211]),
    ("debianred", [215, 7, 81]),
    ("deeppink", [255, 20, 147]),
    ("deeppink1", [255, 20, 147]),
    ("deeppink2", [238, 18, 137]),
    ("deeppink3", [205, 16, 118]),
    ("deeppink4", [139, 10, 80]),
    ("deepskyblue", [0, 191, 255]),
    ("deepskyblue1", [0, 191, 255]),
    ("deepskyblue2", [0, 178, 238]),
    ("deepskyblue3", [0, 154, 205]),
    ("deepskyblue4", [0, 104, 139]),
    ("dimgray", [105, 105, 105]),
    ("dimgrey", [105, 105, 105]),
    ("dodgerblue", [30, 144, 255]),
    ("dodgerblue1", [30, 144, 255]),
    ("dodgerblue2", [28, 134, 238]),
    ("dodgerblue3", [24, 116, 205]),
    ("dodgerblue4", [16, 78, 139]),
    ("firebrick", [178, 34, 34]),
    ("firebrick1", [255, 48, 48]),
    ("firebrick2", [238, 44, 44]),
    ("firebrick3", [205, 38, 38]),
    ("firebrick4", [139, 26, 26]),
    ("floralwhite", [255, 250, 240]),
    ("forestgreen", [34, 139, 34]),
    ("gainsboro", [220, 220, 220]),
    ("ghostwhite", [248, 248, 255]),
    ("gold", [255, 215, 0]),
    ("gold1", [255, 215, 0]),
    ("gold2", [238, 201, 0]),
    ("gold3", [205, 173, 0]),
    ("gold4", [139, 117, 0]),
    ("goldenrod", [218, 165, 32]),
    ("goldenrod1", [255, 193, 37]),
    ("goldenrod2", [238, 180, 34]),
    ("goldenrod3", [205, 155, 29]),
    ("goldenrod4", [139, 105, 20]),
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
    ("green1", [0, 255, 0]),
    ("green2", [0, 238, 0]),
    ("green3", [0, 205, 0]),
    ("green4", [0, 139, 0]),
    ("greenyellow", [173, 255, 47]),
    ("grey", [190, 190, 190]),
    ("grey0", [0, 0, 0]),
    ("grey1", [3, 3, 3]),
    ("grey10", [26, 26, 26]),
    ("grey100", [255, 255, 255]),
    ("grey11", [28, 28, 28]),
    ("grey12", [31, 31, 31]),
    ("grey13", [33, 33, 33]),
    ("grey14", [36, 36, 36]),
    ("grey15", [38, 38, 38]),
    ("grey16", [41, 41, 41]),
    ("grey17", [43, 43, 43]),
    ("grey18", [46, 46, 46]),
    ("grey19", [48, 48, 48]),
    ("grey2", [5, 5, 5]),
    ("grey20", [51, 51, 51]),
    ("grey21", [54, 54, 54]),
    ("grey22", [56, 56, 56]),
    ("grey23", [59, 59, 59]),
    ("grey24", [61, 61, 61]),
    ("grey25", [64, 64, 64]),
    ("grey26", [66, 66, 66]),
    ("grey27", [69, 69, 69]),
    ("grey28", [71, 71, 71]),
    ("grey29", [74, 74, 74]),
    ("grey3", [8, 8, 8]),
    ("grey30", [77, 77, 77]),
    ("grey31", [79, 79, 79]),
    ("grey32", [82, 82, 82]),
    ("grey33", [84, 84, 84]),
    ("grey34", [87, 87, 87]),
    ("grey35", [89, 89, 89]),
    ("grey36", [92, 92, 92]),
    ("grey37", [94, 94, 94]),
    ("grey38", [97, 97, 97]),
    ("grey39", [99, 99, 99]),
    ("grey4", [10, 10, 10]),
    ("grey40", [102, 102, 102]),
    ("grey41", [105, 105, 105]),
    ("grey42", [107, 107, 107]),
    ("grey43", [110, 110, 110]),
    ("grey44", [112, 112, 112]),
    ("grey45", [115, 115, 115]),
    ("grey46", [117, 117, 117]),
    ("grey47", [120, 120, 120]),
    ("grey48", [122, 122, 122]),
    ("grey49", [125, 125, 125]),
    ("grey5", [13, 13, 13]),
    ("grey50", [127, 127, 127]),
    ("grey51", [130, 130, 130]),
    ("grey52", [133, 133, 133]),
    ("grey53", [135, 135, 135]),
    ("grey54", [138, 138, 138]),
    ("grey55", [140, 140, 140]),
    ("grey56", [143, 143, 143]),
    ("grey57", [145, 145, 145]),
    ("grey58", [148, 148, 148]),
    ("grey59", [150, 150, 150]),
    ("grey6", [15, 15, 15]),
    ("grey60", [153, 153, 153]),
    ("grey61", [156, 156, 156]),
    ("grey62", [158, 158, 158]),
    ("grey63", [161, 161, 161]),
    ("grey64", [163, 163, 163]),
    ("grey65", [166, 166, 166]),
    ("grey66", [168, 168, 168]),
    ("grey67", [171, 171, 171]),
    ("grey68", [173, 173, 173]),
    ("grey69", [176, 176, 176]),
    ("grey7", [18, 18, 18]),
    ("grey70", [179, 179, 179]),
    ("grey71", [181, 181, 181]),
    ("grey72", [184, 184, 184]),
    ("grey73", [186, 186, 186]),
    ("grey74", [189, 189, 189]),
    ("grey75", [191, 191, 191]),
    ("grey76", [194, 194, 194]),
    ("grey77", [196, 196, 196]),
    ("grey78", [199, 199, 199]),
    ("grey79", [201, 201, 201]),
    ("grey8", [20, 20, 20]),
    ("grey80", [204, 204, 204]),
    ("grey81", [207, 207, 207]),
    ("grey82", [209, 209, 209]),
    ("grey83", [212, 212, 212]),
    ("grey84", [214, 214, 214]),
    ("grey85", [217, 217, 217]),
    ("grey86", [219, 219, 219]),
    ("grey87", [222, 222, 222]),
    ("grey88", [224, 224, 224]),
    ("grey89", [227, 227, 227]),
    ("grey9", [23, 23, 23]),
    ("grey90", [229, 229, 229]),
    ("grey91", [232, 232, 232]),
    ("grey92", [235, 235, 235]),
    ("grey93", [237, 237, 237]),
    ("grey94", [240, 240, 240]),
    ("grey95", [242, 242, 242]),
    ("grey96", [245, 245, 245]),
    ("grey97", [247, 247, 247]),
    ("grey98", [250, 250, 250]),
    ("grey99", [252, 252, 252]),
    ("honeydew", [240, 255, 240]),
    ("honeydew1", [240, 255, 240]),
    ("honeydew2", [224, 238, 224]),
    ("honeydew3", [193, 205, 193]),
    ("honeydew4", [131, 139, 131]),
    ("hotpink", [255, 105, 180]),
    ("hotpink1", [255, 110, 180]),
    ("hotpink2", [238, 106, 167]),
    ("hotpink3", [205, 96, 144]),
    ("hotpink4", [139, 58, 98]),
    ("indianred", [205, 92, 92]),
    ("indianred1", [255, 106, 106]),
    ("indianred2", [238, 99, 99]),
    ("indianred3", [205, 85, 85]),
    ("indianred4", [139, 58, 58]),
    ("ivory", [255, 255, 240]),
    ("ivory1", [255, 255, 240]),
    ("ivory2", [238, 238, 224]),
    ("ivory3", [205, 205, 193]),
    ("ivory4", [139, 139, 131]),
    ("khaki", [240, 230, 140]),
    ("khaki1", [255, 246, 143]),
    ("khaki2", [238, 230, 133]),
    ("khaki3", [205, 198, 115]),
    ("khaki4", [139, 134, 78]),
    ("lavender", [230, 230, 250]),
    ("lavenderblush", [255, 240, 245]),
    ("lavenderblush1", [255, 240, 245]),
    ("lavenderblush2", [238, 224, 229]),
    ("lavenderblush3", [205, 193, 197]),
    ("lavenderblush4", [139, 131, 134]),
    ("lawngreen", [124, 252, 0]),
    ("lemonchiffon", [255, 250, 205]),
    ("lemonchiffon1", [255, 250, 205]),
    ("lemonchiffon2", [238, 233, 191]),
    ("lemonchiffon3", [205, 201, 165]),
    ("lemonchiffon4", [139, 137, 112]),
    ("lightblue", [173, 216, 230]),
    ("lightblue1", [191, 239, 255]),
    ("lightblue2", [178, 223, 238]),
    ("lightblue3", [154, 192, 205]),
    ("lightblue4", [104, 131, 139]),
    ("lightcoral", [240, 128, 128]),
    ("lightcyan", [224, 255, 255]),
    ("lightcyan1", [224, 255, 255]),
    ("lightcyan2", [209, 238, 238]),
    ("lightcyan3", [180, 205, 205]),
    ("lightcyan4", [122, 139, 139]),
    ("lightgoldenrod", [238, 221, 130]),
    ("lightgoldenrod1", [255, 236, 139]),
    ("lightgoldenrod2", [238, 220, 130]),
    ("lightgoldenrod3", [205, 190, 112]),
    ("lightgoldenrod4", [139, 129, 76]),
    ("lightgoldenrodyellow", [250, 250, 210]),
    ("lightgray", [211, 211, 211]),
    ("lightgreen", [144, 238, 144]),
    ("lightgrey", [211, 211, 211]),
    ("lightpink", [255, 182, 193]),
    ("lightpink1", [255, 174, 185]),
    ("lightpink2", [238, 162, 173]),
    ("lightpink3", [205, 140, 149]),
    ("lightpink4", [139, 95, 101]),
    ("lightsalmon", [255, 160, 122]),
    ("lightsalmon1", [255, 160, 122]),
    ("lightsalmon2", [238, 149, 114]),
    ("lightsalmon3", [205, 129, 98]),
    ("lightsalmon4", [139, 87, 66]),
    ("lightseagreen", [32, 178, 170]),
    ("lightskyblue", [135, 206, 250]),
    ("lightskyblue1", [176, 226, 255]),
    ("lightskyblue2", [164, 211, 238]),
    ("lightskyblue3", [141, 182, 205]),
    ("lightskyblue4", [96, 123, 139]),
    ("lightslateblue", [132, 112, 255]),
    ("lightslategray", [119, 136, 153]),
    ("lightslategrey", [119, 136, 153]),
    ("lightsteelblue", [176, 196, 222]),
    ("lightsteelblue1", [202, 225, 255]),
    ("lightsteelblue2", [188, 210, 238]),
    ("lightsteelblue3", [162, 181, 205]),
    ("lightsteelblue4", [110, 123, 139]),
    ("lightyellow", [255, 255, 224]),
    ("lightyellow1", [255, 255, 224]),
    ("lightyellow2", [238, 238, 209]),
    ("lightyellow3", [205, 205, 180]),
    ("lightyellow4", [139, 139, 122]),
    ("limegreen", [50, 205, 50]),
    ("linen", [250, 240, 230]),
    ("magenta", [255, 0, 255]),
    ("magenta1", [255, 0, 255]),
    ("magenta2", [238, 0, 238]),
    ("magenta3", [205, 0, 205]),
    ("magenta4", [139, 0, 139]),
    ("maroon", [176, 48, 96]),
    ("maroon1", [255, 52, 179]),
    ("maroon2", [238, 48, 167]),
    ("maroon3", [205, 41, 144]),
    ("maroon4", [139, 28, 98]),
    ("mediumaquamarine", [102, 205, 170]),
    ("mediumblue", [0, 0, 205]),
    ("mediumorchid", [186, 85, 211]),
    ("mediumorchid1", [224, 102, 255]),
    ("mediumorchid2", [209, 95, 238]),
    ("mediumorchid3", [180, 82, 205]),
    ("mediumorchid4", [122, 55, 139]),
    ("mediumpurple", [147, 112, 219]),
    ("mediumpurple1", [171, 130, 255]),
    ("mediumpurple2", [159, 121, 238]),
    ("mediumpurple3", [137, 104, 205]),
    ("mediumpurple4", [93, 71, 139]),
    ("mediumseagreen", [60, 179, 113]),
    ("mediumslateblue", [123, 104, 238]),
    ("mediumspringgreen", [0, 250, 154]),
    ("mediumturquoise", [72, 209, 204]),
    ("mediumvioletred", [199, 21, 133]),
    ("midnightblue", [25, 25, 112]),
    ("mintcream", [245, 255, 250]),
    ("mistyrose", [255, 228, 225]),
    ("mistyrose1", [255, 228, 225]),
    ("mistyrose2", [238, 213, 210]),
    ("mistyrose3", [205, 183, 181]),
    ("mistyrose4", [139, 125, 123]),
    ("moccasin", [255, 228, 181]),
    ("navajowhite", [255, 222, 173]),
    ("navajowhite1", [255, 222, 173]),
    ("navajowhite2", [238, 207, 161]),
    ("navajowhite3", [205, 179, 139]),
    ("navajowhite4", [139, 121, 94]),
    ("navy", [0, 0, 128]),
    ("navyblue", [0, 0, 128]),
    ("oldlace", [253, 245, 230]),
    ("olivedrab", [107, 142, 35]),
    ("olivedrab1", [192, 255, 62]),
    ("olivedrab2", [179, 238, 58]),
    ("olivedrab3", [154, 205, 50]),
    ("olivedrab4", [105, 139, 34]),
    ("orange", [255, 165, 0]),
    ("orange1", [255, 165, 0]),
    ("orange2", [238, 154, 0]),
    ("orange3", [205, 133, 0]),
    ("orange4", [139, 90, 0]),
    ("orangered", [255, 69, 0]),
    ("orangered1", [255, 69, 0]),
    ("orangered2", [238, 64, 0]),
    ("orangered3", [205, 55, 0]),
    ("orangered4", [139, 37, 0]),
    ("orchid", [218, 112, 214]),
    ("orchid1", [255, 131, 250]),
    ("orchid2", [238, 122, 233]),
    ("orchid3", [205, 105, 201]),
    ("orchid4", [139, 71, 137]),
    ("palegoldenrod", [238, 232, 170]),
    ("palegreen", [152, 251, 152]),
    ("palegreen1", [154, 255, 154]),
    ("palegreen2", [144, 238, 144]),
    ("palegreen3", [124, 205, 124]),
    ("palegreen4", [84, 139, 84]),
    ("paleturquoise", [175, 238, 238]),
    ("paleturquoise1", [187, 255, 255]),
    ("paleturquoise2", [174, 238, 238]),
    ("paleturquoise3", [150, 205, 205]),
    ("paleturquoise4", [102, 139, 139]),
    ("palevioletred", [219, 112, 147]),
    ("palevioletred1", [255, 130, 171]),
    ("palevioletred2", [238, 121, 159]),
    ("palevioletred3", [205, 104, 137]),
    ("palevioletred4", [139, 71, 93]),
    ("papayawhip", [255, 239, 213]),
    ("peachpuff", [255, 218, 185]),
    ("peachpuff1", [255, 218, 185]),
    ("peachpuff2", [238, 203, 173]),
    ("peachpuff3", [205, 175, 149]),
    ("peachpuff4", [139, 119, 101]),
    ("peru", [205, 133, 63]),
    ("pink", [255, 192, 203]),
    ("pink1", [255, 181, 197]),
    ("pink2", [238, 169, 184]),
    ("pink3", [205, 145, 158]),
    ("pink4", [139, 99, 108]),
    ("plum", [221, 160, 221]),
    ("plum1", [255, 187, 255]),
    ("plum2", [238, 174, 238]),
    ("plum3", [205, 150, 205]),
    ("plum4", [139, 102, 139]),
    ("powderblue", [176, 224, 230]),
    ("purple", [160, 32, 240]),
    ("purple1", [155, 48, 255]),
    ("purple2", [145, 44, 238]),
    ("purple3", [125, 38, 205]),
    ("purple4", [85, 26, 139]),
    ("red", [255, 0, 0]),
    ("red1", [255, 0, 0]),
    ("red2", [238, 0, 0]),
    ("red3", [205, 0, 0]),
    ("red4", [139, 0, 0]),
    ("rosybrown", [188, 143, 143]),
    ("rosybrown1", [255, 193, 193]),
    ("rosybrown2", [238, 180, 180]),
    ("rosybrown3", [205, 155, 155]),
    ("rosybrown4", [139, 105, 105]),
    ("royalblue", [65, 105, 225]),
    ("royalblue1", [72, 118, 255]),
    ("royalblue2", [67, 110, 238]),
    ("royalblue3", [58, 95, 205]),
    ("royalblue4", [39, 64, 139]),
    ("saddlebrown", [139, 69, 19]),
    ("salmon", [250, 128, 114]),
    ("salmon1", [255, 140, 105]),
    ("salmon2", [238, 130, 98]),
    ("salmon3", [205, 112, 84]),
    ("salmon4", [139, 76, 57]),
    ("sandybrown", [244, 164, 96]),
    ("seagreen", [46, 139, 87]),
    ("seagreen1", [84, 255, 159]),
    ("seagreen2", [78, 238, 148]),
    ("seagreen3", [67, 205, 128]),
    ("seagreen4", [46, 139, 87]),
    ("seashell", [255, 245, 238]),
    ("seashell1", [255, 245, 238]),
    ("seashell2", [238, 229, 222]),
    ("seashell3", [205, 197, 191]),
    ("seashell4", [139, 134, 130]),
    ("sienna", [160, 82, 45]),
    ("sienna1", [255, 130, 71]),
    ("sienna2", [238, 121, 66]),
    ("sienna3", [205, 104, 57]),
    ("sienna4", [139, 71, 38]),
    ("skyblue", [135, 206, 235]),
    ("skyblue1", [135, 206, 255]),
    ("skyblue2", [126, 192, 238]),
    ("skyblue3", [108, 166, 205]),
    ("skyblue4", [74, 112, 139]),
    ("slateblue", [106, 90, 205]),
    ("slateblue1", [131, 111, 255]),
    ("slateblue2", [122, 103, 238]),
    ("slateblue3", [105, 89, 205]),
    ("slateblue4", [71, 60, 139]),
    ("slategray", [112, 128, 144]),
    ("slategray1", [198, 226, 255]),
    ("slategray2", [185, 211, 238]),
    ("slategray3", [159, 182, 205]),
    ("slategray4", [108, 123, 139]),
    ("slategrey", [112, 128, 144]),
    ("snow", [255, 250, 250]),
    ("snow1", [255, 250, 250]),
    ("snow2", [238, 233, 233]),
    ("snow3", [205, 201, 201]),
    ("snow4", [139, 137, 137]),
    ("springgreen", [0, 255, 127]),
    ("springgreen1", [0, 255, 127]),
    ("springgreen2", [0, 238, 118]),
    ("springgreen3", [0, 205, 102]),
    ("springgreen4", [0, 139, 69]),
    ("steelblue", [70, 130, 180]),
    ("steelblue1", [99, 184, 255]),
    ("steelblue2", [92, 172, 238]),
    ("steelblue3", [79, 148, 205]),
    ("steelblue4", [54, 100, 139]),
    ("tan", [210, 180, 140]),
    ("tan1", [255, 165, 79]),
    ("tan2", [238, 154, 73]),
    ("tan3", [205, 133, 63]),
    ("tan4", [139, 90, 43]),
    ("thistle", [216, 191, 216]),
    ("thistle1", [255, 225, 255]),
    ("thistle2", [238, 210, 238]),
    ("thistle3", [205, 181, 205]),
    ("thistle4", [139, 123, 139]),
    ("tomato", [255, 99, 71]),
    ("tomato1", [255, 99, 71]),
    ("tomato2", [238, 92, 66]),
    ("tomato3", [205, 79, 57]),
    ("tomato4", [139, 54, 38]),
    ("turquoise", [64, 224, 208]),
    ("turquoise1", [0, 245, 255]),
    ("turquoise2", [0, 229, 238]),
    ("turquoise3", [0, 197, 205]),
    ("turquoise4", [0, 134, 139]),
    ("violet", [238, 130, 238]),
    ("violetred", [208, 32, 144]),
    ("violetred1", [255, 62, 150]),
    ("violetred2", [238, 58, 140]),
    ("violetred3", [205, 50, 120]),
    ("violetred4", [139, 34, 82]),
    ("wheat", [245, 222, 179]),
    ("wheat1", [255, 231, 186]),
    ("wheat2", [238, 216, 174]),
    ("wheat3", [205, 186, 150]),
    ("wheat4", [139, 126, 102]),
    ("white", [255, 255, 255]),
    ("whitesmoke", [245, 245, 245]),
    ("yellow", [255, 255, 0]),
    ("yellow1", [255, 255, 0]),
    ("yellow2", [238, 238, 0]),
    ("yellow3", [205, 205, 0]),
    ("yellow4", [139, 139, 0]),
    ("yellowgreen", [154, 205, 50]),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Each fixture beside its expected RGBA. The fixtures and their pixels
    /// were written by a script from seeded random choices; the pixels are
    /// Pillow 12.3's decode of each fixture, or, for what Pillow cannot read
    /// (`None`, colour names, short and long hex, strings sharing a line,
    /// XPM 2), of the same image rewritten in the one form it reads
    /// (`#rrggbb` colours, one string a line, a stand-in colour for `None`,
    /// made transparent after). netpbm 11.10's `xpmtoppm` decodes
    /// `gimp.xpm`, `two-chars.xpm` and `three-chars.xpm` to the same pixels,
    /// and `named.xpm` too once each colour is one key on a line of its own
    /// (it refuses several keys to a colour, and a comment before the
    /// declaration); it reads no XPM 2.
    macro_rules! fixture {
        ($name:literal) => {
            (
                $name,
                include_bytes!(concat!("../fixtures/xpm/", $name)) as &[u8],
                include_bytes!(concat!("../fixtures/xpm/", $name, ".rgba")) as &[u8],
            )
        };
    }

    #[test]
    fn every_fixture_decodes_to_its_expected_pixels() {
        for (name, bytes, rgba) in [
            fixture!("gimp.xpm"),
            fixture!("two-chars.xpm"),
            fixture!("three-chars.xpm"),
            fixture!("named.xpm"),
            fixture!("xpm2.xpm"),
        ] {
            let image = decode(bytes, &Limits::DEFAULT).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(image.pixels(), rgba, "{name}");
        }
    }

    fn pixels(text: &str) -> Result<Vec<[u8; 4]>, ImageError> {
        decode(text.as_bytes(), &Limits::DEFAULT)
            .map(|image| image.pixels().as_chunks::<4>().0.to_vec())
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const CLEAR: [u8; 4] = [0; 4];

    /// **Hex is scaled to eight bits, rounded; names match as X matches
    /// them.**
    #[test]
    fn hex_is_scaled_and_names_match_as_x_matches_them() {
        assert_eq!(colour(b"#102030"), Some([16, 32, 48, 255]));
        // One digit: v * 255 / 15, so f is 255 and 8 is 136.
        assert_eq!(colour(b"#f80"), Some([255, 136, 0, 255]));
        // Three digits: 0x800 * 255 / 4095 is 127.53.
        assert_eq!(colour(b"#800800800"), Some([128, 128, 128, 255]));
        // Four digits: 0xff * 255 / 65535 is 0.99, and 0xff7f's 254.5
        // rounds up.
        assert_eq!(colour(b"#00ff00ffff7f"), Some([1, 1, 255, 255]));
        assert_eq!(colour(b"NONE"), Some(CLEAR));
        // rgb.txt: 173 216 230 light blue; 127 127 127 gray50.
        assert_eq!(colour(b"LightBlue"), Some([173, 216, 230, 255]));
        assert_eq!(colour(b"LIGHTBLUE"), Some([173, 216, 230, 255]));
        assert_eq!(colour(b"gray50"), Some([127, 127, 127, 255]));
        assert_eq!(colour(b"GREY50"), Some([127, 127, 127, 255]));
        for bad in [
            &b"#12345"[..],
            b"#",
            b"#1234567890abc",
            b"#123456789abcdef",
            b"#+12+12+12",
            b"#gg0000",
            b"no such colour",
            b"",
        ] {
            assert_eq!(colour(bad), None, "{:?}", String::from_utf8_lossy(bad));
        }
    }

    /// **The table binary searches only if it is sorted, and matches only
    /// names written the way a lookup normalises them.**
    #[test]
    fn the_colour_table_is_sorted_and_normalised() {
        assert!(
            X11_COLOURS.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "sorted, no repeats"
        );
        for (name, _) in X11_COLOURS {
            assert!(
                name.bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()),
                "{name}"
            );
        }
        // The first and last names, which a binary search off by one at
        // either end would miss.
        assert_eq!(colour(b"aliceblue"), Some([240, 248, 255, 255]));
        assert_eq!(colour(b"YellowGreen"), Some([154, 205, 50, 255]));
    }

    /// **`c` is taken first, then `g`, `g4` and `m`; `s` is ignored; a
    /// value may be several words; a key defined twice keeps its first.**
    #[test]
    fn the_colour_key_falls_back_in_order() {
        let image = pixels(
            r#"/* XPM */
static char *x[] = {
"6 1 7 1",
"a s red m #ff0000 g #00ff00 c blue",
"b m #ff0000 g4 #0000ff g #00ff00",
"c m #ff0000 g4 #0000ff",
"d s blue m red",
"e c light   blue",
"f s c c red",
"a c red",
"abcdef"};"#,
        )
        .expect("decodes");
        assert_eq!(image, [BLUE, GREEN, BLUE, RED, [173, 216, 230, 255], RED]);
        for bad in [
            "\"a red\"",
            "\"a red c blue\"",
            "\"a s red\"",
            "\"a c\"",
            "\"a c not-a-colour\"",
        ] {
            let text = format!("/* XPM */\n{{\"1 1 1 1\", {bad}, \"a\"}}");
            assert!(
                matches!(pixels(&text), Err(ImageError::Malformed(_))),
                "{bad}"
            );
        }
    }

    /// **Keys take `chars_per_pixel` bytes, spaces included, and a row
    /// longer than its pixels ends where they do.**
    #[test]
    fn wide_keys_read_whole_including_spaces() {
        let image = pixels(
            "/* XPM */\n{\"3 2 3 2\",\n\"  c None\",\n\". c #ff0000\",\n\" .c #00ff00\",\n\"  .  .\",\n\".  .   trailing\"};",
        )
        .expect("decodes");
        assert_eq!(image, [CLEAR, RED, GREEN, RED, GREEN, CLEAR]);
    }

    /// **Strings may sit anywhere among comments and C, and nothing after
    /// the last row is read**, extensions included.
    #[test]
    fn comments_and_extensions_are_stepped_over() {
        let image = pixels(
            r#"
/* XPM */
/* a "quoted" comment */
static const char * const x[] = {
/* width height ncolors cpp x_hot y_hot */ "2 2 2 1 0 1 XPMEXT",
"r c red", /* red */ "b c blue", /*
   a comment over lines */
"rb", "br",
"XPMEXT author anybody",
"XPMENDEXT"
};"#,
        )
        .expect("decodes");
        assert_eq!(image, [RED, BLUE, BLUE, RED]);
    }

    /// **XPM 2 is lines, `!` starting a comment.**
    #[test]
    fn xpm2_is_read_a_line_at_a_time() {
        let image = pixels("! XPM2\r\n2 1 2 1\r\n! a comment\r\nr c red\r\nb c #0000ff\r\nrb\r\n")
            .expect("decodes");
        assert_eq!(image, [RED, BLUE]);
    }

    /// **Damage is malformed, a cap or an old form unsupported, and a size
    /// past the limits too large before anything is built for it.**
    #[test]
    fn hostile_and_damaged_files_are_refused() {
        let xpm = |values: &str, rest: &str| format!("/* XPM */\n{{\"{values}\",\n{rest}}};");
        for bad in [
            String::new(),
            "P6 1 1 255".to_owned(),
            "/* XPM */".to_owned(),
            xpm("1 1 1", "\"a c red\", \"a\""),
            xpm("1 x 1 1", "\"a c red\", \"a\""),
            xpm("1 1 0 1", "\"a\""),
            xpm("1 1 1 0", "\"c red\", \"a\""),
            // Too few colours, too few rows, a short row.
            xpm("1 1 2 1", "\"a c red\""),
            xpm("1 2 1 1", "\"a c red\", \"a\""),
            xpm("2 1 1 1", "\"a c red\", \"a\""),
            // A key shorter than the characters a pixel.
            xpm("1 1 1 2", "\"a\", \"aa\""),
            // A pixel no colour defines.
            xpm("2 1 1 1", "\"a c red\", \"ab\""),
            "/* XPM */ { \"1 1 1 1\", \"a c red\", \"a".to_owned(),
            "/* XPM */ { \"1 1 1 1\", /* never ends".to_owned(),
        ] {
            assert!(
                matches!(pixels(&bad), Err(ImageError::Malformed(_))),
                "{bad:?}"
            );
        }
        for bad in [
            "#define x_format 1\n#define x_width 1\n".to_owned(),
            xpm("1 1 1 32", "\"a c red\", \"a\""),
            xpm(&format!("1 1 {} 1", MAX_COLOURS + 1), "\"a c red\", \"a\""),
        ] {
            assert!(
                matches!(pixels(&bad), Err(ImageError::Unsupported(_))),
                "{bad:?}"
            );
        }
        assert!(
            pixels(&xpm(
                "1 1 1 31",
                &format!("\"{} c red\", \"{0}\"", "a".repeat(31))
            ))
            .is_ok()
        );
        // Gigapixels claimed by a file a few bytes long are refused before
        // the colours, which are not even there, are read.
        assert!(matches!(
            pixels(&xpm("100000 100000 1 1", "")),
            Err(ImageError::TooLarge { .. })
        ));
        assert!(matches!(
            pixels(&xpm("0 1 1 1", "")),
            Err(ImageError::Empty { .. })
        ));
    }
}
