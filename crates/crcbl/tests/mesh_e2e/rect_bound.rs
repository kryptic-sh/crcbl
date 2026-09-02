//! What a rectangle's spherical cluster bound costs, measured rather than
//! argued.
//!
//! ```text
//! CRCBL_GPU=vk crates/crcbl/tests/run-mesh-e2e.sh rect_bound
//! ```
//!
//! # The two claims this exists to settle
//!
//! `light_cluster.slang`'s `KIND_RECT` says a rectangle is "culled as a sphere
//! and nothing more", and argues that a bound tight to the rectangle's own
//! plane would buy only "froxels behind a panel". `docs/backlog.md` carries the
//! opposite reading: that a long thin strip is bounded by a sphere that "fits
//! it loosely and reaches more froxels than it lights", wastefully "in
//! proportion to the aspect ratio". Both were plausible and neither had a
//! number.
//!
//! # What is actually measured, and why those two
//!
//! **Which froxels list the rectangle** comes off the GPU: the clustering pass
//! writes its grid and [`ForwardRenderer::light_grid_buffer`] hands the buffer
//! out for exactly this. That is the direct observation, and it is available —
//! [`froxels`](crate::froxels) reads the list *indirectly* through the
//! volumetric scatter pass because it wants the glow a froxel's list produced,
//! which is a different question from which froxels the list is in.
//!
//! **Which of those froxels the rectangle can light** is decided on the host,
//! and it is a closed form rather than a sample. `mesh.slang` shades a
//! rectangle with `range_window(distance to the centre, position.w)` times a
//! polygon integral that is **one-sided** — `crcbl_shaders::ltc::polygon_irradiance`
//! returns zero for a receiver behind the panel. So the light's support is
//! exactly the sphere the clustering pass tests against, intersected with the
//! half-space the panel faces, and a froxel receives nothing if and only if all
//! eight of its corners are on or behind the rectangle's plane. A froxel is
//! convex, so eight corners settle it — there is no threshold to guess and no
//! sampling to be unlucky with.
//!
//! That is the whole of the over-listing available to a tighter bound. The
//! sphere itself cannot be shrunk: `range_window` reaches zero at the same
//! `position.w` the cull tests, so a bound inside it would darken froxels the
//! shading still lights, which is the seam `crcbl_render::RectLight::radius`
//! documents.
//!
//! # And what it costs
//!
//! [`the_price_of_rectangles_that_light_nothing`] prices the froxels a
//! plane-tight bound would remove, on
//! [`area_light`](crate::area_light)'s machinery and its terms: the same
//! interleaved turns, the same warmup, the same GPU timestamps. A froxel full
//! of rectangles facing *away* from the slab is a list every fragment walks and
//! gets exactly zero from, so what that list costs over the sun-only frame is
//! the ceiling on what the tighter bound could ever save.

use crcbl::hal::{
    Barriers, BufferBarrier, BufferCopy, BufferDesc, BufferUsage, CommandEncoderDesc, Features,
    MemoryLocation, ResourceState, SubmitInfo, depth,
};
use crcbl::math::{Mat4, Vec3, Vec4};
use crcbl::render::{Camera, ForwardRenderer, Grid, Light, Projection, RectLight};
use crcbl::shaders::light::{CLUSTER_LIGHT_CAPACITY, CLUSTER_STRIDE, slice_near};

use crate::area_light::{PRICE_WARMUP, dim_sun, forward_pass_prices, price_frame, slab_scene};
use crate::harness::{Headless, poisoned};
use crate::mesh_scene::{mesh_camera, render_mesh_lit};

/// The frame the froxel counts are taken at.
///
/// Larger than [`MESH_EXTENT`](crate::mesh_scene::MESH_EXTENT), whose grid is
/// a dozen tiles across the whole screen: that is too coarse a ruler for a
/// fraction, and the depth slices do the same work at either extent. The grid
/// this actually produces is [`froxel_grid`]'s assertion rather than a claim
/// here — the froxel budget is what would coarsen it, and a coarsened grid
/// would move both halves of every fraction below at once.
const BOUND_EXTENT: (u32, u32) = (1024, 768);

