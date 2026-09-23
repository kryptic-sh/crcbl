//! Compound collision shapes: several boxes fixed in one body's frame that the
//! contact pipeline collides as one rigid body.
//!
//! [`AabbCompound`] is the query half of a body made of parts; this is the
//! dynamic half. A [`CompoundShape`] is carried by
//! [`ColliderComponent::Compound`], and in a system
//! [`with_contacts`](crate::PhysicsSystem::with_contacts) each of its parts is
//! a box of its own to the broadphase and the narrow phase, turning with the
//! body — see [`crate::contact`]'s "Compounds" section for why each part is
//! its own proxy.
//!
//! # Mass: parts summed, overlaps counted twice
//!
//! [`CompoundShape::mass_properties`] gives each part the mass of its own
//! volume at one density and sums the parts with the parallel-axis theorem,
//! through [`MassProperties::combine`]. Where two parts overlap, the overlap is
//! counted once for each: a padded receiver box inside a stock box weighs
//! twice. That is what Box2D, Rapier and Jolt do with a compound's children,
//! and it is chosen here for the same reasons: it is exact for parts that do
//! not overlap, it holds for turned parts where an exact union would need
//! polyhedral clipping, and a game that does not author mass — EW's items —
//! gets a distribution that follows its silhouette, which is what makes a
//! rifle tumble like a rifle. A game that needs a true mass passes it to
//! [`MassProperties::of_collider`]; the centre of mass and the inertia's shape
//! are the part that matters, and a doubled overlap moves them towards where
//! the parts crowd, which is where the solid is.

use std::sync::Arc;

use glam::{DMat3, DQuat, DVec3};

use crate::collider::Aabb;
use crate::components::{ColliderComponent, RigidBody, Transform};
use crate::compound::{AabbCompound, CompoundError};
use crate::contact::shape::ContactShape;
use crate::mass::MassProperties;

/// One box of a [`CompoundShape`], fixed in the body's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompoundPart {
    /// Its centre, in the shape's frame.
    pub centre: DVec3,
    /// Its orientation in the shape's frame: identity for a box whose faces
    /// are the body's own axes, as every part [`CompoundPart::from_aabb`]
    /// makes is.
    pub rotation: DQuat,
    /// Its half-extents along its own axes, in metres.
    pub half_extents: DVec3,
}

impl CompoundPart {
    /// A part centred at `centre`, turned by `rotation` and `half_extents`
    /// across. [`CompoundShape::new`] checks it.
    #[inline]
    #[must_use]
    pub const fn new(centre: DVec3, rotation: DQuat, half_extents: DVec3) -> Self {
        Self {
            centre,
            rotation,
            half_extents,
        }
    }

    /// The box `aabb` as a part, unturned.
    #[inline]
    #[must_use]
    pub fn from_aabb(aabb: &Aabb) -> Self {
        Self {
            centre: aabb.centre(),
            rotation: DQuat::IDENTITY,
            half_extents: aabb.extents() * 0.5,
        }
    }

    /// Its volume, in cubic metres: zero for a flat part.
    #[inline]
    #[must_use]
    pub fn volume(&self) -> f64 {
        let h = self.half_extents;
        8.0 * h.x * h.y * h.z
    }

    /// Its mass properties as a solid of `density`: [`MassProperties::cuboid`]
    /// turned into the shape's frame, `R I Rᵀ`.
    fn mass_properties(&self, density: f64) -> MassProperties {
        let mass = density * self.volume();
        let own = MassProperties::cuboid(mass, self.half_extents, self.centre);
        let i = own.inertia;
        MassProperties {
            inertia: turned_inertia(
                DMat3::from_quat(self.rotation),
                DVec3::new(i.x_axis.x, i.y_axis.y, i.z_axis.z),
            ),
            ..own
        }
    }
}

/// `R diag(d) Rᵀ`, with each entry below the diagonal copied from the one
/// above it, so the tensor is symmetric to the bit: each off-diagonal entry
/// summed in two orders would round two ways, and
/// [`RigidBody::with_inertia`] refuses a tensor that is not symmetric.
fn turned_inertia(r: DMat3, d: DVec3) -> DMat3 {
    let row = |i: usize| DVec3::new(r.col(0)[i], r.col(1)[i], r.col(2)[i]);
    let rows = [row(0), row(1), row(2)];
    let mut m = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in i..3 {
            let value = (rows[i] * d).dot(rows[j]);
            m[i][j] = value;
            m[j][i] = value;
        }
    }
    DMat3::from_cols_array_2d(&m)
}

