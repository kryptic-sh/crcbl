//! Properties: which ones this renderer reads, which it refuses, their
//! values, and the cascade from presentation attributes, the `<style>` sheet
//! and the `style` attribute to each element's computed style.
//!
//! # Supported, refused, ignored
//!
//! Every property name falls in one of three sets:
//!
//! * **Supported** ([`Property`]): parsed, and a value naming a feature this
//!   renderer lacks (a marker, a blend mode, a filter function, crisp-edged
//!   rendering) refuses the file.
//! * **Refused by name**: [`REFUSED_PROPERTIES`] wherever they are given, and
//!   [`ATTRIBUTE_ONLY_PROPERTIES`] in a declaration. They change the picture
//!   in ways this renderer does not draw, so a file using one fails loudly
//!   rather than drawing without it.
//! * **Everything else is ignored**, as browsers and resvg ignore it: a
//!   property SVG does not define for these elements, one with no effect on
//!   what this renderer draws (`line-height` and other properties of wrapped
//!   text, rendering hints, an editor's own), or a misspelling.
//!
//! Text has properties of its own, in [`text`]: those it lays out, and those
//! whose other values it would not draw, which refuse only the text drawn
//! under them.
//!
//! # Invalid values
//!
//! A value that is no value of its property at all — `fill="#fff;opacity:1"`,
//! an opacity that is not a number, a negative width, a word where a colour
//! belongs that names no colour (a misspelling, a system colour) — is
//! ignored, as CSS
//! ignores an invalid declaration: the next declaration down the cascade
//! applies, or failing one the property is inherited or takes its initial
//! value, as SVG 2 says of an invalid presentation attribute. A value in a
//! form this renderer does not evaluate is not known to be invalid, and is
//! refused like an undrawn feature: a CSS function such as `var()` or
//! `calc()`, a colour function other than `rgb()`, a length in a unit it does
//! not resolve (`em`), or a reference to a paint it does not draw
//! (`context-fill`, a paint server in another file).

mod text;

pub(super) use text::TextStyle;
#[cfg(feature = "svg-text")]
pub(super) use text::{Anchor, Family, Slant};

use self::text::TextProperty;
use super::color::{self, Color, ColorValue};
use super::css::{self, Declaration, Rule};
use super::path::number_prefix;
use super::xml::{Document, Element, Namespace};
use crate::ImageError;
use crate::raster::FillRule;

fn unsupported(what: impl core::fmt::Display) -> ImageError {
    ImageError::Unsupported(format!("SVG {what}"))
}

/// A property this renderer reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Property {
    ClipPath,
    ClipRule,
    Color,
    ColorInterpolationFilters,
    Display,
    Fill,
    FillOpacity,
    FillRule,
    Filter,
    FloodColor,
    FloodOpacity,
    Marker,
    MarkerEnd,
    MarkerMid,
    MarkerStart,
    Mask,
    MaskType,
    MixBlendMode,
    Opacity,
    PaintOrder,
    ShapeRendering,
    StopColor,
    StopOpacity,
    Stroke,
    StrokeDasharray,
    StrokeDashoffset,
    StrokeLinecap,
    StrokeLinejoin,
    StrokeMiterlimit,
    StrokeOpacity,
    StrokeWidth,
    VectorEffect,
    Visibility,
    /// One of the text properties, which [`text`] reads.
    Text(TextProperty),
}

const PROPERTIES: &[(&str, Property)] = &[
    ("clip-path", Property::ClipPath),
    ("clip-rule", Property::ClipRule),
    ("color", Property::Color),
    (
        "color-interpolation-filters",
        Property::ColorInterpolationFilters,
    ),
    ("display", Property::Display),
    ("fill", Property::Fill),
    ("fill-opacity", Property::FillOpacity),
    ("fill-rule", Property::FillRule),
    ("filter", Property::Filter),
    ("flood-color", Property::FloodColor),
    ("flood-opacity", Property::FloodOpacity),
    ("marker", Property::Marker),
    ("marker-end", Property::MarkerEnd),
    ("marker-mid", Property::MarkerMid),
    ("marker-start", Property::MarkerStart),
    ("mask", Property::Mask),
    ("mask-type", Property::MaskType),
    ("mix-blend-mode", Property::MixBlendMode),
    ("opacity", Property::Opacity),
    ("paint-order", Property::PaintOrder),
    ("shape-rendering", Property::ShapeRendering),
    ("stop-color", Property::StopColor),
    ("stop-opacity", Property::StopOpacity),
    ("stroke", Property::Stroke),
    ("stroke-dasharray", Property::StrokeDasharray),
    ("stroke-dashoffset", Property::StrokeDashoffset),
    ("stroke-linecap", Property::StrokeLinecap),
    ("stroke-linejoin", Property::StrokeLinejoin),
    ("stroke-miterlimit", Property::StrokeMiterlimit),
    ("stroke-opacity", Property::StrokeOpacity),
    ("stroke-width", Property::StrokeWidth),
    ("vector-effect", Property::VectorEffect),
    ("visibility", Property::Visibility),
];

/// Properties that change the picture in ways this renderer does not draw,
/// refused by name as presentation attributes and in declarations: a
/// transform's box and origin, CSS Masking's longhands, and the
/// `background-color` resvg paints behind an SVG's root.
pub(super) const REFUSED_PROPERTIES: &[&str] = &[
    "background-color",
    "mask-border",
    "mask-border-mode",
    "mask-border-outset",
    "mask-border-repeat",
    "mask-border-slice",
    "mask-border-source",
    "mask-border-width",
    "mask-clip",
    "mask-composite",
    "mask-image",
    "mask-mode",
    "mask-origin",
    "mask-position",
    "mask-repeat",
    "mask-size",
    "transform-box",
    "transform-origin",
];

