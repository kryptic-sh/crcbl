//! Finalized draw counts across workgroup boundaries, including hidden buckets.

use super::occlusion_cull::{Readable, read_back};
use crate::harness::Headless;
use crate::mesh_scene::render_mesh_lit;
use crcbl::hal::{GeometryPath, ResourceState};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::{DirectionalLight, ForwardRenderer, InstanceDesc, TransientPool};
use crcbl::screenshot::{OCCLUDERS_CULLING, occluders_camera};
use crcbl::shaders::cull::{
    ENTRY_INDEX_MASK, ENTRY_OCCLUDED, ENTRY_RESCUED, INSTANCE_SURVIVOR_WORD,
};
use crcbl::shaders::draw_gen::{
    DRAW_ARGS_WORDS, EARLY_REGION, LATE_REGION, MESH_ARGS_WORDS, WORKGROUP_SIZE,
};

#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn late_finish_closes_hidden_and_rescued_buckets_beyond_one_workgroup() {
    let headless = Headless::open_at(
        crate::mesh_scene::MESH_EXTENT,
        crcbl::screenshot::OffscreenSetup::OPTIONAL_FEATURES,
    );
    let device = headless.device.as_ref();
    let mut scene = crcbl::render::scene::demo();
    // A partial last workgroup also exercises the shader's bounds guard.
    let buckets = WORKGROUP_SIZE * 2 + 9;
    scene.meshes = vec![scene.meshes[0].clone(); buckets as usize];
    scene.capacities.meshes = buckets;
    scene.capacities.vertices = buckets * crcbl::shaders::mesh::CUBE_VERTEX_COUNT as u32;
    scene.capacities.indices = buckets * crcbl::shaders::mesh::CUBE_INDEX_COUNT as u32;
    scene.capacities.instances = buckets;
    let mut renderer = ForwardRenderer::with_scene_on_path(
        device,
        headless.queue,
        headless.format,
        &scene,
        GeometryPath::IndirectPerBatch,
    )
    .expect("the many-bucket renderer");
    renderer.set_occlusion_culling(OCCLUDERS_CULLING);
    for mesh in 0..buckets as usize {
        if mesh != 0 && mesh % 3 == 0 {
            continue;
        }
        let transform = if mesh == 0 {
            Mat4::from_translation(Vec3::new(0.0, 1.5, 0.0))
                * Mat4::from_scale(Vec3::new(18.0, 3.0, 0.5))
        } else {
            let z = if mesh % 3 == 1 { -3.0 } else { 2.0 };
            Mat4::from_translation(Vec3::new(0.0, 0.8, z)) * Mat4::from_scale(Vec3::splat(0.5))
        };
        renderer
            .add_instance(&InstanceDesc {
                mesh,
                material: 0,
                transform,
            })
            .expect("the instance capacity");
    }
    let mut pool = TransientPool::new();
    let mut hidden = 0;
    let mut rescued = 0;
    for frame in [0, 0, 16, 16, 9] {
        render_mesh_lit(
            &headless,
            &mut renderer,
            &mut pool,
            &occluders_camera(frame),
            &DirectionalLight::default(),
            None,
        );
        let (list, stats) = renderer.camera_cull_buffers(renderer.frame());
        let mut copied = read_back(
            &headless,
            &mut pool,
            &[
                Readable::Buffer {
                    buffer: renderer.draw_args(renderer.frame()),
                    state: ResourceState::IndirectArgument,
                    bytes: u64::from(buckets * (LATE_REGION + 1)) * (DRAW_ARGS_WORDS * 4) as u64,
                },
                Readable::Buffer {
                    buffer: renderer.draws().counts(renderer.frame()),
                    state: ResourceState::IndirectArgument,
                    bytes: u64::from(buckets * (LATE_REGION + 1))
                        * ((1 + MESH_ARGS_WORDS) * 4) as u64,
                },
                Readable::Buffer {
                    buffer: list,
                    state: ResourceState::ShaderRead,
                    bytes: u64::from(buckets) * 4,
                },
                Readable::Buffer {
                    buffer: stats,
                    state: ResourceState::ShaderRead,
                    bytes: 32,
                },
            ],
        )
        .into_iter();
        let args = copied.next().expect("arguments").words();
        let counts = copied.next().expect("draw counts and mesh extents").words();
        let entries = copied.next().expect("entries").words();
        let stats = copied.next().expect("statistics").words();
        let (instances, _) = renderer.cull_records();
        let mut early = vec![0; buckets as usize];
        let mut late = vec![0; buckets as usize];
        for &entry in &entries[..stats[INSTANCE_SURVIVOR_WORD as usize] as usize] {
            let mesh = instances[(entry & ENTRY_INDEX_MASK) as usize].mesh as usize;
            if entry & ENTRY_OCCLUDED == 0 {
                early[mesh] += 1;
            } else if entry & ENTRY_RESCUED != 0 {
                late[mesh] += 1;
                rescued += 1;
            } else {
                hidden += 1;
            }
        }
        for bucket in 0..buckets as usize {
            let count = |region: u32| {
                args[(region as usize * buckets as usize + bucket) * DRAW_ARGS_WORDS + 1]
            };
            assert_eq!(
                count(EARLY_REGION),
                early[bucket],
                "frame {frame}, early bucket {bucket}"
            );
            assert_eq!(
                count(LATE_REGION),
                late[bucket],
                "frame {frame}, late bucket {bucket}"
            );
            for (region, expected) in [
                (0, early[bucket] + late[bucket]),
                (LATE_REGION, late[bucket]),
            ] {
                let base = region as usize * (1 + MESH_ARGS_WORDS) * buckets as usize;
                assert_eq!(
                    counts[base + bucket],
                    u32::from(expected != 0),
                    "frame {frame}, draw count region {region}, bucket {bucket}"
                );
                assert_eq!(
                    counts[base + buckets as usize + bucket * MESH_ARGS_WORDS + 1],
                    expected,
                    "frame {frame}, mesh extent region {region}, bucket {bucket}"
                );
            }
            assert_eq!(
                count(0),
                early[bucket] + late[bucket],
                "frame {frame}, forward bucket {bucket}"
            );
        }
    }
    assert!(hidden > 0, "the fixture must exercise hidden survivors");
    assert!(rescued > 0, "the camera cut must exercise late rescues");
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
}
