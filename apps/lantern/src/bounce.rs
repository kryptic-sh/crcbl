//! The irradiance volume this room hands the engine: where the probes stand,
//! computed from the room's own dimension constants.
//!
//! ```text
//!  crate::room's interior box ──▶ PROBE_COUNTS partition ──▶ ProbeGrid
//!                                                             (rows zeroed,
//!                                                              ProbeUpdate::EveryFrame)
//! ```
//!
//! `docs/plan/18-render-features.md`'s irradiance-probe design asks an
//! application for its own probes, "computed analytically from the room's own
//! dimension constants so that moving a wall moves the probes". This is that
//! placement, and every number in it comes from [`crate::room`].
//!
//! # It places the probes and does not light them
//!
//! Until 2026-09-04 this module also *baked* the sun's first bounce into the
//! rows — an analytic gather against the room's interior box, run once at load.
//! `docs/plan/50-irradiance-probes.md`'s no-bake decision replaced it with the
//! engine's own reflective-shadow-map updater, which fills the same rows every
//! frame from the scene as it actually stands: the lamp moves, the sun moves,
//! and every occluder in the room — the plinth, the mirror panel, the metal
//! block, the corner post — occludes the bounce, none of which the analytic
//! gather could see.
//!
//! So [`probes`] ships [`GpuProbe::ZERO`] rows and
//! [`ProbeUpdate::EveryFrame`], and the rows are the volume's *size* rather
//! than its contents: the table is sized
//! from them and `ProbeGrid::check` holds the two to each other, but what a
//! frame reads is what the gather wrote. A frame drawn with the room's shadows
//! switched off records no updater pass and reads the zeroes, which is the
//! honest picture of "no bounce was computed".

use crcbl::math::Vec3;
use crcbl::render::{ProbeGrid, ProbeUpdate};
use crcbl::shaders::probe::{GpuProbe, ProbeSteps, ProbeVolume};

use crate::room::{HALF_DEPTH, HALF_WIDTH, HEIGHT};

// ---------------------------------------------------------------------------
// The room's interior, as one box
// ---------------------------------------------------------------------------

/// The interior box's minimum corner: the floor, the window wall and the back
/// wall.
const INTERIOR_MIN: Vec3 = Vec3::new(-HALF_WIDTH, 0.0, -HALF_DEPTH);

/// Its maximum corner: the ceiling, the coloured wall and the front wall.
const INTERIOR_MAX: Vec3 = Vec3::new(HALF_WIDTH, HEIGHT, HALF_DEPTH);

// ---------------------------------------------------------------------------
// The grid
// ---------------------------------------------------------------------------

/// How many probes the volume holds on each axis.
///
/// Denser along `z` than across, because the room is: the shaft of light runs
/// the depth of the floor and the coloured wall's lit strip runs with it, so `z`
/// is the axis the bounce actually varies along. Three in `y` puts a probe below
/// eye height, one at it and one above, which is the least that lets the floor's
/// bounce fade towards the ceiling rather than fill the room evenly.
pub const PROBE_COUNTS: [u32; 3] = [4, 3, 5];

/// How many probes that is — what [`crate::room::CAPACITIES`] reserves and what
/// `ProbeGrid::check` holds the table against.
pub const PROBE_TOTAL: u32 = PROBE_COUNTS[0] * PROBE_COUNTS[1] * PROBE_COUNTS[2];

/// How far apart the probes stand on each axis: the interior box's extent
/// divided by the counts.
///
/// **Not the distance between the outermost probes**, which would be the extent
/// over `count - 1` and would put a probe in each wall. This is the cell size of
/// a `PROBE_COUNTS` partition of the room, and [`grid_origin`] places the first
/// probe half a cell in from the corner — so every probe stands in open air.
fn spacing() -> Vec3 {
    #[allow(clippy::cast_precision_loss)]
    let counts = Vec3::new(
        PROBE_COUNTS[0] as f32,
        PROBE_COUNTS[1] as f32,
        PROBE_COUNTS[2] as f32,
    );
    (INTERIOR_MAX - INTERIOR_MIN) / counts
}

/// Where probe `(0, 0, 0)` stands: the centre of the corner cell.
fn grid_origin() -> Vec3 {
    INTERIOR_MIN + 0.5 * spacing()
}

/// Where probe `(x, y, z)` stands, in world space.
///
/// The host-side twin of what the engine derives from the volume header —
/// `crcbl::shaders::probe::ProbeVolume::position` — and
/// `every_probe_stands_in_open_air` is what holds the two to one another. Kept
/// here because the claims below are about *this room*, and reading them through
/// the header would be reading them through the thing they are checking.
#[cfg(test)]
fn probe_position(cell: [u32; 3]) -> Vec3 {
    #[allow(clippy::cast_precision_loss)]
    let steps = Vec3::new(cell[0] as f32, cell[1] as f32, cell[2] as f32);
    grid_origin() + spacing() * steps
}

