//! The faces text is drawn in: found through `crcbl-font`, read once per
//! document, asked for outlines and metrics through `skrifa`, and handed to
//! `harfrust` to shape with.
//!
//! # Which face draws a cluster
//!
//! A text's chain of faces is each family of its `font-family` list that
//! the system has, in order, then `serif`, the default font of browsers and
//! of resvg, so that a list naming nothing installed still draws. A cluster
//! (a character and the marks and format characters after it, as
//! [`shape`](super::shape) cuts them) takes the first face of the chain that
//! maps every character of it that draws, else the first of the system's
//! fallbacks for the chain's first face ([`FontSource::fallbacks`]) that
//! does; failing both, the first face of either that maps its first such
//! character, else the chain's first face, which draws what it lacks as its
//! `.notdef`. A cluster of format characters alone takes the chain's first
//! face. A chain with no face at all draws nothing and refuses the text.
//!
//! # Caps
//!
//! A file names the families it likes, so it decides how many lookups and
//! font files a decode costs: [`MAX_LOOKUPS`] lookups and [`MAX_FACES`]
//! faces per document, each found and each read once. A lookup or a face
//! past its cap refuses the file, but a fallback search stops at the face
//! cap and draws `.notdef`, so one character no font has cannot refuse a
//! file whose fonts are all installed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crcbl_font::{FontMatch, FontQuery, FontStyle, SystemFonts};
use skrifa::instance::{Location, Size};
use skrifa::metrics::Metrics;
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::ImageError;
use crate::svg::path::Segment;
use crate::svg::style::{Family, Slant, TextStyle};
use crate::svg::unsupported;

/// The most font lookups one document makes, each distinct query once: a
/// `find` per family and weight and style, a `fallbacks` per first face.
pub(in crate::svg) const MAX_LOOKUPS: usize = 64;

/// The most faces one document reads, each once however much text it draws.
pub(in crate::svg) const MAX_FACES: usize = 16;

/// Where faces are looked up: the system's through [`SystemFonts`], or a
/// test's own.
pub(in crate::svg) trait FontSource: Sync {
    /// The face of `query`'s family closest to its weight and style.
    fn find(&self, query: &FontQuery) -> Option<FontMatch>;
    /// The faces to try, in order, for a character `query`'s face lacks.
    fn fallbacks(&self, query: &FontQuery) -> Vec<FontMatch>;
}

impl FontSource for SystemFonts {
    fn find(&self, query: &FontQuery) -> Option<FontMatch> {
        SystemFonts::find(self, query)
    }

    fn fallbacks(&self, query: &FontQuery) -> Vec<FontMatch> {
        SystemFonts::fallbacks(self, query)
    }
}

/// The system's fonts, for every decode: `crcbl-font` asks for one per
/// process, since its first lookup reads the system's font list.
pub(in crate::svg) fn system() -> &'static SystemFonts {
    static FONTS: OnceLock<SystemFonts> = OnceLock::new();
    FONTS.get_or_init(SystemFonts::new)
}

/// One glyph of one face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::svg) struct Glyph {
    /// The face's index among a [`Faces`]'s.
    pub face: usize,
    pub id: GlyphId,
}

/// A face's measures at one weight, in font units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::svg) struct FaceMetrics {
    pub units_per_em: f64,
    /// Above the baseline, positive.
    pub ascent: f64,
    /// Below the baseline, negative.
    pub descent: f64,
}

/// A text's font, as far as choosing faces goes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FontKey {
    families: Vec<Family>,
    weight: u16,
    slant: Slant,
}

/// One face read: its bytes, a font file of its own, for `skrifa`, and the
/// same bytes read by `harfrust`.
struct Face {
    bytes: Arc<Vec<u8>>,
    shaper: harfrust::Font,
}

/// The faces one document has looked up and read.
#[derive(Default)]
pub(in crate::svg) struct Faces {
    /// Each face read.
    faces: Vec<Face>,
    /// Each face found, by file and index in it: its index in `faces`, or
    /// `None` for one that could not be read or parsed.
    files: HashMap<(PathBuf, u32), Option<usize>>,
    /// Each query's face.
    found: HashMap<FontQuery, Option<usize>>,
    /// Each first face's fallbacks, by the query that found it.
    fallbacks: HashMap<FontQuery, Vec<FontMatch>>,
    /// Each font's chain, and the query its first face answered.
    chains: HashMap<FontKey, (Vec<usize>, Option<FontQuery>)>,
    lookups: usize,
}

