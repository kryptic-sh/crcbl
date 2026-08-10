//! Glyph atlas and text layout for the immediate-mode UI.
//!
//! Provides a built-in monospace bitmap font (the "engine font") with glyph
//! metrics and a simple text-layout function that produces positioned glyph
//! indices for the draw list.

use glam::Vec2;

// ---------------------------------------------------------------------------
// Glyph metrics
// ---------------------------------------------------------------------------

/// Metrics for a single glyph in the atlas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphMetrics {
    /// Width of the glyph bitmap in pixels.
    pub width: u32,
    /// Height of the glyph bitmap in pixels.
    pub height: u32,
    /// Horizontal offset from the cursor position to the left edge of the
    /// glyph bitmap.
    pub bearing_x: i32,
    /// Vertical offset from the baseline to the top of the glyph bitmap.
    pub bearing_y: i32,
    /// Horizontal advance — how far the cursor moves after this glyph.
    pub advance: f32,
}

impl GlyphMetrics {
    /// The bounding rectangle for this glyph, relative to the cursor position.
    ///
    /// `cursor` is a **baseline** position: `bearing_y` is measured up from the
    /// baseline, so the returned `min.y` sits above `cursor.y`.
    #[must_use]
    pub fn rect(&self, cursor: Vec2) -> (Vec2, Vec2) {
        self.rect_scaled(cursor, 1.0)
    }

    /// The bounding rectangle for this glyph at `scale`, relative to the
    /// baseline position `cursor`.
    ///
    /// Only the glyph's own extents are scaled — `cursor` is already in final
    /// screen space, so scaling it again would move the anchor with the font
    /// size.
    #[must_use]
    pub fn rect_scaled(&self, cursor: Vec2, scale: f32) -> (Vec2, Vec2) {
        let min = Vec2::new(
            cursor.x + self.bearing_x as f32 * scale,
            cursor.y - self.bearing_y as f32 * scale,
        );
        let max = Vec2::new(
            min.x + self.width as f32 * scale,
            min.y + self.height as f32 * scale,
        );
        (min, max)
    }
}

// ---------------------------------------------------------------------------
// FontAtlas
// ---------------------------------------------------------------------------

/// A simple monospace bitmap font atlas.
///
/// The built-in engine font is a fixed-pitch 8×13 px bitmap covering ASCII
/// 32–126, plus a trailing `.notdef` box. Glyphs are arranged in a single row
/// in the atlas texture.
///
/// # Atlas layout
///
/// Each glyph is `GLYPH_WIDTH × GLYPH_HEIGHT` pixels. They are packed
/// left-to-right, one row, in ASCII order, with `.notdef` last. The total atlas
/// width is `GLYPH_WIDTH * GLYPH_COUNT` pixels.
pub struct FontAtlas {
    /// Glyph metrics for each atlas column, indexed by [`glyph_index`].
    metrics: Vec<GlyphMetrics>,
    /// Atlas texture dimensions in pixels (width, height).
    pub texture_size: (u32, u32),
}

/// Number of printable-ASCII glyphs in the built-in atlas (32–126 inclusive).
pub const ASCII_GLYPH_COUNT: usize = 95;

/// Atlas column of the `.notdef` glyph — the fallback drawn for every codepoint
/// outside ASCII 32–126.
pub const NOTDEF_INDEX: usize = ASCII_GLYPH_COUNT;

/// Number of glyphs in the built-in atlas: printable ASCII plus `.notdef`.
pub const GLYPH_COUNT: usize = ASCII_GLYPH_COUNT + 1;

/// First ASCII codepoint in the atlas.
pub const FIRST_CHAR: u8 = 32;

/// Last ASCII codepoint in the atlas.
pub const LAST_CHAR: u8 = 126;

/// Width of each glyph in the built-in bitmap font.
pub const GLYPH_WIDTH: u32 = 8;

