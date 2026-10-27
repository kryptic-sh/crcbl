//! Real fonts: parsing, the coverage rasteriser, the glyph atlas and text
//! layout — UI rung 5.
//!
//! ```text
//!  TTF bytes ──skrifa──▶ Font (cmap, advances, metrics, harfrust's tables)
//!                          │
//!   text ──shape──▶ glyphs ──layout──▶ TextLayout (wrapped glyph positions)
//!                          │
//!                          ▼
//!   DrawList::glyphs ──to_triangles_split──▶ GlyphAtlas::glyph
//!                                             │  hinted outline (skrifa)
//!                                             │  ──raster──▶ coverage mask
//!                                             ▼
//!                                   shelf-packed page, dirty rectangle
//! ```
//!
//! * [`Font`] — one parsed font: glyph ids through the cmap, horizontal
//!   advances and line metrics, read through `skrifa` and its `read-fonts`
//!   (`skrifa::raw`).
//! * [`raster`] — this engine's own coverage rasteriser.
//! * [`atlas`] — the glyph atlas: shelf-packed pages, LRU eviction and a
//!   per-frame rasterisation budget.
//! * [`cluster`] — the runs of chars that fall back, break and take the
//!   caret as one: UAX #29's grapheme clusters.
//! * `breaks` — where a line must end and where it may: UAX #14.
//! * `bidi` — each char's direction and a line's visual order: UAX #9.
//! * [`shape`] — clusters to positioned glyphs through `harfrust`: GSUB and
//!   GPOS, kerning and marks, ligatures and emoji sequences.
//! * [`layout`] — shaped words placed and greedily broken into lines.
//! * [`face`] — choosing a family's face by weight and style, and the bold and
//!   italic synthesised for a family without them.
//! * [`fallback`] — which of a font's fallback faces draws each cluster, and
//!   the emoji and text presentation selectors.
//! * [`color`] — colour glyphs, drawn as pictures in the image atlas.
//! * `system` — faces looked up on the system by family name, through
//!   `crcbl-font`, behind the `system-fonts` feature.
//!
//! # Two fonts, two families
//!
//! [`FontFamily::Bitmap`] is [`crate::text::FontAtlas`], the built-in 8×13
//! pixel font every panel, menu and console still draws with.
//! `FontFamily::Sans` is `Font::sans`: Atkinson Hyperlegible Regular, by
//! the Braille Institute of America, under the SIL Open Font License 1.1 —
//! `crates/crcbl-ui/fonts/OFL.txt` is its licence, committed beside
//! `crates/crcbl-ui/fonts/AtkinsonHyperlegible-Regular.ttf` and embedded into
//! every binary that links this crate **with the `parsed-font` feature on**.
//!
//! # The `parsed-font` feature
//!
//! Everything here that reads a font's bytes — `skrifa`, `harfrust`, `SANS_TTF`,
//! `Font::sans`, [`Font::parse`]'s tables and the atlas's outlines — is behind
//! the crate's `parsed-font` feature, off by default, because a browser build
//! that links it pays for it whether or not it draws a parsed glyph. Off, the
//! parsed font cannot exist: [`Font`] holds a field of an uninhabited type, so
//! [`layout`], [`atlas`], [`DrawList::glyphs`](crate::draw_list::DrawList::glyphs)
//! and [`Ui::register_font`](crate::tree::Ui::register_font) still compile —
//! nothing that names a `Font` needs a `cfg` of its own — and can never be
//! called, which the compiler says rather than a comment.
//! [`Font::parse`] refuses with [`FontError::ParsedFontOff`], and
//! `FontFamily::Sans` is not there to name, so a stylesheet naming it is
//! refused as a value `font-family` does not take.
//!
//! # Fonts an application registers
//!
//! Beside the two, a tree draws in any [`Font`] an application registers with
//! [`Ui::register_font`](crate::tree::Ui::register_font) under a
//! [`FamilyName`]: a `font-family` list naming it before its first built-in
//! family selects it, and a span in it is measured and emitted exactly as a
//! `FontFamily::Sans` span is — [`layout::TextLayout`] and
//! [`DrawList::glyphs`](crate::draw_list::DrawList::glyphs) — so its glyphs
//! reach the screen through the same [`atlas`] as the committed font's.
//!
//! A font need not be compiled in: [`Font::parse`] takes any [`FontData`],
//! so bytes an application read at run time — a system font it looked up, a
//! file a user picked — parse, register and draw like the committed font,
//! and are freed with the last clone of the [`Font`] rather than leaked.
//!
//! # Shaped, in both directions
//!
//! Text is shaped by [`shape`]: each cluster goes to the first face that maps
//! all of it — see [`fallback`] — a cluster no face has a glyph for draws
//! `.notdef`, and the font's GSUB and GPOS apply. A right-to-left run is
//! shaped right to left, and [`layout`] puts each line's runs in the visual
//! order UAX #9 gives them.

