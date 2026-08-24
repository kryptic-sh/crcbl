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
use crcbl_render::skinning::{SkinRange, SkinnedRegion, Skinning, SkinningDesc};
use crcbl_render::{MeshPool, MeshPoolDesc, RenderGraph, TransientPool};
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

/// The `count` vertices a pool readback holds starting at `base`, decoded.
fn half(pool: &[u8], base: u32, count: usize) -> Vec<MeshVertex> {
    (0..count)
        .map(|slot| {
            let at = (base as usize + slot) * VERTEX_STRIDE;
            vertex_from_bytes(&pool[at..at + VERTEX_STRIDE])
        })
        .collect()
}

/// The skinned half of a [`SkinProbe`] readback, decoded.
fn skinned(pool: &[u8], count: usize) -> Vec<MeshVertex> {
    half(pool, OUTPUT_BASE, count)
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

// --- the pass, over more than one frame -------------------------------------
//
// Everything above dispatches the kernel through raw `crcbl_hal` calls, which
// is the right shape for asking what the *arithmetic* does and the wrong one
// for asking what a *frame* does: it builds one bind group by hand, uploads one
// uniform block by hand and submits once. The host side that a browser frame
// actually runs — `crcbl_render::skinning::Skinning::begin_frame` moving the
// ping-pong and uploading the palette, `add_pass` recording the dispatch into
// the render graph, `CompiledGraph::execute` emitting the barriers — is not
// exercised by any of it, and neither is the second frame.
//
// This case is that gap. It runs three frames against one `Skinning`, one
// `RenderGraph` per frame and one `TransientPool` across all of them, with a
// materially different palette every frame, and reads the whole pool back after
// each one.

/// Vertices the cross-frame case skins, in each half of its region.
///
/// Fewer than [`WORKGROUP_SIZE`], so every frame is one group: the tail of a
/// partial last workgroup is
/// [`the_tail_of_a_partial_last_workgroup_leaves_the_next_vertices_alone`]'s
/// question and not this one's.
const FRAME_VERTICES: u32 = 5;

/// Frames in flight the cross-frame case's [`Skinning`] is built with, which is
/// how long its palette, binding and uniform rings are.
///
/// Two rather than one, because a ring of one would make "the second frame
/// wrote its own palette" and "the second frame wrote *a* palette" the same
/// question — the slot could not be stale, having no other slot to be stale
/// against.
const FRAME_SLOTS: usize = 2;

/// The steps the cross-frame case's palettes are built from, one per frame.
///
/// Its length is how many frames it runs. Three rather than two: with two, the
/// frame slot and the parity both alternate together and a defect that only
/// shows when a slot is *revisited* has nowhere to appear. The third frame
/// returns to slot 0 and to parity 1, so it overwrites the half frame 1 wrote.
///
/// The three differ in sign and in magnitude rather than by a tweak, and
/// `the_three_frames_ask_for_three_different_poses` is the assertion that keeps
/// them that way.
const FRAME_STEPS: [f32; 3] = [3.0, -11.0, 23.0];

/// A run reserved out of the mesh pool and never released, so that no base the
/// cross-frame case uses is zero.
///
/// [`INPUT_BASE`]'s argument applied to a layout the pool's free list chooses
/// rather than this file: a kernel ignoring `input_base` or `output_base` reads
/// and writes at zero, and a case whose first allocation sits there would call
/// that correct.
const FRAME_PAD: u32 = 4;

/// The `index`th frame's joint palette: two joints, both moved by that frame's
/// own step.
fn frame_palette(index: usize) -> Vec<Mat4> {
    let step = FRAME_STEPS[index];
    vec![
        Mat4::from_translation(Vec3::new(step, 0.0, 0.0)),
        Mat4::from_rotation_translation(
            Quat::from_rotation_z(step * 0.125),
            Vec3::new(0.0, step, -step),
        ),
    ]
}

/// One binding per vertex of the cross-frame case's region, each blended across
/// **both** joints and no two alike.
///
/// Every weight is exact in binary and every pair sums to one, so the oracle
/// and the kernel are comparing the same arithmetic rather than two roundings
/// of it. Blended rather than rigid because a rigid binding reads one matrix,
/// and a palette upload that landed half-written would still look right on
/// whichever half it kept.
fn frame_bindings() -> Vec<SkinBinding> {
    (0..FRAME_VERTICES)
        .map(|index| {
            let first = 0.125 + 0.1875 * index as f32;
            SkinBinding {
                joints: [0, 1, 0, 0],
                weights: [first, 1.0 - first, 0.0, 0.0],
            }
        })
        .collect()
}

/// The cross-frame case's bind pose: every vertex somewhere else, with a normal
/// of its own.
fn frame_bind_pose() -> Vec<MeshVertex> {
    (0..FRAME_VERTICES)
        .map(|index| {
            let step = index as f32 * 0.25;
            vertex(
                Vec3::new(1.0 + step, 2.0 - step, step),
                Vec3::new(step + 1.0, 1.0, -step).normalize(),
            )
        })
        .collect()
}

/// What the `index`th frame's dispatch owes, one vertex at a time.
fn frame_oracle(index: usize) -> Vec<MeshVertex> {
    let palette = frame_palette(index);
    frame_bind_pose()
        .iter()
        .zip(frame_bindings())
        .map(|(vertex, binding)| crcbl_render::skinning::skin_vertex(&palette, &binding, vertex))
        .collect()
}

/// **The three frames ask for three different poses**, so a dispatch that
/// reused an earlier frame's palette could not satisfy the case that follows.
///
/// No device: this is a property of [`FRAME_STEPS`] alone, and it is the reason
/// the numbers in it are what they are. Without it the whole cross-frame case
/// would be a check that cannot fail — three palettes close enough together
/// would agree inside [`TOLERANCE`] and every assertion would pass whichever
/// one the kernel read.
#[test]
fn the_three_frames_ask_for_three_different_poses() {
    let poses: Vec<Vec<MeshVertex>> = (0..FRAME_STEPS.len()).map(frame_oracle).collect();
    for (first, one_pose) in poses.iter().enumerate() {
        for (second, other_pose) in poses.iter().enumerate().skip(first + 1) {
            for (slot, (one, other)) in one_pose.iter().zip(other_pose).enumerate() {
                let one = Vec3::from_slice(&one.position[..3]);
                let other = Vec3::from_slice(&other.position[..3]);
                let apart = (one - other).length();
                assert!(
                    apart > 1.0,
                    "frames {first} and {second} put vertex {slot} {apart} apart, which is \
                     not far enough to tell one palette from the other: {one:?} against \
                     {other:?}"
                );
            }
        }
    }
}

/// **A later frame skins with its own palette, into the half its own parity
/// names** — the pass driven the way a frame drives it, over three frames.
///
/// The suspicion this exists to settle: a skinned mesh in the browser shows one
/// fixed pose while the palette handed to the seam changes every frame, and
/// nothing in the workspace covered a *second* frame with a *different*
/// palette. Every test above submits once, and
/// `crcbl_render::skinning`'s own tests run against the null backend, which
/// records a dispatch and never runs one.
///
/// So this drives the real host side — [`Skinning::begin_frame`],
/// [`Skinning::add_pass`], `RenderGraph::compile` and `CompiledGraph::execute`
/// — three times against one `Skinning` and one `TransientPool`, and reads the
/// whole pool back after each frame. It asserts **both** halves every time: the
/// one this frame's parity names holds this frame's palette, and the other one
/// still holds exactly what the frame before it left there.
///
/// The halves are [`SkinnedRegion::base`] of the parity
/// [`Skinning::parity`] reports rather than a literal, because which half is
/// which is the pass's business; what this case is about is that the second
/// frame wrote at all, and wrote the second palette.
///
/// # What it deliberately does not use
///
/// [`MeshPool::vertex_buffer`]. The pool's own vertex buffer is created
/// `STORAGE | TRANSFER_DST` — see `MeshPool::new` in
/// `crates/crcbl-render/src/mesh_pool.rs` — so nothing may copy out of it, and
/// a readback is what this case rests on. The buffer below carries
/// `TRANSFER_SRC` as well and is otherwise the same thing, and every base in it
/// is still one the pool's free list handed out, so the layout under test is
/// the one a frame really gets. What is therefore **not** covered here is
/// `MeshPool::upload`'s staging copy; the draw side of skinning is
/// `crcbl_render::forward`'s, and
/// [`a_skinned_cube_draws_the_pose_its_palette_asks_for`] is where it is a
/// picture.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_later_frame_skins_with_its_own_palette_into_the_half_its_parity_names() {
    let headless = Headless::open();
    let device = headless.device.as_ref();

    // The real allocator, so the bases are the ones a frame is handed. The
    // pool's own buffers go unused; see this test's docs for why.
    let mut pool = MeshPool::new(
        device,
        &MeshPoolDesc {
            label: Some("skinning frames"),
            vertex_capacity: POOL_VERTICES,
            index_capacity: 64,
            mesh_capacity: 4,
        },
    )
    .expect("a mesh pool");
    let pad = pool.reserve_vertices(FRAME_PAD).expect("an empty pool");
    let input_base = pool
        .reserve_vertices(FRAME_VERTICES)
        .expect("room for the bind pose");
    let region = SkinnedRegion::reserve(&mut pool, FRAME_VERTICES).expect("room for both halves");
    assert_ne!(
        region.base(0),
        region.base(1),
        "the two halves are separate reservations, or a frame would overwrite the one \
         before it by construction"
    );

    let vertices = device
        .create_buffer(&BufferDesc {
            label: Some("skinning frames pool"),
            size: pool_bytes(),
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::DeviceLocal,
        })
        .expect("a vertex pool");
    let upload = device
        .create_buffer(&BufferDesc {
            label: Some("skinning frames upload"),
            size: pool_bytes(),
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })
        .expect("a staging buffer");
    let staging = device
        .create_buffer(&BufferDesc {
            label: Some("skinning frames readback"),
            size: pool_bytes(),
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let bind_pose = frame_bind_pose();
    let bindings = frame_bindings();
    let mut uploaded = guard_pool();
    for (slot, vertex) in bind_pose.iter().enumerate() {
        let at = (input_base as usize + slot) * VERTEX_STRIDE;
        uploaded[at..at + VERTEX_STRIDE].copy_from_slice(&vertex_bytes(vertex));
    }
    device
        .write_buffer(upload, 0, &uploaded)
        .expect("the upload buffer is host-visible");

    let barrier = |buffer, from, to| BufferBarrier {
        buffer,
        from,
        to,
        queue_transfer: None,
    };
    let submit = |encoder: Box<dyn crcbl_hal::CommandEncoder>| {
        let commands = encoder.finish().expect("recording succeeded");
        device
            .submit(headless.queue, &SubmitInfo::new(&[commands]))
            .expect("submit");
        device.wait_idle().expect("idle");
        device.destroy_command_buffer(commands);
    };

    // The pool is filled once, before any frame: the guard everywhere and the
    // bind pose at `input_base`. Nothing refills it between frames, which is
    // what lets a frame be asked whether it left the *other* half alone.
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("skinning frames fill"),
        queue: headless.queue,
    });
    encoder.pipeline_barrier(&Barriers {
        buffers: &[barrier(
            vertices,
            ResourceState::Undefined,
            ResourceState::TransferDst,
        )],
        ..Barriers::default()
    });
    encoder.copy_buffer_to_buffer(&BufferCopy {
        src: upload,
        src_offset: 0,
        dst: vertices,
        dst_offset: 0,
        size: pool_bytes(),
    });
    // The state `Skinning::add_pass` declares its import in, so the first
    // frame's `ImportedBuffer::initial` is true of the buffer it names.
    encoder.pipeline_barrier(&Barriers {
        buffers: &[barrier(
            vertices,
            ResourceState::TransferDst,
            ResourceState::ShaderRead,
        )],
        ..Barriers::default()
    });
    submit(encoder);

    let mut skinning = Skinning::new(
        device,
        &SkinningDesc {
            label: Some("skinning frames"),
            frames: FRAME_SLOTS,
            ranges: 1,
            joints: 2,
            bindings: FRAME_VERTICES,
            vertices,
        },
    )
    .expect("a skinning pass");

    // One pool across every frame, which is what makes the graph's own
    // cross-frame audit real: it is the ledger `RenderGraph::compile` checks
    // each frame's `ImportedBuffer::initial` against.
    let mut transients = TransientPool::new();
    let mut readbacks = Vec::with_capacity(FRAME_STEPS.len());
    let mut parities = Vec::with_capacity(FRAME_STEPS.len());

    for index in 0..FRAME_STEPS.len() {
        let slot = index % FRAME_SLOTS;
        let palette = frame_palette(index);
        let range = SkinRange {
            input_base,
            region: &region,
            palette: &palette,
            bindings: &bindings,
        };
        skinning
            .begin_frame(device, slot, core::slice::from_ref(&range))
            .expect("a legal skinning plan");
        parities.push(skinning.parity());

        let mut graph = RenderGraph::new(headless.queue);
        assert!(
            skinning.add_pass(&mut graph, slot).is_some(),
            "frame {index} was handed a range, so it must add the pass"
        );
        let compiled = graph.compile(&transients).expect("a legal frame");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("skinning frame"),
            queue: headless.queue,
        });
        compiled
            .execute(device, &mut transients, encoder.as_mut(), None)
            .expect("the graph executed");
        // Out of the state the graph returned the import to, through the copy,
        // and back into it — so the next frame's declaration is still true.
        encoder.pipeline_barrier(&Barriers {
            buffers: &[barrier(
                vertices,
                ResourceState::ShaderRead,
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: vertices,
            src_offset: 0,
            dst: staging,
            dst_offset: 0,
            size: pool_bytes(),
        });
        encoder.pipeline_barrier(&Barriers {
            buffers: &[barrier(
                vertices,
                ResourceState::TransferSrc,
                ResourceState::ShaderRead,
            )],
            ..Barriers::default()
        });
        submit(encoder);

        let mut bytes = poisoned(pool_bytes() as usize);
        headless.readback(staging, pool_bytes(), &mut bytes);
        readbacks.push(bytes);
    }

    for (index, bytes) in readbacks.iter().enumerate() {
        let parity = parities[index];
        let mine = region.base(parity);
        let theirs = region.base(parity ^ 1);
        let count = bind_pose.len();

        let what = format!("frame {index}, parity {parity}, base {mine}");
        for (slot, (got, want)) in half(bytes, mine, count)
            .iter()
            .zip(frame_oracle(index))
            .enumerate()
        {
            assert_vertex(got, &want, slot, &what);
        }

        // The half this frame did not name. A frame that wrote both, or that
        // wrote the wrong one, fails here rather than in the loop above — where
        // it could still look right.
        match index.checked_sub(1) {
            Some(previous) => {
                let what = format!(
                    "frame {index}'s other half at base {theirs}, which frame {previous} wrote"
                );
                for (slot, (got, want)) in half(bytes, theirs, count)
                    .iter()
                    .zip(frame_oracle(previous))
                    .enumerate()
                {
                    assert_vertex(got, &want, slot, &what);
                }
            }
            None => {
                for slot in 0..count {
                    let at = (theirs as usize + slot) * VERTEX_STRIDE;
                    assert!(
                        bytes[at..at + VERTEX_STRIDE] == uploaded[at..at + VERTEX_STRIDE],
                        "frame 0 wrote pool vertex {} of the half its parity does not \
                         name; it holds {:02x?} where the fill put {:02x?}",
                        theirs as usize + slot,
                        &bytes[at..at + VERTEX_STRIDE],
                        &uploaded[at..at + VERTEX_STRIDE],
                    );
                }
            }
        }

        // Everything outside both halves is still the fill and the bind pose —
        // exact, because "nothing was written" is a claim about bytes.
        let halves = [region.base(0) as usize, region.base(1) as usize];
        for slot in 0..POOL_VERTICES as usize {
            if halves
                .iter()
                .any(|base| (*base..*base + count).contains(&slot))
            {
                continue;
            }
            let at = slot * VERTEX_STRIDE;
            assert!(
                bytes[at..at + VERTEX_STRIDE] == uploaded[at..at + VERTEX_STRIDE],
                "frame {index} wrote pool vertex {slot}, which belongs to neither half of \
                 the region ({halves:?}, {count} vertices each). It holds {:02x?} where the \
                 fill put {:02x?}",
                &bytes[at..at + VERTEX_STRIDE],
                &uploaded[at..at + VERTEX_STRIDE],
            );
        }
    }

    skinning.destroy(device);
    transients.destroy(device);
    device.destroy_buffer(staging);
    device.destroy_buffer(upload);
    device.destroy_buffer(vertices);
    region.release(&mut pool);
    pool.release_vertices(input_base, FRAME_VERTICES);
    pool.release_vertices(pad, FRAME_PAD);
    pool.destroy(device);
    headless.finish();
}

