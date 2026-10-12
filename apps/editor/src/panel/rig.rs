//! The selected rigged mesh's ragdoll rig in the inspector: **a section for
//! the rig, then one per body and per joint**, under the entity's components.
//!
//! The rig's rows are whether bodies overlapping at rest stop colliding and
//! the solver substeps it asks for; a body's are its shape and size and its
//! mass; a joint's, its kind and limits and its friction — what is tuned on a
//! rig once it is fitted. The bone pairs the file names as never colliding
//! have no rows: a list is not a thing [`crcbl::reflect`] reads. Where a
//! body sits and which way it and a joint's axis point stay in the rig file
//! (`<asset>.ragdoll.ron`): a direction is no row a person can drag, and the
//! viewport has no handle for one yet.
//!
//! The rows are a view of the file's [`RagdollDesc`], not the file's own types:
//! [`crcbl::reflect`] reads neither a tuple nor a list, and a limit reads in
//! degrees where the file holds radians. The view is made afresh each frame
//! and written back only where a row moved, so an angle nobody touched is not
//! rewritten by the round trip through degrees. The section reports that the
//! rig changed and nothing more: resolving it against the skeleton and
//! writing the file are `crate::app`'s.
//!
//! **A shape switched keeps its size** ([`reshaped`]): the switch leaves every
//! field of the new variant at zero, which no body may have, so a capsule
//! turned into a sphere keeps its radius and a box is made to wrap the capsule
//! the body was. A joint's kind switched starts at zero limits, which every
//! kind allows: a joint locked until it is opened.

use crcbl::ragdoll::{JointKindDesc, RagdollDesc, ShapeDesc};
use crcbl::reflect::Reflect;
use crcbl::ui::tree::{InspectorOptions, NodeKey, Ui};

/// The whole rig's rows.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
#[reflect(crate = "crcbl::reflect")]
struct RigRows {
    #[reflect(name = "Ignore rest overlaps")]
    ignore_rest_overlaps: bool,
    /// Zero is the system's own; past the far end a rig is better split.
    /// Advisory, as every range is.
    #[reflect(name = "Substeps", min = 0, max = 32, step = 1)]
    substeps: u32,
}

/// A body's rows.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
#[reflect(crate = "crcbl::reflect")]
struct BodyRows {
    #[reflect(name = "Shape")]
    shape: Shape,
    /// No body is weightless, and none of a person's is heavier than the far
    /// end. Advisory, as every range is.
    #[reflect(name = "Mass (kg)", min = 0.01, max = 200.0, step = 0.1)]
    mass: f64,
}

/// [`ShapeDesc`], with an array where the file has a tuple.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
#[reflect(crate = "crcbl::reflect")]
enum Shape {
    Capsule {
        #[reflect(name = "Radius (m)", min = 0.001, max = 2.0, step = 0.002)]
        radius: f64,
        #[reflect(name = "Half height (m)", min = 0.0, max = 2.0, step = 0.002)]
        half_height: f64,
    },
    Sphere {
        #[reflect(name = "Radius (m)", min = 0.001, max = 2.0, step = 0.002)]
        radius: f64,
    },
    Box {
        #[reflect(name = "Half extents (m)", min = 0.001, max = 2.0, step = 0.002)]
        half_extents: [f64; 3],
    },
}

/// A joint's rows.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
#[reflect(crate = "crcbl::reflect")]
struct JointRows {
    #[reflect(name = "Kind")]
    kind: Kind,
    #[reflect(name = "Friction (N·m)", min = 0.0, max = 500.0, step = 0.1)]
    friction: f64,
}

/// [`JointKindDesc`], its angles in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
#[reflect(crate = "crcbl::reflect")]
enum Kind {
    Ball {
        #[reflect(name = "Cone (°)", min = 0.0, max = 180.0, step = 0.5)]
        cone: f64,
        #[reflect(name = "Least twist (°)", min = -180.0, max = 180.0, step = 0.5)]
        twist_lower: f64,
        #[reflect(name = "Most twist (°)", min = -180.0, max = 180.0, step = 0.5)]
        twist_upper: f64,
    },
    Hinge {
        #[reflect(name = "Least angle (°)", min = -180.0, max = 180.0, step = 0.5)]
        lower: f64,
        #[reflect(name = "Most angle (°)", min = -180.0, max = 180.0, step = 0.5)]
        upper: f64,
    },
    Fixed,
}

/// What [`build`] drew.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Built {
    /// The whole rig's rows, once drawn.
    pub(super) rig: Option<NodeKey>,
    /// Each body's rows — its inspector block — in the rig's order.
    pub(super) bodies: Vec<NodeKey>,
    /// Each joint's rows, in the rig's order.
    pub(super) joints: Vec<NodeKey>,
    /// Whether a row moved this frame, and so `rig` changed.
    pub(super) edited: bool,
}

