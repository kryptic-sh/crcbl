//! **The occlusion cull, on a device** — `docs/plan/03-gpu-driven-rendering.md`
//! §3.3's two-phase pass, where the only thing that can be wrong with it is a
//! picture.
//!
//! A cull that hid something visible draws a frame with a hole in it, and a
//! golden of one still frame is a poor detector: the first phase's mistakes
//! happen on the frames where the camera or an occluder *moved* — a
//! disocclusion, a camera cut, a fast turn — and the second phase is what has to
//! catch them. So the claims here are about a **path**:
//! [`crcbl::screenshot::occluders_camera`]'s, through `Scene::Occluders`' walls,
//! drawn frame by frame through a renderer that culls and one that does not.
//!
//! # What is asked here, and what is asked without a device
//!
//! **What the test decides** — which texels a box reads, which pyramid, what
//! counts as hidden — is `crcbl_render::cull`'s oracle and its unit tests, which
//! include the farthest-against-nearest pyramid trap. **That the GPU runs that
//! arithmetic** is [`the_gpu_verdicts_match_the_oracle_along_the_path`], against
//! pyramid levels and survivor lists read back off the device. **That the passes
//! are recorded in the order the graph needs** is `crcbl::screenshot`'s pass-list
//! test for `Scene::Occluders`.

use crate::harness::{Headless, poisoned};
use crate::mesh_scene::render_mesh_lit;
use crcbl::hal::{
    BufferCopy, BufferDesc, BufferImageCopy, BufferUsage, Capability, CommandEncoderDesc, Extent3d,
    Features, GeometryPath, ImageAspect, ImageSubresourceLayers, MemoryLocation, ResourceState,
    SubmitInfo,
};
use crcbl::render::cull::{
    DepthPyramid, EarlyInputs, Frustum, Reduction, early_entries, late_entry, visible_instances,
};
use crcbl::render::{
    CullStats, DirectionalLight, ForwardRenderer, ImportedBuffer, ImportedImage, InitialClaim,
    InstanceHandle, OcclusionCulling, RenderGraph, TransientPool,
};
use crcbl::screenshot::{
    OCCLUDERS_CULLING, OCCLUDERS_PATH_FRAMES, occluders_camera, occluders_forward_on_path,
    occluders_walker_desc,
};
use crcbl::shaders::cull::{
    ENTRY_INDEX_MASK, ENTRY_OCCLUDED, ENTRY_RESCUED, INSTANCE_SURVIVOR_WORD,
    OCCLUSION_EARLY_REJECT_WORD, OCCLUSION_LATE_REJECT_WORD,
};
use crcbl::shaders::mesh::GpuInstance;

/// The frame this file renders at: the suite's own, whose width keeps the copy
/// pitch every backend enforces.
const EXTENT: (u32, u32) = crate::mesh_scene::MESH_EXTENT;

/// What every device here is opened with: the screenshot path's own list, so a
/// device with a mesh stage builds the mesh path this file draws through too,
/// and debug markers beside it.
const OPTIONAL: Features =
    crcbl::screenshot::OffscreenSetup::OPTIONAL_FEATURES.union(Features::DEBUG_MARKERS);

/// The share of the frustum's survivors the second phase has to keep hidden
/// over the path, for the cull to be doing the work this scene is built for.
///
/// **Measured at 64.7%** on 2026-09-17 — 3663 of 5664 survivors over the 30
/// frames the ring reported, identically on lavapipe and on an RX 7900 XTX and
/// on all three geometry paths — and the floor sits under it with room for a
/// ring that reports a frame or two fewer.
const HIDDEN_SHARE_FLOOR: f64 = 0.5;

/// A renderer on `path` drawing `Scene::Occluders`, culling as `culling` asks.
fn occluders(
    headless: &Headless,
    path: GeometryPath,
    culling: OcclusionCulling,
) -> (ForwardRenderer, TransientPool, InstanceHandle) {
    let built = occluders_forward_on_path(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
        path,
        culling,
    )
    .unwrap_or_else(|why| panic!("the occluders scene builds on {path:?}: {why}"));
    (*built.scene.renderer, TransientPool::new(), built.walker)
}

