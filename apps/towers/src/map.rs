//! The map: the field the creeps cross, the lane they walk, the pads a tower
//! can be built on, and the volume that takes a life off the team.
//!
//! ```text
//!            +X
//!             │   ┌─────────────────────────────────────────────┐  z = -12
//!             │   │                                             │
//!             │   │   ▣ exit  ◀───────────────────── leg 2      │  z =  -6
//!             │   │       ▫ gate      ▫ middle          ▲       │
//!             │   │                                     │ leg 1 │
//!             │   │       ▫ entry     ▫ bend      ▫ east│       │  z =   1..3
//!             │   │   ▲ spawn ──────────────────────────▶       │  z =   8
//!             │   └─────────────────────────────────────────────┘  z = +12
//!            −X       x = -14                            x = +13
//! ```
//!
//! The picture above is the committed `assets/scenes/field.scn/`; a `--scene`
//! directory draws whatever path and plots it holds instead.
//!
//! # One set of numbers, three consumers
//!
//! Every constant here, and every waypoint and plot a [`Map`] holds, is read by
//! the meshes in [`Map::scene`], by the instances in [`Map::place`], and by the
//! colliders in [`Map::world`]. There is no second set of numbers for the
//! physics, which is what makes a lane that looks walkable walkable and an exit
//! volume that looks like a gate the gate. `apps/breach` and `apps/shard` build
//! their rooms the same way, and for the same reason.
//!
//! # The layout is scene data, and the rules are this file's
//!
//! Where the path runs and where the plots stand is a `.scn/` directory, read by
//! [`crate::scene`] — `docs/plan/sample/07-towers.md`'s milestone 2 is a map
//! authored in the editor, and a map that is data is one the editor can open.
//! What stays here is everything a layout is **measured against**: how big the
//! field is, how wide the lane is drawn, what a tower needs around it, and the
//! two caps the reserved pools and a frame's snapshot are sized for.
//! [`Map::new`] holds every layout to them and refuses one that breaks any, by
//! name — see [`MapError`] — because each is an assumption something below
//! makes without checking: one straight `platform` per leg, a pad on the
//! ground, a tower beside the lane rather than on it.
//!
//! # The creeps are not in [`Map::world`]
//!
//! Each adds its own sphere when it spawns and writes it back every tick — see
//! [`crate::creep`]. What [`Map::world`] puts in the world is what does not
//! move: the ground and the exit trigger.
//!
//! # The exit is a trigger, and that is the whole of how a life is lost
//!
//! [`EXIT_HALF`] is registered with
//! [`PhysicsWorld::set_trigger`](crcbl::phys::PhysicsWorld::set_trigger), which
//! makes it **non-solid**: every sweep and every ray passes straight through it
//! and only [`overlap_sphere`](crcbl::phys::PhysicsWorld::overlap_sphere)
//! reports it. So a creep standing in it is found by the overlap query
//! [`crate::creep::has_reached_the_exit`] runs, and a bolt fired down the last
//! leg flies through it rather than exploding on it. Both halves are asserted —
//! see this module's tests.
//!
//! # One material per kind, and two states over the top of them
//!
//! Each [`crate::creep::Kind`] and each [`crate::tower::Kind`] has a row of its
//! own — [`creep_material`] and [`tower_material`] are the mapping — so a
//! reviewer reads the field rather than counting it. Over the creep rows sit two
//! **state** rows shared by every kind: [`CREEP_HURT_MATERIAL`] for one down to
//! half its health and [`CREEP_SLOWED_MATERIAL`] for one a
//! [`crate::tower::Kind::Slow`] tower is holding. Hurt wins where both apply,
//! because a creep about to die is the more urgent reading of the two and a
//! player watching the field is watching for kills.
//!
//! An **upgraded** tower is not another row: it is the same material standing
//! [`UPGRADED_SCALE`] times as tall, which is a transform rather than a palette
//! entry and keeps one row per kind however many tiers a kind grows.
//! `an_upgraded_tower_still_fits_its_pad_and_clears_the_lane` is what says the
//! bigger footprint is still a footprint that fits.
//!
//! # There is a sun in here, because this map has no roof
//!
//! [`sun`] is a real [`DirectionalLight`] rather than the token
//! `apps/breach::map::house_light` hands its ceilinged room, and [`Map::place`]
//! sets no point lights at all. A field under the sky is the one thing on the ladder
//! that wants exactly what `begin_frame` already takes.

use std::borrow::Cow;

use crcbl::greybox::{GREYBOX_TILE_M, cylinder, grid_material, grid_page, platform, sphere};
use crcbl::math::{DVec3, Mat4, Quat, Vec3};
use crcbl::phys::{BoxCollider, ColliderId, PhysicsWorld};
use crcbl::render::scene::{Capacities, Geometry, InstanceDesc, MeshDesc, ProbeGrid, SceneDesc};
use crcbl::render::{DirectionalLight, ForwardRenderer, InstanceHandle, InstancePoolError};
use crcbl::scene::scn::ScnError;
use crcbl::shaders::mesh::GpuMaterial;

use crate::path::Path;
use crate::scene::Plot;
use crate::tower::SHORTEST_RANGE_M;
use crate::wave::MAX_CREEPS;

mod wire;

pub use wire::MapWireError;

// ---------------------------------------------------------------------------
// The field
// ---------------------------------------------------------------------------

/// How far the field reaches either side of its centre line, in metres.
pub const HALF_WIDTH: f64 = 18.0;

/// …and how far up and down it, in metres along `Z`.
pub const HALF_DEPTH: f64 = 12.0;

/// How thick the ground slab is, in metres. Its **top** is `y = 0`, which is
/// what every other height here is measured from.
pub const SLAB_THICKNESS: f64 = 0.6;

// ---------------------------------------------------------------------------
// The map
// ---------------------------------------------------------------------------

/// One field's layout: the path the creeps walk and the plots a tower can be
/// built on, checked against the rules everything below depends on.
///
/// Read out of a `.scn/` directory by [`crate::scene`] — the committed
/// `assets/scenes/field.scn/` unless `--scene` names another — and built by
/// [`Map::new`] and nothing else, so a map that reached the game is one whose
/// legs the lane meshes can draw and whose plots a tower can stand on.
///
/// Not `Eq`: every number in it is a float. [`PartialEq`] is what
/// [`crate::Options`] needs and all a test comparing two parses of one
/// directory wants.
#[derive(Clone, Debug, PartialEq)]
pub struct Map {
    path: Path,
    plots: Vec<Plot>,
}

impl Map {
    /// The map `waypoints` and `plots` describe: the waypoints spawn first, the
    /// plots in the order the overlay lists them and `PlaceTower` numbers them.
    ///
    /// # Errors
    ///
    /// [`MapError`], naming the waypoint, the leg or the plot it is about —
    /// [`Path::new`] says what the path is refused for. A plot is refused when
    /// there are none or more than [`MAX_PLOTS`], when [`footing`] refuses it
    /// (not a finite number, or off the ground) or it is off the field, when it stands within [`PLOT_CLEARANCE`] of the lane, and
    /// when the lane is out of [`SHORTEST_RANGE_M`] of it: the build list offers
    /// every kind on every plot, so a plot the shortest-reaching kind cannot
    /// cover from is one that kind may not be built on — and when
    /// [`check_label`] refuses its label.
    ///
    /// [`footing`] and [`check_label`] are also the plots' and waypoints' own
    /// row rules (their `Validate`), so a map built here without a scene —
    /// from the wire, or by a test — is held to the same per-row rules a load
    /// and an editor's edit are.
    pub fn new(waypoints: Vec<DVec3>, plots: Vec<Plot>) -> Result<Self, MapError> {
        let path = Path::new(waypoints)?;
        if plots.is_empty() {
            return Err(MapError::NoPlots);
        }
        if plots.len() > MAX_PLOTS {
            return Err(MapError::TooManyPlots { found: plots.len() });
        }
        for (index, plot) in plots.iter().enumerate() {
            check_label(&plot.label).map_err(|length| MapError::LabelTooLong {
                plot: index,
                length,
            })?;
            let feet = plot.at();
            let what = || format!("plot {:?}", plot.label);
            footing(feet).map_err(|fault| fault.refusal(what()))?;
            if feet.x.abs() + 0.5 * PAD_EDGE > HALF_WIDTH
                || feet.z.abs() + 0.5 * PAD_EDGE > HALF_DEPTH
            {
                return Err(MapError::OffTheField { what: what() });
            }
            let distance = path.distance_to(feet);
            if distance <= PLOT_CLEARANCE {
                return Err(MapError::OnTheLane {
                    plot: plot.label.clone(),
                    distance,
                });
            }
            if distance >= SHORTEST_RANGE_M {
                return Err(MapError::OutOfReach {
                    plot: plot.label.clone(),
                    distance,
                });
            }
        }
        Ok(Self { path, plots })
    }

