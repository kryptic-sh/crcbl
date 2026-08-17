//! `docs/plan/03-gpu-driven-rendering.md` §3.3's GPU frustum cull pass, against
//! a real driver and against `crcbl::render::cull::visible_instances`.
//!
//! Separate from `draw_gen`, which consumes this pass's output, because the two
//! halves fail differently: a consumer that drew the right picture from the
//! wrong list would say nothing about this pass. So the visible list and its
//! counter are copied back to the host and compared against the CPU oracle
//! directly, which is a check no picture can stand in for.
//!
//! The scene places one instance per rejection the pass can make — outside each
//! of the six planes, inside, straddling an edge, rotated back in, naming a
//! freed mesh, removed, and never written at all — so a dropped plane, a
//! missing `abs` in the bounds transform, a missing empty-entry guard or a
//! missing liveness check each changes a different element of the answer. The
//! last two tests run the pass over `crcbl::render::InstancePool`'s **own
//! buffer** rather than over records typed out here, because a removal that
//! updated the host mirror and never reached the device passes everything
//! above them.

use crate::harness::{Headless, poisoned};
use crcbl::hal::{
    Barriers, BufferDesc, BufferUsage, CommandEncoderDesc, MemoryLocation, ResourceState,
    SubmitInfo,
};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::cull::{Frustum, visible_instances};
use crcbl::render::{Camera, InstanceHandle, InstancePool, InstancePoolDesc, Projection};
use crcbl::shaders::mesh::{GpuInstance, GpuMesh, INSTANCE_STRIDE, MESH_ENTRY_STRIDE};

/// What the visible list holds before every dispatch.
///
/// Not zero and not any instance index the scene uses: a slot the shader never
/// wrote must not be confusable with one it wrote, and instance 0 is a real
/// answer.
///
/// **It reaches the list through a copy, not through
/// [`CommandEncoder::fill_buffer`].** `crcbl-wgpu` refuses a fill with a
/// non-zero value outright — wgpu offers `clear_buffer`, which is a zero fill
/// and nothing else — so the Vulkan original's one-line fill is a
/// backend-specific mechanism wearing a seam call's name. The staging copy in
/// [`CullProbe::over`] and [`CullProbe::run`] puts the same bytes in the same
/// place on every backend; the assertions that read them are unchanged.
///
/// [`CommandEncoder::fill_buffer`]: crcbl::hal::CommandEncoder::fill_buffer
const VISIBLE_SENTINEL: u32 = 0xDEAD_BEEF;

/// Elements the visible list holds, which is [`Params::capacity`].
///
/// Comfortably more than the scene has instances, so the ordinary tests never
/// reach the overflow arm — that one asks for it by rebuilding with a small
/// capacity of its own.
///
/// [`Params::capacity`]: crcbl::shaders::cull::Params::capacity
const VISIBLE_CAPACITY: u32 = 64;

/// The unit cube every "is it in the frustum" instance below draws: table entry
/// 0, and a real index count so the empty-entry guard has nothing to do with it.
fn unit_cube() -> GpuMesh {
    GpuMesh {
        base_vertex: 0,
        base_index: 0,
        index_count: 36,
        bounds_min: [-0.5, -0.5, -0.5],
        bounds_max: [0.5, 0.5, 0.5],
    }
}

/// A tall thin bar: table entry 2, and the mesh whose *rotation* decides
/// whether it is on screen. An axis-aligned box would not — see
/// `crcbl::render::cull::Aabb::transformed`.
fn bar() -> GpuMesh {
    GpuMesh {
        base_vertex: 64,
        base_index: 128,
        index_count: 6,
        bounds_min: [-0.02, -1.0, -0.02],
        bounds_max: [0.02, 1.0, 0.02],
    }
}

/// The mesh table the scene indexes.
///
/// Entry 1 is [`GpuMesh::default`] — the all-zero record `MeshPool::free`
/// leaves behind. It is a *degenerate box at the origin*, so an instance naming
/// it would be "visible" for any camera looking at the origin if the pass
/// decided on the bounds rather than on the index count.
fn meshes() -> Vec<GpuMesh> {
    vec![unit_cube(), GpuMesh::default(), bar()]
}

/// One **live** instance of mesh `mesh` under `transform`.
///
/// Live rather than default: a zeroed record is a dead one — see
/// [`GpuInstance::LIVE`] — which is what an element no instance occupies reads
/// as, and a scene built out of those would be culled entirely.
fn instance(mesh: u32, transform: Mat4) -> GpuInstance {
    GpuInstance {
        transform: transform.to_cols_array(),
        mesh,
        flags: GpuInstance::LIVE,
        ..GpuInstance::default()
    }
}