/// The geometry paths this device builds, the mesh path first.
fn paths(headless: &Headless) -> Vec<GeometryPath> {
    let features = headless.device.caps().features;
    [
        GeometryPath::MeshShader,
        GeometryPath::IndirectCount,
        GeometryPath::IndirectPerBatch,
    ]
    .into_iter()
    .filter(|path| match path {
        GeometryPath::MeshShader => features.contains(Features::MESH_SHADER),
        GeometryPath::IndirectCount => features.contains(Features::DRAW_INDIRECT_COUNT),
        GeometryPath::IndirectPerBatch => true,
    })
    .collect()
}

/// Draws frame `frame` of the path through `renderer`, the walker moved first.
fn path_frame(
    headless: &Headless,
    renderer: &mut ForwardRenderer,
    pool: &mut TransientPool,
    walker: InstanceHandle,
    frame: usize,
) -> crcbl_golden::Image {
    renderer.set_instance(walker, &occluders_walker_desc(frame));
    render_mesh_lit(
        headless,
        renderer,
        pool,
        &occluders_camera(frame),
        &DirectionalLight::default(),
        None,
    )
}

/// How many pixels of `left` and `right` differ at all — **exactly**, because
/// the two are drawn by one device from commands that differ only in which
/// draws they skipped.
fn differing(left: &crcbl_golden::Image, right: &crcbl_golden::Image) -> usize {
    let (width, height) = EXTENT;
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|&(x, y)| left.pixel(x, y) != right.pixel(x, y))
        .count()
}

/// Releases everything in dependency order, then asks the device what it saw.
fn teardown(headless: Headless, renderers: Vec<(ForwardRenderer, TransientPool)>) {
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    for (renderer, mut pool) in renderers {
        renderer.destroy(device);
        pool.destroy(device);
    }
    headless.finish();
}

/// What the culling renderer reported over a path, frame by frame as the ring
/// delivered it.
#[derive(Default)]
struct PathTally {
    frames: std::collections::BTreeMap<u64, CullStats>,
}

impl PathTally {
    fn note(&mut self, stats: Option<CullStats>) {
        if let Some(stats) = stats {
            self.frames.insert(stats.frame, stats);
        }
    }

    fn survivors(&self) -> u64 {
        self.frames.values().map(|stats| stats.instances).sum()
    }

    fn late_rejects(&self) -> u64 {
        self.frames
            .values()
            .map(|stats| stats.occlusion.late_rejects)
            .sum()
    }

    fn rescued(&self) -> u64 {
        self.frames
            .values()
            .map(|stats| stats.occlusion.rescued())
            .sum()
    }
}