/// Half the rectangle's area, held fixed across the sweep.
///
/// **The shape varies and the emitter does not.** `RectLight::color` is a
/// radiance leaving the face, so the power a rectangle puts into the room is
/// its radiance times its area — hold the area and every frame in the sweep
/// emits the same light and differs only in the shape it emits it from. The
/// number is the product of `area_light`'s strip half-extents, so the aspect-1
/// rectangle here is that strip's area drawn square.
const RECT_HALF_AREA: f32 = 0.0595;

/// How far past its own half-diagonal a rectangle's influence reaches.
///
/// `crcbl_render::RectLight::radius` tells a caller to put the radius
/// "comfortably past its own half-diagonal or it will fade out before its own
/// edge does", so this is the margin the caller-realistic half of the sweep
/// adds — see [`Reach`].
const RECT_REACH: f32 = 2.0;

/// How high above the slab the rectangle hangs, facing down at it.
const RECT_HEIGHT: f32 = 1.1;

/// The aspect ratios swept, widest last.
///
/// A power-of-two ladder rather than a fine sweep: the claim under test is
/// whether the curve has a slope at all, and a ladder spanning most of two
/// decades is where a slope would be unmistakable. At the widest of these the
/// rectangle's half-diagonal is several times the square's, which is what
/// [`Reach::PastTheDiagonal`] turns into a larger sphere — each row of the
/// table prints the radius it actually got.
const ASPECTS: [f32; 7] = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0];

/// How a rectangle's influence radius is chosen for a sweep.
///
/// **Two readings of the same doc comment, and the backlog's claim lives in the
/// second one.** A caller who picks one radius and reshapes the emitter under
/// it gets [`Reach::Fixed`]; one who follows `RectLight::radius`' advice for
/// each shape gets [`Reach::PastTheDiagonal`], whose sphere grows with the
/// aspect ratio because the half-diagonal does. Reported side by side, because
/// which one a scene does is the difference between the sphere being
/// shape-blind and the sphere being shape-driven.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reach {
    /// One radius for every shape in the sweep, so the sphere is the same
    /// sphere at every aspect ratio.
    Fixed,
    /// Each shape's own half-diagonal plus [`RECT_REACH`], so the sphere grows
    /// as the rectangle lengthens.
    PastTheDiagonal,
}

impl Reach {
    /// What this reading gives a rectangle of half-extents `half`.
    fn radius(self, half: (f32, f32)) -> f32 {
        let diagonal = |(width, height): (f32, f32)| width.hypot(height);
        match self {
            // The aspect-1 rectangle's own radius, held for every shape — so
            // the two readings agree at the start of the sweep and the curves
            // below are comparable at their left edge.
            Self::Fixed => diagonal(half_extents(ASPECTS[0])) + RECT_REACH,
            Self::PastTheDiagonal => diagonal(half) + RECT_REACH,
        }
    }
}

/// The half-extents of the rectangle at `aspect`, at [`RECT_HALF_AREA`].
fn half_extents(aspect: f32) -> (f32, f32) {
    (
        (RECT_HALF_AREA * aspect).sqrt(),
        (RECT_HALF_AREA / aspect).sqrt(),
    )
}

/// The rectangle at `aspect`, hanging over the slab and facing down at it.
///
/// `tangent` is `+X`, so the long axis lies across the slab and the aspect
/// ratio is a real reshaping of the emitter rather than a turn of it.
fn rect(aspect: f32, reach: Reach) -> RectLight {
    let half = half_extents(aspect);
    RectLight {
        position: Vec3::new(0.0, RECT_HEIGHT, 0.0),
        radius: reach.radius(half),
        // The radiance the `area_light` strip leaves its face at, which is
        // above one for that file's reason: this sweep holds the area fixed, so
        // holding the radiance fixed holds the power fixed too.
        color: Vec3::new(22.0, 19.8, 16.9),
        direction: Vec3::NEG_Y,
        tangent: Vec3::X,
        half_width: half.0,
        half_height: half.1,
        fill: false,
    }
}

