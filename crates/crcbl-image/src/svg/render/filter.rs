//! Reading a `filter` element into a [`Plan`]: its region and units, and
//! each primitive's subregion, inputs and parameters, measured in the pixels
//! of the element it applies to. What the primitives compute is
//! [`crate::svg::filter`]'s.
//!
//! # Refused, not rendered, drawn
//!
//! Primitives outside the drop-shadow set, inputs other than the source
//! graphic, its alpha and earlier results (the background, the fill paint
//! and the stroke paint), filter functions and `href` between filters are
//! refused. What SVG 1.1 says is not rendered is not: an element whose
//! filter reference names nothing, a filter with no primitives or with an
//! empty region, and bounding-box units on an element with no box. An `in`
//! naming no earlier result is read as no `in` at all, as SVG says.
//!
//! # Pixels and transforms
//!
//! The region and the subregions are mapped to the canvas and rounded
//! outwards to whole pixels, and the region is cut to the canvas: what is
//! drawn outside the square is not drawn at all, so no filter can move it
//! in. A primitive's subregion defaults, as the specification says, to the
//! union of the subregions of the results it reads, or to the filter region
//! if it reads the source or nothing. Lengths go through the element's
//! transform: an offset as a vector, a blur's deviations as the spread its
//! Gaussian has along each pixel axis once transformed, which is exact for
//! scales, flips and quarter turns.

use super::{
    DESCRIPTIVE, Renderer, bounding_box_units, check_attributes, fraction, transform_box, union,
    units,
};
use crate::ImageError;
use crate::svg::canvas::Canvas;
use crate::svg::filter::{
    self, Area, IDENTITY_MATRIX, Input, Kind, LUMINANCE_TO_ALPHA, MAX_PRIMITIVES, Operator, Plan,
    Primitive,
};
use crate::svg::path::{Rect, number_list};
use crate::svg::transform::Transform;
use crate::svg::xml::{Element, Namespace};
use crate::svg::{malformed, unsupported};

