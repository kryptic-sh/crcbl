//! The zone's light: the braziers' torches, the shrine's spot, and the
//! irradiance volume the renderer refills from the sun.
//!
//! ```text
//!   zone::LAYOUT ──▶ torches(seconds, lit) ──▶ ForwardRenderer::set_lights   (every frame)
//!         │
//!         └──────────▶ probes() ──▶ SceneDesc::probes                        (placed once)
//!                          │
//!                          └──▶ ProbeUpdate::EveryFrame ──▶ the rsm updater  (every frame)
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
//! [`crate::zone::LAYOUT`]'s row 4, aimed down the corridor — so the doorway's
//! own posts are **inside the cone** and their shadows run down the corridor
//! floor. `apps/lantern/src/room.rs` records why that is not
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
//! # The volume is placed here and filled by the renderer
//!
//! [`probes`] decides where the probes stand and hands the rows over at zero
//! with `ProbeUpdate::EveryFrame` on them; the renderer's reflective shadow map
//! is what puts light in them, one bounce of the **sun** off whatever the zone
//! has standing in it, recomputed every frame.
//!
//! **So this zone's indirect light is thin, and that is honest rather than
//! broken.** The zone is an interior lit by torches, and the updater gathers no
//! punctual light at all — `docs/backlog.md` carries what a producer that did
//! would take. What is left holding the ambient term up is
//! [`zone::house_light`](crate::zone::house_light)'s floor. The rows this module
//! hands over are zero, so a frame drawn before the updater has run is that
//! floor and nothing else.

use crcbl::math::{DVec3, Vec3};
use crcbl::render::scene::ProbeGrid;
use crcbl::render::{Light, PointLight, ProbeUpdate, SpotLight};
use crcbl::shaders::probe::{GpuProbe, ProbeVolume};

use crate::zone::{BRAZIER_HEIGHT, COLS, Cell, ROWS, TILE_M, WALL_TOP_Y, tile_centre, tiles};

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
/// the picture rather than against a photometric unit — `mesh.slang` divides a
/// point light by the square of the distance, so a torch three metres away
/// delivers a tenth of this and one six metres away a fortieth, and a zone whose
/// far wall is black is one whose shadows and reflections have nothing to show.
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
/// [`crate::zone::LAYOUT`] is a torch without a second edit.
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
/// and [`crate::zone::house_light`]'s ambient floor, both far darker than the
/// braziers and both still: the spot does not flicker, so neither does its
/// bounce.
///
/// **That pair is what the browser gate's lighting check is made of**: a lit
/// zone whose picture changes with nothing held, and a doused one whose picture
/// does not.
///
/// **The doused zone's bounce is measurably nothing**, which is a fact about
/// this scene rather than about the updater. Measured 2026-09-04 by running the
/// browser gate twice, once with `crcbl_render::rsm`'s `r_probe_bounce` off:
/// the doused window read a mean luma of 6.68 with the bounce and 6.67 without,
/// where the lit window read 16.01 against 15.73. So the braziers light the
/// volume and [`spot`] does not — it is the coldest and least of the zone's
/// lights and it stands in one corner of it. `docs/backlog.md` carries what
/// that costs the gate.
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
// The volume
// ---------------------------------------------------------------------------

/// How many probes the volume holds on each axis.
///
/// Denser across the floor than up it, because the zone is: the rooms differ
/// from each other along `X` and `Z` and the only thing that varies with height
/// is how far a surface is from a flame. Two in `y` puts one probe below waist
/// height and one near the wall tops, which is the least that lets the floor's
/// bounce fade upward rather than fill the room evenly.
pub const PROBE_COUNTS: [u32; 3] = [7, 2, 8];

/// How many probes that is — what [`crate::zone`]'s capacities reserve and what
/// `ProbeGrid::check` holds the table against.
pub const PROBE_TOTAL: u32 = PROBE_COUNTS[0] * PROBE_COUNTS[1] * PROBE_COUNTS[2];

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
///
/// The host-side twin of what the engine derives from the volume header —
/// `crcbl::shaders::probe::ProbeVolume::position` — and
/// `every_probe_stands_where_the_header_says` is what holds the two to one
/// another. Kept as its own function because reading the claim through the
/// header would be reading it through the thing it is checking.
#[cfg(test)]
fn probe_position(cell: [u32; 3]) -> DVec3 {
    let steps = DVec3::new(f64::from(cell[0]), f64::from(cell[1]), f64::from(cell[2]));
    grid_origin() + spacing() * steps
}