/// The scene, placed against the default camera: two metres back along +Z,
/// looking at the origin, 45° vertical field of view.
///
/// Every entry is here because it exercises something different, and the
/// comments say which. The order is the order the CPU reference walks, so an
/// index in a failure message is directly readable against this list.
fn scene() -> Vec<GpuInstance> {
    vec![
        // 0: at the origin — fully inside.
        instance(0, Mat4::IDENTITY),
        // 1..=4: fully outside one side plane each.
        instance(0, Mat4::from_translation(Vec3::new(20.0, 0.0, 0.0))),
        instance(0, Mat4::from_translation(Vec3::new(-20.0, 0.0, 0.0))),
        instance(0, Mat4::from_translation(Vec3::new(0.0, 20.0, 0.0))),
        instance(0, Mat4::from_translation(Vec3::new(0.0, -20.0, 0.0))),
        // 5: behind the eye, which sits at z = 2 — outside the near plane.
        instance(0, Mat4::from_translation(Vec3::new(0.0, 0.0, 20.0))),
        // 6: ten kilometres down the view direction. **Inside**: the engine's
        //    perspective projection is infinite, so its far plane rejects
        //    nothing. A frustum built for a finite far plane culls this.
        instance(0, Mat4::from_translation(Vec3::new(0.0, 0.0, -10_000.0))),
        // 7: straddling the right-hand plane — a box the size of the frustum's
        //    half-width at that depth, centred on the boundary. Neither wholly
        //    in nor wholly out, and the case a test with only extremes misses.
        instance(0, Mat4::from_translation(Vec3::new(0.83, 0.0, 0.0))),
        // 8: the bar, upright and off to the right. Four centimetres wide, so
        //    it is outside.
        instance(2, Mat4::from_translation(Vec3::new(1.5, 0.0, 0.0))),
        // 9: the same bar at the same place, laid on its side. Two metres wide
        //    now, so it reaches back in — and it only does if the bounds
        //    transform takes the absolute value of the rotation.
        instance(
            2,
            Mat4::from_translation(Vec3::new(1.5, 0.0, 0.0))
                * Mat4::from_rotation_z(core::f32::consts::FRAC_PI_2),
        ),
        // 10: naming the cleared table entry, at the origin. Culled by the
        //     index count, not by the frustum.
        instance(1, Mat4::IDENTITY),
        // 11: a removed instance — the record `InstancePool::remove` leaves
        //     behind, which keeps the transform and mesh id it had. At the
        //     origin and drawing the cube, so it is culled by the liveness bit
        //     and by nothing else: instance 0 is the same record with the bit
        //     set, and it survives.
        GpuInstance {
            flags: 0,
            ..instance(0, Mat4::IDENTITY)
        },
        // 12: an element nothing ever wrote, which is all zeroes — a live
        //     instance of mesh 0 at a degenerate transform if the bit is not
        //     asked, and the origin is exactly where the camera is looking.
        GpuInstance::default(),
    ]
}

/// The camera the scene is placed against.
fn scene_camera() -> Camera {
    Camera::default()
}

/// A camera looking at instance 1 instead, from the same distance.
///
/// The visible set must *change* with it — a cull that always keeps everything
/// passes every comparison against a reference that also always keeps
/// everything, which is why one of the tests below asserts the two sets differ.
fn turned_camera() -> Camera {
    Camera {
        eye: Vec3::new(20.0, 0.0, 2.0),
        target: Vec3::new(20.0, 0.0, 0.0),
        ..Camera::default()
    }
}

/// The aspect ratio every frustum here is built for. The offscreen ring's, so
/// the numbers match what the rest of the suite renders at.
fn aspect() -> f32 {
    crate::harness::EXTENT.0 as f32 / crate::harness::EXTENT.1 as f32
}

/// Everything the cull pass needs on the device, plus the readback path.
struct CullProbe {
    params: crcbl::hal::BufferHandle,
    /// The instance array. Created by [`CullProbe::new`] and destroyed with the
    /// probe, or somebody else's — see [`CullProbe::over`], where it is an
    /// [`InstancePool`]'s and outlives this.
    instances: crcbl::hal::BufferHandle,
    /// Whether [`CullProbe::destroy`] releases `instances`.
    owns_instances: bool,
    meshes: crcbl::hal::BufferHandle,
    visible: crcbl::hal::BufferHandle,
    counter: crcbl::hal::BufferHandle,
    staging: crcbl::hal::BufferHandle,
    /// [`VISIBLE_SENTINEL`] repeated, on the host, ready to be copied over
    /// `visible` before each dispatch. See that constant for why this is not a
    /// `fill_buffer`.
    sentinel: crcbl::hal::BufferHandle,
    capacity: u32,
    instance_count: u32,
    bind_group_layout: crcbl::hal::BindGroupLayoutHandle,
    bind_group: crcbl::hal::BindGroupHandle,
    pipeline_layout: crcbl::hal::PipelineLayoutHandle,
    pipeline: crcbl::hal::ComputePipelineHandle,
}