impl Faces {
    /// The face `style`'s text draws `cluster` in, as the module docs say;
    /// `draws` tells the characters that need a glyph from the format
    /// characters that do not.
    ///
    /// # Errors
    ///
    /// [`ImageError::Unsupported`] if no face can draw `style`'s text at
    /// all, or past [`MAX_LOOKUPS`] or [`MAX_FACES`].
    pub(in crate::svg) fn cluster_face(
        &mut self,
        source: &dyn FontSource,
        style: &TextStyle,
        cluster: &str,
        draws: impl Fn(char) -> bool,
    ) -> Result<usize, ImageError> {
        let (chain, first_query) = self.chain(source, style)?;
        let Some(&first) = chain.first() else {
            return Err(no_face(style));
        };
        let drawn: Vec<char> = cluster.chars().filter(|&ch| draws(ch)).collect();
        let Some(&lead) = drawn.first() else {
            return Ok(first);
        };
        let query = first_query.as_ref();
        let maps_all = |faces: &Self, face| drawn.iter().all(|&ch| faces.map(face, ch).is_some());
        if let Some(face) = self.first_face(source, &chain, query, maps_all)? {
            return Ok(face);
        }
        let maps_lead = |faces: &Self, face| faces.map(face, lead).is_some();
        Ok(self
            .first_face(source, &chain, query, maps_lead)?
            .unwrap_or(first))
    }

    /// The first face of `chain`, then of the fallbacks for `first_query`,
    /// that `fits`. The fallbacks are read up to [`MAX_FACES`], where the
    /// search stops rather than refusing.
    fn first_face(
        &mut self,
        source: &dyn FontSource,
        chain: &[usize],
        first_query: Option<&FontQuery>,
        fits: impl Fn(&Self, usize) -> bool,
    ) -> Result<Option<usize>, ImageError> {
        if let Some(&face) = chain.iter().find(|&&face| fits(self, face)) {
            return Ok(Some(face));
        }
        let Some(query) = first_query else {
            return Ok(None);
        };
        for found in self.fallback_list(source, query)? {
            let key = (found.path.clone(), found.index);
            if !self.files.contains_key(&key) && self.faces.len() >= MAX_FACES {
                break;
            }
            if let Some(face) = self.read(&found)?
                && fits(self, face)
            {
                return Ok(Some(face));
            }
        }
        Ok(None)
    }

    /// The faces `style`'s text tries in order, and the query that found the
    /// first.
    fn chain(
        &mut self,
        source: &dyn FontSource,
        style: &TextStyle,
    ) -> Result<(Vec<usize>, Option<FontQuery>), ImageError> {
        let key = FontKey {
            families: style.families.clone(),
            weight: style.weight,
            slant: style.slant,
        };
        if let Some(chain) = self.chains.get(&key) {
            return Ok(chain.clone());
        }
        let font_style = match style.slant {
            Slant::Normal => FontStyle::Normal,
            Slant::Italic => FontStyle::Italic,
            Slant::Oblique => FontStyle::Oblique,
        };
        let names = style
            .families
            .iter()
            .map(|family| match family {
                Family::Named(name) => name.as_str(),
                Family::Generic(generic) => generic,
            })
            .chain(["serif"]);
        let mut chain = Vec::new();
        let mut first_query = None;
        for name in names {
            let query = FontQuery::new(name).weight(style.weight).style(font_style);
            if let Some(face) = self.find(source, &query)?
                && !chain.contains(&face)
            {
                if chain.is_empty() {
                    first_query = Some(query);
                }
                chain.push(face);
            }
        }
        let entry = (chain, first_query);
        self.chains.insert(key, entry.clone());
        Ok(entry)
    }

    /// Counts one lookup, refusing one past [`MAX_LOOKUPS`].
    fn look_up(&mut self) -> Result<(), ImageError> {
        self.lookups += 1;
        if self.lookups > MAX_LOOKUPS {
            return Err(unsupported(format_args!(
                "text looking up more than {MAX_LOOKUPS} fonts"
            )));
        }
        Ok(())
    }

    /// The face `query` finds, read.
    fn find(
        &mut self,
        source: &dyn FontSource,
        query: &FontQuery,
    ) -> Result<Option<usize>, ImageError> {
        if let Some(&face) = self.found.get(query) {
            return Ok(face);
        }
        self.look_up()?;
        let face = match source.find(query) {
            Some(found) => self.read(&found)?,
            None => None,
        };
        self.found.insert(query.clone(), face);
        Ok(face)
    }

    fn fallback_list(
        &mut self,
        source: &dyn FontSource,
        query: &FontQuery,
    ) -> Result<Vec<FontMatch>, ImageError> {
        if let Some(list) = self.fallbacks.get(query) {
            return Ok(list.clone());
        }
        self.look_up()?;
        let list = source.fallbacks(query);
        self.fallbacks.insert(query.clone(), list.clone());
        Ok(list)
    }

