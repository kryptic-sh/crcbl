//! Path geometry: SVG 2's path-data grammar, the basic shapes as paths,
//! elliptical arcs as cubics, flattening for the stroker, and bounding boxes.
//!
//! # Errors in path data render up to the error
//!
//! SVG specifies that a path whose data goes wrong partway is drawn up to the
//! last command that parsed, and every renderer does that; it is the file's
//! defined meaning, not a feature left out, so it is not refused here.

use crate::raster::{FLATTEN_TOLERANCE, wang_segments};

/// A point in user space.
pub(super) type Vec2 = (f64, f64);

/// One path command, absolute.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Segment {
    MoveTo(Vec2),
    LineTo(Vec2),
    QuadTo(Vec2, Vec2),
    CubicTo(Vec2, Vec2, Vec2),
    Close,
}

/// A path: subpaths, each opened by a [`Segment::MoveTo`].
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Path {
    pub segments: Vec<Segment>,
}

/// A parsed number and what follows it, or `None` if `text` does not start
/// with one: `[+-]? (digits [. digits?] | . digits) ([eE] [+-]? digits)?`.
pub(super) fn number_prefix(text: &str) -> Option<(f64, &str)> {
    let bytes = text.as_bytes();
    let mut at = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        at += 1;
    }
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let whole = digits(at);
    at += whole;
    let mut fraction = 0;
    if bytes.get(at) == Some(&b'.') {
        fraction = digits(at + 1);
        if whole + fraction > 0 {
            at += 1 + fraction;
        }
    }
    if whole + fraction == 0 {
        return None;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        let mut exponent = at + 1;
        if matches!(bytes.get(exponent), Some(b'+' | b'-')) {
            exponent += 1;
        }
        let count = digits(exponent);
        if count > 0 {
            at = exponent + count;
        }
    }
    let value: f64 = text[..at].parse().ok()?;
    value.is_finite().then(|| (value, &text[at..]))
}

/// Whitespace and at most one comma, as between two numbers.
fn skip_separator(text: &str) -> &str {
    let text = text.trim_start();
    text.strip_prefix(',').unwrap_or(text).trim_start()
}

/// A list of numbers separated by whitespace or commas, all of it. `None`
/// if anything else is there.
pub(super) fn number_list(text: &str) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let (value, after) = number_prefix(rest)?;
        out.push(value);
        rest = after.trim_start();
        if let Some(after) = rest.strip_prefix(',') {
            rest = after.trim_start();
            if rest.is_empty() {
                return None;
            }
        }
    }
    Some(out)
}

/// The points of a `polyline` or `polygon`, up to the first that does not
/// parse, without an odd coordinate left over: SVG's error rule for them.
pub(super) fn points(text: &str) -> Vec<Vec2> {
    let mut values = Vec::new();
    let mut rest = text.trim_start();
    while let Some((value, after)) = number_prefix(rest) {
        values.push(value);
        rest = skip_separator(after);
    }
    values
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[x, y]| (x, y))
        .collect()
}