    /// The path the creeps walk.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every plot, in the order the overlay lists them and `PlaceTower`
    /// numbers them.
    #[must_use]
    pub fn plots(&self) -> &[Plot] {
        &self.plots
    }
}

/// Why a directory, or a list of waypoints and plots, is not a towers map.
///
/// [`ScnError`] is the format's half and says which *key* it is about;
/// [`MapError::Missing`] says which *chunk* the manifest left out; every other
/// variant is a rule of the field this module's arithmetic stands on, and names
/// the waypoint, the leg or the plot that broke it. A map is refused whole
/// rather than drawn with the offending row dropped, because a field missing a
/// plot its file names is a field nobody can tell from the one they authored.
#[derive(Debug)]
pub enum MapError {
    /// The directory is not a scene, or a chunk in it would not read.
    Scene(ScnError),
    /// The manifest does not name one of the systems a towers map is made of.
    Missing(&'static str),
    /// Fewer than two waypoints, which is a path with no leg.
    TooFewWaypoints {
        /// How many there are.
        found: usize,
    },
    /// More than [`MAX_WAYPOINTS`].
    TooManyWaypoints {
        /// How many there are.
        found: usize,
    },
    /// Two waypoints claim the same place in the walk.
    RepeatedOrder {
        /// The order both carry.
        order: u32,
    },
    /// A waypoint or a plot is not on the ground's top, `y = 0`.
    OffTheGround {
        /// Which one: `waypoint 2`, `plot "gate"`.
        what: String,
        /// Where it stands instead.
        y: f64,
    },
    /// A waypoint's or a plot's coordinate is not a finite number.
    NotFinite {
        /// Which one, spelled as [`MapError::OffTheGround`] spells it.
        what: String,
        /// Which coordinate, `0` for `X`.
        axis: usize,
        /// What it is instead.
        value: f64,
    },
    /// A waypoint's lane, or a plot's pad, reaches past the field's edge.
    OffTheField {
        /// Which one, spelled as [`MapError::OffTheGround`] spells it.
        what: String,
    },
    /// A leg no longer than the lane is wide, which draws as a square and
    /// reads as no leg at all.
    ShortLeg {
        /// Which leg, counted from the spawn.
        leg: usize,
        /// How long it is, in metres.
        length: f64,
    },
    /// A leg that runs along neither `X` nor `Z`, which one `platform` cannot
    /// draw.
    Diagonal {
        /// Which leg, counted from the spawn.
        leg: usize,
    },
    /// No build plots at all, which is a field a player cannot play on.
    NoPlots,
    /// More than [`MAX_PLOTS`].
    TooManyPlots {
        /// How many there are.
        found: usize,
    },
    /// A plot within [`PLOT_CLEARANCE`] of the lane's centre line.
    OnTheLane {
        /// Its label.
        plot: String,
        /// How far it is from the centre line, in metres.
        distance: f64,
    },
    /// A plot the shortest-reaching kind cannot cover the lane from.
    OutOfReach {
        /// Its label.
        plot: String,
        /// How far it is from the centre line, in metres.
        distance: f64,
    },
    /// A plot's label is longer than [`MAX_LABEL_BYTES`]. Named by its place
    /// in the list, since the label is the thing too long to print.
    LabelTooLong {
        /// Which plot, counted from the first.
        plot: usize,
        /// How long its label is, in bytes.
        length: usize,
    },
}

/// What a waypoint or a plot breaks by where it stands alone, needing no
/// other row: the half of the field's rules a tool runs on every edit, as
/// `crate::scene`'s `Validate` impls, and [`Path::new`] and [`Map::new`] run
/// on every map — one rule, [`footing`], at both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Footing {
    /// A coordinate that is not a finite number: every comparison the field's
    /// other rules make is false for a `NaN`, so it is refused before them.
    NotFinite {
        /// Which coordinate, `0` for `X`.
        axis: usize,
        /// What it is instead.
        value: f64,
    },
    /// Not on the ground's top, `y = 0`.
    OffTheGround {
        /// Where it stands instead.
        y: f64,
    },
}

impl Footing {
    /// The field of the row it is about, as a property write names it.
    #[must_use]
    pub fn field(&self) -> String {
        match self {
            Self::NotFinite { axis, .. } => format!("position.{axis}"),
            Self::OffTheGround { .. } => "position.1".to_owned(),
        }
    }

    /// This fault as [`Map::new`] refuses it, `what` naming the row: `waypoint
    /// 2`, `plot "gate"`.
    #[must_use]
    pub fn refusal(self, what: String) -> MapError {
        match self {
            Self::NotFinite { axis, value } => MapError::NotFinite { what, axis, value },
            Self::OffTheGround { y } => MapError::OffTheGround { what, y },
        }
    }
}

/// The rule over where one waypoint or plot stands, alone: every coordinate
/// finite, and on the ground.
///
/// # Errors
///
/// The first [`Footing`] fault, coordinates checked before the ground.
pub fn footing(position: DVec3) -> Result<(), Footing> {
    for (axis, value) in position.to_array().into_iter().enumerate() {
        if !value.is_finite() {
            return Err(Footing::NotFinite { axis, value });
        }
    }
    // The ground's top is `y = 0` and everything is drawn there: a pad above
    // or below it is a tower whose feet are not where its pad is, and a
    // waypoint off it a creep walking on air beside a lane that says
    // otherwise.
    if position.y != 0.0 {
        return Err(Footing::OffTheGround { y: position.y });
    }
    Ok(())
}

/// The rule over one plot's label, alone: no longer than
/// [`MAX_LABEL_BYTES`].
///
/// # Errors
///
/// The label's length in bytes, when it is past the cap.
pub fn check_label(label: &str) -> Result<(), usize> {
    if label.len() > MAX_LABEL_BYTES {
        Err(label.len())
    } else {
        Ok(())
    }
}

impl std::fmt::Display for MapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scene(error) => write!(f, "{error}"),
            Self::Missing(system) => write!(
                f,
                "the manifest names no `{system}` chunk, which every towers map has"
            ),
            Self::TooFewWaypoints { found } => write!(
                f,
                "the path has {found} waypoint(s), and it takes two to make a leg"
            ),
            Self::TooManyWaypoints { found } => write!(
                f,
                "the path has {found} waypoints, past the {MAX_WAYPOINTS} this sample reserves \
                 room to draw"
            ),
            Self::RepeatedOrder { order } => write!(
                f,
                "two waypoints share order {order}, so the path has no one order to walk them in"
            ),
            Self::OffTheGround { what, y } => write!(
                f,
                "{what}'s `position.1` is {y}, and everything on the field stands on the ground \
                 at y = 0"
            ),
            Self::NotFinite { what, axis, value } => write!(
                f,
                "{what}'s `position.{axis}` is {value}, not a finite number"
            ),
            Self::OffTheField { what } => write!(f, "{what} reaches past the edge of the field"),
            Self::ShortLeg { leg, length } => write!(
                f,
                "leg {leg} is {length:.2} m long, no longer than the lane is wide"
            ),
            Self::Diagonal { leg } => write!(
                f,
                "leg {leg} runs along neither X nor Z, and the lane is one straight platform per \
                 leg"
            ),
            Self::NoPlots => write!(f, "the map has no build plots"),
            Self::TooManyPlots { found } => write!(
                f,
                "the map has {found} plots, past the {MAX_PLOTS} a frame draws"
            ),
            Self::OnTheLane { plot, distance } => write!(
                f,
                "plot {plot:?} is {distance:.2} m from the lane's centre line, inside the \
                 {PLOT_CLEARANCE:.2} m an upgraded tower needs"
            ),
            Self::OutOfReach { plot, distance } => write!(
                f,
                "plot {plot:?} is {distance:.2} m from the lane, past the {SHORTEST_RANGE_M} m the \
                 shortest-reaching tower covers"
            ),
            Self::LabelTooLong { plot, length } => write!(
                f,
                "plot {plot}'s label is {length} bytes, past the {MAX_LABEL_BYTES} a label may hold"
            ),
        }
    }
}

impl std::error::Error for MapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Scene(error) => Some(error),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// The path
// ---------------------------------------------------------------------------

/// The most waypoints a map may have, and so one more than the most legs.
///
/// **What the reserved pools are sized against**, not a limit the path's own
/// arithmetic has: every leg is a lane mesh of its own and a lane instance, so
/// `CAPACITIES`' mesh and instance counts have to cover a map this long —
/// `the_largest_map_the_caps_admit_fits_the_pools_it_reserves` builds one and
/// asserts it does. A longer map is refused by [`MapError::TooManyWaypoints`]
/// when it is read, rather than at start-up by a pool error that names no file.
pub const MAX_WAYPOINTS: usize = 16;