    /// The face `found` names, read once: `None` if its file cannot be read
    /// or parsed, which counts as a face not installed.
    fn read(&mut self, found: &FontMatch) -> Result<Option<usize>, ImageError> {
        let key = (found.path.clone(), found.index);
        if let Some(&face) = self.files.get(&key) {
            return Ok(face);
        }
        if self.faces.len() >= MAX_FACES {
            return Err(unsupported(format_args!(
                "text in more than {MAX_FACES} font faces"
            )));
        }
        let face = found
            .read()
            .ok()
            .filter(|bytes| FontRef::new(bytes).is_ok())
            .and_then(|bytes| {
                let bytes = Arc::new(bytes);
                let shared: Arc<dyn AsRef<[u8]> + Send + Sync> = bytes.clone();
                let shaper = harfrust::Font::new(shared, 0)?;
                self.faces.push(Face { bytes, shaper });
                Some(self.faces.len() - 1)
            });
        self.files.insert(key, face);
        Ok(face)
    }

    fn font(&self, face: usize) -> Option<FontRef<'_>> {
        FontRef::new(&self.faces[face].bytes).ok()
    }

    /// `face` for `harfrust` to shape at `weight`: its `wght` axis set to
    /// it, if the face is variable along one, as outlines and metrics are.
    pub(in crate::svg) fn shaper(&self, face: usize, weight: u16) -> harfrust::Font {
        self.faces[face]
            .shaper
            .instance_builder()
            .variations([("wght", f32::from(weight))])
            .build()
    }

    /// `ch`'s glyph in `face`, if the face maps it.
    fn map(&self, face: usize, ch: char) -> Option<GlyphId> {
        self.font(face)?
            .charmap()
            .map(ch)
            .filter(|id| *id != GlyphId::NOTDEF)
    }

    /// Where in `face`'s design space `weight` falls: its `wght` axis set
    /// to it, if the face is variable along one.
    fn location(font: &FontRef<'_>, weight: u16) -> Location {
        font.axes().location([("wght", f32::from(weight))])
    }

    /// `face`'s ascent, descent and em at `weight`.
    pub(in crate::svg) fn metrics(&self, face: usize, weight: u16) -> FaceMetrics {
        let Some(font) = self.font(face) else {
            return FaceMetrics {
                units_per_em: 1.0,
                ascent: 0.0,
                descent: 0.0,
            };
        };
        let location = Self::location(&font, weight);
        let metrics = Metrics::new(&font, Size::unscaled(), &location);
        FaceMetrics {
            units_per_em: f64::from(metrics.units_per_em.max(1)),
            ascent: f64::from(metrics.ascent),
            descent: f64::from(metrics.descent),
        }
    }

    /// Appends `glyph`'s outline at `weight` to `out`, each point of it
    /// mapped by `place` from font units, `y` up, to user space.
    pub(in crate::svg) fn outline(
        &self,
        glyph: Glyph,
        weight: u16,
        place: impl Fn(f64, f64) -> (f64, f64),
        out: &mut Vec<Segment>,
    ) {
        let Some(font) = self.font(glyph.face) else {
            return;
        };
        let Some(outline) = font.outline_glyphs().get(glyph.id) else {
            return;
        };
        let location = Self::location(&font, weight);
        let start = out.len();
        let mut pen = Pen { place, out };
        if outline
            .draw(
                DrawSettings::unhinted(Size::unscaled(), &location),
                &mut pen,
            )
            .is_err()
        {
            // A damaged glyph draws nothing rather than a fragment of itself.
            out.truncate(start);
        }
    }
}

/// The refusal of text in `style`, none of whose fonts is installed.
fn no_face(style: &TextStyle) -> ImageError {
    let families: Vec<String> = style
        .families
        .iter()
        .map(|family| match family {
            Family::Named(name) => format!("{name:?}"),
            Family::Generic(generic) => (*generic).to_owned(),
        })
        .chain(["serif".to_owned()])
        .collect();
    unsupported(format_args!(
        "text in fonts none of which is installed: {}",
        families.join(", ")
    ))
}

/// An outline drawn into path segments in user space.
struct Pen<'a, F> {
    place: F,
    out: &'a mut Vec<Segment>,
}

impl<F: Fn(f64, f64) -> (f64, f64)> Pen<'_, F> {
    fn point(&self, x: f32, y: f32) -> (f64, f64) {
        (self.place)(f64::from(x), f64::from(y))
    }
}

impl<F: Fn(f64, f64) -> (f64, f64)> OutlinePen for Pen<'_, F> {
    fn move_to(&mut self, x: f32, y: f32) {
        let to = self.point(x, y);
        self.out.push(Segment::MoveTo(to));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.point(x, y);
        self.out.push(Segment::LineTo(to));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let segment = Segment::QuadTo(self.point(cx0, cy0), self.point(x, y));
        self.out.push(segment);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let segment =
            Segment::CubicTo(self.point(cx0, cy0), self.point(cx1, cy1), self.point(x, y));
        self.out.push(segment);
    }

    fn close(&mut self) {
        self.out.push(Segment::Close);
    }
}
