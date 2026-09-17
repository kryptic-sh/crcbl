//! **A point light's faces cull separately, and the atlas does not change.**
//!
//! `docs/plan/45-shadows.md`'s fourth decision gave a point light one cull
//! against the box around its sphere and drew every caster in that box into all
//! six faces, leaving the rasteriser to clip what lay outside each one. The cull
//! now tags each caster with the faces whose side planes its box reaches, and
//! each face draws only its own — `crcbl_render::forward`'s
//! `set_point_face_culls`. The claim is that nothing a shadow map holds moves:
//! the six faces' pyramids tile the box, so a caster a face drops has no texel
//! in that face's tile.
//!
//! # What is asked here
//!
//! * **The atlas, texel for texel**, read back from a renderer culling by face
//!   and one culling by the box, frame by frame while both lights move — and the
//!   two frames' pixels beside it.
//! * **The faces' counts are the oracle's**: each face's instance count, summed
//!   over its buckets out of that face's draw region, is what
//!   `crcbl_render::cull::face_entries` tags — and it is fewer than the box's
//!   survivors on every face, which is the saving.
//!
//! Neither light is in view of anything the camera cannot see, so a map that
//! differed would also differ on screen — the pixel comparison is the second
//! half of the same claim, not a separate one.

use crate::harness::Headless;
use crate::mesh_scene::{place, render_mesh_lit};
use crate::occlusion_cull::{ReadBack, Readable, read_back};
use crcbl::hal::{Capability, Features, ResourceState};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::cull::{face_entries, face_planes};
use crcbl::render::scene::{DEMO_CUBE, DEMO_PYRAMID, DEMO_TINTED, DEMO_UNTINTED};
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, Light, PointLight, Projection, TransientPool,
};
use crcbl::shaders::cull::{ENTRY_FACE_SHIFT, FACE_COUNT, INSTANCE_SURVIVOR_WORD, STATS_WORDS};
use crcbl::shaders::draw_gen::{DRAW_ARGS_SIZE, FACE_REGION_BASE};

/// The frame this file renders at: the suite's own.
const EXTENT: (u32, u32) = crate::mesh_scene::MESH_EXTENT;

/// Frames drawn, each with the lights moved, so every one redraws their maps.
const FRAMES: usize = 6;

/// How far each light reaches.
const REACH: f32 = 6.0;

/// Casters in the ring around each light.
const RING: usize = 16;

/// Where light `index` stands on frame `frame`.
fn light_at(index: usize, frame: usize) -> Vec3 {
    let base = if index == 0 {
        Vec3::new(-4.0, 1.5, 0.0)
    } else {
        Vec3::new(4.0, 2.5, -1.0)
    };
    base + Vec3::new(
        0.15 * frame as f32,
        0.05 * frame as f32,
        -0.1 * frame as f32,
    )
}

/// The two point lights on frame `frame`.
fn lights(frame: usize) -> [PointLight; 2] {
    [0, 1].map(|index| PointLight {
        position: light_at(index, frame),
        radius: REACH,
        color: Vec3::new(1.0, 0.9, 0.8) * 20.0,
        fill: false,
    })
}

/// A floor, and a ring of casters around each light's starting place — at
/// different heights and radii, so they fall into different faces and some into
/// several.
fn lit_floor(headless: &Headless, face_culls: bool) -> (ForwardRenderer, TransientPool) {
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_point_face_culls(face_culls);
    place(
        &mut renderer,
        DEMO_CUBE,
        DEMO_UNTINTED,
        Mat4::from_translation(Vec3::new(0.0, -10.0, 0.0)) * Mat4::from_scale(Vec3::splat(20.0)),
    );
    for light in 0..2 {
        let centre = light_at(light, 0);
        for step in 0..RING {
            let angle = step as f32 / RING as f32 * std::f32::consts::TAU;
            let reach = 1.2 + (step % 4) as f32 * 1.1;
            let height = [0.0, 1.2, 2.6, 3.4][step % 4];
            place(
                &mut renderer,
                DEMO_PYRAMID,
                if step % 2 == 0 {
                    DEMO_TINTED
                } else {
                    DEMO_UNTINTED
                },
                Mat4::from_translation(Vec3::new(
                    centre.x + angle.cos() * reach,
                    height,
                    centre.z + angle.sin() * reach,
                )) * Mat4::from_scale(Vec3::splat(0.4)),
            );
        }
    }
    (renderer, TransientPool::new())
}

/// The camera: above both lights, looking down at the floor between them.
fn overhead() -> Camera {
    Camera {
        eye: Vec3::new(0.0, 14.0, 6.0),
        target: Vec3::new(0.0, 0.0, -0.5),
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// The sun: none, so the lamps are the only shadowed light on screen.
fn dark() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::NEG_Y,
        color: Vec3::ZERO,
        ambient: Vec3::splat(0.03),
    }
}

/// Every draw region's instance counts for one of `renderer`'s shadow culls,
/// out of that cull's argument buffer: `[region][bucket]`.
fn face_counts(words: &[u32], buckets: u32) -> [u64; FACE_COUNT] {
    let stride = DRAW_ARGS_SIZE / 4;
    core::array::from_fn(|face| {
        let region = FACE_REGION_BASE as usize + face;
        (0..buckets as usize)
            .map(|bucket| u64::from(words[(region * buckets as usize + bucket) * stride + 1]))
            .sum()
    })
}