/// What one dispatch produced.
struct CullResult {
    /// The compacted list, as far as the counter says it was filled, **sorted**.
    /// The GPU hands slots out through an atomic, so its order is not the
    /// reference's and is not reproducible between runs.
    visible: Vec<u32>,
    /// The counter the shader wrote: the true number of survivors, which can
    /// exceed the capacity.
    count: u32,
    /// Every element of the list, unsorted and including the slots past the
    /// counter — so a test can assert that the shader wrote nothing where it
    /// said it wrote nothing.
    raw: Vec<u32>,
}

impl CullProbe {
    /// A probe over an instance array of its own, written from `instances`.
    fn new(
        headless: &Headless,
        instances: &[GpuInstance],
        meshes: &[GpuMesh],
        capacity: u32,
    ) -> Self {
        let device = headless.device.as_ref();
        let instance_count = u32::try_from(instances.len()).expect("a small scene");
        // Host-visible, exactly as `crcbl::render::InstancePool`'s buffers are —
        // which is what the other constructor hands over instead of this.
        let instance_buffer = device
            .create_buffer(&BufferDesc {
                label: Some("cull instances"),
                size: (instances.len() * INSTANCE_STRIDE) as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("an instance buffer");
        for (index, instance) in instances.iter().enumerate() {
            device
                .write_buffer(
                    instance_buffer,
                    (index * INSTANCE_STRIDE) as u64,
                    &instance.to_bytes(),
                )
                .expect("write");
        }
        let mut probe = Self::over(headless, instance_buffer, instance_count, meshes, capacity);
        probe.owns_instances = true;
        probe
    }

    /// A probe over an instance array somebody else owns and keeps writing —
    /// [`InstancePool`]'s, whose buffer this binds directly.
    ///
    /// The pool's own bytes rather than a copy of what they ought to be: a
    /// removal that cleared the mirror and never reached the device is exactly
    /// what a host copy could not tell apart from one that worked.
    fn over(
        headless: &Headless,
        instance_buffer: crcbl::hal::BufferHandle,
        instance_count: u32,
        meshes: &[GpuMesh],
        capacity: u32,
    ) -> Self {
        let device = headless.device.as_ref();

        // Host-visible for both remaining inputs, exactly as `crcbl-render`'s
        // own pools are: the mesh table is a `HostUpload` storage buffer there,
        // and the uniform block is one everywhere in the engine. A staging copy
        // would need barriers this test is not about.
        let params = device
            .create_buffer(&BufferDesc {
                label: Some("cull params"),
                size: crcbl::shaders::cull::PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a uniform buffer");
        let mesh_buffer = device
            .create_buffer(&BufferDesc {
                label: Some("cull mesh table"),
                size: (meshes.len() * MESH_ENTRY_STRIDE) as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a mesh table");
        for (index, mesh) in meshes.iter().enumerate() {
            device
                .write_buffer(
                    mesh_buffer,
                    (index * MESH_ENTRY_STRIDE) as u64,
                    &mesh.to_bytes(),
                )
                .expect("write");
        }

        let visible = device
            .create_buffer(&BufferDesc {
                label: Some("cull visible"),
                size: u64::from(capacity) * 4,
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a visible list");
        let counter = device
            .create_buffer(&BufferDesc {
                label: Some("cull counter"),
                size: 4,
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a counter");
        let staging = device
            .create_buffer(&BufferDesc {
                label: Some("cull readback"),
                size: u64::from(capacity) * 4 + 4,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer");
        let sentinel = device
            .create_buffer(&BufferDesc {
                label: Some("cull sentinel"),
                // One word past the list: the counter's zero lives there, so a
                // run needs no buffer fill at all — see the write below.
                size: u64::from(capacity) * 4 + 4,
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a sentinel source");
        // Written once and copied every run. A `HostUpload` source copied from
        // with no barrier of its own is the same shape `crcbl_render::texture`'s
        // upload path uses.
        device
            .write_buffer(
                sentinel,
                0,
                &VISIBLE_SENTINEL
                    .to_le_bytes()
                    .repeat(capacity as usize)
                    .into_iter()
                    .chain(0u32.to_le_bytes())
                    .collect::<Vec<u8>>(),
            )
            .expect("write");

        let storage = |read_only| crcbl::hal::BindGroupLayoutEntry {
            binding: 0,
            visibility: crcbl::hal::ShaderStages::COMPUTE,
            kind: crcbl::hal::BindingKind::StorageBuffer {
                read_only,
                dynamic: false,
            },
            count: 1,
            flags: crcbl::hal::BindingFlags::empty(),
        };
        let layout_entries = [
            crcbl::hal::BindGroupLayoutEntry {
                binding: 0,
                visibility: crcbl::hal::ShaderStages::COMPUTE,
                kind: crcbl::hal::BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: crcbl::hal::BindingFlags::empty(),
            },
            // `StructuredBuffer` in the shader, so read-only here — the cull
            // pass never edits an instance or a mesh entry.
            crcbl::hal::BindGroupLayoutEntry {
                binding: 1,
                ..storage(true)
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 2,
                ..storage(true)
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 3,
                ..storage(false)
            },
            crcbl::hal::BindGroupLayoutEntry {
                binding: 4,
                ..storage(false)
            },
        ];
        let bind_group_layout = device
            .create_bind_group_layout(&crcbl::hal::BindGroupLayoutDesc {
                label: Some("cull"),
                entries: &layout_entries,
            })
            .expect("the cull layout");

        let group_entries = [
            crcbl::hal::BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(params),
            },
            crcbl::hal::BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(instance_buffer),
            },
            crcbl::hal::BindGroupEntry {
                binding: 2,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(mesh_buffer),
            },
            crcbl::hal::BindGroupEntry {
                binding: 3,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(visible),
            },
            crcbl::hal::BindGroupEntry {
                binding: 4,
                array_index: 0,
                resource: crcbl::hal::BindingResource::whole_buffer(counter),
            },
        ];
        let bind_group = device
            .create_bind_group(&crcbl::hal::BindGroupDesc {
                label: Some("cull"),
                layout: bind_group_layout,
                entries: &group_entries,
                variable_count: None,
            })
            .expect("a bind group");

        let set_layouts = [bind_group_layout];
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl::hal::PipelineLayoutDesc {
                label: Some("cull"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("a pipeline layout");

        let module = device
            .create_shader_module(&crcbl::hal::ShaderModuleDesc {
                label: Some("cull.slang"),
                spirv: crcbl::shaders::CULL.spirv(),
                wgsl: crcbl::shaders::CULL.wgsl(),
                msl: crcbl::shaders::CULL.msl(),
                // **Every target, not just SPIR-V.** The Vulkan original passed
                // `&[]` here because only Vulkan ever ran it; a D3D12 device
                // handed an empty container list has no code to compile.
                dxil: &crcbl::shaders::CULL.dxil_containers(),
            })
            .expect("the committed artifacts are accepted");
        // The manifest's name rather than a literal: it is read out of the
        // artifact's own `OpEntryPoint`.
        let entry_point = crcbl::shaders::CULL
            .entry_point(crcbl::shaders::Stage::Compute)
            .expect("the cull pass has exactly one compute entry point");
        let pipeline = device
            .create_compute_pipeline(&crcbl::hal::ComputePipelineDesc {
                label: Some("cull"),
                layout: pipeline_layout,
                compute: crcbl::hal::ShaderEntry {
                    module,
                    entry_point,
                },
                // The shader's own number rather than a literal, for the same
                // reason the entry point is.
                workgroup_size: [crcbl::shaders::cull::WORKGROUP_SIZE, 1, 1],
            })
            .expect("a compute pipeline");
        device.destroy_shader_module(module);

        Self {
            params,
            instances: instance_buffer,
            owns_instances: false,
            meshes: mesh_buffer,
            visible,
            counter,
            staging,
            sentinel,
            capacity,
            instance_count,
            bind_group_layout,
            bind_group,
            pipeline_layout,
            pipeline,
        }
    }

    /// Fills the list with the sentinel, zeroes the counter, dispatches one
    /// thread per instance, and reads both buffers back.
    fn run(&self, headless: &Headless, frustum: &Frustum) -> CullResult {
        let device = headless.device.as_ref();
        let visible_bytes = u64::from(self.capacity) * 4;

        device
            .write_buffer(
                self.params,
                0,
                &crcbl::shaders::cull::Params {
                    planes: frustum.planes.map(|plane| plane.to_array()),
                    instance_count: self.instance_count,
                    capacity: self.capacity,
                }
                .to_bytes(),
            )
            .expect("write");

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("cull dispatch"),
            queue: headless.queue,
        });
        let buffer_barrier = |buffer, from, to| crcbl::hal::BufferBarrier {
            buffer,
            from,
            to,
            queue_transfer: None,
        };
        // `TransferSrc` as the source state is vacuous on the first run and is
        // the real prior use on every later one.
        encoder.pipeline_barrier(&Barriers {
            buffers: &[
                buffer_barrier(
                    self.visible,
                    ResourceState::TransferSrc,
                    ResourceState::TransferDst,
                ),
                buffer_barrier(
                    self.counter,
                    ResourceState::TransferSrc,
                    ResourceState::TransferDst,
                ),
            ],
            ..Barriers::default()
        });
        encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
            src: self.sentinel,
            src_offset: 0,
            dst: self.visible,
            dst_offset: 0,
            size: visible_bytes,
        });
        // **The counter's zero is a copy, not a fill.** `CommandEncoder::
        // fill_buffer` is documented as writing a repeating 32-bit value, and
        // three of the four backends cannot honour that: `crcbl-dx12` has no
        // buffer fill at all, `crcbl-mtl`'s takes a byte rather than a word, and
        // `crcbl-wgpu` refuses any non-zero value. A zero fill is not the safe
        // subset either — dx12 refuses that too. So the zero comes from the same
        // `HostUpload` staging buffer as the sentinel, one word past the list.
        //
        // The shader only ever adds to the counter, so the zero has to come from
        // here. A counter left holding the previous run's total is the defect
        // this exists to prevent, and it is why every test below that runs twice
        // checks the second answer as hard as the first.
        encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
            src: self.sentinel,
            src_offset: u64::from(self.capacity) * 4,
            dst: self.counter,
            dst_offset: 0,
            size: 4,
        });
        // `ShaderReadWrite` rather than a write-only state: a storage-buffer
        // descriptor permits reads whatever the source does with it, and naming
        // only the write leaves the fill unsynchronised against one.
        encoder.pipeline_barrier(&Barriers {
            buffers: &[
                buffer_barrier(
                    self.visible,
                    ResourceState::TransferDst,
                    ResourceState::ShaderReadWrite,
                ),
                buffer_barrier(
                    self.counter,
                    ResourceState::TransferDst,
                    ResourceState::ShaderReadWrite,
                ),
            ],
            ..Barriers::default()
        });

        encoder.begin_compute_pass(&crcbl::hal::ComputePassDesc {
            label: Some("cull"),
        });
        encoder.bind_compute_pipeline(self.pipeline);
        encoder.bind_group(0, self.bind_group, &[], self.pipeline_layout);
        let groups = self
            .instance_count
            .div_ceil(crcbl::shaders::cull::WORKGROUP_SIZE);
        encoder.dispatch(groups, 1, 1);
        encoder.end_compute_pass();

        encoder.pipeline_barrier(&Barriers {
            buffers: &[
                buffer_barrier(
                    self.visible,
                    ResourceState::ShaderReadWrite,
                    ResourceState::TransferSrc,
                ),
                buffer_barrier(
                    self.counter,
                    ResourceState::ShaderReadWrite,
                    ResourceState::TransferSrc,
                ),
            ],
            ..Barriers::default()
        });
        encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
            src: self.visible,
            src_offset: 0,
            dst: self.staging,
            dst_offset: 0,
            size: visible_bytes,
        });
        encoder.copy_buffer_to_buffer(&crcbl::hal::BufferCopy {
            src: self.counter,
            src_offset: 0,
            dst: self.staging,
            dst_offset: visible_bytes,
            size: 4,
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);

        let mut bytes = poisoned((visible_bytes + 4) as usize);
        headless.readback(self.staging, visible_bytes + 4, &mut bytes);
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().expect("four bytes")))
            .collect();
        let (raw, tail) = words.split_at(self.capacity as usize);
        let count = tail[0];
        let mut visible: Vec<u32> = raw
            .iter()
            .copied()
            .take((count as usize).min(raw.len()))
            .collect();
        visible.sort_unstable();
        CullResult {
            visible,
            count,
            raw: raw.to_vec(),
        }
    }

    fn destroy(self, headless: &Headless) {
        let device = headless.device.as_ref();
        device.destroy_compute_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.bind_group);
        device.destroy_bind_group_layout(self.bind_group_layout);
        for buffer in [
            self.sentinel,
            self.staging,
            self.counter,
            self.visible,
            self.meshes,
            self.params,
        ] {
            device.destroy_buffer(buffer);
        }
        if self.owns_instances {
            device.destroy_buffer(self.instances);
        }
    }
}

/// The pass agrees with the CPU reference, instance for instance, on a scene
/// with one case per rejection it can make.
///
/// The reference is the oracle: it is ordinary Rust in `crcbl::render::cull`,
/// with its own unit tests against hand-placed boxes, and this asserts the GPU
/// reproduces it rather than asserting a list somebody typed out.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn the_gpu_visible_list_matches_the_cpu_reference() {
    let headless = Headless::open();
    let (instances, meshes) = (scene(), meshes());
    let probe = CullProbe::new(&headless, &instances, &meshes, VISIBLE_CAPACITY);

    let frustum = Frustum::from_view_projection(scene_camera().view_projection(aspect()));
    let expected = visible_instances(&frustum, &instances, &meshes);
    // The scene is placed so this is a real mixture. Asserting it here means a
    // future edit that made every instance visible — or none — fails as a
    // *scene* problem rather than passing as a vacuous comparison.
    assert!(
        expected.len() > 1 && expected.len() < instances.len(),
        "the reference keeps {expected:?} of {} instances; a scene that is all \
         in or all out proves nothing",
        instances.len()
    );

    let result = probe.run(&headless, &frustum);
    assert_eq!(
        result.visible, expected,
        "the GPU kept a different set of instances than the reference"
    );
    assert_eq!(
        result.count,
        u32::try_from(expected.len()).expect("a small scene"),
        "the counter must be the number of survivors, not one more or one less"
    );
    // Every slot past the count is untouched, which is what says the shader
    // wrote exactly `count` entries rather than writing more and reporting
    // fewer.
    for (slot, value) in result.raw.iter().enumerate().skip(expected.len()) {
        assert_eq!(
            *value, VISIBLE_SENTINEL,
            "slot {slot} was written past the counter"
        );
    }

    probe.destroy(&headless);
    headless.finish();
}

/// The one case the reference cannot be trusted to catch on its own: the
/// visible set must **change** with the camera.
///
/// A cull that kept everything would match a reference that kept everything, so
/// this asserts a specific instance flips — the one at x = 20, which the default
/// camera cannot see and a camera standing in front of it cannot miss — and that
/// the instance at the origin flips the other way.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn the_visible_set_changes_with_the_camera() {
    let headless = Headless::open();
    let (instances, meshes) = (scene(), meshes());
    let probe = CullProbe::new(&headless, &instances, &meshes, VISIBLE_CAPACITY);

    let ahead = Frustum::from_view_projection(scene_camera().view_projection(aspect()));
    let turned = Frustum::from_view_projection(turned_camera().view_projection(aspect()));

    let first = probe.run(&headless, &ahead);
    let second = probe.run(&headless, &turned);

    assert!(
        first.visible.contains(&0) && !first.visible.contains(&1),
        "the default camera sees the cube at the origin and not the one at x = 20: {:?}",
        first.visible
    );
    assert!(
        second.visible.contains(&1) && !second.visible.contains(&0),
        "and the turned camera is the other way round: {:?}",
        second.visible
    );
    assert_ne!(
        first.visible, second.visible,
        "a cull whose answer does not depend on the camera is not a cull"
    );

    // Both still agree with the reference, which is what says the second run
    // re-culled rather than reporting a stale buffer.
    assert_eq!(
        second.visible,
        visible_instances(&turned, &instances, &meshes)
    );
    assert_eq!(
        second.count,
        u32::try_from(second.visible.len()).expect("a small scene")
    );

    probe.destroy(&headless);
    headless.finish();
}

/// The engine's perspective projection is **infinite**, so the far plane
/// rejects nothing — and an orthographic camera's does.
///
/// Same scene, same instance ten kilometres away: kept by one camera and culled
/// by the other. This is the assertion that would fail if the extraction
/// normalized its planes, because the infinite camera's far plane has a zero
/// normal and normalizing it produces `NaN`s that compare false everywhere.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn a_finite_far_plane_culls_what_an_infinite_one_keeps() {
    let headless = Headless::open();
    let (instances, meshes) = (scene(), meshes());
    let probe = CullProbe::new(&headless, &instances, &meshes, VISIBLE_CAPACITY);

    let infinite = Frustum::from_view_projection(scene_camera().view_projection(aspect()));
    let finite = Frustum::from_view_projection(
        scene_camera()
            .with_projection(Projection::Orthographic {
                half_height: 1.0,
                near: 0.1,
                far: 100.0,
            })
            .view_projection(aspect()),
    );

    let distant = probe.run(&headless, &infinite);
    assert!(
        distant.visible.contains(&6),
        "the infinite projection has no far plane, so the instance at z = -10000 is \
         visible: {:?}",
        distant.visible
    );

    let bounded = probe.run(&headless, &finite);
    assert!(
        !bounded.visible.contains(&6),
        "and the orthographic camera's far plane at 100 culls it: {:?}",
        bounded.visible
    );
    assert_eq!(
        bounded.visible,
        visible_instances(&finite, &instances, &meshes)
    );

    probe.destroy(&headless);
    headless.finish();
}

/// A capacity smaller than the number of survivors: the counter keeps counting
/// and the list stops being written.
///
/// That is the shape topic 03's 2026-07-27 correction asks for — "per-bucket
/// capacity is sized from scene stats with an overflow counter" — and it is the
/// difference between an overflow a caller can see and a list that quietly
/// stops growing.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn an_overflowing_list_still_counts_every_survivor() {
    let headless = Headless::open();
    let (instances, meshes) = (scene(), meshes());
    let frustum = Frustum::from_view_projection(scene_camera().view_projection(aspect()));
    let expected = visible_instances(&frustum, &instances, &meshes);
    let capacity = u32::try_from(expected.len()).expect("a small scene") - 1;

