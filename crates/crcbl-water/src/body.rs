//! A body of water and the medium inside it, and what makes one well formed.

/// What light meets inside a body of water, per colour channel.
///
/// The parameter set `docs/plan/55-water.md`'s second decision gives every body
/// kind: an **absorption** coefficient, the fraction of light turned into heat
/// per metre, and a **scattering** coefficient, the fraction sent in a new
/// direction per metre. Both are in **1/m**, one value per linear-RGB channel,
/// and their sum is the extinction — how fast light travelling through the body
/// is lost to either. Clear water absorbs red fastest and blue slowest, which is
/// why a deep pool reads blue-green; a muddy pond scatters more.
///
/// **There is no phase asymmetry here yet**, though that decision lists one. The
/// renderer's rung 1 scatters the ambient and the sun's light isotropically, so
/// an anisotropy field would be a number no reader looks at; the rung whose
/// shading has a direction to weigh adds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Medium {
    /// Absorption per metre, per linear-RGB channel.
    pub absorption: [f32; 3],
    /// Scattering per metre, per linear-RGB channel.
    pub scattering: [f32; 3],
}

/// One body of water: a closed outline on the XZ plane, the height its surface
/// sits at, and what it is made of.
///
/// The outline is a **simple polygon** — convex or not, wound either way, with
/// no edge touching or crossing another — and it is closed implicitly: the last
/// point joins the first. [`crate::surface_mesh`] refuses one that is not,
/// rather than drawing a surface with a hole or a fold in it.
#[derive(Clone, Debug, PartialEq)]
pub struct WaterBody {
    /// The surface's boundary, as `[x, z]` points in metres.
    pub outline: Vec<[f32; 2]>,
    /// The surface's height, in metres.
    pub level: f32,
    /// What the water is made of.
    pub medium: Medium,
}

/// Why a [`WaterBody`] cannot be meshed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum BodyError {
    /// The outline has fewer than three points, so it encloses nothing.
    #[error("an outline needs at least three points, and this one has {count}")]
    TooFewPoints {
        /// How many it has.
        count: usize,
    },
    /// An outline point has a coordinate that is not a finite number.
    #[error("outline point {index} is {point:?}, which is not a finite position")]
    NonFinitePoint {
        /// Which point.
        index: usize,
        /// The point as given.
        point: [f32; 2],
    },
    /// Two consecutive outline points are the same point, so the edge between
    /// them has no direction.
    #[error("outline points {index} and the one after it are both {point:?}")]
    RepeatedPoint {
        /// The first of the two.
        index: usize,
        /// The point both are.
        point: [f32; 2],
    },
    /// The level is not a finite number.
    #[error("the water level {level} is not a finite height")]
    NonFiniteLevel {
        /// The level as given.
        level: f32,
    },
    /// A medium coefficient is negative or not finite. A negative absorption
    /// would add light with depth rather than remove it.
    #[error("the medium's {which} is {values:?}; each channel must be finite and at least zero")]
    InvalidMedium {
        /// `"absorption"` or `"scattering"`.
        which: &'static str,
        /// The three channels as given.
        values: [f32; 3],
    },
    /// The outline's points all lie on one line, so it encloses no area.
    #[error("the outline encloses no area")]
    NoArea,
    /// Two edges of the outline touch or cross, so it is not a simple polygon.
    #[error("outline edges {first} and {second} touch or cross")]
    SelfIntersecting {
        /// The edge starting at this point index.
        first: usize,
        /// The edge starting at this point index.
        second: usize,
    },
    /// The grid spacing is not a finite, positive length.
    #[error("a grid spacing of {spacing} is not a finite positive length")]
    InvalidSpacing {
        /// The spacing as given.
        spacing: f32,
    },
    /// The surface meshes to more vertices than a `u32` index can address.
    #[error("the surface meshes to more vertices than a u32 index addresses")]
    TooManyVertices,
    /// The outline's bounds hold more grid cells than [`crate::MAX_GRID_CELLS`].
    #[error("the outline's bounds hold {cells} grid cells at this spacing, past the {max} allowed")]
    TooManyCells {
        /// How many cells the bounds hold, saturating.
        cells: u64,
        /// [`crate::MAX_GRID_CELLS`].
        max: u64,
    },
}

impl Medium {
    /// Refuses a coefficient that is negative or not finite.
    pub(crate) fn validate(&self) -> Result<(), BodyError> {
        for (which, values) in [
            ("absorption", self.absorption),
            ("scattering", self.scattering),
        ] {
            // Written so a `NaN` fails: every comparison against one is false.
            if !values
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0)
            {
                return Err(BodyError::InvalidMedium { which, values });
            }
        }
        Ok(())
    }
}

