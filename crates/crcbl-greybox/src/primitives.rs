//! The prototyping primitives, every one sized in real-world metres.
//!
//! The world unit is one metre — physics is SI and the engine's unit cube spans
//! `[-0.5, 0.5]` — so a size passed here is a size in the game. Each generator
//! returns a [`Geometry::Flat`] ready to drop into a
//! [`MeshDesc`](crcbl_render::scene::MeshDesc); [`crate::scene3d`] assembles all
//! of them at the default sizes documented below.
//!
//! # Orientation
//!
//! `+Y` is up and the ground is the `XZ` plane, matching the engine's cameras.
//! Standing things — walls, columns, stairs, the capsule — rest their base on
//! `y = 0` and rise into `+Y`, so an instance placed at the origin sits on the
//! floor. The cube and the sphere are centred on the origin, because a die and a
//! ball have no up.

use std::f32::consts::{PI, TAU};

use crcbl_render::scene::Geometry;
use glam::Vec3;

use crate::build::MeshBuilder;

/// The edge of the unit cube, in metres — one metre, the size a size-less "cube"
/// means. See [`cube`] and [`unit_cube`].
pub const UNIT_CUBE_M: f32 = 1.0;

/// The angles [`ramp`] is offered at, in degrees: a gentle, a medium and a steep
/// slope. [`crate::scene3d`] blocks out the medium one.
pub const RAMP_ANGLES_DEG: [f32; 3] = [15.0, 30.0, 45.0];

/// The width a [`ramp`] is extruded to, in metres — the run and angle set its
/// profile, and this sets how wide the wedge is across.
pub const RAMP_WIDTH_M: f32 = 1.0;

/// An axis-aligned box `size_m` metres on every edge, centred on the origin.
///
/// Default size: **1 m** — a one-metre cube, whose bounds are `±0.5 m` on each
/// axis, the same span as the engine's own unit cube. See [`unit_cube`].
#[must_use]
pub fn cube(size_m: f32) -> Geometry<'static> {
    let half = size_m * 0.5;
    let mut builder = MeshBuilder::new();
    builder.cuboid(Vec3::splat(-half), Vec3::splat(half));
    builder.finish()
}

/// The one-metre unit cube — [`cube`] at [`UNIT_CUBE_M`], centred on the origin.
#[must_use]
pub fn unit_cube() -> Geometry<'static> {
    cube(UNIT_CUBE_M)
}

/// A single-sided floor plane on the `XZ` plane, facing `+Y`, centred on the
/// origin at `y = 0`.
///
/// Default size: **1 m × 1 m** (`w_m` across `X`, `d_m` across `Z`).
#[must_use]
pub fn quad(w_m: f32, d_m: f32) -> Geometry<'static> {
    let (hw, hd) = (w_m * 0.5, d_m * 0.5);
    let mut builder = MeshBuilder::new();
    builder.quad(
        [
            Vec3::new(-hw, 0.0, hd),
            Vec3::new(hw, 0.0, hd),
            Vec3::new(hw, 0.0, -hd),
            Vec3::new(-hw, 0.0, -hd),
        ],
        Vec3::Y,
    );
    builder.finish()
}

/// A thin panel: `w_m` wide across `X`, `h_m` tall from the floor up `+Y`, and
/// `thickness_m` deep across `Z`, centred on `X`/`Z` with its base on `y = 0`.
///
/// Default size: **1 m × 2 m × 0.1 m** — a two-metre wall a tenth of a metre
/// thick.
#[must_use]
pub fn wall(w_m: f32, h_m: f32, thickness_m: f32) -> Geometry<'static> {
    let (hw, ht) = (w_m * 0.5, thickness_m * 0.5);
    let mut builder = MeshBuilder::new();
    builder.cuboid(Vec3::new(-hw, 0.0, -ht), Vec3::new(hw, h_m, ht));
    builder.finish()
}