/// **Culling by occlusion draws the frame not culling draws, on every frame of a
/// path that strafes past a wall's end, cuts, turns fast and walks an object out
/// from behind a wall — on every geometry path this device builds.**
///
/// And the cull is doing work while it does: the second phase keeps at least
/// [`HIDDEN_SHARE_FLOOR`] of the frustum's survivors hidden over the path, and
/// the first phase got some wrong that the second rescued — otherwise the
/// identity would be between two renderers doing the same thing, or the path
/// would never have asked the second phase anything.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn occlusion_culling_draws_the_same_frames_along_the_path() {
    let headless = Headless::open_at(EXTENT, OPTIONAL);
    let paths = paths(&headless);
    let mut verdicts = Vec::new();
    for &path in &paths {
        let (mut culled, mut culled_pool, culled_walker) =
            occluders(&headless, path, OCCLUDERS_CULLING);
        let (mut plain, mut plain_pool, plain_walker) =
            occluders(&headless, path, OcclusionCulling::OFF);
        let mut differences = Vec::new();
        let mut tally = PathTally::default();
        for frame in 0..OCCLUDERS_PATH_FRAMES {
            let with = path_frame(
                &headless,
                &mut culled,
                &mut culled_pool,
                culled_walker,
                frame,
            );
            let without = path_frame(&headless, &mut plain, &mut plain_pool, plain_walker, frame);
            differences.push(differing(&with, &without));
            tally.note(culled.cull_stats());
        }
        let device = headless.device.as_ref();
        device.wait_idle().expect("idle");
        for (renderer, mut pool) in [(culled, culled_pool), (plain, plain_pool)] {
            renderer.destroy(device);
            pool.destroy(device);
        }
        let share = tally.late_rejects() as f64 / tally.survivors().max(1) as f64;
        eprintln!(
            "{}: {path:?}: over {} reported frames the frustum kept {} survivors, the second \
             phase kept {} hidden ({:.1}%) and rescued {} the first phase had marked",
            crate::SUITE,
            tally.frames.len(),
            tally.survivors(),
            tally.late_rejects(),
            share * 100.0,
            tally.rescued(),
        );
        for stats in tally.frames.values() {
            eprintln!(
                "{}: {path:?}: frame {} survivors {} marked {} hidden {}",
                crate::SUITE,
                stats.frame,
                stats.instances,
                stats.occlusion.early_rejects,
                stats.occlusion.late_rejects,
            );
        }
        verdicts.push((path, differences, share, tally.rescued()));
    }
    headless.finish();

    for (path, differences, share, rescued) in verdicts {
        let differing_frames: Vec<(usize, usize)> = differences
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, count)| *count != 0)
            .collect();
        assert!(
            differing_frames.is_empty(),
            "{path:?}: these frames of the path, as (frame, pixels), differ between the renderer \
             culling by occlusion and the one that does not — the cull removed something \
             visible: {differing_frames:?}"
        );
        assert!(
            share >= HIDDEN_SHARE_FLOOR,
            "{path:?}: the second phase kept {:.1}% of the survivors hidden, under the \
             {:.0}% floor — the identity above compared two renderers doing nearly the same \
             work",
            share * 100.0,
            HIDDEN_SHARE_FLOOR * 100.0
        );
        assert!(
            rescued > 0,
            "{path:?}: the second phase rescued nothing over a path built to disocclude, so it \
             was never asked a question the first phase got wrong"
        );
    }
}

/// One renderer-owned resource to copy back after a frame.
pub(crate) enum Readable {
    /// A buffer the frame left in `state`, and how many bytes of its front to
    /// read.
    Buffer {
        buffer: crcbl::hal::BufferHandle,
        state: ResourceState,
        bytes: u64,
    },
    /// A `D32Float` image and its view, at `extent`.
    Depth {
        image: crcbl::hal::ImageHandle,
        view: crcbl::hal::ImageViewHandle,
        extent: (u32, u32),
    },
}

/// What a [`Readable`] read back as.
pub(crate) enum ReadBack {
    /// A buffer's bytes, as little-endian words.
    Words(Vec<u32>),
    /// An image's texels, row-major.
    Depth(Vec<f32>),
}

impl ReadBack {
    /// The words of a buffer read back.
    pub(crate) fn words(self) -> Vec<u32> {
        match self {
            Self::Words(words) => words,
            Self::Depth(_) => panic!("a depth image read back as words"),
        }
    }

    /// The texels of a depth image read back.
    pub(crate) fn depth(self) -> Vec<f32> {
        match self {
            Self::Depth(texels) => texels,
            Self::Words(_) => panic!("a buffer read back as depth"),
        }
    }
}