/// How wide the lane is drawn, in metres. Decoration: nothing collides with it.
pub const LANE_WIDTH: f64 = 1.8;

/// How proud of the ground the lane stands, in metres.
pub const LANE_HEIGHT: f64 = 0.06;

// ---------------------------------------------------------------------------
// The exit
// ---------------------------------------------------------------------------

/// Half the exit volume's extent, in metres — a two-metre cube standing on the
/// last waypoint.
///
/// A creep whose sphere touches it costs the team a life. See the module docs
/// for why it is a trigger and what that buys.
pub const EXIT_HALF: DVec3 = DVec3::new(1.0, 1.0, 1.0);

impl Map {
    /// Where the exit volume's centre is, in metres: standing on the last
    /// waypoint.
    ///
    /// Derived rather than authored, so a map cannot put its exit anywhere but
    /// the end of the walk — a file has no way to spell an exit the creeps never
    /// reach.
    #[must_use]
    pub fn exit_centre(&self) -> DVec3 {
        let at = self.path.end();
        DVec3::new(at.x, EXIT_HALF.y, at.z)
    }

    /// The exit volume, as the physics world holds it.
    #[must_use]
    pub fn exit_collider(&self) -> BoxCollider {
        BoxCollider::new(self.exit_centre(), EXIT_HALF)
    }
}

// ---------------------------------------------------------------------------
// The build plots
// ---------------------------------------------------------------------------

/// The most build plots a map may have.
///
/// **What a frame's snapshot is sized against.** [`crate::game::RenderState`] is
/// a `Copy` struct of fixed arrays — its docs say why — and its towers, bolts
/// and bursts each hold one entry per plot, so this is the width of those arrays
/// and a map with more plots could not be drawn. The instance reservation covers
/// it too; see [`MAX_WAYPOINTS`]. A map past it is refused by
/// [`MapError::TooManyPlots`] rather than drawn with its last plots missing.
///
/// Well inside what the `PlaceTower` frame can carry: its plot byte keeps its
/// top value as the "no plot" sentinel, and `crate::game`'s
/// `the_no_plot_sentinel_is_not_a_plot` is what holds the two apart.
pub const MAX_PLOTS: usize = 16;

/// The most bytes a plot's label may hold.
///
/// **What the map's wire form is bounded by.** A host sends its map to every
/// joiner (`map::wire`), and a joiner refuses a label past this before it
/// allocates for it; holding every map to it here is what makes every map a
/// host can load one its joiners accept. Far more than the overlay's plot
/// column shows, and than any committed label needs.
pub const MAX_LABEL_BYTES: usize = 32;

/// How far a plot's centre must stand from the lane's centre line, in metres:
/// half the lane, and an **upgraded** tower's radius beside it.
///
/// The upgraded radius rather than the base one, because every plot can be
/// stepped up and a tower standing over the lane is a tower a creep walks
/// through. [`Map::new`] refuses a plot inside it, as [`MapError::OnTheLane`].
pub const PLOT_CLEARANCE: f64 = 0.5 * LANE_WIDTH + TOWER_RADIUS * UPGRADED_SCALE as f64;

/// How wide a build pad is drawn, in metres.
pub const PAD_EDGE: f64 = 2.0;

/// How proud of the ground a build pad stands, in metres.
pub const PAD_HEIGHT: f64 = 0.08;

// ---------------------------------------------------------------------------
// What the pieces are, in metres
// ---------------------------------------------------------------------------

/// A creep's radius, in metres. **The collider's and the mesh's**, so what
/// looks shootable is shootable.
pub const CREEP_RADIUS: f64 = 0.45;

/// How wide a tower is, in metres.
pub const TOWER_RADIUS: f64 = 0.55;

/// How tall a tower stands, in metres.
pub const TOWER_HEIGHT: f64 = 2.0;

/// How high a tower's muzzle is, in metres — where every bolt starts.
pub const MUZZLE_Y: f64 = 1.6;

/// A bolt's radius, in metres. The **sweep's** radius as well as the mesh's.
pub const BOLT_RADIUS: f64 = 0.16;

/// How much taller an upgraded tower stands than the one it replaced.
///
/// A uniform scale rather than a stretch along `Y`: a non-uniform one tilts every
/// side normal away from the surface it belongs to, and the shading of a greybox
/// cylinder is the one thing on this field that would quietly look wrong. Wider
/// as well as taller is also the more legible reading from directly overhead,
/// which is where this sample is read from — see [`crate::camera`].
pub const UPGRADED_SCALE: f32 = 1.35;

impl Map {
    /// How many bolts a frame draws: one slot per plot.
    ///
    /// A pool rather than a count of what is in flight: instances are added once
    /// at start-up and the unused ones are parked at [`PARK`], because adding and
    /// removing an instance every frame would churn the pool for a thing that
    /// lives a fraction of a second.
    ///
    /// **One slot per plot is the real bound and not a guess.** A bolt is in the
    /// air for less time than a tower takes to reload, so a tower never has two
    /// of them out at once and a full field has one each: `crate::tower`'s
    /// `a_bolt_lands_long_before_its_tower_reloads` asserts that timing on the
    /// longest flight there is, and `crate::game`'s
    /// `a_splash_and_a_slow_tower_hold_the_whole_table` measures the peak over a
    /// whole run against this pool. Bolts past it would be simulated and not
    /// drawn, which would be a presentation limit and never a simulation one —
    /// those two tests are what say the case does not arise.
    #[must_use]
    pub fn max_bolts(&self) -> usize {
        self.plots.len()
    }

    /// How many splash bursts a frame draws: one slot per plot.
    ///
    /// The same argument [`Map::max_bolts`] makes, one step further along. A
    /// burst is drawn for [`crate::tower::BURST_S`] and every bursting row's
    /// reload is longer than that, so a plot never has two bursts on screen at
    /// once and a full field of splash towers has one each — `crate::tower`'s
    /// `a_burst_is_gone_before_its_tower_can_raise_another` asserts the
    /// inequality, and `crate::game`'s
    /// `a_splash_and_a_slow_tower_hold_the_whole_table` measures the peak over a
    /// whole run against this pool.
    #[must_use]
    pub fn max_bursts(&self) -> usize {
        self.plots.len()
    }
}

/// Where an unused instance is parked: under the ground slab, inside its
/// footprint, so the opaque floor hides it.
///
/// `the_parking_spot_is_under_the_ground` asserts both halves.
pub const PARK: DVec3 = DVec3::new(0.0, -6.0, 0.0);

// ---------------------------------------------------------------------------
// The scene description
// ---------------------------------------------------------------------------

/// The ground slab — [`SceneDesc::meshes`] slot 0.
pub const GROUND_MESH: usize = 0;
/// The first leg of the lane; leg `i` is `LANE_MESH + i`. One mesh per leg
/// because each is its own length, which is what keeps the lane the same
/// numbers [`crate::path`] measures.
///
/// Every slot after the lane moves with the map's leg count, so those are
/// methods on [`Map`] rather than constants: [`Map::pad_mesh`] and the ones
/// after it.
pub const LANE_MESH: usize = 1;

impl Map {
    /// A build pad.
    #[must_use]
    pub fn pad_mesh(&self) -> usize {
        LANE_MESH + self.path.legs()
    }

    /// The exit volume, drawn as the cube it is.
    #[must_use]
    pub fn exit_mesh(&self) -> usize {
        self.pad_mesh() + 1
    }

    /// A creep.
    #[must_use]
    pub fn creep_mesh(&self) -> usize {
        self.exit_mesh() + 1
    }

    /// A tower.
    #[must_use]
    pub fn tower_mesh(&self) -> usize {
        self.creep_mesh() + 1
    }

    /// A bolt in flight.
    #[must_use]
    pub fn bolt_mesh(&self) -> usize {
        self.tower_mesh() + 1
    }

    /// A splash burst.
    ///
    /// A **unit** sphere, scaled by the burst's own radius when it is drawn —
    /// the two bursting rows of [`crate::tower::TOWERS`] reach different
    /// distances and one mesh per radius would be a mesh per row for ever.
    #[must_use]
    pub fn burst_mesh(&self) -> usize {
        self.bolt_mesh() + 1
    }

    /// How many meshes this map makes resident.
    #[must_use]
    pub fn meshes(&self) -> usize {
        self.burst_mesh() + 1
    }
}

