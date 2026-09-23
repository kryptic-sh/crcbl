//! Complete logical input records for shadow-atlas group reuse.

use super::ForwardRenderer;
use crate::cull::Frustum;
use crcbl_shaders::mesh;

impl ForwardRenderer {
    /// Everything one group of the shadow atlas is a function of, as bytes to
    /// compare with the reading the image was last drawn from.
    ///
    /// A **group** is a cascade or a light slot's whole run of tiles — what one
    /// cull covers and what [`crate::shadow::Cadence`] schedules. Per group rather than
    /// per atlas since the cadence rung: a lamp that swings makes its own group's
    /// reading differ and leaves every other group's alone, which is what lets
    /// the frame redraw its tiles and hold the rest.
    ///
    /// # What is in it
    ///
    /// * **The group's view blocks and its cull's frustum**, exactly the values
    ///   [`begin_frame_body`](Self::begin_frame_body) is about to write. Those
    ///   carry the cascade's or the light's matrices, the selection's atlas
    ///   rectangles and this frame's shadow LOD budgets — so a light that moved,
    ///   turned, changed radius or angle, gained or lost a tile, or landed in a
    ///   different rectangle is a different reading, and so is a camera that
    ///   moved, because a cascade is fitted to it.
    /// * **The selection eye and the instance count**, which reach the cull
    ///   beside the frustum and are in no block.
    /// * [`super::InstancePool::revision`], which is every caster: a transform, a mesh,
    ///   a material, a removal or a skinned re-point all pass through the one
    ///   write that moves it. It is conservative — it moves for writes no map
    ///   could show, and for writes only some other group's map could — and it
    ///   is never still while a byte the shadow pass draws from has changed.
    ///
    /// The bytes are the blocks' own `to_bytes` — the encoding the GPU reads —
    /// rather than the objects' memory, so no padding and no `f32` bit pattern
    /// nobody wrote is in the comparison. Two readings that differ by `-0.0`
    /// against `0.0` compare unequal and redraw, which is the safe direction.
    ///
    /// A skinned frame is **not** in here and is not meant to be: a record that
    /// merely carried "this frame skins" would still match the last skinned
    /// frame's and cache. [`ForwardRenderer::frame_skins`] is a separate veto on
    /// the answer for that reason, and it is where that limit is written down.
    ///
    /// # What is deliberately not in it
    ///
    /// * **The mesh pool.** Its buffers are written by `build_geometry` and
    ///   `residents` alone, both of which run inside
    ///   [`with_scene`](Self::with_scene) before a renderer exists — this type
    ///   exposes no way to upload or free a mesh afterwards. The one runtime
    ///   caller of the suballocator is
    ///   [`reserve_skinned`](Self::reserve_skinned), whose bytes are a skinned
    ///   region's, and those are `frame_skins`' always-redraw case.
    /// * **Whether the frame has shadows at all**, which is the atlas's *layout*
    ///   rather than any group's reading: a frame with shadows off gives every
    ///   group no views and no cull, so none of them reaches this function, and
    ///   `begin_frame_body`'s empty layout is what makes such a frame clear the
    ///   image once and cost nothing after.
    /// * **Materials, samplers and the colour pipeline.** `depth_pipeline`
    ///   names no fragment stage, so nothing the atlas holds is a function of
    ///   them.
    pub(super) fn shadow_group_record(
        &self,
        group: usize,
        views: &[(usize, usize, mesh::FrameUniforms)],
        culls: &[(usize, Frustum)],
        eye: [f32; 3],
        instance_count: u32,
    ) -> Vec<u8> {
        let view_count = views.iter().filter(|(owner, _, _)| *owner == group).count();
        let cull_count = culls.iter().filter(|(owner, _)| *owner == group).count();
        let capacity = size_of::<u64>()
            + size_of::<u32>()
            + size_of::<[f32; 3]>()
            + view_count * (size_of::<u32>() + mesh::FRAME_UNIFORMS_SIZE)
            + cull_count * (size_of::<u32>() + crate::cull::PLANE_COUNT * size_of::<[f32; 4]>());
        let mut record = Vec::with_capacity(capacity);
        record.extend_from_slice(&self.instances.revision().to_le_bytes());
        record.extend_from_slice(&instance_count.to_le_bytes());
        for value in eye {
            record.extend_from_slice(&value.to_le_bytes());
        }
        for (_, view, block) in views.iter().filter(|(owner, _, _)| *owner == group) {
            record.extend_from_slice(&(*view as u32).to_le_bytes());
            record.extend_from_slice(&block.to_bytes());
        }
        for (cull, frustum) in culls.iter().filter(|(cull, _)| *cull == group) {
            record.extend_from_slice(&(*cull as u32).to_le_bytes());
            for plane in frustum.planes {
                for value in plane.to_array() {
                    record.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        record
    }
}

// `Instance::create_device` is native-only: see the `crcbl_hal::device` module docs.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::scene;
    use crcbl_hal::null::{NullInstance, Recorder};
    use crcbl_hal::{AdapterId, DeviceDesc, Format, Instance, QueueKind};
    use glam::{Mat4, Vec3, Vec4};

    #[test]
    fn group_records_preserve_complete_bytes_and_selected_input_order() {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let device = instance
            .create_device(&DeviceDesc::for_adapter(AdapterId(0)))
            .unwrap();
        let queue = device.queue(QueueKind::Graphics).unwrap();
        let mut renderer = ForwardRenderer::with_scene(
            device.as_ref(),
            queue,
            Format::Rgba8UnormSrgb,
            &scene::demo(),
        )
        .unwrap();
        let base = mesh::FrameUniforms {
            view_proj: Mat4::IDENTITY.to_cols_array(),
            camera_position: [0.0, 0.0, 2.0, 1.0],
            ambient: [0.1, 0.2, 0.3, 0.0],
            shadow_view_proj: [Mat4::IDENTITY.to_cols_array(); mesh::SHADOW_CASCADES],
            cascade_far: [1.0, 2.0, 3.0, 4.0],
            shadow_params: [0.0; 4],
            cluster_grid: [1, 2, 3, 0],
            light_view_proj: [Mat4::IDENTITY.to_cols_array(); mesh::SHADOW_LIGHT_TILES],
            probes: crcbl_shaders::probe::ProbeVolume::default(),
            lod_params: [0.0; 4],
            fog_params: [0.0; 4],
            fog_color: [0.0; 4],
            sky_sh_r: [0.0; 4],
            sky_sh_g: [0.0; 4],
            sky_sh_b: [0.0; 4],
            previous_view_proj: Mat4::IDENTITY.to_cols_array(),
            vertex_pool: [11, 12, 13, 14],
            shadow_atlas_rect: [[0.0; 4]; mesh::SHADOW_ATLAS_TILES],
            shadow_filter: [0; 4],
        };
        let mut views: Vec<_> = [
            (2, 9),
            (0, 3),
            (2, 7),
            (1, 4),
            (2, 1),
            (2, 8),
            (2, 5),
            (2, 6),
        ]
        .into_iter()
        .map(|(owner, id)| {
            let matrix = Mat4::from_scale(Vec3::splat(id as f32 + 1.0)).to_cols_array();
            let block = mesh::FrameUniforms {
                view_proj: matrix,
                previous_view_proj: matrix,
                ambient: [id as f32, -0.0, owner as f32, 1.0],
                ..base
            };
            (owner, id, block)
        })
        .collect();
        let mut culls: Vec<_> = [2, 0, 1, 2]
            .into_iter()
            .map(|owner| {
                (
                    owner,
                    Frustum {
                        planes: std::array::from_fn(|i| {
                            Vec4::new(owner as f32, i as f32, -0.0, 1.0)
                        }),
                    },
                )
            })
            .collect();
        let eye = [-0.0, 7.0, -9.0];
        let count = u32::MAX;
        let expected = |revision: u64,
                        group: usize,
                        views: &[(usize, usize, mesh::FrameUniforms)],
                        culls: &[(usize, Frustum)]| {
            let mut bytes = Vec::new();
            bytes.extend(revision.to_le_bytes());
            bytes.extend(count.to_le_bytes());
            bytes.extend(eye.into_iter().flat_map(f32::to_le_bytes));
            for (owner, id, block) in views {
                if *owner == group {
                    bytes.extend((*id as u32).to_le_bytes());
                    bytes.extend(block.to_bytes());
                }
            }
            for (owner, frustum) in culls {
                if *owner == group {
                    bytes.extend((*owner as u32).to_le_bytes());
                    bytes.extend(
                        frustum
                            .planes
                            .iter()
                            .flat_map(|v| v.to_array())
                            .flat_map(f32::to_le_bytes),
                    );
                }
            }
            bytes
        };
        for group in [0, 1, 2, usize::MAX] {
            let record = renderer.shadow_group_record(group, &views, &culls, eye, count);
            assert_eq!(
                record,
                expected(renderer.instances.revision(), group, &views, &culls)
            );
            assert_eq!(record.capacity(), record.len());
        }
        let original = renderer.shadow_group_record(2, &views, &culls, eye, count);
        assert_ne!(
            original,
            renderer.shadow_group_record(2, &views, &culls, [0.0, 7.0, -9.0], count)
        );
        assert_ne!(
            original,
            renderer.shadow_group_record(2, &views, &culls, eye, count - 1)
        );
        views[1].2.ambient[0] += 1.0;
        culls[1].1.planes[0].x += 1.0;
        assert_eq!(
            original,
            renderer.shadow_group_record(2, &views, &culls, eye, count)
        );
        views[0].1 += 1;
        assert_ne!(
            original,
            renderer.shadow_group_record(2, &views, &culls, eye, count)
        );
        views[0].1 -= 1;
        views[0].2.ambient[0] += 1.0;
        assert_ne!(
            original,
            renderer.shadow_group_record(2, &views, &culls, eye, count)
        );
        views[0].2.ambient[0] -= 1.0;
        culls[0].1.planes[0].z = 0.0;
        assert_ne!(
            original,
            renderer.shadow_group_record(2, &views, &culls, eye, count)
        );
        culls[0].1.planes[0].z = -0.0;
        views.swap(0, 2);
        assert_ne!(
            original,
            renderer.shadow_group_record(2, &views, &culls, eye, count)
        );
        views.swap(0, 2);
        renderer
            .instances
            .insert(&mesh::GpuInstance::default())
            .unwrap();
        let changed = renderer.shadow_group_record(2, &views, &culls, eye, count);
        assert_ne!(original, changed);
        assert_eq!(&original[size_of::<u64>()..], &changed[size_of::<u64>()..]);
        assert_eq!(
            changed,
            expected(renderer.instances.revision(), 2, &views, &culls)
        );
        renderer.destroy(device.as_ref());
        recorder.assert_valid();
        assert_eq!(recorder.total_live_objects(), 0);
    }
}