pub mod atlas;
pub(crate) mod bidi;
pub mod breaks;
pub mod cluster;
pub mod color;
pub mod face;
pub mod fallback;
#[cfg(all(test, feature = "parsed-font"))]
pub(crate) mod fixtures;
pub mod layout;
pub mod rich;
pub mod shape;
#[cfg(feature = "system-fonts")]
pub mod system;
pub use crcbl_image::raster;

use core::fmt;
#[cfg(feature = "parsed-font")]
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
#[cfg(feature = "parsed-font")]
use std::sync::{Mutex, PoisonError};

#[cfg(feature = "parsed-font")]
use skrifa::instance::{LocationRef, Size};
#[cfg(feature = "parsed-font")]
use skrifa::raw::TableProvider;
#[cfg(feature = "parsed-font")]
use skrifa::{FontRef, MetadataProvider};

use face::Synthesis;

/// The committed UI font's bytes: Atkinson Hyperlegible Regular. See the module
/// docs for its licence.
#[cfg(feature = "parsed-font")]
pub const SANS_TTF: &[u8] = include_bytes!("../../fonts/AtkinsonHyperlegible-Regular.ttf");

/// Which font a text node draws in: `font-family` in a stylesheet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontFamily {
    /// [`crate::text::FontAtlas`]'s built-in 8×13 bitmap font — `bitmap` in a
    /// stylesheet, and the initial value, so every panel that names no family
    /// draws as it always has.
    #[default]
    Bitmap,
    /// [`Font::sans`] — `sans-serif` or `"Atkinson Hyperlegible"` in a
    /// stylesheet. Only with the `parsed-font` feature.
    #[cfg(feature = "parsed-font")]
    Sans,
}

impl FontFamily {
    /// The parsed font this family draws with, or `None` for the bitmap font.
    #[must_use]
    pub fn font(self) -> Option<&'static Font> {
        match self {
            Self::Bitmap => None,
            #[cfg(feature = "parsed-font")]
            Self::Sans => Some(Font::sans()),
        }
    }
}

/// A family name no built-in family answers to, hashed: what
/// [`Ui::register_font`](crate::tree::Ui::register_font) registers a font under
/// and a `font-family` list names it by.
///
/// **ASCII case is ignored, and nothing else is**: `Roboto`, `"roboto"` and
/// `ROBOTO` are one name. An unquoted name of several words is its words joined
/// by single spaces, so `Roboto  Mono` in a sheet is `"Roboto Mono"`, while a
/// quoted one is taken as written, spaces and all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FamilyName(u64);

impl FamilyName {
    /// The name `name`, in any ASCII case.
    #[must_use]
    pub fn new(name: &str) -> Self {
        let mut hasher = std::hash::DefaultHasher::new();
        std::hash::Hash::hash(&name.to_ascii_lowercase(), &mut hasher);
        Self(std::hash::Hasher::finish(&hasher))
    }
}

/// The names a `font-family` list never selects a registered font by, ASCII
/// case aside: the built-in families' own, CSS's generic families and its
/// CSS-wide keywords.
const RESERVED_FAMILIES: &[&str] = &[
    "bitmap",
    "atkinson hyperlegible",
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
    "fangsong",
    "initial",
    "inherit",
    "unset",
    "revert",
    "revert-layer",
    "default",
];

/// Whether `name` is one of [`RESERVED_FAMILIES`], or empty.
pub(crate) fn is_reserved_family(name: &str) -> bool {
    name.is_empty()
        || RESERVED_FAMILIES
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(name))
}

/// Why [`Ui::register_font`](crate::tree::Ui::register_font) refused a name:
/// it is empty, a built-in family's, a generic family or a CSS-wide keyword,
/// so no `font-family` list could ever select a font registered under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedFamilyName(pub String);

impl fmt::Display for ReservedFamilyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is reserved: no `font-family` list selects a registered font by it",
            self.0
        )
    }
}

impl std::error::Error for ReservedFamilyName {}

/// Names one parsed [`Font`] for as long as the process runs: what the glyph
/// atlas keys a glyph's font by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontId(u32);

/// A glyph's index in its font.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlyphId(pub u32);

impl GlyphId {
    /// `.notdef`, glyph zero in every font: what a codepoint with no glyph
    /// draws.
    pub const NOTDEF: Self = Self(0);
}

/// Why [`Font::parse`] refused some bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontError {
    /// `skrifa` could not read the font's table directory.
    Unreadable(String),
    /// The font's `head` table gives no units per em, so nothing in it can be
    /// scaled to a pixel size.
    NoUnitsPerEm,
    /// This build has no parsed fonts: the crate's `parsed-font` feature is
    /// off. What [`Font::parse`] returns in such a build, whatever the bytes.
    /// The variant is there with the feature on too, so turning the feature on
    /// from another crate never breaks a `match` written without it.
    ParsedFontOff,
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(why) => write!(f, "the font cannot be read: {why}"),
            Self::NoUnitsPerEm => f.write_str("the font's head table gives no units per em"),
            Self::ParsedFontOff => f.write_str(
                "crcbl-ui was built without its `parsed-font` feature, so it parses no fonts",
            ),
        }
    }
}