/// Draws `rig`'s sections and writes back what their rows moved — see the
/// [module docs](self).
pub(super) fn build(ui: &mut Ui, rig: &mut RagdollDesc, options: &InspectorOptions<'_>) -> Built {
    let mut built = Built::default();
    ui.block_keyed("ragdoll-rig", ".inspector-section", &[], |ui| {
        ui.block(".section-head", &[], |ui| {
            ui.span(".section-title", "Ragdoll rig", &[]);
        });
        let before = RigRows {
            ignore_rest_overlaps: rig.ignore_rest_overlaps,
            substeps: rig.substeps,
        };
        let mut rows = before;
        let inspection = ui.inspector_with(".section-fields", &mut rows, options);
        built.rig = Some(inspection.response.key);
        if rows != before {
            rig.ignore_rest_overlaps = rows.ignore_rest_overlaps;
            rig.substeps = rows.substeps;
            built.edited = true;
        }
    });
    for body in &mut rig.bodies {
        ui.block_keyed(&body.bone, ".inspector-section", &[], |ui| {
            let heading = format!("Ragdoll body · {}", body.bone);
            ui.block(".section-head", &[], |ui| {
                ui.span(".section-title", heading.as_str(), &[]);
            });
            let before = BodyRows {
                shape: shape_rows(body.shape),
                mass: body.mass,
            };
            let mut rows = before;
            let inspection = ui.inspector_with(".section-fields", &mut rows, options);
            built.bodies.push(inspection.response.key);
            if rows.mass != before.mass {
                body.mass = rows.mass;
                built.edited = true;
            }
            if rows.shape != before.shape {
                body.shape = if inspection.switches.is_empty() {
                    shape_desc(rows.shape)
                } else {
                    reshaped(body.shape, rows.shape)
                };
                built.edited = true;
            }
        });
    }
    for joint in &mut rig.joints {
        let key = (joint.parent.as_str(), joint.child.as_str());
        ui.block_keyed(key, ".inspector-section", &[], |ui| {
            let heading = format!("Ragdoll joint · {} → {}", joint.parent, joint.child);
            ui.block(".section-head", &[], |ui| {
                ui.span(".section-title", heading.as_str(), &[]);
            });
            let before = JointRows {
                kind: kind_rows(joint.kind),
                friction: joint.friction,
            };
            let mut rows = before;
            let inspection = ui.inspector_with(".section-fields", &mut rows, options);
            built.joints.push(inspection.response.key);
            if rows.friction != before.friction {
                joint.friction = rows.friction;
                built.edited = true;
            }
            if rows.kind != before.kind {
                joint.kind = kind_desc(joint.kind, before.kind, rows.kind);
                built.edited = true;
            }
        });
    }
    built
}

fn shape_rows(shape: ShapeDesc) -> Shape {
    match shape {
        ShapeDesc::Capsule {
            radius,
            half_height,
        } => Shape::Capsule {
            radius,
            half_height,
        },
        ShapeDesc::Sphere { radius } => Shape::Sphere { radius },
        ShapeDesc::Box {
            half_extents: (x, y, z),
        } => Shape::Box {
            half_extents: [x, y, z],
        },
    }
}

fn shape_desc(shape: Shape) -> ShapeDesc {
    match shape {
        Shape::Capsule {
            radius,
            half_height,
        } => ShapeDesc::Capsule {
            radius,
            half_height,
        },
        Shape::Sphere { radius } => ShapeDesc::Sphere { radius },
        Shape::Box {
            half_extents: [x, y, z],
        } => ShapeDesc::Box {
            half_extents: (x, y, z),
        },
    }
}

/// `old` switched to `to`'s variant, keeping its size — see the
/// [module docs](self). A box's radius is the larger of its two half-extents
/// across the axis, and its half-height what its `y` reaches past that.
fn reshaped(old: ShapeDesc, to: Shape) -> ShapeDesc {
    let (radius, half_height) = match old {
        ShapeDesc::Capsule {
            radius,
            half_height,
        } => (radius, half_height),
        ShapeDesc::Sphere { radius } => (radius, 0.0),
        ShapeDesc::Box {
            half_extents: (x, y, z),
        } => {
            let radius = x.max(z);
            (radius, (y - radius).max(0.0))
        }
    };
    match to {
        Shape::Capsule { .. } => ShapeDesc::Capsule {
            radius,
            half_height,
        },
        Shape::Sphere { .. } => ShapeDesc::Sphere { radius },
        Shape::Box { .. } => ShapeDesc::Box {
            half_extents: (radius, half_height + radius, radius),
        },
    }
}

