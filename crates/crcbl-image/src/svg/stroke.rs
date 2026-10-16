//! Strokes turned into outlines that fill to the stroke under the non-zero
//! rule: dashes cut first, then each subpath offset by half the width on
//! both sides with joins between segments and caps at open ends.
//!
//! # One contour per side
//!
//! An open subpath becomes one contour: out along its left offset, round the
//! end cap, back along its right offset, round the start cap. A closed one
//! becomes two, its left offset forwards and its right offset backwards, so
//! the area between them winds once and the inside not at all. At a corner
//! the outer side gets the join, and the inner side turns where its two
//! offsets cross. Where they cross beyond the end of a segment too short for
//! them, it runs in to the corner point and out again instead, as Skia's
//! stroker always does: that keeps every point near the corner wound at
//! least once, but leaves slivers wound twice, which the rasteriser's area
//! integral counts double in a pixel the stroke's edge crosses. So it is the
//! fallback, not the rule.
//!
//! Points the flattener marked as lying inside a curve get a round join
//! whatever the stroke's own join is: the outline of a stroked curve is the
//! sweep of a disc along it, which round joins between its chords reproduce,
//! where a miter or bevel would put facets on it.
//!
//! The work happens in user space, before the transform, so a stroke under a
//! non-uniform scale or a skew comes out with the shape that transform gives
//! it rather than a constant width on screen.

use super::path::{Polyline, Vec2};
use super::style::{LineCap, LineJoin};
use crate::ImageError;

/// The most dashes one stroke is cut into: past it, a dash pattern far
/// finer than its path is long would only burn time.
pub(super) const MAX_DASHES: usize = 100_000;

/// How a stroke is drawn, every length in user units.
#[derive(Clone, Debug)]
pub(super) struct Stroke {
    pub width: f64,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f64,
    /// Alternating dash and gap lengths, empty for a solid stroke. Already
    /// doubled if it was given with an odd count.
    pub dashes: Vec<f64>,
    pub dash_offset: f64,
}

fn sub(a: Vec2, b: Vec2) -> Vec2 {
    (a.0 - b.0, a.1 - b.1)
}

fn add(a: Vec2, b: Vec2) -> Vec2 {
    (a.0 + b.0, a.1 + b.1)
}

fn scale(a: Vec2, s: f64) -> Vec2 {
    (a.0 * s, a.1 * s)
}

fn dot(a: Vec2, b: Vec2) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

fn cross(a: Vec2, b: Vec2) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

fn length(a: Vec2) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: Vec2) -> Vec2 {
    scale(a, 1.0 / length(a))
}

/// The left normal of a unit direction, `half` long.
fn normal(direction: Vec2, half: f64) -> Vec2 {
    (-direction.1 * half, direction.0 * half)
}

/// The closed outline contours of `polylines` stroked by `stroke`, flattened
/// to within `tolerance` user units.
///
/// # Errors
///
/// [`ImageError::Unsupported`] if dashing would cut more than [`MAX_DASHES`]
/// pieces.
pub(super) fn outline(
    polylines: &[Polyline],
    stroke: &Stroke,
    tolerance: f64,
) -> Result<Vec<Vec<Vec2>>, ImageError> {
    let mut contours = Vec::new();
    let half = stroke.width / 2.0;
    if half <= 0.0 || !half.is_finite() {
        return Ok(contours);
    }
    let pieces;
    let polylines = if stroke.dashes.is_empty() {
        polylines
    } else {
        pieces = dash(polylines, &stroke.dashes, stroke.dash_offset)?;
        &pieces[..]
    };
    let outliner = Outliner {
        stroke,
        half,
        tolerance,
    };
    for polyline in polylines {
        outliner.polyline(polyline, &mut contours);
    }
    Ok(contours)
}

struct Outliner<'a> {
    stroke: &'a Stroke,
    half: f64,
    tolerance: f64,
}