impl Renderer<'_> {
    /// The filter `id` applied to an element whose user space maps to pixels
    /// by `transform` and whose object bounding box is `bbox`: the canvas
    /// pixels its region covers and its plan, or `None` where SVG says the
    /// element is not rendered.
    pub(super) fn filter_plan(
        &mut self,
        id: &str,
        transform: Transform,
        bbox: Option<Rect>,
    ) -> Result<Option<(Area, Plan)>, ImageError> {
        let Some(&index) = self.ids.get(id) else {
            return Ok(None);
        };
        let element = self.element(index);
        if !element.is("filter") {
            return Err(unsupported(format_args!("a filter of <{}>", element.name)));
        }
        check_attributes(element)?;
        if element
            .attribute(Namespace::None, "href")
            .or_else(|| element.attribute(Namespace::Xlink, "href"))
            .is_some()
        {
            return Err(unsupported("an href between filters"));
        }
        let object_units = units(element, "filterUnits", true)?;
        let primitive_units = if units(element, "primitiveUnits", false)? {
            match bounding_box_units(bbox) {
                Some(units) => Some(units),
                None => return Ok(None),
            }
        } else {
            None
        };
        let Some(rect) = self.effect_region(element, object_units, bbox)? else {
            return Ok(None);
        };
        // Written to catch a NaN too.
        if !(rect.width > 0.0 && rect.height > 0.0) {
            return Ok(None);
        }
        let region = self.pixel_area(rect, transform);
        let reader = Primitives {
            transform,
            region: rect,
            pixels: region,
            units: primitive_units,
        };
        let primitives = self.primitives(element, &reader)?;
        if primitives.is_empty() || region.x0 == region.x1 || region.y0 == region.y1 {
            return Ok(None);
        }
        Ok(Some((
            region,
            Plan {
                width: region.x1 - region.x0,
                height: region.y1 - region.y0,
                primitives,
            },
        )))
    }

    /// The canvas pixels `rect` covers once mapped by `transform`, rounded
    /// outwards and cut to the canvas.
    fn pixel_area(&self, rect: Rect, transform: Transform) -> Area {
        let mapped = transform_box(rect, transform);
        let side = f64::from(self.side);
        // `max` and `min` send a NaN to the bound, and `as` truncates what
        // is already whole.
        let edge = |value: f64| value.max(0.0).min(side) as usize;
        Area {
            x0: edge(mapped.x.floor()),
            y0: edge(mapped.y.floor()),
            x1: edge((mapped.x + mapped.width).ceil()),
            y1: edge((mapped.y + mapped.height).ceil()),
        }
    }

    /// The primitives of the filter `element`, in order.
    fn primitives(
        &mut self,
        element: &Element,
        reader: &Primitives,
    ) -> Result<Vec<Primitive>, ImageError> {
        let mut primitives = Vec::new();
        // Each primitive's `result` name and subregion, in user space.
        let mut names: Vec<Option<&str>> = Vec::new();
        let mut subregions: Vec<Rect> = Vec::new();
        let mut count = 0;
        let mut counted = || {
            count += 1;
            if count > MAX_PRIMITIVES {
                return Err(unsupported(format_args!(
                    "a filter of more than {MAX_PRIMITIVES} primitives"
                )));
            }
            Ok(())
        };
        for child in element.children.clone() {
            let primitive = self.element(child);
            if primitive.namespace != Namespace::Svg
                || DESCRIPTIVE.contains(&primitive.name.as_str())
            {
                continue;
            }
            if !matches!(
                primitive.name.as_str(),
                "feGaussianBlur"
                    | "feOffset"
                    | "feFlood"
                    | "feMerge"
                    | "feComposite"
                    | "feColorMatrix"
            ) {
                return Err(unsupported(format_args!(
                    "filter primitive <{}>",
                    primitive.name
                )));
            }
            check_attributes(primitive)?;
            counted()?;
            let style = self.document_style(child)?;
            let input = |name: &str| resolve_input(primitive, name, &names);
            let kind = match primitive.name.as_str() {
                "feGaussianBlur" => {
                    let (sx, sy) = match numbers(primitive, "stdDeviation")?.as_deref() {
                        None => (0.0, 0.0),
                        Some(&[s]) => (s, s),
                        Some(&[sx, sy]) => (sx, sy),
                        Some(_) => return Err(malformed_value(primitive, "stdDeviation")),
                    };
                    Kind::GaussianBlur {
                        input: input("in")?,
                        deviation: reader.deviation(sx, sy),
                    }
                }
                "feOffset" => {
                    let (dx, dy) =
                        reader.vector(number(primitive, "dx", 0.0)?, number(primitive, "dy", 0.0)?);
                    Kind::Offset {
                        input: input("in")?,
                        dx,
                        dy,
                    }
                }
                "feFlood" => {
                    let color = style
                        .resolve(style.flood_color)
                        .with_opacity(style.flood_opacity);
                    Kind::Flood([
                        color.r * color.a,
                        color.g * color.a,
                        color.b * color.a,
                        color.a,
                    ])
                }
                "feComposite" => Kind::Composite {
                    inputs: [input("in")?, input("in2")?],
                    operator: operator(primitive)?,
                },
                "feColorMatrix" => Kind::ColorMatrix {
                    input: input("in")?,
                    matrix: color_matrix(primitive)?,
                },
                _ => {
                    let mut inputs = Vec::new();
                    for node in primitive.children.iter().map(|&node| self.element(node)) {
                        if node.namespace != Namespace::Svg
                            || DESCRIPTIVE.contains(&node.name.as_str())
                        {
                            continue;
                        }
                        if node.name != "feMergeNode" {
                            return Err(unsupported(format_args!(
                                "element <{}> inside <feMerge>",
                                node.name
                            )));
                        }
                        check_attributes(node)?;
                        self.check_shape_children(node)?;
                        counted()?;
                        inputs.push(resolve_input(node, "in", &names)?);
                    }
                    Kind::Merge(inputs)
                }
            };
            if primitive.name != "feMerge" {
                self.check_shape_children(primitive)?;
            }
            let reads = kind.inputs();
            let default =
                if reads.is_empty() || reads.iter().any(|read| !matches!(read, Input::Result(_))) {
                    reader.region
                } else {
                    reads
                        .iter()
                        .filter_map(|read| match read {
                            Input::Result(from) => subregions.get(*from).copied(),
                            _ => None,
                        })
                        .fold(None, |total, one| union(total, Some(one)))
                        .unwrap_or(reader.region)
                };
            let subregion = self.subregion(primitive, reader, default)?;
            primitives.push(Primitive {
                kind,
                linear: style.filters_linear,
                area: reader.relative(self.pixel_area(subregion, reader.transform)),
            });
            names.push(
                primitive
                    .attribute(Namespace::None, "result")
                    .map(str::trim),
            );
            subregions.push(subregion);
        }
        Ok(primitives)
    }

    /// A primitive's subregion in user space: its `x`, `y`, `width` and
    /// `height` in the primitive units, each `default`'s where absent.
    fn subregion(
        &self,
        primitive: &Element,
        reader: &Primitives,
        default: Rect,
    ) -> Result<Rect, ImageError> {
        let rect = match reader.units {
            Some(units) => {
                let coordinate = |name: &str, origin: f64, size: f64, default: f64| {
                    Ok::<_, ImageError>(match primitive.attribute(Namespace::None, name) {
                        Some(_) => origin + fraction(primitive, name, 0.0)? * size,
                        None => default,
                    })
                };
                Rect {
                    x: coordinate("x", units.e, units.a, default.x)?,
                    y: coordinate("y", units.f, units.d, default.y)?,
                    width: coordinate("width", 0.0, units.a, default.width)?,
                    height: coordinate("height", 0.0, units.d, default.height)?,
                }
            }
            None => {
                let (w, h) = self.viewport;
                Rect {
                    x: self.length(primitive, "x", w, default.x)?,
                    y: self.length(primitive, "y", h, default.y)?,
                    width: self.length(primitive, "width", w, default.width)?,
                    height: self.length(primitive, "height", h, default.height)?,
                }
            }
        };
        if rect.width < 0.0 || rect.height < 0.0 {
            return Err(malformed(format_args!(
                "a negative width or height on <{}>",
                primitive.name
            )));
        }
        Ok(rect)
    }

    /// Replaces `layer` with `plan`'s result over the canvas pixels
    /// `region`, transparent outside it.
    pub(super) fn apply_filter(&self, region: Area, plan: &Plan, layer: &mut Canvas) {
        let side = self.side as usize;
        let source: Vec<_> = (region.y0..region.y1)
            .flat_map(|y| (region.x0..region.x1).map(move |x| y * side + x))
            .map(|index| layer.premultiplied(index))
            .collect();
        let result = filter::run(plan, &source);
        layer.replace(|index| {
            let (x, y) = (index % side, index / side);
            if (region.x0..region.x1).contains(&x) && (region.y0..region.y1).contains(&y) {
                result[(y - region.y0) * plan.width + (x - region.x0)]
            } else {
                [0.0; 4]
            }
        });
    }
}