impl std::error::Error for FontError {}

/// Where a font draws an underline, in font units, as its `post` table says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Underline {
    /// The baseline to the line's top edge, positive upwards: negative for
    /// an underline below the baseline, as nearly every font's is.
    pub offset: f32,
    /// How thick it is.
    pub thickness: f32,
}

/// A font's line metrics, in font units: the
/// ascender, descender and line gap, as `skrifa` selects them from `hhea` or
/// `OS/2`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    /// Font units per em: what a pixel size is divided by.
    pub units_per_em: u16,
    /// Baseline to the top of the line's alignment box, positive upwards.
    pub ascent: f32,
    /// Baseline to its bottom, **negative** below the baseline, as the font
    /// stores it.
    pub descent: f32,
    /// Extra space the font recommends between one line and the next.
    pub line_gap: f32,
}

impl FontMetrics {
    /// Pixels per font unit at `size` pixels per em.
    #[must_use]
    pub fn scale(&self, size: f32) -> f32 {
        size / f32::from(self.units_per_em)
    }

    /// `line-height: normal` at `size`: ascent to descent plus the line gap.
    #[must_use]
    pub fn normal_line_height(&self, size: f32) -> f32 {
        (self.ascent - self.descent + self.line_gap) * self.scale(size)
    }
}

/// The first codepoint past Latin-1: everything below it is looked up in a
/// table built at parse time, everything from it on goes through the cmap.
const LATIN1_END: u32 = 0x100;

/// The bytes a [`Font`] is parsed from: compiled in, or found at run time.
///
/// Made with `From` from an `include_bytes!` (`&'static [u8; N]` or
/// `&'static [u8]`), or from a `Vec<u8>`, `Box<[u8]>` or `Arc<[u8]>` an
/// application read itself — a system font it looked up, a font file a user
/// chose. The font keeps them alive for as long as any clone of it lives, so
/// nothing has to be leaked to make them last.
#[derive(Clone)]
pub struct FontData(Bytes);

/// Where [`FontData`]'s bytes live. Compiled-in bytes are borrowed rather
/// than copied into a shared allocation.
#[derive(Clone)]
enum Bytes {
    Static(&'static [u8]),
    /// Shared with `harfrust`'s copy of the font, which takes them in this
    /// form, so the bytes a caller handed over are held once.
    Shared(Arc<dyn AsRef<[u8]> + Send + Sync>),
}

impl FontData {
    /// The bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match &self.0 {
            Bytes::Static(bytes) => bytes,
            Bytes::Shared(bytes) => (**bytes).as_ref(),
        }
    }
}

impl fmt::Debug for FontData {
    /// The length; not the bytes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontData")
            .field("len", &self.as_bytes().len())
            .finish()
    }
}

impl From<&'static [u8]> for FontData {
    fn from(bytes: &'static [u8]) -> Self {
        Self(Bytes::Static(bytes))
    }
}

impl<const N: usize> From<&'static [u8; N]> for FontData {
    fn from(bytes: &'static [u8; N]) -> Self {
        Self(Bytes::Static(bytes))
    }
}

impl From<Arc<[u8]>> for FontData {
    fn from(bytes: Arc<[u8]>) -> Self {
        Self(Bytes::Shared(Arc::new(bytes)))
    }
}

impl From<Vec<u8>> for FontData {
    fn from(bytes: Vec<u8>) -> Self {
        Self(Bytes::Shared(Arc::new(bytes)))
    }
}

impl From<Box<[u8]>> for FontData {
    fn from(bytes: Box<[u8]>) -> Self {
        Self(Bytes::Shared(Arc::new(bytes)))
    }
}

/// One parsed font. See the module docs.
///
/// Parsing reads the Latin-1 cmap and every advance up front, and readies
/// the font for `harfrust`, which reads the layout tables it shapes with
/// once per parse. Outlines are read when the atlas rasterises a glyph.
///
/// A handle: cloning it is a reference count, and every clone is the same
/// font. Two fonts are equal when they are one parse drawn with one
/// [`Synthesis`]: equality is [`Font::id`], and **the fallback faces a handle
/// carries ([`Font::with_fallback`]) are not part of it**.
///
/// Without the `parsed-font` feature there is no value of this type: see the
/// module docs.
#[derive(Clone)]
pub struct Font {
    source: Arc<Source>,
    /// What this handle draws of the face that it lacks; see [`face`].
    synthesis: Synthesis,
    /// The faces asked, in order, for a char this one has no glyph for; see
    /// [`fallback`]. None of them carries fallback faces of its own.
    fallback: Arc<[Font]>,
}

