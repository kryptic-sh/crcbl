//! Properties: which ones this renderer reads, which it ignores by design,
//! their values, and the cascade from presentation attributes, the
//! `<style>` sheet and the `style` attribute to each element's computed
//! style.
//!
//! # Ignored, supported, refused
//!
//! Every property name falls in one of three sets, and nothing falls through:
//!
//! * **Supported** ([`Property`]): parsed, and a value that does not parse,
//!   or that names a feature this renderer lacks, refuses the file.
//! * **Ignored by design** ([`IGNORED_PROPERTIES`]): properties with no
//!   effect on what this renderer can draw. Text properties (text itself is
//!   refused, so they can never take effect), rendering-quality hints that
//!   leave the geometry alone, properties of features that are refused where
//!   they would apply (filters, `solidColor`), and `isolation`, which only
//!   changes a picture that uses blend modes, which are refused. An editor's
//!   vendor-prefixed properties (`-inkscape-…`) are ignored the same way.
//! * **Everything else is refused**, so a property this list has never heard
//!   of fails loudly instead of being dropped.

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
    Display,
    Fill,
    FillOpacity,
    FillRule,
    Filter,
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
}

const PROPERTIES: &[(&str, Property)] = &[
    ("clip-path", Property::ClipPath),
    ("clip-rule", Property::ClipRule),
    ("color", Property::Color),
    ("display", Property::Display),
    ("fill", Property::Fill),
    ("fill-opacity", Property::FillOpacity),
    ("fill-rule", Property::FillRule),
    ("filter", Property::Filter),
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

/// Properties with no effect on anything this renderer draws; the module
/// docs say why each kind is here.
pub(super) const IGNORED_PROPERTIES: &[&str] = &[
    // Text, which is refused wherever it would render.
    "alignment-baseline",
    "baseline-shift",
    "block-progression",
    "direction",
    "dominant-baseline",
    "font",
    "font-family",
    "font-feature-settings",
    "font-kerning",
    "font-size",
    "font-size-adjust",
    "font-stretch",
    "font-style",
    "font-variant",
    "font-variant-alternates",
    "font-variant-caps",
    "font-variant-east-asian",
    "font-variant-east_asian",
    "font-variant-ligatures",
    "font-variant-numeric",
    "font-variant-position",
    "font-variation-settings",
    "font-weight",
    "glyph-orientation-horizontal",
    "glyph-orientation-vertical",
    "inline-size",
    "kerning",
    "letter-spacing",
    "line-height",
    "shape-inside",
    "shape-margin",
    "shape-padding",
    "shape-subtract",
    "text-align",
    "text-align-last",
    "text-anchor",
    "text-decoration",
    "text-decoration-color",
    "text-decoration-line",
    "text-decoration-style",
    "text-indent",
    "text-orientation",
    "text-transform",
    "unicode-bidi",
    "white-space",
    "word-spacing",
    "writing-mode",
    // Rendering hints that leave the geometry and colours alone.
    "color-interpolation",
    "color-profile",
    "color-rendering",
    "image-rendering",
    "text-rendering",
    // Properties of features refused where they would apply: filters,
    // lighting, the `solidColor` paint server, and nested viewports.
    "clip",
    "color-interpolation-filters",
    "enable-background",
    "flood-color",
    "flood-opacity",
    "lighting-color",
    "overflow",
    "solid-color",
    "solid-opacity",
    // Only a picture with blend modes looks different isolated, and blend
    // modes other than `normal` are refused.
    "isolation",
    // Interaction, which a still image has none of.
    "cursor",
    "pointer-events",
    // Inkscape's own, written without a vendor prefix by its older
    // versions and read by nothing else.
    "filter-blend-mode",
    "filter-gaussianblur-deviation",
];

/// What a property name means here.
enum Known {
    Supported(Property),
    Ignored,
}

fn known(name: &str) -> Option<Known> {
    if let Some(&(_, property)) = PROPERTIES.iter().find(|(known, _)| *known == name) {
        return Some(Known::Supported(property));
    }
    (IGNORED_PROPERTIES.contains(&name) || name.starts_with('-')).then_some(Known::Ignored)
}

/// Whether `name`, as an attribute, is a presentation attribute this module
/// handles (supported or ignored) rather than one the element must know.
pub(super) fn is_presentation_attribute(name: &str) -> bool {
    !name.starts_with('-') && known(name).is_some()
}

/// One element's declared values for the supported properties, cascaded:
/// the winning declaration for each.
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

    /// `element`'s specified values: presentation attributes, then the
    /// sheet's rules by specificity and order, then its `style` attribute,
    /// then `!important` declarations in the same order.
    ///
    /// # Errors
    ///
    /// A property neither supported nor ignored is unsupported.
    pub fn specified(&self, element: &Element) -> Result<Specified, ImageError> {
        // The highest-ranked declaration of each property wins.
        let mut ranked: Vec<(Rank, Declaration)> = Vec::new();
        for (order, attribute) in element.attributes.iter().enumerate() {
            if attribute.namespace == Namespace::None && is_presentation_attribute(&attribute.name)
            {
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
            match known(&declaration.name) {
                Some(Known::Supported(property)) => {
                    specified.values.retain(|(known, _)| *known != property);
                    specified.values.push((property, declaration.value));
                }
                Some(Known::Ignored) => {}
                None => {
                    return Err(unsupported(format_args!(
                        "property {} on <{}>",
                        declaration.name, element.name
                    )));
                }
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
    // Not inherited.
    pub opacity: f32,
    pub display: bool,
    pub clip_path: Option<String>,
    pub mask: Option<String>,
    pub stop_color: ColorValue,
    pub stop_opacity: f32,
    /// `mask-type: alpha` rather than luminance.
    pub mask_alpha: bool,
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
            opacity: 1.0,
            display: true,
            clip_path: None,
            mask: None,
            stop_color: ColorValue::Color(Color::BLACK),
            stop_opacity: 1.0,
            mask_alpha: false,
        }
    }

    /// The style of an element with `specified` values whose parent's style
    /// is `parent`.
    ///
    /// # Errors
    ///
    /// A value that does not parse, or that names an unsupported feature
    /// (a filter, a marker, a blend mode, a non-scaling stroke, crisp-edged
    /// rendering), is unsupported.
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
            stop_color: initial.stop_color,
            stop_opacity: initial.stop_opacity,
            mask_alpha: initial.mask_alpha,
            ..parent.clone()
        };
        for (property, value) in &specified.values {
            let value = value.trim();
            let refuse = || {
                unsupported(format_args!(
                    "value {value:?} of {} on <{element}>",
                    PROPERTIES
                        .iter()
                        .find(|(_, known)| known == property)
                        .map_or("?", |(name, _)| name)
                ))
            };
            if value == "inherit" {
                style.inherit(*property, parent);
                continue;
            }
            let opacity = || {
                number_or_percent(value)
                    .map(|v| v.clamp(0.0, 1.0))
                    .ok_or_else(refuse)
            };
            let rule = || match value {
                "nonzero" => Ok(FillRule::NonZero),
                "evenodd" => Ok(FillRule::EvenOdd),
                _ => Err(refuse()),
            };
            match property {
                Property::Fill => style.fill = paint(value).ok_or_else(refuse)?,
                Property::Stroke => style.stroke = paint(value).ok_or_else(refuse)?,
                Property::FillOpacity => style.fill_opacity = opacity()?,
                Property::StrokeOpacity => style.stroke_opacity = opacity()?,
                Property::Opacity => style.opacity = opacity()?,
                Property::StopOpacity => style.stop_opacity = opacity()?,
                Property::FillRule => style.fill_rule = rule()?,
                Property::ClipRule => style.clip_rule = rule()?,
                Property::StrokeWidth => {
                    style.stroke_width = length(value)
                        .filter(|length| !matches!(length, Length::Px(v) | Length::Percent(v) if *v < 0.0))
                        .ok_or_else(refuse)?;
                }
                Property::StrokeLinecap => {
                    style.stroke_linecap = match value {
                        "butt" => LineCap::Butt,
                        "round" => LineCap::Round,
                        "square" => LineCap::Square,
                        _ => return Err(refuse()),
                    };
                }
                Property::StrokeLinejoin => {
                    style.stroke_linejoin = match value {
                        "miter" => LineJoin::Miter,
                        "round" => LineJoin::Round,
                        "bevel" => LineJoin::Bevel,
                        _ => return Err(refuse()),
                    };
                }
                Property::StrokeMiterlimit => {
                    // A limit below one is out of range and is read as given:
                    // no miter is shorter than the stroke is wide, so it
                    // bevels every corner, as resvg's clamp to one does.
                    style.stroke_miterlimit = number_prefix(value)
                        .filter(|(_, rest)| rest.trim().is_empty())
                        .map(|(v, _)| v)
                        .ok_or_else(refuse)?;
                }
                Property::StrokeDasharray => {
                    style.stroke_dasharray = if value == "none" {
                        Vec::new()
                    } else {
                        value
                            .split(|c: char| c == ',' || c.is_ascii_whitespace())
                            .filter(|part| !part.is_empty())
                            .map(length)
                            .collect::<Option<Vec<_>>>()
                            .ok_or_else(refuse)?
                    };
                }
                Property::StrokeDashoffset => {
                    style.stroke_dashoffset = length(value).ok_or_else(refuse)?
                }
                Property::Color => {
                    style.color = match color::parse(value).ok_or_else(refuse)? {
                        ColorValue::Color(color) => color,
                        ColorValue::CurrentColor => parent.color,
                    };
                }
                Property::StopColor => style.stop_color = color::parse(value).ok_or_else(refuse)?,
                Property::Visibility => {
                    style.visible = match value {
                        "visible" => true,
                        "hidden" | "collapse" => false,
                        _ => return Err(refuse()),
                    };
                }
                Property::Display => {
                    style.display = match value {
                        "none" => false,
                        "inline" | "block" | "inline-block" => true,
                        _ => return Err(refuse()),
                    };
                }
                Property::ClipPath => style.clip_path = func_iri(value).ok_or_else(refuse)?,
                Property::Mask => style.mask = func_iri(value).ok_or_else(refuse)?,
                Property::MaskType => {
                    style.mask_alpha = match value {
                        "luminance" => false,
                        "alpha" => true,
                        _ => return Err(refuse()),
                    };
                }
                Property::PaintOrder => {
                    let words: Vec<&str> = value.split_ascii_whitespace().collect();
                    if !words
                        .iter()
                        .all(|word| matches!(*word, "normal" | "fill" | "stroke" | "markers"))
                    {
                        return Err(refuse());
                    }
                    // Unlisted parts follow in their default order, so only
                    // which of fill and stroke comes first matters.
                    let fill = words.iter().position(|word| *word == "fill");
                    let stroke = words.iter().position(|word| *word == "stroke");
                    style.stroke_first = match (fill, stroke) {
                        (Some(fill), Some(stroke)) => stroke < fill,
                        (None, Some(_)) => true,
                        _ => false,
                    };
                }
                // Features this renderer does not have: their initial value
                // is accepted, and nothing else.
                Property::Filter
                | Property::Marker
                | Property::MarkerStart
                | Property::MarkerMid
                | Property::MarkerEnd => {
                    if value != "none" {
                        return Err(refuse());
                    }
                }
                Property::MixBlendMode => {
                    if value != "normal" {
                        return Err(refuse());
                    }
                }
                Property::VectorEffect => {
                    if value != "none" {
                        return Err(refuse());
                    }
                }
                Property::ShapeRendering => {
                    if !matches!(value, "auto" | "geometricPrecision") {
                        return Err(refuse());
                    }
                }
            }
        }
        Ok(style)
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
            Property::Opacity => self.opacity = parent.opacity,
            Property::Display => self.display = parent.display,
            Property::ClipPath => self.clip_path = parent.clip_path.clone(),
            Property::Mask => self.mask = parent.mask.clone(),
            Property::StopColor => self.stop_color = parent.stop_color,
            Property::StopOpacity => self.stop_opacity = parent.stop_opacity,
            Property::MaskType => self.mask_alpha = parent.mask_alpha,
            Property::Filter
            | Property::Marker
            | Property::MarkerStart
            | Property::MarkerMid
            | Property::MarkerEnd
            | Property::MixBlendMode
            | Property::VectorEffect
            | Property::ShapeRendering => {}
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

    /// **Unknown properties, unsupported values and unsupported features are
    /// refused; ignored properties are not.**
    #[test]
    fn what_is_not_supported_is_refused() {
        let svg = |body: &str| format!(r#"<svg xmlns="http://www.w3.org/2000/svg">{body}</svg>"#);
        for body in [
            r#"<rect style="fill: hsl(0, 50%, 50%)"/>"#,
            r#"<rect filter="url(#blur)"/>"#,
            r#"<rect marker-end="url(#m)"/>"#,
            r#"<rect style="mix-blend-mode: multiply"/>"#,
            r#"<rect vector-effect="non-scaling-stroke"/>"#,
            r#"<rect shape-rendering="crispEdges"/>"#,
            r#"<rect stroke-linejoin="miter-clip"/>"#,
            r#"<rect style="transform: rotate(3deg)"/>"#,
            r#"<rect style="frobnicate: 1"/>"#,
            r#"<rect stroke-width="1em"/>"#,
            r#"<rect stroke-width="-1"/>"#,
            r#"<style type="text/sass">a{}</style>"#,
            r#"<style media="print">a{}</style>"#,
        ] {
            assert!(
                matches!(styles(&svg(body)), Err(ImageError::Unsupported(_))),
                "{body}"
            );
        }
        let ignored = styles(&svg(
            r#"<rect style="font-family: Sans; -inkscape-stroke: none; isolation: isolate; enable-background: accumulate" color-rendering="auto"/>"#,
        ))
        .expect("ignored properties are not refused");
        assert_eq!(ignored[1], Style::initial());
    }
}