/// Height of each glyph in the built-in bitmap font.
pub const GLYPH_HEIGHT: u32 = 13;

/// Horizontal advance for each glyph (pixels from one cursor position to the
/// next).
pub const GLYPH_ADVANCE: f32 = 10.0;

/// Line height (vertical advance from one line to the next).
pub const LINE_HEIGHT: f32 = 16.0;

/// Ascender height in pixels (baseline to top of capital letter).
pub const ASCENDER: i32 = 10;

/// The atlas column a codepoint renders from.
///
/// Codepoints outside the printable-ASCII range map to [`NOTDEF_INDEX`]. The
/// test is on the **codepoint**, not on a truncated byte: `'Ł'` (U+0141) and
/// `'A'` (U+0041) share a low byte but are not the same glyph.
#[must_use]
pub fn glyph_index(c: char) -> usize {
    let cp = c as u32;
    if cp >= FIRST_CHAR as u32 && cp <= LAST_CHAR as u32 {
        (cp - FIRST_CHAR as u32) as usize
    } else {
        NOTDEF_INDEX
    }
}

/// Metrics returned when the atlas holds no glyph at all — an empty glyph that
/// advances nothing, so a degenerate atlas lays text out as nothing rather than
/// panicking.
static EMPTY_GLYPH: GlyphMetrics = GlyphMetrics {
    width: 0,
    height: 0,
    bearing_x: 0,
    bearing_y: 0,
    advance: 0.0,
};

impl FontAtlas {
    /// Create the built-in monospace font atlas.
    #[must_use]
    pub fn built_in() -> Self {
        let mut metrics: Vec<GlyphMetrics> = (FIRST_CHAR..=LAST_CHAR)
            .map(|c| {
                let w = if c <= b' ' { 0 } else { GLYPH_WIDTH };
                GlyphMetrics {
                    width: w,
                    height: GLYPH_HEIGHT,
                    bearing_x: 1,
                    bearing_y: ASCENDER,
                    advance: GLYPH_ADVANCE,
                }
            })
            .collect();
        // `.notdef`, drawn for anything outside printable ASCII.
        metrics.push(GlyphMetrics {
            width: GLYPH_WIDTH,
            height: GLYPH_HEIGHT,
            bearing_x: 1,
            bearing_y: ASCENDER,
            advance: GLYPH_ADVANCE,
        });

        let texture_size = (GLYPH_WIDTH * GLYPH_COUNT as u32, GLYPH_HEIGHT);
        Self {
            metrics,
            texture_size,
        }
    }

    /// Look up the metrics for a character.
    ///
    /// Any codepoint outside ASCII 32–126 — including control characters and
    /// everything non-Latin — resolves to the `.notdef` box.
    #[must_use]
    pub fn glyph(&self, c: char) -> &GlyphMetrics {
        let idx = glyph_index(c);
        self.metrics
            .get(idx)
            .or_else(|| self.metrics.get(NOTDEF_INDEX))
            .or_else(|| self.metrics.first())
            .unwrap_or(&EMPTY_GLYPH)
    }

    /// Measure the width of a string in pixels at the given scale.
    ///
    /// `scale` is a multiplier relative to the baked-in glyph size (1.0 =
    /// natural pixel size). Multi-line text measures as its **widest** line, not
    /// as the sum of every line.
    #[must_use]
    pub fn text_width(&self, text: &str, scale: f32) -> f32 {
        let mut widest = 0.0f32;
        let mut line = 0.0f32;
        for c in text.chars() {
            if c == '\n' {
                widest = widest.max(line);
                line = 0.0;
                continue;
            }
            line += self.glyph(c).advance * scale;
        }
        widest.max(line)
    }

    /// Number of lines a string lays out to (always at least one).
    #[must_use]
    pub fn line_count(&self, text: &str) -> usize {
        1 + text.chars().filter(|&c| c == '\n').count()
    }

