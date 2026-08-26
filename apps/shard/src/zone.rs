//! The zone: a grid of modular pieces, and the one table both the meshes and
//! the colliders are read off.
//!
//! ```text
//!            −Z, the shrine
//!   ┌───────────────────────────┐
//!   │ #############             │  # a wall block, a solid tile from the
//!   │ #...........#             │    floor to the top of the walls
//!   │ #...........#             │  . a walkable tile: a floor slab
//!   │ #..P.....P..#             │
//!   │ ######-######             │  P a pillar, floor to the wall top
//!   │ #.T.#...#.T.#             │  T a brazier, and the torch above it
//!   │ #...|...|...#             │  D the dais, a step up
//!   │ #.P.#...#.P.#             │  - a doorway you walk through along Z
//!   │ #####...#####             │  | a doorway you walk through along X
//!   │ #..T..S..T..#             │  S where the character starts
//!   │ #.P.DDDDD.P.#             │
//!   │ #.T.DDDDD.T.#             │
//!   │ #.P.......P.#             │
//!   │ #...........#             │
//!   │ #############             │
//!   └───────────────────────────┘
//!            +Z, the entrance
//! ```
//!
//! # Modular tiling pieces, because `docs/plan/sample/15-shard.md` asks for them
//!
//! That doc's scope section says the zone is "modular hand-authored pieces
//! assembled per seed", and gives the reason: a wall segment whose level-of-
//! detail chain breaks its own edges is visible the moment the next segment no
//! longer meets it, which is what `docs/plan/25-lod.md`'s border locking is for.
//! Slice 1 assembles a **fixed** layout rather than a seeded one — the pieces
//! are what the border-locking argument needs, and a seed is what a later slice
//! adds over them. `docs/backlog.md` carries that.
//!
//! So there is exactly one authored artefact here, [`LAYOUT`], and everything
//! else is read off it: [`scene`] makes one mesh per piece resident, [`place`]
//! walks the grid and writes an instance per tile, and [`world`] walks the
//! *same* grid and writes the colliders. There is no second set of numbers, so
//! a wall you can see is a wall you cannot walk through.
//!
//! # A wall is a solid block, not a panel
//!
//! [`BLOCK_MESH`] is a whole tile of stone from the floor to [`WALL_TOP_Y`].
//! `apps/lantern/src/room.rs` records why that matters and it is not thickness
//! for its own sake: **back faces are culled in the shadow pass too**, so a
//! single-quad wall casts no shadow at all and a room built out of panels is a
//! room whose shadow atlas is written and occludes nothing.
//!
//! # There is no roof over it, and that is the camera's doing
//!
//! Nothing is drawn above [`WALL_TOP_Y`]. [`crate::camera`] hangs the eye five
//! metres over the character's head at the isometric elevation, so a roof over
//! this zone is a roof between the camera and everything the sample exists to
//! show — measured, not guessed: with ceiling slabs over the open tiles the
//! browser gate's canvas came back 93% black and did not change from one frame
//! to the next, because what it was looking at was the *top* of them.
//!
//! It is the answer the genre has always given — an isometric interior is a set
//! seen from above, with the fourth wall and the roof taken away — and it costs
//! the zone nothing it was using: the walls still stand their full height, the
//! doorways still have lintels, and both still cast the shadows the sample is
//! load for.
//!
//! # And still no sun
//!
//! [`house_light`] hands
//! [`begin_frame`](crcbl::render::ForwardRenderer::begin_frame) a
//! [`DirectionalLight`] whose directional term is nearly black. An open-topped
//! zone is one a sun *would* reach, which is exactly the objection: a directional
//! term strong enough to see by would land on every open tile equally and wash
//! out the one thing this sample is here to load. What lights it is
//! [`crate::light`] — the braziers' torches, the shrine's spot, and the
//! irradiance volume that module bakes.

use std::borrow::Cow;

use crcbl::greybox::{GREYBOX_TILE_M, column, doorway, grid_material, grid_page, platform};
use crcbl::math::{DVec3, Mat4, Vec3};
use crcbl::phys::{BoxCollider, PhysicsWorld};
use crcbl::render::scene::{Capacities, Geometry, InstanceDesc, MeshDesc, SceneDesc};
use crcbl::render::{DirectionalLight, ForwardRenderer, InstanceHandle, InstancePoolError};
use crcbl::shaders::mesh::GpuMaterial;

// ---------------------------------------------------------------------------
// The grid
// ---------------------------------------------------------------------------