/// Properties a declaration may set that move or reshape an element:
/// geometry and `transform` given in CSS, which this renderer reads only as
/// attributes, and CSS's individual transforms and motion path. Refused by
/// name in a declaration; as attributes they are an element's own, or mean
/// nothing on it.
pub(super) const ATTRIBUTE_ONLY_PROPERTIES: &[&str] = &[
    "cx",
    "cy",
    "d",
    "height",
    "offset",
    "offset-anchor",
    "offset-distance",
    "offset-path",
    "offset-position",
    "offset-rotate",
    "r",
    "rotate",
    "rx",
    "ry",
    "scale",
    "transform",
    "translate",
    "width",
    "x",
    "y",
];

/// The supported property called `name`, if it is one.
fn supported(name: &str) -> Option<Property> {
    PROPERTIES
        .iter()
        .find(|(known, _)| *known == name)
        .map(|&(_, property)| property)
        .or_else(|| TextProperty::named(name).map(Property::Text))
}

/// The name of `property`.
fn property_name(property: Property) -> &'static str {
    match property {
        Property::Text(text) => text.name(),
        _ => PROPERTIES
            .iter()
            .find(|(_, known)| *known == property)
            .map_or("?", |(name, _)| name),
    }
}

/// One element's declarations of the supported properties, lowest-ranked
/// first: the highest-ranked valid one of each applies.
#[derive(Clone, Debug, Default)]
pub(super) struct Specified {
    values: Vec<(Property, String)>,
}

/// Where a declaration ranks in the cascade, lowest first: whether it is
/// `!important`, its origin (presentation attribute, sheet, `style`
/// attribute), its selector's specificity, and its order.
type Rank = (bool, u8, (usize, usize, usize), usize);

/// The cascade's inputs for a whole document: its sheet, parsed once.
pub(super) struct Cascade {
    rules: Vec<Rule>,
}

impl Cascade {
    /// Gathers every `<style>` element's sheet.
    ///
    /// # Errors
    ///
    /// A sheet of a type other than CSS, or for a medium other than all or
    /// the screen, is unsupported; and see [`css::sheet`].
    pub fn new(document: &Document) -> Result<Self, ImageError> {
        let mut rules = Vec::new();
        for element in &document.elements {
            if !element.is("style") {
                continue;
            }
            if let Some(kind) = element.attribute(Namespace::None, "type")
                && !kind.is_empty()
                && kind != "text/css"
            {
                return Err(unsupported(format_args!("a <style> of type {kind}")));
            }
            if let Some(media) = element.attribute(Namespace::None, "media")
                && !matches!(media.trim(), "" | "all" | "screen")
            {
                return Err(unsupported(format_args!("a <style> for the media {media}")));
            }
            rules.extend(css::sheet(&element.text)?);
        }
        Ok(Self { rules })
    }

    /// `element`'s declarations: presentation attributes, then the sheet's
    /// rules by specificity and order, then its `style` attribute, then
    /// `!important` declarations in the same order.
    ///
    /// # Errors
    ///
    /// A declaration of a property refused by name is unsupported.
    pub fn specified(&self, element: &Element) -> Result<Specified, ImageError> {
        // The highest-ranked declaration of each property wins.
        let mut ranked: Vec<(Rank, Declaration)> = Vec::new();
        for (order, attribute) in element.attributes.iter().enumerate() {
            if attribute.namespace == Namespace::None && supported(&attribute.name).is_some() {
                ranked.push((
                    (false, 0, (0, 0, 0), order),
                    Declaration {
                        name: attribute.name.clone(),
                        value: attribute.value.clone(),
                        important: false,
                    },
                ));
            }
        }
        for (order, rule) in self.rules.iter().enumerate() {
            if rule.selector.matches(element) {
                for declaration in &rule.declarations {
                    ranked.push((
                        (declaration.important, 1, rule.selector.specificity(), order),
                        declaration.clone(),
                    ));
                }
            }
        }
        if let Some(style) = element.attribute(Namespace::None, "style") {
            for declaration in css::declarations(style)? {
                ranked.push(((declaration.important, 2, (0, 0, 0), 0), declaration));
            }
        }
        ranked.sort_by_key(|(rank, _)| *rank);
        let mut specified = Specified::default();
        for (_, declaration) in ranked {
            let name = declaration.name.as_str();
            if REFUSED_PROPERTIES.contains(&name) || ATTRIBUTE_ONLY_PROPERTIES.contains(&name) {
                return Err(unsupported(format_args!(
                    "property {name} on <{}>",
                    element.name
                )));
            }
            if let Some(property) = supported(name) {
                specified.values.push((property, declaration.value));
            }
        }
        Ok(specified)
    }
}

/// A length before it is resolved against a viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Length {
    /// User units: pixels, or an absolute unit converted to them.
    Px(f64),
    /// A percentage of the viewport's width, height or normalised diagonal,
    /// depending on what it measures.
    Percent(f64),
}

impl Length {
    pub const ZERO: Self = Self::Px(0.0);

    /// In user units, a percentage taken of `reference`.
    pub fn resolve(self, reference: f64) -> f64 {
        match self {
            Self::Px(value) => value,
            Self::Percent(percent) => percent / 100.0 * reference,
        }
    }
}

