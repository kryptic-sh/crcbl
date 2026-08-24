//! **The skinning kernel, executed.**
//!
//! `crates/crcbl-shaders/shaders/skinning.slang` is dispatched by
//! `crcbl_render::skinning::Skinning`, whose own tests run against
//! `crcbl_hal::null` — a recorder whose `dispatch` records a command and
//! returns. Those tests prove the group counts, the dispatch order, the bind
//! groups and the exact bytes that reached each buffer; they prove nothing at
//! all about the arithmetic, and `docs/backlog.md` recorded the hole they leave:
//! deleting `if (index >= skin.vertex_count) return;` from the kernel passed
//! every test in the workspace.
//!
//! So this module runs the shader on a real driver and compares what came back
//! against [`crcbl_render::skinning::skin_vertex`], which is that entry point
//! rewritten in Rust term for term. `crates/crcbl-vk/tests/vk_e2e/mesh.rs`
//! already uses `crcbl_render::cull::visible_instances` the same way; this is
//! that precedent applied to the pass that had no executor.
//!
//! # The layout every test here dispatches over
//!
//! One pool, one palette buffer, one binding buffer, and **every base non-zero**
//! — [`INPUT_BASE`], [`OUTPUT_BASE`], [`BINDING_BASE`] and [`JOINT_BASE`] are
//! all offsets the kernel has to honour. A kernel that ignored one of them would
//! read a decoy: the slots below each base hold values chosen so that using them
//! lands nowhere near the oracle's answer. With every base zero — the obvious
//! way to write this — four of the six words of `SkinParams` would be untested.
//!
//! The pool is laid out so that the invocations of a *partial last workgroup*
//! stay inside the buffer even when the kernel's bound is gone:
//!
//! ```text
//!   0        4                    132       192            262        320
//!   │ decoy  │ bind pose ………………… │ guard … │ skinned …… │ guard ……… │
//!             INPUT_BASE                     OUTPUT_BASE
//! ```
//!
//! [`INPUT_BASE`] + [`MAX_INVOCATIONS`] is below [`OUTPUT_BASE`], so a tail
//! invocation reading `input_base + index` never reaches the half it writes;
//! [`OUTPUT_BASE`] + [`MAX_INVOCATIONS`] is [`POOL_VERTICES`], so a tail
//! invocation *writing* `output_base + index` lands in the guard rather than
//! past the end of a storage buffer. That matters because the interesting run is
//! the sabotaged one: a test whose red state depends on undefined behaviour
//! reports whatever the driver felt like, and the two drivers this suite runs on
//! do not have to agree about it.
//!
//! # Floating point
//!
//! [`TOLERANCE`] is where the comparison's give is, and why.

use crcbl_hal::{
    Barriers, BufferBarrier, BufferCopy, BufferDesc, BufferHandle, BufferUsage, CommandEncoderDesc,
    Device, MemoryLocation, ResourceState, SubmitInfo,
};
use crcbl_shaders::mesh::{MeshVertex, VERTEX_STRIDE};
use crcbl_shaders::skinning::{
    JOINT_STRIDE, PARAMS_SIZE, Params, SKIN_BINDING_STRIDE, SkinBinding, WORKGROUP_SIZE,
};
use glam::{Mat3, Mat4, Quat, Vec3};

use crate::harness::{Headless, poisoned};

/// First vertex of the bind-pose run, as an index into the pool.
///
/// Not zero, so `SkinParams::input_base` is a word the kernel has to read. The
/// four vertices below it hold [`GUARD_WORD`]'s fill, which decodes to a
/// position no case here produces.
const INPUT_BASE: u32 = 4;

/// First entry of the run of [`SkinBinding`]s, as an index into the binding
/// buffer.
///
/// Not zero, for [`INPUT_BASE`]'s reason, and its own base rather than
/// [`INPUT_BASE`] reused — which is the whole argument in
/// `crcbl_shaders::skinning::Params::binding_base`. The two entries below it are
/// the decoy: bound wholly to the decoy joint.
const BINDING_BASE: u32 = 2;

/// First matrix of the joint palette, as an index into the palette buffer.
///
/// Not zero. The matrix below it is a translation far enough from anything a
/// case uses that a kernel ignoring this base fails by kilometres rather than by
/// rounding.
const JOINT_BASE: u32 = 1;

/// First vertex of the skinned run.
///
/// Far enough above [`INPUT_BASE`] + [`MAX_INVOCATIONS`] that the tail
/// invocations of a partial last workgroup read bind-pose slots and never the
/// skinned half — see the module docs for why that is a property of this test
/// and not of the pass.
const OUTPUT_BASE: u32 = 192;

/// Vertices the pool holds.
///
/// [`OUTPUT_BASE`] + [`MAX_INVOCATIONS`], so a tail invocation's write is inside
/// the buffer.
const POOL_VERTICES: u32 = 320;

