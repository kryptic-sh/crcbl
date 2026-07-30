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
    #[must_use]
    pub fn rect(&self, cursor: Vec2) -> (Vec2, Vec2) {
        let min = Vec2::new(
            cursor.x + self.bearing_x as f32,
            cursor.y - self.bearing_y as f32,
        );
        let max = Vec2::new(min.x + self.width as f32, min.y + self.height as f32);
        (min, max)
    }
}

// ---------------------------------------------------------------------------
// FontAtlas
// ---------------------------------------------------------------------------

/// A simple monospace bitmap font atlas.
///
/// The built-in engine font is a fixed-pitch 8×13 px bitmap covering ASCII
/// 32–126. Glyphs are arranged in a single row in the atlas texture.
///
/// # Atlas layout
///
/// Each glyph is `GLYPH_WIDTH × GLYPH_HEIGHT` pixels. They are packed
/// left-to-right, one row, in ASCII order. The total atlas width is
/// `GLYPH_WIDTH * COUNT` pixels.
pub struct FontAtlas {
    /// Glyph metrics for each character, indexed by (codepoint - FIRST_CHAR).
    metrics: Vec<GlyphMetrics>,
    /// Atlas texture dimensions in pixels (width, height).
    pub texture_size: (u32, u32),
}

/// Number of glyphs in the built-in atlas (ASCII 32–126 inclusive).
pub const GLYPH_COUNT: usize = 95;

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

impl FontAtlas {
    /// Create the built-in monospace font atlas.
    #[must_use]
    pub fn built_in() -> Self {
        let metrics: Vec<GlyphMetrics> = (FIRST_CHAR..=LAST_CHAR)
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

        let texture_size = (GLYPH_WIDTH * GLYPH_COUNT as u32, GLYPH_HEIGHT);
        Self {
            metrics,
            texture_size,
        }
    }

    /// Look up the metrics for a character.
    ///
    /// Returns the metrics for `'\0'` (empty glyph) for out-of-range or
    /// non-printable characters.
    #[must_use]
    pub fn glyph(&self, c: char) -> &GlyphMetrics {
        let idx = (c as u8).wrapping_sub(FIRST_CHAR) as usize;
        self.metrics.get(idx).unwrap_or(&self.metrics[0])
    }

    /// Measure the width of a string in pixels at the given scale.
    ///
    /// `scale` is a multiplier relative to the baked-in glyph size (1.0 =
    /// natural pixel size).
    #[must_use]
    pub fn text_width(&self, text: &str, scale: f32) -> f32 {
        let mut w = 0.0;
        for c in text.chars() {
            if c == '\n' {
                continue;
            }
            let g = self.glyph(c);
            w += g.advance * scale;
        }
        w
    }

    /// Layout a single line of text into screen-space glyph positions.
    ///
    /// Returns a list of `(char, min, max)` quads where `min`/`max` are the
    /// screen-space corners of each glyph.
    pub fn layout_line(&self, text: &str, pos: Vec2, scale: f32) -> Vec<(char, Vec2, Vec2)> {
        let mut out = Vec::with_capacity(text.len());
        let mut cursor = pos;
        for c in text.chars() {
            if c == '\n' {
                cursor.x = pos.x;
                cursor.y += LINE_HEIGHT * scale;
                continue;
            }
            let g = self.glyph(c);
            if g.width > 0 {
                let (min, max) = g.rect(cursor);
                let min = Vec2::new(min.x * scale, min.y * scale);
                let max = Vec2::new(max.x * scale, max.y * scale);
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
    #[must_use]
    pub fn glyph_u_min(&self, c: char) -> f32 {
        let atlas_w = self.texture_size.0 as f32;
        if atlas_w == 0.0 {
            return 0.0;
        }
        let col = (c as u8).wrapping_sub(FIRST_CHAR) as u32;
        (col * GLYPH_WIDTH) as f32 / atlas_w
    }

    /// UV u-coordinate of the right edge of the given glyph in the atlas
    /// texture (range [0, 1]).
    #[must_use]
    pub fn glyph_u_max(&self, c: char) -> f32 {
        let atlas_w = self.texture_size.0 as f32;
        if atlas_w == 0.0 {
            return 0.0;
        }
        let col = (c as u8).wrapping_sub(FIRST_CHAR) as u32;
        ((col + 1) * GLYPH_WIDTH) as f32 / atlas_w
    }
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
    fn built_in_atlas_has_95_glyphs() {
        let atlas = FontAtlas::built_in();
        assert_eq!(atlas.len(), GLYPH_COUNT);
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
    fn out_of_range_char_returns_space_glyph() {
        let atlas = FontAtlas::built_in();
        let g = atlas.glyph('\0');
        assert_eq!(g.width, 0);
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
    fn text_width_scales() {
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
    fn debug_format() {
        let atlas = FontAtlas::built_in();
        let s = format!("{atlas:?}");
        assert!(s.contains("glyph_count: 95"));
    }

    #[test]
    fn glyph_u_min_for_first_char_is_zero() {
        let atlas = FontAtlas::built_in();
        // ' ' is the first glyph, at column 0.
        let u = atlas.glyph_u_min(' ');
        assert!((u - 0.0).abs() < 0.001);
    }

    #[test]
    fn glyph_u_max_for_last_char_is_one() {
        let atlas = FontAtlas::built_in();
        // '~' is the last glyph (codepoint 126 = FIRST_CHAR + 94).
        let u = atlas.glyph_u_max('~');
        assert!((u - 1.0).abs() < 0.001);
    }

    #[test]
    fn glyph_u_for_letter_a_is_contiguous() {
        let atlas = FontAtlas::built_in();
        let u_max = atlas.glyph_u_max('A'); // column 33
        let u_min_b = atlas.glyph_u_min('B'); // column 34
        assert!((u_max - u_min_b).abs() < 0.001);
    }
}
