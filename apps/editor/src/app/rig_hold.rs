//! The body or joint of the selected rigged mesh's ragdoll rig that the
//! handles move: **held by a click on it, moved by a drag of its arrows, and
//! a body resized by its boxes**.
//!
//! With one rigged mesh selected, its joints are drawn as small spheres, each
//! with a line along its axis, beside its bodies. A click whose ray meets a
//! drawn joint or body holds it rather than picking — a joint met before any
//! body, since a joint sits inside the bodies it joins and is the smaller
//! mark to aim at, and the nearest of each kind first — and a click that meets
//! none lets go of it and picks as ever. A click that meets the held part
//! holds the next one met in that order instead, the first again after the
//! last, so a part another hides is reached by clicking on. A held part is
//! drawn in the selection's colour.
//!
//! **A press on a handle let go with the pointer never off the pixel it was
//! pressed on is a click on the part behind it**: the mesh's own handles, or
//! the held part's, stand over the parts at them, and a drag that never left
//! its press has moved nothing, so the click holds the part there as it
//! would off the handles. One that meets no part changes nothing. The pointer
//! anywhere else, even a pixel off, makes the press a drag.
//!
//! **Shift+click on another body lists it with the held one as a pair that
//! never collides**, or takes the pair off the list if it is on it, and the
//! held body stays held: picking a pair's two bodies where they are drawn
//! rather than finding the pair in the inspector's list of every one.
//!
//! **Translate moves either; scale resizes a body; rotate turns a joint's
//! axis**. Translate's handles stand at the part, and rotate's rings at a
//! joint, turned by its own axes in local space. Scale's boxes stand at a held
//! body, turned by its axes always, and
//! resize it as a box about them would be: a box's half extents, a sphere's
//! radius whichever box is dragged, and a capsule's radius by its `x` or `z`
//! box and its length by its `y` box, whose face is the cap's tip. Ctrl snaps
//! a size to the scale step, as it does an entity's half extents: a size is
//! the same in every frame.
//!
//! **A ring turns a joint's axis and never a body's**: the file gives either
//! an axis alone, with no turn about it. A joint is the same about its axis
//! whatever that turn, so a ring writes exactly the axis it leaves the
//! joint's `z` on — the ring about the axis itself changes nothing — while a
//! body turned about its axis would lose the turn the moment it was written.
//! Ctrl snaps the angle swept, as it does an entity's.
//!
//! **Ctrl snaps how far a move goes, not where it lands**: the distance along
//! each of the handles' axes from the press is a multiple of the grid step,
//! as a turn snaps the angle swept. The scene's grid has no meaning for a
//! body's centre or a joint's place, each measured in a bone's frame, so
//! landing on it would leave the file's numbers no rounder.
//!
//! **A move writes a body's `centre` or a joint's `at` in the rig file**,
//! each in a bone's rigid frame — the body's own bone, or the joint's child
//! bone: where the drag puts the part in the world, less where it stood at
//! the press, is turned back into that frame and added to what the press
//! found. Each drag is an edit as an inspector row's is — resolved at once,
//! one step of the rig's own history however many frames it moved, and
//! written when the pointer lets go (`crate::app`'s `write_edited_rig`).

use crcbl::core::input::Modifiers;
use crcbl::math::{DQuat, DVec3, Vec2};
use crcbl::phys::query::{ray_vs_box, ray_vs_capsule, ray_vs_sphere};
use crcbl::phys::{BoxCollider, Capsule, Ray, Sphere, Transform};
use crcbl::ragdoll::{BodyShape, RagdollDesc, ShapeDesc};
use crcbl::render::ViewRay;
use crcbl::scene::scn::SceneEntityId;
use crcbl::shell::Shell;

use super::{Editor, instances};
use crate::gizmo;
use crate::panel::Tone;

/// The radius a joint is drawn and clicked at, in metres: small beside a
/// limb, and large enough to aim at on a humanoid filling the view.
pub(super) const JOINT_RADIUS: f64 = 0.03;

/// The part of a rig a click took hold of, by its place in the rig, which
/// the file and the resolved rig list alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Part {
    /// A body.
    Body(usize),
    /// A joint.
    Joint(usize),
}

/// The rig part a click took hold of: which entity's rig, and which part of
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Held {
    /// The rigged mesh, the lone selection while the part is held.
    pub(super) entity: SceneEntityId,
    /// Its asset, whose rig file the drag writes.
    pub(super) asset: String,
    /// The part.
    pub(super) part: Part,
}