/// One face, shared by every clone of its [`Font`]: who it is from the start,
/// and what its parse read from the first time anything asks.
struct Source {
    /// Uninhabited, so a build without the feature can name a `Font` and never
    /// hold one: a body that reads the font's bytes matches on this instead.
    #[cfg(not(feature = "parsed-font"))]
    never: NoParsedFont,
    /// This face's identity under each [`Synthesis`], by
    /// [`Synthesis::index`]: a synthesised face's glyphs are not the face's.
    /// Given when the handle is made, so a `Font::lazy` face has one
    /// before it is read.
    ids: [FontId; SYNTHESES],
    /// Its parse: set when the handle is made by [`Font::parse`], and on
    /// first use for a `Font::lazy` one.
    parsed: OnceLock<Parsed>,
    /// What reads a [`Font::lazy`] face's bytes, taken by its first use;
    /// `None` for a face parsed when it was made.
    #[cfg(feature = "parsed-font")]
    load: Mutex<Option<Loader>>,
    /// What a [`Font::lazy`] face's warning calls it when it cannot be read:
    /// its file, for one [`Font::from_system_lazy`] made.
    #[cfg(feature = "parsed-font")]
    what: String,
}

/// What [`Font::lazy`] keeps to read its face: its bytes, or why not.
#[cfg(feature = "parsed-font")]
type Loader = Box<dyn FnOnce() -> Result<FontData, String> + Send>;

/// What one parse read.
struct Parsed {
    data: FontData,
    metrics: FontMetrics,
    /// Its `post` table's underline, if it has one.
    underline: Option<Underline>,
    /// Its weight class, 1 to 1000.
    weight: f32,
    /// Whether it is an italic or oblique face.
    italic: bool,
    /// Whether it has a `COLR`, `CBDT` or `sbix` table: whether any of its
    /// glyphs can be a colour glyph.
    #[cfg(feature = "parsed-font")]
    color: bool,
    /// Glyph ids of the codepoints below [`LATIN1_END`]; zero where the font
    /// has none.
    latin1: [u32; LATIN1_END as usize],
    /// Every glyph's advance, in font units.
    advances: Box<[f32]>,
    /// The font as `harfrust` shapes with it, its layout tables read once;
    /// `None` only for a test's fixed-pitch face, which has no bytes.
    #[cfg(feature = "parsed-font")]
    shaper: Option<harfrust::Font>,
}

impl fmt::Debug for Font {
    /// The id and the metrics; not the tables. A `Font::lazy` face not yet
    /// read says so rather than being read to be printed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("Font");
        debug.field("id", &self.id());
        match self.source.parsed.get() {
            Some(parsed) => debug
                .field("metrics", &parsed.metrics)
                .field("weight", &parsed.weight)
                .field("italic", &parsed.italic)
                .field("glyphs", &parsed.advances.len()),
            None => debug.field("read", &false),
        };
        debug
            .field("synthesis", &self.synthesis)
            .field("fallback", &self.fallback.len())
            .finish_non_exhaustive()
    }
}

impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for Font {}

/// A clone: what lets a caller holding a `&'static Font` pass it wherever a
/// `Font` is taken.
impl From<&Font> for Font {
    fn from(font: &Font) -> Self {
        font.clone()
    }
}

/// No value: what makes [`Font`] uninhabited without the `parsed-font`
/// feature.
#[cfg(not(feature = "parsed-font"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoParsedFont {}

/// How many [`Synthesis`] combinations there are, each an identity of its own.
const SYNTHESES: usize = 4;

/// The next parse's [`FontId`]s, one per [`Synthesis`].
#[cfg(feature = "parsed-font")]
fn next_ids() -> [FontId; SYNTHESES] {
    static NEXT_ID: AtomicU32 = AtomicU32::new(0);
    let first = NEXT_ID.fetch_add(SYNTHESES as u32, Ordering::Relaxed);
    core::array::from_fn(|at| FontId(first + at as u32))
}

