//! Paint at a pixel: a flat colour, or a linear or radial gradient evaluated
//! where the pixel's centre falls in the gradient's own space.
//!
//! # Gradients
//!
//! A radial gradient is the general two-point conical one SVG 2 defines: the
//! colour at a point is the stop at the largest `t` for which the point lies
//! on the circle interpolated between the focal circle (`fx`, `fy`, `fr`) and
//! the end circle (`cx`, `cy`, `r`), with a radius that is not negative. With
//! the focus at the centre and `fr` zero, that is the familiar
//! distance-over-radius.
//!
//! Stop colours interpolate in straight alpha and are premultiplied after,
//! as Skia (and so resvg) does.

use super::canvas::Rgba;
use super::transform::Transform;

/// One gradient stop: its offset in `0..=1`, never below the stop before
/// it, and its colour with `stop-opacity` applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Stop {
    pub offset: f32,
    pub color: Rgba,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Spread {
    Pad,
    Reflect,
    Repeat,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Shape {
    /// From `(x1, y1)` at offset 0 to `(x2, y2)` at offset 1.
    Linear { x1: f64, y1: f64, x2: f64, y2: f64 },
    /// From the focal circle at offset 0 to the end circle at offset 1.
    Radial {
        cx: f64,
        cy: f64,
        r: f64,
        fx: f64,
        fy: f64,
        fr: f64,
    },
}

/// A gradient ready to evaluate: at least two stops.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Gradient {
    pub shape: Shape,
    pub spread: Spread,
    pub stops: Vec<Stop>,
    /// Device pixels to the gradient's own space.
    pub inverse: Transform,
    /// `fill-opacity` or `stroke-opacity`.
    pub opacity: f32,
}

/// What a fill or stroke paints with.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Paint {
    Solid(Rgba),
    Gradient(Gradient),
}

impl Paint {
    /// The colour at the centre of pixel `(x, y)`, or `None` where a
    /// gradient defines none (outside a conical gradient's cone).
    pub fn at(&self, x: u32, y: u32) -> Option<Rgba> {
        match self {
            Self::Solid(color) => Some(*color),
            Self::Gradient(gradient) => {
                let point = gradient
                    .inverse
                    .apply((f64::from(x) + 0.5, f64::from(y) + 0.5));
                let t = gradient.shape.offset(point)?;
                let [r, g, b, a] = gradient.color(t);
                Some([r, g, b, a * gradient.opacity])
            }
        }
    }
}

impl Shape {
    /// The gradient parameter at `point`, before the spread method.
    fn offset(self, (px, py): (f64, f64)) -> Option<f64> {
        match self {
            Self::Linear { x1, y1, x2, y2 } => {
                let (dx, dy) = (x2 - x1, y2 - y1);
                Some(((px - x1) * dx + (py - y1) * dy) / (dx * dx + dy * dy))
            }
            Self::Radial {
                cx,
                cy,
                r,
                fx,
                fy,
                fr,
            } => {
                // |p - f - t (c - f)| = fr + t (r - fr), squared:
                // a t² - 2 b t + c = 0.
                let (cdx, cdy) = (cx - fx, cy - fy);
                let (pdx, pdy) = (px - fx, py - fy);
                let dr = r - fr;
                let a = cdx * cdx + cdy * cdy - dr * dr;
                let b = pdx * cdx + pdy * cdy + fr * dr;
                let c = pdx * pdx + pdy * pdy - fr * fr;
                let radius_ok = |t: f64| fr + t * dr >= 0.0;
                if a.abs() < 1e-12 {
                    let t = c / (2.0 * b);
                    return (t.is_finite() && radius_ok(t)).then_some(t);
                }
                let discriminant = b * b - a * c;
                if discriminant < 0.0 {
                    return None;
                }
                let root = discriminant.sqrt();
                let (t1, t2) = ((b + root) / a, (b - root) / a);
                let (high, low) = if t1 > t2 { (t1, t2) } else { (t2, t1) };
                if radius_ok(high) {
                    Some(high)
                } else if radius_ok(low) {
                    Some(low)
                } else {
                    None
                }
            }
        }
    }
}

impl Gradient {
    /// The colour at gradient parameter `t`, after the spread method.
    fn color(&self, t: f64) -> Rgba {
        let t = match self.spread {
            Spread::Pad => t.clamp(0.0, 1.0),
            Spread::Repeat => t.rem_euclid(1.0),
            Spread::Reflect => {
                let folded = t.rem_euclid(2.0);
                if folded > 1.0 { 2.0 - folded } else { folded }
            }
        } as f32;
        let first = self.stops[0];
        if t <= first.offset {
            return first.color;
        }
        for pair in self.stops.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if t <= b.offset {
                let span = b.offset - a.offset;
                if span <= 0.0 {
                    return b.color;
                }
                let k = (t - a.offset) / span;
                return core::array::from_fn(|channel| {
                    a.color[channel] + (b.color[channel] - a.color[channel]) * k
                });
            }
        }
        self.stops[self.stops.len() - 1].color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(shape: Shape, spread: Spread) -> Gradient {
        Gradient {
            shape,
            spread,
            stops: vec![
                Stop {
                    offset: 0.0,
                    color: [0.0, 0.0, 0.0, 1.0],
                },
                Stop {
                    offset: 1.0,
                    color: [1.0, 1.0, 1.0, 1.0],
                },
            ],
            inverse: Transform::IDENTITY,
            opacity: 1.0,
        }
    }

