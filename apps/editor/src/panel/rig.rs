//! The selected rigged mesh's ragdoll rig in the inspector: **a section for
//! the rig, then one per body and per joint**, under the entity's components.
//!
//! The rig's rows are whether bodies overlapping at rest stop colliding and
//! the solver substeps it asks for; a body's are its shape and size, its mass,
//! and where it sits and which way it points in its bone's frame; a joint's,
//! its kind, limits and neutral, its friction, and its axis and offset from
//! its child bone. A joint's soft zones have no rows yet: they are kept as the
//! file has them while the other rows move. An axis is three numbers, which
//! the rig resolves to a direction (a zero one is refused); the viewport has
//! no handle for it. A joint whose rest pose lies outside its limits as
//! measured from its neutral — one a ragdoll spawned near rest snaps out of —
//! carries a warning under its rows; it is not refused.
//!
//! **The bone pairs the file names as never colliding have a section of their
//! own** rather than rows: a list is not a thing [`crcbl::reflect`] reads.
//! Each listed pair is a line with a Remove button, and a drop-down below them
//! offers every pair of bodies not listed — picking one lists it. The line
//! or the drop-down's option the pointer is over names its pair to
//! `crate::app`, which draws the two bodies apart from the rest, so a pair is
//! seen before it is picked. The pairs
//! whose bodies touch at rest come first, marked so: they are the ones that
//! would push apart the moment the rig falls. A pair the rest rule already
//! leaves out may be listed too: the list is what the file says, and it still
//! holds with the rule switched off.
//!
//! **The rig section's Undo and Redo step the rig's own history**, which
//! `crate::app` keeps apart from the scene's: the rig is the asset's file,
//! and Ctrl+Z is the scene's. Each button is drawn only when there is a step
//! to take — the panels have no disabled button.
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
//! the body was. **A joint's kind switched keeps its swing** ([`kind_desc`]):
//! a ball's wider swing half-angle becomes a hinge's range either side of
//! rest, and a hinge's farther limit a ball's circular swing, with no twist.
//! A switch drops the neutral and the soft zones, which mean something else
//! in the other kind. Every other switch — to or from fixed — starts at zero
//! limits, which every kind allows: a joint locked until it is opened.

use crcbl::ragdoll::{JointKindDesc, RagdollDesc, RigJointKind, ShapeDesc, SwingTwist};
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
    #[reflect(name = "Centre (m)", min = -2.0, max = 2.0, step = 0.002)]
    centre: [f64; 3],
    #[reflect(name = "Axis", min = -1.0, max = 1.0, step = 0.01)]
    axis: [f64; 3],
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
    #[reflect(name = "Axis", min = -1.0, max = 1.0, step = 0.01)]
    axis: [f64; 3],
    #[reflect(name = "Offset (m)", min = -2.0, max = 2.0, step = 0.002)]
    at: [f64; 3],
}

/// [`JointKindDesc`], its angles in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
#[reflect(crate = "crcbl::reflect")]
enum Kind {
    Ball {
        #[reflect(name = "Swing about x (°)", min = 0.0, max = 180.0, step = 0.5)]
        swing_x: f64,
        #[reflect(name = "Swing about y (°)", min = 0.0, max = 180.0, step = 0.5)]
        swing_y: f64,
        #[reflect(name = "Least twist (°)", min = -180.0, max = 180.0, step = 0.5)]
        twist_lower: f64,
        #[reflect(name = "Most twist (°)", min = -180.0, max = 180.0, step = 0.5)]
        twist_upper: f64,
        #[reflect(name = "Neutral swing about x (°)", min = -180.0, max = 180.0, step = 0.5)]
        neutral_swing_x: f64,
        #[reflect(name = "Neutral swing about y (°)", min = -180.0, max = 180.0, step = 0.5)]
        neutral_swing_y: f64,
        #[reflect(name = "Neutral twist (°)", min = -180.0, max = 180.0, step = 0.5)]
        neutral_twist: f64,
    },
    Hinge {
        #[reflect(name = "Least angle (°)", min = -180.0, max = 180.0, step = 0.5)]
        lower: f64,
        #[reflect(name = "Most angle (°)", min = -180.0, max = 180.0, step = 0.5)]
        upper: f64,
        #[reflect(name = "Neutral (°)", min = -180.0, max = 180.0, step = 0.5)]
        neutral: f64,
    },
    Fixed,
}

