//! Mass properties from collider shapes: mass, centre of mass and the inertia
//! tensor a [`RigidBody`](crate::RigidBody) rotates with.
//!
//! Every shape here is a solid of uniform density, and every tensor is about
//! the shape's own centre in the body's frame. A compound body sums its parts
//! with the parallel-axis theorem in [`MassProperties::combine`], which is how a
//! T-handle — a bar and a stem — gets the three different principal moments
//! that make it tumble.
//!
//! The shapes are the ones [`ColliderComponent`] can hold, oriented the way it
//! holds them: a box's axes and a capsule's Y axis are the body's own, and a
//! compound's parts are turned as its shape turns them.

use glam::{DMat3, DVec3};

use crate::components::ColliderComponent;

/// A body's mass, where its centre of mass sits in its own frame, and its
/// inertia tensor about that centre, in kg·m².
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassProperties {
    /// Mass in kilograms.
    pub mass: f64,
    /// The centre of mass, in the body's frame.
    pub centre_of_mass: DVec3,
    /// The inertia tensor about [`centre_of_mass`](Self::centre_of_mass).
    pub inertia: DMat3,
}

impl MassProperties {
    /// A solid sphere of `mass` and `radius` centred at `centre`: `⅖ m r²` on
    /// every axis.
    #[must_use]
    pub fn sphere(mass: f64, radius: f64, centre: DVec3) -> Self {
        Self {
            mass,
            centre_of_mass: centre,
            inertia: DMat3::from_diagonal(DVec3::splat(0.4 * mass * radius * radius)),
        }
    }

    /// A solid box of `mass` and `half_extents` centred at `centre`:
    /// `m (b² + c²) / 12` about the axis whose side is `a`, with each full side
    /// twice its half-extent.
    #[must_use]
    pub fn cuboid(mass: f64, half_extents: DVec3, centre: DVec3) -> Self {
        let squared = half_extents * half_extents;
        let third = mass / 3.0;
        Self {
            mass,
            centre_of_mass: centre,
            inertia: DMat3::from_diagonal(DVec3::new(
                third * (squared.y + squared.z),
                third * (squared.x + squared.z),
                third * (squared.x + squared.y),
            )),
        }
    }

    /// A solid Y-aligned capsule of `mass` centred at `centre`: a cylinder of
    /// `radius` and `2 · half_height` capped by two hemispheres, sharing one
    /// density.
    ///
    /// The cylinder's moments are `m r² / 2` about its axis and
    /// `m (3r² + h²) / 12` across it. A hemisphere's is `⅖ m r²` about both,
    /// taken about the flat face's centre, and moved to the capsule's centre by
    /// the parallel-axis theorem through the hemisphere's own centre of mass,
    /// `3r/8` from that face.
    #[must_use]
    pub fn capsule(mass: f64, radius: f64, half_height: f64, centre: DVec3) -> Self {
        let r2 = radius * radius;
        let height = 2.0 * half_height;
        let cylinder_volume = r2 * height;
        // Both hemispheres together are one sphere: (4/3) r³, beside the
        // cylinder's r² h, with π cancelling out of the ratio.
        let sphere_volume = 4.0 / 3.0 * r2 * radius;
        let cylinder_mass = mass * cylinder_volume / (cylinder_volume + sphere_volume);
        let hemisphere_mass = 0.5 * (mass - cylinder_mass);

        let axial = 0.5 * cylinder_mass * r2 + 2.0 * (0.4 * hemisphere_mass * r2);
        // A hemisphere across its axis, about its own centre of mass: ⅖ m r²
        // about the face, less m (3r/8)², then out to the capsule's centre at
        // half_height + 3r/8.
        let offset = half_height + 0.375 * radius;
        let hemisphere_across = 0.4 * hemisphere_mass * r2
            - hemisphere_mass * (0.375 * radius) * (0.375 * radius)
            + hemisphere_mass * offset * offset;
        let across = cylinder_mass * (3.0 * r2 + height * height) / 12.0 + 2.0 * hemisphere_across;
        Self {
            mass,
            centre_of_mass: centre,
            inertia: DMat3::from_diagonal(DVec3::new(across, axial, across)),
        }
    }

    /// The mass properties of `collider` given `mass`, centred at its offset —
    /// for a compound, at its parts' centre of mass past its offset, `mass`
    /// shared among the parts by volume.
    #[must_use]
    pub fn of_collider(collider: &ColliderComponent, mass: f64) -> Self {
        match *collider {
            ColliderComponent::Compound {
                offset, ref shape, ..
            } => {
                let unit = shape.mass_properties(1.0);
                let scale = mass / unit.mass;
                Self {
                    mass,
                    centre_of_mass: offset + unit.centre_of_mass,
                    inertia: unit.inertia * scale,
                }
            }
            ColliderComponent::Sphere { offset, radius, .. } => Self::sphere(mass, radius, offset),
            ColliderComponent::Box {
                offset,
                half_extents,
                ..
            } => Self::cuboid(mass, half_extents, offset),
            ColliderComponent::Capsule {
                offset,
                radius,
                half_height,
                ..
            } => Self::capsule(mass, radius, half_height, offset),
        }
    }