/// Boxes fixed in one rigid body's frame, validated once: the collision shape
/// [`ColliderComponent::Compound`] carries.
///
/// The parts are shared behind an [`Arc`], so cloning a shape — which
/// [`crate::PhysicsSystem::set_collider`] does to keep its component — copies
/// a pointer. Every part is finite, has no negative half-extent and a unit
/// rotation; there is at least one part, at most [`CompoundShape::MAX_PARTS`],
/// and some volume among them.
#[derive(Debug, Clone, PartialEq)]
pub struct CompoundShape {
    parts: Arc<[CompoundPart]>,
}

/// A dynamic body built from a [`CompoundShape`] by
/// [`CompoundShape::dynamic_body`]: what [`crate::PhysicsSystem::set_body`] and
/// [`crate::PhysicsSystem::set_collider`] take, and where the body's origin
/// sits in the shape's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct CompoundBody {
    /// The body: the parts' mass, and their inertia about their centre of mass.
    pub body: RigidBody,
    /// The collider, offset so the centre of mass is the body's origin.
    pub collider: ColliderComponent,
    /// The centre of mass, in the shape's frame. A game whose own origin is
    /// the shape's places the body at `origin + rotation * centre_of_mass`,
    /// and reads its origin back as `position - rotation * centre_of_mass`.
    pub centre_of_mass: DVec3,
}

impl CompoundShape {
    /// The most parts a shape may have.
    ///
    /// Each part is a broadphase proxy and each touching pair of parts a
    /// manifold of up to [`crate::contact::manifold::MAX_POINTS`] points, so
    /// this is what bounds the proxies a compound body costs and, with the
    /// other body's parts, the points a pair of bodies can put in the solver.
    /// It leaves room above what an item boxed one part per mesh primitive
    /// has: the model of EW's break-action shotgun has thirteen primitives.
    pub const MAX_PARTS: usize = 32;

    /// A density for a body whose mass nobody authored: water's, in kg/m³.
    /// It sets a scale, not a measurement: boxes overstate a model's volume
    /// by their empty corners and by their overlaps, so a game that knows an
    /// item's real mass weighs it with [`MassProperties::of_collider`], which
    /// shares that mass among the parts. The centre of mass and the inertia's
    /// shape, which are what make it tumble, are the same either way.
    pub const DEFAULT_DENSITY: f64 = 1000.0;

    /// A shape of `parts`, each in the shape's frame. Part indices in a
    /// [`crate::ContactReport`] are indices into `parts`.
    ///
    /// A rotation within glam's unit tolerance is kept normalised.
    ///
    /// # Errors
    ///
    /// Naming the first offending part where there is one:
    /// [`CompoundError::NoParts`], [`CompoundError::TooManyParts`],
    /// [`CompoundError::NonFinitePart`] for a `NaN` or infinity anywhere in a
    /// part, [`CompoundError::InvertedPart`] for a negative half-extent,
    /// [`CompoundError::NonUnitRotation`], and [`CompoundError::NoVolume`]
    /// when every part is flat, which leaves nothing for a density to weigh.
    pub fn new(parts: impl Into<Arc<[CompoundPart]>>) -> Result<Self, CompoundError> {
        let parts: Arc<[CompoundPart]> = parts.into();
        if parts.is_empty() {
            return Err(CompoundError::NoParts);
        }
        if parts.len() > Self::MAX_PARTS {
            return Err(CompoundError::TooManyParts { count: parts.len() });
        }
        for (index, part) in parts.iter().enumerate() {
            if !part.centre.is_finite()
                || !part.half_extents.is_finite()
                || !part.rotation.is_finite()
            {
                return Err(CompoundError::NonFinitePart { index });
            }
            if part.half_extents.min_element() < 0.0 {
                return Err(CompoundError::InvertedPart { index });
            }
            if !part.rotation.is_normalized() {
                return Err(CompoundError::NonUnitRotation { index });
            }
        }
        if parts.iter().all(|part| part.volume() == 0.0) {
            return Err(CompoundError::NoVolume);
        }
        let parts = parts
            .iter()
            .map(|part| CompoundPart {
                rotation: part.rotation.normalize(),
                ..*part
            })
            .collect();
        Ok(Self { parts })
    }

