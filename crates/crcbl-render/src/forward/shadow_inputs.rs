//! Complete logical input records for shadow-atlas group reuse.

use super::ForwardRenderer;
use crate::cull::{Aabb, Frustum};
use crate::shadow;
use crcbl_shaders::mesh::{self, GpuInstance, GpuMesh};
use glam::{Mat4, Vec3};

/// How much wider than its own world box an instance's footprint is taken to
/// be, relative to the box's size and its distance from the origin, and in
/// world units on top of that.
///
/// **Slack in the safe direction.** `cull.slang` tests the same box against the
/// same planes in `f32`, and a host and a GPU can round one comparison two ways
/// for a box that grazes a plane. A footprint that said "outside" where the cull
/// said "inside" would hold a map the caster is in, so the host's box is the
/// larger one: a caster grazing a light's frustum redraws that light's map when
/// the cull may not have drawn it, never the other way round.
const FOOTPRINT_SLACK: f32 = 1.0e-3;

/// Where one element of the instance array can put depth into a shadow map, as
/// the shadow culls would decide it.
///
/// **The cull's own question, asked on the host and answered conservatively.**
/// `cull.slang` keeps an instance when it is live and its mesh's box, carried by
/// its transform, reaches inside every plane of the cull's frustum — and only a
/// kept instance draws. [`crate::cull::visible_instances`] is the same rule, and
/// this is it with every doubt resolved towards "it could be anywhere".
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum CasterFootprint {
    /// Draws nothing: a slot no instance holds, or one whose
    /// [`GpuInstance::LIVE`] bit is clear, which the cull asks before anything.
    #[default]
    Nowhere,
    /// Draws only where this world box reaches, slack included.
    Within(Aabb),
    /// Could draw into any map: a skinned instance, whose vertices are not its
    /// mesh's and so are bounded by nothing the host holds, or a record whose
    /// mesh or transform gives no finite box.
    Everywhere,
}

impl CasterFootprint {
    /// The footprint of `record`, with its mesh looked up in `meshes`.
    pub(super) fn of(record: &GpuInstance, meshes: &[GpuMesh]) -> Self {
        if record.flags & GpuInstance::LIVE == 0 {
            return Self::Nowhere;
        }
        // The cull keeps a skinned instance whatever its box says — see
        // `crate::cull::visible_instances` — because its box is its source
        // mesh's and its vertices are a compute pass's.
        if record.flags & GpuInstance::BASE_VERTEX_OVERRIDE != 0 {
            return Self::Everywhere;
        }
        // A mesh id with no entry, or an entry naming no mesh, is one the cull
        // rejects; it is taken as everywhere all the same, because the cost of
        // being wrong about an id nothing should name is a redraw.
        let Some(mesh) = meshes
            .get(record.mesh as usize)
            .filter(|mesh| mesh.index_count > 0)
        else {
            return Self::Everywhere;
        };
        let world = Aabb {
            min: Vec3::from_array(mesh.bounds_min),
            max: Vec3::from_array(mesh.bounds_max),
        }
        .transformed(Mat4::from_cols_array(&record.transform));
        let (center, extent) = (world.center(), world.half_extent());
        let slack =
            FOOTPRINT_SLACK * (1.0 + center.abs().max_element() + extent.abs().max_element());
        if !(center.is_finite() && extent.is_finite() && slack.is_finite()) {
            // A NaN would make every plane test false, which reads as "outside
            // every frustum" — the one answer a footprint must never give by
            // accident.
            return Self::Everywhere;
        }
        Self::Within(Aabb {
            min: world.min - Vec3::splat(slack),
            max: world.max + Vec3::splat(slack),
        })
    }

    /// Whether a cull against `frustum` could keep this instance.
    pub(super) fn reaches(&self, frustum: &Frustum) -> bool {
        match self {
            Self::Nowhere => false,
            Self::Within(bounds) => frustum.intersects(bounds),
            Self::Everywhere => true,
        }
    }
}