/// A drag of a held part's arrow, plane or box, from the press that took it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RigDrag {
    held: Held,
    /// Where the part stood in the world at the press.
    world: DVec3,
    /// Its place in the file at the press: a body's `centre`, a joint's `at`.
    place: DVec3,
    /// A body's shape in the file at the press; [`None`] for a joint.
    shape: Option<ShapeDesc>,
    /// How the bone's rigid frame its place is measured in is turned in the
    /// world.
    bone: DQuat,
    /// How the handles are turned in the world: the axes a snapped move is
    /// measured along.
    frame: DQuat,
}

impl<S: Shell + ?Sized> Editor<S> {
    /// The held part and its frame in the world — a body's transform, or a
    /// joint's frame, its `z` the joint's axis — or [`None`], letting go of
    /// it, once its entity is no longer the lone selection, is no longer a
    /// mesh of its asset, or its rig no longer has it.
    pub(super) fn held_part(&mut self) -> Option<(Part, Transform)> {
        let held = self.held.clone()?;
        let found = self.placed(&held);
        if found.is_none() {
            self.held = None;
        }
        found.map(|(at, _)| (held.part, at))
    }

    /// `held`'s frame at rest, where its mesh stands, and how the bone its
    /// place is measured in is turned.
    fn placed(&mut self, held: &Held) -> Option<(Transform, DQuat)> {
        if self.document.selection() != [held.entity] {
            return None;
        }
        let mesh = self.document.mesh(held.entity)?;
        if mesh.asset != held.asset {
            return None;
        }
        let origin = instances::origin_of(mesh);
        let rig = self.shelf.rig(&held.asset)?;
        match held.part {
            Part::Body(index) => {
                let (_, at) = *rig.ragdoll_bodies(origin).get(index)?;
                let offset = rig.ragdoll.as_ref()?.bodies().get(index)?.offset;
                // The body stands at its offset in the bone's frame, so the
                // frame is its turn less the offset's.
                Some((at, at.rotation * offset.rotation.conjugate()))
            }
            Part::Joint(index) => rig.ragdoll_joints(origin).get(index).copied(),
        }
    }

    /// Lists the held body with the nearest other body along `ray` as a pair
    /// that never collides, or takes the pair off the list, as an edit the
    /// inspector's pair section makes — or says why not on the status line
    /// when no other body is met. Whether a body was held, so the click was
    /// this — see the [module docs](self).
    pub(super) fn pair_at(&mut self, ray: &ViewRay) -> bool {
        let Some(held) = self.held.clone() else {
            return false;
        };
        let Part::Body(first) = held.part else {
            return false;
        };
        if self.placed(&held).is_none() {
            self.held = None;
            return false;
        }
        let Some(mesh) = self.document.mesh(held.entity) else {
            return false;
        };
        let origin = instances::origin_of(mesh);
        let Some(rig) = self.shelf.rig(&held.asset) else {
            return false;
        };
        let ray = Ray::new(ray.origin.as_dvec3(), ray.direction.as_dvec3());
        let other = rig
            .ragdoll_bodies(origin)
            .into_iter()
            .enumerate()
            .filter(|&(index, _)| index != first)
            .filter_map(|(index, (shape, at))| Some((index, hit_body(&ray, shape, &at)?)))
            .min_by(|(_, near), (_, far)| near.total_cmp(far));
        let Some((second, _)) = other else {
            self.panels.set_status(
                "Shift+click another of the rig's bodies to pair it with the held one",
                Tone::Warning,
            );
            return true;
        };
        let asset = held.asset;
        let Some(desc) = self.shelf.rig_mut(&asset).and_then(|rig| rig.desc.as_mut()) else {
            return true;
        };
        let (Some(a), Some(b)) = (desc.bodies.get(first), desc.bodies.get(second)) else {
            return true;
        };
        let (a, b) = (a.bone.clone(), b.bone.clone());
        let before = desc.clone();
        let listed = desc
            .ignore
            .iter()
            .position(|(x, y)| (*x == a && *y == b) || (*x == b && *y == a));
        match listed {
            Some(at) => {
                desc.ignore.remove(at);
            }
            None => desc.ignore.push((a, b)),
        }
        self.rig_history.edited(&asset, &before);
        self.rig_edited(asset);
        true
    }

    /// Takes a press on a handle let go with the pointer never off where it
    /// was pressed as a click on the rig part the handle stands over: holds it, or
    /// the next past the one held, as [`hold_rig_at`](Self::hold_rig_at)
    /// does. A click that meets no part leaves what is held as it was — see
    /// the [module docs](self).
    pub(super) fn click_through_handle(&mut self) {
        let Some(at) = self.pressed_still.take() else {
            return;
        };
        let ray = self.ray_at(at);
        let was = self.held.clone();
        if !self.hold_rig_at(&ray) {
            self.held = was;
        }
    }