/// The zone, one character per tile.
///
/// Row 0 is the far end of the zone — the shrine, at the most negative `Z` —
/// and the last row is the wall behind the character's back, so the table reads
/// the way the frame looks when the camera opens: away from you is up the page.
/// [`Cell::of`] is the one place a character becomes a piece, so a character
/// this file does not know about fails the build rather than becoming a hole in
/// the floor.
pub const LAYOUT: [&str; ROWS] = [
    "#############",
    "#...........#",
    "#...........#",
    "#..P.....P..#",
    "######-######",
    "#.T.#...#.T.#",
    "#...|...|...#",
    "#.P.#...#.P.#",
    "#####...#####",
    "#..T..S..T..#",
    "#.P.DDDDD.P.#",
    "#.T.DDDDD.T.#",
    "#.P.......P.#",
    "#...........#",
    "#############",
];

/// How many tiles the zone is across.
pub const COLS: usize = 13;

/// How many tiles it is deep.
pub const ROWS: usize = 15;

/// How wide one tile is, in metres.
///
/// Three metres, which is a doorway and a bit: wide enough that the corridor is
/// walked down rather than squeezed through, and small enough that a 13 × 15
/// grid is a zone a visitor can cross in under a minute.
pub const TILE_M: f64 = 3.0;

/// How high the walls stand, in metres above the floor.
///
/// Nothing is drawn above it — see the module docs for why this zone has no
/// roof — so it is the height of the walls, the pillars and the doorway frames
/// rather than the underside of anything.
pub const WALL_TOP_Y: f64 = 4.0;

/// How thick a floor slab is, in metres. Its **top** is `y = 0`, which is what
/// every other height here is measured from.
pub const SLAB_THICKNESS: f64 = 0.5;

/// How high the dais stands, in metres.
///
/// **Inside the controller's own
/// [`step_offset`](crcbl::phys::CharacterConfig::step_offset)**, so walking onto
/// it is a step-up rather than a wall — which is what makes it vertical variety
/// the character can use rather than scenery.
/// `the_dais_is_a_step_the_controller_climbs` holds it to the controller's
/// number rather than to this sentence.
pub const DAIS_HEIGHT: f64 = 0.35;

/// How wide a pillar is, in metres.
pub const PILLAR_EDGE: f64 = 0.6;

/// How wide a brazier's bowl is, in metres.
pub const BRAZIER_EDGE: f64 = 0.45;

/// How high a brazier stands, in metres — chest height, so the flame above it
/// lights faces rather than feet.
pub const BRAZIER_HEIGHT: f64 = 1.1;

/// How wide a doorway's opening is, in metres.
pub const DOOR_OPENING_M: f64 = 1.8;

/// How high it is, in metres. Under [`WALL_TOP_Y`] by enough that the lintel is
/// a slab with a shadow rather than a line.
pub const DOOR_HEIGHT_M: f64 = 2.6;

/// One tile of the zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    /// Solid stone, floor to wall top.
    Wall,
    /// Walkable floor, with nothing standing on it.
    Floor,
    /// Floor with a pillar standing on it.
    Pillar,
    /// Floor raised by [`DAIS_HEIGHT`].
    Dais,
    /// Floor with a brazier on it, and a torch above the brazier.
    Brazier,
    /// A wall running across `X` with a hole in it: walked through along `Z`.
    DoorAlongZ,
    /// The same piece turned a quarter turn: walked through along `X`.
    DoorAlongX,
    /// Floor, and where the character starts.
    Spawn,
}

impl Cell {
    /// The piece `glyph` spells.
    ///
    /// `None` for a character [`LAYOUT`] is not written in, which
    /// `every_glyph_in_the_layout_is_a_piece` turns into a failing test rather
    /// than a hole in the zone.
    #[must_use]
    pub const fn of(glyph: char) -> Option<Self> {
        Some(match glyph {
            '#' => Self::Wall,
            '.' => Self::Floor,
            'P' => Self::Pillar,
            'D' => Self::Dais,
            'T' => Self::Brazier,
            '-' => Self::DoorAlongZ,
            '|' => Self::DoorAlongX,
            'S' => Self::Spawn,
            _ => return None,
        })
    }

    /// Whether the character can stand on this tile.
    ///
    /// A doorway is walkable: the piece is a frame with an opening in it, and
    /// the opening is the whole point.
    #[must_use]
    pub const fn is_open(self) -> bool {
        !matches!(self, Self::Wall)
    }

    /// How high the walkable surface of this tile is, in metres.
    #[must_use]
    pub const fn floor_y(self) -> f64 {
        match self {
            Self::Dais => DAIS_HEIGHT,
            _ => 0.0,
        }
    }
}