/// The selection's rig as the inspector is handed it: the file to edit in
/// place, and whether its history has a step back and a step forward.
#[derive(Debug)]
pub struct RigEdit<'a> {
    /// The rig file, edited in place.
    pub desc: &'a mut RagdollDesc,
    /// Whether an edit can be undone: what draws the Undo button.
    pub can_undo: bool,
    /// Whether an undone edit can be redone: what draws the Redo button.
    pub can_redo: bool,
    /// The pairs of bodies, by bone, that touch at rest with no joint
    /// between them: what the pair drop-down offers first.
    pub touching: Vec<(String, String)>,
}

/// What [`build`] drew.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Built {
    /// The whole rig's rows, once drawn.
    pub(super) rig: Option<NodeKey>,
    /// The Undo button, when drawn.
    pub(super) undo_button: Option<NodeKey>,
    /// The Redo button, when drawn.
    pub(super) redo_button: Option<NodeKey>,
    /// Whether Undo was clicked this frame.
    pub(super) undo: bool,
    /// Whether Redo was clicked this frame.
    pub(super) redo: bool,
    /// Each body's rows — its inspector block — in the rig's order.
    pub(super) bodies: Vec<NodeKey>,
    /// Each joint's rows, in the rig's order.
    pub(super) joints: Vec<NodeKey>,
    /// Each listed pair's Remove button, in the file's order.
    pub(super) pair_removes: Vec<NodeKey>,
    /// The drop-down of the pairs not listed, once drawn.
    pub(super) pair_add: Option<NodeKey>,
    /// The pair whose listed line or drop-down option the pointer was over,
    /// by bone.
    pub(super) hovered_pair: Option<(String, String)>,
    /// Whether a row moved this frame, and so `rig` changed.
    pub(super) edited: bool,
}