    /// Holds the joint or body of the lone selected rigged mesh first met
    /// along `ray`, or the next past the one held, and says so on the status
    /// line, or lets go of the one held and says whether it held one — see
    /// the [module docs](self).
    pub(super) fn hold_rig_at(&mut self, ray: &ViewRay) -> bool {
        let was = self.held.take();
        let &[entity] = self.document.selection() else {
            return false;
        };
        let Some(mesh) = self.document.mesh(entity) else {
            return false;
        };
        let (asset, origin) = (mesh.asset.clone(), instances::origin_of(mesh));
        let Some(rig) = self.shelf.rig(&asset) else {
            return false;
        };
        let ray = Ray::new(ray.origin.as_dvec3(), ray.direction.as_dvec3());
        let joint = BodyShape::Sphere {
            radius: JOINT_RADIUS,
        };
        let joints = rig.ragdoll_joints(origin).into_iter().enumerate();
        let bodies = rig.ragdoll_bodies(origin).into_iter().enumerate();
        let mut hits: Vec<(Part, f64)> = joints
            .filter_map(|(index, (at, _))| Some((Part::Joint(index), hit_body(&ray, joint, &at)?)))
            .chain(bodies.filter_map(|(index, (shape, at))| {
                Some((Part::Body(index), hit_body(&ray, shape, &at)?))
            }))
            .collect();
        hits.sort_by(|(part, near), (other, far)| {
            let body = |part: &Part| matches!(part, Part::Body(_));
            body(part).cmp(&body(other)).then(near.total_cmp(far))
        });
        let held = was
            .filter(|was| was.entity == entity && was.asset == asset)
            .and_then(|was| hits.iter().position(|&(part, _)| part == was.part));
        let next = held.map_or(0, |at| (at + 1) % hits.len());
        let Some(&(part, _)) = hits.get(next) else {
            return false;
        };
        self.panels
            .set_status(holding(rig.desc.as_ref(), part), Tone::Info);
        self.held = Some(Held {
            entity,
            asset,
            part,
        });
        true
    }

    /// A drag of the held part through `grip`, an arrow, a plane, a body's
    /// box or a joint's ring, its handles standing at `shown`'s centre and
    /// turned by its frame, pressed at `at` in window pixels — or [`None`]
    /// with nothing held, for a joint's box or a body's ring, or a press
    /// whose ray runs along the handle.
    pub(super) fn begin_rig_drag(
        &mut self,
        grip: gizmo::Grip,
        gesture: crate::command::Gesture,
        shown: (crcbl::math::Vec3, DQuat),
        at: Vec2,
    ) -> Option<(gizmo::Drag, RigDrag)> {
        let held = self.held.clone()?;
        let Some((standing, bone)) = self.placed(&held) else {
            self.held = None;
            return None;
        };
        let desc = self.shelf.rig(&held.asset)?.desc.as_ref()?;
        let (place, shape) = match held.part {
            Part::Body(index) => {
                let body = desc.bodies.get(index)?;
                (DVec3::from(body.centre), Some(body.shape))
            }
            Part::Joint(index) => (DVec3::from(desc.joints.get(index)?.at), None),
        };
        let drag = match (grip, shape) {
            (gizmo::Grip::Rotate(axis), None) => {
                let (corner, _) = self.panels.viewport_pixels();
                let standing = (standing.rotation, standing.position);
                self.turn_from(
                    held.entity,
                    axis,
                    gesture,
                    (shown.0, at - corner),
                    standing,
                    None,
                )?
            }
            (gizmo::Grip::Rotate(_), Some(_))
            | (gizmo::Grip::Scale(_) | gizmo::Grip::ScaleAll, None) => return None,
            (gizmo::Grip::Scale(_) | gizmo::Grip::ScaleAll, Some(shape)) => {
                self.begin_drag(held.entity, grip, gesture, extents(shape), shown, at)?
            }
            (gizmo::Grip::Move(_) | gizmo::Grip::MovePlane(_), _) => {
                let start = standing.position.to_array();
                self.begin_drag(held.entity, grip, gesture, start, shown, at)?
            }
        };
        Some((
            drag,
            RigDrag {
                held,
                world: standing.position,
                place,
                shape,
                bone,
                frame: shown.1,
            },
        ))
    }