/// The part of a shadow view's block the depth pass reads, with every other
/// field zeroed.
///
/// **What the atlas is a function of, read off the shaders** rather than off
/// the block's declaration. The atlas is drawn by `depth_pipeline` and
/// `depth_masked_pipeline` alone: `mesh.slang`'s `depthVertexMain`, or its
/// `vertexMain` ahead of `depthMaskedFragmentMain` for a cutout, or
/// `mesh_cluster.slang`'s amplification and mesh stages on the mesh path, and
/// `depthClearVertexMain` for a tile reset. Between them they read
/// `view_proj`, `previous_view_proj`, `camera_position` — the light, for the
/// normal-cone test — `lod_params`, `vertex_pool` and the debug view switch in
/// `ambient.w`, and nothing else of `FrameUniforms`. Every other field is the
/// colour pass's: the cascades' matrices and reaches, the light tiles', the
/// froxel grid, the probes, the fog and the sky. Several of them are fitted to
/// the camera, and carried into a light's record they redrew the light's maps
/// whenever the camera moved.
///
/// A shader change that makes the depth pass read another field has to add it
/// here, or a map drawn from a stale value of it will be held.
fn depth_pass_reads(block: &mesh::FrameUniforms) -> mesh::FrameUniforms {
    mesh::FrameUniforms {
        view_proj: block.view_proj,
        camera_position: block.camera_position,
        ambient: [0.0, 0.0, 0.0, block.ambient[3]],
        shadow_view_proj: [[0.0; 16]; mesh::SHADOW_CASCADES],
        cascade_far: [0.0; 4],
        shadow_params: [0.0; 4],
        cluster_grid: [0; 4],
        light_view_proj: [[0.0; 16]; mesh::SHADOW_LIGHT_TILES],
        probes: crcbl_shaders::probe::ProbeVolume::default(),
        lod_params: block.lod_params,
        fog_params: [0.0; 4],
        fog_color: [0.0; 4],
        sky_sh_r: [0.0; 4],
        sky_sh_g: [0.0; 4],
        sky_sh_b: [0.0; 4],
        previous_view_proj: block.previous_view_proj,
        vertex_pool: block.vertex_pool,
        shadow_atlas_rect: [[0.0; 4]; mesh::SHADOW_ATLAS_TILES],
        shadow_filter: [0; 4],
    }
}

/// Moves `writes[group]` for every group whose cull any of `moved`'s
/// footprints, before or after its write, could have been kept by.
///
/// Against **this frame's** frustum, and that is enough: a group's record
/// carries its frustum too, so a group whose frustum moved since its maps were
/// drawn already differs from them whatever this says. A group that culls
/// nothing this frame — a free light slot, or every group on a frame with
/// shadows off — is moved by any write, which costs nothing it had not already
/// lost.
///
/// A function of the three rather than a method, because the frame that calls
/// it is holding a closure over another of the renderer's fields.
pub(super) fn shadow_groups_reached(
    writes: &mut [u64],
    moved: &[(CasterFootprint, CasterFootprint)],
    culls: &[(usize, Frustum)],
) {
    for (group, count) in writes.iter_mut().enumerate() {
        let frustum = culls
            .iter()
            .find(|(owner, _)| *owner == group)
            .map(|(_, frustum)| frustum);
        let reached = moved.iter().any(|(before, after)| {
            frustum.is_none_or(|frustum| before.reaches(frustum) || after.reaches(frustum))
        });
        if reached {
            *count = count.wrapping_add(1);
        }
    }
}