/// One segment of a polyline: its unit direction and its length.
#[derive(Clone, Copy)]
struct Edge {
    direction: Vec2,
    length: f64,
}

impl Edge {
    fn between(from: Vec2, to: Vec2) -> Self {
        let delta = sub(to, from);
        Self {
            direction: unit(delta),
            length: length(delta),
        }
    }

    fn reversed(self) -> Self {
        Self {
            direction: scale(self.direction, -1.0),
            ..self
        }
    }
}

impl Outliner<'_> {
    fn polyline(&self, polyline: &Polyline, contours: &mut Vec<Vec<Vec2>>) {
        let mut points = polyline.points.clone();
        // A repeated point is a segment with no direction.
        points.dedup_by(|a, b| a.0 == b.0);
        if polyline.closed && points.len() > 1 && points[0].0 == points[points.len() - 1].0 {
            points.pop();
        }
        if points.len() == 1 {
            self.dot(points[0].0, contours);
            return;
        }
        let count = points.len();
        let edges: Vec<Edge> = (0..count - usize::from(!polyline.closed))
            .map(|index| Edge::between(points[index].0, points[(index + 1) % count].0))
            .collect();
        if polyline.closed {
            // Forwards on the left, then backwards, which is the right.
            let mut left = Vec::new();
            for index in 0..count {
                let incoming = edges[(index + count - 1) % count];
                self.join(points[index], incoming, edges[index], &mut left);
            }
            let mut right = Vec::new();
            for index in (0..count).rev() {
                let incoming = edges[index].reversed();
                let outgoing = edges[(index + count - 1) % count].reversed();
                self.join(points[index], incoming, outgoing, &mut right);
            }
            contours.push(left);
            contours.push(right);
        } else {
            let last = count - 1;
            let mut contour = vec![add(points[0].0, normal(edges[0].direction, self.half))];
            for index in 1..last {
                self.join(points[index], edges[index - 1], edges[index], &mut contour);
            }
            self.cap(points[last].0, edges[last - 1].direction, &mut contour);
            for index in (1..last).rev() {
                self.join(
                    points[index],
                    edges[index].reversed(),
                    edges[index - 1].reversed(),
                    &mut contour,
                );
            }
            self.cap(points[0].0, edges[0].reversed().direction, &mut contour);
            contours.push(contour);
        }
    }

    /// The join at `point` on the left of a path arriving along `incoming`
    /// and leaving along `outgoing`.
    fn join(
        &self,
        (point, smooth): (Vec2, bool),
        incoming: Edge,
        outgoing: Edge,
        contour: &mut Vec<Vec2>,
    ) {
        let before = normal(incoming.direction, self.half);
        let after = normal(outgoing.direction, self.half);
        let turn = cross(incoming.direction, outgoing.direction);
        let cosine = dot(incoming.direction, outgoing.direction);
        if turn.abs() < 1e-12 && cosine > 0.0 {
            contour.push(add(point, after));
            return;
        }
        // Where the two offsets meet: the same point on the inside of a
        // corner as the miter's tip on the outside.
        let meeting = add(point, scale(add(before, after), 1.0 / (1.0 + cosine)));
        // A left turn puts this side on the inside of the corner.
        if turn > 0.0 {
            // The offsets meet `half · tan(φ/2)` back along each segment from
            // the corner, for the turn φ.
            let back = self.half * turn.abs() / (1.0 + cosine);
            if cosine > -1.0 && back <= incoming.length.min(outgoing.length) {
                contour.push(meeting);
            } else {
                contour.push(add(point, before));
                contour.push(point);
                contour.push(add(point, after));
            }
            return;
        }
        let join = if smooth {
            LineJoin::Round
        } else {
            self.stroke.join
        };
        contour.push(add(point, before));
        match join {
            LineJoin::Bevel => {}
            LineJoin::Round => {
                let angle = turn.atan2(cosine);
                self.arc(point, before, angle, contour);
            }
            LineJoin::Miter => {
                // The miter's length over the stroke width is 1 / sin(θ/2)
                // for an interior angle θ, which is 1 / cos(φ/2) for the
                // turn φ between the segments.
                let ratio = 1.0 / ((1.0 + cosine) / 2.0).max(0.0).sqrt();
                if ratio <= self.stroke.miter_limit {
                    contour.push(meeting);
                }
            }
        }
        contour.push(add(point, after));
    }

    /// The cap at the end `point` of a segment running along `direction`,
    /// from its left offset round to its right.
    fn cap(&self, point: Vec2, direction: Vec2, contour: &mut Vec<Vec2>) {
        let left = normal(direction, self.half);
        let right = scale(left, -1.0);
        contour.push(add(point, left));
        match self.stroke.cap {
            LineCap::Butt => {}
            LineCap::Square => {
                let ahead = scale(direction, self.half);
                contour.push(add(add(point, left), ahead));
                contour.push(add(add(point, right), ahead));
            }
            LineCap::Round => self.arc(point, left, -core::f64::consts::PI, contour),
        }
        contour.push(add(point, right));
    }

    /// A zero-length subpath: a disc for round caps, a square aligned with
    /// the user-space axes for square caps, and nothing for butt caps.
    fn dot(&self, point: Vec2, contours: &mut Vec<Vec<Vec2>>) {
        let h = self.half;
        match self.stroke.cap {
            LineCap::Butt => {}
            LineCap::Square => contours.push(vec![
                add(point, (-h, -h)),
                add(point, (h, -h)),
                add(point, (h, h)),
                add(point, (-h, h)),
            ]),
            LineCap::Round => {
                let mut contour = vec![add(point, (h, 0.0))];
                self.arc(point, (h, 0.0), core::f64::consts::TAU, &mut contour);
                contours.push(contour);
            }
        }
    }

    /// The points strictly between the ends of an arc about `centre`,
    /// starting at offset `from` and turning by `angle` radians, close enough
    /// to the circle for the tolerance.
    fn arc(&self, centre: Vec2, from: Vec2, angle: f64, contour: &mut Vec<Vec2>) {
        // A chord of a circle of radius r subtending δ strays r(1 - cos δ/2)
        // from it.
        let ratio = (1.0 - self.tolerance / self.half).clamp(-1.0, 1.0);
        let step = (2.0 * ratio.acos()).max(1e-3);
        let steps = (angle.abs() / step).ceil().min(1024.0) as usize;
        for index in 1..steps {
            let (sin, cos) = (angle * index as f64 / steps as f64).sin_cos();
            contour.push(add(
                centre,
                (from.0 * cos - from.1 * sin, from.0 * sin + from.1 * cos),
            ));
        }
    }
}

