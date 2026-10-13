//! The body of the selected rigged mesh's ragdoll rig that the handles move
//! and resize: **held by a click on it, moved by a drag of its arrows and
//! resized by its boxes**.
//!
//! With one rigged mesh selected, a click whose ray meets one of its drawn
//! bodies holds the nearest one met rather than picking, and a click that
//! meets none lets go of it and picks as ever. A click that meets the held
//! body holds the next one farther along the ray instead, the nearest again
//! after the farthest, so a body another hides is reached by clicking on. A
//! held body is drawn in the selection's colour. Translate's handles stand at
//! its centre, turned by its own axes in local space, and scale's always
//! are; rotate shows none, since an axis turned by a ring would lose the
//! body's turn about it, which the file does not keep.
//!
//! **Scale resizes the body as a box about its own axes would be**: a box's
//! half extents, a sphere's radius whichever box is dragged, and a capsule's
//! radius by its `x` or `z` box and its length by its `y` box, whose face is
//! the cap's tip. Ctrl snaps a size to the scale step, as it does an entity's
//! half extents: a size is the same in every frame.
//!
//! **Ctrl snaps how far a body drag moves, not where it lands**: the distance
//! along each of the handles' axes from the press is a multiple of the grid
//! step, as a turn snaps the angle swept. The scene's grid has no meaning for
//! a body, whose centre is measured in its bone's frame, so landing on it
//! would leave the file's numbers no rounder.
//!
//! **A move writes the body's `centre` in the rig file**, which is in its
//! bone's rigid frame: where the drag puts the body in the world, less where
//! it stood at the press, is turned back into that frame and added to the
//! centre the press found. Each drag is an edit as an inspector row's is —
//! resolved at once, one step of the rig's own history however many frames it
//! moved, and written when the pointer lets go (`crate::app`'s
//! `write_edited_rig`).

use crcbl::core::input::Modifiers;
use crcbl::math::{DQuat, DVec3, Vec2};
use crcbl::phys::query::{ray_vs_box, ray_vs_capsule, ray_vs_sphere};
use crcbl::phys::{BoxCollider, Capsule, Ray, Sphere, Transform};
use crcbl::ragdoll::{BodyShape, ShapeDesc};
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

/// A drag of a held body's arrow, plane or box, from the press that took it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BodyDrag {
    held: Held,
    /// Where the body's centre stood in the world at the press.
    world: DVec3,
    /// Its `centre` in the file at the press.
    centre: DVec3,
    /// Its shape in the file at the press.
    shape: ShapeDesc,
    /// How its bone's rigid frame is turned in the world.
    bone: DQuat,
    /// How the handles are turned in the world: the axes a snapped move is
    /// measured along.
    frame: DQuat,
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

    /// Holds the body of the lone selected rigged mesh nearest along `ray`,
    /// or the next past the one held, and says so on the status line, or lets
    /// go of the one held and says whether it held one — see the
    /// [module docs](self).
    pub(super) fn hold_body_at(&mut self, ray: &ViewRay) -> bool {
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
        let mut hits: Vec<(usize, f64)> = rig
            .ragdoll_bodies(origin)
            .iter()
            .enumerate()
            .filter_map(|(index, (shape, at))| Some((index, hit_body(&ray, *shape, at)?)))
            .collect();
        hits.sort_by(|(_, near), (_, far)| near.total_cmp(far));
        let held = was
            .filter(|was| was.entity == entity && was.asset == asset)
            .and_then(|was| hits.iter().position(|&(index, _)| index == was.index));
        let next = held.map_or(0, |at| (at + 1) % hits.len());
        let Some(&(index, _)) = hits.get(next) else {
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

    /// A drag of the held body through `grip`, an arrow, a plane or a box,
    /// its handles standing at `shown`'s centre and turned by its frame,
    /// pressed at `at` in window pixels — or [`None`] with no body held, for
    /// a ring, or a press whose ray runs along the handle.
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
        let desc = rig.desc.as_ref()?.bodies.get(held.index)?;
        let (centre, shape) = (DVec3::from(desc.centre), desc.shape);
        let start = match grip.mode() {
            gizmo::Mode::Translate => standing.position.to_array(),
            gizmo::Mode::Scale => extents(shape),
            gizmo::Mode::Rotate => return None,
        };
        let drag = self.begin_drag(held.entity, grip, gesture, start, shown, at)?;
        Some((
            drag,
            BodyDrag {
                held,
                world: standing.position,
                centre,
                shape,
                // The body stands at its offset in the bone's frame, so the
                // frame is its turn less the offset's.
                bone: standing.rotation * offset.rotation.conjugate(),
                frame: shown.1,
            },
        ))
    }

    /// Moves or resizes the dragged body to where the pointer at `at` puts
    /// it, snapped with Ctrl held: its `centre` or its shape set in the file,
    /// the rig resolved again, and the file marked to be written when the
    /// pointer lets go.
    pub(super) fn move_body(&mut self, drag: &gizmo::Drag, body: &BodyDrag, at: Vec2) {
        let pointer = self.pointer_at(at);
        let snapping = self.modifiers.contains(Modifiers::CTRL);
        let (centre, shape) = if drag.grip().mode() == gizmo::Mode::Scale {
            let Some(writes) = drag.writes(&pointer, snapping.then_some(self.snap)) else {
                return;
            };
            (body.centre, resized(body.shape, &writes))
        } else {
            let Some(landed) = drag.translated(&pointer, None) else {
                return;
            };
            let mut world = body.world;
            for (axis, value) in landed {
                world[axis.index()] = value;
            }
            let mut moved = world - body.world;
            if snapping {
                let along = body.frame.conjugate() * moved;
                moved =
                    body.frame * DVec3::from_array(along.to_array().map(|by| self.snap.grid(by)));
            }
            (body.centre + body.bone.conjugate() * moved, body.shape)
        };
        let asset = &body.held.asset;
        let Some(desc) = self.shelf.rig_mut(asset).and_then(|rig| rig.desc.as_mut()) else {
            return;
        };
        let before = desc.clone();
        let Some(moved) = desc.bodies.get_mut(body.held.index) else {
            return;
        };
        moved.centre = centre.into();
        moved.shape = shape;
        self.rig_history.edited(asset, &before);
        self.rig_edited(asset.clone());
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