/// The piece at `(col, row)`.
///
/// # Panics
///
/// If `(col, row)` is off the grid, or if [`LAYOUT`] carries a glyph
/// [`Cell::of`] does not know — both of which are this file being wrong rather
/// than a state a run can be in, and both are covered by
/// `every_glyph_in_the_layout_is_a_piece`.
#[must_use]
pub fn cell(col: usize, row: usize) -> Cell {
    let glyph = LAYOUT[row]
        .chars()
        .nth(col)
        .unwrap_or_else(|| panic!("column {col} is off a {COLS}-wide layout"));
    Cell::of(glyph).unwrap_or_else(|| panic!("'{glyph}' at {col},{row} is not a piece"))
}

/// The centre of tile `(col, row)` on the floor plane, in metres.
///
/// The grid is centred on the origin, so the middle column is `x = 0` and the
/// middle row is `z = 0` — which is what lets the camera open looking down `−Z`
/// at a zone that reaches away from it.
#[must_use]
pub fn tile_centre(col: usize, row: usize) -> DVec3 {
    #[allow(clippy::cast_precision_loss)]
    let across = col as f64 - (COLS - 1) as f64 * 0.5;
    #[allow(clippy::cast_precision_loss)]
    let along = row as f64 - (ROWS - 1) as f64 * 0.5;
    DVec3::new(across * TILE_M, 0.0, along * TILE_M)
}

/// Every tile, in row-major order — the order [`place`] inserts instances in.
///
/// **Insertion order is load-bearing** on `crcbl::render::scene`'s terms: the
/// slot an object lands in is `docs/plan/25-lod.md`'s hysteresis key, so two
/// runs have to place things in the same order. One iterator, walked once by
/// both [`place`] and [`world`], is what makes that true by construction.
pub fn tiles() -> impl Iterator<Item = (usize, usize, Cell)> {
    (0..ROWS).flat_map(|row| (0..COLS).map(move |col| (col, row, cell(col, row))))
}

/// Where the character's **feet** start, in metres.
///
/// The one [`Cell::Spawn`] tile, found rather than written twice.
///
/// # Panics
///
/// If [`LAYOUT`] has no spawn tile on it, which
/// `the_zone_has_exactly_one_spawn_and_it_has_floor_under_it` refuses.
#[must_use]
pub fn spawn() -> DVec3 {
    let (col, row, _) = tiles()
        .find(|(_, _, cell)| *cell == Cell::Spawn)
        .expect("the layout carries a spawn tile");
    tile_centre(col, row)
}

// ---------------------------------------------------------------------------
// The scene description
// ---------------------------------------------------------------------------

/// A floor slab, one tile across — [`SceneDesc::meshes`] slot 0.
pub const SLAB_MESH: usize = 0;
/// A solid wall block, a whole tile from the floor to [`WALL_TOP_Y`].
pub const BLOCK_MESH: usize = 1;
/// A pillar.
pub const PILLAR_MESH: usize = 2;
/// The dais's raised slab.
pub const DAIS_MESH: usize = 3;
/// A brazier's bowl.
pub const BRAZIER_MESH: usize = 4;
/// A doorway: a wall block with a hole through it.
pub const DOOR_MESH: usize = 5;
/// The character, which is a capsule the size of the one the controller sweeps.
pub const FIGURE_MESH: usize = 6;

/// Stone: the wall blocks and the doorways. [`SceneDesc::materials`] slot 0,
/// and therefore what an instance placed without a named material would shade
/// through.
pub const STONE_MATERIAL: usize = 0;
/// The flagged floor, a shade off the walls so a corner reads as a corner.
pub const FLOOR_MATERIAL: usize = 1;
/// The pillars, darker than the walls they stand against.
pub const PILLAR_MATERIAL: usize = 2;
/// The dais, warmer than the floor around it.
pub const DAIS_MATERIAL: usize = 3;
/// Iron: a brazier.
pub const IRON_MATERIAL: usize = 4;
/// The character's cloth, bright enough to be found in a dark room.
pub const FIGURE_MATERIAL: usize = 5;

/// How many pieces the zone places, counted off [`LAYOUT`] rather than written
/// down.
///
/// A wall block is one instance; every other tile is a floor slab and whatever
/// stands on it. The character is the one instance that is not a tile.
#[must_use]
pub fn instance_count() -> usize {
    let mut count = 1; // the character
    for (_, _, cell) in tiles() {
        count += match cell {
            Cell::Wall | Cell::Floor | Cell::Spawn | Cell::Dais => 1,
            // A doorway is the frame, and the floor of the tile it stands in,
            // because a character walks through it.
            Cell::Pillar | Cell::Brazier | Cell::DoorAlongZ | Cell::DoorAlongX => 2,
        };
    }
    count
}