impl ForwardRenderer {
    /// Takes the instance elements written since the last frame, and leaves in
    /// [`ForwardRenderer::shadow_moved`] each one's footprint **before and
    /// after** the write, for [`shadow_groups_reached`] to test against the
    /// frame's culls once they are fitted.
    ///
    /// Before matters as much as after: a caster that walked out of a light's
    /// cone leaves a shadow in the held map, and a caster removed from the scene
    /// has a dead record now and a live one then. The before is the footprint
    /// this kept in [`ForwardRenderer::shadow_footprints`] when it last ran,
    /// and the after is the one the element's record gives now.
    ///
    /// It also keeps [`ForwardRenderer::shadow_dag_elements`] — the live
    /// elements whose mesh has a DAG, whose cut the selection eye decides —
    /// current.
    pub(super) fn note_caster_writes(&mut self) {
        let mut written = std::mem::take(&mut self.shadow_written);
        written.clear();
        self.shadow_moved.clear();
        self.instances.take_written(&mut written);
        if !written.is_empty() {
            let meshes = self.pool.table_entries();
            for &element in &written {
                let index = element as usize;
                if self.shadow_footprints.len() <= index {
                    self.shadow_footprints
                        .resize(index + 1, CasterFootprint::Nowhere);
                }
                let before = self.shadow_footprints[index];
                let record = self.instances.record(element);
                let after = record.map_or(CasterFootprint::Nowhere, |record| {
                    CasterFootprint::of(&record, &meshes)
                });
                self.shadow_footprints[index] = after;
                self.shadow_moved.push((before, after));
                let selects_by_eye = record.is_some_and(|record| {
                    record.flags & GpuInstance::LIVE != 0 && self.mesh_selects_by_eye(record.mesh)
                });
                if selects_by_eye {
                    self.shadow_dag_elements.insert(element);
                } else {
                    self.shadow_dag_elements.remove(&element);
                }
            }
        }
        self.shadow_written = written;
    }

