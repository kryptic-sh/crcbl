//! The zone's light: the braziers' torches, the shrine's spot, and the
//! irradiance volume baked from both.
//!
//! ```text
//!   zone::LAYOUT ──▶ torches(seconds, lit)  ──▶ ForwardRenderer::set_lights   (every frame)
//!         │                    │
//!         │                    └──▶ probes() ──▶ SceneDesc::probes            (once, at build)
//!         │
//!         └──▶ zone::world() ──▶ cast_ray ──▶ what a probe can actually see
//! ```
//!
//! # This is the load, not the feature
//!
//! `docs/plan/sample/15-shard.md`'s milestone 1 exists to put **real content**
//! through the raster twin: "point and spot shadows, screen-space AO and
//! reflections, irradiance probes — every raster effect topic 18 owes, in
//! lighting conditions that make errors obvious. A dark interior is the honest
//! test; daylight hides exactly the mistakes this path is prone to." Every one of
//! those already exists — `crates/crcbl-render/src/shadow.rs`, `effects.rs` and
//! `probe.rs` are where they live, and `apps/lantern` and `apps/quarry` are the
//! fixtures that accept them. What this module is is the **load**: a dozen
//! lights in a dark room with stone between them.
//!
//! # The engine picks which lights cast, and there are more than there are slots
//!
//! There is no "casts shadows" flag on a [`Light`]. The renderer's own
//! [`Selection`](crcbl::render::shadow::Selection) ranks the frame's lights by
//! `radius / distance-to-eye` and hands the atlas's tiles to the best
//! [`LIGHT_SLOTS`](crcbl::render::shadow::LIGHT_SLOTS) of them — and the tile
//! region holds two point lights' cubes beside a pair of spots' maps. So what a
//! visitor sees is the **nearest braziers** casting and the shrine's spot
//! casting, and the torches further off lighting without occluding. That is a
//! property of the engine rather than of this zone, and the reason this zone has
//! more lights than slots is that a sample which only ever offered as many as
//! fit would never exercise the selection at all.
//!
//! Until 2026-08-26 the region was one tile past a single cube, so exactly one
//! torch could occlude — and since this zone's braziers flank the walkway in
//! pairs, the twin that lost the tie *re-lit* the shadow the winner cast. That
//! is the defect the atlas's re-tiling fixed, and it is visible here rather than
//! anywhere else because a symmetrical rig is what makes a missing occluder look
//! like a working light.
//!
//! # The spot has something standing in it
//!
//! [`spot`] stands in the shrine, above and behind the doorway at
//! [`zone::LAYOUT`]'s row 4, aimed down the corridor —
//! so the doorway's own posts are **inside the cone** and their shadows run down
//! the corridor floor. `apps/lantern/src/room.rs` records why that is not
//! decoration: with nothing in the cone, "a frame drawn with the spot holding a
//! tile and one drawn with it holding none came back as the same bytes", which
//! is a shadow map written and never read.
//! `the_doorway_posts_stand_inside_the_spot_cone` is what holds it.
//!
//! # The torches flicker, and that is the demo's own steam
//!
//! [`flame`] is a pure function of the **simulated** seconds, so the same tick
//! is the same brightness on every machine — the reason `apps/breach::map::plate_x`
//! is one too. It is also the only thing in the zone that changes while nobody
//! is touching anything, which is what `web/tools/browser-e2e.mjs` reads to tell
//! a running page from a stalled one, and what its lighting check douses.
//!
//! # The volume is a one-bounce gather, and it says so
//!
//! [`probes`] casts rays out of each probe into
//! [`zone::world`], computes the light **arriving** at what
//! each ray hit — through the same [`punctual`] falloff the shader applies, with
//! a visibility ray per torch — and projects the surface's outgoing radiance
//! into the L1 basis [`GpuProbe`] holds. So a sealed alcove with no torch in it
//! is genuinely dark in the ambient term, and the great hall picks up the warm
//! bounce off its own floor.
//!
//! **It is one bounce and it is not a solve.** `apps/lantern/src/bounce.rs` is
//! the sample that bakes a room properly, over a 6 × 32² cube-face quadrature
//! and an analytic model of every face; this is a
//! Fibonacci sphere of `GATHER_DIRECTIONS` rays against the collision world,
//! which is what a zone this size can afford to bake at start-up.
//! The volume is baked **once**, into [`SceneDesc::probes`](crcbl::render::scene::SceneDesc);
//! there is no per-frame probe call, and `crates/crcbl-render/src/probe.rs` says
//! why the table is write-once.

use crcbl::math::{DVec3, Vec3};
use crcbl::phys::{PhysicsWorld, Ray};
use crcbl::render::scene::ProbeGrid;
use crcbl::render::{Light, PointLight, SpotLight};
use crcbl::shaders::probe::{GpuProbe, ProbeVolume};

use crate::zone::{self, BRAZIER_HEIGHT, COLS, Cell, ROWS, TILE_M, WALL_TOP_Y, tile_centre, tiles};

// ---------------------------------------------------------------------------
// The torches
// ---------------------------------------------------------------------------

/// How far above a brazier's rim its flame sits, in metres.
pub const FLAME_LIFT: f64 = 0.25;