/// The zone's irradiance volume: where the probes stand, and rows for the
/// engine's updater to fill.
///
/// Rows in the `x`-fastest order
/// [`ProbeGrid::probes`](crcbl::render::scene::ProbeGrid) declares, so the table
/// and the volume's counts address the same probe.
///
/// # The rows ship zeroed, and what that costs this zone
///
/// Until 2026-09-04 this gathered the torches' first bounce by casting rays into
/// [`crate::zone::world`] at `t = 0` — a bake in the sense
/// `docs/plan/50-irradiance-probes.md`'s no-bake rule forbids, since the flames
/// flicker and the result outlived them. The rows are the engine's updater's
/// now, through [`ProbeUpdate::EveryFrame`].
///
/// **That updater is a reflective shadow map of the *sun's* near cascade**, and
/// this zone's sun is [`zone::house_light`](crate::zone::house_light) — a token
/// whose colour is `0.01`,
/// because a torch-lit interior has no sun. So the volume the updater fills here
/// is very nearly black, and the warm bounce off the great hall's floor that the
/// old gather produced is gone until a producer that gathers punctual lights
/// exists. That is the honest state and it is written here rather than papered
/// over with a bake: the ambient floor in
/// [`zone::house_light`](crate::zone::house_light) is what lights
/// the zone's shadowed surfaces meanwhile.
#[must_use]
pub fn probes() -> ProbeGrid {
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
            // One level, on `apps/lantern`'s `bounce::probes` terms: the zone is
            // the extent the updater's near cascade covers, and a clipmap's
            // coarser levels are for a world larger than this one.
            levels: 1,
        },
        // **Zeroes.** The rows are the volume's size here, not its contents —
        // see above.
        probes: vec![GpuProbe::ZERO; PROBE_TOTAL as usize],
        update: ProbeUpdate::EveryFrame,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zone;

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

    /// **The volume's header puts every probe where [`probe_position`] says**,
    /// which is what the updater places its samples against.
    ///
    /// This used to read a row back through [`irradiance_at`] and assert it was
    /// that probe's own row; it cannot, now that the rows ship zeroed and the
    /// updater fills them. What that test was really about survives here: an
    /// origin at the zone's corner rather than at the first cell's centre, an
    /// `inv_spacing` carrying the spacing instead of its reciprocal, or a
    /// `z`-fastest walk each leave a zone lit from the wrong place, and each is
    /// a claim about the header.
    ///
    /// **A tolerance rather than an equality**, and it is arithmetic rather than
    /// slack: this module places its probes in `f64` and the header carries an
    /// `f32` *reciprocal* spacing, so the two multiply different numbers and
    /// differ in the last bit of a metre-scale coordinate. A micrometre is four
    /// orders under the spacing, so every mistake this test is about — a corner
    /// origin, an uninverted spacing, a transposed walk — is metres out and
    /// still fails.
    #[test]
    fn every_probe_stands_where_the_header_says() {
        /// How far apart the two placements may be, in metres.
        const TOLERANCE_M: f32 = 1.0e-6;

        let grid = probes();
        assert_eq!(grid.volume.counts, PROBE_COUNTS);
        assert_eq!(grid.volume.total(), PROBE_TOTAL);
        assert_eq!(grid.probes.len() as u32, PROBE_TOTAL);
        for z in 0..PROBE_COUNTS[2] {
            for y in 0..PROBE_COUNTS[1] {
                for x in 0..PROBE_COUNTS[0] {
                    let at = probe_position([x, y, z]);
                    #[allow(clippy::cast_possible_truncation)]
                    let want = [at.x as f32, at.y as f32, at.z as f32];
                    let got = grid.volume.position(0, [x, y, z]);
                    for axis in 0..3 {
                        assert!(
                            (got[axis] - want[axis]).abs() <= TOLERANCE_M,
                            "the header puts probe ({x}, {y}, {z}) at {got:?} and this \
                             module puts it at {want:?}"
                        );
                    }
                }
            }
        }
    }

    /// **The rows ship zeroed and the volume asks to be updated.** A grid of
    /// zeroes left at `ProbeUpdate::Authored` is a zone with no bounce at all
    /// and a perfectly plausible picture — see [`probes`] for what the updater
    /// can and cannot give this zone.
    #[test]
    fn the_rows_are_the_updater_s_to_fill() {
        let grid = probes();
        assert_eq!(grid.update, ProbeUpdate::EveryFrame);
        assert!(grid.probes.iter().all(|probe| *probe == GpuProbe::ZERO));
    }
}