/// Parses a length: a number with no unit, `px`, an absolute unit, or a
/// percentage. `None` for anything else, `em` and `ex` included (they need a
/// font size, and the text that would set one is refused).
pub(super) fn length(text: &str) -> Option<Length> {
    let (value, unit) = number_prefix(text.trim())?;
    let px = |factor: f64| Some(Length::Px(value * factor));
    match unit.trim_end() {
        "" | "px" => px(1.0),
        "%" => Some(Length::Percent(value)),
        "in" => px(96.0),
        "cm" => px(96.0 / 2.54),
        "mm" => px(96.0 / 25.4),
        "pt" => px(96.0 / 72.0),
        "pc" => px(16.0),
        _ => None,
    }
}

/// A plain number or a percentage of one, as opacities are written.
pub(super) fn number_or_percent(text: &str) -> Option<f32> {
    let (value, rest) = number_prefix(text.trim())?;
    match rest.trim_end() {
        "" => Some(value as f32),
        "%" => Some((value / 100.0) as f32),
        _ => None,
    }
}

/// A `fill` or `stroke`.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Paint {
    None,
    Color(ColorValue),
    /// A paint server by id, with the paint to use if the reference is to
    /// nothing.
    Server {
        id: String,
        fallback: Option<Box<Paint>>,
    },
}

fn paint(text: &str) -> Option<Paint> {
    let text = text.trim();
    if text == "none" {
        return Some(Paint::None);
    }
    if let Some(rest) = text.strip_prefix("url(") {
        let close = rest.find(')')?;
        let id = iri(&rest[..close])?;
        let after = rest[close + 1..].trim();
        let fallback = if after.is_empty() {
            None
        } else {
            match paint(after)? {
                Paint::Server { .. } => return None,
                other => Some(Box::new(other)),
            }
        };
        return Some(Paint::Server { id, fallback });
    }
    color::parse(text).map(Paint::Color)
}

/// The id a local IRI reference names: `#id`, or `'#id'` quoted. `None` for
/// a reference outside the document.
pub(super) fn iri(text: &str) -> Option<String> {
    let text = text.trim().trim_matches(['"', '\'']);
    text.strip_prefix('#').map(str::to_owned)
}

/// `url(#id)` or `none`.
fn func_iri(text: &str) -> Option<Option<String>> {
    let text = text.trim();
    if text == "none" {
        return Some(None);
    }
    let inner = text.strip_prefix("url(")?.strip_suffix(')')?;
    if inner.contains(')') {
        // More than one reference, which none of these properties takes.
        return None;
    }
    iri(inner).map(Some)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LineCap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LineJoin {
    Miter,
    Round,
    Bevel,
}

/// The computed values of every supported property for one element.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Style {
    // Inherited.
    pub fill: Paint,
    pub fill_opacity: f32,
    pub fill_rule: FillRule,
    pub stroke: Paint,
    pub stroke_width: Length,
    pub stroke_linecap: LineCap,
    pub stroke_linejoin: LineJoin,
    pub stroke_miterlimit: f64,
    pub stroke_dasharray: Vec<Length>,
    pub stroke_dashoffset: Length,
    pub stroke_opacity: f32,
    pub color: Color,
    pub visible: bool,
    pub clip_rule: FillRule,
    /// Whether the stroke is painted before the fill.
    pub stroke_first: bool,
    /// Whether filter primitives work in linearRGB rather than sRGB.
    pub filters_linear: bool,
    // Not inherited.
    pub opacity: f32,
    pub display: bool,
    pub clip_path: Option<String>,
    pub mask: Option<String>,
    pub filter: Option<String>,
    pub flood_color: ColorValue,
    pub flood_opacity: f32,
    pub stop_color: ColorValue,
    pub stop_opacity: f32,
    /// `mask-type: alpha` rather than luminance.
    pub mask_alpha: bool,
    /// `vector-effect: non-scaling-stroke`: the stroke's width and dashes are
    /// measured in the coordinates the SVG is placed in, whatever transform
    /// its element is under.
    pub non_scaling_stroke: bool,
    /// The text properties, every one inherited.
    pub text: TextStyle,
}

impl Style {
    /// The initial values: what the root inherits.
    pub fn initial() -> Self {
        Self {
            fill: Paint::Color(ColorValue::Color(Color::BLACK)),
            fill_opacity: 1.0,
            fill_rule: FillRule::NonZero,
            stroke: Paint::None,
            stroke_width: Length::Px(1.0),
            stroke_linecap: LineCap::Butt,
            stroke_linejoin: LineJoin::Miter,
            stroke_miterlimit: 4.0,
            stroke_dasharray: Vec::new(),
            stroke_dashoffset: Length::ZERO,
            stroke_opacity: 1.0,
            color: Color::BLACK,
            visible: true,
            clip_rule: FillRule::NonZero,
            stroke_first: false,
            filters_linear: true,
            opacity: 1.0,
            display: true,
            clip_path: None,
            mask: None,
            filter: None,
            flood_color: ColorValue::Color(Color::BLACK),
            flood_opacity: 1.0,
            stop_color: ColorValue::Color(Color::BLACK),
            stop_opacity: 1.0,
            mask_alpha: false,
            non_scaling_stroke: false,
            text: TextStyle::initial(),
        }
    }