/// The ground. [`SceneDesc::materials`] slot 0, and therefore what an instance
/// placed without a named material would shade through.
pub const GROUND_MATERIAL: usize = 0;
/// The lane the creeps walk.
pub const LANE_MATERIAL: usize = 1;
/// A build pad.
pub const PAD_MATERIAL: usize = 2;
/// The exit volume.
pub const EXIT_MATERIAL: usize = 3;
/// The first creep kind's row; kind `k` is `CREEP_MATERIAL + k`, in
/// [`crate::creep::ALL`]'s order. [`creep_material`] is the mapping.
pub const CREEP_MATERIAL: usize = 4;
/// A creep of any kind that has been shot down to half its health or less.
/// **The picture says which**, for the reason a knocked-down plate is drawn
/// orange on breach's range: a state a reviewer cannot see is a state they
/// cannot check the readout against.
pub const CREEP_HURT_MATERIAL: usize = CREEP_MATERIAL + crate::creep::KINDS;
/// …and one a [`crate::tower::Kind::Slow`] tower is holding. The only thing on
/// the field that says the hold is being applied rather than merely priced.
pub const CREEP_SLOWED_MATERIAL: usize = CREEP_HURT_MATERIAL + 1;
/// The first tower kind's row; kind `k` is `TOWER_MATERIAL + k`, in
/// [`crate::tower::ALL`]'s order. [`tower_material`] is the mapping.
pub const TOWER_MATERIAL: usize = CREEP_SLOWED_MATERIAL + 1;
/// A tower of any kind that worked this tick — fired, or held a creep.
pub const TOWER_FIRING_MATERIAL: usize = TOWER_MATERIAL + crate::tower::KINDS;
/// A bolt.
pub const BOLT_MATERIAL: usize = TOWER_FIRING_MATERIAL + 1;
/// A splash burst.
pub const BURST_MATERIAL: usize = BOLT_MATERIAL + 1;
/// How many material rows this map declares.
pub const MATERIALS: usize = BURST_MATERIAL + 1;

/// Which row a creep of `view`'s kind and state is drawn through.
///
/// Hurt beats held where both apply — see the module docs.
#[must_use]
pub fn creep_material(view: &crate::creep::CreepView) -> usize {
    if view.hurt {
        CREEP_HURT_MATERIAL
    } else if view.slowed {
        CREEP_SLOWED_MATERIAL
    } else {
        CREEP_MATERIAL + view.kind.index()
    }
}

/// Which row a tower of `kind` is drawn through when it is not working.
#[must_use]
pub const fn tower_material(kind: crate::tower::Kind) -> usize {
    TOWER_MATERIAL + kind.index()
}

/// How many latitude bands and longitude columns a creep is drawn with, and how
/// many facets a tower's cylinder has.
///
/// Enough to read as a ball and a post from the overhead camera and no more:
/// this is a greybox field, and the browser the next slice publishes to is the
/// target.
const CREEP_RINGS: u32 = 8;
const CREEP_SEGMENTS: u32 = 14;
const TOWER_SEGMENTS: u32 = 12;
const BOLT_RINGS: u32 = 4;
const BOLT_SEGMENTS: u32 = 8;
const BURST_RINGS: u32 = 6;
const BURST_SEGMENTS: u32 = 10;

/// What this map reserves, which is a little over what the largest map
/// [`MAX_WAYPOINTS`] and [`MAX_PLOTS`] admit places.
///
/// Sized against the description rather than left at [`Capacities::default`],
/// for `apps/breach/src/map.rs`'s reason: that default reserves far more
/// instances than this field needs, and the level-of-detail state behind that
/// number is a word per instance per draw generator. Filling any of these is a
/// mistake in this file rather than a condition a run can be in:
/// `the_map_fits_the_pools_it_reserves` asserts the committed field fits, and
/// `the_largest_map_the_caps_admit_fits_the_pools_it_reserves` that every map
/// [`Map::new`] accepts does.
const CAPACITIES: Capacities = Capacities {
    vertices: 8 * 1024,
    indices: 16 * 1024,
    meshes: 24,
    instances: 320,
    materials: 16,
    lights: 4,
    probes: 0,
};

/// A painted greybox material: the metric grid of [`grid_page`], tinted, and
/// tiled **physically** so one tile measures [`GREYBOX_TILE_M`] of surface
/// however large the face is.
///
/// The tint is this map's own and the grid is the engine's. It spends the 32²
/// grid page rather than `crcbl::greybox::greybox_material`'s 1024² one,
/// because a demo that runs in a browser should not upload eight megatexels to
/// show a ruler. `apps/breach/src/map.rs` has the same helper for the same
/// reason.
fn painted(tint: [f32; 3]) -> GpuMaterial {
    GpuMaterial {
        base_color: [tint[0], tint[1], tint[2], 1.0],
        tiling: GpuMaterial::TILING_PHYSICAL,
        tile_metres: GREYBOX_TILE_M,
        ..grid_material()
    }
}

impl Map {
    /// How wide and deep leg `leg` of the lane is drawn, in metres.
    ///
    /// The leg's own length across whichever axis it runs, widened by
    /// [`LANE_WIDTH`] on both — so the square end of one leg fills the corner
    /// the next one turns out of and the lane reads as continuous.
    fn lane_extent(&self, leg: usize) -> (f64, f64) {
        let waypoints = self.path.waypoints();
        let step = waypoints[leg + 1] - waypoints[leg];
        (step.x.abs() + LANE_WIDTH, step.z.abs() + LANE_WIDTH)
    }

    /// Everything this map makes resident: [`Map::meshes`] meshes,
    /// [`MATERIALS`] painted rows and the grid page they sample.
    ///
    /// The mesh order is [`GROUND_MESH`], [`LANE_MESH`] and the methods after
    /// it, and the material order is the constants above, in value order; keep
    /// them and this assembly in step, which
    /// `the_constants_name_their_own_meshes` asserts.
    #[must_use]
    pub fn scene(&self) -> SceneDesc<'static> {
        let mesh = |label: &'static str, geometry: Geometry<'static>| MeshDesc {
            label: Cow::Borrowed(label),
            geometry,
        };
        let mut meshes = Vec::with_capacity(self.meshes());
        meshes.push(mesh(
            "ground",
            platform(
                2.0 * HALF_WIDTH as f32,
                2.0 * HALF_DEPTH as f32,
                SLAB_THICKNESS as f32,
            ),
        ));
        for leg in 0..self.path.legs() {
            let (width, depth) = self.lane_extent(leg);
            meshes.push(mesh(
                "lane",
                platform(width as f32, depth as f32, LANE_HEIGHT as f32),
            ));
        }
        meshes.push(mesh(
            "pad",
            platform(PAD_EDGE as f32, PAD_EDGE as f32, PAD_HEIGHT as f32),
        ));
        meshes.push(mesh(
            "exit",
            platform(
                2.0 * EXIT_HALF.x as f32,
                2.0 * EXIT_HALF.z as f32,
                2.0 * EXIT_HALF.y as f32,
            ),
        ));
        meshes.push(mesh(
            "creep",
            sphere(CREEP_RADIUS as f32, CREEP_RINGS, CREEP_SEGMENTS),
        ));
        meshes.push(mesh(
            "tower",
            cylinder(TOWER_RADIUS as f32, TOWER_HEIGHT as f32, TOWER_SEGMENTS),
        ));
        meshes.push(mesh(
            "bolt",
            sphere(BOLT_RADIUS as f32, BOLT_RINGS, BOLT_SEGMENTS),
        ));
        meshes.push(mesh("burst", sphere(1.0, BURST_RINGS, BURST_SEGMENTS)));

        SceneDesc {
            meshes,
            // In the constants' own order — `the_palette_is_the_one_the_constants_index`
            // asserts it, because a row out of place is a creep kind drawn as a tower
            // and a picture nobody would think to disbelieve.
            materials: vec![
                // The field.
                painted([0.26, 0.31, 0.24]),
                painted([0.46, 0.42, 0.32]),
                painted([0.30, 0.38, 0.46]),
                painted([0.72, 0.30, 0.28]),
                // One per creep kind: fast is the green the single archetype always
                // was, tanky a heavier slate, swarm a pale wash — light things read
                // as light ones from overhead.
                painted([0.55, 0.72, 0.40]),
                painted([0.36, 0.40, 0.52]),
                painted([0.82, 0.86, 0.62]),
                // …and the two states over them: hurt, then held.
                painted([0.86, 0.52, 0.24]),
                painted([0.40, 0.72, 0.88]),
                // One per tower kind: the bolt's grey post, the splash's rust, the
                // slow tower's cold blue — the same hue its hold tints a creep.
                painted([0.58, 0.62, 0.70]),
                painted([0.74, 0.44, 0.34]),
                painted([0.34, 0.52, 0.66]),
                // …and a tower of any kind that worked this tick.
                painted([0.95, 0.88, 0.45]),
                // The bolt, and the burst it leaves.
                painted([0.98, 0.94, 0.60]),
                painted([1.0, 0.72, 0.36]),
            ],
            page: grid_page(),
            probes: ProbeGrid::default(),
            capacities: CAPACITIES,
        }
    }
}