/// The camera every frame here is drawn from.
fn bound_camera() -> Camera {
    mesh_camera(Projection::default())
}

/// One frame's froxel grid, rebuilt on the host out of the camera the frame
/// drew with.
///
/// **`light_cluster.slang`'s own construction, transcribed** — the tile's pixel
/// rectangle turned into NDC, the four corner rays unprojected at the near
/// plane, and a corner at view depth `d` taken as `eye + ray * (d /
/// eye_to_near)`. Transcribed rather than read back, because the point of
/// [`the_cluster_list_of_a_rectangle_is_its_sphere_and_not_its_shape`] is to
/// compare the pass's decision against a second derivation of it: a model built
/// from the block the pass read would agree with a wrong block.
struct Froxels {
    grid: Grid,
    eye: Vec3,
    inverse: Mat4,
    depth_row: Vec4,
    viewport: (f32, f32),
}

impl Froxels {
    /// The grid `camera` and `extent` produce, under the renderer's own
    /// [`Grid`].
    fn new(camera: &Camera, extent: (u32, u32), grid: Grid) -> Self {
        let aspect = extent.0 as f32 / extent.1 as f32;
        let view_projection = camera.view_projection(aspect);
        Self {
            grid,
            eye: camera.eye,
            inverse: view_projection.inverse(),
            depth_row: view_projection.row(3),
            viewport: (extent.0 as f32, extent.1 as f32),
        }
    }

    /// How many froxels the grid holds.
    fn count(&self) -> usize {
        self.grid.froxels() as usize
    }

    /// A point's view depth — the `w` a perspective divide would use.
    fn view_depth(&self, point: Vec3) -> f32 {
        self.depth_row.dot(point.extend(1.0))
    }

    /// The world-space point at NDC `(x, y)` on the near plane.
    fn unproject(&self, x: f32, y: f32) -> Vec3 {
        let world = self.inverse * Vec4::new(x, y, depth::NEAR, 1.0);
        world.truncate() / world.w
    }

    /// Froxel `index`'s four corner rays, as points on the near plane, and the
    /// view depth they all sit at.
    fn tile_rays(&self, index: usize) -> ([Vec3; 4], f32) {
        let tile_x = index as u32 % self.grid.x;
        let tile_y = (index as u32 / self.grid.x) % self.grid.y;
        let tile = self.grid.tile_pixels as f32;
        let pixel_min = (tile_x as f32 * tile, tile_y as f32 * tile);
        let pixel_max = (pixel_min.0 + tile, pixel_min.1 + tile);
        // Y-up NDC against a pixel row zero at the top of the screen, so the
        // vertical bounds swap on the way in — `light_cluster.slang` says why
        // the flip lives in the viewport rather than in the matrix.
        let ndc_min = (
            pixel_min.0 / self.viewport.0 * 2.0 - 1.0,
            1.0 - pixel_max.1 / self.viewport.1 * 2.0,
        );
        let ndc_max = (
            pixel_max.0 / self.viewport.0 * 2.0 - 1.0,
            1.0 - pixel_min.1 / self.viewport.1 * 2.0,
        );
        let near = [
            self.unproject(ndc_min.0, ndc_min.1),
            self.unproject(ndc_max.0, ndc_min.1),
            self.unproject(ndc_min.0, ndc_max.1),
            self.unproject(ndc_max.0, ndc_max.1),
        ];
        let eye_to_near = self.view_depth(near[0]);
        (near, eye_to_near)
    }

    /// Which depth slice froxel `index` is in.
    fn slice_of(&self, index: usize) -> u32 {
        index as u32 / (self.grid.x * self.grid.y)
    }