    /// Layout text into screen-space glyph positions.
    ///
    /// `pos` is the **top-left anchor** of the first line's em box — the same
    /// anchor [`DrawCommand::Text::pos`] documents — not a baseline. The
    /// baseline is derived from it as `pos.y + ASCENDER * scale`.
    ///
    /// Returns a list of `(char, min, max)` quads where `min`/`max` are the
    /// screen-space corners of each glyph, with `min` the top-left corner in
    /// the Y-down screen convention.
    ///
    /// [`DrawCommand::Text::pos`]: crate::draw_list::DrawCommand::Text
    pub fn layout_line(&self, text: &str, pos: Vec2, scale: f32) -> Vec<(char, Vec2, Vec2)> {
        let mut out = Vec::with_capacity(text.len());
        // `cursor` is a baseline position; `pos` is the top of the em box.
        let mut cursor = Vec2::new(pos.x, pos.y + ASCENDER as f32 * scale);
        for c in text.chars() {
            if c == '\n' {
                cursor.x = pos.x;
                cursor.y += LINE_HEIGHT * scale;
                continue;
            }
            let g = self.glyph(c);
            if g.width > 0 {
                let (min, max) = g.rect_scaled(cursor, scale);
                out.push((c, min, max));
            }
            cursor.x += g.advance * scale;
        }
        out
    }

    /// Number of glyphs in the atlas.
    #[must_use]
    pub fn len(&self) -> usize {
        self.metrics.len()
    }

    /// Whether the atlas is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.metrics.is_empty()
    }

    /// UV u-coordinate of the left edge of the given glyph in the atlas
    /// texture (range [0, 1]).
    ///
    /// Codepoints with no glyph resolve to `.notdef`, so the result is always
    /// inside `[0, 1]`.
    #[must_use]
    pub fn glyph_u_min(&self, c: char) -> f32 {
        let atlas_w = self.texture_size.0 as f32;
        if atlas_w == 0.0 {
            return 0.0;
        }
        let col = glyph_index(c) as u32;
        (col * GLYPH_WIDTH) as f32 / atlas_w
    }

    /// UV u-coordinate of the right edge of the given glyph in the atlas
    /// texture (range [0, 1]).
    ///
    /// Codepoints with no glyph resolve to `.notdef`, so the result is always
    /// inside `[0, 1]`.
    #[must_use]
    pub fn glyph_u_max(&self, c: char) -> f32 {
        let atlas_w = self.texture_size.0 as f32;
        if atlas_w == 0.0 {
            return 0.0;
        }
        let col = glyph_index(c) as u32;
        ((col + 1) * GLYPH_WIDTH) as f32 / atlas_w
    }

    /// Generate the atlas texture pixel data as R8_UNORM bytes.
    ///
    /// Returns `(width, height, data)` where `data` is row-major, one byte per
    /// pixel. 0 = transparent, 255 = fully opaque glyph.
    #[must_use]
    pub fn glyph_bitmap(&self) -> (u32, u32, Vec<u8>) {
        let (w, h) = self.texture_size;
        let mut data = vec![0u8; (w * h) as usize];

        for glyph_idx in 0..GLYPH_COUNT {
            let rows = glyph_rows_at(glyph_idx);
            let x_offset = glyph_idx as u32 * GLYPH_WIDTH;
            for (row, &row_bits) in rows.iter().enumerate() {
                for col in 0..GLYPH_WIDTH as usize {
                    let bit = (row_bits >> (7 - col)) & 1;
                    let px = (x_offset + col as u32) as usize;
                    let py = row;
                    data[py * w as usize + px] = if bit != 0 { 255 } else { 0 };
                }
            }
        }
        (w, h, data)
    }
}

// ---------------------------------------------------------------------------
// Embedded 8×13 bitmap font (row-major bit patterns, MSB = leftmost pixel)
// ---------------------------------------------------------------------------