    /// The style of an element with `specified` declarations whose parent's
    /// style is `parent`: each property's highest-ranked valid declaration
    /// applies, and a property with none keeps its inherited or initial
    /// value.
    ///
    /// # Errors
    ///
    /// A value naming a feature this renderer does not draw (a filter
    /// function, a marker, a blend mode, crisp-edged rendering), or written
    /// in a form it does not evaluate, is unsupported.
    pub fn compute(
        specified: &Specified,
        parent: &Self,
        element: &str,
    ) -> Result<Self, ImageError> {
        let initial = Self::initial();
        // Non-inherited properties start from their initial values.
        let mut style = Self {
            opacity: initial.opacity,
            display: initial.display,
            clip_path: None,
            mask: None,
            filter: None,
            flood_color: initial.flood_color,
            flood_opacity: initial.flood_opacity,
            stop_color: initial.stop_color,
            stop_opacity: initial.stop_opacity,
            mask_alpha: initial.mask_alpha,
            non_scaling_stroke: initial.non_scaling_stroke,
            ..parent.clone()
        };
        let mut decided: Vec<Property> = Vec::new();
        for (property, value) in specified.values.iter().rev() {
            if !decided.contains(property)
                && style.apply(*property, value.trim(), parent, element)?
            {
                decided.push(*property);
            }
        }
        Ok(style)
    }

    /// Sets `property` from `value`: `true` if it applied, `false` if it is no
    /// value of the property, to be ignored.
    fn apply(
        &mut self,
        property: Property,
        value: &str,
        parent: &Self,
        element: &str,
    ) -> Result<bool, ImageError> {
        let refuse = || {
            unsupported(format_args!(
                "value {value:?} of {} on <{element}>",
                property_name(property)
            ))
        };
        match value {
            "inherit" => {
                self.inherit(property, parent);
                return Ok(true);
            }
            "initial" => {
                self.inherit(property, &Self::initial());
                return Ok(true);
            }
            // Inherited or initial, as this style starts.
            "unset" => return Ok(true),
            _ => {}
        }
        let lower = value.to_ascii_lowercase();
        if ["attr(", "calc(", "env(", "var("]
            .iter()
            .any(|function| lower.contains(function))
        {
            return Err(refuse());
        }
        // A value that did not parse: refused if it is in a form this
        // renderer does not draw, ignored if it is no value at all.
        let unparsed = |undrawn: bool| if undrawn { Err(refuse()) } else { Ok(false) };
        let opacity = || number_or_percent(value).map(|v| v.clamp(0.0, 1.0));
        let rule = || match value {
            "nonzero" => Some(FillRule::NonZero),
            "evenodd" => Some(FillRule::EvenOdd),
            _ => None,
        };
        // A reference to one element, or `none`; a function or a keyword in
        // its place is a feature this renderer does not draw.
        let reference = || func_iri(value).ok_or(is_identifier(value) || value.contains('('));
        match property {
            Property::Fill => match paint(value) {
                Some(paint) => self.fill = paint,
                None => return unparsed(undrawn_paint(value)),
            },
            Property::Stroke => match paint(value) {
                Some(paint) => self.stroke = paint,
                None => return unparsed(undrawn_paint(value)),
            },
            Property::FillOpacity => match opacity() {
                Some(opacity) => self.fill_opacity = opacity,
                None => return Ok(false),
            },
            Property::StrokeOpacity => match opacity() {
                Some(opacity) => self.stroke_opacity = opacity,
                None => return Ok(false),
            },
            Property::Opacity => match opacity() {
                Some(opacity) => self.opacity = opacity,
                None => return Ok(false),
            },
            Property::StopOpacity => match opacity() {
                Some(opacity) => self.stop_opacity = opacity,
                None => return Ok(false),
            },
            Property::FloodOpacity => match opacity() {
                Some(opacity) => self.flood_opacity = opacity,
                None => return Ok(false),
            },
            Property::FillRule => match rule() {
                Some(rule) => self.fill_rule = rule,
                None => return Ok(false),
            },
            Property::ClipRule => match rule() {
                Some(rule) => self.clip_rule = rule,
                None => return Ok(false),
            },
            Property::StrokeWidth => match length(value) {
                Some(Length::Px(width) | Length::Percent(width)) if width < 0.0 => {
                    return Ok(false);
                }
                Some(width) => self.stroke_width = width,
                None => return unparsed(undrawn_length(value)),
            },
            Property::StrokeLinecap => {
                self.stroke_linecap = match value {
                    "butt" => LineCap::Butt,
                    "round" => LineCap::Round,
                    "square" => LineCap::Square,
                    _ => return Ok(false),
                };
            }
            Property::StrokeLinejoin => {
                self.stroke_linejoin = match value {
                    "miter" => LineJoin::Miter,
                    "round" => LineJoin::Round,
                    "bevel" => LineJoin::Bevel,
                    _ => return unparsed(matches!(value, "miter-clip" | "arcs")),
                };
            }
            Property::StrokeMiterlimit => {
                // A limit below one is out of range and is read as given:
                // no miter is shorter than the stroke is wide, so it
                // bevels every corner, as resvg's clamp to one does.
                match number_prefix(value).filter(|(_, rest)| rest.trim().is_empty()) {
                    Some((limit, _)) => self.stroke_miterlimit = limit,
                    None => return Ok(false),
                }
            }
            Property::StrokeDasharray => {
                if value == "none" {
                    self.stroke_dasharray = Vec::new();
                } else {
                    let parts: Vec<&str> = value
                        .split(|c: char| c == ',' || c.is_ascii_whitespace())
                        .filter(|part| !part.is_empty())
                        .collect();
                    match parts
                        .iter()
                        .map(|part| length(part))
                        .collect::<Option<Vec<_>>>()
                    {
                        Some(dashes) => self.stroke_dasharray = dashes,
                        None => {
                            return unparsed(parts.iter().any(|part| undrawn_length(part)));
                        }
                    }
                }
            }
            Property::StrokeDashoffset => match length(value) {
                Some(offset) => self.stroke_dashoffset = offset,
                None => return unparsed(undrawn_length(value)),
            },
            Property::Color => match color::parse(value) {
                Some(ColorValue::Color(color)) => self.color = color,
                Some(ColorValue::CurrentColor) => self.color = parent.color,
                None => return unparsed(undrawn_color(value)),
            },
            Property::StopColor => match color::parse(value) {
                Some(color) => self.stop_color = color,
                None => return unparsed(undrawn_color(value)),
            },
            Property::FloodColor => match color::parse(value) {
                Some(color) => self.flood_color = color,
                None => return unparsed(undrawn_color(value)),
            },
            Property::ColorInterpolationFilters => {
                // `auto` leaves the choice to the renderer, and this one
                // takes the initial value, as resvg does.
                self.filters_linear = match value {
                    "auto" | "linearRGB" => true,
                    "sRGB" => false,
                    _ => return Ok(false),
                };
            }
            Property::Visibility => {
                self.visible = match value {
                    "visible" => true,
                    "hidden" | "collapse" => false,
                    _ => return Ok(false),
                };
            }
            Property::Display => {
                // Every display type but `none` renders an SVG element.
                self.display = match value {
                    "none" => false,
                    _ if !value.is_empty() && value.split_ascii_whitespace().all(is_identifier) => {
                        true
                    }
                    _ => return Ok(false),
                };
            }
            Property::ClipPath => match reference() {
                Ok(id) => self.clip_path = id,
                Err(undrawn) => return unparsed(undrawn),
            },
            // CSS's `mask` shorthand takes images, positions and modes, none
            // of which this renderer draws.
            Property::Mask => match func_iri(value) {
                Some(id) => self.mask = id,
                None => return Err(refuse()),
            },
            // A reference to one `filter` element; filter functions and
            // lists of filters are not drawn.
            Property::Filter => match reference() {
                Ok(id) => self.filter = id,
                Err(undrawn) => return unparsed(undrawn),
            },
            Property::MaskType => {
                self.mask_alpha = match value {
                    "luminance" => false,
                    "alpha" => true,
                    _ => return Ok(false),
                };
            }
            Property::PaintOrder => {
                let words: Vec<&str> = value.split_ascii_whitespace().collect();
                if words.is_empty()
                    || !words
                        .iter()
                        .all(|word| matches!(*word, "normal" | "fill" | "stroke" | "markers"))
                {
                    return Ok(false);
                }
                // Unlisted parts follow in their default order, so only
                // which of fill and stroke comes first matters.
                let fill = words.iter().position(|word| *word == "fill");
                let stroke = words.iter().position(|word| *word == "stroke");
                self.stroke_first = match (fill, stroke) {
                    (Some(fill), Some(stroke)) => stroke < fill,
                    (None, Some(_)) => true,
                    _ => false,
                };
            }
            Property::VectorEffect => {
                self.non_scaling_stroke = match value {
                    "none" => false,
                    "non-scaling-stroke" => true,
                    _ => {
                        return unparsed(matches!(
                            value,
                            "non-scaling-size" | "non-rotation" | "fixed-position"
                        ));
                    }
                };
            }
            // Features this renderer does not have: their initial value is
            // accepted, and nothing else that is a value of theirs.
            Property::Marker
            | Property::MarkerStart
            | Property::MarkerMid
            | Property::MarkerEnd => {
                if value != "none" {
                    return unparsed(func_iri(value).is_some() || value.starts_with("url("));
                }
            }
            Property::MixBlendMode => {
                if value != "normal" {
                    return unparsed(BLEND_MODES.contains(&value));
                }
            }
            Property::ShapeRendering => {
                if !matches!(value, "auto" | "geometricPrecision") {
                    return unparsed(matches!(value, "optimizeSpeed" | "crispEdges"));
                }
            }
            Property::Text(text) => {
                return self
                    .text
                    .apply(text, value, &parent.text)
                    .ok_or_else(refuse);
            }
        }
        Ok(true)
    }