    /// Moves the dragged part, resizes the dragged body or turns the dragged
    /// joint to where the pointer at `at` puts it, snapped with Ctrl held: its
    /// place, its shape or its axis set in the file, the rig resolved again,
    /// and the file marked to be written when the pointer lets go.
    pub(super) fn move_rig(&mut self, drag: &gizmo::Drag, part: &RigDrag, at: Vec2) {
        let pointer = self.pointer_at(at);
        let snap = self
            .modifiers
            .contains(Modifiers::CTRL)
            .then_some(self.snap);
        let (mut place, mut shape, mut axis) = (part.place, part.shape, None);
        match drag.grip().mode() {
            gizmo::Mode::Scale => {
                let Some(writes) = drag.writes(&pointer, snap) else {
                    return;
                };
                shape = part.shape.map(|shape| resized(shape, &writes));
            }
            gizmo::Mode::Rotate => {
                let Some(writes) = drag.writes(&pointer, snap) else {
                    return;
                };
                let Some(turned) = rotation_of(&writes) else {
                    return;
                };
                axis = Some(part.bone.conjugate() * (turned * DVec3::Z));
            }
            gizmo::Mode::Translate => {
                let Some(landed) = drag.translated(&pointer, None) else {
                    return;
                };
                let mut world = part.world;
                for (axis, value) in landed {
                    world[axis.index()] = value;
                }
                let mut moved = world - part.world;
                if let Some(snap) = snap {
                    let along = part.frame.conjugate() * moved;
                    moved =
                        part.frame * DVec3::from_array(along.to_array().map(|by| snap.grid(by)));
                }
                place = part.place + part.bone.conjugate() * moved;
            }
        }
        let asset = &part.held.asset;
        let Some(desc) = self.shelf.rig_mut(asset).and_then(|rig| rig.desc.as_mut()) else {
            return;
        };
        let before = desc.clone();
        match part.held.part {
            Part::Body(index) => {
                let Some(body) = desc.bodies.get_mut(index) else {
                    return;
                };
                body.centre = place.into();
                if let Some(shape) = shape {
                    body.shape = shape;
                }
            }
            Part::Joint(index) => {
                let Some(joint) = desc.joints.get_mut(index) else {
                    return;
                };
                joint.at = place.into();
                if let Some(axis) = axis {
                    joint.axis = axis.into();
                }
            }
        }
        self.rig_history.edited(asset, &before);
        self.rig_edited(asset.clone());
    }
}

/// What the status line says on taking hold of `part` of a rig whose file is
/// `desc`, naming it by its bones.
fn holding(desc: Option<&RagdollDesc>, part: Part) -> String {
    match part {
        Part::Body(index) => {
            let bone = desc.and_then(|desc| desc.bodies.get(index)).map_or_else(
                || format!("body {index}"),
                |body| format!("`{}`", body.bone),
            );
            format!(
                "Holding the body on {bone}: translate's arrows move its centre and scale's \
                 boxes resize it in the rig"
            )
        }
        Part::Joint(index) => {
            let bones = desc.and_then(|desc| desc.joints.get(index)).map_or_else(
                || format!("joint {index}"),
                |joint| format!("`{}` → `{}`", joint.parent, joint.child),
            );
            format!("Holding the joint {bones}: translate's arrows move where it sits in the rig")
        }
    }
}

/// The rotation a ring drag's `writes` set, from their `rotation` leaves in
/// [`crcbl::registry::Rotation::LEAVES`] order, or [`None`] without all four.
fn rotation_of(writes: &[gizmo::Write]) -> Option<DQuat> {
    let leaf = |name: &str| {
        let path = format!("{}.{name}", gizmo::ROTATION);
        writes
            .iter()
            .find(|write| write.path == path)
            .map(|write| write.value)
    };
    let [x, y, z, w] = crcbl::registry::Rotation::LEAVES;
    Some(DQuat::from_xyzw(leaf(x)?, leaf(y)?, leaf(z)?, leaf(w)?))
}

/// `shape`'s half extents along its own axes, as scale's handles size a box:
/// a capsule's `y` reaches the tip of its cap.
fn extents(shape: ShapeDesc) -> [f64; 3] {
    match shape {
        ShapeDesc::Capsule {
            radius,
            half_height,
        } => [radius, half_height + radius, radius],
        ShapeDesc::Sphere { radius } => [radius; 3],
        ShapeDesc::Box {
            half_extents: (x, y, z),
        } => [x, y, z],
    }
}