/// Draws `edit`'s sections and writes back what their rows moved — see the
/// [module docs](self).
pub(super) fn build(ui: &mut Ui, edit: RigEdit<'_>, options: &InspectorOptions<'_>) -> Built {
    let mut built = Built::default();
    let rig = edit.desc;
    ui.block_keyed("ragdoll-rig", ".inspector-section", &[], |ui| {
        ui.block(".section-head", &[], |ui| {
            ui.span(".section-title", "Ragdoll rig", &[]);
            if edit.can_undo {
                let button = ui.button(".section-remove", "Undo");
                ui.tooltip(&button, "Undo the rig's last edit, and write it");
                built.undo = button.clicked;
                built.undo_button = Some(button.key);
            }
            if edit.can_redo {
                let button = ui.button(".section-remove", "Redo");
                ui.tooltip(&button, "Redo the rig's edit last undone, and write it");
                built.redo = button.clicked;
                built.redo_button = Some(button.key);
            }
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
    pairs(ui, rig, &edit.touching, &mut built);
    for body in &mut rig.bodies {
        ui.block_keyed(&body.bone, ".inspector-section", &[], |ui| {
            let heading = format!("Ragdoll body · {}", body.bone);
            ui.block(".section-head", &[], |ui| {
                ui.span(".section-title", heading.as_str(), &[]);
            });
            let before = BodyRows {
                shape: shape_rows(body.shape),
                mass: body.mass,
                centre: array(body.centre),
                axis: array(body.axis),
            };
            let mut rows = before;
            let inspection = ui.inspector_with(".section-fields", &mut rows, options);
            built.bodies.push(inspection.response.key);
            if rows.mass != before.mass {
                body.mass = rows.mass;
                built.edited = true;
            }
            if rows.centre != before.centre {
                body.centre = triple(rows.centre);
                built.edited = true;
            }
            if rows.axis != before.axis {
                body.axis = triple(rows.axis);
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
                axis: array(joint.axis),
                at: array(joint.at),
            };
            let mut rows = before;
            let inspection = ui.inspector_with(".section-fields", &mut rows, options);
            built.joints.push(inspection.response.key);
            if rows.friction != before.friction {
                joint.friction = rows.friction;
                built.edited = true;
            }
            if rows.axis != before.axis {
                joint.axis = triple(rows.axis);
                built.edited = true;
            }
            if rows.at != before.at {
                joint.at = triple(rows.at);
                built.edited = true;
            }
            if rows.kind != before.kind {
                joint.kind = kind_desc(joint.kind, before.kind, rows.kind);
                built.edited = true;
            }
            if !RigJointKind::from(joint.kind).rest_is_within_limits() {
                ui.span(".rig-warning", REST_OUTSIDE, &[]);
            }
        });
    }
    built
}

/// The warning under a joint whose rest pose lies outside its limits as
/// measured from its neutral.
const REST_OUTSIDE: &str =
    "Rest lies outside these limits about the neutral: a ragdoll spawned near rest snaps inside.";

/// The option a pair drop-down shows while nothing is picked.
const ADD_PAIR: &str = "Add a pair...";

/// Draws `rig`'s section of listed pairs, removing the one whose button was
/// clicked and listing the one picked, `touching` offered first — see the
/// [module docs](self).
fn pairs(ui: &mut Ui, rig: &mut RagdollDesc, touching: &[(String, String)], built: &mut Built) {
    ui.block_keyed("ragdoll-pairs", ".inspector-section", &[], |ui| {
        ui.block(".section-head", &[], |ui| {
            ui.span(".section-title", "Ragdoll pairs that never collide", &[]);
        });
        let mut removed = None;
        for (index, (a, b)) in rig.ignore.iter().enumerate() {
            let label = pair_label(a, b);
            let line = ui.block_keyed(label.as_str(), ".rig-pair", &[], |ui| {
                ui.span(".rig-pair-label", label.as_str(), &[]);
                let button = ui.button(".section-remove", "Remove");
                ui.tooltip(&button, "Let these bodies collide again, and write it");
                if button.clicked {
                    removed = Some(index);
                }
                built.pair_removes.push(button.key);
            });
            if line.hovered {
                built.hovered_pair = Some((a.clone(), b.clone()));
            }
        }
        let unlisted = unlisted_pairs(rig, touching);
        let labels: Vec<String> = unlisted
            .iter()
            .map(|((a, b), touches)| {
                let label = pair_label(a, b);
                if *touches {
                    format!("{label} (touching at rest)")
                } else {
                    label
                }
            })
            .collect();
        let options: Vec<&str> = std::iter::once(ADD_PAIR)
            .chain(labels.iter().map(String::as_str))
            .collect();
        let mut chosen = 0;
        let select = ui.select(".rig-pair-add", &options, &mut chosen);
        built.pair_add = Some(select.key);
        // The option under the pointer, else the one the arrows reached. The
        // first option is the prompt, which names no pair.
        if let Some((pair, _)) = ui
            .select_hovered(select.key, &options)
            .or_else(|| ui.select_focused(select.key, &options))
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| unlisted.get(index))
        {
            built.hovered_pair = Some(pair.clone());
        }
        if let Some(index) = removed {
            rig.ignore.remove(index);
            built.edited = true;
        } else if let Some((pair, _)) = chosen.checked_sub(1).and_then(|index| unlisted.get(index))
        {
            rig.ignore.push(pair.clone());
            built.edited = true;
        }
    });
}

/// A pair as its line and its option read.
fn pair_label(a: &str, b: &str) -> String {
    format!("{a} ↔ {b}")
}

/// Every pair of `rig`'s bodies its file does not list, either way round,
/// each with whether it is among `touching`: those first, then the rest, each
/// in the bodies' order.
fn unlisted_pairs(
    rig: &RagdollDesc,
    touching: &[(String, String)],
) -> Vec<((String, String), bool)> {
    let among = |pairs: &[(String, String)], a: &str, b: &str| {
        pairs
            .iter()
            .any(|(x, y)| (x == a && y == b) || (x == b && y == a))
    };
    let mut pairs = Vec::new();
    for (index, first) in rig.bodies.iter().enumerate() {
        for second in &rig.bodies[index + 1..] {
            let (a, b) = (&first.bone, &second.bone);
            if !among(&rig.ignore, a, b) {
                pairs.push(((a.clone(), b.clone()), among(touching, a, b)));
            }
        }
    }
    pairs.sort_by_key(|(_, touches)| !touches);
    pairs
}

/// The file's tuple as a row's array, which [`crcbl::reflect`] reads.
fn array((x, y, z): (f64, f64, f64)) -> [f64; 3] {
    [x, y, z]
}

/// A row's array as the file's tuple.
fn triple([x, y, z]: [f64; 3]) -> (f64, f64, f64) {
    (x, y, z)
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
        ShapeDesc::Box { half_extents } => Shape::Box {
            half_extents: array(half_extents),
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
        Shape::Box { half_extents } => ShapeDesc::Box {
            half_extents: triple(half_extents),
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
            swing_x,
            swing_y,
            twist_lower,
            twist_upper,
            neutral,
            ..
        } => Kind::Ball {
            swing_x: swing_x.to_degrees(),
            swing_y: swing_y.to_degrees(),
            twist_lower: twist_lower.to_degrees(),
            twist_upper: twist_upper.to_degrees(),
            neutral_swing_x: neutral.swing_x.to_degrees(),
            neutral_swing_y: neutral.swing_y.to_degrees(),
            neutral_twist: neutral.twist.to_degrees(),
        },
        JointKindDesc::Hinge {
            lower,
            upper,
            neutral,
            ..
        } => Kind::Hinge {
            lower: lower.to_degrees(),
            upper: upper.to_degrees(),
            neutral: neutral.to_degrees(),
        },
        JointKindDesc::Fixed => Kind::Fixed,
    }
}

/// `old`, drawn as `before`, after its rows moved to `after`: each angle that
/// moved in radians, each that did not exactly as it was, and the soft zones,
/// which have no rows, kept. A switch keeps the swing — see the
/// [module docs](self).
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
                swing_x,
                swing_y,
                twist_lower,
                twist_upper,
                neutral,
                soft_swing,
                soft_twist,
            },
            Kind::Ball {
                swing_x: x_before,
                swing_y: y_before,
                twist_lower: lower_before,
                twist_upper: upper_before,
                neutral_swing_x: neutral_x_before,
                neutral_swing_y: neutral_y_before,
                neutral_twist: neutral_twist_before,
            },
            Kind::Ball {
                swing_x: x_after,
                swing_y: y_after,
                twist_lower: lower_after,
                twist_upper: upper_after,
                neutral_swing_x: neutral_x_after,
                neutral_swing_y: neutral_y_after,
                neutral_twist: neutral_twist_after,
            },
        ) => JointKindDesc::Ball {
            swing_x: angle(swing_x, x_before, x_after),
            swing_y: angle(swing_y, y_before, y_after),
            twist_lower: angle(twist_lower, lower_before, lower_after),
            twist_upper: angle(twist_upper, upper_before, upper_after),
            neutral: SwingTwist {
                swing_x: angle(neutral.swing_x, neutral_x_before, neutral_x_after),
                swing_y: angle(neutral.swing_y, neutral_y_before, neutral_y_after),
                twist: angle(neutral.twist, neutral_twist_before, neutral_twist_after),
            },
            soft_swing,
            soft_twist,
        },
        (
            JointKindDesc::Hinge {
                lower,
                upper,
                neutral,
                soft,
            },
            Kind::Hinge {
                lower: lower_before,
                upper: upper_before,
                neutral: neutral_before,
            },
            Kind::Hinge {
                lower: lower_after,
                upper: upper_after,
                neutral: neutral_after,
            },
        ) => JointKindDesc::Hinge {
            lower: angle(lower, lower_before, lower_after),
            upper: angle(upper, upper_before, upper_after),
            neutral: angle(neutral, neutral_before, neutral_after),
            soft,
        },
        // A switch between the two that swing: the swing, in radians.
        (
            JointKindDesc::Ball {
                swing_x, swing_y, ..
            },
            _,
            Kind::Hinge { .. },
        ) => {
            let swing = swing_x.max(swing_y);
            JointKindDesc::Hinge {
                lower: -swing,
                upper: swing,
                neutral: 0.0,
                soft: None,
            }
        }
        (JointKindDesc::Hinge { lower, upper, .. }, _, Kind::Ball { .. }) => {
            let swing = lower.abs().max(upper.abs());
            JointKindDesc::Ball {
                swing_x: swing,
                swing_y: swing,
                twist_lower: 0.0,
                twist_upper: 0.0,
                neutral: SwingTwist::REST,
                soft_swing: None,
                soft_twist: None,
            }
        }
        // Any other switch: the new kind, from its rows.
        (
            _,
            _,
            Kind::Ball {
                swing_x,
                swing_y,
                twist_lower,
                twist_upper,
                neutral_swing_x,
                neutral_swing_y,
                neutral_twist,
            },
        ) => JointKindDesc::Ball {
            swing_x: swing_x.to_radians(),
            swing_y: swing_y.to_radians(),
            twist_lower: twist_lower.to_radians(),
            twist_upper: twist_upper.to_radians(),
            neutral: SwingTwist {
                swing_x: neutral_swing_x.to_radians(),
                swing_y: neutral_swing_y.to_radians(),
                twist: neutral_twist.to_radians(),
            },
            soft_swing: None,
            soft_twist: None,
        },
        (
            _,
            _,
            Kind::Hinge {
                lower,
                upper,
                neutral,
            },
        ) => JointKindDesc::Hinge {
            lower: lower.to_radians(),
            upper: upper.to_radians(),
            neutral: neutral.to_radians(),
            soft: None,
        },
        (_, _, Kind::Fixed) => JointKindDesc::Fixed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The drop-down offers each pair not listed, once, those touching at
    /// rest first and each part in the bodies' order**: a pair the file lists
    /// or the rest names the other way round counts all the same.
    #[test]
    fn only_pairs_not_listed_are_offered() {
        let body = |bone: &str| crcbl::ragdoll::BodyDesc {
            bone: bone.to_owned(),
            shape: ShapeDesc::Sphere { radius: 0.1 },
            mass: 1.0,
            centre: (0.0, 0.0, 0.0),
            axis: (0.0, 1.0, 0.0),
        };
        let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        let rig = RagdollDesc {
            bodies: vec![body("hips"), body("chest"), body("head"), body("hand")],
            joints: Vec::new(),
            ignore: vec![pair("head", "hips")],
            ignore_rest_overlaps: true,
            fold: Vec::new(),
            substeps: 0,
        };
        let touching = [pair("head", "chest"), pair("hips", "hand")];
        assert_eq!(
            unlisted_pairs(&rig, &touching),
            [
                (pair("hips", "hand"), true),
                (pair("chest", "head"), true),
                (pair("hips", "chest"), false),
                (pair("chest", "hand"), false),
                (pair("head", "hand"), false),
            ]
        );
    }

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

    /// A ball of `swing` about both axes and the given twist, with no
    /// neutral or soft zones.
    fn ball(swing: f64, twist_lower: f64, twist_upper: f64) -> JointKindDesc {
        JointKindDesc::Ball {
            swing_x: swing,
            swing_y: swing,
            twist_lower,
            twist_upper,
            neutral: SwingTwist::REST,
            soft_swing: None,
            soft_twist: None,
        }
    }

    /// A hinge between `lower` and `upper`, with no neutral or soft zone.
    fn hinge(lower: f64, upper: f64) -> JointKindDesc {
        JointKindDesc::Hinge {
            lower,
            upper,
            neutral: 0.0,
            soft: None,
        }
    }

    /// **A switched kind keeps its swing**: a ball's wider half-angle is a
    /// hinge's range either side of rest, and back again a ball with that
    /// swing and no twist; a fixed joint switched to a ball starts locked.
    #[test]
    fn a_switched_kind_keeps_its_swing() {
        let elliptical = JointKindDesc::Ball {
            swing_x: 0.5,
            swing_y: 0.7,
            twist_lower: -0.3,
            twist_upper: 0.2,
            neutral: SwingTwist {
                swing_x: 0.1,
                ..SwingTwist::REST
            },
            soft_swing: None,
            soft_twist: None,
        };
        let zero_hinge = Kind::Hinge {
            lower: 0.0,
            upper: 0.0,
            neutral: 0.0,
        };
        assert_eq!(
            kind_desc(elliptical, kind_rows(elliptical), zero_hinge),
            hinge(-0.7, 0.7)
        );
        let lopsided = hinge(-2.4, 0.1);
        let zero_ball = Kind::Ball {
            swing_x: 0.0,
            swing_y: 0.0,
            twist_lower: 0.0,
            twist_upper: 0.0,
            neutral_swing_x: 0.0,
            neutral_swing_y: 0.0,
            neutral_twist: 0.0,
        };
        assert_eq!(
            kind_desc(lopsided, kind_rows(lopsided), zero_ball),
            ball(2.4, 0.0, 0.0)
        );
        let fixed = JointKindDesc::Fixed;
        assert_eq!(
            kind_desc(fixed, kind_rows(fixed), zero_ball),
            ball(0.0, 0.0, 0.0)
        );
    }

    /// **An angle nobody moved is written back exactly as it was**, though
    /// its row showed it in degrees; the one that moved is in radians, and
    /// the soft zones, which have no rows, are kept.
    #[test]
    fn only_the_moved_angle_is_rewritten() {
        let soft = crcbl::ragdoll::SoftRange {
            lower: -0.3,
            upper: 0.1,
            hertz: 4.0,
            damping_ratio: 0.7,
        };
        let old = JointKindDesc::Ball {
            swing_x: 0.1,
            swing_y: 0.1,
            twist_lower: -0.4,
            twist_upper: 0.2,
            neutral: SwingTwist::REST,
            soft_swing: None,
            soft_twist: Some(soft),
        };
        // Both change on a trip through degrees, so a rewrite would show.
        assert_ne!(0.2_f64.to_degrees().to_radians(), 0.2);
        assert_ne!((-0.4_f64).to_degrees().to_radians(), -0.4);
        let before = kind_rows(old);
        let Kind::Ball {
            swing_y,
            twist_lower,
            twist_upper,
            neutral_swing_x,
            neutral_swing_y,
            neutral_twist,
            ..
        } = before
        else {
            panic!("a ball reads as a ball");
        };
        let after = Kind::Ball {
            swing_x: 90.0,
            swing_y,
            twist_lower,
            twist_upper,
            neutral_swing_x,
            neutral_swing_y,
            neutral_twist,
        };
        assert_eq!(
            kind_desc(old, before, after),
            JointKindDesc::Ball {
                swing_x: core::f64::consts::FRAC_PI_2,
                swing_y: 0.1,
                twist_lower: -0.4,
                twist_upper: 0.2,
                neutral: SwingTwist::REST,
                soft_swing: None,
                soft_twist: Some(soft),
            }
        );
    }
}