/// What this zone reserves, which is a little over what it places.
///
/// Sized against the description rather than left at [`Capacities::default`],
/// for `apps/lantern/src/room.rs`'s reason: every one of these is device-local
/// memory taken at start-up and never grown, and the default reserves sixteen
/// thousand instances for a zone that places a few hundred.
/// `the_zone_fits_the_pools_it_reserves` asserts each against what is actually
/// built.
const CAPACITIES: Capacities = Capacities {
    vertices: 8 * 1024,
    indices: 16 * 1024,
    meshes: 8,
    instances: 512,
    materials: 8,
    lights: 16,
    // The irradiance volume [`crate::light`] bakes, whose size is that module's
    // `PROBE_COUNTS` rather than a number written twice — `ProbeGrid::check`
    // refuses a table that disagrees with its own volume, and this pool is the
    // only other place the count appears.
    probes: crate::light::PROBE_TOTAL,
};

/// A painted greybox material: the metric grid of
/// [`grid_page`], tinted, and tiled **physically** so
/// one square measures [`GREYBOX_TILE_M`] of surface however large the face is.
///
/// The tint is this zone's own and the grid is the engine's. Physical tiling
/// rather than the authored kind [`grid_material`] comes with, because these
/// surfaces are metres across and an authored `0..1` tile would stretch one
/// square over a whole wall. It spends the 32² grid page rather than
/// `crcbl::greybox::greybox_page`'s 1024² one, because a demo that runs in a
/// browser should not upload eight megatexels to show a ruler.
fn painted(tint: [f32; 3], roughness: f32) -> GpuMaterial {
    GpuMaterial {
        base_color: [tint[0], tint[1], tint[2], 1.0],
        tiling: GpuMaterial::TILING_PHYSICAL,
        tile_metres: GREYBOX_TILE_M,
        roughness,
        ..grid_material()
    }
}

/// How rough the stonework is. High, because a mirror floor is not what a
/// screen-space reflection is being asked to show here — see [`FLOOR_ROUGHNESS`].
const STONE_ROUGHNESS: f32 = 0.85;

/// How rough the flagged floor is.
///
/// **The one surface in the zone that is deliberately smooth enough to reflect
/// something.** `docs/plan/18-render-features.md`'s screen-space reflections
/// need a surface whose roughness lets the march contribute at all, and a zone
/// of uniformly matte stone would run the pass over a frame that could not show
/// its result. The braziers stand on this floor, so what it reflects is the
/// thing that moves.
const FLOOR_ROUGHNESS: f32 = 0.34;

/// How rough iron is.
const IRON_ROUGHNESS: f32 = 0.55;

/// Everything this zone makes resident: seven meshes, six painted rows, the grid
/// page they sample and the irradiance volume [`crate::light`] bakes.
///
/// The mesh and material order is the constants above, in value order; keep them
/// and this assembly in step, which `the_constants_name_their_own_meshes`
/// asserts.
#[must_use]
pub fn scene() -> SceneDesc<'static> {
    let mesh = |label: &'static str, geometry: Geometry<'static>| MeshDesc {
        label: Cow::Borrowed(label),
        geometry,
    };
    let config = crcbl::phys::CharacterConfig::default();
    SceneDesc {
        meshes: vec![
            mesh(
                "slab",
                platform(TILE_M as f32, TILE_M as f32, SLAB_THICKNESS as f32),
            ),
            mesh("wall block", column(TILE_M as f32, WALL_TOP_Y as f32)),
            mesh("pillar", column(PILLAR_EDGE as f32, WALL_TOP_Y as f32)),
            mesh(
                "dais",
                platform(TILE_M as f32, TILE_M as f32, DAIS_HEIGHT as f32),
            ),
            mesh(
                "brazier",
                column(BRAZIER_EDGE as f32, BRAZIER_HEIGHT as f32),
            ),
            mesh(
                "doorway",
                doorway(
                    TILE_M as f32,
                    WALL_TOP_Y as f32,
                    TILE_M as f32,
                    DOOR_OPENING_M as f32,
                    DOOR_HEIGHT_M as f32,
                ),
            ),
            // The same capsule the controller sweeps, so the figure on screen is
            // the shape the physics moved rather than a stand-in beside it.
            mesh(
                "figure",
                crcbl::greybox::capsule(
                    config.radius as f32,
                    (2.0 * (config.radius + config.half_height)) as f32,
                    FIGURE_RINGS,
                    FIGURE_SEGMENTS,
                ),
            ),
        ],
        materials: vec![
            painted([0.31, 0.30, 0.29], STONE_ROUGHNESS),
            painted([0.24, 0.23, 0.22], FLOOR_ROUGHNESS),
            painted([0.20, 0.19, 0.19], STONE_ROUGHNESS),
            painted([0.36, 0.30, 0.24], STONE_ROUGHNESS),
            painted([0.13, 0.13, 0.14], IRON_ROUGHNESS),
            painted([0.62, 0.42, 0.24], STONE_ROUGHNESS),
        ],
        page: grid_page(),
        probes: crate::light::probes(),
        capacities: CAPACITIES,
    }
}