/// Copies every one of `readables` back after the last frame, in one graph
/// whose barriers are computed from what that frame left each in — the graph
/// hands each back in the same state, so the next frame's imports still hold.
///
/// **Depth images need [`Capability::DepthImageCopy`]**; a caller asks first.
pub(crate) fn read_back(
    headless: &Headless,
    pool: &mut TransientPool,
    readables: &[Readable],
) -> Vec<ReadBack> {
    let device = headless.device.as_ref();
    // A row of `width` texels padded to the 256-byte pitch D3D12 and wgpu want.
    let row_texels = |width: u32| width.div_ceil(64) * 64;
    let sizes: Vec<u64> = readables
        .iter()
        .map(|readable| match readable {
            Readable::Buffer { bytes, .. } => *bytes,
            Readable::Depth { extent, .. } => {
                u64::from(row_texels(extent.0)) * u64::from(extent.1) * 4
            }
        })
        .collect();
    let stagings: Vec<crcbl::hal::BufferHandle> = sizes
        .iter()
        .map(|size| {
            device
                .create_buffer(&BufferDesc {
                    label: Some("mesh e2e readback"),
                    size: *size,
                    usage: BufferUsage::TRANSFER_DST,
                    memory: MemoryLocation::HostReadback,
                })
                .expect("a readback buffer")
        })
        .collect();

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("mesh e2e readback"),
        queue: headless.queue,
    });
    {
        let mut graph = RenderGraph::new(headless.queue);
        for (index, (readable, (&staging, &size))) in readables
            .iter()
            .zip(stagings.iter().zip(&sizes))
            .enumerate()
        {
            match *readable {
                Readable::Buffer { buffer, state, .. } => {
                    let id = graph.import_buffer(
                        format!("readback-buffer-{index}"),
                        ImportedBuffer {
                            buffer,
                            initial: state,
                            final_state: state,
                        },
                    );
                    graph
                        .add_copy_pass("read a buffer")
                        .use_buffer(id, ResourceState::TransferSrc)
                        .execute(move |ctx| {
                            let source = ctx.buffer(id);
                            ctx.encoder().copy_buffer_to_buffer(&BufferCopy {
                                src: source,
                                src_offset: 0,
                                dst: staging,
                                dst_offset: 0,
                                size,
                            });
                        });
                }
                Readable::Depth {
                    image,
                    view,
                    extent,
                } => {
                    let id = graph.import_image(
                        format!("readback-depth-{index}"),
                        ImportedImage {
                            image,
                            view,
                            format: crcbl::hal::Format::D32Float,
                            extent,
                            initial: pool
                                .imported_image_use(image)
                                .unwrap_or(ResourceState::Undefined),
                            claim: InitialClaim::Tracked,
                            final_state: ResourceState::ShaderRead,
                        },
                    );
                    let (width, height) = extent;
                    let pitch = row_texels(width);
                    graph
                        .add_copy_pass("read a depth image")
                        .use_image(id, ResourceState::TransferSrc)
                        .execute(move |ctx| {
                            let image = ctx.image(id);
                            ctx.encoder().copy_image_to_buffer(&BufferImageCopy {
                                buffer: staging,
                                buffer_offset: 0,
                                buffer_row_length: pitch,
                                buffer_image_height: height,
                                image,
                                image_subresource: ImageSubresourceLayers {
                                    aspect: ImageAspect::DEPTH,
                                    mip: 0,
                                    base_layer: 0,
                                    layer_count: 1,
                                },
                                image_offset: crcbl::hal::Offset3d::default(),
                                image_extent: Extent3d::d2(width, height),
                            });
                        });
                }
            }
        }
        let compiled = graph.compile(pool).expect("a legal readback");
        compiled
            .execute(device, pool, encoder.as_mut(), None)
            .expect("the readback graph executed");
    }
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device.wait_idle().expect("idle");
    device.destroy_command_buffer(commands);

    let mut results = Vec::with_capacity(readables.len());
    for ((readable, staging), size) in readables.iter().zip(&stagings).zip(&sizes) {
        let mut bytes = poisoned(*size as usize);
        headless.readback(*staging, *size, &mut bytes);
        results.push(match readable {
            Readable::Buffer { .. } => ReadBack::Words(
                bytes
                    .chunks_exact(4)
                    .map(|word| u32::from_le_bytes(word.try_into().expect("four bytes")))
                    .collect(),
            ),
            Readable::Depth { extent, .. } => {
                let (width, height) = (extent.0 as usize, extent.1 as usize);
                let pitch = row_texels(extent.0) as usize * 4;
                let mut texels = Vec::with_capacity(width * height);
                for row in 0..height {
                    let at = row * pitch;
                    texels.extend(
                        bytes[at..at + width * 4]
                            .chunks_exact(4)
                            .map(|texel| f32::from_le_bytes(texel.try_into().expect("four bytes"))),
                    );
                }
                ReadBack::Depth(texels)
            }
        });
        device.destroy_buffer(*staging);
    }
    results
}