/// The instances a frame rewrites: one pool per moving thing.
///
/// The field itself — the ground, the lane, the pads and the exit — is placed
/// once and drawn for the rest of the run, so it is not in here.
#[derive(Debug)]
pub struct Field {
    creeps: [InstanceHandle; MAX_CREEPS],
    /// One per plot, in the map's plot order.
    towers: Vec<InstanceHandle>,
    /// [`Map::max_bolts`] of them.
    bolts: Vec<InstanceHandle>,
    /// [`Map::max_bursts`] of them.
    bursts: Vec<InstanceHandle>,
    /// Where each plot's tower stands, in the same order as `towers`.
    plots: Vec<DVec3>,
    /// The mesh slots the pools draw. Held rather than asked of the map each
    /// frame, because they sit after however many legs the lane has — see
    /// [`Map::pad_mesh`].
    creep_mesh: usize,
    tower_mesh: usize,
    bolt_mesh: usize,
    burst_mesh: usize,
}

/// Where a creep's mesh sits, given where its centre is.
fn creep_transform(centre: DVec3) -> Mat4 {
    Mat4::from_translation(Vec3::new(centre.x as f32, centre.y as f32, centre.z as f32))
}

impl Field {
    /// How many plots there are, and so how many tower slots.
    #[must_use]
    pub fn plots(&self) -> usize {
        self.towers.len()
    }

    /// How many bolt slots there are — [`Map::max_bolts`].
    #[must_use]
    pub fn bolt_slots(&self) -> usize {
        self.bolts.len()
    }

    /// How many burst slots there are — [`Map::max_bursts`].
    #[must_use]
    pub fn burst_slots(&self) -> usize {
        self.bursts.len()
    }

    /// Draws one creep, or parks it under the ground when the pool is longer
    /// than the field is populated.
    ///
    /// # Panics
    ///
    /// If `index` is not in the pool. Called only from `crate::gpu`'s own
    /// enumeration of it.
    pub fn set_creep(
        &self,
        renderer: &mut ForwardRenderer,
        index: usize,
        view: Option<crate::creep::CreepView>,
    ) {
        let (material, centre) = match &view {
            Some(view) => (creep_material(view), view.centre),
            None => (CREEP_MATERIAL, PARK),
        };
        renderer.set_instance(
            self.creeps[index],
            &InstanceDesc {
                mesh: self.creep_mesh,
                material,
                transform: creep_transform(centre),
            },
        );
    }

    /// Draws one tower, or parks it on an empty plot.
    ///
    /// An upgraded one stands [`UPGRADED_SCALE`] times as tall on the same pad,
    /// which is the whole of how the picture says a plot has been stepped up.
    ///
    /// # Panics
    ///
    /// If `plot` is not a plot. Called only from `crate::gpu`'s own
    /// enumeration of [`Field::plots`].
    pub fn set_tower(
        &self,
        renderer: &mut ForwardRenderer,
        plot: usize,
        view: Option<crate::tower::TowerView>,
    ) {
        let (material, at, scale) = match view {
            Some(view) => (
                if view.working {
                    TOWER_FIRING_MATERIAL
                } else {
                    tower_material(view.kind)
                },
                self.plots[plot],
                match view.tier {
                    crate::tower::Tier::Base => 1.0,
                    crate::tower::Tier::Upgraded => UPGRADED_SCALE,
                },
            ),
            None => (TOWER_MATERIAL, PARK, 1.0),
        };
        renderer.set_instance(
            self.towers[plot],
            &InstanceDesc {
                mesh: self.tower_mesh,
                material,
                transform: Mat4::from_scale_rotation_translation(
                    Vec3::splat(scale),
                    Quat::IDENTITY,
                    Vec3::new(at.x as f32, at.y as f32, at.z as f32),
                ),
            },
        );
    }

    /// Draws one bolt, or parks it under the ground.
    ///
    /// # Panics
    ///
    /// If `index` is not in the pool. Called only from `crate::gpu`'s own
    /// enumeration of it.
    pub fn set_bolt(&self, renderer: &mut ForwardRenderer, index: usize, at: Option<DVec3>) {
        renderer.set_instance(
            self.bolts[index],
            &InstanceDesc {
                mesh: self.bolt_mesh,
                material: BOLT_MATERIAL,
                transform: creep_transform(at.unwrap_or(PARK)),
            },
        );
    }

    /// Draws one splash burst at the size the overlap that raised it was run
    /// at, or parks it under the ground.
    ///
    /// The parked slot keeps the unit mesh's own size rather than being scaled
    /// to nothing: a zero scale is a degenerate transform, and the slab is what
    /// hides a parked instance here as it does every other.
    ///
    /// # Panics
    ///
    /// If `index` is not in the pool. Called only from `crate::gpu`'s own
    /// enumeration of it.
    pub fn set_burst(
        &self,
        renderer: &mut ForwardRenderer,
        index: usize,
        view: Option<crate::tower::BurstView>,
    ) {
        let (centre, radius) = match view {
            Some(view) => (view.centre, view.radius_m as f32),
            None => (PARK, 1.0),
        };
        renderer.set_instance(
            self.bursts[index],
            &InstanceDesc {
                mesh: self.burst_mesh,
                material: BURST_MATERIAL,
                transform: Mat4::from_scale_rotation_translation(
                    Vec3::splat(radius),
                    Quat::IDENTITY,
                    Vec3::new(centre.x as f32, centre.y as f32, centre.z as f32),
                ),
            },
        );
    }
}

impl Map {
    /// Places the field and hands back the pools that move.
    ///
    /// The lights are set here too, and they are sticky: nothing in this sample
    /// moves the sun.
    ///
    /// # Errors
    ///
    /// [`InstancePoolError`] if `CAPACITIES`' instance count does not cover the
    /// map, which is this file's numbers being wrong rather than a condition a
    /// run can be in — [`Map::new`] refuses every map past the caps that count
    /// is sized for.
    pub fn place(&self, renderer: &mut ForwardRenderer) -> Result<Field, InstancePoolError> {
        let at = |x: f64, y: f64, z: f64| {
            Mat4::from_translation(Vec3::new(x as f32, y as f32, z as f32))
        };

        // A `platform` rises from `y = 0`, so the ground is dropped by its own
        // thickness to put its top there.
        renderer.add_instance(&InstanceDesc {
            mesh: GROUND_MESH,
            material: GROUND_MATERIAL,
            transform: at(0.0, -SLAB_THICKNESS, 0.0),
        })?;

        for (leg, pair) in self.path.waypoints().windows(2).enumerate() {
            let middle = 0.5 * (pair[0] + pair[1]);
            renderer.add_instance(&InstanceDesc {
                mesh: LANE_MESH + leg,
                material: LANE_MATERIAL,
                transform: at(middle.x, 0.0, middle.z),
            })?;
        }

        for plot in &self.plots {
            let feet = plot.at();
            renderer.add_instance(&InstanceDesc {
                mesh: self.pad_mesh(),
                material: PAD_MATERIAL,
                transform: at(feet.x, 0.0, feet.z),
            })?;
        }

        let exit = self.path.end();
        renderer.add_instance(&InstanceDesc {
            mesh: self.exit_mesh(),
            material: EXIT_MATERIAL,
            transform: at(exit.x, 0.0, exit.z),
        })?;

        // The pools last, every one of them parked: the first frame draws the
        // field before the first tick has spawned anything, and a pool left at
        // the origin would put a creep on the ground before the wave began.
        let mut creeps = Vec::with_capacity(MAX_CREEPS);
        for _ in 0..MAX_CREEPS {
            creeps.push(renderer.add_instance(&InstanceDesc {
                mesh: self.creep_mesh(),
                material: CREEP_MATERIAL,
                transform: creep_transform(PARK),
            })?);
        }
        let mut towers = Vec::with_capacity(self.plots.len());
        for _ in 0..self.plots.len() {
            towers.push(renderer.add_instance(&InstanceDesc {
                mesh: self.tower_mesh(),
                material: TOWER_MATERIAL,
                transform: creep_transform(PARK),
            })?);
        }
        let mut bolts = Vec::with_capacity(self.max_bolts());
        for _ in 0..self.max_bolts() {
            bolts.push(renderer.add_instance(&InstanceDesc {
                mesh: self.bolt_mesh(),
                material: BOLT_MATERIAL,
                transform: creep_transform(PARK),
            })?);
        }
        let mut bursts = Vec::with_capacity(self.max_bursts());
        for _ in 0..self.max_bursts() {
            bursts.push(renderer.add_instance(&InstanceDesc {
                mesh: self.burst_mesh(),
                material: BURST_MATERIAL,
                transform: creep_transform(PARK),
            })?);
        }

        // No point lights: this field is outdoors and [`sun`] is what lights it.
        renderer.set_lights(&[]);

        Ok(Field {
            creeps: creeps
                .try_into()
                .unwrap_or_else(|_| unreachable!("one instance per pooled creep was pushed")),
            towers,
            bolts,
            bursts,
            plots: self.plots.iter().map(Plot::at).collect(),
            creep_mesh: self.creep_mesh(),
            tower_mesh: self.tower_mesh(),
            bolt_mesh: self.bolt_mesh(),
            burst_mesh: self.burst_mesh(),
        })
    }
}