    /// A shape of axis-aligned boxes in the body's frame — what
    /// [`AabbCompound`] takes, and what EW's `ItemMotion` keeps.
    ///
    /// # Errors
    ///
    /// [`AabbCompound::new`]'s refusals of a part, then
    /// [`new`](Self::new)'s of the whole.
    pub fn from_aabbs(parts: &[Aabb]) -> Result<Self, CompoundError> {
        let parts = AabbCompound::new(parts)?.parts();
        Self::new(
            parts
                .iter()
                .map(CompoundPart::from_aabb)
                .collect::<Vec<_>>(),
        )
    }

    /// The parts, in the order given.
    #[inline]
    #[must_use]
    pub fn parts(&self) -> &[CompoundPart] {
        &self.parts
    }

    /// The parts' volumes summed, overlaps counted once for each part they
    /// are in.
    #[must_use]
    pub fn volume(&self) -> f64 {
        self.parts.iter().map(CompoundPart::volume).sum()
    }

    /// The shape's mass properties as a solid of `density`, in its own frame:
    /// each part weighed alone and the parts summed — see the module docs for
    /// why an overlap counts twice.
    ///
    /// # Panics
    ///
    /// Panics if `density` is not positive and finite.
    #[must_use]
    pub fn mass_properties(&self, density: f64) -> MassProperties {
        assert!(
            density.is_finite() && density > 0.0,
            "a density is positive and finite: {density}"
        );
        let parts: Vec<MassProperties> = self
            .parts
            .iter()
            .map(|part| part.mass_properties(density))
            .collect();
        MassProperties::combine(&parts)
    }

    /// A dynamic body of `density` with this shape: its mass and inertia from
    /// [`mass_properties`](Self::mass_properties), and a collider offset by
    /// the centre of mass so the body's origin is that centre, as
    /// [`RigidBody`] requires.
    ///
    /// # Panics
    ///
    /// Panics if `density` is not positive and finite.
    #[must_use]
    pub fn dynamic_body(&self, density: f64) -> CompoundBody {
        let props = self.mass_properties(density);
        CompoundBody {
            body: RigidBody::new_dynamic(props.mass).with_inertia(props.inertia),
            collider: ColliderComponent::Compound {
                offset: -props.centre_of_mass,
                shape: self.clone(),
                is_trigger: false,
            },
            centre_of_mass: props.centre_of_mass,
        }
    }