/// A [`wall`] with a rectangular opening cut from its base — a doorway frame,
/// modelled as two posts and a lintel so the opening reaches the floor.
///
/// `w_m` × `h_m` × `thickness_m` is the whole frame; the opening is
/// `opening_w_m` wide and `opening_h_m` tall, centred on `X` and rising from the
/// floor.
///
/// Default size: a **1.6 m × 2.5 m × 0.1 m** frame around a **1 m × 2.1 m**
/// opening — a standard doorway.
///
/// # Panics
///
/// If the opening is not strictly inside the frame — `opening_w_m < w_m` and
/// `opening_h_m < h_m` — since a frame with no post or no lintel is not a
/// doorway.
#[must_use]
pub fn doorway(
    w_m: f32,
    h_m: f32,
    thickness_m: f32,
    opening_w_m: f32,
    opening_h_m: f32,
) -> Geometry<'static> {
    assert!(
        opening_w_m < w_m && opening_h_m < h_m,
        "a doorway's opening ({opening_w_m} m × {opening_h_m} m) must be smaller than its \
         frame ({w_m} m × {h_m} m)"
    );
    let (hw, ht, ho) = (w_m * 0.5, thickness_m * 0.5, opening_w_m * 0.5);
    let mut builder = MeshBuilder::new();
    // Left post, right post, then the lintel spanning the opening.
    builder.cuboid(Vec3::new(-hw, 0.0, -ht), Vec3::new(-ho, h_m, ht));
    builder.cuboid(Vec3::new(ho, 0.0, -ht), Vec3::new(hw, h_m, ht));
    builder.cuboid(Vec3::new(-ho, opening_h_m, -ht), Vec3::new(ho, h_m, ht));
    builder.finish()
}

/// A wedge to walk up: a right-triangle prism `run_m` long across `Z`, rising to
/// `run_m · tan(angle_deg)` in `+Y`, [`RAMP_WIDTH_M`] wide across `X`, with its
/// low edge and base on `y = 0`.
///
/// Offered at the angles in [`RAMP_ANGLES_DEG`] — 15°, 30° and 45°, though any
/// angle in `(0, 90)` works. At 45° the rise equals the run.
///
/// # Panics
///
/// If `angle_deg` is not in `(0, 90)`, where the tangent that sets the rise is
/// undefined or negative.
#[must_use]
pub fn ramp(run_m: f32, angle_deg: f32) -> Geometry<'static> {
    assert!(
        angle_deg > 0.0 && angle_deg < 90.0,
        "a ramp's angle ({angle_deg}°) must be between 0 and 90 degrees"
    );
    let rise = run_m * angle_deg.to_radians().tan();
    let hw = RAMP_WIDTH_M * 0.5;
    let (a0, b0) = (Vec3::new(-hw, 0.0, 0.0), Vec3::new(hw, 0.0, 0.0));
    let (a1, b1) = (Vec3::new(-hw, 0.0, run_m), Vec3::new(hw, 0.0, run_m));
    let (at, bt) = (Vec3::new(-hw, rise, run_m), Vec3::new(hw, rise, run_m));

    let mut builder = MeshBuilder::new();
    builder.quad([a0, b0, b1, a1], Vec3::NEG_Y); // floor
    builder.quad([a1, b1, bt, at], Vec3::Z); // tall vertical face at the far end
    builder.quad([a0, b0, bt, at], Vec3::new(0.0, run_m, -rise)); // the walkable slope
    builder.tri([a0, a1, at], Vec3::NEG_X); // left side
    builder.tri([b0, b1, bt], Vec3::X); // right side
    builder.finish()
}

/// A solid staircase of `steps` steps, `width_m` wide across `X`, climbing `+Y`
/// as it advances `+Z`, with its base on `y = 0`.
///
/// Each step rises `rise_m` and runs `run_m`, so the whole flight is
/// `steps · rise_m` tall and `steps · run_m` deep.
///
/// Default step: **0.2 m rise, 0.28 m run** — a comfortable real-world stair.
///
/// # Panics
///
/// If `steps` is zero, which is not a staircase.
#[must_use]
pub fn stairs(width_m: f32, steps: u32, rise_m: f32, run_m: f32) -> Geometry<'static> {
    assert!(steps > 0, "a staircase needs at least one step");
    let hw = width_m * 0.5;
    let mut builder = MeshBuilder::new();
    for step in 0..steps {
        let front = step as f32 * run_m;
        let top = (step + 1) as f32 * rise_m;
        // A solid column from the floor to this step's tread, one run deep, so
        // the flight's profile is the staircase and its underside is closed.
        builder.cuboid(
            Vec3::new(-hw, 0.0, front),
            Vec3::new(hw, top, front + run_m),
        );
    }
    builder.finish()
}

/// A square pillar `side_m` on a side across `X`/`Z`, `height_m` tall from the
/// floor up `+Y`, centred on `X`/`Z`.
///
/// Default size: **0.3 m × 0.3 m × 3 m** — a three-metre structural column.
#[must_use]
pub fn column(side_m: f32, height_m: f32) -> Geometry<'static> {
    let half = side_m * 0.5;
    let mut builder = MeshBuilder::new();
    builder.cuboid(
        Vec3::new(-half, 0.0, -half),
        Vec3::new(half, height_m, half),
    );
    builder.finish()
}