    /// Whether an instance naming mesh `mesh` has its level chosen from the
    /// selection eye: a mesh with a DAG, whose groups `draw_gen.slang`'s
    /// `select_level` projects from the eye. An id past the table is taken as
    /// one that does.
    fn mesh_selects_by_eye(&self, mesh: u32) -> bool {
        self.draw_tables
            .mesh_levels
            .get(mesh as usize)
            .is_none_or(|levels| levels.group_count > 0)
    }

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
    /// * **The group's view blocks, as the depth pass reads them, and its cull's
    ///   frustum** — the values [`begin_frame_body`](Self::begin_frame_body) is
    ///   about to write, cut down by [`depth_pass_reads`]. They carry the
    ///   cascade's or the light's matrices, the light's position and this
    ///   frame's shadow LOD budgets — so a light that moved, turned, changed
    ///   radius or angle, or gained or lost a tile is a different reading, and so
    ///   is a camera that moved *for a cascade*, because a cascade is fitted to
    ///   it. The atlas rectangles are not in the blocks the depth pass reads;
    ///   where a map lands is the layout, which is compared on its own and
    ///   redraws everything when it moves.
    /// * **The instance count**, which reaches the cull beside the frustum and is
    ///   in no block.
    /// * **The selection eye, where the group's cull reads it.** `draw_gen.slang`
    ///   chooses a DAG mesh's level by projecting its groups' error from the eye,
    ///   so the eye is an input to every group a DAG instance could be kept by:
    ///   always for a cascade, and for a light slot when any live DAG element's
    ///   footprint reaches its frustum. A light whose frustum holds flat meshes
    ///   alone draws the same maps from any eye, and leaving the eye out is what
    ///   lets it hold them while the camera moves. Not quantised: any move of the
    ///   eye can move a group across its budget, and a quantum is a range of eyes
    ///   over which a held map is a cut the cull would not have chosen.
    /// * **[`ForwardRenderer::shadow_group_writes`]**, which moves for every
    ///   instance write whose element's footprint, before or after it, reaches
    ///   this group's frustum — see [`note_caster_writes`](Self::note_caster_writes) and
    ///   [`shadow_groups_reached`].
    ///   A transform, a mesh, a material, a removal or a skinned re-point all
    ///   pass through [`crate::InstancePool::take_written`], and a write whose
    ///   element could be kept by this group's cull neither before nor after it
    ///   draws nothing into this group's maps either way.
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
        let reads_eye = group < shadow::CASCADES
            || culls
                .iter()
                .find(|(owner, _)| *owner == group)
                .is_none_or(|(_, frustum)| {
                    self.shadow_dag_elements
                        .iter()
                        .any(|element| self.shadow_footprints[*element as usize].reaches(frustum))
                });
        let capacity = size_of::<u64>()
            + size_of::<u32>()
            + size_of::<u8>()
            + if reads_eye { size_of::<[f32; 3]>() } else { 0 }
            + view_count * (size_of::<u32>() + mesh::FRAME_UNIFORMS_SIZE)
            + cull_count * (size_of::<u32>() + crate::cull::PLANE_COUNT * size_of::<[f32; 4]>());
        let mut record = Vec::with_capacity(capacity);
        record.extend_from_slice(&self.shadow_group_writes[group].to_le_bytes());
        record.extend_from_slice(&instance_count.to_le_bytes());
        record.push(u8::from(reads_eye));
        if reads_eye {
            for value in eye {
                record.extend_from_slice(&value.to_le_bytes());
            }
        }
        for (_, view, block) in views.iter().filter(|(owner, _, _)| *owner == group) {
            record.extend_from_slice(&(*view as u32).to_le_bytes());
            record.extend_from_slice(&depth_pass_reads(block).to_bytes());
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
    use super::super::SHADOW_CULLS;
    use super::*;
    use crate::scene;
    use crcbl_hal::null::{NullInstance, Recorder};
    use crcbl_hal::{AdapterId, DeviceDesc, Format, Instance, QueueKind};
    use glam::{Vec3, Vec4};

    /// A light group's index: the first light slot, past the cascades.
    const LIGHT: usize = shadow::CASCADES;

    /// A block whose every field holds something, so a field the record
    /// dropped and a field it kept are told apart by changing it.
    fn filled_block(id: usize, owner: usize) -> mesh::FrameUniforms {
        let matrix = Mat4::from_scale(Vec3::splat(id as f32 + 1.0)).to_cols_array();
        mesh::FrameUniforms {
            view_proj: matrix,
            camera_position: [0.0, 0.0, 2.0, 1.0],
            ambient: [id as f32, -0.0, owner as f32, 1.0],
            shadow_view_proj: [Mat4::IDENTITY.to_cols_array(); mesh::SHADOW_CASCADES],
            cascade_far: [1.0, 2.0, 3.0, 4.0],
            shadow_params: [0.5, 0.25, 0.0, 0.0],
            cluster_grid: [1, 2, 3, 0],
            light_view_proj: [Mat4::IDENTITY.to_cols_array(); mesh::SHADOW_LIGHT_TILES],
            probes: crcbl_shaders::probe::ProbeVolume::default(),
            lod_params: [3.0, 4.0, 5.0, 0.0],
            fog_params: [0.1; 4],
            fog_color: [0.2; 4],
            sky_sh_r: [0.3; 4],
            sky_sh_g: [0.4; 4],
            sky_sh_b: [0.5; 4],
            previous_view_proj: matrix,
            vertex_pool: [11, 12, 13, 14],
            shadow_atlas_rect: [[0.75; 4]; mesh::SHADOW_ATLAS_TILES],
            shadow_filter: [1, 2, 3, 4],
        }
    }

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
        let mut views: Vec<_> = [
            (LIGHT, 9),
            (0, 3),
            (LIGHT, 7),
            (1, 4),
            (LIGHT, 1),
            (LIGHT, 8),
            (LIGHT, 5),
            (LIGHT, 6),
        ]
        .into_iter()
        .map(|(owner, id)| (owner, id, filled_block(id, owner)))
        .collect();
        let mut culls: Vec<_> = [LIGHT, 0, 1, LIGHT]
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
        renderer.shadow_group_writes = (0..SHADOW_CULLS as u64).map(|n| 40 + n).collect();
        let expected = |writes: u64,
                        with_eye: bool,
                        group: usize,
                        views: &[(usize, usize, mesh::FrameUniforms)],
                        culls: &[(usize, Frustum)]| {
            let mut bytes = Vec::new();
            bytes.extend(writes.to_le_bytes());
            bytes.extend(count.to_le_bytes());
            bytes.push(u8::from(with_eye));
            if with_eye {
                bytes.extend(eye.into_iter().flat_map(f32::to_le_bytes));
            }
            for (owner, id, block) in views {
                if *owner == group {
                    bytes.extend((*id as u32).to_le_bytes());
                    bytes.extend(depth_pass_reads(block).to_bytes());
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
        for group in 0..SHADOW_CULLS {
            let record = renderer.shadow_group_record(group, &views, &culls, eye, count);
            // A cascade reads the eye always; a light slot culled against a
            // frustum no DAG element reaches does not, and one that culls
            // nothing this frame is taken to.
            let with_eye = group < shadow::CASCADES || group > LIGHT;
            assert_eq!(
                record,
                expected(40 + group as u64, with_eye, group, &views, &culls),
                "group {group}"
            );
            assert_eq!(record.capacity(), record.len());
        }

        let original = renderer.shadow_group_record(LIGHT, &views, &culls, eye, count);
        let cascade = renderer.shadow_group_record(0, &views, &culls, eye, count);
        // The eye: a cascade's input, and not a flat-only light's.
        assert_ne!(
            cascade,
            renderer.shadow_group_record(0, &views, &culls, [0.0, 7.0, -9.0], count)
        );
        assert_eq!(
            original,
            renderer.shadow_group_record(LIGHT, &views, &culls, [5.0, 7.0, -9.0], count),
            "a light whose cull no DAG element reaches drew the same maps from any eye"
        );
        assert_ne!(
            original,
            renderer.shadow_group_record(LIGHT, &views, &culls, eye, count - 1)
        );

        // Another group's blocks and culls are not this one's.
        views[1].2.ambient[3] += 1.0;
        culls[1].1.planes[0].x += 1.0;
        assert_eq!(
            original,
            renderer.shadow_group_record(LIGHT, &views, &culls, eye, count)
        );

        // Every field the depth pass reads is in the record ...
        type Field = fn(&mut mesh::FrameUniforms);
        let read: [(&str, Field); 6] = [
            ("view_proj", |block| block.view_proj[3] += 1.0),
            ("previous_view_proj", |block| {
                block.previous_view_proj[3] += 1.0;
            }),
            ("camera_position", |block| block.camera_position[0] += 1.0),
            ("lod_params", |block| block.lod_params[1] += 1.0),
            ("ambient.w", |block| block.ambient[3] += 1.0),
            ("vertex_pool", |block| block.vertex_pool[0] += 1),
        ];
        for (name, change) in read {
            let saved = views[0].2;
            change(&mut views[0].2);
            assert_ne!(
                original,
                renderer.shadow_group_record(LIGHT, &views, &culls, eye, count),
                "the depth pass reads `{name}` and the record did not change with it"
            );
            views[0].2 = saved;
        }
        // ... and none of the colour pass's is, which is what lets a light hold
        // its maps while the camera, which several of them follow, moves.
        let unread: [(&str, Field); 13] = [
            ("ambient.xyz", |block| block.ambient[0] += 1.0),
            ("shadow_view_proj", |block| {
                block.shadow_view_proj[0][3] += 1.0
            }),
            ("cascade_far", |block| block.cascade_far[0] += 1.0),
            ("shadow_params", |block| block.shadow_params[0] += 1.0),
            ("cluster_grid", |block| block.cluster_grid[0] += 1),
            ("light_view_proj", |block| {
                block.light_view_proj[0][3] += 1.0
            }),
            ("probes", |block| block.probes.counts[0] += 1),
            ("fog_params", |block| block.fog_params[0] += 1.0),
            ("fog_color", |block| block.fog_color[0] += 1.0),
            ("sky_sh", |block| {
                block.sky_sh_r[0] += 1.0;
                block.sky_sh_g[0] += 1.0;
                block.sky_sh_b[0] += 1.0;
            }),
            ("shadow_atlas_rect", |block| {
                block.shadow_atlas_rect[0][0] += 1.0;
            }),
            ("shadow_filter", |block| block.shadow_filter[0] += 1),
            ("everything at once", |block| {
                block.ambient[1] += 1.0;
                block.cascade_far[3] += 1.0;
                block.fog_params[3] += 1.0;
            }),
        ];
        for (name, change) in unread {
            let saved = views[0].2;
            change(&mut views[0].2);
            assert_eq!(
                original,
                renderer.shadow_group_record(LIGHT, &views, &culls, eye, count),
                "the depth pass reads nothing of `{name}` and the record moved with it"
            );
            views[0].2 = saved;
        }

        views[0].1 += 1;
        assert_ne!(
            original,
            renderer.shadow_group_record(LIGHT, &views, &culls, eye, count)
        );
        views[0].1 -= 1;
        culls[0].1.planes[0].z = 0.0;
        assert_ne!(
            original,
            renderer.shadow_group_record(LIGHT, &views, &culls, eye, count)
        );
        culls[0].1.planes[0].z = -0.0;
        views.swap(0, 2);
        assert_ne!(
            original,
            renderer.shadow_group_record(LIGHT, &views, &culls, eye, count)
        );
        views.swap(0, 2);

        // A write this group's cull could see.
        renderer.shadow_group_writes[LIGHT] += 1;
        let changed = renderer.shadow_group_record(LIGHT, &views, &culls, eye, count);
        assert_ne!(original, changed);
        assert_eq!(&original[size_of::<u64>()..], &changed[size_of::<u64>()..]);
        renderer.shadow_group_writes[LIGHT] -= 1;

        // A DAG element whose footprint reaches the light's frustum puts the
        // eye into the light's record; one outside it does not.
        let frustum = Frustum::from_view_projection(glam::camera::rh::proj::directx::orthographic(
            -1.0, 1.0, -1.0, 1.0, 0.0, 10.0,
        ));
        culls[0].1 = frustum;
        culls[3].1 = frustum;
        let light = |renderer: &ForwardRenderer, eye| {
            renderer.shadow_group_record(LIGHT, &views, &culls, eye, count)
        };
        let far_away = CasterFootprint::Within(Aabb {
            min: Vec3::splat(100.0),
            max: Vec3::splat(101.0),
        });
        renderer.shadow_footprints = vec![far_away];
        renderer.shadow_dag_elements.insert(0);
        assert_eq!(
            light(&renderer, eye),
            light(&renderer, [5.0, 7.0, -9.0]),
            "a DAG element no cull of this light can keep chose no level in its maps"
        );
        renderer.shadow_footprints[0] = CasterFootprint::Within(Aabb {
            min: Vec3::new(-0.5, -0.5, -5.0),
            max: Vec3::new(0.5, 0.5, -4.0),
        });
        assert_ne!(
            light(&renderer, eye),
            light(&renderer, [5.0, 7.0, -9.0]),
            "a DAG element inside the light's frustum has its level chosen from the eye"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
        assert_eq!(recorder.total_live_objects(), 0);
    }

    /// A footprint is the cull's own question: dead is nowhere, skinned is
    /// everywhere, and a live rigid instance is its mesh's box carried by its
    /// transform, never smaller.
    #[test]
    fn a_footprint_is_the_cull_s_box_and_never_smaller() {
        let cube = GpuMesh {
            index_count: 36,
            bounds_min: [-0.5; 3],
            bounds_max: [0.5; 3],
            ..GpuMesh::default()
        };
        let meshes = [cube, GpuMesh::default()];
        let at = |x: f32| GpuInstance {
            transform: Mat4::from_translation(Vec3::new(x, 0.0, 0.0)).to_cols_array(),
            flags: GpuInstance::LIVE,
            ..GpuInstance::default()
        };
        let CasterFootprint::Within(bounds) = CasterFootprint::of(&at(10.0), &meshes) else {
            panic!("a live rigid instance of a real mesh has a box");
        };
        assert!(bounds.min.x < 9.5 && bounds.max.x > 10.5);
        assert!(bounds.max.x < 10.6, "the slack is slack, not a second box");

        let dead = GpuInstance {
            flags: 0,
            ..at(0.0)
        };
        assert_eq!(
            CasterFootprint::of(&dead, &meshes),
            CasterFootprint::Nowhere
        );
        let skinned = GpuInstance {
            flags: GpuInstance::LIVE | GpuInstance::BASE_VERTEX_OVERRIDE,
            ..at(0.0)
        };
        assert_eq!(
            CasterFootprint::of(&skinned, &meshes),
            CasterFootprint::Everywhere
        );
        for mesh in [1, 7] {
            let unnamed = GpuInstance { mesh, ..at(0.0) };
            assert_eq!(
                CasterFootprint::of(&unnamed, &meshes),
                CasterFootprint::Everywhere,
                "mesh {mesh}"
            );
        }
        let nan = GpuInstance {
            transform: [f32::NAN; 16],
            ..at(0.0)
        };
        assert_eq!(
            CasterFootprint::of(&nan, &meshes),
            CasterFootprint::Everywhere,
            "a box of NaN would read as outside every frustum"
        );
    }

    /// **A write reaches a group when its element could be kept by the group's
    /// cull before the write or after it**, and a group culling nothing this
    /// frame is reached by any write.
    #[test]
    fn a_write_reaches_the_groups_its_old_or_new_box_is_in() {
        // A box one unit across either side of the origin, looking down -z.
        let frustum = Frustum::from_view_projection(glam::camera::rh::proj::directx::orthographic(
            -1.0, 1.0, -1.0, 1.0, 0.0, 10.0,
        ));
        let inside = CasterFootprint::Within(Aabb {
            min: Vec3::new(-0.25, -0.25, -3.0),
            max: Vec3::new(0.25, 0.25, -2.0),
        });
        let outside = CasterFootprint::Within(Aabb {
            min: Vec3::new(5.0, 5.0, -3.0),
            max: Vec3::new(6.0, 6.0, -2.0),
        });
        assert!(inside.reaches(&frustum) && !outside.reaches(&frustum));
        let reached = |moved: &[(CasterFootprint, CasterFootprint)]| {
            let mut writes = [0u64; 2];
            // Group 0 culls against the box; group 1 culls nothing.
            shadow_groups_reached(&mut writes, moved, &[(0, frustum)]);
            writes
        };
        assert_eq!(reached(&[]), [0, 0], "no write reaches nothing");
        assert_eq!(reached(&[(inside, inside)]), [1, 1], "moved inside");
        assert_eq!(reached(&[(outside, outside)]), [0, 1], "moved outside");
        assert_eq!(reached(&[(inside, outside)]), [1, 1], "moved out of it");
        assert_eq!(reached(&[(outside, inside)]), [1, 1], "moved into it");
        assert_eq!(
            reached(&[(CasterFootprint::Nowhere, inside)]),
            [1, 1],
            "spawned inside"
        );
        assert_eq!(
            reached(&[(inside, CasterFootprint::Nowhere)]),
            [1, 1],
            "removed from inside"
        );
        assert_eq!(
            reached(&[
                (outside, outside),
                (CasterFootprint::Nowhere, CasterFootprint::Everywhere)
            ]),
            [1, 1],
            "a skinned element reaches every group"
        );
    }
}