/// Latitude bands per hemisphere on the character's capsule.
const FIGURE_RINGS: u32 = 5;
/// Longitude columns on it. Enough that the silhouette reads as a body from an
/// isometric camera and few enough that the whole zone still fits the vertex
/// pool.
const FIGURE_SEGMENTS: u32 = 12;

/// The character, as the renderer holds it.
///
/// Handed back by [`place`] because it is the only instance in the zone that is
/// ever rewritten: every piece is placed once and drawn for the rest of the run.
#[derive(Debug)]
pub struct Figure {
    handle: InstanceHandle,
}

impl Figure {
    /// Draws the character with its feet at `feet`.
    ///
    /// The capsule's own base is on `y = 0`, so this is a translation and
    /// nothing else — slice 1 has no animation and no facing on the body, which
    /// [`crate::lib`](crate) says where the scope is set.
    pub fn set_feet(&self, renderer: &mut ForwardRenderer, feet: DVec3) {
        #[allow(clippy::cast_possible_truncation)]
        let at = Vec3::new(feet.x as f32, feet.y as f32, feet.z as f32);
        renderer.set_instance(
            self.handle,
            &InstanceDesc {
                mesh: FIGURE_MESH,
                material: FIGURE_MATERIAL,
                transform: Mat4::from_translation(at),
            },
        );
    }
}

/// Places every piece of the zone and hands back the character.
///
/// # Errors
///
/// [`InstancePoolError`] if `CAPACITIES`'s instance count does not cover the
/// zone, which is this file's numbers being wrong rather than a condition a run
/// can be in.
pub fn place(renderer: &mut ForwardRenderer) -> Result<Figure, InstancePoolError> {
    #[allow(clippy::cast_possible_truncation)]
    let at = |p: DVec3| Mat4::from_translation(Vec3::new(p.x as f32, p.y as f32, p.z as f32));
    let quarter = Mat4::from_rotation_y(core::f32::consts::FRAC_PI_2);

    for (col, row, cell) in tiles() {
        let centre = tile_centre(col, row);
        let mut add = |mesh, material, transform| {
            renderer.add_instance(&InstanceDesc {
                mesh,
                material,
                transform,
            })
        };
        if cell == Cell::Wall {
            add(BLOCK_MESH, STONE_MATERIAL, at(centre))?;
            continue;
        }
        // A `platform` rises from `y = 0`, so the floor is dropped by its own
        // thickness to put its top there.
        add(
            SLAB_MESH,
            FLOOR_MATERIAL,
            at(centre - DVec3::Y * SLAB_THICKNESS),
        )?;
        let standing = match cell {
            Cell::Wall | Cell::Floor | Cell::Spawn => None,
            Cell::Pillar => Some((PILLAR_MESH, PILLAR_MATERIAL, at(centre))),
            Cell::Dais => Some((DAIS_MESH, DAIS_MATERIAL, at(centre))),
            Cell::Brazier => Some((BRAZIER_MESH, IRON_MATERIAL, at(centre))),
            Cell::DoorAlongZ => Some((DOOR_MESH, STONE_MATERIAL, at(centre))),
            // The same frame turned a quarter turn about `+Y`, which puts its
            // opening across `X`. One mesh serves both orientations.
            Cell::DoorAlongX => Some((DOOR_MESH, STONE_MATERIAL, at(centre) * quarter)),
        };
        if let Some((mesh, material, transform)) = standing {
            add(mesh, material, transform)?;
        }
    }

    // Last, so the character is the instance a reader finds at the end of the
    // pool rather than somewhere in the middle of the masonry.
    let handle = renderer.add_instance(&InstanceDesc {
        mesh: FIGURE_MESH,
        material: FIGURE_MATERIAL,
        transform: at(spawn()),
    })?;
    Ok(Figure { handle })
}