/// The `.notdef` glyph: a hollow box, drawn for every codepoint the atlas has
/// no bitmap for.
const NOTDEF_ROWS: [u8; 13] = [
    0xFE, 0x82, 0x82, 0x82, 0x82, 0x82, 0x82, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Returns the 13 row bytes for the atlas column `index`.
///
/// Columns `0..ASCII_GLYPH_COUNT` are printable ASCII in order; the last is
/// `.notdef`.
fn glyph_rows_at(index: usize) -> [u8; 13] {
    if index < ASCII_GLYPH_COUNT {
        glyph_rows(char::from(FIRST_CHAR + index as u8))
    } else {
        NOTDEF_ROWS
    }
}

/// Returns the 13 row bytes for the given ASCII character.
///
/// Each byte encodes one row of the 8-pixel-wide glyph; the MSB (bit 7) is the
/// leftmost pixel. Only characters 32–126 are valid; others return `.notdef`.
fn glyph_rows(c: char) -> [u8; 13] {
    // Compact 8×13 embedded bitmap font.
    // clang-format off
    match c {
        ' ' => [0x00; 13],
        '!' => [
            0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00,
        ],
        '"' => [
            0x6C, 0x6C, 0x6C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '#' => [
            0x00, 0x6C, 0x6C, 0xFE, 0x6C, 0x6C, 0xFE, 0x6C, 0x6C, 0x00, 0x00, 0x00, 0x00,
        ],
        '$' => [
            0x10, 0x7C, 0xD6, 0xD0, 0x7C, 0x16, 0xD6, 0x7C, 0x10, 0x00, 0x00, 0x00, 0x00,
        ],
        '%' => [
            0x00, 0xE6, 0xA6, 0xEC, 0x18, 0x30, 0x6E, 0xCA, 0xCE, 0x00, 0x00, 0x00, 0x00,
        ],
        '&' => [
            0x00, 0x38, 0x6C, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0x76, 0x00, 0x00, 0x00, 0x00,
        ],
        '\'' => [
            0x18, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '(' => [
            0x0C, 0x18, 0x30, 0x30, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00, 0x00, 0x00, 0x00,
        ],
        ')' => [
            0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00,
        ],
        '*' => [
            0x00, 0x00, 0x10, 0xD6, 0x7C, 0x38, 0x7C, 0xD6, 0x10, 0x00, 0x00, 0x00, 0x00,
        ],
        '+' => [
            0x00, 0x00, 0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        ',' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30, 0x00, 0x00, 0x00,
        ],
        '-' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '.' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00,
        ],
        '/' => [
            0x06, 0x0C, 0x0C, 0x18, 0x18, 0x30, 0x30, 0x60, 0x60, 0x00, 0x00, 0x00, 0x00,
        ],
        '0' => [
            0x7C, 0xC6, 0xCE, 0xDE, 0xF6, 0xE6, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '1' => [
            0x18, 0x38, 0x78, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '2' => [
            0x7C, 0xC6, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '3' => [
            0x7C, 0xC6, 0x06, 0x3C, 0x06, 0x06, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '4' => [
            0x0C, 0x1C, 0x3C, 0x6C, 0xCC, 0xFE, 0x0C, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '5' => [
            0xFE, 0xC0, 0xC0, 0xFC, 0x06, 0x06, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '6' => [
            0x3C, 0x60, 0xC0, 0xFC, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '7' => [
            0xFE, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '8' => [
            0x7C, 0xC6, 0xC6, 0x7C, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '9' => [
            0x7C, 0xC6, 0xC6, 0xC6, 0x7E, 0x06, 0x0C, 0x78, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        ':' => [
            0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        ';' => [
            0x00, 0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00,
        ],
        '<' => [
            0x00, 0x06, 0x0C, 0x18, 0x30, 0x18, 0x0C, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '=' => [
            0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '>' => [
            0x00, 0x60, 0x30, 0x18, 0x0C, 0x18, 0x30, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '?' => [
            0x7C, 0xC6, 0x06, 0x0C, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00,
        ],
        '@' => [
            0x7C, 0xC6, 0xDE, 0xDE, 0xDE, 0xDC, 0xC0, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'A' => [
            0x10, 0x38, 0x6C, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'B' => [
            0xFC, 0xC6, 0xC6, 0xFC, 0xC6, 0xC6, 0xC6, 0xFC, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'C' => [
            0x7C, 0xC6, 0xC0, 0xC0, 0xC0, 0xC0, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'D' => [
            0xF8, 0xCC, 0xC6, 0xC6, 0xC6, 0xC6, 0xCC, 0xF8, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'E' => [
            0xFE, 0xC0, 0xC0, 0xFC, 0xC0, 0xC0, 0xC0, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'F' => [
            0xFE, 0xC0, 0xC0, 0xFC, 0xC0, 0xC0, 0xC0, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'G' => [
            0x7C, 0xC6, 0xC0, 0xDE, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'H' => [
            0xC6, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'I' => [
            0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'J' => [
            0x1E, 0x0C, 0x0C, 0x0C, 0x0C, 0xCC, 0xCC, 0x78, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'K' => [
            0xC6, 0xCC, 0xD8, 0xF0, 0xF0, 0xD8, 0xCC, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'L' => [
            0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'M' => [
            0xC6, 0xEE, 0xFE, 0xD6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'N' => [
            0xC6, 0xE6, 0xF6, 0xDE, 0xCE, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'O' => [
            0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'P' => [
            0xFC, 0xC6, 0xC6, 0xC6, 0xFC, 0xC0, 0xC0, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'Q' => [
            0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xDE, 0xCC, 0x7A, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'R' => [
            0xFC, 0xC6, 0xC6, 0xFC, 0xD8, 0xCC, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'S' => [
            0x7C, 0xC6, 0xC0, 0x7C, 0x06, 0x06, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'T' => [
            0xFF, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'U' => [
            0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'V' => [
            0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'W' => [
            0xC6, 0xC6, 0xC6, 0xD6, 0xD6, 0xFE, 0x6C, 0x6C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'X' => [
            0xC6, 0xC6, 0x6C, 0x38, 0x38, 0x6C, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'Y' => [
            0xC3, 0xC3, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'Z' => [
            0xFE, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '[' => [
            0x7C, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '\\' => [
            0x60, 0x30, 0x30, 0x18, 0x18, 0x0C, 0x0C, 0x06, 0x06, 0x00, 0x00, 0x00, 0x00,
        ],
        ']' => [
            0x7C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '^' => [
            0x10, 0x38, 0x6C, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '_' => [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x00, 0x00, 0x00,
        ],
        '`' => [
            0x30, 0x18, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'a' => [
            0x00, 0x00, 0x78, 0x0C, 0x7C, 0xCC, 0xCC, 0x76, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'b' => [
            0xC0, 0xC0, 0xDC, 0xE6, 0xC6, 0xC6, 0xE6, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'c' => [
            0x00, 0x00, 0x7C, 0xC6, 0xC0, 0xC0, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'd' => [
            0x0C, 0x0C, 0x7C, 0xCC, 0xCC, 0xCC, 0xCC, 0x76, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'e' => [
            0x00, 0x00, 0x7C, 0xC6, 0xFE, 0xC0, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'f' => [
            0x1C, 0x36, 0x30, 0xFC, 0x30, 0x30, 0x30, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'g' => [
            0x00, 0x00, 0x76, 0xCC, 0xCC, 0x7C, 0x0C, 0xCC, 0x78, 0x00, 0x00, 0x00, 0x00,
        ],
        'h' => [
            0xC0, 0xC0, 0xDC, 0xE6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'i' => [
            0x18, 0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'j' => [
            0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x0C, 0x0C, 0xCC, 0x78, 0x00, 0x00, 0x00, 0x00,
        ],
        'k' => [
            0xC0, 0xC0, 0xCC, 0xD8, 0xF0, 0xD8, 0xCC, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'l' => [
            0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'm' => [
            0x00, 0x00, 0xEC, 0xFE, 0xD6, 0xD6, 0xD6, 0xD6, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'n' => [
            0x00, 0x00, 0xDC, 0xE6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'o' => [
            0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'p' => [
            0x00, 0x00, 0xDC, 0xE6, 0xC6, 0xE6, 0xDC, 0xC0, 0xC0, 0x00, 0x00, 0x00, 0x00,
        ],
        'q' => [
            0x00, 0x00, 0x76, 0xCC, 0xCC, 0xCC, 0x7C, 0x0C, 0x0C, 0x00, 0x00, 0x00, 0x00,
        ],
        'r' => [
            0x00, 0x00, 0xDC, 0xE6, 0xC0, 0xC0, 0xC0, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        's' => [
            0x00, 0x00, 0x7C, 0xC6, 0x70, 0x1C, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        't' => [
            0x00, 0x30, 0xFC, 0x30, 0x30, 0x30, 0x36, 0x1C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'u' => [
            0x00, 0x00, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0x76, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'v' => [
            0x00, 0x00, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'w' => [
            0x00, 0x00, 0xC6, 0xC6, 0xD6, 0xD6, 0xFE, 0x6C, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'x' => [
            0x00, 0x00, 0xC6, 0x6C, 0x38, 0x38, 0x6C, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        'y' => [
            0x00, 0x00, 0xC6, 0xC6, 0xC6, 0x7E, 0x06, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00,
        ],
        'z' => [
            0x00, 0x00, 0xFE, 0x0C, 0x18, 0x30, 0x60, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '{' => [
            0x0E, 0x18, 0x18, 0x70, 0x18, 0x18, 0x18, 0x0E, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '|' => [
            0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00,
        ],
        '}' => [
            0x70, 0x18, 0x18, 0x0E, 0x18, 0x18, 0x18, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        '~' => [
            0x76, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        _ => NOTDEF_ROWS,
    }
    // clang-format on
}

impl Default for FontAtlas {
    fn default() -> Self {
        Self::built_in()
    }
}

impl std::fmt::Debug for FontAtlas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontAtlas")
            .field("glyph_count", &self.len())
            .field("texture_size", &self.texture_size)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_atlas_has_ascii_plus_notdef() {
        let atlas = FontAtlas::built_in();
        assert_eq!(atlas.len(), GLYPH_COUNT);
        assert_eq!(GLYPH_COUNT, ASCII_GLYPH_COUNT + 1);
    }

    #[test]
    fn space_glyph_has_zero_width() {
        let atlas = FontAtlas::built_in();
        let g = atlas.glyph(' ');
        assert_eq!(g.width, 0);
        assert!((g.advance - GLYPH_ADVANCE).abs() < 0.001);
    }

    #[test]
    fn letter_a_has_positive_width() {
        let atlas = FontAtlas::built_in();
        let g = atlas.glyph('A');
        assert_eq!(g.width, GLYPH_WIDTH);
        assert_eq!(g.height, GLYPH_HEIGHT);
    }

    #[test]
    fn out_of_range_char_returns_notdef() {
        let atlas = FontAtlas::built_in();
        // Control characters, and anything past ASCII 126, get the box.
        for c in ['\0', '\u{7f}', 'é', '€', 'Ł', 'ġ'] {
            assert_eq!(glyph_index(c), NOTDEF_INDEX, "{c:?} should be .notdef");
            let g = atlas.glyph(c);
            assert_eq!(g.width, GLYPH_WIDTH, "{c:?} should draw the box");
        }
    }

    /// The bug: `(c as u8)` kept only the low byte, so U+0141 rendered `'A'`
    /// (U+0041) and U+0121 rendered `'!'` (U+0021).
    #[test]
    fn non_ascii_does_not_alias_onto_an_ascii_glyph() {
        let atlas = FontAtlas::built_in();
        assert_ne!(atlas.glyph_u_min('Ł'), atlas.glyph_u_min('A'));
        assert_ne!(atlas.glyph_u_min('ġ'), atlas.glyph_u_min('!'));
    }

    /// Out-of-range codepoints used to produce `u_min > 1.0`, which samples
    /// outside the atlas.
    #[test]
    fn glyph_uvs_stay_inside_the_atlas_for_any_codepoint() {
        let atlas = FontAtlas::built_in();
        for c in ['\0', 'é', '€', 'Ł', '\u{10ffff}', 'A', ' ', '~'] {
            let (u_min, u_max) = (atlas.glyph_u_min(c), atlas.glyph_u_max(c));
            assert!(
                (0.0..=1.0).contains(&u_min) && (0.0..=1.0).contains(&u_max) && u_min < u_max,
                "{c:?}: u range {u_min}..{u_max} escapes the atlas"
            );
        }
    }

    #[test]
    fn text_width_single_char() {
        let atlas = FontAtlas::built_in();
        let w = atlas.text_width("A", 1.0);
        assert!((w - GLYPH_ADVANCE).abs() < 0.001);
    }

    #[test]
    fn text_width_multiple_chars() {
        let atlas = FontAtlas::built_in();
        let w = atlas.text_width("ABC", 1.0);
        assert!((w - GLYPH_ADVANCE * 3.0).abs() < 0.001);
    }

    #[test]
    fn text_width_scales_linearly_with_the_scale_it_is_given() {
        let atlas = FontAtlas::built_in();
        let w1 = atlas.text_width("Hi", 1.0);
        let w2 = atlas.text_width("Hi", 2.0);
        assert!((w2 - w1 * 2.0).abs() < 0.001);
    }

    #[test]
    fn layout_line_produces_glyphs() {
        let atlas = FontAtlas::built_in();
        let glyphs = atlas.layout_line("AB", Vec2::ZERO, 1.0);
        assert_eq!(glyphs.len(), 2);
        // 'A' should start after bearing_x.
        assert!((glyphs[0].1.x - 1.0).abs() < 0.001);
        // The horizontal advance between glyphs should be GLYPH_ADVANCE.
        assert!(
            (glyphs[1].1.x - glyphs[0].1.x - GLYPH_ADVANCE).abs() < 0.001,
            "expected horizontal advance {}, got {}",
            GLYPH_ADVANCE,
            glyphs[1].1.x - glyphs[0].1.x,
        );
    }

    /// The bug: `min`/`max` were multiplied by `scale` on top of a `cursor`
    /// that already carried `pos` plus a scaled advance, so the position came
    /// out as `pos.x * scale + i * advance * scale²` — the anchor moved with
    /// the font size and the advance scaled twice.
    #[test]
    fn layout_line_scales_extents_but_not_the_anchor() {
        let atlas = FontAtlas::built_in();
        let glyphs = atlas.layout_line("AB", Vec2::new(100.0, 200.0), 2.0);
        assert_eq!(glyphs.len(), 2);

        // First glyph: x = 100 + bearing_x(1) * 2, y = 200 (top of the em box,
        // since bearing_y == ASCENDER for every glyph in this font).
        assert!((glyphs[0].1.x - 102.0).abs() < 0.001, "{:?}", glyphs[0].1);
        assert!((glyphs[0].1.y - 200.0).abs() < 0.001, "{:?}", glyphs[0].1);
        // Extents scale exactly once: 8×13 at 2.0 → 16×26.
        assert!((glyphs[0].2.x - glyphs[0].1.x - 16.0).abs() < 0.001);
        assert!((glyphs[0].2.y - glyphs[0].1.y - 26.0).abs() < 0.001);
        // Advance scales exactly once: 10 → 20, not 40.
        assert!(
            (glyphs[1].1.x - glyphs[0].1.x - GLYPH_ADVANCE * 2.0).abs() < 0.001,
            "advance scaled twice: {}",
            glyphs[1].1.x - glyphs[0].1.x
        );
    }

    /// `pos` is the top-left anchor of the em box, not a baseline: a glyph must
    /// never be laid out above the position it was asked for.
    #[test]
    fn layout_line_anchor_is_the_top_left_not_the_baseline() {
        let atlas = FontAtlas::built_in();
        let pos = Vec2::new(10.0, 100.0);
        for scale in [0.5, 1.0, 2.0] {
            let glyphs = atlas.layout_line("Hi", pos, scale);
            for (c, min, max) in glyphs {
                assert!(min.y >= pos.y - 0.001, "{c:?} at scale {scale}: {min:?}");
                assert!(
                    max.y <= pos.y + LINE_HEIGHT * scale + 0.001,
                    "{c:?} at scale {scale}: {max:?}"
                );
            }
        }
    }

    #[test]
    fn layout_line_handles_newline() {
        let atlas = FontAtlas::built_in();
        let glyphs = atlas.layout_line("A\nB", Vec2::ZERO, 1.0);
        assert_eq!(glyphs.len(), 2);
        // The vertical advance between lines should be LINE_HEIGHT.
        assert!(
            (glyphs[1].1.y - glyphs[0].1.y - LINE_HEIGHT).abs() < 0.001,
            "expected vertical advance {}, got {}",
            LINE_HEIGHT,
            glyphs[1].1.y - glyphs[0].1.y,
        );
    }

    #[test]
    fn empty_string_layout_is_empty() {
        let atlas = FontAtlas::built_in();
        let glyphs = atlas.layout_line("", Vec2::ZERO, 1.0);
        assert!(glyphs.is_empty());
    }

    #[test]
    fn text_width_of_multiline_is_the_widest_line() {
        let atlas = FontAtlas::built_in();
        // Not 5 * advance: the two lines are measured separately.
        let w = atlas.text_width("AB\nCDE", 1.0);
        assert!(
            (w - GLYPH_ADVANCE * 3.0).abs() < 0.001,
            "expected the widest line ({}), got {w}",
            GLYPH_ADVANCE * 3.0
        );
        assert_eq!(atlas.line_count("AB\nCDE"), 2);
        assert_eq!(atlas.line_count("one line"), 1);
    }

    #[test]
    fn the_font_atlas_debug_output_reports_its_glyph_count() {
        let atlas = FontAtlas::built_in();
        let s = format!("{atlas:?}");
        assert!(s.contains(&format!("glyph_count: {GLYPH_COUNT}")));
    }

    #[test]
    fn glyph_u_min_for_first_char_is_zero() {
        let atlas = FontAtlas::built_in();
        // ' ' is the first glyph, at column 0.
        let u = atlas.glyph_u_min(' ');
        assert!((u - 0.0).abs() < 0.001);
    }

    #[test]
    fn glyph_u_max_for_the_notdef_column_is_one() {
        let atlas = FontAtlas::built_in();
        // `.notdef` is the last column, so any unmapped codepoint ends the atlas.
        let u = atlas.glyph_u_max('€');
        assert!((u - 1.0).abs() < 0.001, "got {u}");
        // '~' is the last *ASCII* glyph, one column before it.
        let u = atlas.glyph_u_max('~');
        assert!(
            (u - ASCII_GLYPH_COUNT as f32 / GLYPH_COUNT as f32).abs() < 0.001,
            "got {u}"
        );
    }

    #[test]
    fn glyph_u_for_letter_a_is_contiguous() {
        let atlas = FontAtlas::built_in();
        let u_max = atlas.glyph_u_max('A'); // column 33
        let u_min_b = atlas.glyph_u_min('B'); // column 34
        assert!((u_max - u_min_b).abs() < 0.001);
    }
}