    let probe = CullProbe::new(&headless, &instances, &meshes, capacity);
    let result = probe.run(&headless, &frustum);

    assert_eq!(
        result.count,
        u32::try_from(expected.len()).expect("a small scene"),
        "the counter is the true survivor count even when the list cannot hold them"
    );
    assert_eq!(
        result.raw.len(),
        capacity as usize,
        "and the list is exactly its capacity"
    );
    // Whichever survivors landed, they are survivors — the atomic decides who
    // gets a slot, so *which* of them is not something to assert.
    for value in &result.raw {
        assert!(
            expected.contains(value),
            "slot holds {value}, which the reference does not keep: {expected:?}"
        );
    }
    assert!(
        !result.raw.contains(&VISIBLE_SENTINEL),
        "every slot was filled: {:?}",
        result.raw
    );

    probe.destroy(&headless);
    headless.finish();
}

// --- the pool's own removals -----------------------------------------------
//
// The two tests below run the pass over `crcbl::render::InstancePool`'s **own
// buffer**, so what is culled is what `insert`, `remove` and `begin_frame`
// actually put on the device. A removal that updated the host mirror and never
// reached the GPU passes every test above this line.
//
// The dispatch covers the pool's whole capacity rather than the three slots the
// scene uses, which puts the slots nothing ever wrote inside the tested range.
// Those are all zeroes — a live-looking instance of mesh 0 at a degenerate
// transform on the origin, if the liveness bit is not asked.