// --- the pass, drawn ---------------------------------------------------------
//
// Everything above reads the vertex pool back. That settles what the dispatch
// writes and nothing about what is drawn out of it: the mesh a skinned instance
// names, the base vertex it carries beside it and the parity that base is
// re-pointed at every frame are all bookkeeping this file never touches, and
// `crcbl_render::forward`'s own tests check each of them as a recorded byte
// against the null recorder rather than as a pixel.
//
// So this case renders. One renderer draws the demo cube in its bind pose
// through `add_instance`; another draws the *same* cube out of a skinned region
// through `add_skinned_instance`, over four frames, and the colour target comes
// back each time. What it can then ask is the browser's question in the
// browser's terms: does the picture change when the palette does.

/// The joint palette of a frame that asks for no deformation at all.
///
/// Both joints identity, so [`skin_vertex`](crcbl_render::skinning::skin_vertex)
/// is the bind-pose vertex and the skinned draw owes the same picture the
/// bind-pose draw does. That equality is
/// [`a_skinned_cube_draws_the_pose_its_palette_asks_for`]'s anti-vacuity
/// control.
fn rest_palette() -> Vec<Mat4> {
    vec![Mat4::IDENTITY; 2]
}

/// A palette that slides the cube's `+X` half a cube and a half along `X`.
fn parted_palette() -> Vec<Mat4> {
    vec![
        Mat4::IDENTITY,
        Mat4::from_translation(Vec3::new(0.75, 0.0, 0.0)),
    ]
}