/// The dashes of `polylines`, each an open polyline, cut by the alternating
/// `pattern` starting `offset` into it. A dash running over the start of a
/// closed subpath continues into its first dash rather than ending in caps
/// there.
fn dash(polylines: &[Polyline], pattern: &[f64], offset: f64) -> Result<Vec<Polyline>, ImageError> {
    let period: f64 = pattern.iter().sum();
    let mut out = Vec::new();
    for polyline in polylines {
        let mut points = polyline.points.clone();
        if polyline.closed && points.len() > 1 {
            points.push((points[0].0, false));
        }
        let total: f64 = points
            .windows(2)
            .map(|pair| length(sub(pair[1].0, pair[0].0)))
            .sum();
        if total / period * pattern.len() as f64 > MAX_DASHES as f64 {
            return Err(ImageError::Unsupported(format!(
                "an SVG stroke cut into more than {MAX_DASHES} dashes"
            )));
        }
        // Where in the pattern the subpath starts.
        let mut index = 0;
        let mut remaining = pattern[0];
        let mut skip = offset.rem_euclid(period);
        while skip > 0.0 {
            if skip < remaining {
                remaining -= skip;
                break;
            }
            skip -= remaining;
            index = (index + 1) % pattern.len();
            remaining = pattern[index];
        }
        let starts_on = index % 2 == 0;
        let first_dash = out.len();
        let mut current: Option<Vec<(Vec2, bool)>> = starts_on.then(|| vec![(points[0].0, false)]);
        for pair in points.windows(2) {
            let (from, to) = (pair[0].0, pair[1].0);
            let segment = length(sub(to, from));
            let mut travelled = 0.0;
            while segment - travelled > remaining {
                travelled += remaining;
                let at = add(from, scale(sub(to, from), travelled / segment));
                if index % 2 == 0 {
                    let mut dash = current.take().unwrap_or_default();
                    dash.push((at, false));
                    out.push(Polyline {
                        points: dash,
                        closed: false,
                    });
                } else {
                    current = Some(vec![(at, false)]);
                }
                index = (index + 1) % pattern.len();
                remaining = pattern[index];
            }
            remaining -= segment - travelled;
            if let Some(dash) = &mut current {
                dash.push(pair[1]);
            }
        }
        if let Some(mut dash) = current {
            if polyline.closed && starts_on && out.len() > first_dash {
                // Joined with the first dash, through the start point.
                let first = out.remove(first_dash);
                dash.extend(first.points.into_iter().skip(1));
            }
            if let Some(last) = dash.last_mut() {
                last.1 = false;
            }
            out.push(Polyline {
                points: dash,
                closed: false,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::{FillRule, Point, Rasterizer};
    use crate::svg::path::{self, flatten};

    fn stroke(width: f64, cap: LineCap, join: LineJoin) -> Stroke {
        Stroke {
            width,
            cap,
            join,
            miter_limit: 4.0,
            dashes: Vec::new(),
            dash_offset: 0.0,
        }
    }

    /// The coverage of `contours` filled non-zero on a `side`-square mask.
    fn coverage(contours: &[Vec<Vec2>], side: u32) -> Vec<f32> {
        let mut rasterizer = Rasterizer::new(side, side);
        for contour in contours {
            for (index, &a) in contour.iter().enumerate() {
                let b = contour[(index + 1) % contour.len()];
                rasterizer.line(
                    Point::new(a.0 as f32, a.1 as f32),
                    Point::new(b.0 as f32, b.1 as f32),
                );
            }
        }
        rasterizer.coverage_by(FillRule::NonZero)
    }

    fn outlined(data: &str, stroke: &Stroke) -> Vec<Vec<Vec2>> {
        outline(&flatten(&path::parse_data(data), 0.01), stroke, 0.01).expect("outlines")
    }

    fn total(coverage: &[f32]) -> f32 {
        coverage.iter().sum()
    }

    /// **A butt-capped line covers exactly its rectangle**, and square caps
    /// add half the width at each end.
    #[test]
    fn caps_extend_a_line_as_specified() {
        let butt = coverage(
            &outlined("M2 5 L8 5", &stroke(2.0, LineCap::Butt, LineJoin::Miter)),
            10,
        );
        for y in 0..10 {
            for x in 0..10 {
                let want = if (2..8).contains(&x) && (4..6).contains(&y) {
                    1.0
                } else {
                    0.0
                };
                assert!((butt[y * 10 + x] - want).abs() < 1e-5, "({x}, {y})");
            }
        }
        let square = coverage(
            &outlined("M2 5 L8 5", &stroke(2.0, LineCap::Square, LineJoin::Miter)),
            10,
        );
        assert!((total(&square) - 8.0 * 2.0).abs() < 1e-4);
        // A round cap adds a half disc at each end: πr² in all.
        let round = coverage(
            &outlined("M2 5 L8 5", &stroke(2.0, LineCap::Round, LineJoin::Miter)),
            10,
        );
        let want = 6.0 * 2.0 + core::f32::consts::PI;
        // Arcs are drawn as inscribed polygons whose chords stray at most the
        // tolerance t from the circle, which loses at most ⅔·t of area per
        // unit of arc: ⅔ · 0.01 · 2π for the two half circles.
        let polygon_loss = 2.0 / 3.0 * 0.01 * core::f32::consts::TAU;
        let round_area = total(&round);
        assert!(
            round_area <= want + 1e-3 && round_area >= want - polygon_loss,
            "{round_area}"
        );
    }

    /// **Joins add what their geometry says at a right-angle corner**:
    /// nothing past the bevel, a square corner for the miter, a quarter disc
    /// for round; and a miter past its limit falls back to the bevel.
    #[test]
    fn joins_fill_the_corner_by_their_definition() {
        let corner = "M2 2 L8 2 L8 8";
        let area = |join, limit| {
            let mut s = stroke(2.0, LineCap::Butt, join);
            s.miter_limit = limit;
            total(&coverage(&outlined(corner, &s), 12))
        };
        // Two 6×2 arms overlapping in a 1×1 square at the inner corner, plus
        // the outer corner's 1×1 square: a bevel takes half of it.
        let arms = 6.0 * 2.0 * 2.0 - 1.0;
        assert!((area(LineJoin::Bevel, 4.0) - (arms + 0.5)).abs() < 1e-3);
        assert!((area(LineJoin::Miter, 4.0) - (arms + 1.0)).abs() < 1e-3);
        let quarter = core::f32::consts::PI / 4.0;
        assert!((area(LineJoin::Round, 4.0) - (arms + quarter)).abs() < 0.01);
        // A right angle's miter ratio is √2: a limit below it bevels.
        assert!((area(LineJoin::Miter, 1.4) - (arms + 0.5)).abs() < 1e-3);
    }

    /// **A closed subpath strokes as a ring**: a square outline covers the
    /// band between its offsets and leaves its middle empty.
    #[test]
    fn closed_subpaths_stroke_as_rings() {
        let ring = coverage(
            &outlined(
                "M2 2 H8 V8 H2 Z",
                &stroke(2.0, LineCap::Butt, LineJoin::Miter),
            ),
            10,
        );
        // 8×8 outside, 4×4 hole.
        assert!(
            (total(&ring) - (64.0 - 16.0)).abs() < 1e-3,
            "{}",
            total(&ring)
        );
        assert_eq!(ring[5 * 10 + 5], 0.0);
        assert!((ring[10 + 1] - 1.0).abs() < 1e-5);
    }

    /// **A circle's stroke is the annulus between its offset circles**, its
    /// curve points joined round whatever the stroke's join.
    #[test]
    fn a_stroked_circle_is_an_annulus() {
        let s = stroke(2.0, LineCap::Butt, LineJoin::Bevel);
        let contours = outline(
            &flatten(&path::ellipse(10.0, 10.0, 6.0, 6.0), 0.001),
            &s,
            0.001,
        )
        .expect("outlines");
        let area = total(&coverage(&contours, 20));
        let want = core::f32::consts::PI * (7.0 * 7.0 - 5.0 * 5.0);
        assert!((area - want).abs() < 0.05, "{area} vs {want}");
    }

    /// **Dashes cut the path by the pattern from the offset**, and a dash
    /// over the start of a closed subpath stays one piece.
    #[test]
    fn dashes_follow_the_pattern() {
        let mut s = stroke(2.0, LineCap::Butt, LineJoin::Miter);
        s.dashes = vec![2.0, 1.0];
        // 0..9 along y = 5: dashes at 0..2, 3..5, 6..8.
        let dashed = coverage(&outlined("M0 5 H9", &s), 10);
        let row: Vec<f32> = (0..10).map(|x| dashed[4 * 10 + x]).collect();
        assert_eq!(row, [1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0]);
        // An offset of 1 shifts the pattern back: 0..1, 2..4, 5..7, 8..9.
        s.dash_offset = 1.0;
        let shifted = coverage(&outlined("M0 5 H9", &s), 10);
        let row: Vec<f32> = (0..10).map(|x| shifted[4 * 10 + x]).collect();
        assert_eq!(row, [1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0]);

        // A 4×4 square of perimeter 16 with a pattern of 3 on, 1 off,
        // starting 1 before a dash ends: the last dash runs over the start
        // and joins the first.
        let square = path::parse_data("M0 0 H4 V4 H0 Z");
        let mut s = stroke(1.0, LineCap::Butt, LineJoin::Miter);
        s.dashes = vec![3.0, 1.0];
        s.dash_offset = 2.0;
        let pieces = dash(&flatten(&square, 0.01), &s.dashes, s.dash_offset).expect("dashes");
        assert_eq!(pieces.len(), 4);
        assert!(
            pieces
                .iter()
                .any(|piece| piece.points.contains(&((0.0, 0.0), false))
                    && piece.points.first() != Some(&((0.0, 0.0), false)))
        );
        // Outlined, each of the four dashes of 3 turns one corner, where the
        // miter adds what the inside of the turn overlaps: 4 · 3 · 1 in all,
        // the joined dash included. The path returns to its start before it
        // closes, as a circle's last arc does, so the joined dash passes
        // through that point twice.
        let ring = outlined("M2 2 H6 V6 H2 V2 Z", &s);
        assert_eq!(ring.len(), 4);
        let area = total(&coverage(&ring, 8));
        assert!((area - 12.0).abs() < 1e-3, "{area}");
    }

    /// **A curve strokes the same whatever the stroke's join**: the points
    /// flattening puts inside it join round, as the disc swept along a curve
    /// does, so a miter or a bevel there would put facets on it. Flattened
    /// coarsely, so the chords meet at angles a join would show.
    #[test]
    fn a_curve_strokes_the_same_whatever_its_join() {
        let polylines = flatten(&path::parse_data("M2 10 C2 2 18 2 18 10"), 0.2);
        assert!(polylines[0].points.iter().any(|point| point.1));
        let area = |join| {
            let contours =
                outline(&polylines, &stroke(4.0, LineCap::Butt, join), 0.01).expect("outlines");
            total(&coverage(&contours, 20))
        };
        let round = area(LineJoin::Round);
        assert_eq!(area(LineJoin::Miter), round);
        assert_eq!(area(LineJoin::Bevel), round);
    }

    /// **A pattern far finer than its path is refused**, not cut into
    /// millions of pieces.
    #[test]
    fn too_many_dashes_are_refused() {
        let mut s = stroke(1.0, LineCap::Butt, LineJoin::Miter);
        s.dashes = vec![0.001, 0.001];
        assert!(matches!(
            outline(&flatten(&path::parse_data("M0 0 H1000"), 0.01), &s, 0.01),
            Err(ImageError::Unsupported(_))
        ));
    }

    /// **Zero-length subpaths draw only round and square caps.**
    #[test]
    fn zero_length_subpaths_draw_their_caps() {
        let dot = |cap| {
            total(&coverage(
                &outlined("M5 5 Z", &stroke(4.0, cap, LineJoin::Miter)),
                10,
            ))
        };
        assert_eq!(dot(LineCap::Butt), 0.0);
        assert!((dot(LineCap::Square) - 16.0).abs() < 1e-4);
        // A disc of radius 2, less what its inscribed polygon loses: at most
        // ⅔ · 0.01 per unit of its 4π circumference.
        let disc = core::f32::consts::PI * 4.0;
        let round = dot(LineCap::Round);
        assert!(
            round <= disc + 1e-3 && round >= disc - 2.0 / 3.0 * 0.01 * 4.0 * core::f32::consts::PI,
            "{round}"
        );
    }
}