/// Slots in the pooled scene's array; three are used and the rest are never
/// written.
const POOL_CAPACITY: u32 = 8;

/// A cube at `at`, as a caller hands one to the pool: **without** the liveness
/// bit, which is [`InstancePool`]'s to set.
///
/// That is the ownership `InstancePool::insert` documents, and handing over a
/// record that already carries the bit would make a pool that never set one
/// indistinguishable from the pool this suite is testing.
fn pooled(at: Vec3) -> GpuInstance {
    GpuInstance {
        flags: 0,
        ..instance(0, Mat4::from_translation(at))
    }
}

/// The pooled scene: a cube at the origin, a second cube beside it, and a third
/// twenty metres to the right that only [`turned_camera`] can see.
///
/// One buffer in the ring, so [`InstancePool::begin_frame`] always rotates to
/// the same one and the bind group can name it once. Which buffer a write lands
/// in is `instance_pool`'s own suite's subject, not this one's.
fn pooled_scene(headless: &Headless) -> (InstancePool, [InstanceHandle; 3]) {
    let device = headless.device.as_ref();
    let mut pool = InstancePool::new(
        device,
        &InstancePoolDesc {
            label: Some("cull pool"),
            capacity: POOL_CAPACITY,
            frames_in_flight: 1,
        },
    )
    .expect("a small pool");
    let handles = [
        Vec3::ZERO,
        Vec3::new(0.5, 0.0, 0.0),
        Vec3::new(20.0, 0.0, 0.0),
    ]
    .map(|at| pool.insert(&pooled(at)).expect("room"));
    assert_eq!(
        handles.map(|handle| pool.index(handle)),
        [Some(0), Some(1), Some(2)],
        "the three cubes are the first three slots, which the assertions below name"
    );
    (pool, handles)
}