/// A raised slab `w_m` × `d_m` across the floor and `h_m` thick, centred on
/// `X`/`Z` with its base on `y = 0`.
///
/// Default size: **1 m × 1 m × 0.2 m** — a low platform to stand on.
#[must_use]
pub fn platform(w_m: f32, d_m: f32, h_m: f32) -> Geometry<'static> {
    let (hw, hd) = (w_m * 0.5, d_m * 0.5);
    let mut builder = MeshBuilder::new();
    builder.cuboid(Vec3::new(-hw, 0.0, -hd), Vec3::new(hw, h_m, hd));
    builder.finish()
}

/// An upright cylinder of `radius_m` and `height_m`, its base on `y = 0` and its
/// axis on `+Y`, its side smooth-shaded over `segments` facets with flat caps.
///
/// Default size: **0.5 m radius, 1 m tall**, 24 segments.
///
/// # Panics
///
/// If `segments` is under three, which is not a cylinder.
#[must_use]
pub fn cylinder(radius_m: f32, height_m: f32, segments: u32) -> Geometry<'static> {
    assert!(segments >= 3, "a cylinder needs at least three segments");
    let mut builder = MeshBuilder::new();

    // Smooth side: two rings, radial normals, stitched as a grid.
    let mut side = Vec::with_capacity(2);
    for &y in &[0.0, height_m] {
        let mut row = Vec::with_capacity(segments as usize + 1);
        for j in 0..=segments {
            let theta = TAU * j as f32 / segments as f32;
            let (s, c) = theta.sin_cos();
            row.push(builder.vertex(
                Vec3::new(radius_m * c, y, radius_m * s),
                Vec3::new(c, 0.0, s),
                [j as f32 / segments as f32, y / height_m],
            ));
        }
        side.push(row);
    }
    builder.stitch_grid(&side);

    // Flat caps, each a fan around its centre with its own axial normals.
    cap(&mut builder, radius_m, height_m, segments, true);
    cap(&mut builder, radius_m, 0.0, segments, false);

    builder.finish()
}

/// One flat end cap of a [`cylinder`], a triangle fan wound to face `+Y` when
/// `up` and `-Y` otherwise.
fn cap(builder: &mut MeshBuilder, radius_m: f32, y: f32, segments: u32, up: bool) {
    let normal = if up { Vec3::Y } else { Vec3::NEG_Y };
    let center = builder.vertex(Vec3::new(0.0, y, 0.0), normal, [0.5, 0.5]);
    let mut ring = Vec::with_capacity(segments as usize + 1);
    for j in 0..=segments {
        let theta = TAU * j as f32 / segments as f32;
        let (s, c) = theta.sin_cos();
        ring.push(builder.vertex(
            Vec3::new(radius_m * c, y, radius_m * s),
            normal,
            [0.5 + 0.5 * c, 0.5 + 0.5 * s],
        ));
    }
    for j in 0..segments as usize {
        if up {
            builder.triangle(center, ring[j + 1], ring[j]);
        } else {
            builder.triangle(center, ring[j], ring[j + 1]);
        }
    }
}

/// A UV sphere of `radius_m`, centred on the origin, with `rings` latitude bands
/// and `segments` longitude columns and smooth radial normals.
///
/// Default size: **0.5 m radius** (a one-metre ball), 16 rings, 24 segments.
///
/// # Panics
///
/// If `rings` is under two or `segments` is under three, below which there is no
/// closed surface.
#[must_use]
pub fn sphere(radius_m: f32, rings: u32, segments: u32) -> Geometry<'static> {
    assert!(
        rings >= 2 && segments >= 3,
        "a sphere needs at least two rings and three segments"
    );
    let mut builder = MeshBuilder::new();
    let mut grid = Vec::with_capacity(rings as usize + 1);
    for i in 0..=rings {
        // Bottom pole to top pole, so rows ascend in +Y for `stitch_grid`.
        let phi = PI * (1.0 - i as f32 / rings as f32);
        let (sphi, cphi) = phi.sin_cos();
        let (y, ring_radius) = (radius_m * cphi, radius_m * sphi);
        let v = i as f32 / rings as f32;
        // The two poles are a single shared vertex, not a ring: `sin(π)` is not
        // exactly zero in `f32`, so a computed pole ring would be a knot of
        // near-coincident points with flipped winding. A row of one repeated
        // index makes the pole a point and `stitch_grid` fan to it.
        let row = if i == 0 || i == rings {
            let pole = builder.vertex(
                Vec3::new(0.0, y, 0.0),
                if cphi < 0.0 { Vec3::NEG_Y } else { Vec3::Y },
                [0.5, v],
            );
            vec![pole; segments as usize + 1]
        } else {
            (0..=segments)
                .map(|j| {
                    let theta = TAU * j as f32 / segments as f32;
                    let (s, c) = theta.sin_cos();
                    let position = Vec3::new(ring_radius * c, y, ring_radius * s);
                    builder.vertex(
                        position,
                        position.normalize_or(Vec3::Y),
                        [j as f32 / segments as f32, v],
                    )
                })
                .collect()
        };
        grid.push(row);
    }
    builder.stitch_grid(&grid);
    builder.finish()
}