// ---------------------------------------------------------------------------
// The collision side
// ---------------------------------------------------------------------------

impl Map {
    /// The field as the colliders a bolt sweeps against, with the exit
    /// trigger's id — which is what [`crate::creep`] compares an overlap's
    /// answer to.
    ///
    /// Two colliders and no more: the ground, so a bolt whose target died stops
    /// in it rather than flying under the map for ever, and the exit volume.
    /// The creeps add their own — see the module docs.
    #[must_use]
    pub fn world(&self) -> (PhysicsWorld, ColliderId) {
        let mut world = PhysicsWorld::new();
        world.add_box(BoxCollider::new(
            DVec3::new(0.0, -0.5 * SLAB_THICKNESS, 0.0),
            DVec3::new(HALF_WIDTH, 0.5 * SLAB_THICKNESS, HALF_DEPTH),
        ));
        let exit = world.add_box(self.exit_collider());
        world.set_trigger(exit, true);
        (world, exit)
    }
}

// ---------------------------------------------------------------------------
// The light
// ---------------------------------------------------------------------------

/// How bright the sun is, before its colour.
const SUN_INTENSITY: f32 = 2.6;

/// How high it stands, as the `Y` component of a unit direction **toward** it.
const SUN_ELEVATION: f32 = 0.72;