/// How far a torch reaches, in metres.
///
/// Four tiles, so the pools of two braziers in the same room meet and the
/// corridor between two rooms is reached by neither — which is what makes
/// walking from one lit room to another something a visitor can see happening,
/// and what leaves the corridor for the shrine's spot to light.
pub const TORCH_REACH: f32 = 12.0;

/// A torch's colour, before its intensity. Firelight: strong red, half green,
/// a quarter blue.
pub const TORCH_COLOR: Vec3 = Vec3::new(1.0, 0.58, 0.24);

/// How bright a torch is at rest.
///
/// Well above 1.0, like every other light in this engine: the scene target is
/// `Rgba16Float` and the tonemap pass is what brings it back. Chosen against
/// the picture rather than against a photometric unit — [`punctual`] divides by
/// the square of the distance, so a torch three metres away delivers a tenth of
/// this and one six metres away a fortieth, and a zone whose far wall is black
/// is one whose shadows and reflections have nothing to show.
pub const TORCH_INTENSITY: f32 = 26.0;

/// How far [`flame`] swings either side of 1.0.
///
/// A quarter, which is a flame rather than a fault: enough that the shadows it
/// casts breathe and that consecutive `[HUD]` heartbeats — half a simulated
/// second apart — report values that differ far above the three decimal places
/// the line prints them to, and not so much that the zone reads as a strobe.
pub const FLICKER_DEPTH: f64 = 0.25;

/// The two periods [`flame`] sums, in seconds.
///
/// Two rather than one, and deliberately not a ratio of small whole numbers: one
/// sine is a metronome, and two whose periods do not divide each other is a
/// flame that never quite repeats over a visit.
const FLICKER_PERIODS: [f64; 2] = [2.3, 0.79];

/// How the two periods are weighted. Sums to one, so [`flame`] stays inside
/// `1 ± FLICKER_DEPTH`.
const FLICKER_WEIGHTS: [f64; 2] = [0.62, 0.38];

/// Every tile with a brazier on it, in [`tiles`] order.
///
/// Read off the layout rather than listed, so a brazier added to
/// [`zone::LAYOUT`] is a torch without a second edit.
#[must_use]
pub fn brazier_tiles() -> Vec<(usize, usize)> {
    tiles()
        .filter(|(_, _, cell)| *cell == Cell::Brazier)
        .map(|(col, row, _)| (col, row))
        .collect()
}

/// Where torch `index`'s flame is, in metres.
///
/// # Panics
///
/// If `index` is not a brazier on the layout, which is this crate's own indices
/// being wrong rather than a state a run can reach.
#[must_use]
pub fn flame_at(index: usize) -> DVec3 {
    let (col, row) = brazier_tiles()[index];
    tile_centre(col, row) + DVec3::Y * (BRAZIER_HEIGHT + FLAME_LIFT)
}

/// How bright torch `index` is, `seconds` into the run, as a multiple of
/// [`TORCH_INTENSITY`].
///
/// A pure function of the simulated time and of which torch it is, so the whole
/// row never brightens together — the phase is the torch's index, which is what
/// makes a room with two braziers in it look lit by two fires rather than by one
/// on a dimmer.
#[must_use]
pub fn flame(index: usize, seconds: f64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let phase = index as f64;
    let swing: f64 = FLICKER_PERIODS
        .iter()
        .zip(FLICKER_WEIGHTS)
        .enumerate()
        .map(|(harmonic, (period, weight))| {
            #[allow(clippy::cast_precision_loss)]
            let offset = phase * (1.0 + harmonic as f64 * 0.7);
            weight * (core::f64::consts::TAU * seconds / period + offset).sin()
        })
        .sum();
    FLICKER_DEPTH.mul_add(swing, 1.0)
}

// ---------------------------------------------------------------------------
// The shrine's spot
// ---------------------------------------------------------------------------

/// Which row of the layout the spot hangs over — the shrine's, one tile north
/// of the doorway.
const SPOT_ROW: usize = 3;

/// Which row its cone lands on: down the corridor, past the doorway.
const SPOT_POOL_ROW: usize = 6;

/// How high the spot hangs, in metres.
const SPOT_HEIGHT: f64 = 3.5;

/// How far it reaches, in metres — the length of the corridor it lights.
pub const SPOT_REACH: f32 = 14.0;

/// The half-angle of its bright core, in radians.
pub const SPOT_INNER_ANGLE: f32 = 12.0 * core::f32::consts::PI / 180.0;

/// The half-angle at which it closes, in radians.
///
/// Wide enough that the doorway's posts stand inside it at the distance the
/// doorway is — see the module docs, and
/// `the_doorway_posts_stand_inside_the_spot_cone`.
pub const SPOT_OUTER_ANGLE: f32 = 26.0 * core::f32::consts::PI / 180.0;

/// The spot's colour before its intensity: cold, against the torches' fire, so
/// the two lights in the zone are told apart by their colour and not only by
/// where they are.
pub const SPOT_COLOR: Vec3 = Vec3::new(0.70, 0.80, 1.0);