/// The room's irradiance volume: where the probes stand, and rows for the
/// engine's updater to fill.
///
/// Rows in the `x`-fastest order `ProbeGrid::probes` declares, so the table and
/// the volume's counts address the same probe.
#[must_use]
pub fn probes() -> ProbeGrid {
    ProbeGrid {
        volume: ProbeVolume {
            origin: grid_origin().to_array(),
            // **The reciprocal**, which is what the field is: the shader
            // multiplies by it where it would otherwise divide. Inverted here and
            // nowhere else, and a volume carrying the spacing itself would light
            // the room from the wrong place.
            inv_spacing: (Vec3::ONE / spacing()).to_array(),
            counts: PROBE_COUNTS,
            // **One level.** The clipmap's coarser levels are for a world larger
            // than this room, and the updater covers the sun's near cascade —
            // which already reaches past every wall of it.
            levels: 1,
            steps: ProbeSteps::default(),
        },
        // **Zeroes, and the updater overwrites them on the first frame it
        // records.** See the module docs: the rows are the volume's size here,
        // not its contents.
        probes: vec![GpuProbe::ZERO; PROBE_TOTAL as usize],
        update: ProbeUpdate::EveryFrame,
    }
}

/// How many **whole probe steps** the volume may take from where it was
/// authored, on each axis, without standing a probe inside a wall.
///
/// `docs/plan/50-irradiance-probes.md`'s scrolling is by whole steps of a
/// level's own spacing, so the budget is in steps rather than in metres: the
/// room has [`spacing`] to spare over the level's own extent — half a cell at
/// each end, which is exactly where [`grid_origin`] put the outermost probes —
/// and half a cell is less than one step. **So this room's volume has nowhere to
/// scroll to, and the number is zero.**
///
/// It is computed rather than written down because the computation is the rule:
/// a room made larger, or a level made finer, gives the volume room to move and
/// this starts answering something else. `the_volume_has_nowhere_to_scroll_in_this_room`
/// is what says which answer it gives today, and why.
fn travel() -> Vec3 {
    let step = spacing();
    // Half of what the room has over the level, which is how far either way the
    // level may travel and still stand wholly inside — then floored to whole
    // steps, because a fraction of a step is not a move this volume can make.
    let slack = 0.5 * ((INTERIOR_MAX - INTERIOR_MIN) - grid_extent()).max(Vec3::ZERO);
    (slack / step).floor()
}

/// How far the outermost probes stand from one another on each axis: the
/// spacing times one less than the count, which is a cell short of the room.
fn grid_extent() -> Vec3 {
    #[allow(clippy::cast_precision_loss)]
    let last = Vec3::new(
        (PROBE_COUNTS[0] - 1) as f32,
        (PROBE_COUNTS[1] - 1) as f32,
        (PROBE_COUNTS[2] - 1) as f32,
    );
    spacing() * last
}

/// The point the level is centred on before anything scrolls it — the host-side
/// twin of `ProbeVolume::centre`, on `probe_position`'s terms, and held to it by
/// `the_volume_has_nowhere_to_scroll_in_this_room`.
fn grid_centre() -> Vec3 {
    grid_origin() + 0.5 * grid_extent()
}