/// `(b - a) × (c - a)` on the XZ plane, in `f64`: positive when `a`, `b`, `c`
/// turn counter-clockwise with `x` across and `z` up the page.
///
/// `f64` because the sign is what every decision in this crate reads, and a
/// nearly collinear corner is decided with more than twice the precision the
/// `f32` coordinates carry. It is **not** an exact predicate — Shewchuk's
/// adaptive arithmetic would be — and nothing here relies on one: a corner
/// misjudged by rounding is one whose triangle has no area to lose.
pub(crate) fn orient(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f64 {
    let (ax, az) = (f64::from(a[0]), f64::from(a[1]));
    (f64::from(b[0]) - ax) * (f64::from(c[1]) - az)
        - (f64::from(b[1]) - az) * (f64::from(c[0]) - ax)
}

impl WaterBody {
    /// Refuses a body [`crate::surface_mesh`] could not mesh faithfully: too few
    /// points, a non-finite coordinate or level, a repeated point, an invalid
    /// medium, no area, or an outline that touches or crosses itself.
    ///
    /// The self-intersection test compares every pair of edges, which is
    /// quadratic in the outline's length — a pool's outline is tens of points,
    /// and a body is validated when it is set rather than every frame.
    pub(crate) fn validate(&self) -> Result<(), BodyError> {
        let outline = &self.outline;
        if outline.len() < 3 {
            return Err(BodyError::TooFewPoints {
                count: outline.len(),
            });
        }
        if let Some((index, point)) = outline
            .iter()
            .enumerate()
            .find(|(_, point)| !(point[0].is_finite() && point[1].is_finite()))
        {
            return Err(BodyError::NonFinitePoint {
                index,
                point: *point,
            });
        }
        if !self.level.is_finite() {
            return Err(BodyError::NonFiniteLevel { level: self.level });
        }
        self.medium.validate()?;
        let count = outline.len();
        for index in 0..count {
            if outline[index] == outline[(index + 1) % count] {
                return Err(BodyError::RepeatedPoint {
                    index,
                    point: outline[index],
                });
            }
        }
        if signed_area(outline) == 0.0 {
            return Err(BodyError::NoArea);
        }
        for first in 0..count {
            for second in first + 1..count {
                if edges_meet(outline, first, second) {
                    return Err(BodyError::SelfIntersecting { first, second });
                }
            }
        }
        Ok(())
    }
}

/// Twice the outline's signed area, positive when it winds counter-clockwise in
/// [`orient`]'s sense — the shoelace sum, in `f64`.
pub(crate) fn signed_area(outline: &[[f32; 2]]) -> f64 {
    let origin = outline[0];
    (1..outline.len() - 1)
        .map(|index| orient(origin, outline[index], outline[index + 1]))
        .sum()
}

/// Whether edges `first` and `second` of `outline` — each from a point to the
/// next — share any point they should not.
///
/// Two edges that follow one another share their joint and nothing else is
/// allowed: the far end of either lying on the other is a fold back along
/// itself. Any other pair may share nothing at all, touching included.
fn edges_meet(outline: &[[f32; 2]], first: usize, second: usize) -> bool {
    let count = outline.len();
    let (a, b) = (outline[first], outline[(first + 1) % count]);
    let (c, d) = (outline[second], outline[(second + 1) % count]);
    if (first + 1) % count == second {
        // `b == c` is the joint: the edges fold if `d` lies on `ab` or `a` on
        // `cd`, which for two segments from one point means they are collinear
        // and pointing the same way.
        return on_segment(a, b, d) || on_segment(c, d, a);
    }
    if (second + 1) % count == first {
        return on_segment(c, d, b) || on_segment(a, b, c);
    }
    segments_meet(a, b, c, d)
}

/// Whether `p` lies on the closed segment `ab`.
fn on_segment(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> bool {
    orient(a, b, p) == 0.0 && within(a[0], b[0], p[0]) && within(a[1], b[1], p[1])
}

/// Whether `value` lies between `a` and `b`, inclusive, in either order.
fn within(a: f32, b: f32, value: f32) -> bool {
    a.min(b) <= value && value <= a.max(b)
}

/// Whether the closed segments `ab` and `cd` share a point — the textbook
/// orientation test, with the collinear cases decided by [`on_segment`].
fn segments_meet(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> bool {
    let abc = orient(a, b, c);
    let abd = orient(a, b, d);
    let cda = orient(c, d, a);
    let cdb = orient(c, d, b);
    if ((abc > 0.0 && abd < 0.0) || (abc < 0.0 && abd > 0.0))
        && ((cda > 0.0 && cdb < 0.0) || (cda < 0.0 && cdb > 0.0))
    {
        return true;
    }
    on_segment(a, b, c) || on_segment(a, b, d) || on_segment(c, d, a) || on_segment(c, d, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAR: Medium = Medium {
        absorption: [0.4, 0.1, 0.05],
        scattering: [0.01, 0.01, 0.01],
    };

    fn body(outline: &[[f32; 2]]) -> WaterBody {
        WaterBody {
            outline: outline.to_vec(),
            level: 0.0,
            medium: CLEAR,
        }
    }

    #[test]
    fn a_square_is_well_formed_either_way_round() {
        let square = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        assert_eq!(body(&square).validate(), Ok(()));
        let mut reversed = square;
        reversed.reverse();
        assert_eq!(body(&reversed).validate(), Ok(()));
        assert_eq!(signed_area(&square), 8.0);
        assert_eq!(signed_area(&reversed), -8.0);
    }

    #[test]
    fn a_concave_outline_is_well_formed() {
        let ell = [
            [0.0, 0.0],
            [3.0, 0.0],
            [3.0, 1.0],
            [1.0, 1.0],
            [1.0, 3.0],
            [0.0, 3.0],
        ];
        assert_eq!(body(&ell).validate(), Ok(()));
    }

    #[test]
    fn too_few_points_are_refused() {
        assert_eq!(
            body(&[[0.0, 0.0], [1.0, 0.0]]).validate(),
            Err(BodyError::TooFewPoints { count: 2 })
        );
    }

    #[test]
    fn a_non_finite_point_or_level_is_refused() {
        let outline = [[0.0, 0.0], [f32::NAN, 0.0], [0.0, 1.0]];
        assert!(matches!(
            body(&outline).validate(),
            Err(BodyError::NonFinitePoint { index: 1, .. })
        ));
        let mut level = body(&[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        level.level = f32::INFINITY;
        assert!(matches!(
            level.validate(),
            Err(BodyError::NonFiniteLevel { .. })
        ));
    }

    #[test]
    fn a_repeated_point_is_refused() {
        let outline = [[0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        assert_eq!(
            body(&outline).validate(),
            Err(BodyError::RepeatedPoint {
                index: 1,
                point: [1.0, 0.0]
            })
        );
        // And the closing edge, from the last point back to the first.
        let closing = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [0.0, 0.0]];
        assert!(matches!(
            body(&closing).validate(),
            Err(BodyError::RepeatedPoint { index: 3, .. })
        ));
    }

    #[test]
    fn a_collinear_outline_has_no_area() {
        let line = [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]];
        assert_eq!(body(&line).validate(), Err(BodyError::NoArea));
    }

    #[test]
    fn a_bow_tie_crosses_itself() {
        // Lopsided, so its two lobes do not cancel to no area at all — that
        // outline would be refused as `NoArea` before its edges were compared.
        let bow_tie = [[0.0, 0.0], [2.0, 2.0], [2.0, 0.0], [0.0, 1.0]];
        assert_eq!(
            body(&bow_tie).validate(),
            Err(BodyError::SelfIntersecting {
                first: 0,
                second: 2
            })
        );
    }

    #[test]
    fn an_edge_touching_a_corner_is_refused() {
        // Point 4 sits on edge 0's span: the outline pinches there.
        let pinched = [
            [0.0, 0.0],
            [4.0, 0.0],
            [4.0, 2.0],
            [3.0, 2.0],
            [2.0, 0.0],
            [0.0, 2.0],
        ];
        assert!(matches!(
            body(&pinched).validate(),
            Err(BodyError::SelfIntersecting { .. })
        ));
    }

    #[test]
    fn a_fold_back_along_an_edge_is_refused() {
        // Point 2 walks back along edge 0 toward where it started.
        let spike = [[0.0, 0.0], [2.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
        assert!(matches!(
            body(&spike).validate(),
            Err(BodyError::SelfIntersecting { .. })
        ));
    }

    #[test]
    fn a_negative_or_non_finite_coefficient_is_refused() {
        let mut negative = body(&[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        negative.medium.absorption[1] = -0.1;
        assert!(matches!(
            negative.validate(),
            Err(BodyError::InvalidMedium {
                which: "absorption",
                ..
            })
        ));
        let mut not_a_number = body(&[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        not_a_number.medium.scattering[2] = f32::NAN;
        assert!(matches!(
            not_a_number.validate(),
            Err(BodyError::InvalidMedium {
                which: "scattering",
                ..
            })
        ));
    }
}