/// How bright it is, on the same scale as [`TORCH_INTENSITY`] and a little over
/// half of it — the shrine reads as the coldest and least of the zone's lights,
/// which is what keeps the torches the subject.
pub const SPOT_INTENSITY: f32 = 16.0;

/// Where the spot hangs, in metres.
#[must_use]
pub fn spot_at() -> DVec3 {
    tile_centre(COLS / 2, SPOT_ROW) + DVec3::Y * SPOT_HEIGHT
}

/// Where its cone lands, in metres.
#[must_use]
pub fn spot_pool() -> DVec3 {
    tile_centre(COLS / 2, SPOT_POOL_ROW)
}

/// The shrine's spot light.
#[must_use]
pub fn spot() -> Light {
    #[allow(clippy::cast_possible_truncation)]
    let vector = |p: DVec3| Vec3::new(p.x as f32, p.y as f32, p.z as f32);
    let at = spot_at();
    Light::Spot(SpotLight {
        position: vector(at),
        radius: SPOT_REACH,
        color: SPOT_COLOR * SPOT_INTENSITY,
        direction: (vector(spot_pool()) - vector(at)).normalize(),
        inner_angle: SPOT_INNER_ANGLE,
        outer_angle: SPOT_OUTER_ANGLE,
        fill: false,
    })
}

// ---------------------------------------------------------------------------
// The frame's light list
// ---------------------------------------------------------------------------

/// Every light in the zone `seconds` into the run, or **only the spot** when the
/// torches have been put out.
///
/// `lit` is what `L` toggles — see `crate::app`. It is presentation and not
/// simulation: the light list is the renderer's, the shrine's spot is a fixture
/// that does not go out, and a torch nobody has lit is a torch that is not in
/// this list at all. What that leaves lighting the zone is the irradiance volume
/// and [`zone::house_light`]'s ambient floor, which is
/// far darker and — because [`probes`] is baked once — completely still.
///
/// **That pair is what the browser gate's lighting check is made of**: a lit
/// zone whose picture changes with nothing held, and a doused one whose picture
/// does not.
#[must_use]
pub fn torches(seconds: f64, lit: bool) -> Vec<Light> {
    let mut lights = vec![spot()];
    if !lit {
        return lights;
    }
    for index in 0..brazier_tiles().len() {
        let at = flame_at(index);
        #[allow(clippy::cast_possible_truncation)]
        lights.push(Light::Point(PointLight {
            position: Vec3::new(at.x as f32, at.y as f32, at.z as f32),
            radius: TORCH_REACH,
            color: TORCH_COLOR * TORCH_INTENSITY * flame(index, seconds) as f32,
            fill: false,
        }));
    }
    lights
}

// ---------------------------------------------------------------------------
// The bake
// ---------------------------------------------------------------------------

/// How many probes the volume holds on each axis.
///
/// Denser across the floor than up it, because the zone is: the rooms differ
/// from each other along `X` and `Z` and the only thing that varies with height
/// is how far a surface is from a flame. Two in `y` puts one probe below waist
/// height and one near the wall tops, which is the least that lets the floor's
/// bounce fade upward rather than fill the room evenly.
pub const PROBE_COUNTS: [u32; 3] = [7, 2, 8];

/// How many probes that is — what
/// [`zone`]'s capacities reserve and what `ProbeGrid::check` holds
/// the table against.
pub const PROBE_TOTAL: u32 = PROBE_COUNTS[0] * PROBE_COUNTS[1] * PROBE_COUNTS[2];

/// How many directions each probe gathers over.
///
/// A Fibonacci sphere, so every direction stands for the **same** solid angle
/// and the quadrature weight is one number rather than a table. Thirty-two is
/// what a zone of this size can afford at start-up: the whole bake is
/// `PROBE_TOTAL × GATHER_DIRECTIONS` rays into the collision world plus a
/// visibility ray per torch behind each hit.
const GATHER_DIRECTIONS: usize = 32;

/// How far a gather ray looks, in metres. Past the diagonal of the zone, so a
/// ray that finds nothing has genuinely left the building.
const GATHER_M: f64 = 64.0;

/// How far off a surface the visibility ray starts, in metres — enough that it
/// does not immediately strike the surface it left.
const BOUNCE_EPSILON: f64 = 0.02;

/// How much light a stone surface sends back, per channel.
///
/// One number for the whole zone rather than the material table's six rows: the
/// bake is an approximation and reading the rows would make it look like a
/// solve. Stone at a little under a third is the usual figure and is what
/// `crcbl::greybox`'s own `GREYBOX_ALBEDO` sits near.
const BOUNCE_ALBEDO: f32 = 0.28;

/// How bright a punctual light is at `distance`, reaching **exactly zero** at
/// `radius`.
///
/// The Rust mirror of `punctual_falloff` in
/// `crates/crcbl-shaders/shaders/mesh.slang` — inverse square with Karis'
/// quartic window over it — so the light this bake gathers is the light the
/// shader will draw. A second falloff of this module's own invention would make
/// the ambient disagree with the direct term everywhere, which is the class of
/// error that reads as a shading bug and is nobody's fault in particular.
#[must_use]
pub fn punctual(distance: f32, radius: f32) -> f32 {
    let ratio = distance / radius.max(1e-6);
    let window = (1.0 - ratio * ratio * ratio * ratio).clamp(0.0, 1.0);
    window * window / distance.mul_add(distance, 1.0)
}

