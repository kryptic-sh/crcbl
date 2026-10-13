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
//! **Translate moves either; scale resizes a body; rotate shows neither**.
//! Translate's handles stand at the part, turned by its own axes in local
//! space. Scale's boxes stand at a held body, turned by its axes always, and
//! resize it as a box about them would be: a box's half extents, a sphere's
//! radius whichever box is dragged, and a capsule's radius by its `x` or `z`
//! box and its length by its `y` box, whose face is the cap's tip. Ctrl snaps
//! a size to the scale step, as it does an entity's half extents: a size is
//! the same in every frame. A body's axis turned by a ring would lose the
//! body's turn about it, which the file does not keep.
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

    /// A drag of the held part through `grip`, an arrow, a plane or a body's
    /// box, its handles standing at `shown`'s centre and turned by its frame,
    /// pressed at `at` in window pixels — or [`None`] with nothing held, for
    /// a ring or a joint's box, or a press whose ray runs along the handle.
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
        let start = match (grip.mode(), shape) {
            (gizmo::Mode::Translate, _) => standing.position.to_array(),
            (gizmo::Mode::Scale, Some(shape)) => extents(shape),
            (gizmo::Mode::Scale, None) | (gizmo::Mode::Rotate, _) => return None,
        };
        let drag = self.begin_drag(held.entity, grip, gesture, start, shown, at)?;
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

    /// Moves the dragged part, or resizes the dragged body, to where the
    /// pointer at `at` puts it, snapped with Ctrl held: its place or its
    /// shape set in the file, the rig resolved again, and the file marked to
    /// be written when the pointer lets go.
    pub(super) fn move_rig(&mut self, drag: &gizmo::Drag, part: &RigDrag, at: Vec2) {
        let pointer = self.pointer_at(at);
        let snapping = self.modifiers.contains(Modifiers::CTRL);
        let (place, shape) = if drag.grip().mode() == gizmo::Mode::Scale {
            let Some(writes) = drag.writes(&pointer, snapping.then_some(self.snap)) else {
                return;
            };
            (part.place, part.shape.map(|shape| resized(shape, &writes)))
        } else {
            let Some(landed) = drag.translated(&pointer, None) else {
                return;
            };
            let mut world = part.world;
            for (axis, value) in landed {
                world[axis.index()] = value;
            }
            let mut moved = world - part.world;
            if snapping {
                let along = part.frame.conjugate() * moved;
                moved =
                    part.frame * DVec3::from_array(along.to_array().map(|by| self.snap.grid(by)));
            }
            (part.place + part.bone.conjugate() * moved, part.shape)
        };
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