    /// The eight world-space corners of froxel `index` — the volume the
    /// fragments shaded out of its list actually occupy.
    ///
    /// The last slice ends where [`slice_near`] puts the one past it — which
    /// at a full-depth grid is `CLUSTER_FAR` — where the clustering pass leaves
    /// it unbounded: the pass has to list a light for the fragments past the
    /// grid's resolution, and this is asking where the froxel *is*.
    /// [`froxels`](crate::froxels)' own `slab` makes the same choice for the
    /// same reason.
    fn corners(&self, index: usize) -> [Vec3; 8] {
        let (near, eye_to_near) = self.tile_rays(index);
        let slice = self.slice_of(index);
        let band_lo = slice_near(slice);
        let band_hi = slice_near(slice + 1);
        core::array::from_fn(|corner| {
            let ray = near[corner % 4] - self.eye;
            let depth = if corner < 4 { band_lo } else { band_hi };
            self.eye + ray * (depth / eye_to_near)
        })
    }

    /// Whether the clustering pass would list a light at `centre` reaching
    /// `radius` in froxel `index`.
    ///
    /// `light_cluster.slang`'s non-directional arm, transcribed step for step:
    /// the froxel cut down to the light's own depth band, the axis-aligned box
    /// of the cut froxel's corners, and the squared distance from the centre to
    /// the nearest point of that box. A rectangle takes this arm and no other,
    /// which is the claim under test.
    fn lists(&self, index: usize, centre: Vec3, radius: f32) -> bool {
        let (near, eye_to_near) = self.tile_rays(index);
        let slice = self.slice_of(index);
        let band_lo = slice_near(slice);
        let band_hi = if slice + 1 >= self.grid.slices {
            // The shader's `DEPTH_UNBOUNDED`, whose only use is a `min` against
            // a finite depth.
            f32::MAX
        } else {
            slice_near(slice + 1)
        };

        let depth = self.view_depth(centre);
        let light_lo = depth - radius;
        let light_hi = depth + radius;
        if light_hi < band_lo || light_lo > band_hi {
            return false;
        }
        // `light_cluster.slang` clamps `cut_lo` to the froxel's own near side a
        // second time — a light straddling or behind the eye must clamp to it
        // rather than reflect through it — which this `max` has already done.
        let cut_lo = band_lo.max(light_lo);
        let cut_hi = band_hi.min(light_hi).max(cut_lo);

        let first = self.eye + (near[0] - self.eye) * (cut_lo / eye_to_near);
        let mut box_lo = first;
        let mut box_hi = first;
        for corner in near {
            let ray = corner - self.eye;
            let at_lo = self.eye + ray * (cut_lo / eye_to_near);
            let at_hi = self.eye + ray * (cut_hi / eye_to_near);
            box_lo = box_lo.min(at_lo.min(at_hi));
            box_hi = box_hi.max(at_lo.max(at_hi));
        }
        let closest = centre.clamp(box_lo, box_hi);
        (centre - closest).length_squared() <= radius * radius
    }
}