/// `GATHER_DIRECTIONS` unit vectors spread evenly over the sphere.
///
/// The Fibonacci (golden-angle) spiral, which is the standard construction for
/// an equal-area point set on a sphere: the `y` coordinates are spaced evenly
/// over `-1..1` — which is what makes the areas equal, by Archimedes' theorem —
/// and each is turned by the golden angle from the last.
/// `the_gather_directions_are_unit_and_spread` is what holds it to that rather
/// than to this paragraph.
fn gather_directions() -> Vec<DVec3> {
    // π(3 − √5), the golden angle in radians.
    let golden = core::f64::consts::PI * (3.0 - 5.0f64.sqrt());
    (0..GATHER_DIRECTIONS)
        .map(|index| {
            #[allow(clippy::cast_precision_loss)]
            let step = index as f64;
            #[allow(clippy::cast_precision_loss)]
            let y = 1.0 - 2.0 * (step + 0.5) / GATHER_DIRECTIONS as f64;
            let radius = (1.0 - y * y).max(0.0).sqrt();
            let theta = golden * step;
            DVec3::new(radius * theta.cos(), y, radius * theta.sin())
        })
        .collect()
}

/// The light arriving at `point`, whose surface faces `normal`, from every torch
/// that can see it.
///
/// The direct term and nothing else, in linear RGB: what the surface then sends
/// back is this times [`BOUNCE_ALBEDO`] over π, which is a Lambertian
/// reflector's outgoing radiance.
///
/// **The visibility ray is what makes the volume a map of the zone** rather than
/// of the torches. Without it every probe in the sealed alcoves would gather the
/// hall's braziers straight through the stone, and the ambient would say the
/// zone is one room.
fn arriving(world: &mut PhysicsWorld, point: DVec3, normal: DVec3, seconds: f64) -> Vec3 {
    let from = point + normal * BOUNCE_EPSILON;
    let mut sum = Vec3::ZERO;
    for index in 0..brazier_tiles().len() {
        let at = flame_at(index);
        let to = at - from;
        let distance = to.length();
        if distance <= f64::EPSILON || distance >= f64::from(TORCH_REACH) {
            continue;
        }
        let direction = to / distance;
        let facing = normal.dot(direction);
        if facing <= 0.0 {
            continue;
        }
        // Bounded just short of the flame, so the ray asks "is there stone in
        // the way" rather than "is there anything at all out there".
        let ray = Ray::new(from, direction).with_bounds(0.0, distance - BOUNCE_EPSILON);
        if world.cast_ray(&ray).is_some() {
            continue;
        }
        #[allow(clippy::cast_possible_truncation)]
        let reach = punctual(distance as f32, TORCH_REACH) * facing as f32;
        sum += TORCH_COLOR * TORCH_INTENSITY * (flame(index, seconds) as f32) * reach;
    }
    sum
}

/// One probe: the whole sphere gathered from `position`.
///
/// A probe standing inside stone gathers nothing and stays at
/// [`GpuProbe::ZERO`]. That is the honest row for it — a point inside a wall has
/// no irradiance — and it matters because the shader interpolates over the eight
/// probes around a fragment: a probe inside a wall that had gathered the far
/// side's light would bleed a lit room's ambient through into a dark one.
fn bake(world: &mut PhysicsWorld, position: DVec3, directions: &[DVec3], seconds: f64) -> GpuProbe {
    let mut probe = GpuProbe::ZERO;
    if !inside_open_air(position) {
        return probe;
    }
    #[allow(clippy::cast_precision_loss)]
    let solid_angle = 4.0 * core::f32::consts::PI / GATHER_DIRECTIONS as f32;
    for &direction in directions {
        let ray = Ray::new(position, direction).with_bounds(0.0, GATHER_M);
        // A direction that leaves the zone through no surface carries no light.
        // Unreachable inside a sealed room, and skipped rather than asserted
        // because a probe half inside a doorway's lintel is a legitimate way to
        // reach it.
        let Some((_, hit)) = world.cast_ray(&ray) else {
            continue;
        };
        let outgoing =
            arriving(world, hit.point, hit.normal, seconds) * BOUNCE_ALBEDO / core::f32::consts::PI;
        #[allow(clippy::cast_possible_truncation)]
        let unit = [direction.x as f32, direction.y as f32, direction.z as f32];
        probe.accumulate(unit, outgoing.to_array(), solid_angle);
    }
    probe
}

/// Whether `position` is in air a character could stand in rather than inside a
/// piece of the zone.
///
/// Read off [`zone::LAYOUT`] rather than out of the physics
/// world, because "is this point inside a collider" is not a question
/// [`PhysicsWorld`] answers and the layout knows it exactly.
fn inside_open_air(position: DVec3) -> bool {
    let index = |value: f64, count: usize, span: f64| -> Option<usize> {
        #[allow(clippy::cast_precision_loss)]
        let grid = (value / span + count as f64 * 0.5).floor();
        if grid < 0.0 || grid >= count as f64 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(grid as usize)
    };
    let (Some(col), Some(row)) = (
        index(position.x, COLS, TILE_M),
        index(position.z, ROWS, TILE_M),
    ) else {
        return false;
    };
    let cell = zone::cell(col, row);
    cell.is_open() && position.y > cell.floor_y() && position.y < WALL_TOP_Y
}