/// What reading a filter's primitives needs of the filter.
struct Primitives {
    /// The element's user space to pixels.
    transform: Transform,
    /// The filter region in user space.
    region: Rect,
    /// The canvas pixels the region covers.
    pixels: Area,
    /// The map from the bounding box's unit square to user space, in
    /// `objectBoundingBox` primitive units; `None` in user units.
    units: Option<Transform>,
}

impl Primitives {
    /// A user-space horizontal and vertical length in the primitive units.
    fn lengths(&self, x: f64, y: f64) -> (f64, f64) {
        match self.units {
            Some(units) => (x * units.a, y * units.d),
            None => (x, y),
        }
    }

    /// A vector in the primitive units, in pixels.
    fn vector(&self, x: f64, y: f64) -> (f64, f64) {
        let (x, y) = self.lengths(x, y);
        let t = self.transform;
        (t.a * x + t.c * y, t.b * x + t.d * y)
    }

    /// `stdDeviation` in the primitive units, as the deviations in pixels
    /// along each axis of the Gaussian transformed. A negative deviation
    /// disables the blur, passing its input through, as Filter Effects
    /// says.
    fn deviation(&self, sx: f64, sy: f64) -> (f64, f64) {
        if sx < 0.0 || sy < 0.0 {
            return (0.0, 0.0);
        }
        let (sx, sy) = self.lengths(sx, sy);
        let t = self.transform;
        ((t.a * sx).hypot(t.c * sy), (t.b * sx).hypot(t.d * sy))
    }

    /// Canvas pixels as pixels of the filter region.
    fn relative(&self, area: Area) -> Area {
        let region = self.pixels;
        let x = |value: usize| value.clamp(region.x0, region.x1) - region.x0;
        let y = |value: usize| value.clamp(region.y0, region.y1) - region.y0;
        Area {
            x0: x(area.x0),
            y0: y(area.y0),
            x1: x(area.x1),
            y1: y(area.y1),
        }
    }
}

/// The input `element`'s attribute `name` names, given the `result` names
/// of the primitives before it: by default, or for a name none of them has,
/// the one just before, or the source graphic for the first.
fn resolve_input(
    element: &Element,
    name: &str,
    results: &[Option<&str>],
) -> Result<Input, ImageError> {
    let previous = match results.len() {
        0 => Input::SourceGraphic,
        count => Input::Result(count - 1),
    };
    Ok(
        match element.attribute(Namespace::None, name).map(str::trim) {
            None => previous,
            Some("SourceGraphic") => Input::SourceGraphic,
            Some("SourceAlpha") => Input::SourceAlpha,
            Some(other @ ("BackgroundImage" | "BackgroundAlpha" | "FillPaint" | "StrokePaint")) => {
                return Err(unsupported(format_args!("filter input {other}")));
            }
            Some(reference) => results
                .iter()
                .rposition(|result| *result == Some(reference))
                .map_or(previous, Input::Result),
        },
    )
}