/// The pool's array as the host believes it stands, for the CPU oracle.
///
/// Live records come from the pool itself; a removed slot's is the caller's
/// last record with the bit cleared, which is [`InstancePool::remove`]'s whole
/// documented effect. Slots nothing wrote are [`GpuInstance::default`].
fn host_array(pool: &InstancePool, handles: &[Option<InstanceHandle>]) -> Vec<GpuInstance> {
    let mut array = vec![GpuInstance::default(); pool.capacity() as usize];
    for (index, handle) in handles.iter().enumerate() {
        if let Some(live) = handle.and_then(|handle| pool.get(handle)) {
            array[index] = live;
        }
    }
    array
}

/// **A removed instance is not in the visible list.** The slice's headline
/// claim, against a real driver and through the pool that makes it.
///
/// The removed instance is a unit cube at the origin, which the camera is
/// looking straight at from two metres — so it is not merely absent, it is
/// absent while being the most visible thing in the scene. Its live twin sits
/// half a metre away and stays in the list, which is what says the pass still
/// ran.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn a_removed_instance_is_not_in_the_visible_list() {
    let headless = Headless::open();
    let device = headless.device.as_ref();
    let meshes = meshes();
    let (mut pool, handles) = pooled_scene(&headless);
    let [removed, control, distant] = handles;

    let slot = pool
        .begin_frame(device)
        .expect("the upload reaches the buffer");
    let probe = CullProbe::over(
        &headless,
        pool.buffers()[slot],
        pool.capacity(),
        &meshes,
        VISIBLE_CAPACITY,
    );
    let frustum = Frustum::from_view_projection(scene_camera().view_projection(aspect()));

    let live = [Some(removed), Some(control), Some(distant)];
    let before = probe.run(&headless, &frustum);
    assert_eq!(
        before.visible,
        vec![0, 1],
        "both cubes in front of the camera are visible, and the one at x = 20 is not"
    );
    assert_eq!(
        before.visible,
        visible_instances(&frustum, &host_array(&pool, &live), &meshes),
        "the reference agrees before anything is removed"
    );
    // The slots nothing was ever inserted into are inside the dispatch, and
    // none of them is in the list: an unwritten element is a dead instance.
    assert_eq!(before.count, 2);

    assert!(pool.remove(removed));
    pool.begin_frame(device)
        .expect("the removal reaches the buffer");
    let after = probe.run(&headless, &frustum);
    assert!(
        !after.visible.contains(&0),
        "the removed instance is still being drawn: {:?}",
        after.visible
    );
    assert_eq!(
        after.visible,
        vec![1],
        "and its live neighbour is not disturbed"
    );
    assert_eq!(
        after.count, 1,
        "the counter is the survivors, so the removal is not merely unwritten to the list"
    );
    let mut expected = host_array(&pool, &[None, Some(control), Some(distant)]);
    // The removed slot keeps its transform and mesh id and loses the bit, which
    // is what the pool documents and what the reference is run over here.
    expected[0] = pooled(Vec3::ZERO);
    assert_eq!(
        after.visible,
        visible_instances(&frustum, &expected, &meshes),
        "the reference learned liveness too, and agrees"
    );

    probe.destroy(&headless);
    pool.destroy(device);
    headless.finish();
}

