//! `docs/plan/25-lod.md`'s **uniform cut** — one level of the dunes DAG per
//! instance, chosen on the GPU and drawn as ordinary index ranges.
//!
//! This is the half of runtime level selection that every backend runs. The
//! plan gives the two indirect tails "every cluster at one depth, which is
//! exactly a whole-mesh level — drawn as ordinary index ranges. Same hierarchy,
//! same error metric, one decision per instance instead of per cluster", and
//! that decision needs no mesh stage and no amplification stage: `draw_gen`
//! scatters the instance into the bucket for the level it chose, so **the bucket
//! whose `instance_count` came out non-zero is the level**.
//!
//! The per-cluster cut is the other half and it stayed in `vk_e2e/mesh.rs`: it
//! is recorded by the amplification stage into a buffer only a device with
//! `Features::TASK_SHADER` ever writes, and reading it back on a backend with no
//! mesh stage would be reading a buffer nothing filled in. What stayed there
//! with it is the cross-check between the two granularities — that the uniform
//! cut's level is the finest level the per-cluster cut reaches — because half of
//! that comparison only exists on Vulkan.

use crate::harness::{Headless, poisoned};
use crate::mesh_scene::{place, place_cube, render_mesh};
use crcbl::hal::{
    Barriers, BufferBarrier, BufferCopy, BufferDesc, BufferUsage, CommandEncoderDesc, Features,
    GeometryPath, MemoryLocation, ResourceState, SubmitInfo,
};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::{Camera, ForwardRenderer, Projection, TransientPool};
use crcbl::shaders::cluster_dag::ClusterDag;
use crcbl::shaders::dunes::DUNES_EXTENT;

/// The demo scene's dunes patch, **at the origin and with no rotation**.
///
/// The identity is load-bearing rather than a default: the selection rule puts
/// each group's sphere through the instance transform and leaves the camera
/// where it is, which is the same statement as putting the camera through the
/// inverse — and at the identity the two are the same *bits*, which is what lets
/// the host-versus-GPU comparisons below be equalities rather than tolerances.
///
/// Every caller asks [`ForwardRenderer::selects_levels`] first. A device that
/// cannot choose a level of a DAG at all has no bucket to report one from, and
/// `add_instance` has no vocabulary for refusing the patch.
fn place_dunes(renderer: &mut ForwardRenderer) {
    place(
        renderer,
        crcbl::render::scene::DEMO_DUNES,
        crcbl::render::scene::DEMO_UNTINTED,
        Mat4::IDENTITY,
    );
}