impl Font {
    /// The committed UI font, parsed once per process.
    ///
    /// # Panics
    ///
    /// If [`SANS_TTF`] does not parse, which the crate's own tests rule out:
    /// the bytes are compiled in, so they are the same bytes the tests read.
    #[cfg(feature = "parsed-font")]
    #[must_use]
    pub fn sans() -> &'static Self {
        static SANS: OnceLock<Font> = OnceLock::new();
        SANS.get_or_init(|| Self::parse(SANS_TTF).expect("the committed UI font parses"))
    }

    /// Refuses `data`: this build has no parsed fonts. See the module docs.
    ///
    /// # Errors
    ///
    /// [`FontError::ParsedFontOff`], always.
    #[cfg(not(feature = "parsed-font"))]
    pub fn parse(data: impl Into<FontData>) -> Result<Self, FontError> {
        drop(data.into());
        Err(FontError::ParsedFontOff)
    }

    /// Parses `data` — see [`FontData`] for what it can be made from — and
    /// keeps it for the outlines the atlas reads later and for shaping.
    ///
    /// # Errors
    ///
    /// [`FontError`] when the table directory cannot be read, by `skrifa` or
    /// by `harfrust`, or the font has no units per em.
    #[cfg(feature = "parsed-font")]
    pub fn parse(data: impl Into<FontData>) -> Result<Self, FontError> {
        Ok(Self::of(Source {
            ids: next_ids(),
            parsed: OnceLock::from(Parsed::read(data.into())?),
            load: Mutex::new(None),
            what: String::new(),
        }))
    }

    /// A face read from `load`'s bytes the first time anything asks it a
    /// question — a glyph, a metric, an outline — rather than now.
    ///
    /// **What it is for is fallback faces** ([`Font::with_fallback`],
    /// [`Ui::set_fallback_fonts`](crate::tree::Ui::set_fallback_fonts)): a
    /// cluster asks them in order and stops at the first that maps it, so a
    /// face is read only when a char first misses every face before it, and a
    /// launcher that lists a dozen system fallbacks, CJK collections among
    /// them, reads none of them until the text needs one. Its [`Font::id`] is
    /// given now, so a tree's fallback list hashes without reading anything.
    ///
    /// A face that fails to load or to parse is logged once, as a warning,
    /// and maps no char from then on, so fallback goes past it, as it would
    /// past a face that lacks the glyph.
    #[cfg(feature = "parsed-font")]
    #[must_use]
    pub fn lazy<D, E>(load: impl FnOnce() -> Result<D, E> + Send + 'static) -> Self
    where
        D: Into<FontData>,
        E: fmt::Display,
    {
        Self::lazy_called("a lazily loaded font face".to_owned(), load)
    }

    /// [`Font::lazy`], whose warning calls the face `what`.
    #[cfg(feature = "parsed-font")]
    pub(crate) fn lazy_called<D, E>(
        what: String,
        load: impl FnOnce() -> Result<D, E> + Send + 'static,
    ) -> Self
    where
        D: Into<FontData>,
        E: fmt::Display,
    {
        let load: Loader =
            Box::new(move || load().map(Into::into).map_err(|error| error.to_string()));
        Self::of(Source {
            ids: next_ids(),
            parsed: OnceLock::new(),
            load: Mutex::new(Some(load)),
            what,
        })
    }

    /// Whether this face has been read: always for one [`Font::parse`]
    /// made, and for a `Font::lazy` one once something asked it.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.source.parsed.get().is_some()
    }

    /// A handle on `source`, drawn as it is, with no fallback faces.
    #[cfg(feature = "parsed-font")]
    fn of(source: Source) -> Self {
        Self {
            source: Arc::new(source),
            synthesis: Synthesis::NONE,
            fallback: no_fallback(),
        }
    }

    /// The parse, read now if this is a [`Font::lazy`] face not yet asked.
    #[cfg(feature = "parsed-font")]
    fn parsed(&self) -> &Parsed {
        self.source.parsed.get_or_init(|| self.source.read())
    }

    /// Without the `parsed-font` feature there is no font to ask.
    #[cfg(not(feature = "parsed-font"))]
    fn parsed(&self) -> &Parsed {
        match self.source.never {}
    }
}

#[cfg(feature = "parsed-font")]
impl Parsed {
    /// What [`Font::parse`] reads from `data`.
    fn read(data: FontData) -> Result<Self, FontError> {
        let font = FontRef::new(data.as_bytes())
            .map_err(|error| FontError::Unreadable(error.to_string()))?;
        let unscaled = font.metrics(Size::unscaled(), LocationRef::default());
        if unscaled.units_per_em == 0 {
            return Err(FontError::NoUnitsPerEm);
        }
        let metrics = FontMetrics {
            units_per_em: unscaled.units_per_em,
            ascent: unscaled.ascent,
            descent: unscaled.descent,
            line_gap: unscaled.leading,
        };
        let underline = unscaled.underline.map(|line| Underline {
            offset: line.offset,
            thickness: line.thickness,
        });
        let attributes = font.attributes();
        let color = font.colr().is_ok() || font.cbdt().is_ok() || font.sbix().is_ok();

        let charmap = font.charmap();
        let mut latin1 = [0; LATIN1_END as usize];
        for (codepoint, glyph) in latin1.iter_mut().enumerate() {
            *glyph = charmap
                .map(codepoint as u32)
                .map_or(0, skrifa::GlyphId::to_u32);
        }

        let glyph_metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
        let advances = (0..glyph_metrics.glyph_count())
            .map(|glyph| {
                glyph_metrics
                    .advance_width(skrifa::GlyphId::new(glyph))
                    .unwrap_or(0.0)
            })
            .collect();

        let shaper = match &data.0 {
            Bytes::Static(bytes) => harfrust::Font::new(*bytes, 0),
            Bytes::Shared(bytes) => harfrust::Font::new(Arc::clone(bytes), 0),
        }
        .ok_or_else(|| FontError::Unreadable("harfrust cannot read it".to_owned()))?;

        Ok(Self {
            data,
            metrics,
            underline,
            weight: attributes.weight.value(),
            italic: attributes.style != skrifa::attribute::Style::Normal,
            color,
            latin1,
            advances,
            shaper: Some(shaper),
        })
    }