/// The scale figure: an upright capsule of `radius_m` and total tip-to-tip
/// `height_m`, its base on `y = 0` and its axis on `+Y` — a cylinder capped by
/// two hemispheres, smooth-shaded.
///
/// Default size: **0.5 m diameter × 1.8 m tall** (`radius_m` = 0.25) — a human
/// reference to check a blockout against. `rings` is the latitude bands **per
/// hemisphere** and `segments` the longitude columns.
///
/// # Panics
///
/// If `height_m < 2 · radius_m` (the hemispheres would overlap), `rings` is
/// under one, or `segments` is under three.
#[must_use]
pub fn capsule(radius_m: f32, height_m: f32, rings: u32, segments: u32) -> Geometry<'static> {
    assert!(
        height_m >= 2.0 * radius_m,
        "a capsule's height ({height_m} m) must be at least its diameter ({} m)",
        2.0 * radius_m
    );
    assert!(
        rings >= 1 && segments >= 3,
        "a capsule needs at least one ring per hemisphere and three segments"
    );
    let spec = CapsuleSpec {
        radius_m,
        height_m,
        rings,
        segments,
    };
    let mut builder = MeshBuilder::new();
    let mut grid = Vec::with_capacity(2 * (rings as usize + 1));

    // The rows run bottom tip → bottom equator → top equator → top tip. The two
    // equator rows sit `height - 2·radius` apart, so the band between them is
    // the straight cylinder and the two hemispheres cap it.
    //
    // Bottom hemisphere: phi from π (down pole) to π/2 (equator).
    capsule_hemisphere(&mut builder, &mut grid, spec, radius_m, |f| {
        PI - (PI / 2.0) * f
    });
    // Top hemisphere: phi from π/2 (equator) to 0 (up pole).
    capsule_hemisphere(&mut builder, &mut grid, spec, height_m - radius_m, |f| {
        (PI / 2.0) * (1.0 - f)
    });

    builder.stitch_grid(&grid);
    builder.finish()
}

/// The dimensions and tessellation of a [`capsule`], shared by its two
/// hemispheres.
#[derive(Clone, Copy)]
struct CapsuleSpec {
    radius_m: f32,
    height_m: f32,
    rings: u32,
    segments: u32,
}

/// One hemisphere of a [`capsule`], as `rings + 1` rows of `segments + 1`
/// vertices appended to `grid`, its pole at `center_y ± radius_m`.
///
/// `phi_of` maps a `0..=1` sweep to the colatitude, so the caller decides which
/// way the hemisphere opens.
fn capsule_hemisphere(
    builder: &mut MeshBuilder,
    grid: &mut Vec<Vec<u32>>,
    spec: CapsuleSpec,
    center_y: f32,
    phi_of: impl Fn(f32) -> f32,
) {
    for t in 0..=spec.rings {
        let phi = phi_of(t as f32 / spec.rings as f32);
        let (sphi, cphi) = phi.sin_cos();
        let y = center_y + spec.radius_m * cphi;
        let v = y / spec.height_m;
        // A tip is a point, not a ring — collapse it to one shared vertex, for
        // the reason [`sphere`] does: `sin` at the pole is a hair off zero and a
        // computed ring there winds inconsistently.
        let row = if sphi.abs() < 1e-6 {
            let tip = builder.vertex(
                Vec3::new(0.0, y, 0.0),
                if cphi < 0.0 { Vec3::NEG_Y } else { Vec3::Y },
                [0.5, v],
            );
            vec![tip; spec.segments as usize + 1]
        } else {
            (0..=spec.segments)
                .map(|j| {
                    let theta = TAU * j as f32 / spec.segments as f32;
                    let (s, c) = theta.sin_cos();
                    builder.vertex(
                        Vec3::new(spec.radius_m * sphi * c, y, spec.radius_m * sphi * s),
                        Vec3::new(sphi * c, cphi, sphi * s).normalize_or(Vec3::Y),
                        [j as f32 / spec.segments as f32, v],
                    )
                })
                .collect()
        };
        grid.push(row);
    }
}