/// Parses path data, up to the first error.
pub(super) fn parse_data(text: &str) -> Path {
    let mut path = Path::default();
    let mut rest = text.trim_start();
    let mut current = (0.0, 0.0);
    let mut start = (0.0, 0.0);
    // The control point a following smooth command reflects, if the previous
    // command was the matching kind of curve.
    let mut last_cubic: Option<Vec2> = None;
    let mut last_quad: Option<Vec2> = None;
    let mut command: Option<u8> = None;
    let mut first = true;
    loop {
        rest = rest.trim_start();
        let Some(&next) = rest.as_bytes().first() else {
            break;
        };
        let letter = if next.is_ascii_alphabetic() {
            rest = rest[1..].trim_start();
            next
        } else {
            // A repeated command: after a moveto, it continues as a lineto.
            match command {
                Some(b'M') => b'L',
                Some(b'm') => b'l',
                Some(b'Z' | b'z') | None => break,
                Some(other) => other,
            }
        };
        if first && !matches!(letter, b'M' | b'm') {
            break;
        }
        first = false;
        command = Some(letter);
        let relative = letter.is_ascii_lowercase();
        let base = if relative { current } else { (0.0, 0.0) };
        let offset = |(x, y): Vec2| (x + base.0, y + base.1);
        let mut reflected_cubic = None;
        let mut reflected_quad = None;
        match letter.to_ascii_uppercase() {
            b'Z' => {
                path.segments.push(Segment::Close);
                current = start;
            }
            b'M' => {
                let Some((point, after)) = pair(rest) else {
                    break;
                };
                rest = after;
                current = offset(point);
                start = current;
                path.segments.push(Segment::MoveTo(current));
            }
            b'L' => {
                let Some((point, after)) = pair(rest) else {
                    break;
                };
                rest = after;
                current = offset(point);
                path.segments.push(Segment::LineTo(current));
            }
            b'H' => {
                let Some((x, after)) = number_prefix(rest) else {
                    break;
                };
                rest = skip_separator(after);
                current = (x + base.0, current.1);
                path.segments.push(Segment::LineTo(current));
            }
            b'V' => {
                let Some((y, after)) = number_prefix(rest) else {
                    break;
                };
                rest = skip_separator(after);
                current = (current.0, y + base.1);
                path.segments.push(Segment::LineTo(current));
            }
            b'C' => {
                let Some((c1, after)) = pair(rest) else { break };
                let Some((c2, after)) = pair(after) else {
                    break;
                };
                let Some((to, after)) = pair(after) else {
                    break;
                };
                rest = after;
                let (c1, c2, to) = (offset(c1), offset(c2), offset(to));
                path.segments.push(Segment::CubicTo(c1, c2, to));
                reflected_cubic = Some(c2);
                current = to;
            }
            b'S' => {
                let Some((c2, after)) = pair(rest) else { break };
                let Some((to, after)) = pair(after) else {
                    break;
                };
                rest = after;
                let c1 = last_cubic.map_or(current, |c| reflect(c, current));
                let (c2, to) = (offset(c2), offset(to));
                path.segments.push(Segment::CubicTo(c1, c2, to));
                reflected_cubic = Some(c2);
                current = to;
            }
            b'Q' => {
                let Some((control, after)) = pair(rest) else {
                    break;
                };
                let Some((to, after)) = pair(after) else {
                    break;
                };
                rest = after;
                let (control, to) = (offset(control), offset(to));
                path.segments.push(Segment::QuadTo(control, to));
                reflected_quad = Some(control);
                current = to;
            }
            b'T' => {
                let Some((to, after)) = pair(rest) else { break };
                rest = after;
                let control = last_quad.map_or(current, |c| reflect(c, current));
                let to = offset(to);
                path.segments.push(Segment::QuadTo(control, to));
                reflected_quad = Some(control);
                current = to;
            }
            b'A' => {
                let Some((arc, after)) = arc_arguments(rest) else {
                    break;
                };
                rest = after;
                let to = offset(arc.to);
                arc_to_cubics(
                    current,
                    arc.radii,
                    arc.rotation,
                    arc.large,
                    arc.sweep,
                    to,
                    &mut path.segments,
                );
                current = to;
            }
            _ => break,
        }
        last_cubic = reflected_cubic;
        last_quad = reflected_quad;
    }
    path
}

fn reflect(control: Vec2, about: Vec2) -> Vec2 {
    (2.0 * about.0 - control.0, 2.0 * about.1 - control.1)
}

/// Two numbers and the separators after them.
fn pair(text: &str) -> Option<(Vec2, &str)> {
    let (x, rest) = number_prefix(text.trim_start())?;
    let (y, rest) = number_prefix(skip_separator(rest))?;
    Some(((x, y), skip_separator(rest)))
}

struct ArcArguments {
    radii: Vec2,
    rotation: f64,
    large: bool,
    sweep: bool,
    to: Vec2,
}

/// An arc's seven arguments; its two flags are single characters that need
/// no separator after them.
fn arc_arguments(text: &str) -> Option<(ArcArguments, &str)> {
    let (radii, rest) = pair(text)?;
    let (rotation, rest) = number_prefix(rest)?;
    let rest = skip_separator(rest);
    fn flag(text: &str) -> Option<(bool, &str)> {
        match text.as_bytes().first()? {
            b'0' => Some((false, skip_separator(&text[1..]))),
            b'1' => Some((true, skip_separator(&text[1..]))),
            _ => None,
        }
    }
    let (large, rest) = flag(rest)?;
    let (sweep, rest) = flag(rest)?;
    let (to, rest) = pair(rest)?;
    Some((
        ArcArguments {
            radii,
            rotation,
            large,
            sweep,
            to,
        },
        rest,
    ))
}