/// `shape` with the half extents a scale drag `writes` set — see the
/// [module docs](self). A capsule's length shrinks no further than its caps
/// meeting.
fn resized(shape: ShapeDesc, writes: &[gizmo::Write]) -> ShapeDesc {
    let written = |index: usize| {
        writes
            .iter()
            .find(|write| write.path.rsplit('.').next() == Some(index.to_string().as_str()))
            .map(|write| write.value)
    };
    match shape {
        ShapeDesc::Capsule {
            radius,
            half_height,
        } => {
            let radius = written(0).or_else(|| written(2)).unwrap_or(radius);
            let half_height = written(1).map_or(half_height, |tip| (tip - radius).max(0.0));
            ShapeDesc::Capsule {
                radius,
                half_height,
            }
        }
        ShapeDesc::Sphere { radius } => ShapeDesc::Sphere {
            radius: (0..3).find_map(written).unwrap_or(radius),
        },
        ShapeDesc::Box {
            half_extents: (x, y, z),
        } => ShapeDesc::Box {
            half_extents: (
                written(0).unwrap_or(x),
                written(1).unwrap_or(y),
                written(2).unwrap_or(z),
            ),
        },
    }
}

/// How far along `ray` it first meets a body of `shape` standing at `at`, or
/// [`None`] where it misses: the ray is turned into the body's own frame, where
/// every shape stands untilted at the origin.
fn hit_body(ray: &Ray, shape: BodyShape, at: &Transform) -> Option<f64> {
    let turn = at.rotation.conjugate();
    let local = Ray {
        origin: turn * (ray.origin - at.position),
        dir: turn * ray.dir,
        ..*ray
    };
    let hit = match shape {
        BodyShape::Capsule {
            radius,
            half_height,
        } => ray_vs_capsule(
            &local,
            &Capsule {
                centre: DVec3::ZERO,
                radius,
                half_height,
            },
        ),
        BodyShape::Sphere { radius } => ray_vs_sphere(
            &local,
            &Sphere {
                centre: DVec3::ZERO,
                radius,
            },
        ),
        BodyShape::Box { half_extents } => ray_vs_box(
            &local,
            &BoxCollider {
                centre: DVec3::ZERO,
                half_extents,
                rotation: DQuat::IDENTITY,
            },
        ),
    }?;
    Some(hit.t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writes(set: &[(usize, f64)]) -> Vec<gizmo::Write> {
        set.iter()
            .map(|&(axis, value)| gizmo::Write {
                path: format!("{}.{axis}", gizmo::HALF_EXTENTS),
                value,
            })
            .collect()
    }

    /// **A sphere takes whichever box was dragged as its radius, and a box
    /// each half extent written**, the rest kept.
    #[test]
    fn a_sphere_and_a_box_take_what_was_written() {
        let sphere = ShapeDesc::Sphere { radius: 0.1 };
        for axis in 0..3 {
            assert_eq!(
                resized(sphere, &writes(&[(axis, 0.3)])),
                ShapeDesc::Sphere { radius: 0.3 },
                "box {axis}"
            );
        }
        let box_shape = ShapeDesc::Box {
            half_extents: (0.1, 0.2, 0.3),
        };
        assert_eq!(
            resized(box_shape, &writes(&[(1, 0.5)])),
            ShapeDesc::Box {
                half_extents: (0.1, 0.5, 0.3)
            }
        );
    }

    /// **A capsule's `y` box is its tip, and its caps meet at the least**: an
    /// even resize scales its radius and its cylinder alike, and a tip pulled
    /// inside the cap leaves a sphere-long capsule rather than a negative one.
    #[test]
    fn a_capsule_resizes_by_its_tip() {
        let capsule = ShapeDesc::Capsule {
            radius: 0.1,
            half_height: 0.2,
        };
        assert_eq!(extents(capsule), [0.1, 0.1 + 0.2, 0.1]);
        let doubled = extents(capsule).map(|extent| extent * 2.0);
        let ShapeDesc::Capsule {
            radius,
            half_height,
        } = resized(
            capsule,
            &writes(&[(0, doubled[0]), (1, doubled[1]), (2, doubled[2])]),
        )
        else {
            panic!("still a capsule");
        };
        assert!((radius - 0.2).abs() < 1e-12 && (half_height - 0.4).abs() < 1e-12);
        assert_eq!(
            resized(capsule, &writes(&[(1, 0.05)])),
            ShapeDesc::Capsule {
                radius: 0.1,
                half_height: 0.0
            }
        );
        assert_eq!(
            resized(capsule, &writes(&[(2, 0.15)])),
            ShapeDesc::Capsule {
                radius: 0.15,
                half_height: 0.2
            }
        );
    }
}