// ---------------------------------------------------------------------------
// The collision side
// ---------------------------------------------------------------------------

/// The same zone, as the colliders the capsule sweeps against and the probe
/// bake's visibility rays are cast into.
///
/// Every one of them is written from [`LAYOUT`], through the same [`tiles`] the
/// meshes above are placed from — which is the whole reason that iterator
/// exists.
#[must_use]
pub fn world() -> PhysicsWorld {
    let mut world = PhysicsWorld::new();
    // One slab under the whole grid and one over it, rather than one per tile:
    // a floor is a floor, and a hundred coplanar boxes are a hundred sweeps a
    // tick for a surface that has no seams in it.
    let half_span = DVec3::new(
        COLS as f64 * TILE_M * 0.5,
        0.5 * SLAB_THICKNESS,
        ROWS as f64 * TILE_M * 0.5,
    );
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, -0.5 * SLAB_THICKNESS, 0.0),
        half_span,
    ));
    world.add_box(BoxCollider::new(
        DVec3::new(0.0, WALL_TOP_Y + 0.5 * SLAB_THICKNESS, 0.0),
        half_span,
    ));

    for (col, row, cell) in tiles() {
        let centre = tile_centre(col, row);
        let upright = |world: &mut PhysicsWorld, edge: f64, height: f64| {
            world.add_box(BoxCollider::new(
                centre + DVec3::Y * (0.5 * height),
                DVec3::new(0.5 * edge, 0.5 * height, 0.5 * edge),
            ));
        };
        match cell {
            Cell::Wall => upright(&mut world, TILE_M, WALL_TOP_Y),
            Cell::Pillar => upright(&mut world, PILLAR_EDGE, WALL_TOP_Y),
            Cell::Brazier => upright(&mut world, BRAZIER_EDGE, BRAZIER_HEIGHT),
            Cell::Dais => {
                world.add_box(BoxCollider::new(
                    centre + DVec3::Y * (0.5 * DAIS_HEIGHT),
                    DVec3::new(0.5 * TILE_M, 0.5 * DAIS_HEIGHT, 0.5 * TILE_M),
                ));
            }
            Cell::DoorAlongZ => door_colliders(&mut world, centre, false),
            Cell::DoorAlongX => door_colliders(&mut world, centre, true),
            Cell::Floor | Cell::Spawn => {}
        }
    }
    world
}

/// A doorway's three boxes: two posts and the lintel over them.
///
/// **The same three cuboids [`doorway`] builds**, in the same places, which is
/// what makes the opening on screen the opening the capsule fits through. Turned
/// a quarter turn when `across_x` is set, exactly as [`place`] turns the mesh.
fn door_colliders(world: &mut PhysicsWorld, centre: DVec3, across_x: bool) {
    let post = 0.5 * (TILE_M - DOOR_OPENING_M);
    let offset = 0.5 * (DOOR_OPENING_M + post);
    let lintel_h = WALL_TOP_Y - DOOR_HEIGHT_M;
    // Along the wall, and through it. `doorway` builds the frame across `X` with
    // its thickness on `Z`; a quarter turn about `+Y` swaps the two.
    let along = |v: f64| if across_x { DVec3::Z * v } else { DVec3::X * v };
    let extents = |along_wall: f64, height: f64| {
        let through = 0.5 * TILE_M;
        if across_x {
            DVec3::new(through, height, along_wall)
        } else {
            DVec3::new(along_wall, height, through)
        }
    };
    for side in [-1.0, 1.0] {
        world.add_box(BoxCollider::new(
            centre + along(side * offset) + DVec3::Y * (0.5 * WALL_TOP_Y),
            extents(0.5 * post, 0.5 * WALL_TOP_Y),
        ));
    }
    world.add_box(BoxCollider::new(
        centre + DVec3::Y * (DOOR_HEIGHT_M + 0.5 * lintel_h),
        extents(0.5 * DOOR_OPENING_M, 0.5 * lintel_h),
    ));
}

// ---------------------------------------------------------------------------
// The sun that is not one
// ---------------------------------------------------------------------------