/// Invocations the widest dispatch here launches: two full workgroups.
///
/// Every buffer is sized against this rather than against a case's vertex count,
/// because the case that matters dispatches more invocations than it has
/// vertices.
const MAX_INVOCATIONS: u32 = 2 * WORKGROUP_SIZE;

/// Palette matrices a case may use, past [`JOINT_BASE`].
const MAX_JOINTS: u32 = 4;

/// The word the pool is filled with before every dispatch, varied per word.
///
/// Deliberately not zero and deliberately not the harness's `POISON`: a slot the
/// kernel never wrote must be distinguishable both from one it zeroed and from
/// one no copy ever reached. Its bit pattern is a finite `float` (exponent
/// `0x81`), and the per-word variation only ever touches mantissa bits, so a
/// sabotaged kernel that reads the guard as geometry gets numbers rather than
/// `NaN`s — which keeps the failure it produces readable.
const GUARD_WORD: u32 = 0xC0DE_FACE;

/// How far a read-back component may sit from the oracle's, relative to the
/// larger of the expected magnitude and one.
///
/// **Not bit equality.** `docs/backlog.md` records that float output is not
/// bit-portable across implementations, and it is not portable across *sides*
/// here either: the CPU oracle evaluates a 4×4 blend and a matrix-vector product
/// in `glam`'s order, and the driver is free to contract any of those
/// multiply-adds into an FMA and to re-associate the sums. Neither is wrong;
/// they simply round differently.
///
/// `1e-5` is about 84 ulps of `f32` — room for the ten or so roundings between
/// a palette matrix and a written component, plus the two ulps Vulkan's
/// precision table allows `InverseSqrt`, which is what normalises the normal.
/// It is also three to five orders of magnitude tighter than any of the
/// mistakes this file exists to catch: using the bare 3×3 instead of the
/// cofactor, dropping a base offset, blending with the wrong weight or reading
/// the wrong joint each move a component by a quantity of order one.
/// `a_normal_under_a_non_uniform_scale_goes_through_the_cofactor_basis` asserts
/// that margin rather than assuming it.
const TOLERANCE: f32 = 1e-5;

/// Bytes of the vertex pool.
const fn pool_bytes() -> u64 {
    POOL_VERTICES as u64 * VERTEX_STRIDE as u64
}

/// Bytes of the binding buffer.
const fn binding_bytes() -> u64 {
    (BINDING_BASE + MAX_INVOCATIONS) as u64 * SKIN_BINDING_STRIDE as u64
}

/// Bytes of the palette buffer.
const fn joint_bytes() -> u64 {
    (JOINT_BASE + MAX_JOINTS) as u64 * JOINT_STRIDE as u64
}