    /// What a [`Font::lazy`] face that could not be read answers: no glyph
    /// for any char, so fallback goes past it, and no bytes to shape.
    fn empty() -> Self {
        Self {
            data: FontData::from(&[]),
            metrics: FontMetrics {
                units_per_em: 1,
                ascent: 0.0,
                descent: 0.0,
                line_gap: 0.0,
            },
            underline: None,
            weight: 400.0,
            italic: false,
            color: false,
            latin1: [0; LATIN1_END as usize],
            advances: Box::new([]),
            shaper: None,
        }
    }
}

#[cfg(feature = "parsed-font")]
impl Source {
    /// A [`Font::lazy`] face's parse, read from its loader, or
    /// [`Parsed::empty`] with a warning when it cannot be.
    fn read(&self) -> Parsed {
        let load = self
            .load
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        // `parsed` runs this at most once, so a face made without a loader —
        // one `Font::parse` made — has its parse set and never gets here.
        let read = load
            .ok_or_else(|| "it has no loader".to_owned())
            .and_then(|load| load())
            .and_then(|data| Parsed::read(data).map_err(|error| error.to_string()));
        read.unwrap_or_else(|error| {
            crcbl_core::warn!("{} maps no char: {error}", self.what);
            Parsed::empty()
        })
    }
}

impl Font {
    /// This font's identity: one per parse and [`Synthesis`].
    #[must_use]
    pub fn id(&self) -> FontId {
        self.source.ids[self.synthesis.index()]
    }

    /// This face drawn with `synthesis` — see [`face`] — sharing its parse:
    /// a handle of its own identity, the same one every time it is asked
    /// for, so a span in it is measured and its glyphs cached once.
    #[must_use]
    pub fn synthesized(&self, synthesis: Synthesis) -> Self {
        Self {
            source: Arc::clone(&self.source),
            synthesis,
            fallback: Arc::clone(&self.fallback),
        }
    }

    /// This face with `fallback` asked, in order, for every cluster it does
    /// not map whole — see [`fallback`] — in place of whatever it carried. The
    /// faces' own fallback faces are left behind, and past
    /// [`fallback::MAX_FALLBACK_FACES`] they are left out.
    #[must_use]
    pub fn with_fallback(&self, fallback: &[Self]) -> Self {
        let faces = fallback
            .iter()
            .take(fallback::MAX_FALLBACK_FACES)
            .map(|face| Self {
                source: Arc::clone(&face.source),
                synthesis: face.synthesis,
                fallback: no_fallback(),
            })
            .collect();
        Self {
            source: Arc::clone(&self.source),
            synthesis: self.synthesis,
            fallback: faces,
        }
    }

    /// The fallback faces this handle carries, in the order they are asked.
    #[must_use]
    pub fn fallback(&self) -> &[Self] {
        &self.fallback
    }

    /// Face `index` of this handle: itself for zero, its `index`th fallback
    /// face from one — what [`shape::ShapedGlyph::face`] numbers — and itself
    /// for an index past them.
    #[must_use]
    pub fn face(&self, index: u8) -> &Self {
        match index.checked_sub(1) {
            Some(at) => self.fallback.get(usize::from(at)).unwrap_or(self),
            None => self,
        }
    }

    /// Without the `parsed-font` feature there is no font to ask.
    #[cfg(not(feature = "parsed-font"))]
    #[must_use]
    pub fn has_color_glyph(&self, _glyph: GlyphId) -> bool {
        match self.source.never {}
    }

    /// Without the `parsed-font` feature there is no font to ask.
    #[cfg(not(feature = "parsed-font"))]
    pub(crate) fn has_color_tables(&self) -> bool {
        match self.source.never {}
    }

    /// Whether any of its glyphs can be a colour glyph: whether it has a
    /// `COLR`, `CBDT` or `sbix` table. Read once, when it was parsed.
    #[cfg(feature = "parsed-font")]
    pub(crate) fn has_color_tables(&self) -> bool {
        self.parsed().color
    }

