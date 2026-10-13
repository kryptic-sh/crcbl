//! The body of the selected rigged mesh's ragdoll rig that translate's handles
//! move: **held by a click on it, moved by a drag of its arrows**.
//!
//! With one rigged mesh selected, a click whose ray meets one of its drawn
//! bodies holds the nearest one met rather than picking, and a click that
//! meets none lets go of it and picks as ever. A held body is drawn in the
//! selection's colour, and in translate mode the handles stand at its centre,
//! turned by its own axes in local space. Scale and rotate show no handles for
//! it, and a drag of it does not snap: the grid is the scene's, and a body's
//! centre is measured in its bone's frame.
//!
//! **A drag writes the body's `centre` in the rig file**, which is in its
//! bone's rigid frame: where the drag puts the body in the world, less where
//! it stood at the press, is turned back into that frame and added to the
//! centre the press found. Each move is an edit as an inspector row's is —
//! resolved at once, one step of the rig's own history however many frames it
//! moved, and written when the pointer lets go (`crate::app`'s
//! `write_edited_rig`).

use crcbl::math::{DQuat, DVec3, Vec2};
use crcbl::phys::query::{ray_vs_box, ray_vs_capsule, ray_vs_sphere};
use crcbl::phys::{BoxCollider, Capsule, Ray, Sphere, Transform};
use crcbl::ragdoll::BodyShape;
use crcbl::render::ViewRay;
use crcbl::scene::scn::SceneEntityId;
use crcbl::shell::Shell;

use super::{Editor, instances};
use crate::gizmo;
use crate::panel::Tone;

/// The rig body a click took hold of: which entity's rig, and which body of
/// it, in rig order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Held {
    /// The rigged mesh, the lone selection while the body is held.
    pub(super) entity: SceneEntityId,
    /// Its asset, whose rig file the drag writes.
    pub(super) asset: String,
    /// The body, as the file and the resolved rig both list it.
    pub(super) index: usize,
}

/// A drag of a held body's arrow or plane, from the press that took it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BodyDrag {
    held: Held,
    /// Where the body's centre stood in the world at the press.
    world: DVec3,
    /// Its `centre` in the file at the press.
    centre: DVec3,
    /// How its bone's rigid frame is turned in the world.
    bone: DQuat,
}

impl<S: Shell + ?Sized> Editor<S> {
    /// The held body's shape and where it stands, or [`None`] — letting go of
    /// it — once its entity is no longer the lone selection, is no longer a
    /// mesh of its asset, or its rig no longer has it.
    pub(super) fn held_body(&mut self) -> Option<(BodyShape, Transform)> {
        let held = self.held.clone()?;
        let found = self.body_of(&held);
        if found.is_none() {
            self.held = None;
        }
        found
    }

    /// `held`'s shape and where it stands at rest, where its mesh stands.
    fn body_of(&mut self, held: &Held) -> Option<(BodyShape, Transform)> {
        if self.document.selection() != [held.entity] {
            return None;
        }
        let mesh = self.document.mesh(held.entity)?;
        if mesh.asset != held.asset {
            return None;
        }
        let origin = instances::origin_of(mesh);
        self.shelf
            .rig(&held.asset)?
            .ragdoll_bodies(origin)
            .get(held.index)
            .copied()
    }

    /// Holds the body of the lone selected rigged mesh nearest along `ray`
    /// and says so on the status line, or lets go of the one held and says
    /// whether it held one — see the [module docs](self).
    pub(super) fn hold_body_at(&mut self, ray: &ViewRay) -> bool {
        self.held = None;
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
        let nearest = rig
            .ragdoll_bodies(origin)
            .iter()
            .enumerate()
            .filter_map(|(index, (shape, at))| Some((index, hit_body(&ray, *shape, at)?)))
            .min_by(|(_, near), (_, far)| near.total_cmp(far));
        let Some((index, _)) = nearest else {
            return false;
        };
        let bone = rig
            .desc
            .as_ref()
            .and_then(|desc| desc.bodies.get(index))
            .map_or_else(
                || format!("body {index}"),
                |body| format!("`{}`", body.bone),
            );
        self.panels.set_status(
            format!("Holding the body on {bone}: translate's arrows move its centre in the rig"),
            Tone::Info,
        );
        self.held = Some(Held {
            entity,
            asset,
            index,
        });
        true
    }

    /// A drag of the held body through `grip`, an arrow or a plane, its
    /// handles standing at `shown`'s centre and turned by its frame, pressed
    /// at `at` in window pixels — or [`None`] with no body held, for a grip
    /// that does not translate, or a press whose ray runs along the handle.
    pub(super) fn begin_body_drag(
        &mut self,
        grip: gizmo::Grip,
        gesture: crate::command::Gesture,
        shown: (crcbl::math::Vec3, DQuat),
        at: Vec2,
    ) -> Option<(gizmo::Drag, BodyDrag)> {
        let (_, standing) = self.held_body()?;
        let held = self.held.clone()?;
        let rig = self.shelf.rig(&held.asset)?;
        let offset = rig.ragdoll.as_ref()?.bodies().get(held.index)?.offset;
        let centre = DVec3::from(rig.desc.as_ref()?.bodies.get(held.index)?.centre);
        let drag = self.begin_drag(
            held.entity,
            grip,
            gesture,
            standing.position.to_array(),
            shown,
            at,
        )?;
        Some((
            drag,
            BodyDrag {
                held,
                world: standing.position,
                centre,
                // The body stands at its offset in the bone's frame, so the
                // frame is its turn less the offset's.
                bone: standing.rotation * offset.rotation.conjugate(),
            },
        ))
    }

    /// Moves the dragged body to where the pointer at `at` puts it: its
    /// `centre` set in the file, the rig resolved again, and the file marked
    /// to be written when the pointer lets go.
    pub(super) fn move_body(&mut self, drag: &gizmo::Drag, body: &BodyDrag, at: Vec2) {
        let pointer = self.pointer_at(at);
        let Some(landed) = drag.translated(&pointer, None) else {
            return;
        };
        let mut world = body.world;
        for (axis, value) in landed {
            world[axis.index()] = value;
        }
        let centre = body.centre + body.bone.conjugate() * (world - body.world);
        let asset = &body.held.asset;
        let Some(desc) = self.shelf.rig_mut(asset).and_then(|rig| rig.desc.as_mut()) else {
            return;
        };
        let before = desc.clone();
        let Some(moved) = desc.bodies.get_mut(body.held.index) else {
            return;
        };
        moved.centre = centre.into();
        self.rig_history.edited(asset, &before);
        self.rig_edited(asset.clone());
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