    /// Takes `property`'s value from `parent`, for `inherit`.
    fn inherit(&mut self, property: Property, parent: &Self) {
        match property {
            Property::Fill => self.fill = parent.fill.clone(),
            Property::FillOpacity => self.fill_opacity = parent.fill_opacity,
            Property::FillRule => self.fill_rule = parent.fill_rule,
            Property::Stroke => self.stroke = parent.stroke.clone(),
            Property::StrokeWidth => self.stroke_width = parent.stroke_width,
            Property::StrokeLinecap => self.stroke_linecap = parent.stroke_linecap,
            Property::StrokeLinejoin => self.stroke_linejoin = parent.stroke_linejoin,
            Property::StrokeMiterlimit => self.stroke_miterlimit = parent.stroke_miterlimit,
            Property::StrokeDasharray => self.stroke_dasharray = parent.stroke_dasharray.clone(),
            Property::StrokeDashoffset => self.stroke_dashoffset = parent.stroke_dashoffset,
            Property::StrokeOpacity => self.stroke_opacity = parent.stroke_opacity,
            Property::Color => self.color = parent.color,
            Property::Visibility => self.visible = parent.visible,
            Property::ClipRule => self.clip_rule = parent.clip_rule,
            Property::PaintOrder => self.stroke_first = parent.stroke_first,
            Property::ColorInterpolationFilters => self.filters_linear = parent.filters_linear,
            Property::Opacity => self.opacity = parent.opacity,
            Property::Display => self.display = parent.display,
            Property::ClipPath => self.clip_path = parent.clip_path.clone(),
            Property::Mask => self.mask = parent.mask.clone(),
            Property::Filter => self.filter = parent.filter.clone(),
            Property::FloodColor => self.flood_color = parent.flood_color,
            Property::FloodOpacity => self.flood_opacity = parent.flood_opacity,
            Property::StopColor => self.stop_color = parent.stop_color,
            Property::StopOpacity => self.stop_opacity = parent.stop_opacity,
            Property::MaskType => self.mask_alpha = parent.mask_alpha,
            Property::VectorEffect => self.non_scaling_stroke = parent.non_scaling_stroke,
            Property::Marker
            | Property::MarkerStart
            | Property::MarkerMid
            | Property::MarkerEnd
            | Property::MixBlendMode
            | Property::ShapeRendering => {}
            Property::Text(text) => self.text.inherit(text, &parent.text),
        }
    }