/// A palette that drops the cube's `-X` half instead, and turns it.
///
/// The other joint from [`parted_palette`] and a different axis, so a frame
/// drawn with one of them cannot be mistaken for a frame drawn with the other
/// however the two halves happen to overlap on screen.
fn dropped_palette() -> Vec<Mat4> {
    vec![
        Mat4::from_rotation_translation(
            Quat::from_rotation_z(core::f32::consts::FRAC_PI_4),
            Vec3::new(0.0, -0.75, 0.0),
        ),
        Mat4::IDENTITY,
    ]
}

/// One binding per cube vertex, splitting the mesh at the `YZ` plane: the `+X`
/// corners to joint 1, the rest to joint 0.
///
/// Read out of [`crcbl_shaders::mesh::cube_vertices`] rather than assumed from
/// the face order, so the split is a property of where the corners are and
/// survives a rewrite of `FACES`. Splitting by position is what makes the
/// deformed frames unreachable by any single model transform — half the cube
/// moves and half does not, which no instance matrix can express.
fn cube_bindings() -> Vec<SkinBinding> {
    crcbl_shaders::mesh::cube_vertices()
        .iter()
        .map(|vertex| SkinBinding {
            joints: [u32::from(vertex.position[0] > 0.0), 0, 0, 0],
            weights: [1.0, 0.0, 0.0, 0.0],
        })
        .collect()
}