/// Appends the elliptical arc from `from` to `to` as cubics of at most a
/// quarter turn each: the endpoint-to-centre conversion of SVG's
/// implementation notes (appendix B.2.4), with out-of-range radii scaled up
/// as they prescribe.
pub(super) fn arc_to_cubics(
    from: Vec2,
    radii: Vec2,
    rotation: f64,
    large: bool,
    sweep: bool,
    to: Vec2,
    out: &mut Vec<Segment>,
) {
    if from == to {
        return;
    }
    let (mut rx, mut ry) = (radii.0.abs(), radii.1.abs());
    if rx == 0.0 || ry == 0.0 {
        out.push(Segment::LineTo(to));
        return;
    }
    let (sin, cos) = rotation.to_radians().sin_cos();
    let (dx, dy) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
    let x1 = cos * dx + sin * dy;
    let y1 = -sin * dx + cos * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let root = lambda.sqrt();
        rx *= root;
        ry *= root;
    }
    let numerator = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coefficient = (numerator / denominator).max(0.0).sqrt();
    if large == sweep {
        coefficient = -coefficient;
    }
    let cx1 = coefficient * rx * y1 / ry;
    let cy1 = -coefficient * ry * x1 / rx;
    let centre = (
        cos * cx1 - sin * cy1 + (from.0 + to.0) / 2.0,
        sin * cx1 + cos * cy1 + (from.1 + to.1) / 2.0,
    );
    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let (ux, uy) = ((x1 - cx1) / rx, (y1 - cy1) / ry);
    let (vx, vy) = ((-x1 - cx1) / rx, (-y1 - cy1) / ry);
    let start = angle(1.0, 0.0, ux, uy);
    let mut sweep_angle = angle(ux, uy, vx, vy);
    if !sweep && sweep_angle > 0.0 {
        sweep_angle -= core::f64::consts::TAU;
    } else if sweep && sweep_angle < 0.0 {
        sweep_angle += core::f64::consts::TAU;
    }
    let pieces = (sweep_angle.abs() / core::f64::consts::FRAC_PI_2 - 1e-9)
        .ceil()
        .max(1.0);
    let step = sweep_angle / pieces;
    // The control-arm length that makes a cubic osculate a circular arc of
    // `step` radians at both ends.
    let arm = 4.0 / 3.0 * (step / 4.0).tan();
    let on_ellipse = |theta: f64| {
        let (s, c) = theta.sin_cos();
        let (x, y) = (rx * c, ry * s);
        (cos * x - sin * y + centre.0, sin * x + cos * y + centre.1)
    };
    let tangent = |theta: f64| {
        let (s, c) = theta.sin_cos();
        let (x, y) = (-rx * s, ry * c);
        (cos * x - sin * y, sin * x + cos * y)
    };
    let mut theta = start;
    let mut point = from;
    for piece in 0..pieces as usize {
        let next = theta + step;
        let end = if piece + 1 == pieces as usize {
            to
        } else {
            on_ellipse(next)
        };
        let (t0, t1) = (tangent(theta), tangent(next));
        out.push(Segment::CubicTo(
            (point.0 + arm * t0.0, point.1 + arm * t0.1),
            (end.0 - arm * t1.0, end.1 - arm * t1.1),
            end,
        ));
        theta = next;
        point = end;
    }
}

/// A rectangle, its corners rounded by `rx` and `ry` (already resolved and
/// clamped to half its sides), starting at its top edge and running
/// clockwise on screen, as SVG 2 defines it.
pub(super) fn rect(x: f64, y: f64, width: f64, height: f64, rx: f64, ry: f64) -> Path {
    let mut segments = Vec::new();
    if rx > 0.0 && ry > 0.0 {
        let corner = |from, to, out: &mut Vec<Segment>| {
            arc_to_cubics(from, (rx, ry), 0.0, false, true, to, out);
        };
        segments.push(Segment::MoveTo((x + rx, y)));
        segments.push(Segment::LineTo((x + width - rx, y)));
        corner((x + width - rx, y), (x + width, y + ry), &mut segments);
        segments.push(Segment::LineTo((x + width, y + height - ry)));
        corner(
            (x + width, y + height - ry),
            (x + width - rx, y + height),
            &mut segments,
        );
        segments.push(Segment::LineTo((x + rx, y + height)));
        corner((x + rx, y + height), (x, y + height - ry), &mut segments);
        segments.push(Segment::LineTo((x, y + ry)));
        corner((x, y + ry), (x + rx, y), &mut segments);
    } else {
        segments.push(Segment::MoveTo((x, y)));
        segments.push(Segment::LineTo((x + width, y)));
        segments.push(Segment::LineTo((x + width, y + height)));
        segments.push(Segment::LineTo((x, y + height)));
    }
    segments.push(Segment::Close);
    Path { segments }
}