/// A [`MeshVertex`] as the sixteen little-endian floats the pool holds.
///
/// The pool has no decoder of its own — `crcbl_render::mesh_pool` uploads
/// `VERTEX_STRIDE`-strided bytes and never decodes them — so this file owns both
/// halves of the conversion and [`vertex_from_bytes`] is its inverse.
fn vertex_bytes(vertex: &MeshVertex) -> [u8; VERTEX_STRIDE] {
    let mut bytes = [0u8; VERTEX_STRIDE];
    let mut at = 0usize;
    for field in [vertex.position, vertex.normal, vertex.color, vertex.uv] {
        for value in field {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
    }
    assert_eq!(at, VERTEX_STRIDE, "four float4s and no padding");
    bytes
}

/// The inverse of [`vertex_bytes`].
fn vertex_from_bytes(bytes: &[u8]) -> MeshVertex {
    assert_eq!(bytes.len(), VERTEX_STRIDE, "one whole vertex");
    let float = |at: usize| {
        f32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes of a float"))
    };
    let field = |at: usize| [float(at), float(at + 4), float(at + 8), float(at + 12)];
    MeshVertex {
        position: field(0),
        normal: field(16),
        color: field(32),
        uv: field(48),
    }
}

/// The pool's fill: [`GUARD_WORD`] with the word's own index folded in, so a
/// write landing one slot over is as visible as one landing on top.
fn guard_pool() -> Vec<u8> {
    let words = pool_bytes() as usize / size_of::<u32>();
    (0..words as u32)
        .flat_map(|word| (GUARD_WORD ^ word).to_le_bytes())
        .collect()
}

/// The pool as it is uploaded for `case`: [`guard_pool`] with the bind-pose run
/// written over it at [`INPUT_BASE`].
///
/// One function rather than two, because [`assert_untouched`] compares against
/// exactly the bytes the dispatch was handed — a second copy of this arithmetic
/// would be a second chance to disagree with it, and the assertion it feeds is
/// the one this whole module is for.
fn uploaded_pool(case: &Case) -> Vec<u8> {
    let mut pool = guard_pool();
    for (slot, vertex) in case.input.iter().enumerate() {
        let at = (INPUT_BASE as usize + slot) * VERTEX_STRIDE;
        pool[at..at + VERTEX_STRIDE].copy_from_slice(&vertex_bytes(vertex));
    }
    pool
}

/// One dispatch's inputs: what goes at [`INPUT_BASE`], [`BINDING_BASE`] and
/// [`JOINT_BASE`].
struct Case {
    /// This range's palette, which lands at [`JOINT_BASE`].
    palette: Vec<Mat4>,
    /// One binding per bind-pose vertex, in the same order.
    bindings: Vec<SkinBinding>,
    /// The bind-pose vertices, which land at [`INPUT_BASE`].
    input: Vec<MeshVertex>,
}

impl Case {
    /// The uniform block this case dispatches with.
    fn params(&self) -> Params {
        Params {
            vertex_count: u32::try_from(self.input.len()).expect("a small case"),
            input_base: INPUT_BASE,
            output_base: OUTPUT_BASE,
            binding_base: BINDING_BASE,
            joint_base: JOINT_BASE,
            joint_count: u32::try_from(self.palette.len()).expect("a small palette"),
        }
    }

    /// What [`crcbl_render::skinning::skin_vertex`] says each vertex becomes.
    fn oracle(&self) -> Vec<MeshVertex> {
        self.input
            .iter()
            .zip(&self.bindings)
            .map(|(vertex, binding)| {
                crcbl_render::skinning::skin_vertex(&self.palette, binding, vertex)
            })
            .collect()
    }
}

/// Everything one skinning dispatch needs, built through the seam.
struct SkinProbe {
    params: BufferHandle,
    joints: BufferHandle,
    bindings: BufferHandle,
    /// The vertex pool: read at `input_base + i`, written at `output_base + i`,
    /// bound **once**. Two views of one buffer is a `createBindGroup` failure in
    /// the browser, which is why the kernel is written this way.
    vertices: BufferHandle,
    /// A host-visible buffer holding every run's four uploads end to end.
    upload: BufferHandle,
    /// Host-readable copy target for the whole pool.
    staging: BufferHandle,
    bind_group_layout: crcbl_hal::BindGroupLayoutHandle,
    bind_group: crcbl_hal::BindGroupHandle,
    pipeline_layout: crcbl_hal::PipelineLayoutHandle,
    pipeline: crcbl_hal::ComputePipelineHandle,
}

impl SkinProbe {
    fn new(headless: &Headless) -> Self {
        // In `const` blocks, so a layout that stopped containing the tail of a
        // partial last workgroup fails to compile rather than failing on the
        // one run that would have caught it.
        const {
            assert!(
                INPUT_BASE + MAX_INVOCATIONS <= OUTPUT_BASE,
                "a tail invocation must not read out of the half it writes"
            );
            assert!(
                OUTPUT_BASE + MAX_INVOCATIONS <= POOL_VERTICES,
                "a tail invocation must not write past the end of the pool"
            );
        }

        let device = headless.device.as_ref();
        let upload = device
            .create_buffer(&BufferDesc {
                label: Some("skinning probe upload"),
                size: PARAMS_SIZE as u64 + joint_bytes() + binding_bytes() + pool_bytes(),
                usage: BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::HostUpload,
            })
            .expect("a staging buffer");
        let params = device
            .create_buffer(&BufferDesc {
                label: Some("skinning probe params"),
                size: PARAMS_SIZE as u64,
                usage: BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a uniform buffer");
        let joints = device
            .create_buffer(&BufferDesc {
                label: Some("skinning probe palette"),
                size: joint_bytes(),
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a palette buffer");
        let bindings = device
            .create_buffer(&BufferDesc {
                label: Some("skinning probe bindings"),
                size: binding_bytes(),
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a binding buffer");
        let vertices = device
            .create_buffer(&BufferDesc {
                label: Some("skinning probe pool"),
                size: pool_bytes(),
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
                memory: MemoryLocation::DeviceLocal,
            })
            .expect("a vertex pool");
        let staging = device
            .create_buffer(&BufferDesc {
                label: Some("skinning probe readback"),
                size: pool_bytes(),
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer");

        // The order `crcbl_shaders::skinning`'s module docs pin: the uniform
        // block, the palettes, the bindings, then the pool bound read-write.
        let layout_entries = [
            crcbl_hal::BindGroupLayoutEntry {
                binding: 0,
                visibility: crcbl_hal::ShaderStages::COMPUTE,
                kind: crcbl_hal::BindingKind::UniformBuffer { dynamic: false },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 1,
                visibility: crcbl_hal::ShaderStages::COMPUTE,
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 2,
                visibility: crcbl_hal::ShaderStages::COMPUTE,
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: true,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
            crcbl_hal::BindGroupLayoutEntry {
                binding: 3,
                visibility: crcbl_hal::ShaderStages::COMPUTE,
                kind: crcbl_hal::BindingKind::StorageBuffer {
                    read_only: false,
                    dynamic: false,
                },
                count: 1,
                flags: crcbl_hal::BindingFlags::empty(),
            },
        ];
        let bind_group_layout = device
            .create_bind_group_layout(&crcbl_hal::BindGroupLayoutDesc {
                label: Some("skinning probe"),
                entries: &layout_entries,
            })
            .expect("the probe's layout");

        let group_entries = [
            crcbl_hal::BindGroupEntry {
                binding: 0,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(params),
            },
            crcbl_hal::BindGroupEntry {
                binding: 1,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(joints),
            },
            crcbl_hal::BindGroupEntry {
                binding: 2,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(bindings),
            },
            crcbl_hal::BindGroupEntry {
                binding: 3,
                array_index: 0,
                resource: crcbl_hal::BindingResource::whole_buffer(vertices),
            },
        ];
        let bind_group = device
            .create_bind_group(&crcbl_hal::BindGroupDesc {
                label: Some("skinning probe"),
                layout: bind_group_layout,
                entries: &group_entries,
                variable_count: None,
            })
            .expect("a bind group");

        let set_layouts = [bind_group_layout];
        let pipeline_layout = device
            .create_pipeline_layout(&crcbl_hal::PipelineLayoutDesc {
                label: Some("skinning probe"),
                bind_group_layouts: &set_layouts,
                push_constants: None,
            })
            .expect("a pipeline layout");

        let module = device
            .create_shader_module(&crcbl_hal::ShaderModuleDesc {
                label: Some("skinning.slang"),
                spirv: crcbl_shaders::SKINNING.spirv(),
                wgsl: crcbl_shaders::SKINNING.wgsl(),
                msl: crcbl_shaders::SKINNING.msl(),
                dxil: &[],
            })
            .expect("the committed SPIR-V is accepted");
        // The manifest's name rather than a literal: it is read out of the
        // artifact's `OpEntryPoint` by the compile script, so a Slang release
        // that renamed it fails here rather than in a driver.
        let entry_point = crcbl_shaders::SKINNING
            .entry_point(crcbl_shaders::Stage::Compute)
            .expect("the kernel has exactly one compute entry point");
        let pipeline = device
            .create_compute_pipeline(&crcbl_hal::ComputePipelineDesc {
                label: Some("skinning probe"),
                layout: pipeline_layout,
                compute: crcbl_hal::ShaderEntry {
                    module,
                    entry_point,
                },
                // The shader's own number: `crcbl-shaders` checks this constant
                // against the `[numthreads(…)]` in `skinning.slang`, and this
                // backend checks it again against the SPIR-V it compiles.
                workgroup_size: [WORKGROUP_SIZE, 1, 1],
            })
            .expect("a compute pipeline");
        device.destroy_shader_module(module);

        Self {
            params,
            joints,
            bindings,
            vertices,
            upload,
            staging,
            bind_group_layout,
            bind_group,
            pipeline_layout,
            pipeline,
        }
    }

    /// Uploads `case`, dispatches it, and hands back the whole pool's bytes.
    ///
    /// The pool is refilled with [`guard_pool`] on every run, so what a slot
    /// holds afterwards is either this dispatch's work or the fill — never a
    /// previous run's.
    fn run(&self, headless: &Headless, case: &Case) -> Vec<u8> {
        let device = headless.device.as_ref();
        let params = case.params();
        assert_eq!(
            case.bindings.len(),
            case.input.len(),
            "one binding per bind-pose vertex"
        );
        assert!(
            params.vertex_count <= MAX_INVOCATIONS,
            "the buffers are sized for {MAX_INVOCATIONS} invocations"
        );
        assert!(
            u32::try_from(case.palette.len()).expect("a small palette") <= MAX_JOINTS,
            "the palette buffer is sized for {MAX_JOINTS} matrices past JOINT_BASE"
        );

        let params_bytes = params.to_bytes();

        // The decoy matrix at slot 0: a kernel that ignored `joint_base` would
        // blend this, and no case's answer is within a kilometre of it.
        let mut joint_bytes_out = Mat4::from_translation(Vec3::new(-1024.0, 2048.0, -4096.0))
            .to_cols_array()
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<u8>>();
        for matrix in &case.palette {
            joint_bytes_out.extend(
                matrix
                    .to_cols_array()
                    .iter()
                    .flat_map(|value| value.to_le_bytes()),
            );
        }
        joint_bytes_out.resize(joint_bytes() as usize, 0);

        // The decoy bindings below `BINDING_BASE` name the decoy joint, and
        // every entry past the case's own is a binding with no weight at all —
        // whose blended matrix is zero, which is defined rather than undefined
        // and is what a tail invocation would read if the kernel let one run.
        let decoy = SkinBinding {
            joints: [0; 4],
            weights: [1.0, 0.0, 0.0, 0.0],
        };
        let mut binding_bytes_out = Vec::with_capacity(binding_bytes() as usize);
        for _ in 0..BINDING_BASE {
            binding_bytes_out.extend(decoy.to_bytes());
        }
        for binding in &case.bindings {
            binding_bytes_out.extend(binding.to_bytes());
        }
        binding_bytes_out.resize(binding_bytes() as usize, 0);

        let pool = uploaded_pool(case);

        let mut at = 0u64;
        let mut stage = |bytes: &[u8]| {
            device
                .write_buffer(self.upload, at, bytes)
                .expect("the upload buffer is host-visible");
            let offset = at;
            at += bytes.len() as u64;
            offset
        };
        let params_at = stage(&params_bytes);
        let joints_at = stage(&joint_bytes_out);
        let bindings_at = stage(&binding_bytes_out);
        let pool_at = stage(&pool);

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("skinning probe dispatch"),
            queue: headless.queue,
        });
        let barrier = |buffer, from, to| BufferBarrier {
            buffer,
            from,
            to,
            queue_transfer: None,
        };
        // The source states are the previous run's and are vacuous on the first
        // one — a buffer barrier carries no layout, so naming a wider source
        // scope than actually happened costs a stage mask and cannot be wrong.
        // This is `compute.rs`'s argument for the same shape.
        encoder.pipeline_barrier(&Barriers {
            buffers: &[
                barrier(
                    self.params,
                    ResourceState::ShaderRead,
                    ResourceState::TransferDst,
                ),
                barrier(
                    self.joints,
                    ResourceState::ShaderRead,
                    ResourceState::TransferDst,
                ),
                barrier(
                    self.bindings,
                    ResourceState::ShaderRead,
                    ResourceState::TransferDst,
                ),
                barrier(
                    self.vertices,
                    ResourceState::TransferSrc,
                    ResourceState::TransferDst,
                ),
            ],
            ..Barriers::default()
        });
        for (src_offset, dst, size) in [
            (params_at, self.params, params_bytes.len() as u64),
            (joints_at, self.joints, joint_bytes()),
            (bindings_at, self.bindings, binding_bytes()),
            (pool_at, self.vertices, pool_bytes()),
        ] {
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: self.upload,
                src_offset,
                dst,
                dst_offset: 0,
                size,
            });
        }
        // `ShaderReadWrite` for the pool rather than `ShaderWrite`: a barrier
        // names the access the *descriptor* permits, and Slang emits no
        // `NonReadable` for an `RWStructuredBuffer`. `compute.rs` records that
        // the narrower spelling is a `SYNC-HAZARD-READ-AFTER-WRITE` CI's
        // validation layer reports and this machine's does not.
        encoder.pipeline_barrier(&Barriers {
            buffers: &[
                barrier(
                    self.params,
                    ResourceState::TransferDst,
                    ResourceState::ShaderRead,
                ),
                barrier(
                    self.joints,
                    ResourceState::TransferDst,
                    ResourceState::ShaderRead,
                ),
                barrier(
                    self.bindings,
                    ResourceState::TransferDst,
                    ResourceState::ShaderRead,
                ),
                barrier(
                    self.vertices,
                    ResourceState::TransferDst,
                    ResourceState::ShaderReadWrite,
                ),
            ],
            ..Barriers::default()
        });

        encoder.begin_compute_pass(&crcbl_hal::ComputePassDesc {
            label: Some("skinning probe"),
            timestamp_writes: None,
        });
        encoder.bind_compute_pipeline(self.pipeline);
        encoder.bind_group(0, self.bind_group, &[], self.pipeline_layout);
        // The caller's own rounding-up, which is what puts a partial workgroup
        // on the end.
        encoder.dispatch(params.vertex_count.div_ceil(WORKGROUP_SIZE), 1, 1);
        encoder.end_compute_pass();

        encoder.pipeline_barrier(&Barriers {
            buffers: &[barrier(
                self.vertices,
                ResourceState::ShaderReadWrite,
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: self.vertices,
            src_offset: 0,
            dst: self.staging,
            dst_offset: 0,
            size: pool_bytes(),
        });
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);

        let mut bytes = poisoned(pool_bytes() as usize);
        headless.readback(self.staging, pool_bytes(), &mut bytes);
        bytes
    }

    fn destroy(self, device: &dyn Device) {
        device.destroy_compute_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group(self.bind_group);
        device.destroy_bind_group_layout(self.bind_group_layout);
        device.destroy_buffer(self.staging);
        device.destroy_buffer(self.upload);
        device.destroy_buffer(self.vertices);
        device.destroy_buffer(self.bindings);
        device.destroy_buffer(self.joints);
        device.destroy_buffer(self.params);
    }
}

/// Whether `got` is within [`TOLERANCE`] of `want`, relative to the larger of
/// `|want|` and one.
fn close(got: f32, want: f32) -> bool {
    (got - want).abs() <= TOLERANCE * want.abs().max(1.0)
}

/// The skinned half of a pool readback, decoded.
fn skinned(pool: &[u8], count: usize) -> Vec<MeshVertex> {
    (0..count)
        .map(|slot| {
            let at = (OUTPUT_BASE as usize + slot) * VERTEX_STRIDE;
            vertex_from_bytes(&pool[at..at + VERTEX_STRIDE])
        })
        .collect()
}

/// Compares one skinned vertex against the oracle's.
///
/// `position` and `normal` go through [`close`]; `color` and `uv` are compared
/// **exactly**, because the kernel copies them through and a copy that rounds is
/// not a copy. So are the two unused lanes, which the kernel writes as `1.0` and
/// `0.0` rather than leaving alone.
fn assert_vertex(got: &MeshVertex, want: &MeshVertex, slot: usize, what: &str) {
    for (lane, (lane_got, lane_want)) in got.position.iter().zip(want.position).enumerate() {
        assert!(
            close(*lane_got, lane_want),
            "{what}: vertex {slot} position lane {lane} is {lane_got}, expected \
             {lane_want} (tolerance {TOLERANCE}); the whole position is {:?} against {:?}",
            got.position,
            want.position,
        );
    }
    for (lane, (lane_got, lane_want)) in got.normal.iter().zip(want.normal).enumerate() {
        assert!(
            close(*lane_got, lane_want),
            "{what}: vertex {slot} normal lane {lane} is {lane_got}, expected \
             {lane_want} (tolerance {TOLERANCE}); the whole normal is {:?} against {:?}",
            got.normal,
            want.normal,
        );
    }
    assert_eq!(
        got.position[3], 1.0,
        "{what}: vertex {slot}'s position `w` is written as 1.0"
    );
    assert_eq!(
        got.normal[3], 0.0,
        "{what}: vertex {slot}'s normal `w` is written as 0.0"
    );
    assert_eq!(
        got.color, want.color,
        "{what}: vertex {slot}'s colour is copied through unchanged"
    );
    assert_eq!(
        got.uv, want.uv,
        "{what}: vertex {slot}'s uv is copied through unchanged"
    );
}

/// Every slot the dispatch did not own must still hold the bytes it was
/// uploaded with.
///
/// **Exact**, not toleranced: the claim is that nothing was written, and that is
/// a claim about bytes. Both ends are checked — the decoy vertices below
/// [`INPUT_BASE`] as well as the tail past the skinned run — because a kernel
/// that mishandled `output_base` could land on either.
fn assert_untouched(pool: &[u8], case: &Case, what: &str) {
    let uploaded = uploaded_pool(case);
    let skinned = OUTPUT_BASE as usize..OUTPUT_BASE as usize + case.input.len();
    for slot in 0..POOL_VERTICES as usize {
        if skinned.contains(&slot) {
            continue;
        }
        let at = slot * VERTEX_STRIDE;
        let got = &pool[at..at + VERTEX_STRIDE];
        let want = &uploaded[at..at + VERTEX_STRIDE];
        assert!(
            got == want,
            "{what}: pool vertex {slot} was written, and the dispatch owned only \
             {OUTPUT_BASE}..{}. It holds {got:02x?} where the upload put {want:02x?}. \
             That is the tail of a partial last workgroup reaching past its range — \
             what `if (index >= skin.vertex_count) return;` in \
             `crates/crcbl-shaders/shaders/skinning.slang` exists to stop.",
            skinned.end,
        );
    }
}

/// A vertex distinct in every field, so a copy landing in the wrong lane could
/// not compare equal by accident.
fn vertex(position: Vec3, normal: Vec3) -> MeshVertex {
    MeshVertex {
        position: [position.x, position.y, position.z, 1.0],
        normal: [normal.x, normal.y, normal.z, 0.0],
        color: [0.25, 0.5, 0.75, 1.0],
        uv: [0.125, 0.375, 0.0, 0.0],
    }
}

/// **A vertex whose weight is entirely on one joint is transformed by that joint
/// and nothing else** — the kernel's own claim, and the one case where linear
/// blend skinning owes an exact answer rather than an approximation.
///
/// Three vertices rather than one, each bound wholly to a *different* joint of
/// the same palette: a kernel that read one binding for the whole range, or that
/// ignored `SkinBinding::joints` past the first lane, produces the same answer
/// for all three and this does not.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_vertex_bound_wholly_to_one_joint_is_transformed_by_that_joint_alone() {
    let headless = Headless::open();
    let probe = SkinProbe::new(&headless);

    let case = Case {
        palette: vec![
            Mat4::from_rotation_translation(
                Quat::from_rotation_z(core::f32::consts::FRAC_PI_3),
                Vec3::new(2.0, -3.0, 0.5),
            ),
            Mat4::from_translation(Vec3::new(-1.5, 0.25, 4.0)),
            Mat4::from_rotation_x(core::f32::consts::FRAC_PI_2),
        ],
        bindings: vec![
            SkinBinding {
                joints: [0, 2, 1, 1],
                weights: [1.0, 0.0, 0.0, 0.0],
            },
            SkinBinding {
                joints: [2, 1, 0, 0],
                weights: [0.0, 1.0, 0.0, 0.0],
            },
            SkinBinding {
                joints: [1, 0, 2, 0],
                weights: [0.0, 0.0, 1.0, 0.0],
            },
        ],
        input: vec![
            vertex(Vec3::new(1.0, 2.0, 3.0), Vec3::Y),
            vertex(Vec3::new(-2.0, 0.5, 1.0), Vec3::X),
            vertex(Vec3::new(0.25, -1.0, 2.0), Vec3::Z),
        ],
    };

    let pool = probe.run(&headless, &case);
    let got = skinned(&pool, case.input.len());
    for (slot, (got, want)) in got.iter().zip(case.oracle()).enumerate() {
        assert_vertex(got, &want, slot, "rigidly bound");
    }
    // The three answers must differ, or a kernel blending the wrong joint could
    // satisfy every assertion above.
    assert!(
        got[0].position != got[1].position && got[1].position != got[2].position,
        "the three joints must move their vertices to three different places, or \
         reading the wrong one would go unnoticed: {:?}",
        got.iter().map(|v| v.position).collect::<Vec<_>>()
    );

    probe.destroy(headless.device.as_ref());
    headless.finish();
}

/// **A vertex blended across two joints is the weighted sum of both.**
///
/// Two pure translations and weights of a quarter and three quarters, which is
/// `crcbl_render::skinning`'s own hand-computed case: both weights are exact in
/// binary and both linear parts are the identity, so the blended matrix is the
/// identity with `0.25 * t0 + 0.75 * t1` in its translation and the CPU side has
/// no rounding in it at all.
///
/// The second vertex is the same blend with the weights swapped, so the two
/// land in different places — a kernel that used one weight for all four lanes,
/// or that renormalised, would put them together.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_vertex_blended_across_two_joints_is_the_weighted_sum_of_both() {
    let headless = Headless::open();
    let probe = SkinProbe::new(&headless);

    let case = Case {
        palette: vec![
            Mat4::from_translation(Vec3::new(4.0, 0.0, 0.0)),
            Mat4::from_translation(Vec3::new(0.0, 8.0, 0.0)),
        ],
        bindings: vec![
            SkinBinding {
                joints: [0, 1, 0, 0],
                weights: [0.25, 0.75, 0.0, 0.0],
            },
            SkinBinding {
                joints: [0, 1, 0, 0],
                weights: [0.75, 0.25, 0.0, 0.0],
            },
        ],
        input: vec![
            vertex(Vec3::new(1.0, 1.0, 1.0), Vec3::Z),
            vertex(Vec3::new(1.0, 1.0, 1.0), Vec3::Z),
        ],
    };

    let pool = probe.run(&headless, &case);
    let got = skinned(&pool, case.input.len());
    for (slot, (got, want)) in got.iter().zip(case.oracle()).enumerate() {
        assert_vertex(got, &want, slot, "blended across two joints");
    }
    // The oracle is the comparison above; this is the arithmetic written out, so
    // an oracle that drifted from the kernel could not take the test with it.
    assert!(
        close(got[0].position[0], 1.0 + 0.25 * 4.0) && close(got[0].position[1], 1.0 + 0.75 * 8.0),
        "a quarter of the first joint's translation and three quarters of the \
         second's: {:?}",
        got[0].position
    );

    probe.destroy(headless.device.as_ref());
    headless.finish();
}

/// **A normal under a joint carrying non-uniform scale goes through the cofactor
/// basis, not the bare 3×3.**
///
/// The case the cofactor exists for. A normal is perpendicular to a surface and
/// only an angle-preserving transform carries a perpendicular the way it carries
/// a tangent, so a joint that scales `x` by two and `y` by a half moves the two
/// answers apart by a quantity of order one rather than by rounding.
///
/// The test says so rather than assuming it: it computes what the bare 3×3 would
/// have produced and asserts the read-back normal is further from *that* than
/// from the oracle by a wide margin. Without that line the case would be
/// indistinguishable from any other blend, and a kernel using the wrong matrix
/// could still be inside [`TOLERANCE`] for a normal that happened to sit on an
/// eigenvector.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_normal_under_a_non_uniform_scale_goes_through_the_cofactor_basis() {
    let headless = Headless::open();
    let probe = SkinProbe::new(&headless);

    let joint = Mat4::from_scale_rotation_translation(
        Vec3::new(2.0, 0.5, 1.0),
        Quat::from_rotation_z(core::f32::consts::FRAC_PI_4),
        Vec3::new(1.0, -2.0, 0.5),
    );
    let bind_pose_normal = Vec3::new(1.0, 1.0, 0.0).normalize();
    let case = Case {
        palette: vec![Mat4::IDENTITY, joint],
        bindings: vec![SkinBinding {
            joints: [1, 0, 0, 0],
            weights: [1.0, 0.0, 0.0, 0.0],
        }],
        input: vec![vertex(Vec3::new(0.5, 1.5, -1.0), bind_pose_normal)],
    };

    let pool = probe.run(&headless, &case);
    let got = skinned(&pool, case.input.len());
    let want = case.oracle();
    assert_vertex(&got[0], &want[0], 0, "non-uniform scale");

    // What the bare 3×3 would have written, which is the mistake this case is
    // shaped to catch.
    let bare = (Mat3::from_mat4(joint) * bind_pose_normal).normalize();
    let cofactor = Vec3::new(want[0].normal[0], want[0].normal[1], want[0].normal[2]);
    let separation = (bare - cofactor).length();
    assert!(
        separation > 1e-2,
        "this case is only worth running while the cofactor and the bare 3x3 \
         disagree; they are {separation} apart, which is inside the noise. The \
         joint's scale is what separates them, so a case that lost it would pass \
         with either matrix."
    );
    let read = Vec3::new(got[0].normal[0], got[0].normal[1], got[0].normal[2]);
    assert!(
        (read - cofactor).length() < (read - bare).length(),
        "the read-back normal {read:?} is nearer the bare 3x3's {bare:?} than the \
         cofactor's {cofactor:?}, so the kernel is carrying normals through the \
         wrong matrix"
    );

    probe.destroy(headless.device.as_ref());
    headless.finish();
}

/// **The tail of a partial last workgroup writes nothing.**
///
/// The case `if (index >= skin.vertex_count) return;` exists for, and the one
/// `docs/backlog.md` recorded as passing every test in the workspace with the
/// line deleted. A caller dispatches `vertex_count.div_ceil(WORKGROUP_SIZE)`
/// groups, so a range of [`TAIL_VERTICES`] vertices launches two full workgroups
/// and the last of them carries invocations for vertices the range has not got.
///
/// The pool immediately after the skinned run holds [`guard_pool`]'s fill, and
/// the assertion is that those bytes are **unchanged** — exact, because "nothing
/// was written" is a claim about bytes and not about arithmetic. A test that
/// only checked the skinned vertices cannot see the tail trampling its
/// neighbour: every one of the range's own vertices is still correct in that
/// state, which is exactly why nothing caught it.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn the_tail_of_a_partial_last_workgroup_leaves_the_next_vertices_alone() {
    /// Vertices the range holds: two workgroups' worth of invocations less a
    /// tail the range has not got.
    ///
    /// Deliberately more than one workgroup: with a single group the tail and
    /// the range are the same launch, and a kernel that clamped rather than
    /// returned would look the same.
    const TAIL_VERTICES: u32 = 70;

    assert!(
        !TAIL_VERTICES.is_multiple_of(WORKGROUP_SIZE),
        "the case is a range that does not fill its last workgroup"
    );
    assert!(
        TAIL_VERTICES.div_ceil(WORKGROUP_SIZE) > 1,
        "and one that dispatches more than one"
    );

    let headless = Headless::open();
    let probe = SkinProbe::new(&headless);

    // Every vertex a different place and a different blend, so a write landing
    // in the wrong slot is visible as a wrong value and not only as a wrong
    // count.
    let palette = vec![
        Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        Mat4::from_rotation_y(core::f32::consts::FRAC_PI_6),
        Mat4::from_scale(Vec3::new(1.5, 1.0, 0.75)),
        Mat4::from_translation(Vec3::new(0.0, -2.0, 1.0)),
    ];
    let case = Case {
        bindings: (0..TAIL_VERTICES)
            .map(|index| {
                let first = index % 4;
                let second = (index + 1) % 4;
                let weight = 0.25 + 0.5 * (index % 3) as f32 / 3.0;
                SkinBinding {
                    joints: [first, second, 0, 0],
                    weights: [weight, 1.0 - weight, 0.0, 0.0],
                }
            })
            .collect(),
        input: (0..TAIL_VERTICES)
            .map(|index| {
                let step = index as f32 * 0.03125;
                vertex(
                    Vec3::new(step, 1.0 - step, step * 0.5),
                    Vec3::new(step + 1.0, 1.0, -step).normalize(),
                )
            })
            .collect(),
        palette,
    };

    let pool = probe.run(&headless, &case);
    let got = skinned(&pool, case.input.len());
    for (slot, (got, want)) in got.iter().zip(case.oracle()).enumerate() {
        assert_vertex(got, &want, slot, "partial last workgroup");
    }
    assert_untouched(&pool, &case, "partial last workgroup");

    probe.destroy(headless.device.as_ref());
    headless.finish();
}