/// **Culling a point light by face writes the atlas culling by its box writes,
/// texel for texel, while both lights move — and draws fewer casters into every
/// face, in the counts the oracle tags.**
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn culling_a_point_light_by_face_leaves_its_maps_unchanged() {
    let headless = Headless::open_at(EXTENT, Features::GPU_DRIVEN | Features::DEBUG_MARKERS);
    let device = headless.device.as_ref();
    let copies_depth = device.supports(Capability::DepthImageCopy).is_yes();
    let (mut by_face, mut by_face_pool) = lit_floor(&headless, true);
    let (mut by_box, mut by_box_pool) = lit_floor(&headless, false);

    let mut failures = Vec::new();
    let mut saved = [0u64; FACE_COUNT];
    let mut kept = 0u64;
    for frame in 0..FRAMES {
        let placed = lights(frame).map(Light::Point);
        by_face.set_lights(&placed);
        by_box.set_lights(&placed);
        let with = render_mesh_lit(
            &headless,
            &mut by_face,
            &mut by_face_pool,
            &overhead(),
            &dark(),
            None,
        );
        let without = render_mesh_lit(
            &headless,
            &mut by_box,
            &mut by_box_pool,
            &overhead(),
            &dark(),
            None,
        );
        let differing = (0..EXTENT.1)
            .flat_map(|y| (0..EXTENT.0).map(move |x| (x, y)))
            .filter(|&(x, y)| with.pixel(x, y) != without.pixel(x, y))
            .count();
        if differing != 0 {
            failures.push(format!(
                "frame {frame}: the two frames differ in {differing} pixels"
            ));
        }

        if copies_depth {
            let (width, height) = crcbl::render::shadow::atlas_extent();
            let atlas = |renderer: &ForwardRenderer, pool: &mut TransientPool| {
                read_back(
                    &headless,
                    pool,
                    &[Readable::Depth {
                        image: renderer.shadow_atlas(),
                        view: renderer.shadow_atlas_view(),
                        extent: (width, height),
                    }],
                )
                .into_iter()
                .next()
                .map(ReadBack::depth)
                .expect("one image")
            };
            let face_atlas = atlas(&by_face, &mut by_face_pool);
            let box_atlas = atlas(&by_box, &mut by_box_pool);
            let written = face_atlas.iter().filter(|texel| **texel > 0.0).count();
            let moved = face_atlas
                .iter()
                .zip(&box_atlas)
                .filter(|(a, b)| a.to_bits() != b.to_bits())
                .count();
            if written == 0 {
                failures.push(format!(
                    "frame {frame}: the atlas holds nothing, so the comparison is empty"
                ));
            }
            if moved != 0 {
                failures.push(format!(
                    "frame {frame}: {moved} of the atlas's texels differ between the face culls \
                     and the box cull ({written} written)"
                ));
            }
        }

        // Each light slot's face counts against the oracle for whichever of the
        // two lights holds it.
        let (instances, meshes) = by_face.cull_records();
        let oracles: Vec<(usize, [u64; FACE_COUNT])> = lights(frame)
            .iter()
            .map(|light| {
                let matrices =
                    core::array::from_fn(|face| crcbl::render::shadow::point_matrix(light, face));
                let entries = face_entries(
                    &crcbl::render::shadow::point_frustum(light),
                    &face_planes(&matrices),
                    &instances,
                    &meshes,
                );
                let counts = core::array::from_fn(|face| {
                    entries
                        .iter()
                        .filter(|entry| *entry & (1 << (ENTRY_FACE_SHIFT as usize + face)) != 0)
                        .count() as u64
                });
                (entries.len(), counts)
            })
            .collect();
        for slot in 0..2 {
            let generator = by_face.shadow_generator(crcbl::render::shadow::CASCADES + slot);
            let buckets = generator.bucket_count();
            let frame_slot = by_face.frame();
            let mut results = read_back(
                &headless,
                &mut by_face_pool,
                &[
                    Readable::Buffer {
                        buffer: generator.args(frame_slot),
                        state: ResourceState::IndirectArgument,
                        bytes: generator.args_region_offset(crcbl::shaders::draw_gen::DRAW_REGIONS),
                    },
                    Readable::Buffer {
                        buffer: generator.visible_count(frame_slot),
                        state: ResourceState::ShaderRead,
                        bytes: u64::from(STATS_WORDS) * 4,
                    },
                ],
            )
            .into_iter();
            let args = results.next().expect("the arguments").words();
            let stats = results.next().expect("the statistics").words();
            let survivors = stats[INSTANCE_SURVIVOR_WORD as usize] as usize;
            let counts = face_counts(&args, buckets);
            match oracles
                .iter()
                .find(|(expected, faces)| *expected == survivors && *faces == counts)
            {
                Some(_) => {
                    kept += survivors as u64;
                    for (face, count) in counts.iter().enumerate() {
                        saved[face] += survivors as u64 - count;
                    }
                }
                None => failures.push(format!(
                    "frame {frame}: light slot {slot} kept {survivors} casters with faces \
                     {counts:?}, and neither light's oracle says so: {oracles:?}"
                )),
            }
        }
    }

    eprintln!(
        "{}: over {FRAMES} frames the two lights' box culls kept {kept} casters, and each face \
         drew that many less {saved:?} (faces +X, -X, +Y, -Y, +Z, -Z){}",
        crate::SUITE,
        if copies_depth {
            ""
        } else {
            " — this device cannot copy a depth image out, so the atlas itself was not compared"
        },
    );
    device.wait_idle().expect("idle");
    for (renderer, mut pool) in [(by_face, by_face_pool), (by_box, by_box_pool)] {
        renderer.destroy(device);
        pool.destroy(device);
    }
    headless.finish();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        kept > 0 && saved.iter().all(|saved| *saved > 0),
        "every face has to drop a caster the box kept, or the face culls saved nothing to \
         compare: {saved:?} of {kept}"
    );
}