    /// Whether `glyph` has a colour representation: a `COLR` layer list or
    /// paint graph, or a bitmap in one of its `CBDT` or `sbix` strikes.
    #[cfg(feature = "parsed-font")]
    #[must_use]
    pub fn has_color_glyph(&self, glyph: GlyphId) -> bool {
        self.parsed().color
            && FontRef::new(self.data())
                .is_ok_and(|font| color::has_color_glyph(&font, skrifa::GlyphId::new(glyph.0)))
    }

    /// What this handle synthesises; [`Synthesis::NONE`] for a face as
    /// parsed.
    #[must_use]
    pub const fn synthesis(&self) -> Synthesis {
        self.synthesis
    }

    /// The face's weight class, 1 to 1000, as its `OS/2` table gives it — or
    /// its `head` table's bold bit, for a font with none.
    #[must_use]
    pub fn weight(&self) -> f32 {
        self.parsed().weight
    }

    /// Whether the face is italic or oblique, by its `OS/2` or `head` table.
    #[must_use]
    pub fn is_italic(&self) -> bool {
        self.parsed().italic
    }

    /// Where the font says an underline goes, or `None` when it does not
    /// say.
    #[must_use]
    pub fn underline(&self) -> Option<Underline> {
        self.parsed().underline
    }

    /// The bytes it was parsed from.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        self.parsed().data.as_bytes()
    }

    /// Its line metrics.
    #[must_use]
    pub fn metrics(&self) -> FontMetrics {
        self.parsed().metrics
    }

    /// How many glyphs it holds.
    #[must_use]
    pub fn glyph_count(&self) -> usize {
        self.parsed().advances.len()
    }

    /// The glyph `c` draws as: through the cmap, or [`GlyphId::NOTDEF`] when
    /// the font maps it to nothing.
    #[must_use]
    pub fn glyph_id(&self, c: char) -> GlyphId {
        let codepoint = c as u32;
        if codepoint < LATIN1_END {
            return GlyphId(self.parsed().latin1[codepoint as usize]);
        }
        #[cfg(not(feature = "parsed-font"))]
        match self.source.never {}
        #[cfg(feature = "parsed-font")]
        FontRef::new(self.data())
            .ok()
            .and_then(|font| font.charmap().map(codepoint))
            .map_or(GlyphId::NOTDEF, |glyph| GlyphId(glyph.to_u32()))
    }

    /// `glyph`'s advance in font units, widened by a synthesised bold's
    /// [`face::SYNTHETIC_BOLD_EM`]; zero for a glyph the font does not have.
    #[must_use]
    pub fn advance(&self, glyph: GlyphId) -> f32 {
        let Some(&advance) = self.parsed().advances.get(glyph.0 as usize) else {
            return 0.0;
        };
        if self.synthesis.bold {
            advance + f32::from(self.parsed().metrics.units_per_em) * face::SYNTHETIC_BOLD_EM
        } else {
            advance
        }
    }

    /// A font with `metrics` and no outlines, leaked for the process: every
    /// Latin-1 codepoint is glyph 1, `advance` font units wide, and nothing
    /// is shaped. What a test measures known widths in; it cannot be
    /// rasterised, and without the `parsed-font` feature it cannot be made.
    #[cfg(all(test, feature = "parsed-font"))]
    pub(crate) fn fixed_pitch(metrics: FontMetrics, advance: f32) -> &'static Self {
        Self::fixed_pitch_face(metrics, advance, 400.0, false)
    }

    /// [`Font::fixed_pitch`] as a face of `weight`, italic or not: what a
    /// test builds a family of faces from.
    #[cfg(all(test, feature = "parsed-font"))]
    pub(crate) fn fixed_pitch_face(
        metrics: FontMetrics,
        advance: f32,
        weight: f32,
        italic: bool,
    ) -> &'static Self {
        Box::leak(Box::new(Self::of(Source {
            ids: next_ids(),
            parsed: OnceLock::from(Parsed {
                data: FontData::from(&[]),
                metrics,
                underline: None,
                weight,
                italic,
                color: false,
                latin1: [1; LATIN1_END as usize],
                advances: Box::new([0.0, advance]),
                shaper: None,
            }),
            load: Mutex::new(None),
            what: String::new(),
        })))
    }
}

/// An empty fallback list.
fn no_fallback() -> Arc<[Font]> {
    Arc::from(Vec::new())
}

#[cfg(all(test, not(feature = "parsed-font")))]
mod off_tests {
    use super::*;

    /// **Without the feature, nothing parses** — not even the committed font's
    /// own bytes, which a build with it on parses — and the refusal says why.
    #[test]
    fn without_the_feature_every_parse_is_refused_and_says_why() {
        let bytes = include_bytes!("../../fonts/AtkinsonHyperlegible-Regular.ttf");
        let refused = Font::parse(bytes);
        assert!(matches!(refused, Err(FontError::ParsedFontOff)));
        let message = FontError::ParsedFontOff.to_string();
        assert!(message.contains("parsed-font"), "{message}");
        assert_eq!(FontFamily::default().font(), None);
    }
}