/// Where the room's irradiance volume should be centred for an eye at `eye`.
///
/// `docs/plan/50-irradiance-probes.md`'s camera follow, **held inside the
/// room**: the engine re-centres each level on the point it is handed, and a
/// point this room's volume cannot reach without putting a probe in a wall is
/// not one to hand it. `travel` is that budget, and in this room it is zero on
/// every axis — so what this answers is the volume's own centre, whatever the
/// camera does, and `crate::gpu`'s per-frame call is then a comparison and no
/// device work at all.
///
/// **It is a clamp rather than that constant** because the clamp is the rule and
/// the constant is only today's answer to it. A room whose walls moved out, or a
/// volume with a finer level in it, has somewhere to scroll to and this hands
/// the engine the camera instead.
#[must_use]
pub fn follow_point(eye: Vec3) -> Vec3 {
    let centre = grid_centre();
    let reach = travel() * spacing();
    eye.clamp(centre - reach, centre + reach)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every probe stands inside the room**, and the cells partition it
    /// exactly — the one mistake here that would light the room from outside it.
    ///
    /// Half a cell in from each face on every axis, so the outermost probes are
    /// clear of the shell and the volume covers the room rather than a box
    /// inside it.
    ///
    /// **And the volume's header agrees with [`probe_position`]**, which is the
    /// half that used to be `every_probe_reads_back_at_its_own_position`'s: that
    /// test read a row back through `irradiance_at` and cannot exist now that
    /// every row is zero, but what it was really about — an origin at the room's
    /// corner rather than at the first cell's centre, an `inv_spacing` carrying
    /// the spacing instead of its reciprocal — is a claim about the header, and
    /// the header is what the updater places its samples against.
    #[test]
    fn every_probe_stands_in_open_air() {
        let grid = probes();
        assert_eq!(grid.probes.len(), PROBE_TOTAL as usize);
        assert_eq!(grid.volume.total(), PROBE_TOTAL);
        assert_eq!(grid.volume.counts, PROBE_COUNTS);
        for z in 0..PROBE_COUNTS[2] {
            for y in 0..PROBE_COUNTS[1] {
                for x in 0..PROBE_COUNTS[0] {
                    let at = probe_position([x, y, z]);
                    for axis in 0..3 {
                        assert!(
                            at[axis] > INTERIOR_MIN[axis] && at[axis] < INTERIOR_MAX[axis],
                            "probe ({x}, {y}, {z}) is at {at:?}, which is not inside the room"
                        );
                    }
                    assert_eq!(
                        grid.volume.position(0, [x, y, z]),
                        at.to_array(),
                        "the volume's header puts probe ({x}, {y}, {z}) somewhere else"
                    );
                }
            }
        }
        // The spacing is the room over the counts, so moving a wall moves every
        // probe rather than only the volume's bounds.
        assert_eq!(
            spacing() * Vec3::new(4.0, 3.0, 5.0),
            INTERIOR_MAX - INTERIOR_MIN,
            "the cells must partition the room exactly"
        );
    }

    /// **This room's volume has nowhere to scroll to, and that is arithmetic
    /// rather than a decision.**
    ///
    /// The level's probes span `spacing · (count - 1)` and the interior is
    /// `spacing · count`, so the room has exactly one cell to spare — half of it
    /// at each end, which is where [`grid_origin`] stands the outermost probes.
    /// Half a cell is less than one whole probe step, so the follow can take
    /// none: [`follow_point`] answers the volume's centre for every eye, and the
    /// engine's `follow_probe_volume` then re-captures nothing and re-addresses
    /// nothing.
    ///
    /// **Asserted through the extents rather than as a bare zero**, so a room
    /// that gained a wall's worth of width, or a volume that gained a level,
    /// reds this instead of silently starting to scroll.
    #[test]
    fn the_volume_has_nowhere_to_scroll_in_this_room() {
        let step = spacing();
        let extent = grid_extent();
        let spare = (INTERIOR_MAX - INTERIOR_MIN) - extent;
        for axis in 0..3 {
            assert!(
                spare[axis] < 2.0 * step[axis],
                "axis {axis} has {} to spare over the level's {}, which is a whole probe \
                 step either way — this room can scroll and nothing here says where to",
                spare[axis],
                extent[axis]
            );
        }
        assert_eq!(travel(), Vec3::ZERO);

        // So every eye, inside the room and outside it, asks for the centre —
        // and the centre this module computes is the one the header carries,
        // which is the same pairing `every_probe_stands_in_open_air` holds
        // `probe_position` to.
        let centre = grid_centre();
        assert_eq!(centre.to_array(), probes().volume.centre());
        assert_eq!(centre, Vec3::new(0.0, 0.5 * HEIGHT, 0.0));
        for eye in [
            centre,
            Vec3::new(0.0, 1.6, 3.3),
            Vec3::new(-HALF_WIDTH, 0.1, -HALF_DEPTH),
            Vec3::new(50.0, -20.0, 100.0),
        ] {
            assert_eq!(
                follow_point(eye),
                centre,
                "an eye at {eye:?} moved a volume that covers its whole room"
            );
        }

        // And the follow really is the thing being pinned: a volume handed the
        // camera outright *would* step, which is what makes the clamp load
        // bearing rather than decoration.
        let mut loose = probes().volume;
        assert!(
            !loose.follow(Vec3::new(0.0, 1.6, 3.3).to_array()).is_empty(),
            "the fixed camera is inside the level's own extent, so the clamp above is \
             holding nothing back"
        );
    }

    /// **The rows ship zeroed and the volume asks to be updated**, which is the
    /// pair that makes the room lit at all: a grid of zeroes left at
    /// `ProbeUpdate::Authored` is a room with no bounce and a perfectly
    /// plausible picture.
    #[test]
    fn the_rows_are_the_updater_s_to_fill() {
        let grid = probes();
        assert_eq!(grid.update, ProbeUpdate::EveryFrame);
        assert!(
            grid.probes.iter().all(|probe| *probe == GpuProbe::ZERO),
            "a row authored here is a bake, and the updater overwrites it anyway"
        );
    }
}