/// `feComposite`'s `operator`, with `k1` to `k4` for `arithmetic`.
fn operator(element: &Element) -> Result<Operator, ImageError> {
    Ok(
        match element
            .attribute(Namespace::None, "operator")
            .map(str::trim)
        {
            None | Some("over") => Operator::Over,
            Some("in") => Operator::In,
            Some("out") => Operator::Out,
            Some("atop") => Operator::Atop,
            Some("xor") => Operator::Xor,
            Some("arithmetic") => {
                let k = |name: &str| number(element, name, 0.0).map(|k| k as f32);
                Operator::Arithmetic([k("k1")?, k("k2")?, k("k3")?, k("k4")?])
            }
            Some(other) => {
                return Err(unsupported(format_args!("feComposite operator {other:?}")));
            }
        },
    )
}

/// `feColorMatrix`'s `type` and `values` as the matrix they stand for.
fn color_matrix(element: &Element) -> Result<[f32; 20], ImageError> {
    let values = numbers(element, "values")?;
    let kind = element
        .attribute(Namespace::None, "type")
        .map_or("matrix", str::trim);
    Ok(match (kind, values.as_deref()) {
        ("matrix", None) => IDENTITY_MATRIX,
        ("matrix", Some(values)) if values.len() == IDENTITY_MATRIX.len() => {
            std::array::from_fn(|at| values[at] as f32)
        }
        ("saturate", None) => filter::saturate(1.0),
        ("saturate", Some(&[s])) => filter::saturate(s as f32),
        ("hueRotate", None) => filter::hue_rotate(0.0),
        ("hueRotate", Some(&[degrees])) => filter::hue_rotate(degrees as f32),
        ("luminanceToAlpha", _) => LUMINANCE_TO_ALPHA,
        ("matrix" | "saturate" | "hueRotate", Some(_)) => {
            return Err(malformed_value(element, "values"));
        }
        (other, _) => return Err(unsupported(format_args!("feColorMatrix type {other:?}"))),
    })
}

fn malformed_value(element: &Element, name: &str) -> ImageError {
    malformed(format_args!(
        "the {name} {:?} on <{}>",
        element.attribute(Namespace::None, name).unwrap_or(""),
        element.name
    ))
}

/// The attribute `name` as a list of finite numbers, `None` if absent.
fn numbers(element: &Element, name: &str) -> Result<Option<Vec<f64>>, ImageError> {
    let Some(text) = element.attribute(Namespace::None, name) else {
        return Ok(None);
    };
    match number_list(text) {
        Some(list) if list.iter().all(|value| value.is_finite()) => Ok(Some(list)),
        _ => Err(malformed_value(element, name)),
    }
}

/// The attribute `name` as one finite number, `default` if absent.
fn number(element: &Element, name: &str, default: f64) -> Result<f64, ImageError> {
    match numbers(element, name)?.as_deref() {
        None => Ok(default),
        Some(&[value]) => Ok(value),
        Some(_) => Err(malformed_value(element, name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(transform: Transform, units: Option<Transform>) -> Primitives {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        let pixels = Area {
            x0: 0,
            y0: 0,
            x1: 1,
            y1: 1,
        };
        Primitives {
            transform,
            region: rect,
            pixels,
            units,
        }
    }

    /// **A blur's deviations are the spread of its transformed Gaussian
    /// along each pixel axis**: under `skewX(45)`, `x' = x + y`, so
    /// deviations of 2 and 1 spread `√(2² + 1²)` across and 1 down; and an
    /// offset is the vector mapped.
    #[test]
    fn lengths_go_through_the_transform() {
        let skew = reader(Transform::new(1.0, 0.0, 1.0, 1.0, 5.0, 7.0), None);
        let (x, y) = skew.deviation(2.0, 1.0);
        assert!((x - 5f64.sqrt()).abs() < 1e-12, "{x}");
        assert!((y - 1.0).abs() < 1e-12, "{y}");
        assert_eq!(skew.vector(2.0, 1.0), (3.0, 1.0));
        // A quarter turn and a scale of two: x' = −2y, y' = x.
        let turned = reader(Transform::new(0.0, 1.0, -2.0, 0.0, 0.0, 0.0), None);
        assert_eq!(turned.deviation(2.0, 0.5), (1.0, 2.0));
        assert_eq!(turned.vector(1.0, 1.0), (-2.0, 1.0));
        // In bounding-box units a length is a fraction of the box's side.
        let boxed = reader(
            Transform::IDENTITY,
            Some(Transform::new(10.0, 0.0, 0.0, 4.0, 1.0, 1.0)),
        );
        assert_eq!(boxed.deviation(0.5, 0.5), (5.0, 2.0));
        assert_eq!(
            boxed.deviation(-0.5, 0.5),
            (0.0, 0.0),
            "negative passes through"
        );
    }
}