/// The sun this field is lit by.
///
/// Fixed rather than turning: a tower defense is read from directly overhead
/// and a moving shadow would be the one thing on screen a player is not meant
/// to be watching. `apps/puppet::map::sun` is the one that turns, and it says
/// why it does.
#[must_use]
pub fn sun() -> DirectionalLight {
    let flat = (1.0 - SUN_ELEVATION * SUN_ELEVATION).sqrt();
    DirectionalLight {
        direction: Vec3::new(flat * -0.55, SUN_ELEVATION, flat * 0.84),
        color: Vec3::new(1.0, 0.97, 0.90) * SUN_INTENSITY,
        ambient: Vec3::new(0.17, 0.19, 0.22),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed field, which is what every test here reads the path and
    /// the plots off.
    fn map() -> Map {
        Map::built_in()
    }

    /// **Every mesh the description makes resident is placed, and every row it
    /// declares is named**, in the order the constants say.
    ///
    /// A mesh nothing places is memory taken for geometry no frame draws, and a
    /// row nothing names is a colour nobody can see — both of which leave a
    /// perfectly plausible picture. `apps/breach/src/map.rs` and
    /// `apps/puppet/src/map.rs` assert the same pair.
    #[test]
    fn the_constants_name_their_own_meshes() {
        let map = map();
        let scene = map.scene();
        assert_eq!(
            scene.meshes.len(),
            map.meshes(),
            "the mesh list is not Map::meshes long"
        );
        assert_eq!(
            scene.materials.len(),
            MATERIALS,
            "the material list is not MATERIALS long",
        );
        for (slot, label) in [
            (GROUND_MESH, "ground"),
            (LANE_MESH, "lane"),
            (map.pad_mesh(), "pad"),
            (map.exit_mesh(), "exit"),
            (map.creep_mesh(), "creep"),
            (map.tower_mesh(), "tower"),
            (map.bolt_mesh(), "bolt"),
            (map.burst_mesh(), "burst"),
        ] {
            assert_eq!(
                scene.meshes[slot].label, label,
                "slot {slot} is not the {label}",
            );
        }
    }

    /// **Every material row is named by something, exactly once, and every kind
    /// gets one of its own.**
    ///
    /// The per-kind rows are the half worth asserting. A build that mapped two
    /// creep kinds — or two tower kinds — to one row leaves a field that reads
    /// perfectly and is telling the player the wrong thing about what is walking
    /// at them, and nothing else in this crate would notice: the simulation is
    /// unaffected. The count is the control: a row nothing indexes is a colour
    /// nobody can see, and a row two things index is the drift above.
    #[test]
    fn the_palette_is_the_one_the_constants_index() {
        let mut named = [0_u32; MATERIALS];
        for row in [
            GROUND_MATERIAL,
            LANE_MATERIAL,
            PAD_MATERIAL,
            EXIT_MATERIAL,
            CREEP_HURT_MATERIAL,
            CREEP_SLOWED_MATERIAL,
            TOWER_FIRING_MATERIAL,
            BOLT_MATERIAL,
            BURST_MATERIAL,
        ] {
            named[row] += 1;
        }
        for kind in crate::creep::ALL {
            let view = crate::creep::CreepView {
                kind,
                ..crate::creep::CreepView::default()
            };
            named[creep_material(&view)] += 1;
        }
        for kind in crate::tower::ALL {
            named[tower_material(kind)] += 1;
        }
        for (row, times) in named.iter().enumerate() {
            assert_eq!(
                *times, 1,
                "material row {row} is named {times} time(s), not once",
            );
        }

        // And the two states win over the kind, in the order the module docs
        // give: hurt beats held.
        let held = crate::creep::CreepView {
            slowed: true,
            ..crate::creep::CreepView::default()
        };
        assert_eq!(creep_material(&held), CREEP_SLOWED_MATERIAL);
        let both = crate::creep::CreepView { hurt: true, ..held };
        assert_eq!(
            creep_material(&both),
            CREEP_HURT_MATERIAL,
            "a hurt creep that is also held is not drawn hurt",
        );
    }

    /// **The map fits the pools it reserves**, with the pooled creeps, towers
    /// and bolts counted in — the instances a run can reach are the fixed field
    /// plus every pool, and a description that only fitted an empty field would
    /// fail on the first wave rather than at start-up.
    #[test]
    fn the_map_fits_the_pools_it_reserves() {
        let map = map();
        let plots = map.plots().len();
        let placed = 1
            + map.path().legs()
            + plots
            + 1
            + MAX_CREEPS
            + plots
            + map.max_bolts()
            + map.max_bursts();
        assert!(
            placed <= CAPACITIES.instances as usize,
            "the map places {placed} instances into {}",
            CAPACITIES.instances,
        );
        assert!(map.meshes() <= CAPACITIES.meshes as usize);
        assert!(MATERIALS <= CAPACITIES.materials as usize);
    }

    /// **An unused instance is parked where the ground hides it**: under the
    /// slab, and inside its footprint. A park point beside the field would be a
    /// creep a player can see standing in the grass before the wave starts.
    #[test]
    fn the_parking_spot_is_under_the_ground() {
        // Every operand is a constant, so this is checked while the crate is
        // compiled rather than while its tests run — which is the earliest a
        // wrong number here could possibly be caught.
        const {
            assert!(
                PARK.y + CREEP_RADIUS < -SLAB_THICKNESS,
                "a parked creep pokes up through the ground",
            );
            assert!(
                PARK.x > -HALF_WIDTH && PARK.x < HALF_WIDTH,
                "the park point is off the field across X",
            );
            assert!(
                PARK.z > -HALF_DEPTH && PARK.z < HALF_DEPTH,
                "the park point is off the field along Z",
            );
        }
    }

    /// **Every leg of the path runs along one axis**, which is what lets the
    /// lane be one `platform` per leg and what [`crate::path`] measures.
    #[test]
    fn every_leg_of_the_path_is_axis_aligned() {
        let map = map();
        let path = map.path().waypoints();
        for leg in 0..map.path().legs() {
            let step = path[leg + 1] - path[leg];
            assert_eq!(step.y, 0.0, "leg {leg} climbs");
            assert!(
                (step.x == 0.0) != (step.z == 0.0),
                "leg {leg} runs diagonally, at {step:?}",
            );
            assert!(
                step.length() > LANE_WIDTH,
                "leg {leg} is shorter than the lane is wide"
            );
        }
    }

    /// **The whole path is on the field**, lane and all: a leg that ran off the
    /// slab would be creeps walking on nothing.
    #[test]
    fn the_path_stays_on_the_ground() {
        for &point in map().path().waypoints() {
            assert!(
                point.x.abs() + 0.5 * LANE_WIDTH <= HALF_WIDTH,
                "{point:?} is off the field across X",
            );
            assert!(
                point.z.abs() + 0.5 * LANE_WIDTH <= HALF_DEPTH,
                "{point:?} is off the field along Z",
            );
        }
    }

    /// **The exit volume is a trigger, so a bolt goes through it and an overlap
    /// finds it.** The two halves of what `set_trigger` buys, asserted against
    /// the world this map builds rather than against the engine's own docs.
    ///
    /// The sweep is the control for the overlap: a build in which the exit was
    /// an ordinary box passes the overlap and fails the sweep, which is exactly
    /// the failure that would stop every bolt fired down the last leg.
    #[test]
    fn the_exit_is_a_volume_a_bolt_flies_through_and_an_overlap_reports() {
        use crcbl::phys::Segment;

        let map = map();
        let (mut world, exit) = map.world();
        assert!(world.is_trigger(exit), "the exit was registered solid");

        let centre = map.exit_centre();
        assert!(
            world.overlap_sphere(centre, CREEP_RADIUS).contains(&exit),
            "a creep standing in the exit is not reported by the overlap",
        );

        // Straight through the volume, at the height a bolt aimed at a creep
        // standing in it would be.
        let across = Segment::new(
            centre + DVec3::new(4.0, 0.0, 0.0),
            centre + DVec3::new(-4.0, 0.0, 0.0),
        );
        assert_eq!(
            world.sweep_sphere(&across, BOLT_RADIUS),
            None,
            "the exit volume stopped a bolt",
        );
    }

    /// **Every plot stands clear of the lane**, so a tower is beside the path
    /// rather than on it, and **near enough to reach it**, so a plot is a build
    /// site rather than a decoration. The pair is what makes the five plots a
    /// choice.
    #[test]
    fn every_plot_stands_clear_of_the_lane_and_still_covers_it() {
        let clearance = 0.5 * LANE_WIDTH + TOWER_RADIUS;
        let map = map();
        let path = map.path().waypoints();
        for plot in map.plots() {
            let mut nearest = f64::INFINITY;
            for leg in 0..map.path().legs() {
                let (from, to) = (path[leg], path[leg + 1]);
                let step = to - from;
                let t = ((plot.at() - from).dot(step) / step.length_squared()).clamp(0.0, 1.0);
                nearest = nearest.min((from + step * t - plot.at()).length());
            }
            assert!(
                nearest > clearance,
                "{} sits {nearest:.2} m from the lane, inside the {clearance:.2} m it is wide",
                plot.label,
            );
            assert!(
                nearest < crate::tower::SHORTEST_RANGE_M,
                "{} is {nearest:.2} m from the nearest leg, past the {} m the \
                 shortest-reaching kind covers",
                plot.label,
                crate::tower::SHORTEST_RANGE_M,
            );
        }
    }

    /// **An upgraded tower still fits its pad and still clears the lane.**
    ///
    /// [`UPGRADED_SCALE`] makes a stepped-up tower wider as well as taller, and
    /// a tower standing over the lane is a tower a creep walks through — which
    /// looks like a bug in the physics and is a number in this file.
    #[test]
    fn an_upgraded_tower_still_fits_its_pad_and_clears_the_lane() {
        let radius = TOWER_RADIUS * f64::from(UPGRADED_SCALE);
        assert!(
            radius < 0.5 * PAD_EDGE,
            "an upgraded tower is {radius:.2} m across the radius on a {PAD_EDGE} m pad",
        );
        let clearance = 0.5 * LANE_WIDTH + radius;
        let map = map();
        let path = map.path().waypoints();
        for plot in map.plots() {
            let mut nearest = f64::INFINITY;
            for leg in 0..map.path().legs() {
                let (from, to) = (path[leg], path[leg + 1]);
                let step = to - from;
                let t = ((plot.at() - from).dot(step) / step.length_squared()).clamp(0.0, 1.0);
                nearest = nearest.min((from + step * t - plot.at()).length());
            }
            assert!(
                nearest > clearance,
                "an upgraded tower on {} sits {nearest:.2} m from the lane, inside the \
                 {clearance:.2} m it then needs",
                plot.label,
            );
        }
    }

    // -- the rules a layout is held to ----------------------------------------

    /// The committed layout, as the two lists [`Map::new`] takes — what every
    /// refusal below starts from and breaks one thing in.
    fn layout() -> (Vec<DVec3>, Vec<Plot>) {
        let map = map();
        (map.path().waypoints().to_vec(), map.plots().to_vec())
    }

    /// What [`Map::new`] refuses `waypoints` and `plots` for.
    fn refused(waypoints: Vec<DVec3>, plots: Vec<Plot>) -> MapError {
        match Map::new(waypoints, plots) {
            Ok(_) => panic!("the layout was accepted"),
            Err(error) => error,
        }
    }

    /// A plot called `label` standing at `x`, `z`.
    fn plot_at(label: &str, x: f64, z: f64) -> Plot {
        Plot {
            label: label.to_string(),
            position: [x, 0.0, z],
        }
    }

    /// **The committed layout passes every rule**, which is the control for
    /// each refusal below: a rule that refused everything would pass all of
    /// them.
    #[test]
    fn the_committed_layout_passes_every_rule() {
        let (waypoints, plots) = layout();
        assert_eq!(
            Map::new(waypoints, plots).expect("the committed layout is a map"),
            map()
        );
    }

    /// **A path is refused for too few waypoints, too many, and one that is
    /// off the ground or off the field** — each by the waypoint at fault.
    #[test]
    fn a_path_is_refused_by_the_waypoint_that_breaks_a_rule() {
        let (waypoints, plots) = layout();

        assert!(matches!(
            refused(waypoints[..1].to_vec(), plots.clone()),
            MapError::TooFewWaypoints { found: 1 }
        ));

        let long: Vec<DVec3> = (0..=MAX_WAYPOINTS)
            .map(|at| waypoints[at % waypoints.len()])
            .collect();
        assert!(matches!(
            refused(long, plots.clone()),
            MapError::TooManyWaypoints { found } if found == MAX_WAYPOINTS + 1
        ));

        let mut raised = waypoints.clone();
        raised[2].y = 0.5;
        assert!(matches!(
            refused(raised, plots.clone()),
            MapError::OffTheGround { what, .. } if what == "waypoint 2"
        ));

        let mut outside = waypoints.clone();
        outside[1].x = HALF_WIDTH;
        outside[2].x = HALF_WIDTH;
        assert!(matches!(
            refused(outside, plots),
            MapError::OffTheField { what } if what == "waypoint 1"
        ));
    }

    /// **A leg is refused for being diagonal or no longer than the lane is
    /// wide** — the two things one `platform` per leg cannot draw.
    #[test]
    fn a_leg_one_platform_cannot_draw_is_refused_by_the_leg() {
        let (waypoints, plots) = layout();

        let mut diagonal = waypoints.clone();
        diagonal[2].x -= 3.0;
        assert!(matches!(
            refused(diagonal, plots.clone()),
            MapError::Diagonal { leg: 1 }
        ));

        let mut short = waypoints.clone();
        short.insert(1, waypoints[0] + DVec3::new(0.5 * LANE_WIDTH, 0.0, 0.0));
        assert!(matches!(
            refused(short, plots.clone()),
            MapError::ShortLeg { leg: 0, .. }
        ));

        // A leg of no length at all is a short leg, not a diagonal one.
        let mut repeated = waypoints;
        repeated.insert(1, repeated[0]);
        assert!(matches!(
            refused(repeated, plots),
            MapError::ShortLeg { leg: 0, length } if length == 0.0
        ));
    }

    /// **The plots are refused when there are none or too many, and a plot is
    /// refused when it is off the ground, off the field, on the lane or out of
    /// the shortest-reaching kind's range of it** — each by its label — **or
    /// when its label is too long**, by its place in the list.
    #[test]
    fn a_plot_is_refused_by_the_rule_it_breaks() {
        let (waypoints, plots) = layout();

        assert!(matches!(
            refused(waypoints.clone(), Vec::new()),
            MapError::NoPlots
        ));

        let crowd: Vec<Plot> = (0..=MAX_PLOTS)
            .map(|at| plots[at % plots.len()].clone())
            .collect();
        assert!(matches!(
            refused(waypoints.clone(), crowd),
            MapError::TooManyPlots { found } if found == MAX_PLOTS + 1
        ));

        let mut raised = plots.clone();
        raised[0].position[1] = 0.25;
        assert!(matches!(
            refused(waypoints.clone(), raised),
            MapError::OffTheGround { what, .. } if what == "plot \"entry\""
        ));

        // Half a pad past the edge, and still beside the lane.
        let mut outside = plots.clone();
        outside[2] = plot_at("east", HALF_WIDTH - 0.5 * PAD_EDGE + 0.1, 1.0);
        assert!(matches!(
            refused(waypoints.clone(), outside),
            MapError::OffTheField { what } if what == "plot \"east\""
        ));

        // On the first leg's centre line.
        let mut on_lane = plots.clone();
        on_lane[1] = plot_at("bend", 0.0, waypoints[0].z);
        assert!(matches!(
            refused(waypoints.clone(), on_lane),
            MapError::OnTheLane { plot, distance } if plot == "bend" && distance == 0.0
        ));

        let mut long = plots.clone();
        long[3].label = "m".repeat(MAX_LABEL_BYTES + 1);
        assert!(matches!(
            refused(waypoints.clone(), long.clone()),
            MapError::LabelTooLong { plot: 3, length } if length == MAX_LABEL_BYTES + 1
        ));
        long[3].label.pop();
        assert!(
            Map::new(waypoints.clone(), long).is_ok(),
            "the cap itself is a label"
        );

        // In the far corner, where nothing on the lane is in reach.
        let mut far = plots;
        far[4] = plot_at(
            "gate",
            -HALF_WIDTH + 0.5 * PAD_EDGE,
            -HALF_DEPTH + 0.5 * PAD_EDGE,
        );
        assert!(matches!(
            refused(waypoints, far),
            MapError::OutOfReach { plot, distance } if plot == "gate" && distance >= SHORTEST_RANGE_M
        ));
    }

    /// The field a per-row refusal of [`Map::new`]'s is about, as the row's
    /// own rule names it — or [`None`] for a map it builds, and for one it
    /// refuses by a rule that needs another row.
    fn row_fault(map: Result<Map, MapError>) -> Option<String> {
        match map {
            Err(MapError::OffTheGround { .. }) => Some("position.1".to_owned()),
            Err(MapError::NotFinite { axis, .. }) => Some(format!("position.{axis}")),
            Err(MapError::LabelTooLong { .. }) => Some("label".to_owned()),
            _ => None,
        }
    }

    /// **The row rules and [`Map::new`] give the same verdict**: each plot and
    /// each corner below, put into the committed layout, is refused by
    /// `Map::new` for a rule of its own row exactly when its own
    /// [`Validate`](crcbl::registry::Validate) refuses it, and for the same
    /// field — so a map built without a scene, from the wire or a test, is
    /// held to what a load and an edit are. Rules that need another row (the
    /// lane's clearance here) are `Map::new`'s alone.
    #[test]
    fn the_row_rules_and_map_new_give_the_same_verdict() {
        use crcbl::registry::Validate;

        use crate::scene::Waypoint;

        let (waypoints, plots) = layout();
        let entry = plots[0].clone();
        let moved = |at: [f64; 3]| Plot {
            position: at,
            ..entry.clone()
        };
        let named = |label: String| Plot {
            label,
            ..entry.clone()
        };
        let [x, _, z] = entry.position;
        let mut tall_and_raised = named("m".repeat(MAX_LABEL_BYTES + 1));
        tall_and_raised.position[1] = 0.25;
        let candidates = [
            entry.clone(),
            moved([x, 0.25, z]),
            moved([x, -0.0, z]),
            moved([x, f64::NAN, z]),
            moved([f64::NAN, 0.0, z]),
            moved([x, 0.0, f64::INFINITY]),
            moved([0.0, 0.0, waypoints[0].z]),
            named("m".repeat(MAX_LABEL_BYTES)),
            named("m".repeat(MAX_LABEL_BYTES + 1)),
            tall_and_raised,
        ];
        let mut refusals = 0;
        for plot in candidates {
            let mut layout = plots.clone();
            layout[0] = plot.clone();
            let whole = row_fault(Map::new(waypoints.clone(), layout));
            refusals += usize::from(whole.is_some());
            assert_eq!(
                plot.validate().err().map(|error| error.field),
                whole,
                "{plot:?}"
            );
        }

        let spawn = waypoints[0];
        for at in [
            spawn,
            DVec3::new(spawn.x, 0.5, spawn.z),
            DVec3::new(spawn.x, -0.0, spawn.z),
            DVec3::new(f64::NAN, 0.0, spawn.z),
            DVec3::new(spawn.x, 0.0, f64::NEG_INFINITY),
        ] {
            let mut path = waypoints.clone();
            path[0] = at;
            let whole = row_fault(Map::new(path, plots.clone()));
            refusals += usize::from(whole.is_some());
            let waypoint = Waypoint {
                order: 0,
                position: at.to_array(),
            };
            assert_eq!(
                waypoint.validate().err().map(|error| error.field),
                whole,
                "{at:?}"
            );
        }
        // The cases above that a row rule refuses, so a rule that refused
        // nothing on either side could not agree its way through.
        assert_eq!(refusals, 9);
    }

    /// **The clearance a plot is held to is an upgraded tower's**, so every
    /// plot [`Map::new`] accepts is one
    /// `an_upgraded_tower_still_fits_its_pad_and_clears_the_lane` would pass.
    /// A plot just outside the base tower's clearance and inside the upgraded
    /// one's is the case that tells the two apart.
    #[test]
    fn a_plot_is_held_to_an_upgraded_towers_clearance() {
        let (waypoints, mut plots) = layout();
        let base = 0.5 * LANE_WIDTH + TOWER_RADIUS;
        let between = 0.5 * (base + PLOT_CLEARANCE);
        assert!(base < between && between < PLOT_CLEARANCE);
        plots[1] = plot_at("bend", 0.0, waypoints[0].z - between);
        assert!(matches!(
            refused(waypoints, plots),
            MapError::OnTheLane { plot, .. } if plot == "bend"
        ));
    }

    /// **The largest map the caps admit fits the pools this file reserves**,
    /// so no map [`Map::new`] accepts can fail at start-up for want of room.
    ///
    /// A real layout rather than a count: a serpentine of [`MAX_WAYPOINTS`]
    /// corners filling the field, with [`MAX_PLOTS`] plots down its two sides,
    /// built through [`Map::new`] — so the caps are shown to be reachable by a
    /// layout the rules accept, and its description is measured the way the
    /// renderer will measure it.
    #[test]
    fn the_largest_map_the_caps_admit_fits_the_pools_it_reserves() {
        use crcbl::shaders::mesh;

        let lanes = MAX_WAYPOINTS / 2;
        let spacing = 3.0;
        let lane_z = |lane: usize| (lane as f64 - 0.5 * (lanes - 1) as f64) * spacing;
        let reach = HALF_WIDTH - 4.0;
        let mut waypoints = Vec::with_capacity(MAX_WAYPOINTS);
        for lane in 0..lanes {
            let (from, to) = if lane % 2 == 0 {
                (-reach, reach)
            } else {
                (reach, -reach)
            };
            waypoints.push(DVec3::new(from, 0.0, lane_z(lane)));
            waypoints.push(DVec3::new(to, 0.0, lane_z(lane)));
        }
        let plots: Vec<Plot> = (0..MAX_PLOTS)
            .map(|at| {
                let side = if at % 2 == 0 { 1.0 } else { -1.0 };
                plot_at(&format!("p{at}"), side * (reach + 2.0), lane_z(at / 2))
            })
            .collect();
        let map = Map::new(waypoints, plots).expect("the serpentine is a map");
        assert_eq!(map.path().waypoints().len(), MAX_WAYPOINTS);
        assert_eq!(map.plots().len(), MAX_PLOTS);

        let plots = map.plots().len();
        let placed = 1
            + map.path().legs()
            + plots
            + 1
            + MAX_CREEPS
            + plots
            + map.max_bolts()
            + map.max_bursts();
        assert!(
            placed <= CAPACITIES.instances as usize,
            "the largest map places {placed} instances into {}",
            CAPACITIES.instances,
        );

        let scene = map.scene();
        let (mut vertices, mut indices) = (0usize, 0usize);
        for desc in &scene.meshes {
            let Geometry::Flat {
                vertices: bytes,
                indices: list,
                ..
            } = &desc.geometry
            else {
                panic!("{}: the field has no cluster DAG in it", desc.label);
            };
            vertices += bytes.len() / mesh::VERTEX_STRIDE;
            indices += list.len();
        }
        assert!(
            scene.meshes.len() <= CAPACITIES.meshes as usize,
            "the largest map has {} meshes and reserves {}",
            scene.meshes.len(),
            CAPACITIES.meshes,
        );
        assert!(
            vertices <= CAPACITIES.vertices as usize,
            "the largest map has {vertices} vertices and reserves {}",
            CAPACITIES.vertices,
        );
        assert!(
            indices <= CAPACITIES.indices as usize,
            "the largest map has {indices} indices and reserves {}",
            CAPACITIES.indices,
        );
    }
}