    fn red(paint: &Paint, x: u32, y: u32) -> Option<f32> {
        paint.at(x, y).map(|color| color[0])
    }

    /// **A linear gradient's colour is the projection onto its vector**, at
    /// pixel centres, padded past its ends by default.
    #[test]
    fn linear_gradients_project_onto_their_vector() {
        let paint = Paint::Gradient(gradient(
            Shape::Linear {
                x1: 0.0,
                y1: 0.0,
                x2: 10.0,
                y2: 0.0,
            },
            Spread::Pad,
        ));
        assert_eq!(red(&paint, 0, 3), Some(0.05));
        assert_eq!(red(&paint, 4, 9), Some(0.45));
        assert_eq!(red(&paint, 20, 0), Some(1.0));
        // Diagonal: (x + y) / 20 at the centre.
        let diagonal = Paint::Gradient(gradient(
            Shape::Linear {
                x1: 0.0,
                y1: 0.0,
                x2: 10.0,
                y2: 10.0,
            },
            Spread::Pad,
        ));
        assert!((red(&diagonal, 2, 6).expect("defined") - 0.45).abs() < 1e-6);
    }

    /// **Reflect and repeat fold the parameter as their names say.**
    #[test]
    fn spread_methods_fold_the_parameter() {
        let shape = Shape::Linear {
            x1: 0.0,
            y1: 0.0,
            x2: 10.0,
            y2: 0.0,
        };
        let reflect = Paint::Gradient(gradient(shape, Spread::Reflect));
        let repeat = Paint::Gradient(gradient(shape, Spread::Repeat));
        // x = 12.5: t = 1.25.
        assert!((red(&reflect, 12, 0).expect("defined") - 0.75).abs() < 1e-6);
        assert!((red(&repeat, 12, 0).expect("defined") - 0.25).abs() < 1e-6);
        // x = -2.5: t = -0.25.
        assert!((red(&reflect, 0, 0).map(|_| ()).is_some()));
        let at = |paint: &Paint| {
            let Paint::Gradient(g) = paint else {
                unreachable!()
            };
            g.color(-0.25)[0]
        };
        assert!((at(&reflect) - 0.25).abs() < 1e-6);
        assert!((at(&repeat) - 0.75).abs() < 1e-6);
    }

    /// **A radial gradient with its focus at its centre is distance over
    /// radius; with the focus off centre, it is the two-point cone**, checked
    /// at a point whose circle can be solved by hand.
    #[test]
    fn radial_gradients_solve_their_cone() {
        let centred = Paint::Gradient(gradient(
            Shape::Radial {
                cx: 0.0,
                cy: 0.0,
                r: 10.0,
                fx: 0.0,
                fy: 0.0,
                fr: 0.0,
            },
            Spread::Pad,
        ));
        // (2.5, 5.5) is √36.5 from the centre.
        let want = (2.5f32 * 2.5 + 5.5 * 5.5).sqrt() / 10.0;
        assert!((red(&centred, 2, 5).expect("defined") - want).abs() < 1e-6);

        // Focus at (-5, 0), circle about the origin of radius 10. The point
        // (5.5, 0.5)'s circle has centre (-5 + 5t, 0) and radius 10t; solving
        // (10.5 - 5t)² + 0.25 = 100t² gives the larger root below.
        let focal = Paint::Gradient(gradient(
            Shape::Radial {
                cx: 0.0,
                cy: 0.0,
                r: 10.0,
                fx: -5.0,
                fy: 0.0,
                fr: 0.0,
            },
            Spread::Pad,
        ));
        // 75 t² + 105 t - 110.5 = 0.
        let t = (-105.0f64 + (105.0f64 * 105.0 + 4.0 * 75.0 * 110.5).sqrt()) / 150.0;
        assert!(
            (f64::from(red(&focal, 5, 0).expect("defined")) - t).abs() < 1e-6,
            "{t}"
        );
    }

    /// **Stops interpolate in straight alpha, and the paint's opacity scales
    /// the result.**
    #[test]
    fn stops_interpolate_straight() {
        let mut g = gradient(
            Shape::Linear {
                x1: 0.0,
                y1: 0.0,
                x2: 1.0,
                y2: 0.0,
            },
            Spread::Pad,
        );
        g.stops = vec![
            Stop {
                offset: 0.2,
                color: [1.0, 0.0, 0.0, 0.0],
            },
            Stop {
                offset: 0.6,
                color: [0.0, 0.0, 1.0, 1.0],
            },
        ];
        g.opacity = 0.5;
        assert_eq!(g.color(0.1), [1.0, 0.0, 0.0, 0.0]);
        let middle = g.color(0.4);
        for (got, want) in middle.iter().zip([0.5, 0.0, 0.5, 0.5]) {
            assert!((got - want).abs() < 1e-6, "{middle:?}");
        }
        assert_eq!(g.color(0.9), [0.0, 0.0, 1.0, 1.0]);
        let paint = Paint::Gradient(g);
        // Past the last stop, opaque blue at half the paint's opacity.
        assert_eq!(paint.at(1, 0).map(|c| c[3]), Some(0.5));
    }
}