    /// A colour value with `currentColor` resolved against this style.
    pub fn resolve(&self, value: ColorValue) -> Color {
        match value {
            ColorValue::Color(color) => color,
            ColorValue::CurrentColor => self.color,
        }
    }
}

/// The blend modes other than `normal`, which this renderer does not draw.
const BLEND_MODES: &[&str] = &[
    "color",
    "color-burn",
    "color-dodge",
    "darken",
    "difference",
    "exclusion",
    "hard-light",
    "hue",
    "lighten",
    "luminosity",
    "multiply",
    "overlay",
    "plus-darker",
    "plus-lighter",
    "saturation",
    "screen",
    "soft-light",
];

/// Whether `text` is a CSS identifier, a keyword.
fn is_identifier(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '-')
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// Whether a colour that did not parse is in a form this renderer does not
/// draw, a colour function other than `rgb()`, rather than no colour at all.
/// A word that names no colour is no colour, as it is to a browser.
fn undrawn_color(text: &str) -> bool {
    text.split_once('(').is_some_and(|(name, rest)| {
        is_identifier(name)
            && rest.ends_with(')')
            && !name.eq_ignore_ascii_case("rgb")
            && !name.eq_ignore_ascii_case("rgba")
    })
}

/// Whether a paint that did not parse is one this renderer does not draw: a
/// paint server in another file, a marker's or `use`'s context paint, or a
/// colour as [`undrawn_color`] has it.
fn undrawn_paint(text: &str) -> bool {
    match text.strip_prefix("url(") {
        Some(rest) => rest
            .split(')')
            .next()
            .is_some_and(|inner| iri(inner).is_none()),
        None => matches!(text, "context-fill" | "context-stroke") || undrawn_color(text),
    }
}