/// How far apart the probes stand on each axis: the zone's interior divided by
/// the counts.
///
/// **Not the distance between the outermost probes**, which would be the extent
/// over `count - 1` and would put a probe in each wall. This is the cell size of
/// a [`PROBE_COUNTS`] partition of the zone, and [`grid_origin`] places the first
/// probe half a cell in from the corner.
fn spacing() -> DVec3 {
    #[allow(clippy::cast_precision_loss)]
    let counts = DVec3::new(
        f64::from(PROBE_COUNTS[0]),
        f64::from(PROBE_COUNTS[1]),
        f64::from(PROBE_COUNTS[2]),
    );
    DVec3::new(COLS as f64 * TILE_M, WALL_TOP_Y, ROWS as f64 * TILE_M) / counts
}

/// Where probe `(0, 0, 0)` stands: the centre of the corner cell.
fn grid_origin() -> DVec3 {
    let step = spacing();
    DVec3::new(
        -0.5 * COLS as f64 * TILE_M,
        0.0,
        -0.5 * ROWS as f64 * TILE_M,
    ) + 0.5 * step
}

/// Where probe `(x, y, z)` stands, in world space.
fn probe_position(cell: [u32; 3]) -> DVec3 {
    let steps = DVec3::new(f64::from(cell[0]), f64::from(cell[1]), f64::from(cell[2]));
    grid_origin() + spacing() * steps
}