/// An ellipse, starting at its rightmost point and running clockwise on
/// screen through four quarter arcs, as SVG 2 defines `circle` and
/// `ellipse`.
pub(super) fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Path {
    let mut segments = vec![Segment::MoveTo((cx + rx, cy))];
    let quarters = [(cx, cy + ry), (cx - rx, cy), (cx, cy - ry), (cx + rx, cy)];
    let mut from = (cx + rx, cy);
    for to in quarters {
        arc_to_cubics(from, (rx, ry), 0.0, false, true, to, &mut segments);
        from = to;
    }
    segments.push(Segment::Close);
    Path { segments }
}

/// A polyline, closed into a polygon if `close`.
pub(super) fn polyline(points: &[Vec2], close: bool) -> Path {
    let mut segments = Vec::with_capacity(points.len() + 1);
    for (index, &point) in points.iter().enumerate() {
        segments.push(if index == 0 {
            Segment::MoveTo(point)
        } else {
            Segment::LineTo(point)
        });
    }
    if close && !segments.is_empty() {
        segments.push(Segment::Close);
    }
    Path { segments }
}

/// One subpath flattened to lines.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Polyline {
    /// Its points, each with whether it lies inside a curve (where the
    /// stroker joins round, following the curve) rather than at a corner.
    pub points: Vec<(Vec2, bool)>,
    pub closed: bool,
}

/// `path` as polylines, every curve within `tolerance` of its chords.
///
/// A subpath that is a lone moveto is dropped; one whose segments have no
/// length keeps a single point, which a round or square cap still draws.
pub(super) fn flatten(path: &Path, tolerance: f64) -> Vec<Polyline> {
    let mut out: Vec<Polyline> = Vec::new();
    let mut current: Option<Polyline> = None;
    let mut point = (0.0, 0.0);
    let mut drawn = false;
    let finish = |polyline: Option<Polyline>, drawn: bool, out: &mut Vec<Polyline>| {
        if let Some(mut polyline) = polyline
            && drawn
        {
            polyline.points.dedup_by(|a, b| a.0 == b.0);
            out.push(polyline);
        }
    };
    for segment in &path.segments {
        match *segment {
            Segment::MoveTo(to) => {
                finish(current.take(), drawn, &mut out);
                current = Some(Polyline {
                    points: vec![(to, false)],
                    closed: false,
                });
                drawn = false;
                point = to;
            }
            Segment::Close => {
                if let Some(mut polyline) = current.take() {
                    polyline.closed = true;
                    let start = polyline.points[0].0;
                    finish(Some(polyline), true, &mut out);
                    // A segment after a close starts a new subpath at the old
                    // start.
                    current = Some(Polyline {
                        points: vec![(start, false)],
                        closed: false,
                    });
                    point = start;
                }
                drawn = false;
            }
            segment => {
                let polyline = current.get_or_insert_with(|| Polyline {
                    points: vec![(point, false)],
                    closed: false,
                });
                drawn = true;
                point = flatten_segment(point, segment, tolerance, &mut polyline.points);
            }
        }
    }
    finish(current, drawn, &mut out);
    out
}