    /// Several parts as one body: the masses summed, the centre of mass their
    /// mass-weighted mean, and each part's tensor moved to that centre by the
    /// parallel-axis theorem, `I + m (|d|² E − d dᵀ)`, before they are summed.
    ///
    /// # Panics
    ///
    /// Panics if `parts` is empty or their total mass is not positive.
    #[must_use]
    pub fn combine(parts: &[Self]) -> Self {
        let mass: f64 = parts.iter().map(|part| part.mass).sum();
        assert!(mass > 0.0, "a compound body needs mass: {parts:?}");
        let centre_of_mass = parts
            .iter()
            .map(|part| part.centre_of_mass * part.mass)
            .fold(DVec3::ZERO, |sum, moment| sum + moment)
            / mass;
        let inertia = parts.iter().fold(DMat3::ZERO, |sum, part| {
            let d = part.centre_of_mass - centre_of_mass;
            let shift = DMat3::from_diagonal(DVec3::splat(d.length_squared()))
                - DMat3::from_cols(d * d.x, d * d.y, d * d.z);
            sum + part.inertia + shift * part.mass
        });
        Self {
            mass,
            centre_of_mass,
            inertia,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(got: DMat3, want: DMat3, what: &str) {
        let error = (got - want)
            .to_cols_array()
            .iter()
            .fold(0.0f64, |worst, e| worst.max(e.abs()));
        assert!(error < 1e-12, "{what}: {got:?}, want {want:?}");
    }

    #[test]
    fn a_sphere_is_two_fifths_m_r_squared() {
        let props = MassProperties::sphere(5.0, 2.0, DVec3::ZERO);
        assert_close(
            props.inertia,
            DMat3::from_diagonal(DVec3::splat(8.0)),
            "sphere",
        );
    }

    /// A 1 × 2 × 3 m box of 12 kg: `I_x = 12 (4 + 9) / 12 = 13`, and so on.
    #[test]
    fn a_box_is_m_over_twelve_times_the_other_two_sides_squared() {
        let props = MassProperties::cuboid(12.0, DVec3::new(0.5, 1.0, 1.5), DVec3::ZERO);
        assert_close(
            props.inertia,
            DMat3::from_diagonal(DVec3::new(13.0, 10.0, 5.0)),
            "box",
        );
    }

    /// A capsule with no cylinder is a sphere, and one with a vanishing radius
    /// is a thin rod, `m L² / 12` across its axis: the formula's two limits.
    #[test]
    fn a_capsule_meets_the_sphere_and_the_rod_at_its_limits() {
        let ball = MassProperties::capsule(5.0, 2.0, 0.0, DVec3::ZERO);
        assert_close(
            ball.inertia,
            DMat3::from_diagonal(DVec3::splat(8.0)),
            "capsule as sphere",
        );

        let rod = MassProperties::capsule(3.0, 1e-12, 1.0, DVec3::ZERO);
        let across = 3.0 * 2.0 * 2.0 / 12.0;
        assert!(
            (rod.inertia.x_axis.x - across).abs() < 1e-9,
            "rod across: {:?}",
            rod.inertia
        );
        assert!(
            rod.inertia.y_axis.y.abs() < 1e-9,
            "rod along: {:?}",
            rod.inertia
        );
    }

    /// Two point-like spheres a metre either side of the origin: the centre of
    /// mass is between them and the moment across the line is `2 · m · 1²`,
    /// all of it from the parallel-axis term.
    #[test]
    fn combining_parts_applies_the_parallel_axis_theorem() {
        let tiny = 1e-9;
        let left = MassProperties::sphere(1.0, tiny, DVec3::new(-1.0, 0.0, 0.0));
        let right = MassProperties::sphere(1.0, tiny, DVec3::new(1.0, 0.0, 0.0));
        let both = MassProperties::combine(&[left, right]);
        assert_eq!(both.mass, 2.0);
        assert_eq!(both.centre_of_mass, DVec3::ZERO);
        assert_close(
            both.inertia,
            DMat3::from_diagonal(DVec3::new(0.0, 2.0, 2.0)),
            "dumbbell",
        );

        // Off the axes, the products of inertia appear: `-m x y` each.
        let skew = MassProperties::combine(&[
            MassProperties::sphere(1.0, tiny, DVec3::new(1.0, 1.0, 0.0)),
            MassProperties::sphere(1.0, tiny, DVec3::new(-1.0, -1.0, 0.0)),
        ]);
        assert!(
            (skew.inertia.x_axis.y + 2.0).abs() < 1e-12,
            "{:?}",
            skew.inertia
        );
        assert_eq!(skew.inertia.x_axis.y, skew.inertia.y_axis.x, "symmetric");
    }
}