/// Whether a length that did not parse is a number in a unit this renderer
/// does not resolve, such as `em`, rather than no length at all.
fn undrawn_length(text: &str) -> bool {
    number_prefix(text.trim()).is_some_and(|(_, unit)| is_identifier(unit.trim_end()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::xml;

    fn document(text: &str) -> Document {
        xml::parse(text.as_bytes()).expect("parses")
    }

    /// The computed style of each element below the root, root first.
    fn styles(text: &str) -> Result<Vec<Style>, ImageError> {
        let document = document(text);
        let cascade = Cascade::new(&document)?;
        let mut out = Vec::new();
        fn walk(
            document: &Document,
            cascade: &Cascade,
            index: usize,
            parent: &Style,
            out: &mut Vec<Style>,
        ) -> Result<(), ImageError> {
            let element = &document.elements[index];
            let style = Style::compute(&cascade.specified(element)?, parent, &element.name)?;
            out.push(style.clone());
            for &child in &element.children {
                walk(document, cascade, child, &style, out)?;
            }
            Ok(())
        }
        walk(&document, &cascade, 0, &Style::initial(), &mut out)?;
        Ok(out)
    }

    /// **The cascade orders presentation attributes, then sheet rules by
    /// specificity, then the `style` attribute, then `!important`.**
    #[test]
    fn the_cascade_ranks_every_origin() {
        let styles = styles(
            r##"<svg xmlns="http://www.w3.org/2000/svg">
<style>rect { fill: #010101 } .a { fill: #020202 } #i { fill: #030303 } .b { stroke: #040404 !important }</style>
<rect fill="#ffffff"/>
<rect fill="#ffffff" class="a"/>
<rect fill="#ffffff" class="a" id="i"/>
<rect class="a" id="i" style="fill: #050505"/>
<rect class="b" style="stroke: #060606"/>
<rect style="fill: #070707 !important; fill: #080808"/>
</svg>"##,
        )
        .expect("styles");
        let fill = |style: &Style| match &style.fill {
            Paint::Color(ColorValue::Color(color)) => (color.r * 255.0).round() as u8,
            other => panic!("{other:?}"),
        };
        let stroke = |style: &Style| match &style.stroke {
            Paint::Color(ColorValue::Color(color)) => (color.r * 255.0).round() as u8,
            other => panic!("{other:?}"),
        };
        // styles[1] is the <style> element itself.
        assert_eq!(fill(&styles[2]), 1, "a type rule beats the attribute");
        assert_eq!(fill(&styles[3]), 2, "a class beats a type");
        assert_eq!(fill(&styles[4]), 3, "an id beats a class");
        assert_eq!(fill(&styles[5]), 5, "the style attribute beats the sheet");
        assert_eq!(
            stroke(&styles[6]),
            4,
            "!important beats the style attribute"
        );
        assert_eq!(
            fill(&styles[7]),
            7,
            "!important beats a later normal declaration"
        );
    }

    /// **Inherited properties pass down and others do not**; `inherit` takes
    /// the parent's value either way, and `currentColor` resolves against the
    /// element using it.
    #[test]
    fn inheritance_follows_each_property() {
        let styles = styles(
            r##"<svg xmlns="http://www.w3.org/2000/svg">
<g fill="red" opacity="0.5" color="#00ff00" stroke-width="3">
  <rect/>
  <rect opacity="inherit" fill="currentColor" color="#0000ff"/>
</g>
</svg>"##,
        )
        .expect("styles");
        let (group, plain, inheriting) = (&styles[1], &styles[2], &styles[3]);
        assert_eq!(group.opacity, 0.5);
        assert_eq!(plain.opacity, 1.0, "opacity is not inherited");
        assert_eq!(plain.fill, group.fill, "fill is");
        assert_eq!(plain.stroke_width, Length::Px(3.0));
        assert_eq!(inheriting.opacity, 0.5);
        assert_eq!(inheriting.fill, Paint::Color(ColorValue::CurrentColor));
        assert_eq!(
            inheriting.resolve(ColorValue::CurrentColor),
            Color::rgb8(0, 0, 255)
        );
    }

    /// **The filter properties parse and inherit as Filter Effects defines
    /// them**: `color-interpolation-filters` is inherited and starts as
    /// linearRGB, `flood-color` and `flood-opacity` and `filter` are not.
    #[test]
    fn filter_properties_follow_their_definitions() {
        let styles = styles(
            r##"<svg xmlns="http://www.w3.org/2000/svg">
<g style="color-interpolation-filters: sRGB; filter: url('#f')" flood-color="red" flood-opacity="0.5">
  <rect/>
  <rect color-interpolation-filters="auto" flood-color="currentColor" color="blue" flood-opacity="25%" filter="none"/>
</g>
</svg>"##,
        )
        .expect("styles");
        assert!(styles[0].filters_linear, "linearRGB at first");
        let (group, plain, own) = (&styles[1], &styles[2], &styles[3]);
        assert!(!group.filters_linear);
        assert_eq!(group.filter.as_deref(), Some("f"));
        assert_eq!(group.flood_opacity, 0.5);
        assert_eq!(group.flood_color, ColorValue::Color(Color::rgb8(255, 0, 0)));
        assert!(
            !plain.filters_linear,
            "color-interpolation-filters is inherited"
        );
        assert_eq!(plain.filter, None, "filter is not");
        assert_eq!(plain.flood_opacity, 1.0, "nor flood-opacity");
        assert_eq!(
            plain.flood_color,
            ColorValue::Color(Color::BLACK),
            "nor flood-color"
        );
        assert!(own.filters_linear, "auto is linearRGB");
        assert_eq!(own.filter, None);
        assert_eq!(own.flood_opacity, 0.25);
        assert_eq!(own.resolve(own.flood_color), Color::rgb8(0, 0, 255));
    }

    /// **Values parse to what they mean**: lengths in every unit, opacities
    /// as numbers or percentages, paint servers with fallbacks, dash arrays
    /// and paint order.
    #[test]
    fn values_parse_to_their_meaning() {
        assert_eq!(length("2in"), Some(Length::Px(192.0)));
        assert_eq!(length("72pt"), Some(Length::Px(96.0)));
        assert_eq!(length("1pc"), Some(Length::Px(16.0)));
        assert_eq!(length("25.4mm"), Some(Length::Px(96.0)));
        assert_eq!(length("50%"), Some(Length::Percent(50.0)));
        assert_eq!(length("3px"), Some(Length::Px(3.0)));
        assert_eq!(length("2em"), None);
        assert_eq!(Length::Percent(50.0).resolve(30.0), 15.0);
        assert_eq!(number_or_percent("40%"), Some(0.4));
        assert_eq!(
            paint("url(#g) #fff"),
            Some(Paint::Server {
                id: "g".to_owned(),
                fallback: Some(Box::new(Paint::Color(ColorValue::Color(Color::rgb8(
                    255, 255, 255
                )))))
            })
        );
        assert_eq!(paint("url(other.svg#g)"), None);
        let styles = styles(
            r##"<svg xmlns="http://www.w3.org/2000/svg"><path stroke-dasharray="1, 2 3" paint-order="stroke" fill-opacity="150%"/></svg>"##,
        )
        .expect("styles");
        assert_eq!(
            styles[1].stroke_dasharray,
            [Length::Px(1.0), Length::Px(2.0), Length::Px(3.0)]
        );
        assert!(styles[1].stroke_first);
        assert_eq!(styles[1].fill_opacity, 1.0);
    }

    /// **Values naming a feature this renderer does not draw, or written in
    /// a form it does not evaluate, are refused, and so are the properties
    /// refused by name.**
    #[test]
    fn what_is_not_drawn_is_refused() {
        let svg = |body: &str| format!(r#"<svg xmlns="http://www.w3.org/2000/svg">{body}</svg>"#);
        for body in [
            r#"<rect style="fill: hsl(0, 50%, 50%)"/>"#,
            r#"<rect stroke="context-stroke"/>"#,
            r#"<rect fill="url(other.svg#g)"/>"#,
            r#"<rect fill="var(--c, red)"/>"#,
            r#"<rect opacity="var(--o)"/>"#,
            r#"<rect stroke-width="calc(1px + 1px)"/>"#,
            r#"<rect stop-color="color-mix(in srgb, red, blue)"/>"#,
            r#"<rect filter="blur(2px)"/>"#,
            r#"<rect filter="url(#a) url(#b)"/>"#,
            r#"<rect clip-path="circle(5px)"/>"#,
            r#"<rect clip-path="border-box"/>"#,
            r#"<rect mask="url(#m) luminance"/>"#,
            r#"<rect marker-end="url(#m)"/>"#,
            r#"<rect style="mix-blend-mode: multiply"/>"#,
            r#"<rect vector-effect="non-scaling-size"/>"#,
            r#"<rect shape-rendering="crispEdges"/>"#,
            r#"<rect stroke-linejoin="miter-clip"/>"#,
            r#"<rect stroke-width="1em"/>"#,
            r#"<rect stroke-dasharray="1em, 2"/>"#,
            r#"<rect style="transform: rotate(3deg)"/>"#,
            r#"<rect style="transform-origin: 0 0"/>"#,
            r#"<rect style="r: 2px"/>"#,
            r#"<svg style="background-color: red"/>"#,
            r#"<style type="text/sass">a{}</style>"#,
            r#"<style media="print">a{}</style>"#,
        ] {
            assert!(
                matches!(styles(&svg(body)), Err(ImageError::Unsupported(_))),
                "{body}"
            );
        }
    }

    /// **A value that is no value of its property is ignored**: the next
    /// declaration down the cascade applies, else the inherited or initial
    /// value; **a property this renderer does not know is ignored**; and
    /// `initial` and `unset` mean what CSS says.
    #[test]
    fn invalid_values_and_unknown_properties_are_ignored() {
        let styles = styles(
            r##"<svg xmlns="http://www.w3.org/2000/svg">
<style>.c { fill: #0000ff }</style>
<g fill="#ff0000" stroke-width="3" opacity="0.5" stroke-linecap="round" paint-order="stroke">
  <rect fill="#f5f5f5;opacity:0.26"/>
  <rect class="c" style="fill: #nothex; fill"/>
  <rect stroke-width="-1" opacity="half"/>
  <rect fill="currentColor;fill-opacity:1;stroke:none" color="#00ff00"/>
  <rect fill-rule="even-odd" stroke-linecap="flat" visibility="no" display="!" color-interpolation-filters="optimizeSpeed" paint-order="fill fill2" mask-type="lum" stroke-dasharray="1,x" stroke-miterlimit="4px" vector-effect="scaling" mix-blend-mode="mixed" shape-rendering="smooth" marker-end="#m" stroke-linejoin="mitre" clip-path="#c" filter="#f" stroke="#12345"/>
  <rect fill="initial" stroke-width="unset" opacity="inherit"/>
  <rect class="c" style="fill: unset" stop-color="rgb(1, 2)" color="1abc" paint-order=""/>
  <rect display="none" style="display: 9lives"/>
  <rect class="c" style="fill: bleu" stroke="ButtonText" color="notacolour"/>
  <rect style="frobnicate: 1; -inkscape-stroke: none; line-height: 1.25; isolation: isolate" tyle="fill: #00ff00" adobe-blending-mode="normal"/>
</g>
</svg>"##,
        )
        .expect("styles");
        let red = Paint::Color(ColorValue::Color(Color::rgb8(255, 0, 0)));
        let group = &styles[2];
        assert_eq!(styles[3].fill, red, "an invalid fill inherits");
        assert_eq!(
            styles[4].fill,
            Paint::Color(ColorValue::Color(Color::rgb8(0, 0, 255))),
            "the sheet's fill, under an invalid style attribute"
        );
        assert_eq!(styles[5].stroke_width, Length::Px(3.0), "inherited");
        assert_eq!(styles[5].opacity, 1.0, "initial: opacity is not inherited");
        assert_eq!(styles[6].fill, red);
        let mut inherited = group.clone();
        inherited.opacity = 1.0;
        assert_eq!(styles[7], inherited, "every invalid value ignored");
        assert_eq!(
            styles[8].fill,
            Paint::Color(ColorValue::Color(Color::BLACK))
        );
        assert_eq!(styles[8].stroke_width, Length::Px(3.0));
        assert_eq!(styles[8].opacity, 0.5);
        assert_eq!(styles[9].fill, red, "unset beats the sheet, and inherits");
        assert_eq!(styles[9], inherited, "an invalid rgb(), colour and order");
        assert!(!styles[10].display, "the attribute under an invalid style");
        assert_eq!(
            styles[11].fill,
            Paint::Color(ColorValue::Color(Color::rgb8(0, 0, 255))),
            "a word that is no colour gives way to the sheet"
        );
        assert_eq!(styles[11].stroke, inherited.stroke, "a system colour");
        assert_eq!(styles[11].color, inherited.color);
        assert_eq!(styles[12], inherited, "unknown properties ignored");
    }

    /// **`vector-effect` is not inherited.**
    #[test]
    fn a_non_scaling_stroke_is_the_elements_own() {
        let styles = styles(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><g vector-effect="non-scaling-stroke"><rect/><rect vector-effect="inherit"/></g></svg>"#,
        )
        .expect("styles");
        assert!(styles[1].non_scaling_stroke);
        assert!(!styles[2].non_scaling_stroke);
        assert!(styles[3].non_scaling_stroke);
    }
}