    /// The world bounds of every part, with the shape `offset` from a body at
    /// `transform`, as [`ColliderComponent::Compound`] places it.
    #[must_use]
    pub fn world_bounds(&self, offset: DVec3, transform: &Transform) -> Aabb {
        self.parts.iter().fold(Aabb::EMPTY, |bounds, part| {
            let placed = ContactShape::compound_part(part, offset, transform);
            bounds.union(placed.aabb().expect("a box is bounded"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::FRAC_1_SQRT_2;

    fn assert_close(got: DMat3, want: DMat3, what: &str) {
        let error = (got - want)
            .to_cols_array()
            .iter()
            .fold(0.0f64, |worst, e| worst.max(e.abs()));
        assert!(error < 1e-12, "{what}: {got:?}, want {want:?}");
    }

    /// **An L of two unit cubes, by hand.** Cube 0 spans `[0,1]³` and cube 1
    /// `[1,2]×[0,1]×[0,1]`, at density 1: each weighs 1 kg with `⅙` about
    /// every axis through its own centre. The centre of mass is `(1, ½, ½)`,
    /// each cube's centre half a metre from it along `x`, so the parallel-axis
    /// term adds `2 · 1 · ½² = ½` about `y` and `z` and nothing about `x`:
    /// `I = diag(⅓, ⅚, ⅚)`, with no products of inertia since both centres
    /// lie on one line parallel to `x`.
    #[test]
    fn two_cubes_side_by_side_weigh_and_turn_as_computed_by_hand() {
        let shape = CompoundShape::from_aabbs(&[
            Aabb::new(DVec3::ZERO, DVec3::ONE),
            Aabb::new(DVec3::new(1.0, 0.0, 0.0), DVec3::new(2.0, 1.0, 1.0)),
        ])
        .unwrap();
        let props = shape.mass_properties(1.0);
        assert_eq!(props.mass, 2.0);
        assert_eq!(props.centre_of_mass, DVec3::new(1.0, 0.5, 0.5));
        assert_close(
            props.inertia,
            DMat3::from_diagonal(DVec3::new(1.0 / 3.0, 5.0 / 6.0, 5.0 / 6.0)),
            "two cubes",
        );
    }

    /// **A given mass is shared among the parts**: the two cubes above at
    /// 6 kg are three times the density-1 pair, `diag(1, 5/2, 5/2)`, about the
    /// same centre moved by the collider's offset.
    #[test]
    fn a_given_mass_is_shared_among_the_parts() {
        let shape = CompoundShape::from_aabbs(&[
            Aabb::new(DVec3::ZERO, DVec3::ONE),
            Aabb::new(DVec3::new(1.0, 0.0, 0.0), DVec3::new(2.0, 1.0, 1.0)),
        ])
        .unwrap();
        let collider = ColliderComponent::Compound {
            offset: DVec3::new(0.0, -1.0, 0.0),
            shape,
            is_trigger: false,
        };
        let props = MassProperties::of_collider(&collider, 6.0);
        assert_eq!(props.mass, 6.0);
        assert_eq!(props.centre_of_mass, DVec3::new(1.0, -0.5, 0.5));
        assert_close(
            props.inertia,
            DMat3::from_diagonal(DVec3::new(1.0, 2.5, 2.5)),
            "two cubes at 6 kg",
        );
    }

    /// **An L-shaped compound, by hand, products of inertia and all.** A
    /// 2 × 1 × 1 bar along `x` over `[0,2]×[0,1]×[0,1]` (2 kg at density 1)
    /// and a 1 × 1 × 1 cube standing on its left end at `[0,1]×[1,2]×[0,1]`
    /// (1 kg). Centre of mass: `(2·(1, ½, ½) + (½, 3/2, ½)) / 3 = (5/6, 5/6, ½)`.
    ///
    /// Bar, about its own centre: `2 (1 + 1) / 12 = ⅓` about `x`,
    /// `2 (4 + 1) / 12 = ⅚` about `y` and `z`. Its offset from the centre of
    /// mass is `d = (⅙, −⅓, 0)`: `2 (|d|² − d dᵀ)` adds `2·⅑ = 2/9` about `x`,
    /// `2/36` about `y`, `2·5/36` about `z`, and `−2·(⅙)(−⅓) = +⅑` as the `xy`
    /// product. Cube: `⅙` each way; offset `(−⅓, ⅔, 0)` adds `4/9` about `x`,
    /// `1/9` about `y`, `5/9` about `z`, and `−(−⅓)(⅔) = +2/9` as `xy`.
    ///
    /// Summed: `I_xx = ⅓ + 2/9 + ⅙ + 4/9 = 7/6`, `I_yy = ⅚ + 1/18 + ⅙ + ⅑ =
    /// 7/6`, `I_zz = ⅚ + 5/18 + ⅙ + 5/9 = 11/6`, `I_xy = ⅓`.
    #[test]
    fn an_l_shaped_compound_has_the_hand_computed_tensor() {
        let shape = CompoundShape::from_aabbs(&[
            Aabb::new(DVec3::ZERO, DVec3::new(2.0, 1.0, 1.0)),
            Aabb::new(DVec3::new(0.0, 1.0, 0.0), DVec3::new(1.0, 2.0, 1.0)),
        ])
        .unwrap();
        let props = shape.mass_properties(1.0);
        assert_eq!(props.mass, 3.0);
        assert!(
            (props.centre_of_mass - DVec3::new(5.0 / 6.0, 5.0 / 6.0, 0.5)).length() < 1e-15,
            "{:?}",
            props.centre_of_mass
        );
        let third = 1.0 / 3.0;
        assert_close(
            props.inertia,
            DMat3::from_cols(
                DVec3::new(7.0 / 6.0, third, 0.0),
                DVec3::new(third, 7.0 / 6.0, 0.0),
                DVec3::new(0.0, 0.0, 11.0 / 6.0),
            ),
            "L",
        );
    }

    /// **Two parts overlapping count the overlap twice**, the documented
    /// choice: a cube listed twice weighs two cubes, about the same centre.
    #[test]
    fn an_overlap_is_counted_once_for_each_part_it_is_in() {
        let cube = Aabb::new(DVec3::ZERO, DVec3::ONE);
        let props = CompoundShape::from_aabbs(&[cube, cube])
            .unwrap()
            .mass_properties(1.0);
        assert_eq!(props.mass, 2.0);
        assert_close(
            props.inertia,
            DMat3::from_diagonal(DVec3::splat(2.0 / 6.0)),
            "doubled cube",
        );
    }

    /// **A turned part turns its tensor.** A 12 kg box 1 × 2 × 3 has
    /// `diag(13, 10, 5)` (see `mass::tests`); turned a quarter about `z`, its
    /// long `y` side lies along `x`, so `x` and `y` swap: `diag(10, 13, 5)`,
    /// and the tensor stays symmetric to the bit.
    #[test]
    fn a_turned_part_swaps_its_moments() {
        let quarter = DQuat::from_xyzw(0.0, 0.0, FRAC_1_SQRT_2, FRAC_1_SQRT_2);
        let part = CompoundPart::new(DVec3::ZERO, quarter, DVec3::new(0.5, 1.0, 1.5));
        let shape = CompoundShape::new(vec![part]).unwrap();
        let props = shape.mass_properties(2.0);
        assert_eq!(props.mass, 12.0);
        assert_close(
            props.inertia,
            DMat3::from_diagonal(DVec3::new(10.0, 13.0, 5.0)),
            "turned box",
        );
        let m = props.inertia;
        assert_eq!(m.x_axis.y, m.y_axis.x);
        assert_eq!(m.x_axis.z, m.z_axis.x);
        assert_eq!(m.y_axis.z, m.z_axis.y);

        // Turned about a skew axis the products appear, and the tensor is
        // still symmetric to the bit, so a body accepts it.
        let skew = crate::rotation_from_scaled_axis(DVec3::new(0.3, 0.7, -0.2));
        let turned = CompoundShape::new(vec![CompoundPart::new(
            DVec3::ZERO,
            skew,
            DVec3::new(0.5, 1.0, 1.5),
        )])
        .unwrap()
        .dynamic_body(2.0);
        assert!(turned.body.has_rotational_inertia());
    }

    /// The body a shape builds is centred on the parts' centre of mass: its
    /// collider is offset by that much back, so the parts sit where they were
    /// relative to the shape's own origin.
    #[test]
    fn a_dynamic_body_is_offset_to_its_centre_of_mass() {
        let shape = CompoundShape::from_aabbs(&[
            Aabb::new(DVec3::ZERO, DVec3::ONE),
            Aabb::new(DVec3::new(1.0, 0.0, 0.0), DVec3::new(2.0, 1.0, 1.0)),
        ])
        .unwrap();
        let built = shape.dynamic_body(500.0);
        assert_eq!(built.body.mass, 1000.0);
        assert_eq!(built.centre_of_mass, DVec3::new(1.0, 0.5, 0.5));
        let ColliderComponent::Compound { offset, .. } = built.collider else {
            panic!("{:?}", built.collider);
        };
        assert_eq!(offset, -built.centre_of_mass);
        let bounds = shape.world_bounds(offset, &Transform::from_position(built.centre_of_mass));
        assert_eq!(bounds, Aabb::new(DVec3::ZERO, DVec3::new(2.0, 1.0, 1.0)));
    }

    #[test]
    fn a_shape_refuses_what_cannot_be_a_body() {
        let unit = CompoundPart::from_aabb(&Aabb::new(DVec3::ZERO, DVec3::ONE));
        let flat = CompoundPart::from_aabb(&Aabb::new(DVec3::ZERO, DVec3::new(1.0, 0.0, 1.0)));
        assert_eq!(CompoundShape::new(Vec::new()), Err(CompoundError::NoParts));
        assert_eq!(
            CompoundShape::new(vec![unit; CompoundShape::MAX_PARTS + 1]),
            Err(CompoundError::TooManyParts {
                count: CompoundShape::MAX_PARTS + 1
            })
        );
        assert!(CompoundShape::new(vec![unit; CompoundShape::MAX_PARTS]).is_ok());
        assert_eq!(
            CompoundShape::new(vec![
                unit,
                CompoundPart {
                    centre: DVec3::new(0.0, f64::NAN, 0.0),
                    ..unit
                }
            ]),
            Err(CompoundError::NonFinitePart { index: 1 })
        );
        assert_eq!(
            CompoundShape::new(vec![CompoundPart {
                half_extents: DVec3::new(1.0, -0.5, 1.0),
                ..unit
            }]),
            Err(CompoundError::InvertedPart { index: 0 })
        );
        assert_eq!(
            CompoundShape::new(vec![
                unit,
                CompoundPart {
                    rotation: DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0),
                    ..unit
                }
            ]),
            Err(CompoundError::NonUnitRotation { index: 1 })
        );
        assert_eq!(
            CompoundShape::new(vec![flat, flat]),
            Err(CompoundError::NoVolume)
        );
        assert!(
            CompoundShape::new(vec![flat, unit]).is_ok(),
            "one flat part is fine"
        );
        assert_eq!(
            CompoundShape::from_aabbs(&[Aabb::new(DVec3::ONE, DVec3::ZERO)]),
            Err(CompoundError::InvertedPart { index: 0 }),
            "AabbCompound's own refusals come first"
        );
    }
}