#[cfg(all(test, feature = "parsed-font"))]
mod tests {
    use super::*;

    /// **The committed font parses into what layout reads**: its units per em
    /// and line metrics as `skrifa` reports them, a glyph for every printable
    /// Latin-1 codepoint, and its tables ready to shape with.
    #[test]
    fn the_committed_font_parses_with_latin1_and_shaping() {
        let font = Font::sans();
        let reference = FontRef::new(SANS_TTF).expect("reads");
        let metrics = reference.metrics(Size::unscaled(), LocationRef::default());
        assert_eq!(font.metrics().units_per_em, metrics.units_per_em);
        assert_eq!(font.metrics().ascent, metrics.ascent);
        assert_eq!(font.metrics().descent, metrics.descent);
        assert!(font.metrics().ascent > 0.0 && font.metrics().descent < 0.0);

        let printable = (0x20u32..0x7f).chain(0xa0..0x100);
        for codepoint in printable {
            let c = char::from_u32(codepoint).expect("Latin-1 is valid");
            assert_ne!(
                font.glyph_id(c),
                GlyphId::NOTDEF,
                "U+{codepoint:04X} has no glyph"
            );
        }
        assert!(
            font.parsed().shaper.is_some(),
            "the committed font is not ready to shape"
        );
    }

    /// **A codepoint the cmap does not map draws `.notdef`**, inside Latin-1
    /// and past it, and one it does map is read through the cmap past Latin-1
    /// too.
    #[test]
    fn an_unmapped_codepoint_is_notdef_and_a_mapped_one_is_not() {
        let font = Font::sans();
        let reference = FontRef::new(SANS_TTF).expect("reads");
        let charmap = reference.charmap();
        for c in ['\u{1}', '\u{7f}', '\u{4e2d}', '\u{1f600}'] {
            assert_eq!(charmap.map(c as u32), None, "{c:?} is mapped after all");
            assert_eq!(font.glyph_id(c), GlyphId::NOTDEF, "{c:?}");
        }
        // Past Latin-1 and mapped: the Euro sign and the en dash.
        for c in ['€', '–'] {
            let want = charmap.map(c as u32).expect("the font maps it");
            assert_eq!(font.glyph_id(c), GlyphId(want.to_u32()), "{c:?}");
            assert_ne!(font.glyph_id(c), GlyphId::NOTDEF);
        }
    }

    /// **Bytes that are not a font are refused**, not panicked on.
    #[test]
    fn bytes_that_are_not_a_font_are_refused() {
        assert!(matches!(
            Font::parse(b"not a font at all"),
            Err(FontError::Unreadable(_))
        ));
    }

    /// Advances are the font's `hmtx` widths, one per glyph.
    #[test]
    fn advances_are_the_fonts_own() {
        let font = Font::sans();
        let reference = FontRef::new(SANS_TTF).expect("reads");
        let metrics = reference.glyph_metrics(Size::unscaled(), LocationRef::default());
        assert_eq!(font.glyph_count(), metrics.glyph_count() as usize);
        for c in ['A', 'i', 'W', ' ', 'é'] {
            let glyph = font.glyph_id(c);
            assert_eq!(
                Some(font.advance(glyph)),
                metrics.advance_width(skrifa::GlyphId::new(glyph.0)),
                "{c:?}"
            );
        }
        assert_eq!(font.advance(GlyphId(u32::MAX)), 0.0);
    }

    /// **Bytes read at run time parse as the compiled-in ones do**, and the
    /// font holds them once, however many clones of it there are, until the
    /// last clone goes: nothing is leaked to make them last.
    #[test]
    fn a_font_from_run_time_bytes_holds_them_until_its_last_clone_goes() {
        let bytes: Arc<[u8]> = Arc::from(SANS_TTF);
        let font = Font::parse(Arc::clone(&bytes)).expect("parses");
        assert_eq!(font.metrics(), Font::sans().metrics());
        assert_eq!(font.glyph_id('A'), Font::sans().glyph_id('A'));
        let copy = font.clone();
        assert_eq!(copy, font, "a clone is the same font");
        assert_eq!(Arc::strong_count(&bytes), 2);
        drop(font);
        assert_eq!(Arc::strong_count(&bytes), 2, "the clone still holds them");
        drop(copy);
        assert_eq!(Arc::strong_count(&bytes), 1, "freed with the last clone");

        let owned = Font::parse(SANS_TTF.to_vec()).expect("parses");
        assert_eq!(owned.data(), SANS_TTF);
    }

    /// Two parses are two identities, so an atlas never confuses their glyphs.
    #[test]
    fn every_parse_is_its_own_id() {
        let a = Font::parse(SANS_TTF).expect("parses");
        let b = Font::parse(SANS_TTF).expect("parses");
        assert_ne!(a.id(), b.id());
    }
}
