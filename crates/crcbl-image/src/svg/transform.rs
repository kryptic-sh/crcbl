//! Affine transforms and the `transform` attribute's list of them.

use super::path::number_list;

/// An affine map `(x, y) ↦ (a x + c y + e, b x + d y + f)`: SVG's
/// `matrix(a b c d e f)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Transform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Transform {
    pub const IDENTITY: Self = Self::new(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);

    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Self { a, b, c, d, e, f }
    }

    pub const fn translate(x: f64, y: f64) -> Self {
        Self::new(1.0, 0.0, 0.0, 1.0, x, y)
    }

    pub const fn scale(x: f64, y: f64) -> Self {
        Self::new(x, 0.0, 0.0, y, 0.0, 0.0)
    }

    /// A rotation by `degrees`, clockwise on screen (y grows downwards).
    pub fn rotate(degrees: f64) -> Self {
        let (sin, cos) = degrees.to_radians().sin_cos();
        Self::new(cos, sin, -sin, cos, 0.0, 0.0)
    }

    /// `self` applied after `inner`: a point goes through `inner` first.
    #[must_use]
    pub fn then(self, inner: Self) -> Self {
        Self::new(
            self.a * inner.a + self.c * inner.b,
            self.b * inner.a + self.d * inner.b,
            self.a * inner.c + self.c * inner.d,
            self.b * inner.c + self.d * inner.d,
            self.a * inner.e + self.c * inner.f + self.e,
            self.b * inner.e + self.d * inner.f + self.f,
        )
    }

    pub fn apply(self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    pub fn determinant(self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// The inverse, or `None` if the map collapses the plane onto a line.
    pub fn invert(self) -> Option<Self> {
        let det = self.determinant();
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Self::new(
            a,
            b,
            c,
            d,
            -(a * self.e + c * self.f),
            -(b * self.e + d * self.f),
        ))
    }

    /// The largest factor the map stretches any length by: the larger
    /// singular value of its linear part.
    pub fn max_scale(self) -> f64 {
        let p = self.a * self.a + self.b * self.b;
        let q = self.c * self.c + self.d * self.d;
        let r = self.a * self.c + self.b * self.d;
        let half_sum = 0.5 * (p + q);
        let root = (0.25 * (p - q) * (p - q) + r * r).sqrt();
        (half_sum + root).sqrt()
    }

    pub fn is_finite(self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .iter()
            .all(|v| v.is_finite())
    }
}

/// Parses a `transform` attribute: a list of `matrix`, `translate`, `scale`,
/// `rotate`, `skewX` and `skewY`, applied right to left as SVG composes them.
/// `None` if any of it does not parse.
pub(super) fn parse(text: &str) -> Option<Transform> {
    let mut total = Transform::IDENTITY;
    let mut rest = text.trim_start_matches(is_separator);
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = rest[..open].trim();
        let close = rest[open..].find(')')? + open;
        let arguments = number_list(&rest[open + 1..close])?;
        let one = match (name, arguments.as_slice()) {
            ("matrix", &[a, b, c, d, e, f]) => Transform::new(a, b, c, d, e, f),
            ("translate", &[x]) => Transform::translate(x, 0.0),
            ("translate", &[x, y]) => Transform::translate(x, y),
            ("scale", &[s]) => Transform::scale(s, s),
            ("scale", &[x, y]) => Transform::scale(x, y),
            ("rotate", &[angle]) => Transform::rotate(angle),
            ("rotate", &[angle, x, y]) => Transform::translate(x, y)
                .then(Transform::rotate(angle))
                .then(Transform::translate(-x, -y)),
            ("skewX", &[angle]) => {
                Transform::new(1.0, 0.0, angle.to_radians().tan(), 1.0, 0.0, 0.0)
            }
            ("skewY", &[angle]) => {
                Transform::new(1.0, angle.to_radians().tan(), 0.0, 1.0, 0.0, 0.0)
            }
            _ => return None,
        };
        total = total.then(one);
        rest = rest[close + 1..].trim_start_matches(is_separator);
    }
    total.is_finite().then_some(total)
}

fn is_separator(c: char) -> bool {
    c.is_ascii_whitespace() || c == ','
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    /// **Each transform function moves a point where its definition says**,
    /// and a list applies its rightmost function first.
    #[test]
    fn each_function_maps_points_by_its_definition() {
        let at = |text: &str, point| parse(text).expect(text).apply(point);
        assert!(close(at("translate(3)", (1.0, 1.0)), (4.0, 1.0)));
        assert!(close(at("translate(3 -2)", (1.0, 1.0)), (4.0, -1.0)));
        assert!(close(at("scale(2)", (1.0, 3.0)), (2.0, 6.0)));
        assert!(close(at("scale(2,-1)", (1.0, 3.0)), (2.0, -3.0)));
        // A quarter turn takes +x to +y: clockwise on a y-down screen.
        assert!(close(at("rotate(90)", (1.0, 0.0)), (0.0, 1.0)));
        // About (1, 1), the point (2, 1) goes to (1, 2).
        assert!(close(at("rotate(90 1 1)", (2.0, 1.0)), (1.0, 2.0)));
        // tan(45°) = 1: skewX adds y to x, skewY adds x to y.
        assert!(close(at("skewX(45)", (0.0, 2.0)), (2.0, 2.0)));
        assert!(close(at("skewY(45)", (2.0, 0.0)), (2.0, 2.0)));
        assert!(close(at("matrix(1 2 3 4 5 6)", (1.0, 1.0)), (9.0, 12.0)));
        // Rightmost first: scale, then translate.
        assert!(close(
            at("translate(10,0) scale(2)", (1.0, 0.0)),
            (12.0, 0.0)
        ));
        assert!(close(
            at(" scale(2),translate(10,0) ", (1.0, 0.0)),
            (22.0, 0.0)
        ));
        assert_eq!(parse(""), Some(Transform::IDENTITY));
    }

    /// **What is not a transform list is `None`.**
    #[test]
    fn malformed_lists_are_refused() {
        for text in [
            "translate(1 2 3)",
            "rotate(1 2)",
            "spin(3)",
            "scale(",
            "matrix(1 2 3 4 5)",
            "scale(1e400)",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }

    /// **The inverse undoes the map, and the largest stretch is the larger
    /// singular value.**
    #[test]
    fn inverse_and_stretch_are_exact() {
        let t = parse("translate(3 4) rotate(30) scale(2 0.5)").expect("parses");
        let back = t.invert().expect("invertible");
        assert!(close(back.apply(t.apply((1.5, -2.0))), (1.5, -2.0)));
        assert!((t.max_scale() - 2.0).abs() < 1e-12);
        assert_eq!(Transform::scale(0.0, 1.0).invert(), None);
    }
}