/// Copies the last frame's pyramid levels, survivor list and statistics back.
///
/// Returns the levels as depth texels, level 1 first, the survivor entries the
/// statistics count, and the statistics.
fn read_cull_state(
    headless: &Headless,
    renderer: &ForwardRenderer,
    pool: &mut TransientPool,
) -> (Vec<Vec<f32>>, Vec<u32>, [u32; 8]) {
    let (list, stats) = renderer.camera_cull_buffers(renderer.frame());
    // The survivor list is the front of the buffer, one word per instance the
    // scene's pool can hold.
    let list_bytes = u64::from(crcbl::render::scene::demo().capacities.instances) * 4;
    let mut readables = vec![
        Readable::Buffer {
            buffer: list,
            state: ResourceState::ShaderRead,
            bytes: list_bytes,
        },
        Readable::Buffer {
            buffer: stats,
            state: ResourceState::ShaderRead,
            bytes: u64::from(crcbl::shaders::cull::STATS_WORDS) * 4,
        },
    ];
    readables.extend(
        renderer
            .occlusion_pyramid()
            .into_iter()
            .map(|(image, view, extent)| Readable::Depth {
                image,
                view,
                extent,
            }),
    );
    let mut results = read_back(headless, pool, &readables).into_iter();
    let list = results.next().expect("the list").words();
    let stats: [u32; 8] = results
        .next()
        .expect("the statistics")
        .words()
        .try_into()
        .expect("the statistics are eight words");
    let count = stats[INSTANCE_SURVIVOR_WORD as usize] as usize;
    let levels = results.map(ReadBack::depth).collect();
    (levels, list[..count].to_vec(), stats)
}

