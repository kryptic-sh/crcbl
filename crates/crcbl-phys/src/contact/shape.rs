//! The shapes the contact pipeline collides: a collider placed on its body.
//!
//! [`crate::world::PhysicsWorld`] keeps colliders unturned — a box is an AABB
//! and a capsule stands up the Y axis whatever its body's orientation. The
//! solver cannot: a capsule lying on its side must roll on its side. So this is
//! the contact pipeline's own placement of a [`ColliderComponent`], built each
//! tick from the body's transform, with the offset, the capsule's axis and the
//! box's faces all turned by the body's rotation. The query world's unturned
//! colliders are unchanged by it.

use glam::{DMat3, DQuat, DVec3};

use crate::collider::Aabb;
use crate::components::{ColliderComponent, Transform};

/// A collider placed in the world, as the manifold functions take it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContactShape {
    /// A sphere.
    Sphere {
        /// Its centre.
        centre: DVec3,
        /// Its radius.
        radius: f64,
    },
    /// A capsule: every point within `radius` of the segment `a`–`b`.
    Capsule {
        /// One end of the core segment.
        a: DVec3,
        /// The other end.
        b: DVec3,
        /// Its radius.
        radius: f64,
    },
    /// An oriented box.
    Box {
        /// Its centre.
        centre: DVec3,
        /// Its orientation.
        rotation: DQuat,
        /// Its half-extents along its own axes.
        half: DVec3,
    },
    /// A static half-space: the points `x` with `normal · x ≤ offset` are
    /// solid.
    Plane {
        /// The unit outward normal.
        normal: DVec3,
        /// The plane's distance from the origin along `normal`.
        offset: f64,
    },
}

impl ContactShape {
    /// `component` on a body at `transform`, or `None` for a trigger, which
    /// the solver never collides.
    #[must_use]
    pub fn placed(component: &ColliderComponent, transform: &Transform) -> Option<Self> {
        let rotation = transform.rotation;
        match *component {
            ColliderComponent::Sphere {
                offset,
                radius,
                is_trigger,
            } => (!is_trigger).then(|| Self::Sphere {
                centre: transform.position + rotation * offset,
                radius,
            }),
            ColliderComponent::Capsule {
                offset,
                radius,
                half_height,
                is_trigger,
            } => (!is_trigger).then(|| {
                let centre = transform.position + rotation * offset;
                let axis = rotation * DVec3::new(0.0, half_height, 0.0);
                Self::Capsule {
                    a: centre - axis,
                    b: centre + axis,
                    radius,
                }
            }),
            ColliderComponent::Box {
                offset,
                half_extents,
                is_trigger,
            } => (!is_trigger).then(|| Self::Box {
                centre: transform.position + rotation * offset,
                rotation,
                half: half_extents,
            }),
        }
    }

    /// The order a contact's two shapes are kept in: the lower rank is shape
    /// `A`, so every manifold function takes its pair one way round only.
    #[must_use]
    pub const fn rank(&self) -> u8 {
        match self {
            Self::Sphere { .. } => 0,
            Self::Capsule { .. } => 1,
            Self::Box { .. } => 2,
            Self::Plane { .. } => 3,
        }
    }

    /// The tight bounds, or `None` for a plane, which has none.
    #[must_use]
    pub fn aabb(&self) -> Option<Aabb> {
        match *self {
            Self::Sphere { centre, radius } => {
                Some(Aabb::from_centre_half(centre, DVec3::splat(radius)))
            }
            Self::Capsule { a, b, radius } => Some(Aabb::new(
                a.min(b) - DVec3::splat(radius),
                a.max(b) + DVec3::splat(radius),
            )),
            Self::Box {
                centre,
                rotation,
                half,
            } => {
                let m = DMat3::from_quat(rotation);
                let reach = DVec3::new(
                    m.x_axis.x.abs() * half.x
                        + m.y_axis.x.abs() * half.y
                        + m.z_axis.x.abs() * half.z,
                    m.x_axis.y.abs() * half.x
                        + m.y_axis.y.abs() * half.y
                        + m.z_axis.y.abs() * half.z,
                    m.x_axis.z.abs() * half.x
                        + m.y_axis.z.abs() * half.y
                        + m.z_axis.z.abs() * half.z,
                );
                Some(Aabb::from_centre_half(centre, reach))
            }
            Self::Plane { .. } => None,
        }
    }

    /// How far any point of the shape lies from `centre`: what a turn of one
    /// radian moves its surface by, at most.
    #[must_use]
    pub fn reach_from(&self, centre: DVec3) -> f64 {
        match *self {
            Self::Sphere {
                centre: c, radius, ..
            } => (c - centre).length() + radius,
            Self::Capsule { a, b, radius } => {
                (a - centre).length().max((b - centre).length()) + radius
            }
            Self::Box {
                centre: c, half, ..
            } => (c - centre).length() + half.length(),
            Self::Plane { .. } => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A capsule lying on its side is placed on its side, and its bounds are
    /// wide where it lies and narrow where it does not.
    #[test]
    fn a_turned_capsule_lies_along_its_turned_axis() {
        let quarter = DQuat::from_xyzw(
            0.0,
            0.0,
            core::f64::consts::FRAC_1_SQRT_2,
            core::f64::consts::FRAC_1_SQRT_2,
        );
        let shape = ContactShape::placed(
            &ColliderComponent::Capsule {
                offset: DVec3::ZERO,
                radius: 0.1,
                half_height: 0.5,
                is_trigger: false,
            },
            &Transform::new(DVec3::new(0.0, 1.0, 0.0), quarter),
        )
        .expect("a solid capsule");
        let ContactShape::Capsule { a, b, .. } = shape else {
            panic!("placed as {shape:?}");
        };
        assert!((a - DVec3::new(0.5, 1.0, 0.0)).length() < 1e-12, "{a:?}");
        assert!((b - DVec3::new(-0.5, 1.0, 0.0)).length() < 1e-12, "{b:?}");
        let bounds = shape.aabb().expect("bounded");
        assert!((bounds.extents().x - 1.2).abs() < 1e-12, "{bounds:?}");
        assert!((bounds.extents().y - 0.2).abs() < 1e-12, "{bounds:?}");
    }

    /// A turned box's bounds hold every corner, and a trigger is not a shape.
    #[test]
    fn a_turned_boxs_bounds_hold_its_corners_and_a_trigger_places_nothing() {
        let turn = crate::rotation_from_scaled_axis(DVec3::new(0.3, 0.7, -0.2));
        let half = DVec3::new(0.5, 0.2, 0.9);
        let shape = ContactShape::Box {
            centre: DVec3::new(1.0, 2.0, 3.0),
            rotation: turn,
            half,
        };
        let bounds = shape.aabb().expect("bounded");
        for i in 0..8u32 {
            let sign = |bit: u32| if i & (1 << bit) == 0 { -1.0 } else { 1.0 };
            let corner =
                DVec3::new(1.0, 2.0, 3.0) + turn * (half * DVec3::new(sign(0), sign(1), sign(2)));
            assert!(
                bounds.inflated(1e-12).contains(&Aabb::new(corner, corner)),
                "corner {corner:?} outside {bounds:?}"
            );
        }
        assert_eq!(
            ContactShape::placed(
                &ColliderComponent::Sphere {
                    offset: DVec3::ZERO,
                    radius: 1.0,
                    is_trigger: true,
                },
                &Transform::IDENTITY,
            ),
            None
        );
    }
}