/// The zone's irradiance volume, gathered from the zone's own colliders.
///
/// Rows in the `x`-fastest order
/// [`ProbeGrid::probes`](crcbl::render::scene::ProbeGrid) declares, so the table
/// and the volume's counts address the same probe —
/// `every_probe_reads_back_at_its_own_position` is what checks that against
/// [`crcbl::shaders::probe::irradiance_at`] rather than against a second copy of
/// the index arithmetic.
///
/// Gathered at `t = 0`, which is the flames' own rest phase and the only instant
/// a volume baked once can be gathered at.
#[must_use]
pub fn probes() -> ProbeGrid {
    let directions = gather_directions();
    let mut world = zone::world();
    let mut rows = Vec::with_capacity(PROBE_TOTAL as usize);
    for z in 0..PROBE_COUNTS[2] {
        for y in 0..PROBE_COUNTS[1] {
            for x in 0..PROBE_COUNTS[0] {
                rows.push(bake(
                    &mut world,
                    probe_position([x, y, z]),
                    &directions,
                    0.0,
                ));
            }
        }
    }
    #[allow(clippy::cast_possible_truncation)]
    let f32s = |v: DVec3| [v.x as f32, v.y as f32, v.z as f32];
    ProbeGrid {
        volume: ProbeVolume {
            origin: f32s(grid_origin()),
            // **The reciprocal**, which is what the field is: the shader
            // multiplies by it where it would otherwise divide. Inverted here
            // and nowhere else, and a volume carrying the spacing itself would
            // light the zone from the wrong place.
            inv_spacing: f32s(DVec3::ONE / spacing()),
            counts: PROBE_COUNTS,
            // One level, on `apps/lantern`'s `bounce::probes` terms: the zone
            // is the extent this gather covers, and the updater that fills a
            // clipmap's coarser levels is what replaces this bake.
            levels: 1,
        },
        probes: rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::shaders::probe::irradiance_at;
    use crcbl::shaders::probe_visibility::ProbeVisibility;

    /// How many times brighter the ambient beside a brazier has to be than the
    /// ambient out in the corridor.
    ///
    /// **Measured, not guessed.** Sweeping every row of the volume when this was
    /// written put the brightest non-zero probe at 0.120 and the darkest at
    /// 0.0007, and the two points below at 0.066 and 0.0015 — a factor of
    /// forty-three. The threshold sits well under that, so re-tuning a torch does
    /// not red the test, and well over one, so a volume that gathered the same
    /// light everywhere could not pass it.
    const VOLUME_CONTRAST: f32 = 8.0;

    /// **Every brazier on the layout is a torch, and every torch stands over its
    /// own brazier.** The two lists are read off one table, and this is what
    /// says the reading is the right way round.
    #[test]
    fn every_brazier_carries_a_torch_over_it() {
        let braziers = brazier_tiles();
        assert!(braziers.len() >= 4, "only {} braziers", braziers.len());
        let lit = torches(0.0, true);
        // One spot and one point light per brazier.
        assert_eq!(lit.len(), braziers.len() + 1);
        assert!(matches!(lit[0], Light::Spot(_)), "the spot is row 0");

        for (index, (col, row)) in braziers.iter().enumerate() {
            let at = flame_at(index);
            let tile = tile_centre(*col, *row);
            assert!(
                (at.x - tile.x).abs() < 1e-9 && (at.z - tile.z).abs() < 1e-9,
                "torch {index} is at {at} and its brazier is at {tile}",
            );
            assert!(at.y > BRAZIER_HEIGHT, "the flame is inside the bowl");
            assert!(at.y < WALL_TOP_Y, "the flame is over the wall tops");
            let Light::Point(point) = lit[index + 1] else {
                panic!("torch {index} is not a point light");
            };
            assert!((f64::from(point.position.x) - at.x).abs() < 1e-4);
        }
    }

    /// **The torches go out and the spot does not.** The pair the browser gate's
    /// lighting check rests on: a doused zone still has a light in it, so the
    /// claim being made is "the picture went dark and still", not "the renderer
    /// was handed nothing".
    #[test]
    fn dousing_the_torches_leaves_the_shrines_spot_alone() {
        let doused = torches(1.0, false);
        assert_eq!(doused.len(), 1);
        assert!(matches!(doused[0], Light::Spot(_)));
        assert_eq!(doused, torches(9.0, false), "a doused zone is still");
    }

    /// **The flame moves, stays positive, and stays inside its own depth** —
    /// and two torches are not the same torch.
    ///
    /// Swept over a long run rather than checked at two instants: a sum of sines
    /// is exactly the shape whose extremes a two-point check misses.
    #[test]
    fn the_flame_swings_within_its_depth_and_never_goes_out() {
        let mut lowest = f64::INFINITY;
        let mut highest = f64::NEG_INFINITY;
        for step in 0..20_000 {
            let seconds = f64::from(step) * 0.01;
            let value = flame(0, seconds);
            lowest = lowest.min(value);
            highest = highest.max(value);
        }
        assert!(
            lowest > 1.0 - FLICKER_DEPTH - 1e-9 && highest < 1.0 + FLICKER_DEPTH + 1e-9,
            "the flame ran {lowest:.3}..{highest:.3}, outside 1 ± {FLICKER_DEPTH}",
        );
        assert!(lowest > 0.0, "the flame went out at {lowest}");
        // And it really swings, rather than sitting at one value.
        assert!(
            highest - lowest > FLICKER_DEPTH,
            "it only covered {:.3}",
            highest - lowest,
        );
        // Two torches in the same room are not one torch on a dimmer.
        assert!(
            (flame(0, 0.0) - flame(1, 0.0)).abs() > 1e-3,
            "every torch flickers in step",
        );
    }

    /// **Consecutive heartbeats read different flames**, which is the claim
    /// `web/tools/browser-e2e.mjs`'s `moving` check makes of this demo: half a
    /// simulated second apart, to the three decimal places the `[HUD]` line
    /// prints.
    #[test]
    fn a_heartbeat_apart_the_flame_reads_differently() {
        #[allow(clippy::cast_precision_loss)]
        let beat = crate::game::HEARTBEAT_TICKS as f64 / f64::from(crate::game::DEFAULT_TICK_HZ);
        let printed = |seconds: f64| format!("{:.3}", flame(0, seconds));
        let mut seen = std::collections::BTreeSet::new();
        for step in 0..40 {
            seen.insert(printed(f64::from(step) * beat));
        }
        assert!(
            seen.len() > 20,
            "over forty beats the flame printed {} values",
            seen.len(),
        );
        // And no two adjacent beats collide, which is what the gate reads.
        for step in 0..40 {
            let at = f64::from(step) * beat;
            assert_ne!(printed(at), printed(at + beat), "at {at} s");
        }
    }

    /// **The doorway's posts stand inside the spot's cone**, so the shadow tile
    /// the spot holds has something to write into it.
    ///
    /// Measured against the cone the light actually carries rather than against
    /// the constants: the half-angle is read off the [`SpotLight`] the frame is
    /// handed, and the post's offset off the zone's own geometry.
    #[test]
    fn the_doorway_posts_stand_inside_the_spot_cone() {
        let Light::Spot(spot) = spot() else {
            panic!("the shrine's light is not a spot");
        };
        let (col, row, _) = tiles()
            .find(|(_, _, cell)| *cell == Cell::DoorAlongZ)
            .expect("the layout carries a doorway walked through along Z");
        let door = tile_centre(col, row);
        // The inner edge of one post, at the height the cone crosses it.
        let post_x = 0.5 * zone::DOOR_OPENING_M;
        #[allow(clippy::cast_possible_truncation)]
        let at = Vec3::new(post_x as f32, 1.6, door.z as f32);

        let to = at - spot.position;
        let cosine = to.normalize().dot(spot.direction.normalize());
        assert!(
            cosine > spot.outer_angle.cos(),
            "the post at {at} is {:.1}° off the cone's axis, which closes at {:.1}°",
            to.normalize()
                .dot(spot.direction.normalize())
                .acos()
                .to_degrees(),
            spot.outer_angle.to_degrees(),
        );
        assert!(
            to.length() < spot.radius,
            "the post is {:.1} m away and the spot reaches {:.1} m",
            to.length(),
            spot.radius,
        );
        // The control: the corridor's own side wall is *outside* the cone, so
        // this test is about the post rather than about a cone that swallows the
        // room.
        #[allow(clippy::cast_possible_truncation)]
        let wall = Vec3::new((0.5 * TILE_M * 3.0) as f32, 1.6, door.z as f32);
        assert!(
            (wall - spot.position)
                .normalize()
                .dot(spot.direction.normalize())
                < spot.outer_angle.cos(),
            "the cone reaches the side wall too, so it swallows the corridor",
        );
    }

    /// **A spot the renderer would refuse to shadow is a spot with no shadow.**
    /// `crates/crcbl-render/src/shadow.rs` disqualifies a cone at or past
    /// `MAX_SPOT_HALF_ANGLE`, and nothing else in this sample would notice.
    #[test]
    fn the_spot_is_one_the_shadow_selection_will_take() {
        let Light::Spot(spot) = spot() else {
            panic!("the shrine's light is not a spot");
        };
        assert!(spot.radius > 0.0);
        assert!(spot.position.is_finite());
        assert!(spot.direction.normalize_or_zero().length_squared() > 0.0);
        assert!(
            spot.outer_angle.max(spot.inner_angle) < crcbl::render::shadow::MAX_SPOT_HALF_ANGLE,
            "a {:.1}° cone is past the {:.1}° the shadow atlas will take",
            spot.outer_angle.to_degrees(),
            crcbl::render::shadow::MAX_SPOT_HALF_ANGLE.to_degrees(),
        );
        assert!(spot.inner_angle <= spot.outer_angle);
    }

    /// **The gather directions are unit vectors spread over the whole sphere.**
    /// A quadrature that pointed all one way would bake a volume lit from one
    /// side, and every probe would still be a plausible-looking row.
    #[test]
    fn the_gather_directions_are_unit_and_spread() {
        let directions = gather_directions();
        assert_eq!(directions.len(), GATHER_DIRECTIONS);
        let mut sum = DVec3::ZERO;
        for direction in &directions {
            assert!(
                (direction.length() - 1.0).abs() < 1e-9,
                "{direction} is not a unit vector",
            );
            sum += *direction;
        }
        // An equal-area set over the sphere sums to nearly nothing.
        assert!(sum.length() < 0.2, "the set is biased toward {sum}");
        // And it reaches every octant, which a bias no sum could see would not.
        for signs in 0..8u8 {
            let wanted = |bit: u8, value: f64| {
                if signs & (1 << bit) == 0 {
                    value > 0.0
                } else {
                    value < 0.0
                }
            };
            assert!(
                directions
                    .iter()
                    .any(|d| wanted(0, d.x) && wanted(1, d.y) && wanted(2, d.z)),
                "no direction in octant {signs}",
            );
        }
    }

    /// **The falloff is the shader's**, checked at the two ends the mirror has to
    /// get right: zero exactly at the radius, and finite at zero distance.
    #[test]
    fn the_falloff_reaches_zero_at_the_radius() {
        assert_eq!(punctual(TORCH_REACH, TORCH_REACH), 0.0);
        assert_eq!(punctual(TORCH_REACH * 2.0, TORCH_REACH), 0.0);
        assert!(punctual(0.0, TORCH_REACH).is_finite());
        // Monotone on the way out, which is what makes "further is darker" true.
        let mut last = f32::INFINITY;
        for step in 0..=100 {
            #[allow(clippy::cast_precision_loss)]
            let distance = TORCH_REACH * step as f32 / 100.0;
            let value = punctual(distance, TORCH_REACH);
            assert!(value <= last + 1e-9, "it rose at {distance} m");
            last = value;
        }
    }

    /// **The volume's header addresses the table it is shipped with**, checked
    /// through [`irradiance_at`] — the engine's own reader — rather than through
    /// a second copy of the index arithmetic.
    #[test]
    fn every_probe_reads_back_at_its_own_position() {
        let grid = probes();
        assert_eq!(grid.volume.counts, PROBE_COUNTS);
        assert_eq!(grid.volume.total(), PROBE_TOTAL);
        assert_eq!(grid.probes.len() as u32, PROBE_TOTAL);

        // One row read at its own probe's position is that row, because the
        // trilinear weights collapse onto it.
        for z in 0..PROBE_COUNTS[2] {
            for y in 0..PROBE_COUNTS[1] {
                for x in 0..PROBE_COUNTS[0] {
                    let index = ((z * PROBE_COUNTS[1] + y) * PROBE_COUNTS[0] + x) as usize;
                    let at = probe_position([x, y, z]);
                    #[allow(clippy::cast_possible_truncation)]
                    let point = [at.x as f32, at.y as f32, at.z as f32];
                    let normal = [0.0, 1.0, 0.0];
                    let read = irradiance_at(
                        &grid.volume,
                        &grid.probes,
                        &ProbeVisibility::NONE,
                        point,
                        normal,
                    );
                    let want = grid.probes[index].irradiance(normal);
                    for channel in 0..3 {
                        assert!(
                            (read[channel] - want[channel]).abs() < 1e-5,
                            "probe {x},{y},{z} reads back {read:?} rather than {want:?}",
                        );
                    }
                }
            }
        }
    }

    /// **A torch behind a pillar does not reach the surface behind it**, which
    /// is what the visibility ray in [`arriving`] buys and the only thing that
    /// makes the baked volume a map of the zone rather than of the torches.
    ///
    /// The control is a second point at the **same distance** from the same
    /// flame with nothing in the way: without it, "the shaded point is dark"
    /// passes for a falloff that had simply run out, and this test would say
    /// nothing about the ray at all.
    #[test]
    fn a_torch_behind_a_pillar_does_not_reach_the_probe() {
        /// What the shaded point may still collect, as a fraction of what the
        /// clear one does. Measured at 0.0003 against 1.18, which is three
        /// parts in ten thousand; this is thirty times that.
        const PILLAR_LEAK: f32 = 0.01;

        let mut world = zone::world();
        // The hall's left-hand brazier, with a pillar one tile north of it.
        let brazier = brazier_tiles()
            .iter()
            .position(|(col, row)| *col == 2 && *row == 11)
            .expect("the hall carries a brazier at column 2, row 11");
        assert_eq!(zone::cell(2, 10), Cell::Pillar, "the pillar moved");
        let flame = flame_at(brazier);

        // Two metres above the floor on either side, both two tiles from the
        // flame: one with the pillar on the line and one without.
        let shaded = tile_centre(2, 9) + DVec3::Y * 1.0;
        let open = tile_centre(4, 11) + DVec3::Y * 1.0;
        let mut reach = |at: DVec3| {
            let normal = (flame - at).normalize();
            arriving(&mut world, at, normal, 0.0)
        };
        assert!(
            ((flame - shaded).length() - (flame - open).length()).abs() < 0.05,
            "the two points are {:.2} m and {:.2} m from the flame, so this is a \
             test of the falloff",
            (flame - shaded).length(),
            (flame - open).length(),
        );

        let lit = reach(open).length();
        let dark = reach(shaded).length();
        assert!(lit > 0.0, "the clear point got no light at all");
        // Not zero, and it is worth saying why: [`arriving`] sums *every* light
        // in the zone, so the shaded point still collects a trace from the
        // braziers that flank the spawn two tiles further on. What it does not
        // collect is the flame the pillar stands between it and — and the
        // equal-distance control above is what makes that a statement about the
        // ray rather than about the falloff.
        assert!(
            dark < lit * PILLAR_LEAK,
            "the pillar let {dark} through, against {lit} on the clear side",
        );
    }

    /// **The volume varies across the zone**: the ambient beside a brazier is far
    /// above the ambient in the corridor, which every torch is out of reach of.
    ///
    /// Read through [`irradiance_at`] — the engine's own reader, and the same
    /// arithmetic `mesh.slang` runs — rather than off the rows, so what is
    /// asserted is what a fragment would be handed.
    #[test]
    fn the_volume_is_brighter_beside_a_brazier_than_out_in_the_corridor() {
        let grid = probes();
        let luminance = |at: DVec3| {
            #[allow(clippy::cast_possible_truncation)]
            let point = [at.x as f32, at.y as f32, at.z as f32];
            let value = irradiance_at(
                &grid.volume,
                &grid.probes,
                &ProbeVisibility::NONE,
                point,
                [0.0, 1.0, 0.0],
            );
            0.2126f32.mul_add(value[0], 0.7152f32.mul_add(value[1], 0.0722 * value[2]))
        };

        assert_eq!(zone::cell(2, 11), Cell::Brazier, "the hall's brazier moved");
        assert_eq!(zone::cell(6, 6), Cell::Floor, "the corridor moved");
        let beside = luminance(tile_centre(2, 11) + DVec3::Y * 1.0);
        let corridor = luminance(tile_centre(6, 6) + DVec3::Y * 1.0);
        assert!(beside > 0.0, "the brazier's probes gathered nothing at all");
        assert!(
            beside > corridor * VOLUME_CONTRAST,
            "beside a brazier reads {beside:.5} and the corridor {corridor:.5}, which is \
             a volume that does not know where the light is",
        );
    }

    /// **A probe inside stone stays at zero.** The row a wall's probe carries is
    /// what would bleed a lit room's ambient into a dark one, and nothing else
    /// here would see it.
    #[test]
    fn a_probe_inside_the_stone_gathers_nothing() {
        // The corner of the grid, which is always a wall block — see
        // `zone::the_border_of_the_zone_is_solid`.
        let corner = tile_centre(0, 0) + DVec3::Y * 2.0;
        assert!(!inside_open_air(corner));
        let mut world = zone::world();
        let baked = bake(&mut world, corner, &gather_directions(), 0.0);
        assert_eq!(baked, GpuProbe::ZERO);

        // The control: a point in the middle of the hall is open air, and a
        // probe there gathers something.
        let open = tile_centre(6, 13) + DVec3::Y * 1.5;
        assert!(inside_open_air(open));
        let gathered = bake(&mut world, open, &gather_directions(), 0.0);
        assert_ne!(gathered, GpuProbe::ZERO);

        // …and so is a point outside the grid altogether, which the index
        // arithmetic has to refuse rather than wrap.
        assert!(!inside_open_air(DVec3::new(1e6, 1.0, 0.0)));
        assert!(!inside_open_air(DVec3::new(0.0, -1.0, 0.0)));
        assert!(!inside_open_air(DVec3::new(0.0, WALL_TOP_Y + 1.0, 0.0)));
    }
}