/// **The GPU's occlusion verdicts are the oracle's, frame by frame along the
/// path.**
///
/// After each frame the pyramid levels and the survivor list are read back.
/// The frame's survivors are held to `crcbl_render::cull::visible_instances`;
/// its first-phase marks to [`early_entries`] against the **previous** frame's
/// levels through the previous frame's matrix; its second-phase rescues to
/// [`late_entry`] against **this** frame's levels; and every level below the
/// first to the CPU reduction of the level above it. The statistics' two
/// occlusion words are held to the marks the list carries.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_gpu_verdicts_match_the_oracle_along_the_path() {
    let headless = Headless::open_at(EXTENT, OPTIONAL);
    let device = headless.device.as_ref();
    if !device.supports(Capability::DepthImageCopy).is_yes() {
        eprintln!(
            "{}: this device cannot copy a depth image out, so the pyramid cannot be read \
             back and the oracle comparison is not run here — the identity test still is",
            crate::SUITE
        );
        headless.finish();
        return;
    }
    let path = device.preferred_geometry_path();
    let (mut renderer, mut pool, walker) = occluders(&headless, path, OCCLUDERS_CULLING);
    let aspect = EXTENT.0 as f32 / EXTENT.1 as f32;
    let hidden_view = 1 << GpuInstance::HIDDEN_VIEWS_SHIFT;

    let mut failures = Vec::new();
    let mut previous: Option<DepthPyramid> = None;
    let mut marked_total = 0;
    let mut rescued_total = 0;
    for frame in 0..OCCLUDERS_PATH_FRAMES {
        let _ = path_frame(&headless, &mut renderer, &mut pool, walker, frame);
        let (levels, entries, stats) = read_cull_state(&headless, &renderer, &mut pool);
        let pyramid = DepthPyramid::from_levels(EXTENT, levels.clone());
        let (instances, meshes) = renderer.cull_records();
        let camera = occluders_camera(frame);
        let view_projection = camera.view_projection(aspect);
        let frustum = Frustum::from_view_projection(view_projection);

        let mut gpu: Vec<u32> = entries
            .iter()
            .map(|entry| entry & ENTRY_INDEX_MASK)
            .collect();
        gpu.sort_unstable();
        let oracle = visible_instances(&frustum, &instances, &meshes, hidden_view);
        if gpu != oracle {
            failures.push(format!(
                "frame {frame}: the GPU kept {} survivors and the oracle {}",
                gpu.len(),
                oracle.len()
            ));
        }

        let marked: Vec<u32> = entries
            .iter()
            .copied()
            .filter(|entry| entry & ENTRY_OCCLUDED != 0)
            .collect();
        marked_total += marked.len();
        rescued_total += marked
            .iter()
            .filter(|entry| **entry & ENTRY_RESCUED != 0)
            .count();
        if let Some(history) = &previous {
            let previous_camera = occluders_camera(frame - 1);
            let inputs = EarlyInputs {
                view_projection,
                previous_view_projection: previous_camera.view_projection(aspect),
                history: Some(history),
                target: EXTENT,
                small_feature_pixels: None,
            };
            let mut expected: Vec<u32> =
                early_entries(&frustum, &instances, &meshes, hidden_view, &inputs)
                    .into_iter()
                    .filter(|entry| entry & ENTRY_OCCLUDED != 0)
                    .collect();
            expected.sort_unstable();
            let mut gpu_marked: Vec<u32> =
                marked.iter().map(|entry| entry & !ENTRY_RESCUED).collect();
            gpu_marked.sort_unstable();
            if gpu_marked != expected {
                failures.push(format!(
                    "frame {frame}: the first phase marked {} and the oracle {} (GPU-only {:?}, \
                     oracle-only {:?})",
                    gpu_marked.len(),
                    expected.len(),
                    gpu_marked
                        .iter()
                        .filter(|entry| !expected.contains(entry))
                        .map(|entry| entry & ENTRY_INDEX_MASK)
                        .take(8)
                        .collect::<Vec<_>>(),
                    expected
                        .iter()
                        .filter(|entry| !gpu_marked.contains(entry))
                        .map(|entry| entry & ENTRY_INDEX_MASK)
                        .take(8)
                        .collect::<Vec<_>>(),
                ));
            }
        } else if !marked.is_empty() {
            failures.push(format!(
                "frame {frame}: the first frame has no history and still marked {}",
                marked.len()
            ));
        }
        for entry in &marked {
            let unmarked = entry & !ENTRY_RESCUED;
            let expected = late_entry(unmarked, &instances, &meshes, view_projection, &pyramid);
            if expected != *entry {
                failures.push(format!(
                    "frame {frame}: instance {} came out of the second phase as {:#x}, the \
                     oracle says {expected:#x}",
                    entry & ENTRY_INDEX_MASK,
                    entry
                ));
            }
        }
        let early_word = stats[OCCLUSION_EARLY_REJECT_WORD as usize] as usize;
        let late_word = stats[OCCLUSION_LATE_REJECT_WORD as usize] as usize;
        let still_hidden = marked
            .iter()
            .filter(|entry| **entry & ENTRY_RESCUED == 0)
            .count();
        if early_word != marked.len() || late_word != still_hidden {
            failures.push(format!(
                "frame {frame}: the statistics say {early_word} marked and {late_word} kept \
                 hidden, the list says {} and {still_hidden}",
                marked.len()
            ));
        }
        for level in 2..=pyramid.levels() {
            let (width, height) = crcbl::render::occlusion_cull::level_extent(EXTENT, level - 1);
            let reduced = DepthPyramid::reduce_level(
                pyramid.level(level - 1),
                (width, height),
                Reduction::Farthest,
            );
            if reduced != pyramid.level(level) {
                failures.push(format!(
                    "frame {frame}: pyramid level {level} is not the farthest reduction of \
                     level {}",
                    level - 1
                ));
            }
        }
        previous = Some(pyramid);
    }
    eprintln!(
        "{}: {path:?}: the path marked {marked_total} survivors in its first phase and \
         rescued {rescued_total} in its second",
        crate::SUITE
    );
    teardown(headless, vec![(renderer, pool)]);
    assert!(
        marked_total > 0 && rescued_total > 0,
        "the path marked {marked_total} and rescued {rescued_total}, so the comparison above \
         held nothing that could have disagreed"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The small-feature threshold [`small_features_drop_draws_and_move_pixels`]
/// measures at, in pixels of a projected box's longer side.
const SMALL_FEATURE_PIXELS: f32 = 12.0;

/// **What dropping small features saves, and what it costs in pixels**, along
/// the occluders' path at [`SMALL_FEATURE_PIXELS`].
///
/// Two renderers culling by occlusion, one of them also dropping every
/// instance whose projected box is under the threshold. Unlike the occlusion
/// cull this changes pixels by design, which is why it is off unless a caller
/// names a threshold: what is asserted is that it did drop instances and that
/// the frames it drew are the plain ones less something, and what is printed is
/// the saving and the pixels it moved.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn small_features_drop_draws_and_move_pixels() {
    let headless = Headless::open_at(EXTENT, OPTIONAL);
    let path = headless.device.preferred_geometry_path();
    let (mut kept, mut kept_pool, kept_walker) = occluders(&headless, path, OCCLUDERS_CULLING);
    let (mut dropped, mut dropped_pool, dropped_walker) = occluders(
        &headless,
        path,
        OcclusionCulling {
            small_feature_pixels: Some(SMALL_FEATURE_PIXELS),
            ..OCCLUDERS_CULLING
        },
    );
    let mut moved = 0;
    let mut kept_tally = PathTally::default();
    let mut dropped_tally = PathTally::default();
    for frame in 0..OCCLUDERS_PATH_FRAMES {
        let with = path_frame(&headless, &mut kept, &mut kept_pool, kept_walker, frame);
        let without = path_frame(
            &headless,
            &mut dropped,
            &mut dropped_pool,
            dropped_walker,
            frame,
        );
        moved += differing(&with, &without);
        kept_tally.note(kept.cull_stats());
        dropped_tally.note(dropped.cull_stats());
    }
    teardown(headless, vec![(kept, kept_pool), (dropped, dropped_pool)]);
    let small: u64 = dropped_tally
        .frames
        .values()
        .map(|stats| stats.occlusion.small_feature_rejects)
        .sum();
    let drawn = |tally: &PathTally| tally.survivors() - tally.late_rejects();
    let pixels = u64::from(EXTENT.0 * EXTENT.1) * OCCLUDERS_PATH_FRAMES as u64;
    eprintln!(
        "{}: {path:?}: at {SMALL_FEATURE_PIXELS} px the path dropped {small} small features; \
         {} instances drawn against {} without, and {moved} of {pixels} pixels moved ({:.3}%)",
        crate::SUITE,
        drawn(&dropped_tally),
        drawn(&kept_tally),
        moved as f64 * 100.0 / pixels as f64,
    );
    assert!(
        small > 0,
        "nothing on the path projects under {SMALL_FEATURE_PIXELS} px, so the threshold was \
         never tested"
    );
    assert!(
        drawn(&dropped_tally) < drawn(&kept_tally),
        "dropping {small} small features drew no fewer instances"
    );
}