/// Which froxels of the frame just drawn carry light row `row` in their list.
///
/// The whole grid copied back and every froxel's kept prefix walked, because
/// the counts below are totals and a total over some froxels is a different
/// total. The copy is its own submission after the frame's: the graph leaves
/// the grid in [`ResourceState::ShaderRead`], which is where the next frame on
/// that slot expects it, so this moves it out and puts it straight back —
/// `forward_e2e/lights.rs` reads the same buffer the same way for its own
/// total.
fn listed_froxels(headless: &Headless, renderer: &ForwardRenderer, row: u32) -> Vec<bool> {
    let device = headless.device.as_ref();
    let grid = renderer.light_grid_buffer(renderer.frame());
    let froxels = renderer.grid().froxels();
    let size = u64::from(froxels * CLUSTER_STRIDE) * 4;
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("rect bound grid readback"),
            size,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("rect bound grid copy"),
        queue: headless.queue,
    });
    let barrier = |from: ResourceState, to: ResourceState| {
        [BufferBarrier {
            buffer: grid,
            from,
            to,
            queue_transfer: None,
        }]
    };
    let out = barrier(ResourceState::ShaderRead, ResourceState::TransferSrc);
    let back = barrier(ResourceState::TransferSrc, ResourceState::ShaderRead);
    encoder.pipeline_barrier(&Barriers {
        buffers: &out,
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&BufferCopy {
        src: grid,
        src_offset: 0,
        dst: staging,
        dst_offset: 0,
        size,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &back,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    let mut bytes = poisoned(size as usize);
    headless.readback(staging, size, &mut bytes);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let word = |froxel: &[u8], slot: usize| {
        u32::from_le_bytes(
            froxel[slot * 4..slot * 4 + 4]
                .try_into()
                .expect("four bytes are inside the record"),
        )
    };
    bytes
        .chunks_exact(CLUSTER_STRIDE as usize * 4)
        .map(|froxel| {
            // The count first, then the indices it kept — clamped exactly as
            // `mesh.slang` clamps it, so a corrupt count cannot walk past the
            // record.
            let kept = word(froxel, 0).min(CLUSTER_LIGHT_CAPACITY) as usize;
            (1..=kept).any(|slot| word(froxel, slot) == row)
        })
        .collect()
}

/// The row a single extra light occupies: the sun is row zero of every frame's
/// list, so the one light `set_lights` was given is row one.
const RECT_ROW: u32 = 1;

/// Draws one frame of the slab under `light` alone and answers which froxels
/// listed it, beside the host's model of the grid it was clustered on.
fn cluster_the_rect(headless: &Headless, light: &RectLight) -> (Vec<bool>, Froxels) {
    let (mut renderer, mut pool) = slab_scene(headless, &[Light::Rect(*light)]);
    let camera = bound_camera();
    let _ = render_mesh_lit(
        headless,
        &mut renderer,
        &mut pool,
        &camera,
        &dim_sun(),
        None,
    );
    let listed = listed_froxels(headless, &renderer, RECT_ROW);
    let froxels = Froxels::new(&camera, BOUND_EXTENT, renderer.grid());

    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    (listed, froxels)
}

/// The grid this fixture's extent produces, asserted rather than assumed.
///
/// Every fraction below is a count over this, so a grid that quietly coarsened
/// — the froxel budget is what would do it — would move both halves and the
/// fractions would go on agreeing with themselves.
fn froxel_grid(grid: Grid) -> u32 {
    assert_eq!(
        (grid.x, grid.y, grid.slices, grid.tile_pixels),
        (16, 12, 24, 64),
        "a {BOUND_EXTENT:?} perspective frame is sixteen tiles by twelve at every slice"
    );
    grid.froxels()
}

/// **A rectangle's cluster list is its sphere, and its shape does not enter
/// it.**
///
/// Two claims in one frame, and the second is what makes the measurement below
/// readable:
///
/// * The pass's decision is the sphere `Light::sphere` names. The host
///   transcription of `light_cluster.slang`'s box test is compared froxel by
///   froxel against the grid the GPU wrote, so a rectangle culled by anything
///   other than that sphere — a plane, a capsule, a box round its own extents —
///   fails here.
/// * That decision does not move when the rectangle is reshaped. The area is
///   held and the aspect ratio runs the whole of [`ASPECTS`]; every frame lists
///   the same froxels, which is `KIND_RECT`'s "culled as a sphere and nothing
///   more" stated as a number.
///
/// **This test is meant to go red when the tighter bound is built**, and that is
/// not a defect in it: `KIND_RECT`'s doc comment is the thing it pins, and a
/// pass that culls a rectangle by its plane needs that comment rewritten in the
/// same change.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_cluster_list_of_a_rectangle_is_its_sphere_and_not_its_shape() {
    let headless = Headless::open_at(BOUND_EXTENT, Features::GPU_DRIVEN);
    let mut first: Option<Vec<bool>> = None;
    let mut disagreed = 0usize;
    let mut listed_total = 0usize;
    for aspect in ASPECTS {
        let light = rect(aspect, Reach::Fixed);
        let (listed, froxels) = cluster_the_rect(&headless, &light);
        let count = froxel_grid(froxels.grid);
        assert_eq!(
            listed.len(),
            count as usize,
            "the grid holds {count} froxels and {} records came back",
            listed.len()
        );

        // The host's own reading of the pass's test, against the sphere the
        // renderer's helper names — `rect.radius`, not the row's `position.w`,
        // so a row builder that wrote a different number is a disagreement
        // rather than a shared mistake.
        let (centre, radius) = Light::Rect(light).sphere();
        let modelled: Vec<bool> = (0..froxels.count())
            .map(|froxel| froxels.lists(froxel, centre, radius))
            .collect();
        let apart = listed
            .iter()
            .zip(&modelled)
            .filter(|(theirs, mine)| theirs != mine)
            .count();
        disagreed += apart;
        listed_total += listed.iter().filter(|held| **held).count();

        match &first {
            None => first = Some(listed),
            Some(before) => {
                // The first differing froxel rather than the two lists, which
                // are one `bool` per froxel and would bury the message.
                let moved = before
                    .iter()
                    .zip(&listed)
                    .position(|(square, wide)| square != wide);
                assert!(
                    moved.is_none(),
                    "at aspect ratio {aspect} froxel {} lists the rectangle where the square one \
                     does not, or the other way about, so the rectangle's shape is reaching the \
                     clustering pass",
                    moved.unwrap_or_default()
                );
            }
        }
    }

    headless.finish();

    let listed = first.expect("the sweep is not empty");
    let held = listed.iter().filter(|froxel| **froxel).count();
    eprintln!(
        "{}: rect bound — {held} of {} froxels list the rectangle at every one of {} aspect \
         ratios ({listed_total} assignment(s) over the sweep), {disagreed} froxel(s) apart from \
         the host's reading of the same test",
        crate::SUITE,
        listed.len(),
        ASPECTS.len(),
    );

    assert!(
        held > 0 && held < listed.len(),
        "{held} of {} froxels list the rectangle, so the bound is either rejecting everything \
         or accepting everything and the sweep above compared two constants",
        listed.len()
    );
    assert_eq!(
        disagreed, 0,
        "the clustering pass and the host's transcription of its own test disagree about \
         {disagreed} (froxel, aspect ratio) pair(s), so either the pass is not culling a \
         rectangle by `Light::sphere`'s sphere or this module's froxel geometry is not the \
         pass's"
    );
}

/// One shape's row of the sweep.
struct Row {
    aspect: f32,
    radius: f32,
    /// Froxels the clustering pass put the rectangle in.
    listed: usize,
    /// Of those, the ones lying wholly on or behind the rectangle's plane,
    /// where its one-sided integral is exactly zero.
    behind: usize,
}

impl Row {
    /// What share of the listed froxels receive nothing at all, in per cent.
    fn wasted(&self) -> f32 {
        self.behind as f32 * 100.0 / self.listed.max(1) as f32
    }
}

/// Sweeps [`ASPECTS`] under one reading of the radius and answers a row each.
fn sweep(headless: &Headless, reach: Reach) -> Vec<Row> {
    ASPECTS
        .iter()
        .map(|aspect| {
            let light = rect(*aspect, reach);
            let (listed, froxels) = cluster_the_rect(headless, &light);
            froxel_grid(froxels.grid);
            // The plane the rectangle emits from, as the row builder
            // orthogonalises it — not `RectLight::direction` raw, because a
            // caller's vector need not be unit and the shading uses the frame.
            let (normal, _) = light.frame();
            let behind = (0..froxels.count())
                .filter(|froxel| listed[*froxel])
                .filter(|froxel| {
                    // A froxel is convex, so it lies wholly in the half-space
                    // its eight corners all lie in. That makes this the exact
                    // set on which the polygon integral is zero, with no
                    // sampling and no threshold in it.
                    froxels
                        .corners(*froxel)
                        .iter()
                        .all(|corner| (*corner - light.position).dot(normal) <= 0.0)
                })
                .count();
            Row {
                aspect: *aspect,
                radius: light.radius,
                listed: listed.iter().filter(|held| **held).count(),
                behind,
            }
        })
        .collect()
}

/// **How much of a rectangle's spherical bound falls behind the rectangle**, as
/// a function of its aspect ratio.
///
/// The measurement this module exists for, printed as a table and asserted only
/// where a number would be meaningless. Both readings of the radius are swept —
/// see [`Reach`] — because the backlog's "fits it loosely" is a claim about the
/// second one and `KIND_RECT`'s "froxels behind a panel" is a claim about what
/// is left after either.
///
/// **The `behind` column is an over-estimate of what could be removed**, and
/// deliberately so: it tests the froxel's own eight corners, where a bound
/// built into the clustering pass would have to test the axis-aligned box of
/// those corners — which contains the froxel and therefore straddles the plane
/// wherever the froxel does, and more often. So a real plane-tight bound
/// removes at most this many froxels and usually fewer, which is the safe
/// direction for a number that is about to decide whether to build one.
///
/// Two things have to hold for the table to be evidence rather than arithmetic:
/// some listed froxels are behind the panel (or the plane-tight bound would
/// remove nothing at all here, and this fixture would be measuring the wrong
/// scene), and some are in front of it (or the rectangle lights nothing and
/// every froxel in the list is waste for a reason that is not the bound's).
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn how_much_of_a_rectangle_s_sphere_falls_behind_the_rectangle() {
    let headless = Headless::open_at(BOUND_EXTENT, Features::GPU_DRIVEN);
    let fixed = sweep(&headless, Reach::Fixed);
    let grown = sweep(&headless, Reach::PastTheDiagonal);
    headless.finish();

    for (reach, rows) in [
        ("fixed radius", &fixed),
        ("radius past the diagonal", &grown),
    ] {
        for row in rows {
            eprintln!(
                "{}: rect bound — {reach}, aspect {:>5.1}, radius {:.3}: {} froxel(s) listed, \
                 {} of them behind the panel ({:.1}%)",
                crate::SUITE,
                row.aspect,
                row.radius,
                row.listed,
                row.behind,
                row.wasted(),
            );
        }
    }

    for rows in [&fixed, &grown] {
        for row in rows {
            assert!(
                row.behind > 0,
                "at aspect ratio {} no listed froxel is behind the panel, so a bound tight to \
                 its plane would remove nothing here and this fixture cannot price one",
                row.aspect
            );
            assert!(
                row.behind < row.listed,
                "at aspect ratio {} every one of the {} listed froxels is behind the panel, so \
                 the rectangle lights nothing in this frame and the share above is not a share \
                 of anything",
                row.aspect,
                row.listed
            );
        }
    }
}

/// How many rectangles the price below puts in a froxel.
///
/// `crcbl_shaders::light::CLUSTER_LIGHT_CAPACITY` for
/// `the_price_of_a_froxel_full_of_area_lights`' reason: every light here
/// reaches every froxel over the slab, so a full list is the worst case the
/// grid allows and one more would be dropped rather than shaded.
const WASTED_LIGHTS: usize = crcbl_shaders::light::CLUSTER_LIGHT_CAPACITY as usize;

/// The ring the priced rectangles hang on, over the slab.
fn priced_ring(index: usize) -> Vec3 {
    let turn = index as f32 * std::f32::consts::TAU / WASTED_LIGHTS as f32;
    Vec3::new(turn.cos() * 0.6, RECT_HEIGHT, turn.sin() * 0.6)
}

/// **What the froxels a plane-tight bound would remove actually cost.**
///
/// The number that decides whether the tighter bound is worth building, and it
/// is measured as an upper bound rather than estimated: a froxel's worth of
/// rectangles facing *straight up*, away from the slab under them. Every froxel
/// over the slab lists every one of them — the sphere is the same sphere
/// whichever way a panel faces, which is
/// [`the_cluster_list_of_a_rectangle_is_its_sphere_and_not_its_shape`]'s claim
/// — and every fragment gets exactly zero from every one of them, because the
/// polygon integral is one-sided. So the whole of what they cost over the
/// sun-only frame is shading that a bound tight to their plane would have
/// skipped, and nothing a tighter bound could save is outside it.
///
/// The same panels turned to face the slab are priced beside them, because the
/// interesting ratio is not the absolute figure — that is a property of this
/// machine — but how the wasted work compares with the useful work. A backend
/// that skipped the integral for a back-facing receiver early would show up
/// here as a wasted cost far under the useful one; `mesh.slang` has no such
/// branch, and this is what says so.
///
/// Prints milliseconds and asserts only the shape, on
/// `the_price_of_a_froxel_full_of_area_lights`' terms exactly: a duration is a
/// property of the machine it was measured on, and an assertion on one is red
/// on every other machine.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh price"]
fn the_price_of_rectangles_that_light_nothing() {
    let (extent, frames) = price_frame();
    let panel = |index: usize, facing: Vec3| {
        Light::Rect(RectLight {
            position: priced_ring(index),
            direction: facing,
            ..rect(1.0, Reach::Fixed)
        })
    };
    let facing: Vec<Light> = (0..WASTED_LIGHTS)
        .map(|index| panel(index, Vec3::NEG_Y))
        .collect();
    let away: Vec<Light> = (0..WASTED_LIGHTS)
        .map(|index| panel(index, Vec3::Y))
        .collect();

    let Some(prices) = forward_pass_prices(&[&[], &facing, &away], extent, frames) else {
        // Drawn, not priced — `forward_pass_prices` says why a backend can be
        // in that position, and the frames still went through the whole
        // forward pass with a full froxel of rectangles in it.
        eprintln!(
            "{}: rect bound — the forward pass drew {WASTED_LIGHTS} back-facing rectangles and \
             this backend reports no TIMESTAMP_QUERY, so what they cost went unmeasured here",
            crate::SUITE,
        );
        return;
    };
    let [none, lighting, dark] = prices[..].try_into().expect("one price per set");
    let ms = |nanos: u64| nanos as f64 / 1.0e6;
    eprintln!(
        "{}: rect bound — forward at {}x{} over {frames} recorded frames ({PRICE_WARMUP} warmup) \
         — sun only {:.3}/{:.3} ms, {WASTED_LIGHTS} rect facing the slab {:.3}/{:.3} ms, the \
         same {WASTED_LIGHTS} facing away {:.3}/{:.3} ms (p50/p95)",
        crate::SUITE,
        extent.0,
        extent.1,
        ms(none.0),
        ms(none.1),
        ms(lighting.0),
        ms(lighting.1),
        ms(dark.0),
        ms(dark.1),
    );

    // Anti-vacuity first: a run whose timestamps came back as zeroes would
    // satisfy everything below without having measured anything.
    assert!(
        none.0 > 0 && lighting.0 > 0 && dark.0 > 0,
        "a forward pass that took no time at all was not measured"
    );
    let useful = lighting.0.saturating_sub(none.0);
    let wasted = dark.0.saturating_sub(none.0);
    eprintln!(
        "{}: rect bound — over the sun-only frame, {WASTED_LIGHTS} rectangles that light the \
         slab cost {useful} ns and {WASTED_LIGHTS} that light nothing cost {wasted} ns, which \
         is the ceiling on what a bound tight to their plane could save",
        crate::SUITE
    );
    assert!(
        wasted > 0,
        "shading {WASTED_LIGHTS} rectangles that contribute exactly zero cost no more than the \
         sun alone, so either the light loop is not reaching them or this run measured nothing"
    );
}