/// The sun row, which in here is the ambient floor and almost nothing else.
///
/// [`begin_frame`](crcbl::render::ForwardRenderer::begin_frame) takes a
/// [`DirectionalLight`] whatever the scene is, and it is the light that owns the
/// flat ambient term the irradiance volume is **added to**. A torch-lit zone has
/// no sun whether or not it has a roof, so the directional part is a token —
/// enough to keep the shadow cascades pointing somewhere sane — and the ambient
/// is kept very low
/// deliberately: it is the floor under [`crate::light`]'s volume, and a flat
/// term bright enough to see by would be a room that looks lit whether or not
/// anything lit it.
#[must_use]
pub fn house_light() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::new(0.0, 1.0, 0.0),
        color: Vec3::splat(0.01),
        ambient: Vec3::new(0.012, 0.011, 0.014),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every row is the same width and every glyph is a piece.** A layout with
    /// a short row is a zone with a hole in one wall, and a glyph nothing maps
    /// would panic inside [`cell`] on the first frame rather than here.
    #[test]
    fn every_glyph_in_the_layout_is_a_piece() {
        for (row, line) in LAYOUT.iter().enumerate() {
            assert_eq!(line.chars().count(), COLS, "row {row} is {line:?}");
            for (col, glyph) in line.chars().enumerate() {
                assert!(
                    Cell::of(glyph).is_some(),
                    "'{glyph}' at {col},{row} is not a piece",
                );
            }
        }
        assert_eq!(tiles().count(), COLS * ROWS);
    }

    /// **The zone is sealed**: every tile on its border is stone, so there is no
    /// edge a character can walk off into the space outside the grid — where the
    /// floor slab still reaches but nothing is drawn.
    #[test]
    fn the_border_of_the_zone_is_solid() {
        for row in 0..ROWS {
            for col in 0..COLS {
                let on_border = row == 0 || row == ROWS - 1 || col == 0 || col == COLS - 1;
                if on_border {
                    assert_eq!(
                        cell(col, row),
                        Cell::Wall,
                        "the tile at {col},{row} is on the border and is not stone",
                    );
                }
            }
        }
    }

    /// **One spawn, with floor under it and room to walk.** The browser gate
    /// holds the walk key from here, so the run-up down `−Z` has to be open for
    /// several metres or "the character advances" is a claim about a wall.
    #[test]
    fn the_zone_has_exactly_one_spawn_and_it_has_floor_under_it() {
        let spawns: Vec<_> = tiles()
            .filter(|(_, _, cell)| *cell == Cell::Spawn)
            .collect();
        assert_eq!(spawns.len(), 1, "{spawns:?}");
        let (col, row, _) = spawns[0];

        let mut world = world();
        let config = crcbl::phys::CharacterConfig::default();
        let mut player = crcbl::phys::CharacterController::new(
            config,
            spawn() + DVec3::Y * (config.radius + config.half_height),
        );
        player.move_and_slide(&mut world, DVec3::ZERO);
        assert!(player.is_grounded(), "the spawn has no floor under it");
        let feet = player.position().y - (config.radius + config.half_height);
        assert!(
            feet.abs() < config.skin_width * 3.0,
            "the character settled at {feet} rather than on the floor",
        );

        // And the tiles ahead of it are open, which is the run-up.
        for ahead in 1..=3 {
            assert!(
                cell(col, row - ahead).is_open(),
                "the tile {ahead} north of the spawn is stone",
            );
        }
    }

    /// **The dais is a step the controller climbs**, which is what makes it
    /// vertical variety a character can use. Held to the controller's own number
    /// rather than to [`DAIS_HEIGHT`]'s doc comment.
    #[test]
    fn the_dais_is_a_step_the_controller_climbs() {
        let config = crcbl::phys::CharacterConfig::default();
        assert!(
            DAIS_HEIGHT < config.step_offset,
            "a {DAIS_HEIGHT} m dais is over the {} m the controller steps up",
            config.step_offset,
        );
        const { assert!(DAIS_HEIGHT > 0.0, "a dais of no height is a floor tile") };
        assert_eq!(Cell::Dais.floor_y(), DAIS_HEIGHT);
        assert_eq!(Cell::Floor.floor_y(), 0.0);
    }

    /// **A doorway is walked through and a wall is not**, checked by sweeping
    /// the capsule at each rather than by reading the layout back.
    ///
    /// The wall is the control: a world whose colliders were all missing would
    /// pass the doorway half on its own.
    #[test]
    fn a_doorway_is_an_opening_and_the_wall_beside_it_is_not() {
        let config = crcbl::phys::CharacterConfig::default();
        let lift = DVec3::Y * (config.radius + config.half_height);
        let (col, row, _) = tiles()
            .find(|(_, _, cell)| *cell == Cell::DoorAlongZ)
            .expect("the layout carries a doorway walked through along Z");

        let push = |from: DVec3| {
            let mut world = world();
            let mut walker = crcbl::phys::CharacterController::new(config, from + lift);
            for _ in 0..240 {
                walker.move_and_slide(&mut world, DVec3::new(0.0, -0.02, -0.05));
            }
            walker.position().z - from.z
        };

        // Straight at the opening, from one tile back.
        let through = push(tile_centre(col, row + 1));
        assert!(
            through < -TILE_M,
            "the doorway stopped the character after {through:.2} m",
        );
        // …and straight at the stone next to it.
        let stopped = push(tile_centre(col - 1, row + 1));
        assert!(
            stopped > -TILE_M * 0.5,
            "the wall beside the doorway let the character {stopped:.2} m through",
        );
    }

    /// **Every mesh the description makes resident is placed, and every row it
    /// declares is named**, in the order the constants say.
    ///
    /// A mesh nothing places is memory taken for geometry no frame draws, and a
    /// row nothing names is a colour nobody can see — both of which leave a
    /// perfectly plausible picture.
    #[test]
    fn the_constants_name_their_own_meshes() {
        let scene = scene();
        let labels: Vec<&str> = scene.meshes.iter().map(|m| m.label.as_ref()).collect();
        assert_eq!(
            [
                SLAB_MESH,
                BLOCK_MESH,
                PILLAR_MESH,
                DAIS_MESH,
                BRAZIER_MESH,
                DOOR_MESH,
                FIGURE_MESH,
            ]
            .map(|mesh| labels[mesh]),
            [
                "slab",
                "wall block",
                "pillar",
                "dais",
                "brazier",
                "doorway",
                "figure",
            ],
        );
        assert_eq!(scene.meshes.len(), FIGURE_MESH + 1);
        assert_eq!(
            scene.materials.len(),
            FIGURE_MATERIAL + 1,
            "one painted row per material constant",
        );
        for row in [
            STONE_MATERIAL,
            FLOOR_MATERIAL,
            PILLAR_MATERIAL,
            DAIS_MATERIAL,
            IRON_MATERIAL,
            FIGURE_MATERIAL,
        ] {
            assert_eq!(
                scene.materials[row].tiling,
                GpuMaterial::TILING_PHYSICAL,
                "row {row} must measure its grid in metres, not in its own UV",
            );
        }
        // **One surface is smooth enough for the reflection march to show
        // anything**, which is what stops `RenderEffects::REFLECTIONS` running
        // over a frame that could not report its result.
        assert!(
            scene.materials[FLOOR_MATERIAL].roughness
                < scene.materials[STONE_MATERIAL].roughness - 0.2,
            "the floor is as matte as the walls, so nothing in the zone reflects",
        );
    }

    /// **The zone fits the pools it reserves**, counted off the description and
    /// the layout rather than restated: a piece added to [`LAYOUT`] without a
    /// number raised here is a pool refusal at start-up on every machine.
    #[test]
    fn the_zone_fits_the_pools_it_reserves() {
        let scene = scene();
        assert!(
            scene.meshes.len() <= CAPACITIES.meshes as usize,
            "{} meshes in a pool of {}",
            scene.meshes.len(),
            CAPACITIES.meshes,
        );
        assert!(
            scene.materials.len() <= CAPACITIES.materials as usize,
            "{} material rows in a table of {}",
            scene.materials.len(),
            CAPACITIES.materials,
        );
        let instances = instance_count();
        assert!(
            instances <= CAPACITIES.instances as usize,
            "{instances} instances in a pool of {}",
            CAPACITIES.instances,
        );
        assert!(
            crate::light::torches(0.0, true).len() + 1 < CAPACITIES.lights as usize,
            "the zone's lights do not fit a list of {}",
            CAPACITIES.lights,
        );
        assert_eq!(scene.probes.probes.len() as u32, CAPACITIES.probes);
        assert_eq!(scene.probes.volume.total(), CAPACITIES.probes);
    }

    /// **There is no sun in here.** The directional term is a token and the flat
    /// ambient is a floor rather than the light you see by — see
    /// [`house_light`], and [`crate::light`] for what actually lights the zone.
    #[test]
    fn the_house_light_is_an_ambient_floor_and_not_a_sun() {
        let light = house_light();
        assert!(
            light.color.max_element() < 0.05,
            "a torch-lit zone is lit by a sun of {}",
            light.color,
        );
        assert!(
            light.ambient.max_element() < 0.05,
            "the flat ambient at {} is bright enough to see by on its own",
            light.ambient,
        );
        assert!(
            light.direction.length() > 0.0,
            "the cascades need somewhere to point",
        );
    }
}