/// **A slot reused after a removal is live again, and draws its new contents.**
///
/// The other direction of the same defect: a liveness bit that stuck would make
/// the pool leak capacity that looks allocated and draws nothing.
///
/// The reused slot is given a transform the *default* camera cannot see and the
/// turned one cannot miss, so its presence in the turned camera's list says
/// both halves at once — a dead slot is absent from every camera's list, and
/// the contents it replaced were at the origin, which the turned camera cannot
/// see either.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-draw-gen-e2e.sh"]
fn a_slot_reused_after_a_removal_draws_its_new_contents() {
    let headless = Headless::open();
    let device = headless.device.as_ref();
    let meshes = meshes();
    let (mut pool, handles) = pooled_scene(&headless);
    let [removed, control, distant] = handles;

    let slot = pool
        .begin_frame(device)
        .expect("the upload reaches the buffer");
    let probe = CullProbe::over(
        &headless,
        pool.buffers()[slot],
        pool.capacity(),
        &meshes,
        VISIBLE_CAPACITY,
    );
    let ahead = Frustum::from_view_projection(scene_camera().view_projection(aspect()));
    let turned = Frustum::from_view_projection(turned_camera().view_projection(aspect()));

    assert!(pool.remove(removed));
    let reused = pool
        .insert(&pooled(Vec3::new(20.0, 0.0, 0.0)))
        .expect("the freed slot");
    assert_eq!(pool.index(reused), Some(0), "the low slot comes back");
    pool.begin_frame(device)
        .expect("the reuse reaches the buffer");

    let from_ahead = probe.run(&headless, &ahead);
    assert!(
        !from_ahead.visible.contains(&0),
        "the slot's new contents are twenty metres to the right, and this camera cannot \
         see them: {:?}",
        from_ahead.visible
    );
    let from_the_side = probe.run(&headless, &turned);
    assert!(
        from_the_side.visible.contains(&0),
        "the reused slot is not live, or is still being culled on the contents it \
         replaced: {:?}",
        from_the_side.visible
    );
    assert!(
        !from_the_side.visible.contains(&1),
        "and the cube at the origin is behind this camera: {:?}",
        from_the_side.visible
    );

    let live = [Some(reused), Some(control), Some(distant)];
    for (what, frustum, result) in [
        ("ahead", &ahead, &from_ahead),
        ("turned", &turned, &from_the_side),
    ] {
        assert_eq!(
            result.visible,
            visible_instances(frustum, &host_array(&pool, &live), &meshes),
            "the reference disagrees with the {what} camera"
        );
    }

    probe.destroy(&headless);
    pool.destroy(device);
    headless.finish();
}