fn kind_rows(kind: JointKindDesc) -> Kind {
    match kind {
        JointKindDesc::Ball {
            cone,
            twist_lower,
            twist_upper,
        } => Kind::Ball {
            cone: cone.to_degrees(),
            twist_lower: twist_lower.to_degrees(),
            twist_upper: twist_upper.to_degrees(),
        },
        JointKindDesc::Hinge { lower, upper } => Kind::Hinge {
            lower: lower.to_degrees(),
            upper: upper.to_degrees(),
        },
        JointKindDesc::Fixed => Kind::Fixed,
    }
}

/// `old`, drawn as `before`, after its rows moved to `after`: each angle that
/// moved in radians, and each that did not exactly as it was.
fn kind_desc(old: JointKindDesc, before: Kind, after: Kind) -> JointKindDesc {
    let angle = |old: f64, before: f64, after: f64| {
        if after == before {
            old
        } else {
            after.to_radians()
        }
    };
    match (old, before, after) {
        (
            JointKindDesc::Ball {
                cone,
                twist_lower,
                twist_upper,
            },
            Kind::Ball {
                cone: cone_before,
                twist_lower: lower_before,
                twist_upper: upper_before,
            },
            Kind::Ball {
                cone: cone_after,
                twist_lower: lower_after,
                twist_upper: upper_after,
            },
        ) => JointKindDesc::Ball {
            cone: angle(cone, cone_before, cone_after),
            twist_lower: angle(twist_lower, lower_before, lower_after),
            twist_upper: angle(twist_upper, upper_before, upper_after),
        },
        (
            JointKindDesc::Hinge { lower, upper },
            Kind::Hinge {
                lower: lower_before,
                upper: upper_before,
            },
            Kind::Hinge {
                lower: lower_after,
                upper: upper_after,
            },
        ) => JointKindDesc::Hinge {
            lower: angle(lower, lower_before, lower_after),
            upper: angle(upper, upper_before, upper_after),
        },
        // A switch: the new kind, from its rows.
        (
            _,
            _,
            Kind::Ball {
                cone,
                twist_lower,
                twist_upper,
            },
        ) => JointKindDesc::Ball {
            cone: cone.to_radians(),
            twist_lower: twist_lower.to_radians(),
            twist_upper: twist_upper.to_radians(),
        },
        (_, _, Kind::Hinge { lower, upper }) => JointKindDesc::Hinge {
            lower: lower.to_radians(),
            upper: upper.to_radians(),
        },
        (_, _, Kind::Fixed) => JointKindDesc::Fixed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A switched shape keeps the body's size**, and switching back gives
    /// the body it was: a capsule's radius and half-height survive a box.
    #[test]
    fn a_switched_shape_keeps_its_size() {
        let capsule = ShapeDesc::Capsule {
            radius: 0.07,
            half_height: 0.15,
        };
        let boxed = reshaped(
            capsule,
            Shape::Box {
                half_extents: [0.0; 3],
            },
        );
        assert_eq!(
            boxed,
            ShapeDesc::Box {
                half_extents: (0.07, 0.22, 0.07),
            }
        );
        let zero = Shape::Capsule {
            radius: 0.0,
            half_height: 0.0,
        };
        assert_eq!(reshaped(boxed, zero), capsule);
        assert_eq!(
            reshaped(capsule, Shape::Sphere { radius: 0.0 }),
            ShapeDesc::Sphere { radius: 0.07 }
        );
    }

    /// **An angle nobody moved is written back exactly as it was**, though
    /// its row showed it in degrees; the one that moved is in radians.
    #[test]
    fn only_the_moved_angle_is_rewritten() {
        let old = JointKindDesc::Ball {
            cone: 0.1,
            twist_lower: -0.4,
            twist_upper: 0.2,
        };
        // Both change on a trip through degrees, so a rewrite would show.
        assert_ne!(0.2_f64.to_degrees().to_radians(), 0.2);
        assert_ne!((-0.4_f64).to_degrees().to_radians(), -0.4);
        let before = kind_rows(old);
        let Kind::Ball {
            twist_lower,
            twist_upper,
            ..
        } = before
        else {
            panic!("a ball reads as a ball");
        };
        let after = Kind::Ball {
            cone: 90.0,
            twist_lower,
            twist_upper,
        };
        assert_eq!(
            kind_desc(old, before, after),
            JointKindDesc::Ball {
                cone: core::f64::consts::FRAC_PI_2,
                twist_lower: -0.4,
                twist_upper: 0.2,
            }
        );
    }
}
