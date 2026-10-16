//! The colour-emoji test fonts under `crates/crcbl-ui/tests/fixtures/emoji`,
//! parsed once each.
//!
//! Each is a few glyphs on a 16-unit em, 12 units up and 4 down, every glyph
//! 16 units wide, so at 16 pixels per em a unit is a pixel and a test can
//! work out every pixel by hand:
//!
//! * `text.ttf` — an outline heart at U+2764 and nothing in colour.
//! * `cbdt.ttf` — CBDT/CBLC strikes at 16 and 8 pixels per em: U+1F600 in
//!   both (red and blue halves at 16, green at 8), U+2764 only at 16
//!   (magenta), and U+1F4A5 at 16 with a PNG that does not decode.
//! * `sbix.ttf` — sbix strikes at 16 (yellow) and 8 (cyan) for U+1F600, each
//!   with its origin a quarter of the strike below the baseline.
//! * `colrv0.ttf` — COLR v0 and CPAL: U+1F600 is three overlapping squares in
//!   red, blue and half-transparent green, and U+1F601 three layers of one.
//! * `colrv1.ttf` — COLR v1 paint graphs from U+1F7E5: a solid fill, linear,
//!   radial and sweep gradients, a translation, a composite, a nested clip,
//!   and forty nested transforms.
//!
//! They were built by this script, run as `python -I make_fixtures.py
//! crates/crcbl-ui/tests/fixtures/emoji`:
//!
//! ```text
//! """crcbl-ui's colour-emoji test fonts: python -I make_fixtures.py <out dir>.
//!
//! fontTools 4.67 and Pillow 12.3. Every font is 16 units to the em, 12 up and
//! 4 down, and every glyph 16 units wide.
//! """
//!
//! import io
//! import sys
//!
//! from fontTools.colorLib.builder import buildCOLR, buildCPAL
//! from fontTools.fontBuilder import FontBuilder
//! from fontTools.pens.ttGlyphPen import TTGlyphPen
//! from fontTools.ttLib import newTable
//! from fontTools.ttLib.tables import otTables as ot
//! from fontTools.ttLib.tables.BitmapGlyphMetrics import SmallGlyphMetrics
//! from fontTools.ttLib.tables.C_B_D_T_ import cbdt_bitmap_format_17
//! from fontTools.ttLib.tables.E_B_L_C_ import (
//!     BitmapSizeTable, SbitLineMetrics, Strike, eblc_index_sub_table_1)
//! from fontTools.ttLib.tables.sbixGlyph import Glyph as SbixGlyph
//! from fontTools.ttLib.tables.sbixStrike import Strike as SbixStrike
//! from PIL import Image
//!
//! UPEM, ASCENT, DESCENT, ADVANCE = 16, 12, -4, 16
//! LINE_FIELDS = ("caretSlopeNumerator", "caretSlopeDenominator", "caretOffset",
//!                "minOriginSB", "minAdvanceSB", "maxBeforeBL", "minAfterBL",
//!                "pad1", "pad2")
//!
//!
//! def square(x0, y0, x1, y1):
//!     pen = TTGlyphPen(None)
//!     pen.moveTo((x0, y0))
//!     pen.lineTo((x0, y1))
//!     pen.lineTo((x1, y1))
//!     pen.lineTo((x1, y0))
//!     pen.closePath()
//!     return pen.glyph()
//!
//!
//! def empty():
//!     return TTGlyphPen(None).glyph()
//!
//!
//! def build(name, glyphs, cmap):
//!     """A font of `glyphs` (name: outline) mapped by `cmap`."""
//!     order = [".notdef"] + list(glyphs)
//!     fb = FontBuilder(UPEM, isTTF=True)
//!     fb.setupGlyphOrder(order)
//!     fb.setupCharacterMap(cmap)
//!     fb.setupGlyf({g: glyphs.get(g, empty()) for g in order})
//!     glyf = fb.font["glyf"]
//!     for g in order:
//!         glyf[g].recalcBounds(glyf)
//!     fb.setupHorizontalMetrics(
//!         {g: (ADVANCE, getattr(glyf[g], "xMin", 0)) for g in order})
//!     fb.setupHorizontalHeader(ascent=ASCENT, descent=DESCENT)
//!     fb.setupNameTable({"familyName": name, "styleName": "Regular"})
//!     fb.setupOS2(sTypoAscender=ASCENT, sTypoDescender=DESCENT,
//!                 usWinAscent=ASCENT, usWinDescent=-DESCENT)
//!     fb.setupPost()
//!     return fb.font
//!
//!
//! def png(size, fill):
//!     """A `size`-square PNG, `fill(x, y)` each pixel's RGBA."""
//!     image = Image.new("RGBA", (size, size))
//!     for y in range(size):
//!         for x in range(size):
//!             image.putpixel((x, y), fill(x, y))
//!     out = io.BytesIO()
//!     image.save(out, "PNG")
//!     return out.getvalue()
//!
//!
//! RED, BLUE = (255, 0, 0, 255), (0, 0, 255, 255)
//! SMILE16 = png(16, lambda x, y: RED if x < 8 else BLUE)
//! SMILE8 = png(8, lambda x, y: (0, 255, 0, 255))
//! HEART16 = png(16, lambda x, y: (255, 0, 255, 255))
//! # A PNG signature and the IHDR size an sbix reader peeks at, then garbage.
//! BROKEN = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR" + b"\0\0\0\x10" * 2 + b"not a png"
//! YELLOW16 = png(16, lambda x, y: (255, 255, 0, 255))
//! CYAN8 = png(8, lambda x, y: (0, 255, 255, 255))
//!
//!
//! def text_font():
//!     return build("Text", {"space": empty(), "heart": square(2, 0, 14, 12)},
//!                  {0x20: "space", 0x2764: "heart"})
//!
//!
//! def cbdt_strike(font, ppem, images):
//!     """One CBLC strike of `images` (glyph: PNG) and its CBDT glyphs."""
//!     data = {}
//!     for glyph, image in images.items():
//!         bitmap = cbdt_bitmap_format_17(None, None)
//!         metrics = bitmap.metrics = SmallGlyphMetrics()
//!         metrics.width = metrics.height = metrics.Advance = ppem
//!         metrics.BearingX, metrics.BearingY = 0, ppem * ASCENT // UPEM
//!         bitmap.imageData = image
//!         data[glyph] = bitmap
//!     strike = Strike()
//!     size = strike.bitmapSizeTable = BitmapSizeTable()
//!     for direction in ("hori", "vert"):
//!         line = SbitLineMetrics()
//!         line.ascender = ppem * ASCENT // UPEM
//!         line.descender = ppem * DESCENT // UPEM
//!         line.widthMax = ppem
//!         for field in LINE_FIELDS:
//!             setattr(line, field, 0)
//!         setattr(size, direction, line)
//!     size.colorRef, size.bitDepth, size.flags = 0, 32, 1
//!     size.ppemX = size.ppemY = ppem
//!     ids = sorted(font.getGlyphID(g) for g in images)
//!     if ids != list(range(ids[0], ids[-1] + 1)):
//!         raise ValueError("a strike's glyphs must be consecutive")
//!     size.startGlyphIndex, size.endGlyphIndex = ids[0], ids[-1]
//!     index = eblc_index_sub_table_1(None, None)
//!     index.indexFormat, index.imageFormat = 1, 17
//!     index.firstGlyphIndex, index.lastGlyphIndex = ids[0], ids[-1]
//!     index.names = [font.getGlyphName(g) for g in ids]
//!     strike.indexSubTables = [index]
//!     return strike, data
//!
//!
//! def cbdt_font():
//!     font = build("Cbdt", {"smile": empty(), "heart": empty(), "broken": empty()},
//!                  {0x1F600: "smile", 0x2764: "heart", 0x1F4A5: "broken"})
//!     cblc, cbdt = newTable("CBLC"), newTable("CBDT")
//!     cblc.version = cbdt.version = 3.0
//!     cblc.strikes, cbdt.strikeData = [], []
//!     strikes = {16: {"smile": SMILE16, "heart": HEART16, "broken": BROKEN},
//!                8: {"smile": SMILE8}}
//!     for ppem, images in strikes.items():
//!         strike, data = cbdt_strike(font, ppem, images)
//!         cblc.strikes.append(strike)
//!         cbdt.strikeData.append(data)
//!     font["CBDT"], font["CBLC"] = cbdt, cblc
//!     return font
//!
//!
//! def sbix_font():
//!     font = build("Sbix", {"smile": empty()}, {0x1F600: "smile"})
//!     sbix = newTable("sbix")
//!     sbix.version, sbix.flags = 1, 1
//!     for ppem, image in ((16, YELLOW16), (8, CYAN8)):
//!         strike = SbixStrike(ppem=ppem, resolution=72)
//!         for glyph in font.getGlyphOrder():
//!             strike.glyphs[glyph] = SbixGlyph(glyphName=glyph)
//!         strike.glyphs["smile"] = SbixGlyph(
//!             glyphName="smile", graphicType="png ", imageData=image,
//!             originOffsetX=0, originOffsetY=ppem * DESCENT // UPEM)
//!         sbix.strikes[ppem] = strike
//!     font["sbix"] = sbix
//!     return font
//!
//!
//! def colrv0_font():
//!     glyphs = {"smile": square(0, 0, 12, 12), "many": square(0, 0, 12, 12),
//!               "la": square(0, 0, 8, 8), "lb": square(4, 4, 12, 12),
//!               "lc": square(2, 2, 6, 6)}
//!     font = build("Colr0", glyphs, {0x1F600: "smile", 0x1F601: "many"})
//!     layers = {"smile": [("la", 0), ("lb", 1), ("lc", 2)], "many": [("la", 0)] * 3}
//!     font["COLR"] = buildCOLR(layers, version=0, glyphMap=font.getReverseGlyphMap())
//!     font["CPAL"] = buildCPAL([[(1, 0, 0, 1), (0, 0, 1, 1), (0, 1, 0, 128 / 255)]])
//!     return font
//!
//!
//! def solid(index):
//!     return {"Format": ot.PaintFormat.PaintSolid, "PaletteIndex": index, "Alpha": 1.0}
//!
//!
//! def glyph(name, paint):
//!     return {"Format": ot.PaintFormat.PaintGlyph, "Paint": paint, "Glyph": name}
//!
//!
//! def red_to_blue():
//!     stops = [{"StopOffset": o, "PaletteIndex": o, "Alpha": 1.0} for o in (0, 1)]
//!     return {"Extend": "pad", "ColorStop": stops}
//!
//!
//! def colrv1_font():
//!     names = ["solid", "linear", "radial", "sweep", "moved", "composite",
//!              "clipped", "deep"]
//!     glyphs = {"sq": square(0, 0, 12, 12), "la": square(0, 0, 8, 8),
//!               "lb": square(4, 4, 12, 12), "dot": square(0, 0, 4, 4)}
//!     glyphs.update({name: empty() for name in names})
//!     font = build("Colr1", glyphs, {0x1F7E5 + i: n for i, n in enumerate(names)})
//!     Format = ot.PaintFormat
//!     deep = glyph("sq", solid(0))
//!     for _ in range(40):
//!         deep = {"Format": Format.PaintTranslate, "Paint": deep, "dx": 0, "dy": 0}
//!     paints = {
//!         "solid": glyph("sq", solid(0)),
//!         "linear": glyph("sq", {
//!             "Format": Format.PaintLinearGradient, "ColorLine": red_to_blue(),
//!             "x0": 0, "y0": 0, "x1": 12, "y1": 0, "x2": 0, "y2": 12}),
//!         "radial": glyph("sq", {
//!             "Format": Format.PaintRadialGradient, "ColorLine": red_to_blue(),
//!             "x0": 6, "y0": 6, "r0": 0, "x1": 6, "y1": 6, "r1": 6}),
//!         "sweep": glyph("sq", {
//!             "Format": Format.PaintSweepGradient, "ColorLine": red_to_blue(),
//!             "centerX": 6, "centerY": 6, "startAngle": 0, "endAngle": 90}),
//!         "moved": {"Format": Format.PaintTranslate, "Paint": glyph("dot", solid(1)),
//!                   "dx": 8, "dy": 4},
//!         "composite": {"Format": Format.PaintComposite,
//!                       "SourcePaint": glyph("lb", solid(1)),
//!                       "CompositeMode": "src_in",
//!                       "BackdropPaint": glyph("la", solid(0))},
//!         "clipped": glyph("la", glyph("lb", solid(2))),
//!         "deep": deep,
//!     }
//!     font["COLR"] = buildCOLR(paints, version=1, glyphMap=font.getReverseGlyphMap())
//!     font["CPAL"] = buildCPAL([[(1, 0, 0, 1), (0, 0, 1, 1), (0, 1, 0, 1)]])
//!     return font
//!
//!
//! for name, make in (("text", text_font), ("cbdt", cbdt_font), ("sbix", sbix_font),
//!                    ("colrv0", colrv0_font), ("colrv1", colrv1_font)):
//!     make().save(f"{sys.argv[1]}/{name}.ttf")
//! ```

use std::sync::OnceLock;

use super::Font;

/// Parses `bytes` the first time `cell` is asked, and clones it after.
fn parsed(cell: &'static OnceLock<Font>, bytes: &'static [u8]) -> Font {
    cell.get_or_init(|| Font::parse(bytes).expect("the fixture parses"))
        .clone()
}

/// `text.ttf`.
pub(crate) fn text() -> Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    parsed(&FONT, include_bytes!("../../tests/fixtures/emoji/text.ttf"))
}

/// `cbdt.ttf`.
pub(crate) fn cbdt() -> Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    parsed(&FONT, include_bytes!("../../tests/fixtures/emoji/cbdt.ttf"))
}

/// `sbix.ttf`.
pub(crate) fn sbix() -> Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    parsed(&FONT, include_bytes!("../../tests/fixtures/emoji/sbix.ttf"))
}

/// `colrv0.ttf`.
pub(crate) fn colrv0() -> Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    parsed(
        &FONT,
        include_bytes!("../../tests/fixtures/emoji/colrv0.ttf"),
    )
}