/// Appends one drawing segment from `from` as points, returning where it
/// ends.
fn flatten_segment(
    from: Vec2,
    segment: Segment,
    tolerance: f64,
    out: &mut Vec<(Vec2, bool)>,
) -> Vec2 {
    let length = |(x, y): Vec2| (x * x + y * y).sqrt();
    let tolerance = tolerance as f32;
    match segment {
        Segment::LineTo(to) => {
            out.push((to, false));
            to
        }
        Segment::QuadTo(c, to) => {
            let second = length((from.0 - 2.0 * c.0 + to.0, from.1 - 2.0 * c.1 + to.1));
            let steps = wang_segments(2, second as f32, tolerance);
            for step in 1..steps {
                let t = f64::from(step) / f64::from(steps);
                let u = 1.0 - t;
                let at = |a: f64, b: f64, c: f64| u * u * a + 2.0 * u * t * b + t * t * c;
                out.push(((at(from.0, c.0, to.0), at(from.1, c.1, to.1)), true));
            }
            out.push((to, false));
            to
        }
        Segment::CubicTo(c1, c2, to) => {
            let second = length((from.0 - 2.0 * c1.0 + c2.0, from.1 - 2.0 * c1.1 + c2.1))
                .max(length((c1.0 - 2.0 * c2.0 + to.0, c1.1 - 2.0 * c2.1 + to.1)));
            let steps = wang_segments(3, second as f32, tolerance);
            for step in 1..steps {
                let t = f64::from(step) / f64::from(steps);
                let u = 1.0 - t;
                let at = |a: f64, b: f64, c: f64, d: f64| {
                    u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
                };
                out.push((
                    (at(from.0, c1.0, c2.0, to.0), at(from.1, c1.1, c2.1, to.1)),
                    true,
                ));
            }
            out.push((to, false));
            to
        }
        Segment::MoveTo(to) => to,
        Segment::Close => from,
    }
}

/// The tolerance, in user units, that keeps a flattening within the
/// rasteriser's own once `scale` maps it to pixels.
pub(super) fn user_tolerance(scale: f64) -> f64 {
    f64::from(FLATTEN_TOLERANCE) / scale.max(1e-9)
}

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The exact bounding box of a path's geometry (control points do not count,
/// curve extrema do), or `None` for a path with no points.
pub(super) fn bounding_box(path: &Path) -> Option<Rect> {
    let mut min = (f64::INFINITY, f64::INFINITY);
    let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut add = |(x, y): Vec2| {
        min = (min.0.min(x), min.1.min(y));
        max = (max.0.max(x), max.1.max(y));
    };
    let mut current = (0.0, 0.0);
    let mut start = (0.0, 0.0);
    for segment in &path.segments {
        match *segment {
            Segment::MoveTo(to) => {
                add(to);
                current = to;
                start = to;
            }
            Segment::LineTo(to) => {
                add(to);
                current = to;
            }
            Segment::QuadTo(c, to) => {
                for axis in [0, 1] {
                    let (p0, p1, p2) = (get(current, axis), get(c, axis), get(to, axis));
                    let denominator = p0 - 2.0 * p1 + p2;
                    if denominator != 0.0 {
                        let t = (p0 - p1) / denominator;
                        if t > 0.0 && t < 1.0 {
                            add(quad_at(current, c, to, t));
                        }
                    }
                }
                add(to);
                current = to;
            }
            Segment::CubicTo(c1, c2, to) => {
                for axis in [0, 1] {
                    let (p0, p1, p2, p3) = (
                        get(current, axis),
                        get(c1, axis),
                        get(c2, axis),
                        get(to, axis),
                    );
                    // The derivative's coefficients, divided by three.
                    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
                    let b = 2.0 * (p0 - 2.0 * p1 + p2);
                    let c = p1 - p0;
                    for t in quadratic_roots(a, b, c) {
                        if t > 0.0 && t < 1.0 {
                            add(cubic_at(current, c1, c2, to, t));
                        }
                    }
                }
                add(to);
                current = to;
            }
            Segment::Close => current = start,
        }
    }
    (min.0 <= max.0).then_some(Rect {
        x: min.0,
        y: min.1,
        width: max.0 - min.0,
        height: max.1 - min.1,
    })
}

fn get(point: Vec2, axis: usize) -> f64 {
    if axis == 0 { point.0 } else { point.1 }
}

fn quad_at(p0: Vec2, p1: Vec2, p2: Vec2, t: f64) -> Vec2 {
    let u = 1.0 - t;
    let at = |a: f64, b: f64, c: f64| u * u * a + 2.0 * u * t * b + t * t * c;
    (at(p0.0, p1.0, p2.0), at(p0.1, p1.1, p2.1))
}

fn cubic_at(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f64) -> Vec2 {
    let u = 1.0 - t;
    let at = |a: f64, b: f64, c: f64, d: f64| {
        u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
    };
    (at(p0.0, p1.0, p2.0, p3.0), at(p0.1, p1.1, p2.1, p3.1))
}