/// Renders one frame through the real renderer and reads the colour target
/// back, with or without a skinning plan.
///
/// **One function for both**, and that is the whole point of it: the bind-pose
/// frame and the identity-palette frame below differ in the single call this
/// branches on and in nothing else, so a disagreement between them is the
/// skinning path rather than two harnesses that drifted apart.
///
/// It is `crate::mesh::render_mesh`'s shape with that branch added. Not shared
/// with it because sharing would mean editing `mesh.rs`, and the frames this
/// compares have to come from one body whatever module it sits in.
fn render_frame(
    headless: &Headless,
    what: &str,
    renderer: &mut crcbl_render::ForwardRenderer,
    transients: &mut TransientPool,
    camera: &crcbl_render::Camera,
    plan: Option<(&mut Skinning, &[SkinRange<'_>])>,
) -> crcbl_golden::Image {
    // Named on the way in, because a frame that takes the device with it never
    // reaches an assertion and the log is then the only thing that says which
    // of the five was recording when it went.
    eprintln!("vk e2e: skinned suite recording frame \"{what}\"");
    let device = headless.device.as_ref();
    let (width, height) = crate::mesh::MESH_EXTENT;
    let acquired = device
        .acquire_next_frame(headless.swapchain)
        .expect("the ring always has an image");
    assert_eq!(acquired.extent, crate::mesh::MESH_EXTENT);

    let color_bytes = u64::from(width) * u64::from(height) * 4;
    let color_staging = device
        .create_buffer(&crcbl_hal::BufferDesc {
            label: Some("skinned readback"),
            size: color_bytes,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })
        .expect("a readback buffer");

    let light = crcbl_render::DirectionalLight::default();
    let mut plan = plan;
    match plan.as_mut() {
        // **The skinned entry point, not `begin_frame` with a flag.** It moves
        // the ping-pong and re-points every skinned object at the half this
        // frame's dispatch fills, in that order — which is the ordering the
        // browser symptom would be explained by getting wrong.
        Some((skinning, ranges)) => renderer
            .begin_skinned_frame(
                device,
                skinning,
                ranges,
                camera,
                &light,
                crate::mesh::MESH_EXTENT,
            )
            .expect("a legal skinning plan"),
        None => renderer
            .begin_frame(device, camera, &light, crate::mesh::MESH_EXTENT)
            .expect("the uniform buffer is writable"),
    }

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("skinned frame"),
        queue: headless.queue,
    });
    let compiled = {
        let mut graph = RenderGraph::new(headless.queue);
        let target = graph.import_image(
            "swapchain",
            crcbl_render::ImportedImage {
                image: acquired.image,
                view: acquired.view,
                format: headless.format,
                extent: crate::mesh::MESH_EXTENT,
                initial: ResourceState::Undefined,
                claim: crcbl_render::InitialClaim::Acquired,
                // Read back rather than shown, so the graph is asked to leave it
                // as a copy source and the copy below needs no barrier of its
                // own — `crate::mesh::render_mesh`'s argument.
                final_state: ResourceState::TransferSrc,
            },
        );
        let _hdr = match plan.as_ref() {
            Some((skinning, _)) => renderer.add_skinned_passes(
                &mut graph,
                &*transients,
                target,
                crate::mesh::MESH_EXTENT,
                skinning,
            ),
            None => renderer.add_passes(&mut graph, &*transients, target, crate::mesh::MESH_EXTENT),
        };
        graph.compile(&*transients).expect("a legal frame")
    };
    compiled
        .execute(device, transients, encoder.as_mut(), None)
        .expect("the graph executed");

    encoder.copy_image_to_buffer(&crcbl_hal::BufferImageCopy {
        buffer: color_staging,
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image: acquired.image,
        image_subresource: crcbl_hal::ImageSubresourceLayers {
            aspect: crcbl_hal::ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: crcbl_hal::Offset3d::default(),
        image_extent: crcbl_hal::Extent3d::d2(width, height),
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");
    device
        .present(
            headless.queue,
            &crcbl_hal::PresentInfo {
                swapchain: headless.swapchain,
                waits: acquired.present_semaphore.as_slice(),
                present_id: None,
            },
        )
        .expect("present");

    let mut color = poisoned(color_bytes as usize);
    headless.readback(color_staging, color_bytes, &mut color);
    device.destroy_command_buffer(commands);
    device.destroy_buffer(color_staging);

    let order = match headless.format {
        crcbl_hal::Format::Bgra8Unorm | crcbl_hal::Format::Bgra8UnormSrgb => {
            crcbl_golden::ChannelOrder::Bgra
        }
        _ => crcbl_golden::ChannelOrder::Rgba,
    };
    crcbl_golden::Image::from_readback(width, height, &color, order)
        .expect("the readback is exactly one image")
}

/// Asserts two frames are the same picture, at the bound a golden is held to.
///
/// Not [`crcbl_golden::Tolerance::EXACT`]: the skinned copy of a vertex goes
/// through the kernel's blend, its cofactor basis and a normalise even when the
/// palette is the identity, so a lit pixel may land one level away from the
/// bind-pose draw's. [`crcbl_golden::Tolerance::RASTERISER`] is what the rest
/// of this suite compares frames with, and its numbers are measured rather than
/// guessed.
fn assert_same_picture(reference: &crcbl_golden::Image, actual: &crcbl_golden::Image, what: &str) {
    let comparison = crcbl_golden::compare(reference, actual, &crcbl_golden::Tolerance::RASTERISER);
    eprintln!("vk e2e: skinned {what} — {}", comparison.summary());
    assert!(
        comparison.is_match(),
        "{what}: the two frames are not the same picture — {}",
        comparison.summary()
    );
}

/// Asserts two frames are visibly different pictures, and says by how much.
///
/// **Grossly** different, on a share of the frame: "not equal" is satisfied by
/// one drifting pixel, which is what a renderer that redrew the same pose with
/// a different rounding would produce. `gross_ratio` counts pixels past
/// [`crcbl_golden::Tolerance::RASTERISER`]'s `gross_channel_delta`, which is
/// the threshold that crate sized to separate a real recolour from every
/// driver disagreement it has measured.
fn assert_different_picture(
    reference: &crcbl_golden::Image,
    actual: &crcbl_golden::Image,
    least_gross_ratio: f64,
    what: &str,
) {
    let comparison = crcbl_golden::compare(reference, actual, &crcbl_golden::Tolerance::RASTERISER);
    eprintln!("vk e2e: skinned {what} — {}", comparison.summary());
    assert!(
        comparison.gross_ratio >= least_gross_ratio,
        "{what}: the two frames are the same picture to within {:.4}% of pixels grossly \
         wrong, and this case needs at least {:.4}% — {}",
        comparison.gross_ratio * 100.0,
        least_gross_ratio * 100.0,
        comparison.summary()
    );
}

/// The share of the frame two pictures this case calls different must disagree
/// on, grossly.
///
/// **Swept rather than guessed.** The cube covers 20.43% of this frame at this
/// camera, and moving the whole of it through the same instance transform —
/// which a palette cannot exceed, since a palette here moves half of it —
/// costs, on radv at 256×192:
///
/// | the whole cube moved by | pixels grossly wrong |
/// |---|---|
/// | [`parted_palette`]'s joint, `+0.75` on `X` | 18.97% |
/// | [`dropped_palette`]'s joint | 7.47% |
/// | half a cube sideways | 8.83% |
/// | a tenth of a cube sideways | 1.85% |
///
/// So a tenth of a cube is already visible at 1.85%, and the smallest whole-cube
/// move either palette asks for is 7.47%. This sits just above the first and
/// several times below the second, and the defect it has to refuse answers
/// **zero** — the frames are bit-identical. Every comparison prints its own
/// number, so the margin is readable on any run rather than taken from here.
const LEAST_GROSS_RATIO: f64 = 0.02;

/// The demo description cut down to its cube, which is the scene this case
/// draws a skinned mesh out of.
///
/// **Not [`scene::demo`](crcbl_render::scene::demo)**, and the history is worth
/// keeping: while a reservation took mesh-table entries of its own, a skinned
/// instance named an id past every bucket and past the level tables
/// `draw_gen.slang` indexes with `GpuInstance::mesh`. On the full description —
/// whose dunes patch is a cluster DAG, and so takes a table entry per level —
/// the first skinned frame did not draw a wrong picture, it took the device with
/// it: radv answered `VK_ERROR_DEVICE_LOST` after a hard GPU recovery and
/// lavapipe ran the frame for about a minute. Both were observed on 2026-08-25.
///
/// A skinned instance now names its **source** mesh, which is an id every one of
/// those tables already has an entry for, so that cliff has no edge left to fall
/// off. The scene stays cut down anyway: one mesh is the smallest description
/// that draws a skinned cube, the claims below are about the share of the frame
/// that cube covers, and running them against the full demo has not been tried.
fn cube_only_scene() -> crcbl_render::scene::SceneDesc<'static> {
    let mut scene = crcbl_render::scene::demo();
    scene.meshes.truncate(1);
    scene
}

/// **A skinned cube draws the pose its palette asks for, and a new palette
/// draws a new pose.**
///
/// The browser symptom as a pixel claim. A skinned mesh there shows one fixed
/// pose for ever while the palette handed to the seam changes every frame; the
/// pool readback above says the dispatch writes the right vertices into the
/// right half on every frame, so what was left unasked is whether the *draw*
/// follows. Nothing in this workspace rendered a skinned mesh and looked at the
/// result.
///
/// Five frames, two renderers, one device — the second built on
/// [`cube_only_scene`], whose docs say what the full demo description does
/// instead of drawing:
///
/// * an empty scene, so the cube's own share of the frame is measured rather
///   than assumed — without it every claim below could be about a background;
/// * the same cube through [`crcbl_render::ForwardRenderer::add_instance`], which is the bind
///   pose and the reference the skinned frames are judged against;
/// * the cube through [`crcbl_render::ForwardRenderer::add_skinned_instance`] at
///   [`rest_palette`], which owes that same picture;
/// * at [`parted_palette`], which must not;
/// * at [`dropped_palette`], which must differ from *that* — the second frame
///   with a second palette, which is the case the browser is stuck on;
/// * at [`parted_palette`] again, which must come back to the third frame's
///   picture. That is the control that says the differences track the palette
///   rather than the frame index.
///
/// Both renderers draw through the same [`render_frame`], so no claim here
/// rests on two frame drivers agreeing.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_skinned_cube_draws_the_pose_its_palette_asks_for() {
    let headless = crate::harness::Headless::open_for_mesh();
    skinned_palette_case(headless);
}

/// **The same claim on the mesh-shader path**, which is a different vertex
/// stage reading the same instance record.
///
/// [`a_skinned_cube_draws_the_pose_its_palette_asks_for`] runs on a device
/// opened with [`crcbl_hal::Features::GPU_DRIVEN`] alone, which selects
/// [`GeometryPath::IndirectCount`](crcbl_hal::GeometryPath::IndirectCount) and
/// draws through `mesh.slang`'s `vertexMain`. A device that reports
/// `MESH_SHADER` selects [`GeometryPath::MeshShader`](crcbl_hal::GeometryPath::MeshShader)
/// instead and draws **every** mesh — flat ones included — through
/// `mesh_cluster.slang`'s `meshMain`, which resolves its own base vertex out of
/// its own copy of the instance record. Both radv and lavapipe report the
/// feature, so this is the path the hardware in front of this suite actually
/// takes; without this case the base-vertex override would be exercised on the
/// arm the harness asks for and unexercised on the arm a device chooses.
///
/// The assertion that the path really is the mesh one is not decoration: a
/// driver that stopped reporting the feature would run this case as a second,
/// silent copy of the one above and go on passing.
#[test]
#[ignore = "needs a real Vulkan implementation; run tests/run-vk-e2e.sh"]
fn a_skinned_cube_draws_the_pose_its_palette_asks_for_through_the_mesh_stage() {
    let headless = crate::harness::Headless::open_for_mesh_with(
        crcbl_hal::Features::GPU_DRIVEN
            | crcbl_hal::Features::MESH_SHADER
            | crcbl_hal::Features::TASK_SHADER,
    );
    assert_eq!(
        headless.device.caps().geometry_path(),
        crcbl_hal::GeometryPath::MeshShader,
        "this case is about `mesh_cluster.slang`, and a device that selected another path \
         would run it as a duplicate of the raster one"
    );
    skinned_palette_case(headless);
}

/// The five frames both cases above compare, driven against whichever device
/// they opened.
///
/// One body rather than two, because the claim is the same claim: what differs
/// is the geometry path the device selected, and a second copy of these
/// assertions would be a second thing to keep in step with the first.
fn skinned_palette_case(headless: Headless) {
    let device = headless.device.as_ref();
    let camera = crate::mesh::mesh_camera(crcbl_render::Projection::default());
    let transform = crcbl_render::ForwardRenderer::spin(crate::mesh::MESH_SECONDS);

    // --- the bind pose, through an ordinary instance ---
    let mut bind_transients = TransientPool::new();
    let mut bind_renderer =
        crcbl_render::ForwardRenderer::new(device, headless.queue, headless.format)
            .expect("the forward renderer builds");
    let empty = render_frame(
        &headless,
        "empty",
        &mut bind_renderer,
        &mut bind_transients,
        &camera,
        None,
    );
    crate::mesh::place_cube_at(&mut bind_renderer, transform);
    let bind = render_frame(
        &headless,
        "bind pose",
        &mut bind_renderer,
        &mut bind_transients,
        &camera,
        None,
    );

    // --- the same cube, drawn out of a skinned region ---
    let mut skinned_transients = TransientPool::new();
    let mut skinned_renderer = crcbl_render::ForwardRenderer::with_scene(
        device,
        headless.queue,
        headless.format,
        &cube_only_scene(),
    )
    .expect("the forward renderer builds");
    let region = skinned_renderer
        .reserve_skinned(crcbl_render::scene::DEMO_CUBE)
        .expect("the demo pool has room for two halves of a cube");
    let bindings = cube_bindings();
    assert_eq!(
        bindings.len() as u32,
        region.vertex_count(),
        "one binding per bind-pose vertex, or `begin_frame` refuses the range"
    );
    skinned_renderer
        .add_skinned_instance(&crcbl_render::SkinnedInstanceDesc {
            mesh: &region,
            material: crcbl_render::scene::DEMO_UNTINTED,
            transform,
        })
        .expect("an instance pool of thousands has room for one object");
    let mut skinning = Skinning::new(
        device,
        &SkinningDesc {
            label: Some("skinned cube"),
            frames: crcbl_render::forward::FRAMES_IN_FLIGHT,
            ranges: 1,
            joints: 2,
            bindings: region.vertex_count(),
            // The renderer's own pool, which is what makes the dispatch's
            // output reachable by its draws at all.
            vertices: skinned_renderer.vertex_buffer(),
        },
    )
    .expect("a skinning pass");

    let mut draw = |what: &str, palette: &[Mat4], skinning: &mut Skinning| {
        let range = region.skin_range(palette, &bindings);
        render_frame(
            &headless,
            what,
            &mut skinned_renderer,
            &mut skinned_transients,
            &camera,
            Some((skinning, core::slice::from_ref(&range))),
        )
    };
    let rest = draw("rest palette", &rest_palette(), &mut skinning);
    let parted = draw("parted palette", &parted_palette(), &mut skinning);
    let dropped = draw("dropped palette", &dropped_palette(), &mut skinning);
    let parted_again = draw("parted palette again", &parted_palette(), &mut skinning);

    // The cube is on screen and covers a real share of it. Every claim below is
    // about pixels the cube owns, so this is what stops them being claims about
    // a background.
    assert_different_picture(&empty, &bind, LEAST_GROSS_RATIO, "empty against bind pose");
    // The anti-vacuity control: an identity palette owes the bind pose's own
    // picture, so a renderer drawing garbage out of the region fails here even
    // though it would satisfy every "these differ" claim below.
    assert_same_picture(&bind, &rest, "bind pose against an identity palette");
    // (1) a deformed palette draws a different pose.
    assert_different_picture(
        &rest,
        &parted,
        LEAST_GROSS_RATIO,
        "identity palette against a parted one",
    );
    // (2) the browser's question: a second frame with a second palette.
    assert_different_picture(
        &parted,
        &dropped,
        LEAST_GROSS_RATIO,
        "a parted palette against a dropped one",
    );
    assert_different_picture(
        &rest,
        &dropped,
        LEAST_GROSS_RATIO,
        "identity palette against a dropped one",
    );
    // The differences track the palette and not the frame index.
    assert_same_picture(
        &parted,
        &parted_again,
        "a palette repeated two frames later",
    );

    device.wait_idle().expect("idle");
    skinning.destroy(device);
    skinned_renderer.release_skinned(region);
    skinned_renderer.destroy(device);
    skinned_transients.destroy(device);
    bind_renderer.destroy(device);
    bind_transients.destroy(device);
    headless.finish();
}