/// Where the camera stands for the dunes patch: at its near edge, a little way
/// up, looking along the surface as it recedes.
///
/// `docs/plan/25-lod.md`'s shape for level selection: the patch is centred on
/// the origin in `x` and `z` with its height on `y`, so an eye at negative `z`
/// and a small `y` is a viewer standing at one end of a ground plane whose far
/// edge is tens of times further away than its near one. That ratio is the whole
/// source of the variation — the distance term is what makes the choice move.
fn dunes_camera() -> Camera {
    Camera {
        eye: Vec3::new(0.0, 4.0, -DUNES_EXTENT - 2.0),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// A camera `back` units past the dunes patch's near edge, looking at it.
fn dunes_camera_back(back: f32) -> Camera {
    Camera {
        eye: Vec3::new(0.0, 4.0, -DUNES_EXTENT - back),
        ..dunes_camera()
    }
}

/// Where the camera stands to make a uniform cut pick each of three levels.
///
/// **Distance along `-z` and nothing else**: the same eye height and the same
/// target as [`dunes_camera`], so the only thing that changes between them is
/// the term the metric divides by. Positions rather than levels, because which
/// level each produces is what the test reads back — writing the levels here and
/// the eyes there would be two halves of one claim with nothing holding them
/// together.
///
/// The three are far apart on purpose: a group's sphere is grown to contain
/// everything below it, so the levels are separated by hundreds of units rather
/// than by tens, and an eye a little further back than another selects the same
/// level. `crcbl::shaders::cluster_dag`'s
/// `the_uniform_level_is_the_finest_level_the_per_cluster_cut_draws` sweeps the
/// same rule host-side without needing a camera at all.
const DUNES_RECEDING_CAMERAS: [f32; 3] = [2.0, 200.0, 1000.0];

/// The host's copy of the state `docs/plan/25-lod.md`'s hysteresis makes the GPU
/// carry between frames.
///
/// **The oracle stopped being a function of one camera when hysteresis landed**,
/// and this is what replaces it: a frame's cut depends on every frame before it,
/// so the host rule has to be walked frame for frame beside the device rather
/// than evaluated once at the end. One of these is built per renderer, because
/// the state is the renderer's buffer and a fresh renderer starts from the
/// zeroes `DrawGen` wrote into it.
struct DunesHistory {
    dag: ClusterDag,
    /// Which groups were expanded after the last frame this walked, in
    /// `ClusterDag::level_groups` order.
    expanded: Vec<bool>,
}

impl DunesHistory {
    fn new() -> Self {
        let dag = crcbl::shaders::cluster_dag::dunes_dag();
        let expanded = vec![false; dag.group_count()];
        Self { dag, expanded }
    }

    /// Renders one frame at `camera` and advances the host state by the same
    /// frame.
    ///
    /// The three numbers come back out of the renderer rather than being
    /// re-derived here: the viewport and the field of view are the harness's and
    /// the hold budget is the renderer's constant, and a test that recomputed
    /// either would be comparing two derivations instead of two implementations
    /// of one rule.
    fn step(
        &mut self,
        headless: &Headless,
        renderer: &mut ForwardRenderer,
        pool: &mut TransientPool,
        camera: &Camera,
    ) -> [f32; 3] {
        let _ = render_mesh(headless, renderer, pool, camera, None);
        let [pixels_per_unit, expand, hold] = renderer.lod_params();
        self.expanded = self.dag.expand(
            camera.eye.to_array(),
            pixels_per_unit,
            crcbl::shaders::cluster_select::LodBudgets { expand, hold },
            &self.expanded,
        );
        [pixels_per_unit, expand, hold]
    }

    /// The level the host rule says that frame's uniform cut drew.
    fn level(&self) -> usize {
        self.dag.uniform_level_from(&self.expanded)
    }
}

/// The dunes level this frame's uniform cut selected, read out of the bucket
/// that actually drew.
///
/// **The indirect arguments are the observable**, and there is no second buffer
/// recording an intention: `draw_gen.slang` scatters the instance into the
/// bucket for the level it chose, so the bucket whose `instance_count` came out
/// non-zero *is* the level. A selection that picked a level and then failed to
/// route it would leave every bucket at zero and be reported here rather than
/// drawn as an empty frame.
fn selected_dunes_level(headless: &Headless, renderer: &ForwardRenderer) -> Option<usize> {
    use crcbl::shaders::draw_gen::{DRAW_ARGS_SIZE, DrawIndexedArgs};

    let device = headless.device.as_ref();
    let buckets = renderer.level_buckets(crcbl::render::scene::DEMO_DUNES);
    assert!(
        !buckets.is_empty(),
        "this device selects per cluster, so no bucket is a level"
    );
    let args = renderer.draw_args(renderer.frame());
    let bytes = DRAW_ARGS_SIZE as u64 * u64::from(buckets[buckets.len() - 1] + 1);

    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("draw args readback"),
            size: bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("draw args copy"),
        queue: headless.queue,
    });
    // The graph leaves the argument buffer in `IndirectArgument`, which is where
    // the next frame on this slot expects it — so this moves it out and puts it
    // straight back.
    let barrier = |from: ResourceState, to: ResourceState| {
        [BufferBarrier {
            buffer: args,
            from,
            to,
            queue_transfer: None,
        }]
    };
    let out = barrier(ResourceState::IndirectArgument, ResourceState::TransferSrc);
    let back = barrier(ResourceState::TransferSrc, ResourceState::IndirectArgument);
    encoder.pipeline_barrier(&Barriers {
        buffers: &out,
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&BufferCopy {
        src: args,
        src_offset: 0,
        dst: staging,
        dst_offset: 0,
        size: bytes,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &back,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    let mut words = poisoned(bytes as usize);
    headless.readback(staging, bytes, &mut words);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(staging);

    let mut drew: Vec<usize> = Vec::new();
    for (level, &bucket) in buckets.iter().enumerate() {
        let at = bucket as usize * DRAW_ARGS_SIZE;
        let args = DrawIndexedArgs::from_bytes(
            words[at..at + DRAW_ARGS_SIZE]
                .try_into()
                .expect("one argument structure"),
        );
        if args.instance_count > 0 {
            assert!(
                args.index_count > 0,
                "level {level}'s bucket drew {} instance(s) of no indices at all",
                args.instance_count
            );
            drew.push(level);
        }
    }
    assert!(
        drew.len() <= 1,
        "one instance was scattered into levels {drew:?}, which is not a uniform cut"
    );
    drew.first().copied()
}

/// Opens a device that takes the patch through a **uniform** cut, with the cube
/// and the dunes patch already in the frame.
///
/// The features asked for are `GPU_DRIVEN` and nothing more. On the three
/// backends with no mesh stage that is simply what they have; on Vulkan it is a
/// subtraction, and the assertion below is what says the subtraction landed —
/// without it, an adapter reporting `VK_EXT_mesh_shader` would take the
/// per-cluster path and every bucket here would stay at zero.
fn uniform_scene() -> (Headless, ForwardRenderer, TransientPool) {
    let headless = Headless::open_for_mesh_with(Features::GPU_DRIVEN);
    assert_ne!(
        headless.device.caps().geometry_path(),
        GeometryPath::MeshShader,
        "this suite needs the uniform cut, whose selected level a bucket reports. \
         The mesh-stage features were withheld and this device selected the mesh path \
         anyway"
    );
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    place_cube(&mut renderer);
    assert!(
        renderer.selects_levels(),
        "a device with no mesh stage takes the patch through a uniform cut"
    );
    place_dunes(&mut renderer);
    (headless, renderer, TransientPool::new())
}

/// **The uniform cut picks a coarser level the further away the patch is, and
/// the level it picks is the host rule's own.**
///
/// Two assertions, and the first is what makes the second worth making:
///
/// * **The decision moves with distance.** Three cameras, strictly increasing
///   levels. A uniform cut that always answered zero draws every frame correctly
///   and proves nothing at all, which is why this is three positions and a
///   strict comparison rather than "it drew".
/// * **Each level is the host rule's own**, at the pixels-per-unit and budget
///   the renderer actually wrote into its params block rather than a second
///   derivation of them, walked frame by frame because hysteresis makes a
///   frame's answer depend on every frame before it.
///
/// `vk_e2e/mesh.rs`'s `the_two_geometry_paths_agree_about_how_fine_the_dunes_
/// patch_is` is the other half of the original test: it runs this arm beside a
/// device with an amplification stage and asserts the level chosen here is the
/// finest level that device's per-cluster cut reaches. That comparison needs a
/// mesh stage on one side, so it could not come.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_uniform_cut_gets_coarser_as_the_dunes_patch_recedes() {
    let (headless, mut renderer, mut pool) = uniform_scene();

    let mut history = DunesHistory::new();
    let mut chosen: Vec<usize> = Vec::new();
    for back in DUNES_RECEDING_CAMERAS {
        let camera = dunes_camera_back(back);
        let [pixels_per_unit, budget, hold] =
            history.step(&headless, &mut renderer, &mut pool, &camera);
        let level = selected_dunes_level(&headless, &renderer)
            .unwrap_or_else(|| panic!("no bucket drew the patch from {back} units back"));
        let want = history.level();
        eprintln!(
            "{}: dunes from {back} units back — level {level} at {pixels_per_unit} \
             px/unit, budgets {budget}/{hold} (host rule says {want})",
            crate::SUITE
        );
        assert_eq!(
            level, want,
            "from {back} units back the GPU took level {level} and the host rule takes \
             {want}"
        );
        chosen.push(level);
    }

    // **Strictly increasing**, which is the whole claim: a selection wired to a
    // constant draws every one of these frames correctly.
    assert!(
        chosen.windows(2).all(|pair| pair[0] < pair[1]),
        "the levels selected at {DUNES_RECEDING_CAMERAS:?} units back are {chosen:?}, \
         which does not get coarser with distance"
    );

    renderer.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
}

/// How far either side of the boundary the drifting camera steps, as a fraction
/// of the boundary distance.
///
/// A thousandth: far enough that a camera under one threshold crosses the
/// boundary on every frame, and nowhere near far enough to leave the hysteresis
/// band, which is a fifth of the budget wide. `crcbl::shaders::cluster_dag`'s
/// `a_camera_drifting_across_a_threshold_settles_on_one_level` is the same walk
/// with no device in it.
const DUNES_DRIFT_SWING: f32 = 1.0e-3;

/// How many frames the drift runs for. Even, so the walk ends where it started.
const DUNES_DRIFT_FRAMES: usize = 12;

/// The bracket the boundary is bisected inside, in units past the patch's near
/// edge.
const DUNES_DRIFT_BRACKET: (f32, f32) = (2.0, 1000.0);

/// **A camera drifting across a level boundary settles**, on a real device, and
/// the same camera with the band removed does not.
///
/// `docs/plan/25-lod.md`: "**Hysteresis** on the threshold (switch-up and
/// switch-down differ) kills boundary flicker." The flicker is the observable
/// and this counts it, on the uniform cut where the level a frame selected is
/// something a buffer says outright.
///
/// Three assertions, and the first is what makes the second mean anything:
///
/// * **With the band removed the level flicks on nearly every frame.** The band
///   is removed by making the two budgets equal, which
///   `ForwardRenderer::set_lod_hold_ratio(1.0)` does — the same code path, one
///   number apart. A swing that had quietly stopped crossing the boundary would
///   fail here rather than making the count below look good.
/// * **With it, the level changes at most once.** Once and not never: the state
///   starts collapsed and the first frame over the expand budget is a real
///   switch.
/// * **And a decisive move still switches**, out to the far end of the bracket
///   and back.
///
/// The boundary is bisected against the committed DAG at the pixels-per-unit the
/// renderer actually selected under, rather than written down: it is a property
/// of the artifact and the viewport, and a constant here would be a number to
/// re-derive whenever either moved.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_camera_drifting_across_a_level_boundary_stops_flickering() {
    let dag = crcbl::shaders::cluster_dag::dunes_dag();

    // One frame from a throwaway renderer, only to learn the pixels-per-unit
    // this harness's viewport and field of view produce. The boundary depends on
    // it, and re-deriving it here would be a second derivation to keep in step.
    let (headless, mut renderer, mut pool) = uniform_scene();
    let pixels_per_unit = {
        let _ = render_mesh(
            &headless,
            &mut renderer,
            &mut pool,
            &dunes_camera_back(DUNES_DRIFT_BRACKET.0),
            None,
        );
        let [scale, budget, hold] = renderer.lod_params();
        assert!(
            hold < budget,
            "the renderer ships one threshold ({budget} and {hold}), so there is no band \
             for this test to be about"
        );
        renderer.destroy(headless.device.as_ref());
        scale
    };

    // The distance at which the uniform cut changes level, to a tenth of the
    // swing — so the swing straddles it with room to spare.
    let budget = ForwardRenderer::LOD_ERROR_BUDGET;
    let level_at = |back: f32| {
        dag.uniform_level(
            dunes_camera_back(back).eye.to_array(),
            pixels_per_unit,
            budget,
        )
    };
    let (mut lo, mut hi) = DUNES_DRIFT_BRACKET;
    let near = level_at(lo);
    assert_ne!(
        near,
        level_at(hi),
        "the bracket {DUNES_DRIFT_BRACKET:?} draws one level at {pixels_per_unit} px/unit, \
         so there is no boundary in it to drift across"
    );
    while hi - lo > lo * DUNES_DRIFT_SWING / 10.0 {
        let mid = 0.5 * (lo + hi);
        if level_at(mid) == near {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let at = 0.5 * (lo + hi);
    let swing = at * DUNES_DRIFT_SWING;
    eprintln!(
        "{}: dunes level boundary at {at} units back, swing {swing}",
        crate::SUITE
    );

    // A square wave straddling the boundary, so every step is a crossing.
    let path: Vec<f32> = (0..DUNES_DRIFT_FRAMES)
        .map(|frame| {
            if frame % 2 == 0 {
                at - swing
            } else {
                at + swing
            }
        })
        .collect();

    let mut walk = |hold_ratio: f32, path: &[f32]| -> (Vec<usize>, usize) {
        let mut renderer =
            ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
                .expect("the forward renderer builds");
        place_cube(&mut renderer);
        assert!(renderer.selects_levels());
        place_dunes(&mut renderer);
        renderer.set_lod_hold_ratio(hold_ratio);
        let mut history = DunesHistory::new();
        let mut levels = Vec::with_capacity(path.len());
        for &back in path {
            let camera = dunes_camera_back(back);
            history.step(&headless, &mut renderer, &mut pool, &camera);
            let level = selected_dunes_level(&headless, &renderer)
                .unwrap_or_else(|| panic!("no bucket drew the patch from {back} units back"));
            assert_eq!(
                level,
                history.level(),
                "from {back} units back under a hold ratio of {hold_ratio} the GPU took \
                 level {level} and the host rule takes {}",
                history.level()
            );
            levels.push(level);
        }
        renderer.destroy(headless.device.as_ref());
        let flips = levels.windows(2).filter(|pair| pair[0] != pair[1]).count();
        (levels, flips)
    };

    // **The band removed**, which is the same code with one number changed.
    let (sharp_levels, sharp_flips) = walk(1.0, &path);
    // And the band as the renderer ships it.
    let (held_levels, held_flips) = walk(ForwardRenderer::LOD_HOLD_RATIO, &path);
    eprintln!(
        "{}: dunes drift — {sharp_flips} flip(s) with one threshold {sharp_levels:?}, \
         {held_flips} with two {held_levels:?}",
        crate::SUITE
    );
    assert!(
        sharp_flips >= DUNES_DRIFT_FRAMES - 2,
        "one threshold flipped {sharp_flips} time(s) over {DUNES_DRIFT_FRAMES} frames \
         ({sharp_levels:?}), so the swing is not crossing the boundary and the count \
         below would be low for the wrong reason"
    );
    assert!(
        held_flips <= 1,
        "two thresholds flipped {held_flips} time(s) ({held_levels:?}), which is not \
         settling"
    );

    // And a decisive move still switches, under the band the renderer ships.
    let (decisive, decisive_flips) = walk(
        ForwardRenderer::LOD_HOLD_RATIO,
        &[at, DUNES_DRIFT_BRACKET.1, DUNES_DRIFT_BRACKET.1, at, at],
    );
    eprintln!("{}: dunes decisive move — {decisive:?}", crate::SUITE);
    assert!(
        decisive_flips >= 2,
        "a camera pulled out to {} units and brought back selected {decisive:?}, so \
         hysteresis is a latch rather than a band",
        DUNES_DRIFT_BRACKET.1
    );

    pool.destroy(headless.device.as_ref());
    headless.finish();
}