/// The real roots of `a t² + b t + c`.
fn quadratic_roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a.abs() < 1e-12 {
        return if b == 0.0 { Vec::new() } else { vec![-c / b] };
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return Vec::new();
    }
    let root = discriminant.sqrt();
    vec![(-b + root) / (2.0 * a), (-b - root) / (2.0 * a)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    /// **Numbers follow the grammar**: signs, fractions without a leading
    /// digit, exponents, and two numbers run together where the grammar
    /// separates them.
    #[test]
    fn numbers_follow_the_grammar() {
        assert_eq!(number_prefix("-1.5e2x"), Some((-150.0, "x")));
        assert_eq!(number_prefix(".5.5"), Some((0.5, ".5")));
        assert_eq!(number_prefix("+3."), Some((3.0, "")));
        assert_eq!(number_prefix("2e"), Some((2.0, "e")));
        assert_eq!(number_prefix("1e999"), None);
        assert_eq!(number_prefix("."), None);
        assert_eq!(number_prefix("inf"), None);
        assert_eq!(number_list("1,2 3 ,4"), Some(vec![1.0, 2.0, 3.0, 4.0]));
        assert_eq!(number_list("1,,2"), None);
        assert_eq!(number_list("1,"), None);
        assert_eq!(points("1,2 3,4 5"), vec![(1.0, 2.0), (3.0, 4.0)]);
        assert_eq!(points("1,2 3,x 5,6"), vec![(1.0, 2.0)]);
    }

    /// **Every command parses to its absolute geometry**: relative forms,
    /// implicit repeats, the horizontal and vertical lines, and the smooth
    /// curves' reflected control points.
    #[test]
    fn every_command_parses_to_absolute_segments() {
        use Segment::*;
        let path =
            parse_data("m1 2 3 4 h 1 V 0 l-1-1 z M10 10 c1 0 2 1 2 2 s2 2 3 3 Q20 20 21 21 t2 2");
        assert_eq!(
            path.segments,
            vec![
                MoveTo((1.0, 2.0)),
                LineTo((4.0, 6.0)),
                LineTo((5.0, 6.0)),
                LineTo((5.0, 0.0)),
                LineTo((4.0, -1.0)),
                Close,
                MoveTo((10.0, 10.0)),
                CubicTo((11.0, 10.0), (12.0, 11.0), (12.0, 12.0)),
                // The first control reflects (12, 11) about (12, 12).
                CubicTo((12.0, 13.0), (14.0, 14.0), (15.0, 15.0)),
                QuadTo((20.0, 20.0), (21.0, 21.0)),
                // The control reflects (20, 20) about (21, 21).
                QuadTo((22.0, 22.0), (23.0, 23.0)),
            ]
        );
        // A smooth curve after a non-curve reflects nothing: its first control
        // is the current point.
        assert_eq!(
            parse_data("M0 0 L1 1 S2 2 3 3").segments[2],
            CubicTo((1.0, 1.0), (2.0, 2.0), (3.0, 3.0))
        );
        // After a close, a relative command is relative to the subpath start.
        assert_eq!(
            parse_data("M5 5 l1 0 z l1 1").segments[3],
            LineTo((6.0, 6.0))
        );
    }

    /// **Path data that goes wrong is drawn up to the error**, and data not
    /// starting with a moveto draws nothing.
    #[test]
    fn errors_stop_the_path_where_they_are() {
        assert_eq!(parse_data("M0 0 L1 1 L2 x L3 3").segments.len(), 2);
        assert_eq!(parse_data("L1 1").segments, vec![]);
        assert_eq!(parse_data("M0 0 L1").segments.len(), 1);
        assert_eq!(parse_data("M0 0 X1 1").segments.len(), 1);
    }

    /// **An arc's cubics lie on its ellipse**: endpoints exact, midpoints of
    /// each piece within the cubic approximation's known error, and the flag
    /// combinations choosing the four different arcs. Flags need no
    /// separators.
    #[test]
    fn arcs_become_cubics_on_their_ellipse() {
        // A half circle of radius 5 about (5, 0), from (0, 0) to (10, 0).
        for (data, centre_y, below) in [
            ("M0 0 A5 5 0 0 1 10 0", 0.0, false),
            ("M0 0 A5 5 0 0 0 10 0", 0.0, true),
            ("M0 0a5,5,0,0,1,10,0", 0.0, false),
        ] {
            let path = parse_data(data);
            assert_eq!(path.segments.len(), 3, "{data}: two quarter pieces");
            let Segment::CubicTo(_, _, end) = path.segments[2] else {
                panic!("{data}");
            };
            assert!(close(end, (10.0, 0.0)));
            let Segment::CubicTo(c1, c2, mid) = path.segments[1] else {
                panic!("{data}");
            };
            let middle = cubic_at((0.0, 0.0), c1, c2, mid, 0.5);
            let radius = ((middle.0 - 5.0).powi(2) + (middle.1 - centre_y).powi(2)).sqrt();
            // A cubic quarter circle strays from the circle by at most 2.7e-4
            // of its radius.
            assert!((radius - 5.0).abs() < 5.0 * 3e-4, "{data}: {radius}");
            assert_eq!(mid.1 > 0.0, below, "{data}: sweep picks the side");
        }
        // Radii too small for the endpoints scale up until they fit: a radius
        // of 1 between points 10 apart becomes 5, a half circle again.
        let path = parse_data("M0 0 A1 1 0 0 1 10 0");
        let Segment::CubicTo(_, _, mid) = path.segments[1] else {
            panic!();
        };
        assert!(close(mid, (5.0, -5.0)), "{mid:?}");
        // The large-arc flag takes three quarters of a circle of radius 5
        // through (0, 0) and (5, 5) rather than one quarter.
        assert_eq!(parse_data("M0 0 A5 5 0 0 0 5 5").segments.len(), 2);
        assert_eq!(parse_data("M0 0 A5 5 0 1 0 5 5").segments.len(), 4);
        // A zero radius is a straight line.
        assert_eq!(
            parse_data("M0 0 A0 5 0 0 1 3 4").segments[1],
            Segment::LineTo((3.0, 4.0))
        );
    }

    /// **The bounding box is the geometry's, curve extrema included, control
    /// points excluded.**
    #[test]
    fn bounding_boxes_are_exact() {
        // y = 3t(1 - t)·4 peaks at t = ½ with y = 3; the controls at y = 4
        // must not count.
        let cubic = parse_data("M0 0 C0 4 10 4 10 0");
        assert_eq!(
            bounding_box(&cubic),
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 3.0
            })
        );
        let quad = parse_data("M0 0 Q5 10 10 0");
        let rect = bounding_box(&quad).expect("box");
        assert!((rect.height - 5.0).abs() < 1e-12);
        let circle = ellipse(1.0, 2.0, 3.0, 4.0);
        let rect = bounding_box(&circle).expect("box");
        assert!(
            close((rect.x, rect.y), (-2.0, -2.0)) && close((rect.width, rect.height), (6.0, 8.0))
        );
        assert_eq!(bounding_box(&Path::default()), None);
    }

    /// **Flattening keeps every curve within its tolerance, marks the points
    /// inside curves, and keeps a zero-length subpath as one point.**
    #[test]
    fn flattening_stays_within_tolerance() {
        let tolerance = 0.01;
        let polylines = flatten(&ellipse(0.0, 0.0, 10.0, 10.0), tolerance);
        assert_eq!(polylines.len(), 1);
        let polyline = &polylines[0];
        assert!(polyline.closed);
        for pair in polyline.points.windows(2) {
            let (a, b) = (pair[0].0, pair[1].0);
            let middle = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let radius = (middle.0 * middle.0 + middle.1 * middle.1).sqrt();
            // A chord's midpoint is the furthest it gets from the circle.
            assert!(10.0 - radius <= tolerance + 3e-3, "{radius}");
        }
        assert!(polyline.points.iter().filter(|p| p.1).count() > 8);
        assert!(!polyline.points[0].1);

        let dot = flatten(&parse_data("M3 3 L3 3"), tolerance);
        assert_eq!(dot.len(), 1);
        assert_eq!(dot[0].points, vec![((3.0, 3.0), false)]);
        assert_eq!(flatten(&parse_data("M3 3"), tolerance), vec![]);
        assert_eq!(flatten(&parse_data("M0 0 L1 0 Z L0 1"), tolerance).len(), 2);
    }
}
